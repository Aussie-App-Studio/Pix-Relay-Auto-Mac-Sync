use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

pub mod wire_types {
    pub const SECURE_HELLO_REQ: &str = "SECURE_HELLO_REQ";
    pub const SECURE_HELLO_RES: &str = "SECURE_HELLO_RES";
    pub const TELEMETRY_REQ: &str = "TELEMETRY_REQ";
    pub const TELEMETRY_RES: &str = "TELEMETRY_RES";
    pub const PAIR_INIT_REQ: &str = "PAIR_INIT_REQ";
    pub const PAIR_INIT_RES: &str = "PAIR_INIT_RES";
    pub const PAIR_CONFIRM_REQ: &str = "PAIR_CONFIRM_REQ";
    pub const PAIR_CONFIRM_RES: &str = "PAIR_CONFIRM_RES";
    pub const CHECK_LEDGER_REQ: &str = "CHECK_LEDGER_REQ";
    pub const CHECK_LEDGER_RES: &str = "CHECK_LEDGER_RES";
    pub const FILE_OFFER_REQ: &str = "FILE_OFFER_REQ";
    pub const FILE_OFFER_RES: &str = "FILE_OFFER_RES";
    pub const FILE_COMMIT_RES: &str = "FILE_COMMIT_RES";
    pub const GET_RECEIVER_SETTINGS_REQ: &str = "GET_RECEIVER_SETTINGS_REQ";
    pub const GET_RECEIVER_SETTINGS_RES: &str = "GET_RECEIVER_SETTINGS_RES";
    pub const UPDATE_RECEIVER_SETTINGS_REQ: &str = "UPDATE_RECEIVER_SETTINGS_REQ";
    pub const UPDATE_RECEIVER_SETTINGS_RES: &str = "UPDATE_RECEIVER_SETTINGS_RES";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceRole {
    Sender,
    Receiver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SaveLocationMode {
    CameraFolder,
    SeparateByPhone,
    MirrorOriginalFolders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageFullCleanupMode {
    NotifyForFreeUpSpace,
    AutoCleanAfterUpload,
    PauseOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireEnvelope {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryRequest {
    pub sender_device_id: String,
    pub sender_device_name: String,
    #[serde(default)]
    pub auth_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverTelemetry {
    pub device_id: String,
    pub device_name: String,
    pub model_name: String,
    #[serde(default)]
    pub pixel_benefit_summary: String,
    pub free_bytes: i64,
    pub total_bytes: i64,
    pub reserve_storage_gb: i32,
    pub battery_percent: i32,
    pub is_charging: bool,
    pub temperature_celsius: f32,
    pub max_safe_temp_celsius: f32,
    pub is_thermal_paused: bool,
    pub is_storage_paused: bool,
    pub is_screen_off_ready: bool,
    pub outbound_upload_bytes_per_sec: i64,
    #[serde(default)]
    pub friendly_cloud_status: String,
    #[serde(default)]
    pub historical_received_count: i64,
    #[serde(default)]
    pub historical_received_bytes: i64,
    #[serde(default)]
    pub staged_files_on_disk_count: i32,
    #[serde(default)]
    pub timestamp_epoch_ms: i64,
    #[serde(default)]
    pub is_manual_paused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryResponse {
    pub telemetry: ReceiverTelemetry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecureHelloRequest {
    pub sender_device_id: String,
    pub client_nonce_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecureHelloResponse {
    pub accepted: bool,
    #[serde(default)]
    pub server_nonce_b64: String,
    #[serde(default)]
    pub server_proof: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairInitRequest {
    pub sender_device_id: String,
    pub sender_device_name: String,
    pub sender_model: String,
    pub sender_role: DeviceRole,
    pub sender_ip: String,
    pub sender_port: i32,
    pub session_nonce: String,
    #[serde(default)]
    pub sender_ecdh_public_key_b64: String,
    #[serde(default)]
    pub qr_pin_proof: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairInitResponse {
    pub accepted: bool,
    pub receiver_device_id: String,
    pub receiver_device_name: String,
    pub receiver_model: String,
    pub receiver_role: DeviceRole,
    #[serde(default)]
    pub verification_code_6_digit: String,
    #[serde(default)]
    pub receiver_ecdh_public_key_b64: String,
    #[serde(default)]
    pub qr_server_proof: Option<String>,
    #[serde(default)]
    pub instant_paired_via_qr: bool,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairConfirmRequest {
    pub sender_device_id: String,
    pub session_nonce: String,
    pub approved: bool,
    #[serde(default)]
    pub confirmation_proof: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairConfirmResponse {
    pub paired: bool,
    pub receiver_approved: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrPairingPayload {
    pub device_id: String,
    pub device_name: String,
    pub model_name: String,
    pub role: DeviceRole,
    pub ip_address: String,
    pub port: i32,
    pub pin_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckLedgerBatchRequest {
    pub sender_device_id: String,
    pub fingerprints: Vec<String>,
    #[serde(default)]
    pub auth_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckLedgerBatchResponse {
    pub already_received_fingerprints: Vec<String>,
    #[serde(default = "default_true")]
    pub authorized: bool,
    #[serde(default)]
    pub cleaned_from_disk_fingerprints: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOfferRequest {
    pub sender_device_id: String,
    pub sender_device_name: String,
    pub auth_signature: String,
    pub display_name: String,
    pub bucket_name: String,
    pub relative_path: String,
    pub size_bytes: i64,
    pub date_taken_epoch_ms: i64,
    pub date_modified_epoch_sec: i64,
    pub mime_type: String,
    pub is_video: bool,
    pub fingerprint_key: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileOfferDecision {
    SendBytes,
    SkipAlreadyHave,
    PausedStorageFull,
    PausedThermalHot,
    PausedManual,
    RejectedUnauthorized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOfferResponse {
    pub decision: FileOfferDecision,
    pub friendly_message: String,
    #[serde(default)]
    pub resume_offset_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCommitResult {
    pub verified_and_saved: bool,
    pub sha256: String,
    pub saved_display_path: String,
    pub friendly_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteReceiverSettingsPayload {
    #[serde(default)]
    pub receiving_paused: bool,
    #[serde(default = "default_reserve_gb")]
    pub storage_reserve_gb: i32,
    #[serde(default = "default_cleanup_mode")]
    pub storage_cleanup_mode: StorageFullCleanupMode,
    #[serde(default = "default_auto_clean_days")]
    pub auto_clean_after_days: i32,
    #[serde(default = "default_true")]
    pub thermal_guard_enabled: bool,
    #[serde(default = "default_max_temp")]
    pub max_safe_temp_celsius: f32,
    #[serde(default = "default_save_mode")]
    pub default_save_location_mode: SaveLocationMode,
}

fn default_reserve_gb() -> i32 {
    3
}
fn default_cleanup_mode() -> StorageFullCleanupMode {
    StorageFullCleanupMode::NotifyForFreeUpSpace
}
fn default_auto_clean_days() -> i32 {
    7
}
fn default_max_temp() -> f32 {
    40.0
}
fn default_save_mode() -> SaveLocationMode {
    SaveLocationMode::CameraFolder
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetReceiverSettingsRequest {
    pub sender_device_id: String,
    #[serde(default)]
    pub auth_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetReceiverSettingsResponse {
    pub accepted: bool,
    #[serde(default)]
    pub settings: Option<RemoteReceiverSettingsPayload>,
    #[serde(default)]
    pub telemetry: Option<ReceiverTelemetry>,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReceiverSettingsRequest {
    pub sender_device_id: String,
    #[serde(default)]
    pub auth_signature: String,
    pub settings: RemoteReceiverSettingsPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReceiverSettingsResponse {
    pub accepted: bool,
    #[serde(default)]
    pub settings: Option<RemoteReceiverSettingsPayload>,
    #[serde(default)]
    pub telemetry: Option<ReceiverTelemetry>,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UdpBeaconPayload {
    #[serde(default = "default_app_id")]
    pub app: String,
    pub device_id: String,
    pub device_name: String,
    pub model_name: String,
    pub role: DeviceRole,
    pub tcp_port: i32,
}

fn default_app_id() -> String {
    "PIX_RELAY_1".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPeer {
    pub device_id: String,
    pub device_name: String,
    pub model_name: String,
    pub role: DeviceRole,
    pub ip_address: String,
    pub port: i32,
    pub is_paired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub device_id: String,
    pub device_name: String,
    pub model_name: String,
    pub role: DeviceRole,
    pub last_ip_address: String,
    pub last_port: i32,
    pub shared_secret: String,
    pub paired_at_epoch_ms: i64,
    pub last_seen_epoch_ms: i64,
    pub save_location_override: Option<SaveLocationMode>,
    pub total_files_transferred: i64,
    pub total_bytes_transferred: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSessionState {
    pub peer_id: String,
    pub peer_name: String,
    pub peer_model: String,
    pub peer_ip: String,
    pub peer_port: i32,
    pub verification_code_6_digit: String,
    pub shared_secret_candidate: String,
    pub session_nonce: String,
}

pub fn decode_pairing_code(raw: &str) -> Option<QrPairingPayload> {
    let trimmed = raw.trim();
    let payload_part = if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("PIX1:") {
        &trimmed[5..]
    } else {
        trimmed
    };
    let decoded = URL_SAFE_NO_PAD.decode(payload_part.trim()).ok()?;
    let json_str = String::from_utf8(decoded).ok()?;
    serde_json::from_str::<QrPairingPayload>(&json_str).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_serialization_matches_android_kotlin_field_names() {
        let req = CheckLedgerBatchResponse {
            already_received_fingerprints: vec!["a.jpg|12|100".into()],
            authorized: true,
            cleaned_from_disk_fingerprints: vec!["b.jpg|34|200".into()],
        };
        let raw = serde_json::to_string(&req).unwrap();
        assert!(raw.contains("\"alreadyReceivedFingerprints\""));
        assert!(raw.contains("\"cleanedFromDiskFingerprints\""));

        let old_json = r#"{"alreadyReceivedFingerprints":["x.jpg|1|2"],"authorized":true}"#;
        let parsed: CheckLedgerBatchResponse = serde_json::from_str(old_json).unwrap();
        assert!(parsed.cleaned_from_disk_fingerprints.is_empty());

        let beacon_json = r#"{"app":"PIX_RELAY_1","deviceId":"px-1","deviceName":"OG Pixel","modelName":"Pixel XL","role":"RECEIVER","tcpPort":42888}"#;
        let beacon: UdpBeaconPayload = serde_json::from_str(beacon_json).unwrap();
        assert_eq!(beacon.role, DeviceRole::Receiver);
        assert_eq!(beacon.tcp_port, 42888);
    }

    #[test]
    fn decode_pix1_pairing_code_works() {
        let payload = QrPairingPayload {
            device_id: "pixel-1".into(),
            device_name: "Backup Pixel".into(),
            model_name: "Pixel XL".into(),
            role: DeviceRole::Receiver,
            ip_address: "192.168.1.44".into(),
            port: 42888,
            pin_token: "secret-pin".into(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let code = format!("PIX1:{}", URL_SAFE_NO_PAD.encode(json.as_bytes()));
        let decoded = decode_pairing_code(&code).unwrap();
        assert_eq!(decoded.device_id, "pixel-1");
        assert_eq!(decoded.port, 42888);
    }
}
