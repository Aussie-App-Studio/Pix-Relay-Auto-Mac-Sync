pub mod apple_photos;
pub mod cleanup;
pub mod db;
pub mod error;
pub mod firewall;
pub mod license;
pub mod protocol;
pub mod scanner;
pub mod thumbnails;
pub mod tray;
pub mod watcher;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

use crate::apple_photos::{
    detect_apple_photos_libraries as find_apple_photos_libraries, is_apple_photos_library,
    ApplePhotosLibraryInfo,
};
use crate::cleanup::{
    execute_free_up_space, preview_free_up_space, CleanupExecuteRequest, CleanupExecuteResult,
    CleanupPreview, CleanupPreviewRequest,
};
use crate::db::{ActivityEntry, Database, FolderPolicy, MacSettings, WatchedFolder};
use crate::error::{AppError, AppResult};
use crate::firewall::{check_firewall_health, FirewallHealthStatus};
use crate::license::{LicenseService, MacLicenseStatus};
use crate::protocol::client::{
    get_remote_receiver_settings, poll_telemetry, send_media_items, sync_ledger_with_receiver,
    update_remote_receiver_settings, TransferCandidate, TransferProgressEvent,
};
use crate::protocol::discovery::DiscoveryManager;
use crate::protocol::messages::{
    DiscoveredPeer, PairedDevice, PairingSessionState, ReceiverTelemetry,
    RemoteReceiverSettingsPayload,
};
use crate::protocol::pairing::{
    confirm_interactive_pairing, initiate_direct_ip_pairing, initiate_interactive_pairing,
    pair_with_code,
};
use crate::scanner::{
    group_items_by_date, scan_folder_items, scan_library_snapshot, scan_standalone_files,
    DateGroupingMode, LibrarySnapshot, MacMediaItem, MediaDateGroup, MediaSyncStatus,
};
use crate::thumbnails::ThumbnailService;
use crate::watcher::FolderWatcherHandle;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub discovery: DiscoveryManager,
    pub license: Arc<LicenseService>,
    pub thumbnails: Arc<ThumbnailService>,
    pub watcher: Arc<Mutex<Option<Arc<FolderWatcherHandle>>>>,
    pub transfer_lock: Arc<tokio::sync::Mutex<()>>,
    pub cancel_transfer: Arc<AtomicBool>,
    pub transfer_status: Arc<Mutex<TransferProgressEvent>>,
    pub last_telemetry: Arc<Mutex<Option<ReceiverTelemetry>>>,
}

impl AppState {
    pub fn primary_receiver(&self) -> AppResult<Option<PairedDevice>> {
        let settings = self.db.get_settings()?;
        let devices = self.db.list_paired_devices()?;
        if devices.is_empty() {
            return Ok(None);
        }
        if let Some(sel) = settings.selected_receiver_device_id {
            if let Some(found) = devices.iter().find(|d| d.device_id == sel) {
                return Ok(Some(found.clone()));
            }
        }
        Ok(devices.into_iter().next())
    }

    pub async fn run_transfer_batch(
        &self,
        app: &AppHandle,
        candidates: Vec<TransferCandidate>,
    ) -> AppResult<()> {
        let license = self.license.get_status(&self.db)?;
        if !license.can_sync {
            let msg = "Your 7-day free trial has ended. Enter a license key in Settings to continue syncing photos.".to_string();
            let ev = TransferProgressEvent::Failed {
                friendly_message: msg.clone(),
            };
            *self.transfer_status.lock().unwrap() = ev.clone();
            let _ = app.emit("transfer-progress", &ev);
            return Err(AppError::Message(msg));
        }

        let Some(target) = self.primary_receiver()? else {
            return Err(AppError::Message(
                "No Backup Pixel paired yet. Pair your Pixel first.".to_string(),
            ));
        };

        let Ok(_guard) = self.transfer_lock.try_lock() else {
            return Err(AppError::Message(
                "A transfer is already running.".to_string(),
            ));
        };

        let settings = self.db.get_settings()?;
        let mut sorted = candidates;
        if settings.photos_before_videos {
            sorted.sort_by(|a, b| {
                a.is_video
                    .cmp(&b.is_video)
                    .then_with(|| a.date_taken_epoch_ms.cmp(&b.date_taken_epoch_ms))
            });
        } else {
            sorted.sort_by(|a, b| a.date_taken_epoch_ms.cmp(&b.date_taken_epoch_ms));
        }

        let app_c = app.clone();
        let status_arc = self.transfer_status.clone();
        let outcome = send_media_items(
            &self.db,
            &self.discovery,
            &target,
            sorted,
            self.cancel_transfer.clone(),
            move |ev| {
                *status_arc.lock().unwrap() = ev.clone();
                let _ = app_c.emit("transfer-progress", &ev);
            },
        )
        .await;

        if let TransferProgressEvent::Completed {
            transferred_count,
            target_device_name,
            ..
        } = &outcome
        {
            if *transferred_count > 0 {
                let _ = app
                    .notification()
                    .builder()
                    .title("Pix Relay Sync Complete")
                    .body(format!(
                        "Backed up {transferred_count} {} to {target_device_name}.",
                        if *transferred_count == 1 { "item" } else { "items" }
                    ))
                    .show();
            }
        }

        let _ = app.emit("library-updated", ());
        if let Ok(tel) = poll_telemetry(&self.db, &self.discovery, &target).await {
            *self.last_telemetry.lock().unwrap() = Some(tel.clone());
            let _ = app.emit("pixel-telemetry", &tel);
        }
        Ok(())
    }

