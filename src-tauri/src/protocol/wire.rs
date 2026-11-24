use std::io::{Read, Write};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

use crate::error::{AppError, AppResult};
use crate::protocol::crypto;
use crate::protocol::messages::{
    wire_types, SecureHelloRequest, SecureHelloResponse, WireEnvelope,
};

const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const KIND_ENVELOPE: u8 = 0x10;
const KIND_STREAM_CHUNK: u8 = 0x20;
const KIND_STREAM_END: u8 = 0x21;
const DIR_CLIENT_TO_SERVER: u8 = 0x01;
const DIR_SERVER_TO_CLIENT: u8 = 0x02;
const GCM_TAG_BYTES: usize = 16;
const MAX_CIPHERTEXT_BYTES: usize = MAX_FRAME_BYTES + GCM_TAG_BYTES;
pub const STREAM_CHUNK_BYTES: usize = 128 * 1024;

pub fn write_plain_envelope<W: Write>(
    writer: &mut W,
    msg_type: &str,
    payload_json: &str,
) -> AppResult<()> {
    let env = WireEnvelope {
        msg_type: msg_type.to_string(),
        payload_json: payload_json.to_string(),
    };
    let bytes = serde_json::to_vec(&env)?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| AppError::Message("Envelope too large".to_string()))?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn read_plain_envelope<R: Read>(reader: &mut R) -> AppResult<WireEnvelope> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(AppError::Message(format!("Invalid frame length: {len}")));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    let env: WireEnvelope = serde_json::from_slice(&buf)?;
    Ok(env)
}

pub enum SecureSessionOutcome<S> {
    Established(SecureWireChannel<S>),
    Rejected(String),
}

pub struct SecureWireChannel<S> {
    stream: S,
    cipher: Aes256Gcm,
    outbound_dir: u8,
    inbound_dir: u8,
    send_seq: u64,
    recv_seq: u64,
}

