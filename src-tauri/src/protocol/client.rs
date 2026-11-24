use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::protocol::crypto;
use crate::protocol::discovery::DiscoveryManager;
use crate::protocol::messages::{
    wire_types, CheckLedgerBatchRequest, CheckLedgerBatchResponse, FileCommitResult,
    FileOfferDecision, FileOfferRequest, FileOfferResponse, GetReceiverSettingsRequest,
    GetReceiverSettingsResponse, PairedDevice, ReceiverTelemetry, RemoteReceiverSettingsPayload,
    TelemetryRequest, TelemetryResponse, UpdateReceiverSettingsRequest,
    UpdateReceiverSettingsResponse,
};
use crate::protocol::wire::{SecureSessionOutcome, SecureWireChannel};

#[derive(Debug, Clone)]
pub struct TransferCandidate {
    pub file_path: String,
    pub display_name: String,
    pub bucket_name: String,
    pub relative_path: String,
    pub size_bytes: i64,
    pub date_taken_epoch_ms: i64,
    pub date_modified_epoch_sec: i64,
    pub mime_type: String,
    pub is_video: bool,
    pub fingerprint_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferProgressEvent {
    Idle,
    Scanning {
        message: String,
    },
    Active {
        #[serde(rename = "targetDeviceName")]
        target_device_name: String,
        #[serde(rename = "currentFileName")]
        current_file_name: String,
        #[serde(rename = "currentFileIndex")]
        current_file_index: usize,
        #[serde(rename = "totalFiles")]
        total_files: usize,
        #[serde(rename = "bytesTransferred")]
        bytes_transferred: i64,
        #[serde(rename = "totalBytes")]
        total_bytes: i64,
        #[serde(rename = "speedMbPerSec")]
        speed_mb_per_sec: f64,
        #[serde(rename = "verifiedCount")]
        verified_count: usize,
        #[serde(rename = "skippedDuplicateCount")]
        skipped_duplicate_count: usize,
    },
    Paused {
        #[serde(rename = "targetDeviceName")]
        target_device_name: String,
        #[serde(rename = "friendlyReason")]
        friendly_reason: String,
        #[serde(rename = "completedFiles")]
        completed_files: usize,
        #[serde(rename = "remainingFiles")]
        remaining_files: usize,
    },
    Completed {
        #[serde(rename = "targetDeviceName")]
        target_device_name: String,
        #[serde(rename = "transferredCount")]
        transferred_count: usize,
        #[serde(rename = "skippedDuplicateCount")]
        skipped_duplicate_count: usize,
        #[serde(rename = "totalBytesSent")]
        total_bytes_sent: i64,
        #[serde(rename = "elapsedSeconds")]
        elapsed_seconds: f64,
    },
    Failed {
        #[serde(rename = "friendlyMessage")]
        friendly_message: String,
    },
}

fn open_secure_channel(
    device: &PairedDevice,
    sender_device_id: &str,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> AppResult<SecureWireChannel<TcpStream>> {
    let addr: SocketAddr = format!("{}:{}", device.last_ip_address, device.last_port)
        .parse()
        .map_err(|_| AppError::Message("Invalid Pixel IP/port".to_string()))?;
    let stream = TcpStream::connect_timeout(&addr, connect_timeout)?;
    stream.set_read_timeout(Some(io_timeout))?;
    stream.set_write_timeout(Some(io_timeout))?;
    let _ = stream.set_nodelay(true);

    match SecureWireChannel::open_client_session(stream, sender_device_id, &device.shared_secret)? {
        SecureSessionOutcome::Established(ch) => Ok(ch),
        SecureSessionOutcome::Rejected(msg) => Err(AppError::Message(msg)),
    }
}

async fn resolve_with_rediscovery<T, F>(
    db: &Database,
    discovery: &DiscoveryManager,
    device: &PairedDevice,
    op: F,
) -> AppResult<(PairedDevice, T)>
where
    T: Send + 'static,
    F: Fn(&PairedDevice) -> AppResult<T> + Send + Sync + Clone + 'static,
{
    let latest = db
        .get_paired_device(&device.device_id)?
        .unwrap_or_else(|| device.clone());
    let op_clone = op.clone();
    let latest_clone = latest.clone();
    let first = tokio::task::spawn_blocking(move || op_clone(&latest_clone))
        .await
        .map_err(|e| AppError::Message(e.to_string()))?;
    match first {
        Ok(val) => Ok((latest, val)),
        Err(AppError::Io(_)) => {
            let (new_ip, new_port) = discovery
                .rediscover_peer_endpoint(&latest.device_id, Duration::from_secs(4))
                .await
                .ok_or_else(|| {
                    AppError::Message(format!(
                        "Could not reach {} on Wi-Fi ({}). Make sure both devices are on the same network.",
                        latest.device_name, latest.last_ip_address
                    ))
                })?;
            db.update_peer_endpoint(&latest.device_id, &new_ip, new_port)?;
            let mut updated = latest;
            updated.last_ip_address = new_ip;
            updated.last_port = new_port;
            let updated_clone = updated.clone();
            let val = tokio::task::spawn_blocking(move || op(&updated_clone))
                .await
                .map_err(|e| AppError::Message(e.to_string()))??;
            Ok((updated, val))
        }
        Err(e) => Err(e),
    }
}

pub async fn poll_telemetry(
    db: &Database,
    discovery: &DiscoveryManager,
    device: &PairedDevice,
) -> AppResult<ReceiverTelemetry> {
    let settings = db.get_settings()?;
    let sender_id = settings.device_id;
    let sender_name = settings.device_name;
    let (_dev, telemetry) = resolve_with_rediscovery(db, discovery, device, move |dev| {
        let mut channel = open_secure_channel(
            dev,
            &sender_id,
            Duration::from_millis(3500),
            Duration::from_secs(5),
        )?;
        let sig = crypto::compute_rpc_signature(
            &sender_id,
            wire_types::TELEMETRY_REQ,
            &dev.shared_secret,
        )?;
        let req = TelemetryRequest {
            sender_device_id: sender_id.clone(),
            sender_device_name: sender_name.clone(),
            auth_signature: sig,
        };
        channel.write_envelope(wire_types::TELEMETRY_REQ, &serde_json::to_string(&req)?)?;
        let env = channel.read_envelope()?;
        if env.msg_type != wire_types::TELEMETRY_RES {
            return Err(AppError::Message("Unexpected telemetry response".to_string()));
        }
        let res: TelemetryResponse = serde_json::from_str(&env.payload_json)?;
        Ok(res.telemetry)
    })
    .await?;
    Ok(telemetry)
}

pub async fn get_remote_receiver_settings(
    db: &Database,
    discovery: &DiscoveryManager,
    device: &PairedDevice,
) -> AppResult<(RemoteReceiverSettingsPayload, Option<ReceiverTelemetry>)> {
    let sender_id = db.get_settings()?.device_id;
    let (_dev, pair) = resolve_with_rediscovery(db, discovery, device, move |dev| {
        let mut channel = open_secure_channel(
            dev,
            &sender_id,
            Duration::from_secs(4),
            Duration::from_secs(6),
        )?;
        let sig = crypto::compute_rpc_signature(
            &sender_id,
            wire_types::GET_RECEIVER_SETTINGS_REQ,
            &dev.shared_secret,
        )?;
        let req = GetReceiverSettingsRequest {
            sender_device_id: sender_id.clone(),
            auth_signature: sig,
        };
        channel.write_envelope(
            wire_types::GET_RECEIVER_SETTINGS_REQ,
            &serde_json::to_string(&req)?,
        )?;
        let env = channel.read_envelope()?;
        if env.msg_type != wire_types::GET_RECEIVER_SETTINGS_RES {
            return Err(AppError::Message("Unexpected response from Pixel".to_string()));
        }
        let res: GetReceiverSettingsResponse = serde_json::from_str(&env.payload_json)?;
        if res.accepted {
            if let Some(s) = res.settings {
                return Ok((s, res.telemetry));
            }
        }
        Err(AppError::Message(if res.message.is_empty() {
            "Unable to load settings from Pixel".to_string()
        } else {
            res.message
        }))
    })
    .await?;
    Ok(pair)
}

pub async fn update_remote_receiver_settings(
    db: &Database,
    discovery: &DiscoveryManager,
    device: &PairedDevice,
    payload: RemoteReceiverSettingsPayload,
) -> AppResult<(RemoteReceiverSettingsPayload, Option<ReceiverTelemetry>)> {
    let sender_id = db.get_settings()?.device_id;
    let (_dev, pair) = resolve_with_rediscovery(db, discovery, device, move |dev| {
        let mut channel = open_secure_channel(
            dev,
            &sender_id,
            Duration::from_secs(4),
            Duration::from_secs(6),
        )?;
        let sig = crypto::compute_rpc_signature(
            &sender_id,
            wire_types::UPDATE_RECEIVER_SETTINGS_REQ,
            &dev.shared_secret,
        )?;
        let req = UpdateReceiverSettingsRequest {
            sender_device_id: sender_id.clone(),
            auth_signature: sig,
            settings: payload.clone(),
        };
        channel.write_envelope(
            wire_types::UPDATE_RECEIVER_SETTINGS_REQ,
            &serde_json::to_string(&req)?,
        )?;
        let env = channel.read_envelope()?;
        if env.msg_type != wire_types::UPDATE_RECEIVER_SETTINGS_RES {
            return Err(AppError::Message("Unexpected response from Pixel".to_string()));
        }
        let res: UpdateReceiverSettingsResponse = serde_json::from_str(&env.payload_json)?;
        if res.accepted {
            if let Some(s) = res.settings {
                return Ok((s, res.telemetry));
            }
        }
        Err(AppError::Message(if res.message.is_empty() {
            "Unable to update settings on Pixel".to_string()
        } else {
            res.message
        }))
    })
    .await?;
    Ok(pair)
}

pub async fn sync_ledger_with_receiver(
    db: &Database,
    discovery: &DiscoveryManager,
    device: &PairedDevice,
    items: &[TransferCandidate],
) -> AppResult<(PairedDevice, CheckLedgerBatchResponse)> {
    let settings = db.get_settings()?;
    let sender_id = settings.device_id;
    let mut active_dev = device.clone();
    let mut all_received = Vec::new();
    let mut all_cleaned = Vec::new();

    for chunk in items.chunks(500) {
        let fps: Vec<String> = chunk.iter().map(|i| i.fingerprint_key.clone()).collect();
        let sender_id_c = sender_id.clone();
        let (resolved_dev, res) = resolve_with_rediscovery(db, discovery, &active_dev, move |dev| {
            let mut channel = open_secure_channel(
                dev,
                &sender_id_c,
                Duration::from_secs(5),
                Duration::from_secs(10),
            )?;
            let sig = crypto::compute_rpc_signature(
                &sender_id_c,
                wire_types::CHECK_LEDGER_REQ,
                &dev.shared_secret,
            )?;
            let req = CheckLedgerBatchRequest {
                sender_device_id: sender_id_c.clone(),
                fingerprints: fps.clone(),
                auth_signature: sig,
            };
            channel.write_envelope(
                wire_types::CHECK_LEDGER_REQ,
                &serde_json::to_string(&req)?,
            )?;
            let env = channel.read_envelope()?;
            if env.msg_type != wire_types::CHECK_LEDGER_RES {
                return Err(AppError::Message("Unexpected ledger response".to_string()));
            }
            let res: CheckLedgerBatchResponse = serde_json::from_str(&env.payload_json)?;
            if !res.authorized {
                return Err(AppError::Message(
                    "Security verification failed. Please re-pair your Pixel.".to_string(),
                ));
            }
            Ok(res)
        })
        .await?;
        active_dev = resolved_dev;
        all_received.extend(res.already_received_fingerprints);
        all_cleaned.extend(res.cleaned_from_disk_fingerprints);
    }

    Ok((
        active_dev,
        CheckLedgerBatchResponse {
            already_received_fingerprints: all_received,
            authorized: true,
            cleaned_from_disk_fingerprints: all_cleaned,
        },
    ))
}

pub async fn send_media_items<F>(
    db: &Database,
    discovery: &DiscoveryManager,
    device: &PairedDevice,
    items: Vec<TransferCandidate>,
    cancel_flag: Arc<AtomicBool>,
    mut emit_progress: F,
) -> TransferProgressEvent
where
    F: FnMut(TransferProgressEvent) + Send,
{
    if items.is_empty() {
        let done = TransferProgressEvent::Completed {
            target_device_name: device.device_name.clone(),
            transferred_count: 0,
            skipped_duplicate_count: 0,
            total_bytes_sent: 0,
            elapsed_seconds: 0.0,
        };
        emit_progress(done.clone());
        return done;
    }

    cancel_flag.store(false, Ordering::SeqCst);
    let started = Instant::now();
    emit_progress(TransferProgressEvent::Scanning {
        message: format!(
            "Checking {} for media already backed up...",
            device.device_name
        ),
    });

    let (active_dev, ledger_res) = match sync_ledger_with_receiver(db, discovery, device, &items).await {
        Ok(res) => res,
        Err(e) => {
            let fail = TransferProgressEvent::Failed {
                friendly_message: e.to_string(),
            };
            emit_progress(fail.clone());
            return fail;
        }
    };

    let known_set: std::collections::HashSet<String> = ledger_res
        .already_received_fingerprints
        .iter()
        .cloned()
        .collect();
    let mut skipped_duplicates: usize = 0;
    let mut already_batch = Vec::new();
    let mut to_send = Vec::new();

    for item in items {
        if known_set.contains(&item.fingerprint_key) {
            skipped_duplicates += 1;
            already_batch.push((
                item.fingerprint_key,
                item.display_name,
                item.size_bytes,
                item.date_taken_epoch_ms,
            ));
        } else {
            to_send.push(item);
        }
    }

    let _ = db.mark_fingerprints_sent_batch(&already_batch, &active_dev.device_id);
    if !ledger_res.cleaned_from_disk_fingerprints.is_empty() {
        let _ = db.mark_fingerprints_cleared_batch(
            &ledger_res.cleaned_from_disk_fingerprints,
            &active_dev.device_id,
        );
    }

    if to_send.is_empty() {
        let done = TransferProgressEvent::Completed {
            target_device_name: active_dev.device_name,
            transferred_count: 0,
            skipped_duplicate_count: skipped_duplicates,
            total_bytes_sent: 0,
            elapsed_seconds: started.elapsed().as_secs_f64(),
        };
        emit_progress(done.clone());
        return done;
    }

    let settings = match db.get_settings() {
        Ok(s) => s,
        Err(e) => {
            let fail = TransferProgressEvent::Failed {
                friendly_message: e.to_string(),
            };
            emit_progress(fail.clone());
            return fail;
        }
    };

    let total_bytes: i64 = to_send.iter().map(|i| i.size_bytes).sum();
    let mut total_sent_so_far: i64 = 0;
    let mut verified_count: usize = 0;
    let total_files = to_send.len();

    for (idx, item) in to_send.into_iter().enumerate() {
        if cancel_flag.load(Ordering::SeqCst) {
            let paused = TransferProgressEvent::Paused {
                target_device_name: active_dev.device_name,
                friendly_reason: "Transfer paused by you.".to_string(),
                completed_files: verified_count,
                remaining_files: total_files - idx,
            };
            emit_progress(paused.clone());
            return paused;
        }

        let path_clone = item.file_path.clone();
        let sha_res = tokio::task::spawn_blocking(move || sha256_file(Path::new(&path_clone))).await;
        let sha256 = match sha_res {
            Ok(Ok(h)) => h,
            _ => continue,
        };

        let elapsed = started.elapsed().as_secs_f64().max(0.1);
        let speed = (total_sent_so_far as f64 / (1024.0 * 1024.0)) / elapsed;
        emit_progress(TransferProgressEvent::Active {
            target_device_name: active_dev.device_name.clone(),
            current_file_name: item.display_name.clone(),
            current_file_index: idx + 1,
            total_files,
            bytes_transferred: total_sent_so_far,
            total_bytes,
            speed_mb_per_sec: speed,
            verified_count,
            skipped_duplicate_count: skipped_duplicates,
        });

        let (tx_prog, mut rx_prog) = tokio::sync::mpsc::unbounded_channel::<i64>();
        let dev_c = active_dev.clone();
        let item_c = item.clone();
        let sha_c = sha256.clone();
        let sender_id = settings.device_id.clone();
        let sender_name = settings.device_name.clone();

        let send_task = tokio::task::spawn_blocking(move || {
            send_single_file_blocking(
                &dev_c,
                &sender_id,
                &sender_name,
                &item_c,
                &sha_c,
                |chunk_bytes| {
                    let _ = tx_prog.send(chunk_bytes as i64);
                },
            )
        });

        let mut last_emit = Instant::now();
        while let Some(delta) = rx_prog.recv().await {
            total_sent_so_far += delta;
            if last_emit.elapsed() >= Duration::from_millis(200) {
                last_emit = Instant::now();
                let sec = started.elapsed().as_secs_f64().max(0.1);
                let mbps = (total_sent_so_far as f64 / (1024.0 * 1024.0)) / sec;
                emit_progress(TransferProgressEvent::Active {
                    target_device_name: active_dev.device_name.clone(),
                    current_file_name: item.display_name.clone(),
                    current_file_index: idx + 1,
                    total_files,
                    bytes_transferred: total_sent_so_far,
                    total_bytes,
                    speed_mb_per_sec: mbps,
                    verified_count,
                    skipped_duplicate_count: skipped_duplicates,
                });
            }
        }

        let step = match send_task.await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(e)) => SingleFileOutcome::Failed(format!(
                "Connection interrupted while sending {}: {e}",
                item.display_name
            )),
            Err(e) => SingleFileOutcome::Failed(e.to_string()),
        };

        match step {
            SingleFileOutcome::Verified => {
                let _ = db.mark_file_sent(
                    &item.fingerprint_key,
                    &active_dev.device_id,
                    &sha256,
                    &item.display_name,
                    item.size_bytes,
                    item.date_taken_epoch_ms,
                );
                let _ = db.increment_device_stats(&active_dev.device_id, 1, item.size_bytes);
                verified_count += 1;
            }
            SingleFileOutcome::SkippedAlreadyHave => {
                let _ = db.mark_file_sent(
                    &item.fingerprint_key,
                    &active_dev.device_id,
                    &sha256,
                    &item.display_name,
                    item.size_bytes,
                    item.date_taken_epoch_ms,
                );
                skipped_duplicates += 1;
                total_sent_so_far += item.size_bytes;
            }
            SingleFileOutcome::ResumedOffset(offset) => {
                total_sent_so_far += offset;
            }
            SingleFileOutcome::Paused(reason) => {
                let paused = TransferProgressEvent::Paused {
                    target_device_name: active_dev.device_name,
                    friendly_reason: reason,
                    completed_files: verified_count,
                    remaining_files: total_files - idx,
                };
                emit_progress(paused.clone());
                return paused;
            }
            SingleFileOutcome::Failed(msg) => {
                let fail = TransferProgressEvent::Failed {
                    friendly_message: msg,
                };
                emit_progress(fail.clone());
                return fail;
            }
        }
    }