    pub async fn trigger_auto_sync(&self, app: &AppHandle) -> AppResult<()> {
        let settings = self.db.get_settings()?;
        if !settings.auto_sync_enabled {
            return Ok(());
        }
        if self.primary_receiver()?.is_none() {
            return Ok(());
        }
        let folders = self.db.list_watched_folders()?;
        let mut candidates = Vec::new();
        for folder in folders {
            if folder.policy != FolderPolicy::Auto {
                continue;
            }
            if let Ok((items, true)) = scan_folder_items(&self.db, &folder, settings.include_raw_files) {
                for item in items {
                    if item.sync_status == MediaSyncStatus::Unsent
                        && (settings.auto_send_cutoff_epoch_ms <= 0
                            || item.date_taken_epoch_ms >= settings.auto_send_cutoff_epoch_ms)
                    {
                        candidates.push(item.to_transfer_candidate());
                    }
                }
            }
        }
        if candidates.is_empty() {
            return Ok(());
        }
        self.run_transfer_batch(app, candidates).await
    }

    pub async fn sync_cleared_status(&self, app: &AppHandle) -> AppResult<usize> {
        let Some(target) = self.primary_receiver()? else {
            return Ok(0);
        };
        let (_snap, all_items) = scan_library_snapshot(&self.db)?;
        let pending_check: Vec<TransferCandidate> = all_items
            .into_iter()
            .filter(|i| i.sync_status == MediaSyncStatus::OnPixel)
            .map(|i| i.to_transfer_candidate())
            .collect();
        if pending_check.is_empty() {
            return Ok(0);
        }
        let (active_dev, res) =
            sync_ledger_with_receiver(&self.db, &self.discovery, &target, &pending_check).await?;
        let count = res.cleaned_from_disk_fingerprints.len();
        if count > 0 {
            self.db.mark_fingerprints_cleared_batch(
                &res.cleaned_from_disk_fingerprints,
                &active_dev.device_id,
            )?;
            let _ = app.emit("library-updated", ());
        }
        Ok(count)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DropActionSummary {
    added_folders: usize,
    queued_files: usize,
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> AppResult<MacSettings> {
    state.db.get_settings()
}

#[tauri::command]
fn update_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    settings: MacSettings,
) -> AppResult<MacSettings> {
    state.db.update_settings(&settings)?;
    if settings.start_at_login {
        let _ = app.autolaunch().enable();
    } else {
        let _ = app.autolaunch().disable();
    }
    let _ = app.emit("library-updated", ());
    let updated = state.db.get_settings()?;
    if updated.auto_sync_enabled {
        let st = state.inner().clone();
        let app_c = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = st.trigger_auto_sync(&app_c).await;
        });
    }
    Ok(updated)
}

#[tauri::command]
async fn get_library_snapshot(state: tauri::State<'_, AppState>) -> AppResult<LibrarySnapshot> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || scan_library_snapshot(&db).map(|(snap, _)| snap))
        .await
        .map_err(|e| AppError::Message(e.to_string()))?
}

#[tauri::command]
async fn detect_apple_photos_libraries(
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<ApplePhotosLibraryInfo>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || find_apple_photos_libraries(&db))
        .await
        .map_err(|e| AppError::Message(e.to_string()))
}

#[tauri::command]
async fn add_apple_photos_library(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
) -> AppResult<WatchedFolder> {
    let p = Path::new(&path);
    if !p.exists() || !p.is_dir() || !is_apple_photos_library(p) {
        return Err(AppError::Message(
            "Selected bundle is not a valid Apple Photos Library (.photoslibrary).".to_string(),
        ));
    }
    let raw_name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Photos Library".to_string());
    let display_name = raw_name
        .strip_suffix(".photoslibrary")
        .or_else(|| raw_name.strip_suffix(".PHOTOSLIBRARY"))
        .unwrap_or(&raw_name)
        .to_string();

    let folder = state
        .db
        .add_watched_folder(&path, &display_name, FolderPolicy::Auto, true)?;
    if let Some(ref w) = *state.watcher.lock().unwrap() {
        w.reload_watches(&state.db);
    }
    let _ = state
        .db
        .append_activity("FOLDER", &format!("Connected Apple Photos Library {}", folder.display_name));
    let _ = app.emit("library-updated", ());

    let st = state.inner().clone();
    let app_c = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = st.trigger_auto_sync(&app_c).await;
    });
    Ok(folder)
}

