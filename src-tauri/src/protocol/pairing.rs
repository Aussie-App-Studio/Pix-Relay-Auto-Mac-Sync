use std::io::{BufReader, BufWriter};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

use uuid::Uuid;

use crate::db::{now_ms, Database};
use crate::error::{AppError, AppResult};
use crate::protocol::crypto;
use crate::protocol::discovery::{mac_model_name, primary_local_ipv4, DEFAULT_TCP_PORT};
use crate::protocol::messages::{
    decode_pairing_code, wire_types, DeviceRole, DiscoveredPeer, PairConfirmRequest,
    PairConfirmResponse, PairInitRequest, PairInitResponse, PairedDevice, PairingSessionState,
};
use crate::protocol::wire::{read_plain_envelope, write_plain_envelope};

pub fn initiate_interactive_pairing(
    db: &Database,
    peer: &DiscoveredPeer,
) -> AppResult<PairingSessionState> {
    let settings = db.get_settings()?;
    let nonce = Uuid::new_v4().to_string().chars().take(12).collect::<String>();
    let local_ip = primary_local_ipv4();
    let (secret_key, pub_key) = crypto::generate_ecdh_keypair();
    let sender_pub_b64 = crypto::encode_public_key_spki_b64(&pub_key)?;

    let req = PairInitRequest {
        sender_device_id: settings.device_id.clone(),
        sender_device_name: settings.device_name,
        sender_model: mac_model_name(),
        sender_role: DeviceRole::Sender,
        sender_ip: local_ip,
        sender_port: DEFAULT_TCP_PORT,
        session_nonce: nonce.clone(),
        sender_ecdh_public_key_b64: sender_pub_b64,
        qr_pin_proof: None,
    };

    let addr: SocketAddr = format!("{}:{}", peer.ip_address, peer.port)
        .parse()
        .map_err(|_| AppError::Message("Invalid peer IP or port".to_string()))?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(6))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);

    write_plain_envelope(
        &mut writer,
        wire_types::PAIR_INIT_REQ,
        &serde_json::to_string(&req)?,
    )?;

    let env = read_plain_envelope(&mut reader)?;
    if env.msg_type != wire_types::PAIR_INIT_RES {
        return Err(AppError::Message("Unexpected pairing response".to_string()));
    }
    let res: PairInitResponse = serde_json::from_str(&env.payload_json)?;
    if !res.accepted {
        let msg = if res.message.trim().is_empty() {
            "Pairing declined".to_string()
        } else {
            res.message
        };
        return Err(AppError::Message(msg));
    }
    if res.receiver_ecdh_public_key_b64.trim().is_empty() {
        return Err(AppError::Message(
            "Peer did not provide an ECDH public key".to_string(),
        ));
    }

    let ecdh_secret =
        crypto::compute_ecdh_shared_secret(&secret_key, &res.receiver_ecdh_public_key_b64)?;
    let local_code6 = crypto::derive_6digit_sas_code(
        &ecdh_secret,
        &settings.device_id,
        &res.receiver_device_id,
        &nonce,
    )?;
    if local_code6 != res.verification_code_6_digit {
        return Err(AppError::Crypto(
            "Security verification code mismatch".to_string(),
        ));
    }

    let secret = crypto::derive_pairing_shared_secret(
        &ecdh_secret,
        &settings.device_id,
        &res.receiver_device_id,
        &nonce,
        None,
    )?;

    Ok(PairingSessionState {
        peer_id: res.receiver_device_id,
        peer_name: res.receiver_device_name,
        peer_model: res.receiver_model,
        peer_ip: peer.ip_address.clone(),
        peer_port: peer.port,
        verification_code_6_digit: local_code6,
        shared_secret_candidate: secret,
        session_nonce: nonce,
    })
}

pub fn confirm_interactive_pairing(
    db: &Database,
    session: &PairingSessionState,
) -> AppResult<PairedDevice> {
    let settings = db.get_settings()?;
    let addr: SocketAddr = format!("{}:{}", session.peer_ip, session.peer_port)
        .parse()
        .map_err(|_| AppError::Message("Invalid peer address".to_string()))?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(6))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);

    let proof = crypto::compute_pair_confirm_proof(
        &session.shared_secret_candidate,
        &settings.device_id,
        &session.session_nonce,
    )?;
    let req = PairConfirmRequest {
        sender_device_id: settings.device_id.clone(),
        session_nonce: session.session_nonce.clone(),
        approved: true,
        confirmation_proof: proof,
    };

    write_plain_envelope(
        &mut writer,
        wire_types::PAIR_CONFIRM_REQ,
        &serde_json::to_string(&req)?,
    )?;

    let env = read_plain_envelope(&mut reader)?;
    let res: PairConfirmResponse = serde_json::from_str(&env.payload_json)?;
    if !(res.paired && res.receiver_approved) {
        let msg = if res.message.trim().is_empty() {
            "The Pixel did not confirm pairing.".to_string()
        } else {
            res.message
        };
        return Err(AppError::Message(msg));
    }

    let now = now_ms();
    let paired = PairedDevice {
        device_id: session.peer_id.clone(),
        device_name: session.peer_name.clone(),
        model_name: session.peer_model.clone(),
        role: DeviceRole::Receiver,
        last_ip_address: session.peer_ip.clone(),
        last_port: session.peer_port,
        shared_secret: session.shared_secret_candidate.clone(),
        paired_at_epoch_ms: now,
        last_seen_epoch_ms: now,
        save_location_override: None,
        total_files_transferred: 0,
        total_bytes_transferred: 0,
    };
    db.upsert_paired_device(&paired)?;
    if settings.selected_receiver_device_id.is_none() {
        db.set_selected_receiver(Some(&paired.device_id))?;
    }
    db.append_activity("PAIR", &format!("Paired with {}", paired.device_name))?;
    Ok(paired)
}