    let _ = db.append_activity(
        "SYNC",
        &format!(
            "Synced {} files ({} skipped) to {}",
            verified_count, skipped_duplicates, active_dev.device_name
        ),
    );
    let done = TransferProgressEvent::Completed {
        target_device_name: active_dev.device_name,
        transferred_count: verified_count,
        skipped_duplicate_count: skipped_duplicates,
        total_bytes_sent: total_sent_so_far,
        elapsed_seconds: started.elapsed().as_secs_f64().max(0.1),
    };
    emit_progress(done.clone());
    done
}

enum SingleFileOutcome {
    Verified,
    SkippedAlreadyHave,
    #[allow(dead_code)]
    ResumedOffset(i64),
    Paused(String),
    Failed(String),
}

fn send_single_file_blocking<F: FnMut(usize)>(
    device: &PairedDevice,
    sender_id: &str,
    sender_name: &str,
    item: &TransferCandidate,
    sha256: &str,
    mut on_chunk: F,
) -> AppResult<SingleFileOutcome> {
    let mut channel = open_secure_channel(
        device,
        sender_id,
        Duration::from_secs(6),
        Duration::from_secs(60),
    )?;
    let auth_sig =
        crypto::compute_file_offer_signature(sender_id, sha256, &device.shared_secret);

    let offer = FileOfferRequest {
        sender_device_id: sender_id.to_string(),
        sender_device_name: sender_name.to_string(),
        auth_signature: auth_sig,
        display_name: item.display_name.clone(),
        bucket_name: item.bucket_name.clone(),
        relative_path: item.relative_path.clone(),
        size_bytes: item.size_bytes,
        date_taken_epoch_ms: item.date_taken_epoch_ms,
        date_modified_epoch_sec: item.date_modified_epoch_sec,
        mime_type: item.mime_type.clone(),
        is_video: item.is_video,
        fingerprint_key: item.fingerprint_key.clone(),
        sha256: sha256.to_string(),
    };

    channel.write_envelope(wire_types::FILE_OFFER_REQ, &serde_json::to_string(&offer)?)?;
    let offer_env = channel.read_envelope()?;
    let offer_res: FileOfferResponse = serde_json::from_str(&offer_env.payload_json)?;

    match offer_res.decision {
        FileOfferDecision::SkipAlreadyHave => Ok(SingleFileOutcome::SkippedAlreadyHave),
        FileOfferDecision::PausedStorageFull
        | FileOfferDecision::PausedThermalHot
        | FileOfferDecision::PausedManual => Ok(SingleFileOutcome::Paused(offer_res.friendly_message)),
        FileOfferDecision::RejectedUnauthorized => {
            Ok(SingleFileOutcome::Failed(offer_res.friendly_message))
        }
        FileOfferDecision::SendBytes => {
            let resume_offset = offer_res.resume_offset_bytes.clamp(0, item.size_bytes);
            if resume_offset > 0 {
                on_chunk(resume_offset as usize);
            }
            let remaining = (item.size_bytes - resume_offset) as u64;
            let file = File::open(&item.file_path)?;
            let mut reader = BufReader::with_capacity(128 * 1024, file);
            if resume_offset > 0 {
                reader.seek(SeekFrom::Start(resume_offset as u64))?;
            }
            channel.write_encrypted_stream(&mut reader, remaining, on_chunk)?;

            let commit_env = channel.read_envelope()?;
            let commit_res: FileCommitResult = serde_json::from_str(&commit_env.payload_json)?;
            if commit_res.verified_and_saved {
                Ok(SingleFileOutcome::Verified)
            } else {
                Ok(SingleFileOutcome::Failed(commit_res.friendly_message))
            }
        }
    }
}

pub fn sha256_file(path: &Path) -> AppResult<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(128 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[allow(dead_code)]
type _UnusedWriter = BufWriter<TcpStream>;