#[tauri::command]
async fn get_folder_media(
    state: tauri::State<'_, AppState>,
    folder_id: Option<String>,
    grouping: DateGroupingMode,
    filter: String,
    hide_uploaded: bool,
) -> AppResult<Vec<MediaDateGroup>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let settings = db.get_settings()?;
        let mut items: Vec<MacMediaItem> = if let Some(fid) = folder_id {
            let folder = db
                .list_watched_folders()?
                .into_iter()
                .find(|f| f.folder_id == fid)
                .ok_or_else(|| AppError::Message("Folder not found".to_string()))?;
            let (folder_items, _) = scan_folder_items(&db, &folder, settings.include_raw_files)?;
            folder_items
        } else {
            scan_library_snapshot(&db)?.1
        };

        items.retain(|i| {
            if hide_uploaded && i.sync_status != MediaSyncStatus::Unsent {
                return false;
            }
            match filter.as_str() {
                "UNSENT" => i.sync_status == MediaSyncStatus::Unsent,
                "ON_PIXEL" => i.sync_status == MediaSyncStatus::OnPixel,
                "CLEARED" => i.sync_status == MediaSyncStatus::ClearedByPixel,
                "VIDEOS" => i.is_video,
                _ => true,
            }
        });

        Ok(group_items_by_date(items, grouping))
    })
    .await
    .map_err(|e| AppError::Message(e.to_string()))?
}

#[tauri::command]
async fn add_watched_folder(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
    policy: Option<FolderPolicy>,
    recursive: Option<bool>,
) -> AppResult<WatchedFolder> {
    let p = Path::new(&path);
    if !p.exists() || !p.is_dir() {
        return Err(AppError::Message("Selected path is not a folder.".to_string()));
    }
    let raw_name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.clone());
    let name = if is_apple_photos_library(p) {
        raw_name
            .strip_suffix(".photoslibrary")
            .or_else(|| raw_name.strip_suffix(".PHOTOSLIBRARY"))
            .unwrap_or(&raw_name)
            .to_string()
    } else {
        raw_name
    };
    let folder = state.db.add_watched_folder(
        &path,
        &name,
        policy.unwrap_or(FolderPolicy::Auto),
        recursive.unwrap_or(true),
    )?;
    if let Some(ref w) = *state.watcher.lock().unwrap() {
        w.reload_watches(&state.db);
    }
    let _ = state.db.append_activity("FOLDER", &format!("Added folder {}", folder.display_name));
    let _ = app.emit("library-updated", ());
    if folder.policy == FolderPolicy::Auto {
        let st = state.inner().clone();
        let app_c = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = st.trigger_auto_sync(&app_c).await;
        });
    }
    Ok(folder)
}

#[tauri::command]
fn update_folder_policy(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    folder_id: String,
    policy: FolderPolicy,
) -> AppResult<()> {
    state.db.update_folder_policy(&folder_id, policy)?;
    if let Some(ref w) = *state.watcher.lock().unwrap() {
        w.reload_watches(&state.db);
    }
    let _ = app.emit("library-updated", ());
    if policy == FolderPolicy::Auto {
        let st = state.inner().clone();
        let app_c = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = st.trigger_auto_sync(&app_c).await;
        });
    }
    Ok(())
}

#[tauri::command]
fn update_folder_recursive(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    folder_id: String,
    recursive: bool,
) -> AppResult<()> {
    state.db.update_folder_recursive(&folder_id, recursive)?;
    if let Some(ref w) = *state.watcher.lock().unwrap() {
        w.reload_watches(&state.db);
    }
    let _ = app.emit("library-updated", ());
    Ok(())
}

#[tauri::command]
fn remove_watched_folder(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    folder_id: String,
) -> AppResult<()> {
    state.db.remove_watched_folder(&folder_id)?;
    if let Some(ref w) = *state.watcher.lock().unwrap() {
        w.reload_watches(&state.db);
    }
    let _ = app.emit("library-updated", ());
    Ok(())
}

#[tauri::command]
fn get_discovered_peers(state: tauri::State<'_, AppState>) -> Vec<DiscoveredPeer> {
    state.discovery.current_peers()
}

#[tauri::command]
fn get_paired_devices(state: tauri::State<'_, AppState>) -> AppResult<Vec<PairedDevice>> {
    state.db.list_paired_devices()
}

#[tauri::command]
fn set_selected_receiver(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    device_id: Option<String>,
) -> AppResult<()> {
    state.db.set_selected_receiver(device_id.as_deref())?;
    let _ = app.emit("library-updated", ());
    Ok(())
}

#[tauri::command]
fn remove_paired_device(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    device_id: String,
) -> AppResult<()> {
    state.db.remove_paired_device(&device_id)?;
    *state.last_telemetry.lock().unwrap() = None;
    let _ = app.emit("library-updated", ());
    Ok(())
}

#[tauri::command]
async fn start_interactive_pairing(
    state: tauri::State<'_, AppState>,
    peer: DiscoveredPeer,
) -> AppResult<PairingSessionState> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || initiate_interactive_pairing(&db, &peer))
        .await
        .map_err(|e| AppError::Message(e.to_string()))?
}

#[tauri::command]
async fn start_direct_ip_pairing(
    state: tauri::State<'_, AppState>,
    ip_address: String,
    port: Option<i32>,
) -> AppResult<PairingSessionState> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || initiate_direct_ip_pairing(&db, &ip_address, port))
        .await
        .map_err(|e| AppError::Message(e.to_string()))?
}

#[tauri::command]
async fn complete_interactive_pairing(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session: PairingSessionState,
) -> AppResult<PairedDevice> {
    let db = state.db.clone();
    let paired = tokio::task::spawn_blocking(move || confirm_interactive_pairing(&db, &session))
        .await
        .map_err(|e| AppError::Message(e.to_string()))??;
    let _ = app.emit("library-updated", ());
    Ok(paired)
}