impl<S: Read + Write> SecureWireChannel<S> {
    pub fn new(stream: S, session_key: &[u8; 32], is_client: bool) -> Self {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(session_key));
        let (outbound_dir, inbound_dir) = if is_client {
            (DIR_CLIENT_TO_SERVER, DIR_SERVER_TO_CLIENT)
        } else {
            (DIR_SERVER_TO_CLIENT, DIR_CLIENT_TO_SERVER)
        };
        Self {
            stream,
            cipher,
            outbound_dir,
            inbound_dir,
            send_seq: 0,
            recv_seq: 0,
        }
    }

    pub fn open_client_session(
        mut stream: S,
        sender_device_id: &str,
        shared_secret: &str,
    ) -> AppResult<SecureSessionOutcome<S>> {
        if shared_secret.trim().is_empty() {
            return Ok(SecureSessionOutcome::Rejected(
                "Security verification failed. Please re-pair your Pixel.".to_string(),
            ));
        }

        let client_nonce = crypto::generate_random_bytes(16);
        let client_nonce_b64 = URL_SAFE_NO_PAD.encode(&client_nonce);
        let req = SecureHelloRequest {
            sender_device_id: sender_device_id.to_string(),
            client_nonce_b64: client_nonce_b64.clone(),
        };
        write_plain_envelope(
            &mut stream,
            wire_types::SECURE_HELLO_REQ,
            &serde_json::to_string(&req)?,
        )?;

        let env = read_plain_envelope(&mut stream)?;
        if env.msg_type != wire_types::SECURE_HELLO_RES {
            return Err(AppError::Message(format!(
                "Unexpected handshake response: {}",
                env.msg_type
            )));
        }
        let res: SecureHelloResponse = serde_json::from_str(&env.payload_json)?;
        if !res.accepted {
            let msg = if res.message.trim().is_empty() {
                "Security verification failed. Please re-pair your Pixel.".to_string()
            } else {
                res.message
            };
            return Ok(SecureSessionOutcome::Rejected(msg));
        }

        let server_nonce = match URL_SAFE_NO_PAD.decode(res.server_nonce_b64.trim()) {
            Ok(bytes) if (16..=64).contains(&bytes.len()) => bytes,
            _ => {
                return Ok(SecureSessionOutcome::Rejected(
                    "Security verification failed. Please re-pair your Pixel.".to_string(),
                ));
            }
        };

        let session_key = crypto::derive_session_key(shared_secret, &client_nonce, &server_nonce)?;
        let expected_proof = crypto::compute_server_hello_proof(
            &session_key,
            sender_device_id,
            &client_nonce_b64,
            &res.server_nonce_b64,
        )?;
        if !crypto::verify_signature(&expected_proof, &res.server_proof) {
            return Ok(SecureSessionOutcome::Rejected(
                "Security verification failed. Please re-pair your Pixel.".to_string(),
            ));
        }

        Ok(SecureSessionOutcome::Established(Self::new(
            stream,
            &session_key,
            true,
        )))
    }

    pub fn write_envelope(&mut self, msg_type: &str, payload_json: &str) -> AppResult<()> {
        let env = WireEnvelope {
            msg_type: msg_type.to_string(),
            payload_json: payload_json.to_string(),
        };
        let bytes = serde_json::to_vec(&env)?;
        self.write_frame(KIND_ENVELOPE, &bytes)?;
        self.stream.flush()?;
        Ok(())
    }

    pub fn read_envelope(&mut self) -> AppResult<WireEnvelope> {
        let (kind, plaintext) = self.read_frame()?;
        if kind != KIND_ENVELOPE {
            return Err(AppError::Crypto(format!(
                "Expected encrypted envelope frame, got {kind}"
            )));
        }
        let env: WireEnvelope = serde_json::from_slice(&plaintext)?;
        Ok(env)
    }

    pub fn write_encrypted_stream<R: Read, F: FnMut(usize)>(
        &mut self,
        source: &mut R,
        total_bytes: u64,
        mut on_bytes_written: F,
    ) -> AppResult<()> {
        let mut buf = vec![0u8; STREAM_CHUNK_BYTES];
        let mut remaining = total_bytes;
        while remaining > 0 {
            let to_read = (remaining as usize).min(buf.len());
            let read = source.read(&mut buf[..to_read])?;
            if read == 0 {
                return Err(AppError::Message(format!(
                    "Unexpected end of file before {total_bytes} bytes"
                )));
            }
            self.write_frame(KIND_STREAM_CHUNK, &buf[..read])?;
            remaining -= read as u64;
            on_bytes_written(read);
        }
        self.write_frame(KIND_STREAM_END, &[])?;
        self.stream.flush()?;
        Ok(())
    }

    pub fn read_encrypted_stream<F: FnMut(&[u8]) -> AppResult<()>>(
        &mut self,
        expected_bytes: u64,
        mut on_chunk: F,
    ) -> AppResult<()> {
        let mut received: u64 = 0;
        loop {
            let (kind, plaintext) = self.read_frame()?;
            match kind {
                KIND_STREAM_CHUNK => {
                    received += plaintext.len() as u64;
                    if received > expected_bytes {
                        return Err(AppError::Crypto(format!(
                            "Encrypted stream exceeded expected {expected_bytes} bytes"
                        )));
                    }
                    on_chunk(&plaintext)?;
                }
                KIND_STREAM_END => {
                    if received != expected_bytes {
                        return Err(AppError::Crypto(format!(
                            "Encrypted stream truncated: expected {expected_bytes}, got {received}"
                        )));
                    }
                    return Ok(());
                }
                other => {
                    return Err(AppError::Crypto(format!(
                        "Unexpected encrypted stream frame kind: {other}"
                    )));
                }
            }
        }
    }

    fn write_frame(&mut self, kind: u8, plaintext: &[u8]) -> AppResult<()> {
        let seq = self.send_seq;
        self.send_seq += 1;
        let nonce = build_nonce(self.outbound_dir, seq);
        let aad = build_aad(kind, seq);

        let ciphertext = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|e| AppError::Crypto(format!("AES-GCM encrypt failed: {e}")))?;

        let len = u32::try_from(ciphertext.len())
            .map_err(|_| AppError::Crypto("Ciphertext frame too large".to_string()))?;
        self.stream.write_all(&[kind])?;
        self.stream.write_all(&len.to_be_bytes())?;
        self.stream.write_all(&ciphertext)?;
        Ok(())
    }

    fn read_frame(&mut self) -> AppResult<(u8, Vec<u8>)> {
        let mut kind_buf = [0u8; 1];
        self.stream.read_exact(&mut kind_buf)?;
        let kind = kind_buf[0];

        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if !(GCM_TAG_BYTES..=MAX_CIPHERTEXT_BYTES).contains(&len) {
            return Err(AppError::Crypto(format!(
                "Invalid encrypted frame length: {len}"
            )));
        }

        let mut ciphertext = vec![0u8; len];
        self.stream.read_exact(&mut ciphertext)?;

        let seq = self.recv_seq;
        self.recv_seq += 1;
        let nonce = build_nonce(self.inbound_dir, seq);
        let aad = build_aad(kind, seq);

        let plaintext = self
            .cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|e| AppError::Crypto(format!("AES-GCM decrypt failed: {e}")))?;

        Ok((kind, plaintext))
    }
}

