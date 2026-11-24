use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use p256::ecdh::diffie_hellman;
use p256::pkcs8::{DecodePublicKey, EncodePublicKey};
use p256::{PublicKey, SecretKey};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::error::{AppError, AppResult};

type HmacSha256 = Hmac<Sha256>;

pub fn generate_ecdh_keypair() -> (SecretKey, PublicKey) {
    let secret = SecretKey::random(&mut OsRng);
    let public = secret.public_key();
    (secret, public)
}

pub fn encode_public_key_spki_b64(public: &PublicKey) -> AppResult<String> {
    let der = public
        .to_public_key_der()
        .map_err(|e| AppError::Crypto(format!("SPKI encode failed: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(der.as_bytes()))
}

pub fn decode_public_key_spki_b64(encoded_b64: &str) -> AppResult<PublicKey> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded_b64.trim())
        .map_err(|e| AppError::Crypto(format!("Invalid base64 public key: {e}")))?;
    PublicKey::from_public_key_der(&bytes)
        .map_err(|e| AppError::Crypto(format!("Invalid SPKI public key: {e}")))
}

pub fn compute_ecdh_shared_secret(secret: &SecretKey, peer_pub_b64: &str) -> AppResult<Vec<u8>> {
    let peer_pub = decode_public_key_spki_b64(peer_pub_b64)?;
    let shared = diffie_hellman(secret.to_nonzero_scalar(), peer_pub.as_affine());
    Ok(shared.raw_secret_bytes().to_vec())
}

pub fn hmac_sha256_bytes(key: &[u8], data: &[u8]) -> AppResult<Vec<u8>> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key)
        .map_err(|e| AppError::Crypto(format!("HMAC key error: {e}")))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

pub fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> AppResult<String> {
    Ok(hex::encode(hmac_sha256_bytes(key, data)?))
}

pub fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8], length: usize) -> AppResult<Vec<u8>> {
    if length == 0 || length > 255 * 32 {
        return Err(AppError::Crypto(format!(
            "Invalid HKDF output length: {length}"
        )));
    }
    let zero_salt = [0u8; 32];
    let effective_salt = if salt.is_empty() { &zero_salt[..] } else { salt };
    let hk = Hkdf::<Sha256>::new(Some(effective_salt), ikm);
    let mut okm = vec![0u8; length];
    hk.expand(info, &mut okm)
        .map_err(|e| AppError::Crypto(format!("HKDF expand failed: {e}")))?;
    Ok(okm)
}

pub fn generate_random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    OsRng.fill_bytes(&mut buf);
    buf
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn derive_pairing_shared_secret(
    ecdh_secret: &[u8],
    device_id_a: &str,
    device_id_b: &str,
    session_nonce: &str,
    qr_pin_token: Option<&str>,
) -> AppResult<String> {
    let mut ids = [device_id_a.trim(), device_id_b.trim()];
    ids.sort();
    let ordered = format!("{}|{}", ids[0], ids[1]);
    let salt_input = match qr_pin_token.filter(|s| !s.trim().is_empty()) {
        Some(token) => format!("{ordered}|{session_nonce}|{token}"),
        None => format!("{ordered}|{session_nonce}"),
    };
    let key_bytes = hkdf_sha256(
        ecdh_secret,
        salt_input.as_bytes(),
        b"PIX_RELAY_SHARED_SECRET_V2",
        32,
    )?;
    Ok(hex::encode(key_bytes))
}

pub fn derive_6digit_sas_code(
    ecdh_secret: &[u8],
    device_id_a: &str,
    device_id_b: &str,
    session_nonce: &str,
) -> AppResult<String> {
    let mut ids = [device_id_a.trim(), device_id_b.trim()];
    ids.sort();
    let ordered = format!("{}|{}", ids[0], ids[1]);
    let salt_input = format!("{ordered}|{session_nonce}");
    let sas_bytes = hkdf_sha256(ecdh_secret, salt_input.as_bytes(), b"PIX_RELAY_SAS_V2", 4)?;
    let value = u32::from_be_bytes([sas_bytes[0], sas_bytes[1], sas_bytes[2], sas_bytes[3]]);
    let six = value % 1_000_000;
    Ok(format!("{:03} {:03}", six / 1000, six % 1000))
}

pub fn compute_qr_pin_proof(
    qr_pin_token: &str,
    sender_device_id: &str,
    receiver_device_id: &str,
    session_nonce: &str,
    sender_ecdh_pub_b64: &str,
) -> AppResult<String> {
    let data = format!(
        "QR_CLIENT_PROOF|{sender_device_id}|{receiver_device_id}|{session_nonce}|{sender_ecdh_pub_b64}"
    );
    hmac_sha256_hex(qr_pin_token.as_bytes(), data.as_bytes())
}