#[tauri::command]
async fn pair_pixel_with_code(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    code: String,
) -> AppResult<PairedDevice> {
    let db = state.db.clone();
    let paired = tokio::task::spawn_blocking(move || pair_with_code(&db, &code))
        .await
        .map_err(|e| AppError::Message(e.to_string()))??;
    let _ = app.emit("library-updated", ());
    Ok(paired)
}

#[tauri::command]
async fn poll_pixel_telemetry(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Option<ReceiverTelemetry>> {
    let Some(target) = state.primary_receiver()? else {
        *state.last_telemetry.lock().unwrap() = None;
        return Ok(None);
    };
    match poll_telemetry(&state.db, &state.discovery, &target).await {
        Ok(tel) => {
            *state.last_telemetry.lock().unwrap() = Some(tel.clone());
            let _ = app.emit("pixel-telemetry", &tel);
            Ok(Some(tel))
        }
        Err(_) => Ok(state.last_telemetry.lock().unwrap().clone()),
    }
}

#[tauri::command]
async fn fetch_remote_pixel_settings(
    state: tauri::State<'_, AppState>,
    device_id: String,
) -> AppResult<RemoteReceiverSettingsPayload> {
    let dev = state
        .db
        .get_paired_device(&device_id)?
        .ok_or_else(|| AppError::Message("Paired Pixel not found".to_string()))?;
    let (settings, tel) = get_remote_receiver_settings(&state.db, &state.discovery, &dev).await?;
    if let Some(t) = tel {
        *state.last_telemetry.lock().unwrap() = Some(t);
    }
    Ok(settings)
}

#[tauri::command]
async fn push_remote_pixel_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    device_id: String,
    payload: RemoteReceiverSettingsPayload,
) -> AppResult<RemoteReceiverSettingsPayload> {
    let dev = state
        .db
        .get_paired_device(&device_id)?
        .ok_or_else(|| AppError::Message("Paired Pixel not found".to_string()))?;
    let (updated, tel) =
        update_remote_receiver_settings(&state.db, &state.discovery, &dev, payload).await?;
    if let Some(t) = tel {
        *state.last_telemetry.lock().unwrap() = Some(t.clone());
        let _ = app.emit("pixel-telemetry", &t);
    }
    Ok(updated)
}

#[tauri::command]
async fn sync_cleared_status_now(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<usize> {
    state.sync_cleared_status(&app).await
}

#[tauri::command]
async fn send_selected_files(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    file_paths: Vec<String>,
) -> AppResult<()> {
    let db = state.db.clone();
    let items = tokio::task::spawn_blocking(move || scan_standalone_files(&db, &file_paths))
        .await
        .map_err(|e| AppError::Message(e.to_string()))??;
    let candidates: Vec<TransferCandidate> =
        items.into_iter().map(|i| i.to_transfer_candidate()).collect();
    let app_c = app.clone();
    let st = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = st.run_transfer_batch(&app_c, candidates).await;
    });
    Ok(())
}

#[tauri::command]
async fn send_folder_unsent(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    folder_id: String,
) -> AppResult<()> {
    let db = state.db.clone();
    let candidates = tokio::task::spawn_blocking(move || -> AppResult<Vec<TransferCandidate>> {
        let settings = db.get_settings()?;
        let folder = db
            .list_watched_folders()?
            .into_iter()
            .find(|f| f.folder_id == folder_id)
            .ok_or_else(|| AppError::Message("Folder not found".to_string()))?;
        let (items, _) = scan_folder_items(&db, &folder, settings.include_raw_files)?;
        Ok(items
            .into_iter()
            .filter(|i| i.sync_status == MediaSyncStatus::Unsent)
            .map(|i| i.to_transfer_candidate())
            .collect())
    })
    .await
    .map_err(|e| AppError::Message(e.to_string()))??;

    let app_c = app.clone();
    let st = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = st.run_transfer_batch(&app_c, candidates).await;
    });
    Ok(())
}

#[tauri::command]
async fn send_all_auto_unsent(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let app_c = app.clone();
    let st = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = st.trigger_auto_sync(&app_c).await;
    });
    Ok(())
}

#[tauri::command]
async fn handle_dropped_paths(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> AppResult<DropActionSummary> {
    let mut added_folders = 0usize;
    let mut file_paths = Vec::new();
    for raw in paths {
        let p = Path::new(&raw);
        if p.is_dir() {
            let raw_name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| raw.clone());
            let name = if is_apple_photos_library(p) {
                raw_name
                    .strip_suffix(".photoslibrary")
                    .or_else(|| raw_name.strip_suffix(".PHOTOSLIBRARY"))
                    .unwrap_or(&raw_name)
                    .to_string()
            } else {
                raw_name
            };
            if state
                .db
                .add_watched_folder(&raw, &name, FolderPolicy::Auto, true)
                .is_ok()
            {
                added_folders += 1;
            }
        } else if p.is_file() {
            file_paths.push(raw);
        }
    }
    if added_folders > 0 {
        if let Some(ref w) = *state.watcher.lock().unwrap() {
            w.reload_watches(&state.db);
        }
        let _ = app.emit("library-updated", ());
    }
    let queued_files = file_paths.len();
    if !file_paths.is_empty() {
        let db = state.db.clone();
        let items = tokio::task::spawn_blocking(move || scan_standalone_files(&db, &file_paths))
            .await
            .map_err(|e| AppError::Message(e.to_string()))??;
        let candidates: Vec<TransferCandidate> =
            items.into_iter().map(|i| i.to_transfer_candidate()).collect();
        if !candidates.is_empty() {
            let app_c = app.clone();
            let st = state.inner().clone();
            tauri::async_runtime::spawn(async move {
                let _ = st.run_transfer_batch(&app_c, candidates).await;
            });
        }
    }
    Ok(DropActionSummary {
        added_folders,
        queued_files,
    })
}