pub fn pair_with_code(db: &Database, raw_code: &str) -> AppResult<PairedDevice> {
    let payload = decode_pairing_code(raw_code)
        .ok_or_else(|| AppError::Message("Invalid Pix Relay pairing code.".to_string()))?;

    let settings = db.get_settings()?;
    let nonce = Uuid::new_v4().to_string().chars().take(12).collect::<String>();
    let local_ip = primary_local_ipv4();
    let (secret_key, pub_key) = crypto::generate_ecdh_keypair();
    let sender_pub_b64 = crypto::encode_public_key_spki_b64(&pub_key)?;
    let qr_pin_proof = crypto::compute_qr_pin_proof(
        &payload.pin_token,
        &settings.device_id,
        &payload.device_id,
        &nonce,
        &sender_pub_b64,
    )?;

    let req = PairInitRequest {
        sender_device_id: settings.device_id.clone(),
        sender_device_name: settings.device_name,
        sender_model: mac_model_name(),
        sender_role: DeviceRole::Sender,
        sender_ip: local_ip,
        sender_port: DEFAULT_TCP_PORT,
        session_nonce: nonce.clone(),
        sender_ecdh_public_key_b64: sender_pub_b64,
        qr_pin_proof: Some(qr_pin_proof),
    };

    let addr: SocketAddr = format!("{}:{}", payload.ip_address, payload.port)
        .parse()
        .map_err(|_| AppError::Message("Pairing code contains invalid IP/port".to_string()))?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(6))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);

    write_plain_envelope(
        &mut writer,
        wire_types::PAIR_INIT_REQ,
        &serde_json::to_string(&req)?,
    )?;
    let env = read_plain_envelope(&mut reader)?;
    let res: PairInitResponse = serde_json::from_str(&env.payload_json)?;
    if !(res.accepted && res.instant_paired_via_qr && !res.receiver_ecdh_public_key_b64.is_empty()) {
        let msg = if res.message.trim().is_empty() {
            "Invalid or expired pairing code".to_string()
        } else {
            res.message
        };
        return Err(AppError::Message(msg));
    }

    let expected_server_proof = crypto::compute_qr_server_proof(
        &payload.pin_token,
        &res.receiver_device_id,
        &settings.device_id,
        &nonce,
        &res.receiver_ecdh_public_key_b64,
    )?;
    let actual_proof = res.qr_server_proof.as_deref().unwrap_or("");
    if !crypto::verify_signature(&expected_server_proof, actual_proof) {
        return Err(AppError::Crypto(
            "Pairing code server proof verification failed".to_string(),
        ));
    }

    let ecdh_secret =
        crypto::compute_ecdh_shared_secret(&secret_key, &res.receiver_ecdh_public_key_b64)?;
    let secret = crypto::derive_pairing_shared_secret(
        &ecdh_secret,
        &settings.device_id,
        &res.receiver_device_id,
        &nonce,
        Some(&payload.pin_token),
    )?;

    let now = now_ms();
    let paired = PairedDevice {
        device_id: res.receiver_device_id,
        device_name: res.receiver_device_name,
        model_name: res.receiver_model,
        role: res.receiver_role,
        last_ip_address: payload.ip_address,
        last_port: payload.port,
        shared_secret: secret,
        paired_at_epoch_ms: now,
        last_seen_epoch_ms: now,
        save_location_override: None,
        total_files_transferred: 0,
        total_bytes_transferred: 0,
    };
    db.upsert_paired_device(&paired)?;
    db.set_selected_receiver(Some(&paired.device_id))?;
    db.append_activity("PAIR", &format!("Paired via code with {}", paired.device_name))?;
    Ok(paired)
}