pub fn compute_qr_server_proof(
    qr_pin_token: &str,
    receiver_device_id: &str,
    sender_device_id: &str,
    session_nonce: &str,
    receiver_ecdh_pub_b64: &str,
) -> AppResult<String> {
    let data = format!(
        "QR_SERVER_PROOF|{receiver_device_id}|{sender_device_id}|{session_nonce}|{receiver_ecdh_pub_b64}"
    );
    hmac_sha256_hex(qr_pin_token.as_bytes(), data.as_bytes())
}

pub fn compute_pair_confirm_proof(
    shared_secret: &str,
    sender_device_id: &str,
    session_nonce: &str,
) -> AppResult<String> {
    let data = format!("PAIR_CONFIRM|{sender_device_id}|{session_nonce}");
    hmac_sha256_hex(shared_secret.as_bytes(), data.as_bytes())
}

pub fn derive_session_key(
    shared_secret: &str,
    client_nonce: &[u8],
    server_nonce: &[u8],
) -> AppResult<[u8; 32]> {
    let mut salt = Vec::with_capacity(client_nonce.len() + server_nonce.len());
    salt.extend_from_slice(client_nonce);
    salt.extend_from_slice(server_nonce);
    let key = hkdf_sha256(
        shared_secret.as_bytes(),
        &salt,
        b"PIX_RELAY_SESSION_AES256_GCM_V2",
        32,
    )?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&key);
    Ok(out)
}

pub fn compute_server_hello_proof(
    session_key: &[u8],
    sender_device_id: &str,
    client_nonce_b64: &str,
    server_nonce_b64: &str,
) -> AppResult<String> {
    let data = format!("SERVER_HELLO|{sender_device_id}|{client_nonce_b64}|{server_nonce_b64}");
    hmac_sha256_hex(session_key, data.as_bytes())
}

pub fn compute_file_offer_signature(
    sender_device_id: &str,
    sha256: &str,
    shared_secret: &str,
) -> String {
    sha256_hex(format!("{sender_device_id}|{sha256}|{shared_secret}").as_bytes())
}

pub fn compute_rpc_signature(
    sender_device_id: &str,
    rpc_type: &str,
    shared_secret: &str,
) -> AppResult<String> {
    hmac_sha256_hex(
        shared_secret.as_bytes(),
        format!("{sender_device_id}|{rpc_type}").as_bytes(),
    )
}

pub fn verify_signature(expected: &str, actual: &str) -> bool {
    expected.len() == actual.len() && bool::from(expected.as_bytes().ct_eq(actual.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecdh_spki_key_exchange_is_symmetric() {
        let (priv_a, pub_a) = generate_ecdh_keypair();
        let (priv_b, pub_b) = generate_ecdh_keypair();
        let (priv_m, pub_m) = generate_ecdh_keypair();

        let pub_a_b64 = encode_public_key_spki_b64(&pub_a).unwrap();
        let pub_b_b64 = encode_public_key_spki_b64(&pub_b).unwrap();
        let pub_m_b64 = encode_public_key_spki_b64(&pub_m).unwrap();
        // X.509 SPKI DER for P-256 uncompressed key is always 91 bytes -> 122 base64url chars
        assert_eq!(pub_a_b64.len(), 122);

        let sec_ab = compute_ecdh_shared_secret(&priv_a, &pub_b_b64).unwrap();
        let sec_ba = compute_ecdh_shared_secret(&priv_b, &pub_a_b64).unwrap();
        let sec_am = compute_ecdh_shared_secret(&priv_a, &pub_m_b64).unwrap();
        let _ = priv_m;

        assert_eq!(sec_ab.len(), 32);
        assert_eq!(sec_ab, sec_ba);
        assert_ne!(sec_ab, sec_am);
    }

    #[test]
    fn hkdf_and_sas_match_deterministic_android_logic() {
        let ikm = b"ikm-material";
        let k1 = hkdf_sha256(ikm, b"", b"PIX_RELAY_SHARED_SECRET_V2", 32).unwrap();
        let k2 = hkdf_sha256(ikm, &[0u8; 32], b"PIX_RELAY_SHARED_SECRET_V2", 32).unwrap();
        assert_eq!(k1, k2);

        let sas1 = derive_6digit_sas_code(&k1, "mac-1", "pixel-1", "nonce42").unwrap();
        let sas2 = derive_6digit_sas_code(&k1, "pixel-1", "mac-1", "nonce42").unwrap();
        assert_eq!(sas1, sas2);
        assert_eq!(sas1.len(), 7);
        assert_eq!(&sas1[3..4], " ");

        let s1 = derive_pairing_shared_secret(&k1, "mac-1", "pixel-1", "nonce42", Some("tok")).unwrap();
        let s2 = derive_pairing_shared_secret(&k1, "pixel-1", "mac-1", "nonce42", Some("tok")).unwrap();
        assert_eq!(s1.len(), 64);
        assert_eq!(s1, s2);
    }
}