#[tauri::command]
fn cancel_transfer(state: tauri::State<'_, AppState>) {
    state.cancel_transfer.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn get_transfer_status(state: tauri::State<'_, AppState>) -> TransferProgressEvent {
    state.transfer_status.lock().unwrap().clone()
}

#[tauri::command]
async fn preview_cleanup(
    state: tauri::State<'_, AppState>,
    request: CleanupPreviewRequest,
) -> AppResult<CleanupPreview> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || preview_free_up_space(&db, &request))
        .await
        .map_err(|e| AppError::Message(e.to_string()))?
}

#[tauri::command]
async fn execute_cleanup(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: CleanupExecuteRequest,
) -> AppResult<CleanupExecuteResult> {
    let db = state.db.clone();
    let res = tokio::task::spawn_blocking(move || execute_free_up_space(&db, &request))
        .await
        .map_err(|e| AppError::Message(e.to_string()))??;
    let _ = app.emit("library-updated", ());
    Ok(res)
}

#[tauri::command]
async fn get_firewall_health(state: tauri::State<'_, AppState>) -> AppResult<FirewallHealthStatus> {
    let disc = state.discovery.clone();
    tokio::task::spawn_blocking(move || check_firewall_health(&disc))
        .await
        .map_err(|e| AppError::Message(e.to_string()))
}

#[tauri::command]
fn get_license_status(state: tauri::State<'_, AppState>) -> AppResult<MacLicenseStatus> {
    state.license.get_status(&state.db)
}

#[tauri::command]
fn activate_license_key(
    state: tauri::State<'_, AppState>,
    key: String,
) -> AppResult<MacLicenseStatus> {
    state.license.activate_license_key(&state.db, &key)
}

#[tauri::command]
fn clear_license_key(state: tauri::State<'_, AppState>) -> AppResult<MacLicenseStatus> {
    state.license.clear_license_key(&state.db)
}

#[tauri::command]
fn set_dev_license_override(
    state: tauri::State<'_, AppState>,
    mode: Option<String>,
) -> AppResult<MacLicenseStatus> {
    state.license.set_dev_override(mode);
    state.license.get_status(&state.db)
}

#[tauri::command]
fn list_activity_log(state: tauri::State<'_, AppState>) -> AppResult<Vec<ActivityEntry>> {
    state.db.list_recent_activity(40)
}

#[tauri::command]
fn open_macos_settings<R: Runtime>(app: AppHandle<R>, pane: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let url = match pane.as_str() {
        "wifi" => "x-apple.systempreferences:com.apple.wifi-settings-extension",
        "firewall" => "x-apple.systempreferences:com.apple.Network-Settings.extension?Firewall",
        "local-network" => "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_LocalNetwork",
        "full-disk-access" => "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AllFiles",
        "photos" => "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Photos",
        _ => "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension",
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| AppError::Message(e.to_string()))
}

#[tauri::command]
fn open_website_url<R: Runtime>(app: AppHandle<R>) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(crate::license::WEBSITE_URL, None::<&str>)
        .map_err(|e| AppError::Message(e.to_string()))
}