fn build_nonce(direction: u8, seq: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[0] = direction;
    nonce[4..12].copy_from_slice(&seq.to_be_bytes());
    nonce
}

fn build_aad(kind: u8, seq: u64) -> [u8; 9] {
    let mut aad = [0u8; 9];
    aad[0] = kind;
    aad[1..9].copy_from_slice(&seq.to_be_bytes());
    aad
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    struct PipeBuffer {
        read_cur: Cursor<Vec<u8>>,
        written: Vec<u8>,
    }

    impl PipeBuffer {
        fn new(incoming: Vec<u8>) -> Self {
            Self {
                read_cur: Cursor::new(incoming),
                written: Vec::new(),
            }
        }
    }

    impl Read for PipeBuffer {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.read_cur.read(buf)
        }
    }

    impl Write for PipeBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn encrypted_envelope_roundtrip_tamper_and_reorder() {
        let secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let c_nonce = crypto::generate_random_bytes(16);
        let s_nonce = crypto::generate_random_bytes(16);
        let key = crypto::derive_session_key(secret, &c_nonce, &s_nonce).unwrap();

        let mut client = SecureWireChannel::new(PipeBuffer::new(vec![]), &key, true);
        client.write_envelope(wire_types::TELEMETRY_REQ, r#"{"a":1}"#).unwrap();
        client.write_envelope(wire_types::CHECK_LEDGER_REQ, r#"{"b":2}"#).unwrap();
        let raw = client.stream.written;

        let mut server = SecureWireChannel::new(PipeBuffer::new(raw.clone()), &key, false);
        let e1 = server.read_envelope().unwrap();
        let e2 = server.read_envelope().unwrap();
        assert_eq!(e1.msg_type, wire_types::TELEMETRY_REQ);
        assert_eq!(e2.msg_type, wire_types::CHECK_LEDGER_REQ);

        let mut tampered = raw.clone();
        let last = tampered.len() - 3;
        tampered[last] ^= 0xFF;
        let mut tampered_server = SecureWireChannel::new(PipeBuffer::new(tampered), &key, false);
        assert!(tampered_server.read_envelope().is_ok());
        assert!(tampered_server.read_envelope().is_err());

        let f0_len = u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]]) as usize;
        let f0_total = 5 + f0_len;
        let mut swapped = Vec::with_capacity(raw.len());
        swapped.extend_from_slice(&raw[f0_total..]);
        swapped.extend_from_slice(&raw[..f0_total]);
        let mut swapped_server = SecureWireChannel::new(PipeBuffer::new(swapped), &key, false);
        assert!(swapped_server.read_envelope().is_err());
    }

    #[test]
    fn encrypted_stream_multi_chunk_and_truncation_detected() {
        let mut key = [0u8; 32];
        key[..32].copy_from_slice(&crypto::generate_random_bytes(32));

        let payload = vec![0x5Au8; 300 * 1024];
        let mut client = SecureWireChannel::new(PipeBuffer::new(vec![]), &key, true);
        let mut written_total = 0usize;
        client
            .write_encrypted_stream(&mut Cursor::new(&payload), payload.len() as u64, |n| {
                written_total += n;
            })
            .unwrap();
        assert_eq!(written_total, payload.len());

        let wire_bytes = client.stream.written;
        let mut server = SecureWireChannel::new(PipeBuffer::new(wire_bytes.clone()), &key, false);
        let mut received = Vec::new();
        server
            .read_encrypted_stream(payload.len() as u64, |chunk| {
                received.extend_from_slice(chunk);
                Ok(())
            })
            .unwrap();
        assert_eq!(received, payload);

        let without_end = &wire_bytes[..wire_bytes.len() - 21];
        let mut broken_server =
            SecureWireChannel::new(PipeBuffer::new(without_end.to_vec()), &key, false);
        assert!(broken_server
            .read_encrypted_stream(payload.len() as u64, |_| Ok(()))
            .is_err());
    }
}