pub fn parse_direct_ip_target(
    raw_ip: &str,
    port_override: Option<i32>,
) -> AppResult<(String, i32)> {
    let trimmed = raw_ip.trim();
    if trimmed.is_empty() {
        return Err(AppError::Message(
            "Enter your Pixel's Wi-Fi IP address first.".to_string(),
        ));
    }

    if let Ok(sock_addr) = trimmed.parse::<SocketAddr>() {
        let port = i32::from(sock_addr.port());
        if port <= 0 {
            return Err(AppError::Message("Port must be between 1 and 65535.".to_string()));
        }
        return Ok((sock_addr.ip().to_string(), port));
    }

    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        let port = port_override.unwrap_or(DEFAULT_TCP_PORT);
        if !(1..=65535).contains(&port) {
            return Err(AppError::Message("Port must be between 1 and 65535.".to_string()));
        }
        return Ok((ip.to_string(), port));
    }

    if let Some((ip_part, port_part)) = trimmed.rsplit_once(':') {
        if let Ok(ip) = ip_part.trim().parse::<IpAddr>() {
            let port = port_part.trim().parse::<i32>().map_err(|_| {
                AppError::Message("Invalid port number after ':'.".to_string())
            })?;
            if !(1..=65535).contains(&port) {
                return Err(AppError::Message("Port must be between 1 and 65535.".to_string()));
            }
            return Ok((ip.to_string(), port));
        }
    }

    Err(AppError::Message(
        "Enter a valid IPv4 address such as 192.168.1.42 (or 192.168.1.42:42888).".to_string(),
    ))
}

pub fn initiate_direct_ip_pairing(
    db: &Database,
    raw_ip: &str,
    port_override: Option<i32>,
) -> AppResult<PairingSessionState> {
    let (ip, port) = parse_direct_ip_target(raw_ip, port_override)?;
    let synthetic_peer = DiscoveredPeer {
        device_id: String::new(),
        device_name: format!("Pixel ({ip})"),
        model_name: "Backup Pixel".to_string(),
        role: DeviceRole::Receiver,
        ip_address: ip,
        port,
        is_paired: false,
    };
    initiate_interactive_pairing(db, &synthetic_peer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parse_direct_ip_target_handles_plain_ip_port_suffix_and_invalid() {
        assert_eq!(
            parse_direct_ip_target("192.168.1.42", None).unwrap(),
            ("192.168.1.42".to_string(), DEFAULT_TCP_PORT)
        );
        assert_eq!(
            parse_direct_ip_target("192.168.1.42", Some(42889)).unwrap(),
            ("192.168.1.42".to_string(), 42889)
        );
        assert_eq!(
            parse_direct_ip_target(" 10.0.0.15:42900 ", Some(42888)).unwrap(),
            ("10.0.0.15".to_string(), 42900)
        );
        assert!(parse_direct_ip_target("", None).is_err());
        assert!(parse_direct_ip_target("not-an-ip", None).is_err());
        assert!(parse_direct_ip_target("192.168.1.42:99999", None).is_err());
    }

    #[test]
    fn direct_ip_pairing_establishes_sas_session_without_discovery() {
        let tmp = std::env::temp_dir().join(format!("pixrelay_directip_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let db = Database::open_path(&tmp.join("mac.db")).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port() as i32;

        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = BufWriter::new(stream);
                let env = read_plain_envelope(&mut reader).unwrap();
                assert_eq!(env.msg_type, wire_types::PAIR_INIT_REQ);
                let req: PairInitRequest = serde_json::from_str(&env.payload_json).unwrap();

                let (sk, pk) = crypto::generate_ecdh_keypair();
                let rx_pub = crypto::encode_public_key_spki_b64(&pk).unwrap();
                let ecdh = crypto::compute_ecdh_shared_secret(&sk, &req.sender_ecdh_public_key_b64).unwrap();
                let sas = crypto::derive_6digit_sas_code(
                    &ecdh,
                    &req.sender_device_id,
                    "pixel-direct-1",
                    &req.session_nonce,
                )
                .unwrap();

                let res = PairInitResponse {
                    accepted: true,
                    receiver_device_id: "pixel-direct-1".to_string(),
                    receiver_device_name: "Pixel 2 XL".to_string(),
                    receiver_model: "Pixel 2 XL".to_string(),
                    receiver_role: DeviceRole::Receiver,
                    verification_code_6_digit: sas,
                    receiver_ecdh_public_key_b64: rx_pub,
                    qr_server_proof: None,
                    instant_paired_via_qr: false,
                    message: "OK".to_string(),
                };
                write_plain_envelope(
                    &mut writer,
                    wire_types::PAIR_INIT_RES,
                    &serde_json::to_string(&res).unwrap(),
                )
                .unwrap();
            }
        });

        let session = initiate_direct_ip_pairing(&db, &format!("127.0.0.1:{port}"), None).unwrap();
        assert_eq!(session.peer_id, "pixel-direct-1");
        assert_eq!(session.peer_name, "Pixel 2 XL");
        assert_eq!(session.peer_ip, "127.0.0.1");
        assert_eq!(session.peer_port, port);
        assert_eq!(session.verification_code_6_digit.len(), 7);
        assert_eq!(session.shared_secret_candidate.len(), 64);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