fn spawn_background_loops(app: AppHandle, state: AppState) {
    // 1. Live discovered peers relay to open frontend windows
    let app_peers = app.clone();
    let mut peer_rx = state.discovery.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Ok(peers) = peer_rx.recv().await {
            let _ = app_peers.emit("peers-updated", &peers);
        }
    });

    // 2. Folder watcher for AUTO folders (FSEvents on macOS)
    let app_watch = app.clone();
    let state_watch = state.clone();
    let handle = FolderWatcherHandle::start(state.db.clone(), move || {
        let _ = app_watch.emit("library-updated", ());
        let a = app_watch.clone();
        let s = state_watch.clone();
        tauri::async_runtime::spawn(async move {
            let _ = s.trigger_auto_sync(&a).await;
        });
    });
    *state.watcher.lock().unwrap() = Some(handle);

    // 3. Periodic telemetry & cleared-by-Pixel sync
    //    Slows down to 60s when window is closed to menu bar so idle CPU/battery stays ~0%
    let app_bg = app.clone();
    let state_bg = state.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick: u64 = 0;
        loop {
            let window_open = app_bg.get_webview_window("main").is_some();
            let interval = if window_open {
                Duration::from_secs(15)
            } else {
                Duration::from_secs(60)
            };
            tokio::time::sleep(interval).await;
            tick += 1;

            if let Ok(Some(target)) = state_bg.primary_receiver() {
                if let Ok(tel) = poll_telemetry(&state_bg.db, &state_bg.discovery, &target).await {
                    *state_bg.last_telemetry.lock().unwrap() = Some(tel.clone());
                    let _ = app_bg.emit("pixel-telemetry", &tel);
                }
                if tick % 5 == 0 {
                    let _ = state_bg.sync_cleared_status(&app_bg).await;
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db = Database::open_default().expect("Failed to open Pix Relay Mac SQLite database");
    let discovery = DiscoveryManager::new(db.clone());
    discovery.start();

    let thumbnails = Arc::new(ThumbnailService::new(db.clone()));
    let thumbs_proto = thumbnails.clone();

    let state = AppState {
        db,
        discovery,
        license: Arc::new(LicenseService::new()),
        thumbnails,
        watcher: Arc::new(Mutex::new(None)),
        transfer_lock: Arc::new(tokio::sync::Mutex::new(())),
        cancel_transfer: Arc::new(AtomicBool::new(false)),
        transfer_status: Arc::new(Mutex::new(TransferProgressEvent::Idle)),
        last_telemetry: Arc::new(Mutex::new(None)),
    };

    let launched_minimized = std::env::args().any(|a| a == "--minimized");
    let state_for_setup = state.clone();

    let app = tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .register_uri_scheme_protocol("pixthumb", move |_ctx, request| {
            let (status, mime, body) = thumbs_proto.handle_protocol_uri(&request.uri().to_string());
            tauri::http::Response::builder()
                .status(status)
                .header("Content-Type", mime)
                .header("Cache-Control", "max-age=3600")
                .body(body)
                .unwrap()
        })
        .setup(move |app| {
            tray::setup_tray(app.handle())?;
            if launched_minimized {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.destroy();
                }
                #[cfg(target_os = "macos")]
                {
                    let _ = app.handle().set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
            spawn_background_loops(app.handle().clone(), state_for_setup);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                let minimize = state
                    .db
                    .get_settings()
                    .map(|s| s.minimize_to_tray)
                    .unwrap_or(true);
                if minimize {
                    api.prevent_close();
                    // Destroy WKWebView completely so background idle RAM stays ~10-15 MB
                    let _ = window.destroy();
                    #[cfg(target_os = "macos")]
                    {
                        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                    }
                } else {
                    app.exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            get_library_snapshot,
            detect_apple_photos_libraries,
            add_apple_photos_library,
            get_folder_media,
            add_watched_folder,
            update_folder_policy,
            update_folder_recursive,
            remove_watched_folder,
            get_discovered_peers,
            get_paired_devices,
            set_selected_receiver,
            remove_paired_device,
            start_interactive_pairing,
            start_direct_ip_pairing,
            complete_interactive_pairing,
            pair_pixel_with_code,
            poll_pixel_telemetry,
            fetch_remote_pixel_settings,
            push_remote_pixel_settings,
            sync_cleared_status_now,
            send_selected_files,
            send_folder_unsent,
            send_all_auto_unsent,
            handle_dropped_paths,
            cancel_transfer,
            get_transfer_status,
            preview_cleanup,
            execute_cleanup,
            get_firewall_health,
            get_license_status,
            activate_license_key,
            clear_license_key,
            set_dev_license_override,
            list_activity_log,
            open_macos_settings,
            open_website_url,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Pix Relay: Auto Mac Sync");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            if code.is_none() {
                api.prevent_exit();
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen {
            has_visible_windows,
            ..
        } => {
            if !has_visible_windows {
                tray::show_or_recreate_main_window(app_handle);
            }
        }
        _ => {
            let _ = app_handle;
        }
    });
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::io::{BufReader, BufWriter};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use sha2::{Digest, Sha256};

    use crate::db::{build_fingerprint_key, Database, FolderPolicy};
    use crate::protocol::client::{send_media_items, sync_ledger_with_receiver, TransferProgressEvent};
    use crate::protocol::crypto;
    use crate::protocol::discovery::DiscoveryManager;
    use crate::protocol::messages::{
        wire_types, CheckLedgerBatchRequest, CheckLedgerBatchResponse, DeviceRole,
        FileCommitResult, FileOfferDecision, FileOfferRequest, FileOfferResponse, PairInitRequest,
        PairInitResponse, QrPairingPayload, ReceiverTelemetry, SecureHelloRequest,
        SecureHelloResponse, TelemetryResponse,
    };
    use crate::protocol::pairing::pair_with_code;
    use crate::protocol::wire::{read_plain_envelope, write_plain_envelope, SecureWireChannel};
    use crate::scanner::scan_folder_items;

    #[tokio::test]
    async fn full_loopback_pairing_ledger_cleared_resume_and_transfer() {
        let tmp = std::env::temp_dir().join(format!("pixrelay_e2e_{}", uuid::Uuid::new_v4()));
        let photos_dir = tmp.join("CameraRoll");
        std::fs::create_dir_all(&photos_dir).unwrap();

        let photo1_bytes = b"JPEG_MAC_TEST_PHOTO_1".to_vec();
        let photo2_bytes = vec![0x7Bu8; 280 * 1024];
        let p1 = photos_dir.join("IMG_1001.jpg");
        let p2 = photos_dir.join("VID_1002.mp4");
        std::fs::write(&p1, &photo1_bytes).unwrap();
        std::fs::write(&p2, &photo2_bytes).unwrap();

        let db = Database::open_path(&tmp.join("mac.db")).unwrap();
        let folder = db
            .add_watched_folder(
                &photos_dir.to_string_lossy(),
                "CameraRoll",
                FolderPolicy::Auto,
                true,
            )
            .unwrap();
        let (items, _) = scan_folder_items(&db, &folder, false).unwrap();
        assert_eq!(items.len(), 2);

        let rx_secret_store = Arc::new(Mutex::new(String::new()));
        let rx_received_fps = Arc::new(Mutex::new(HashSet::<String>::new()));
        let rx_cleaned_fps = Arc::new(Mutex::new(HashSet::<String>::new()));
        let rx_saved_payloads = Arc::new(Mutex::new(Vec::<(String, Vec<u8>)>::new()));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let rx_port = listener.local_addr().unwrap().port() as i32;
        let qr_pin = "test-qr-pin-123".to_string();

        let rx_sec_c = rx_secret_store.clone();
        let rx_rec_c = rx_received_fps.clone();
        let rx_clr_c = rx_cleaned_fps.clone();
        let rx_pay_c = rx_saved_payloads.clone();
        let qr_pin_c = qr_pin.clone();

        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = BufWriter::new(stream);
                let Ok(env) = read_plain_envelope(&mut reader) else { continue };
                match env.msg_type.as_str() {
                    wire_types::PAIR_INIT_REQ => {
                        let req: PairInitRequest = serde_json::from_str(&env.payload_json).unwrap();
                        let (sk, pk) = crypto::generate_ecdh_keypair();
                        let rx_pub = crypto::encode_public_key_spki_b64(&pk).unwrap();
                        let ecdh = crypto::compute_ecdh_shared_secret(&sk, &req.sender_ecdh_public_key_b64).unwrap();
                        let shared = crypto::derive_pairing_shared_secret(
                            &ecdh,
                            &req.sender_device_id,
                            "pixel-mock",
                            &req.session_nonce,
                            Some(&qr_pin_c),
                        )
                        .unwrap();
                        *rx_sec_c.lock().unwrap() = shared;
                        let s_proof = crypto::compute_qr_server_proof(
                            &qr_pin_c,
                            "pixel-mock",
                            &req.sender_device_id,
                            &req.session_nonce,
                            &rx_pub,
                        )
                        .unwrap();
                        let res = PairInitResponse {
                            accepted: true,
                            receiver_device_id: "pixel-mock".into(),
                            receiver_device_name: "Mock OG Pixel".into(),
                            receiver_model: "Pixel XL".into(),
                            receiver_role: DeviceRole::Receiver,
                            verification_code_6_digit: "123 456".into(),
                            receiver_ecdh_public_key_b64: rx_pub,
                            qr_server_proof: Some(s_proof),
                            instant_paired_via_qr: true,
                            message: "OK".into(),
                        };
                        write_plain_envelope(
                            &mut writer,
                            wire_types::PAIR_INIT_RES,
                            &serde_json::to_string(&res).unwrap(),
                        )
                        .unwrap();
                    }
                    wire_types::SECURE_HELLO_REQ => {
                        let hello: SecureHelloRequest = serde_json::from_str(&env.payload_json).unwrap();
                        let secret = rx_sec_c.lock().unwrap().clone();
                        let c_nonce = URL_SAFE_NO_PAD.decode(&hello.client_nonce_b64).unwrap();
                        let s_nonce = crypto::generate_random_bytes(16);
                        let s_nonce_b64 = URL_SAFE_NO_PAD.encode(&s_nonce);
                        let sess_key = crypto::derive_session_key(&secret, &c_nonce, &s_nonce).unwrap();
                        let s_proof = crypto::compute_server_hello_proof(
                            &sess_key,
                            &hello.sender_device_id,
                            &hello.client_nonce_b64,
                            &s_nonce_b64,
                        )
                        .unwrap();
                        let hello_res = SecureHelloResponse {
                            accepted: true,
                            server_nonce_b64: s_nonce_b64,
                            server_proof: s_proof,
                            message: "OK".into(),
                        };
                        write_plain_envelope(
                            &mut writer,
                            wire_types::SECURE_HELLO_RES,
                            &serde_json::to_string(&hello_res).unwrap(),
                        )
                        .unwrap();

                        let inner_stream = writer.into_inner().unwrap();
                        let mut channel = SecureWireChannel::new(inner_stream, &sess_key, false);
                        let inner = channel.read_envelope().unwrap();
                        match inner.msg_type.as_str() {
                            wire_types::TELEMETRY_REQ => {
                                let tel = ReceiverTelemetry {
                                    device_id: "pixel-mock".into(),
                                    device_name: "Mock OG Pixel".into(),
                                    model_name: "Pixel XL".into(),
                                    pixel_benefit_summary: "Original Pixel".into(),
                                    free_bytes: 20_000_000_000,
                                    total_bytes: 128_000_000_000,
                                    reserve_storage_gb: 3,
                                    battery_percent: 90,
                                    is_charging: true,
                                    temperature_celsius: 31.5,
                                    max_safe_temp_celsius: 40.0,
                                    is_thermal_paused: false,
                                    is_storage_paused: false,
                                    is_screen_off_ready: true,
                                    outbound_upload_bytes_per_sec: 0,
                                    friendly_cloud_status: "Ready".into(),
                                    historical_received_count: 0,
                                    historical_received_bytes: 0,
                                    staged_files_on_disk_count: 0,
                                    timestamp_epoch_ms: 0,
                                    is_manual_paused: false,
                                };
                                channel
                                    .write_envelope(
                                        wire_types::TELEMETRY_RES,
                                        &serde_json::to_string(&TelemetryResponse { telemetry: tel }).unwrap(),
                                    )
                                    .unwrap();
                            }
                            wire_types::CHECK_LEDGER_REQ => {
                                let req: CheckLedgerBatchRequest = serde_json::from_str(&inner.payload_json).unwrap();
                                let rec = rx_rec_c.lock().unwrap();
                                let clr = rx_clr_c.lock().unwrap();
                                let already: Vec<String> = req
                                    .fingerprints
                                    .iter()
                                    .filter(|f| rec.contains(*f))
                                    .cloned()
                                    .collect();
                                let cleaned: Vec<String> = req
                                    .fingerprints
                                    .iter()
                                    .filter(|f| clr.contains(*f))
                                    .cloned()
                                    .collect();
                                channel
                                    .write_envelope(
                                        wire_types::CHECK_LEDGER_RES,
                                        &serde_json::to_string(&CheckLedgerBatchResponse {
                                            already_received_fingerprints: already,
                                            authorized: true,
                                            cleaned_from_disk_fingerprints: cleaned,
                                        })
                                        .unwrap(),
                                    )
                                    .unwrap();
                            }
                            wire_types::FILE_OFFER_REQ => {
                                let offer: FileOfferRequest = serde_json::from_str(&inner.payload_json).unwrap();
                                let expected_sig = crypto::compute_file_offer_signature(
                                    &offer.sender_device_id,
                                    &offer.sha256,
                                    &secret,
                                );
                                assert!(crypto::verify_signature(&expected_sig, &offer.auth_signature));

                                channel
                                    .write_envelope(
                                        wire_types::FILE_OFFER_RES,
                                        &serde_json::to_string(&FileOfferResponse {
                                            decision: FileOfferDecision::SendBytes,
                                            friendly_message: "Ready".into(),
                                            resume_offset_bytes: 0,
                                        })
                                        .unwrap(),
                                    )
                                    .unwrap();

                                let mut buf = Vec::new();
                                channel
                                    .read_encrypted_stream(offer.size_bytes as u64, |chunk| {
                                        buf.extend_from_slice(chunk);
                                        Ok(())
                                    })
                                    .unwrap();
                                let mut h = Sha256::new();
                                h.update(&buf);
                                let got_sha = hex::encode(h.finalize());
                                assert_eq!(got_sha, offer.sha256);

                                rx_rec_c.lock().unwrap().insert(offer.fingerprint_key.clone());
                                rx_pay_c.lock().unwrap().push((offer.display_name.clone(), buf));

                                channel
                                    .write_envelope(
                                        wire_types::FILE_COMMIT_RES,
                                        &serde_json::to_string(&FileCommitResult {
                                            verified_and_saved: true,
                                            sha256: got_sha,
                                            saved_display_path: format!("DCIM/Camera/{}", offer.display_name),
                                            friendly_message: "Saved".into(),
                                        })
                                        .unwrap(),
                                    )
                                    .unwrap();
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        });

        let payload = QrPairingPayload {
            device_id: "pixel-mock".into(),
            device_name: "Mock OG Pixel".into(),
            model_name: "Pixel XL".into(),
            role: DeviceRole::Receiver,
            ip_address: "127.0.0.1".into(),
            port: rx_port,
            pin_token: qr_pin,
        };
        let code = format!(
            "PIX1:{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).unwrap().as_bytes())
        );
        let paired = pair_with_code(&db, &code).unwrap();
        assert_eq!(paired.shared_secret.len(), 64);

        let discovery = DiscoveryManager::new(db.clone());
        let candidates: Vec<_> = items.iter().map(|i| i.to_transfer_candidate()).collect();
        let cancel = Arc::new(AtomicBool::new(false));
        let outcome = send_media_items(&db, &discovery, &paired, candidates.clone(), cancel, |_| {}).await;
        match outcome {
            TransferProgressEvent::Completed {
                transferred_count,
                skipped_duplicate_count,
                ..
            } => {
                assert_eq!(transferred_count, 2);
                assert_eq!(skipped_duplicate_count, 0);
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
        assert_eq!(rx_saved_payloads.lock().unwrap().len(), 2);

        let img1_fp = build_fingerprint_key(
            "IMG_1001.jpg",
            photo1_bytes.len() as i64,
            items
                .iter()
                .find(|i| i.display_name == "IMG_1001.jpg")
                .unwrap()
                .date_taken_epoch_ms,
        );
        rx_cleaned_fps.lock().unwrap().insert(img1_fp.clone());

        let (_, ledger_res) = sync_ledger_with_receiver(&db, &discovery, &paired, &candidates).await.unwrap();
        assert_eq!(ledger_res.already_received_fingerprints.len(), 2);
        assert_eq!(ledger_res.cleaned_from_disk_fingerprints, vec![img1_fp.clone()]);

        db.mark_fingerprints_cleared_batch(&ledger_res.cleaned_from_disk_fingerprints, &paired.device_id)
            .unwrap();
        let (sent_set, cleared_set) = db.get_sent_and_cleared_sets().unwrap();
        assert_eq!(sent_set.len(), 2);
        assert!(cleared_set.contains(&img1_fp));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
