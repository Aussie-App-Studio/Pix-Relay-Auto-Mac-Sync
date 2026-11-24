use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppResult;
use crate::protocol::messages::{DeviceRole, PairedDevice, SaveLocationMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
    Matrix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FolderPolicy {
    Auto,
    Choose,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CleanupActionMode {
    Trash,
    ArchiveFolder,
    Permanent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacSettings {
    pub device_id: String,
    pub device_name: String,
    pub theme_mode: ThemeMode,
    pub auto_sync_enabled: bool,
    pub auto_send_cutoff_epoch_ms: i64,
    pub photos_before_videos: bool,
    pub include_raw_files: bool,
    pub minimize_to_tray: bool,
    pub start_at_login: bool,
    pub selected_receiver_device_id: Option<String>,
    pub default_cleanup_mode: CleanupActionMode,
    pub archive_folder_path: Option<String>,
    pub license_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    pub folder_id: String,
    pub path: String,
    pub display_name: String,
    pub policy: FolderPolicy,
    pub recursive: bool,
    pub added_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: i64,
    pub timestamp_ms: i64,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SentMeta {
    pub sent_at_ms: i64,
    pub cleared_by_pixel: bool,
}

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open_default() -> AppResult<Self> {
        let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = base.join("Pix Relay Auto Mac Sync");
        std::fs::create_dir_all(&dir)?;
        Self::open_path(&dir.join("pix_relay_mac.db"))
    }

    pub fn open_path(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            ",
        )?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                device_id TEXT NOT NULL,
                device_name TEXT NOT NULL,
                theme_mode TEXT NOT NULL DEFAULT 'DARK',
                auto_sync_enabled INTEGER NOT NULL DEFAULT 1,
                auto_send_cutoff_epoch_ms INTEGER NOT NULL DEFAULT 0,
                photos_before_videos INTEGER NOT NULL DEFAULT 1,
                include_raw_files INTEGER NOT NULL DEFAULT 0,
                minimize_to_tray INTEGER NOT NULL DEFAULT 1,
                start_at_login INTEGER NOT NULL DEFAULT 0,
                selected_receiver_device_id TEXT,
                default_cleanup_mode TEXT NOT NULL DEFAULT 'TRASH',
                archive_folder_path TEXT,
                trial_started_at_ms INTEGER NOT NULL DEFAULT 0,
                license_key TEXT
            );

            CREATE TABLE IF NOT EXISTS paired_devices (
                device_id TEXT PRIMARY KEY,
                device_name TEXT NOT NULL,
                model_name TEXT NOT NULL,
                role TEXT NOT NULL,
                last_ip TEXT NOT NULL,
                last_port INTEGER NOT NULL,
                shared_secret TEXT NOT NULL,
                paired_at_ms INTEGER NOT NULL,
                last_seen_ms INTEGER NOT NULL,
                save_location_override TEXT,
                total_files_transferred INTEGER NOT NULL DEFAULT 0,
                total_bytes_transferred INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS watched_folders (
                folder_id TEXT PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                display_name TEXT NOT NULL,
                policy TEXT NOT NULL DEFAULT 'AUTO',
                recursive INTEGER NOT NULL DEFAULT 1,
                added_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sent_history (
                fingerprint_key TEXT NOT NULL,
                target_device_id TEXT NOT NULL,
                sha256 TEXT NOT NULL,
                display_name TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                date_taken_ms INTEGER NOT NULL,
                sent_at_ms INTEGER NOT NULL,
                cleared_by_pixel INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (fingerprint_key, target_device_id)
            );
            CREATE INDEX IF NOT EXISTS idx_sent_fp ON sent_history(fingerprint_key);
            CREATE INDEX IF NOT EXISTS idx_sent_cleared ON sent_history(cleared_by_pixel);

            CREATE TABLE IF NOT EXISTS scan_cache (
                file_path TEXT PRIMARY KEY,
                mtime_sec INTEGER NOT NULL,
                size_bytes INTEGER NOT NULL,
                date_taken_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS activity_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp_ms INTEGER NOT NULL,
                kind TEXT NOT NULL,
                message TEXT NOT NULL
            );
            ",
        )?;

        let _ = conn.execute(
            "ALTER TABLE settings ADD COLUMN auto_send_cutoff_epoch_ms INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE settings ADD COLUMN license_key TEXT",
            [],
        );

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM settings WHERE id = 1", [], |r| r.get(0))?;
        if count == 0 {
            let dev_id = format!(
                "mac-{}",
                Uuid::new_v4()
                    .to_string()
                    .replace('-', "")
                    .chars()
                    .take(12)
                    .collect::<String>()
            );
            let host = default_mac_name();
            let now = now_ms();
            conn.execute(
                "INSERT INTO settings (id, device_id, device_name, theme_mode, trial_started_at_ms) VALUES (1, ?, ?, 'DARK', ?)",
                params![dev_id, host, now],
            )?;
        }
        Ok(())
    }

    pub fn get_settings(&self) -> AppResult<MacSettings> {
        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT device_id, device_name, theme_mode, auto_sync_enabled, auto_send_cutoff_epoch_ms,
                    photos_before_videos, include_raw_files, minimize_to_tray, start_at_login,
                    selected_receiver_device_id, default_cleanup_mode, archive_folder_path, license_key
             FROM settings WHERE id = 1",
            [],
            |r| {
                let theme_str: String = r.get(2)?;
                let cleanup_str: String = r.get(10)?;
                Ok(MacSettings {
                    device_id: r.get(0)?,
                    device_name: r.get(1)?,
                    theme_mode: parse_theme(&theme_str),
                    auto_sync_enabled: r.get::<_, i64>(3)? != 0,
                    auto_send_cutoff_epoch_ms: r.get(4)?,
                    photos_before_videos: r.get::<_, i64>(5)? != 0,
                    include_raw_files: r.get::<_, i64>(6)? != 0,
                    minimize_to_tray: r.get::<_, i64>(7)? != 0,
                    start_at_login: r.get::<_, i64>(8)? != 0,
                    selected_receiver_device_id: r.get(9)?,
                    default_cleanup_mode: parse_cleanup_mode(&cleanup_str),
                    archive_folder_path: r.get(11)?,
                    license_key: r.get(12)?,
                })
            },
        )?;
        Ok(row)
    }

    pub fn update_settings(&self, s: &MacSettings) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let clean_name = if s.device_name.trim().is_empty() {
            "Mac".to_string()
        } else {
            s.device_name.trim().to_string()
        };
        conn.execute(
            "UPDATE settings SET
                device_name = ?,
                theme_mode = ?,
                auto_sync_enabled = ?,
                auto_send_cutoff_epoch_ms = ?,
                photos_before_videos = ?,
                include_raw_files = ?,
                minimize_to_tray = ?,
                start_at_login = ?,
                selected_receiver_device_id = ?,
                default_cleanup_mode = ?,
                archive_folder_path = ?,
                license_key = ?
             WHERE id = 1",
            params![
                clean_name,
                theme_to_str(s.theme_mode),
                i64::from(s.auto_sync_enabled),
                s.auto_send_cutoff_epoch_ms,
                i64::from(s.photos_before_videos),
                i64::from(s.include_raw_files),
                i64::from(s.minimize_to_tray),
                i64::from(s.start_at_login),
                s.selected_receiver_device_id,
                cleanup_to_str(s.default_cleanup_mode),
                s.archive_folder_path,
                s.license_key
            ],
        )?;
        Ok(())
    }

    pub fn set_license_key(&self, key: Option<&str>) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE settings SET license_key = ? WHERE id = 1",
            params![key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty())],
        )?;
        Ok(())
    }

    pub fn set_selected_receiver(&self, device_id: Option<&str>) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE settings SET selected_receiver_device_id = ? WHERE id = 1",
            params![device_id],
        )?;
        Ok(())
    }

    pub fn get_or_init_trial_started_ms(&self) -> AppResult<i64> {
        let conn = self.conn.lock().unwrap();
        let val: i64 = conn.query_row(
            "SELECT trial_started_at_ms FROM settings WHERE id = 1",
            [],
            |r| r.get(0),
        )?;
        if val > 0 {
            return Ok(val);
        }
        let now = now_ms();
        conn.execute(
            "UPDATE settings SET trial_started_at_ms = ? WHERE id = 1",
            params![now],
        )?;
        Ok(now)
    }

    pub fn upsert_paired_device(&self, device: &PairedDevice) -> AppResult<()> {
        let existing = self.get_paired_device(&device.device_id)?;
        let paired_at = existing
            .as_ref()
            .map(|d| d.paired_at_epoch_ms)
            .unwrap_or(device.paired_at_epoch_ms);
        let save_override = device
            .save_location_override
            .or_else(|| existing.as_ref().and_then(|d| d.save_location_override));
        let files_count = device
            .total_files_transferred
            .max(existing.as_ref().map(|d| d.total_files_transferred).unwrap_or(0));
        let bytes_count = device
            .total_bytes_transferred
            .max(existing.as_ref().map(|d| d.total_bytes_transferred).unwrap_or(0));

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO paired_devices (
                device_id, device_name, model_name, role, last_ip, last_port, shared_secret,
                paired_at_ms, last_seen_ms, save_location_override, total_files_transferred, total_bytes_transferred
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                device.device_id,
                device.device_name,
                device.model_name,
                role_to_str(device.role),
                device.last_ip_address,
                device.last_port,
                device.shared_secret,
                paired_at,
                device.last_seen_epoch_ms,
                save_override.map(save_mode_to_str),
                files_count,
                bytes_count
            ],
        )?;
        Ok(())
    }

    pub fn update_peer_endpoint(&self, device_id: &str, ip: &str, port: i32) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE paired_devices SET last_ip = ?, last_port = ?, last_seen_ms = ? WHERE device_id = ?",
            params![ip, port, now_ms(), device_id],
        )?;
        Ok(())
    }

    pub fn increment_device_stats(
        &self,
        device_id: &str,
        files_added: i64,
        bytes_added: i64,
    ) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE paired_devices
             SET total_files_transferred = total_files_transferred + ?,
                 total_bytes_transferred = total_bytes_transferred + ?,
                 last_seen_ms = ?
             WHERE device_id = ?",
            params![files_added, bytes_added, now_ms(), device_id],
        )?;
        Ok(())
    }

    pub fn remove_paired_device(&self, device_id: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM paired_devices WHERE device_id = ?",
            params![device_id],
        )?;
        conn.execute(
            "UPDATE settings SET selected_receiver_device_id = NULL WHERE id = 1 AND selected_receiver_device_id = ?",
            params![device_id],
        )?;
        Ok(())
    }

    pub fn get_paired_device(&self, device_id: &str) -> AppResult<Option<PairedDevice>> {
        let devices = self.list_paired_devices()?;
        Ok(devices.into_iter().find(|d| d.device_id == device_id))
    }

    pub fn list_paired_devices(&self) -> AppResult<Vec<PairedDevice>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT device_id, device_name, model_name, role, last_ip, last_port, shared_secret,
                    paired_at_ms, last_seen_ms, save_location_override, total_files_transferred, total_bytes_transferred
             FROM paired_devices ORDER BY last_seen_ms DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let role_s: String = r.get(3)?;
            let override_s: Option<String> = r.get(9)?;
            Ok(PairedDevice {
                device_id: r.get(0)?,
                device_name: r.get(1)?,
                model_name: r.get(2)?,
                role: parse_role(&role_s),
                last_ip_address: r.get(4)?,
                last_port: r.get(5)?,
                shared_secret: r.get(6)?,
                paired_at_epoch_ms: r.get(7)?,
                last_seen_epoch_ms: r.get(8)?,
                save_location_override: override_s.as_deref().and_then(parse_save_mode),
                total_files_transferred: r.get(10)?,
                total_bytes_transferred: r.get(11)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn add_watched_folder(
        &self,
        path: &str,
        display_name: &str,
        policy: FolderPolicy,
        recursive: bool,
    ) -> AppResult<WatchedFolder> {
        let norm_path = normalize_folder_path(path);
        let conn = self.conn.lock().unwrap();
        let existing: Option<WatchedFolder> = conn
            .query_row(
                "SELECT folder_id, path, display_name, policy, recursive, added_at_ms FROM watched_folders WHERE path = ? COLLATE NOCASE",
                params![norm_path],
                |r| {
                    let pol: String = r.get(3)?;
                    Ok(WatchedFolder {
                        folder_id: r.get(0)?,
                        path: r.get(1)?,
                        display_name: r.get(2)?,
                        policy: parse_policy(&pol),
                        recursive: r.get::<_, i64>(4)? != 0,
                        added_at_ms: r.get(5)?,
                    })
                },
            )
            .ok();

        if let Some(folder) = existing {
            return Ok(folder);
        }

        let folder = WatchedFolder {
            folder_id: format!(
                "fld-{}",
                Uuid::new_v4()
                    .to_string()
                    .replace('-', "")
                    .chars()
                    .take(12)
                    .collect::<String>()
            ),
            path: norm_path,
            display_name: display_name.trim().to_string(),
            policy,
            recursive,
            added_at_ms: now_ms(),
        };
        conn.execute(
            "INSERT INTO watched_folders (folder_id, path, display_name, policy, recursive, added_at_ms) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                folder.folder_id,
                folder.path,
                folder.display_name,
                policy_to_str(folder.policy),
                i64::from(folder.recursive),
                folder.added_at_ms
            ],
        )?;
        Ok(folder)
    }

    pub fn update_folder_policy(&self, folder_id: &str, policy: FolderPolicy) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE watched_folders SET policy = ? WHERE folder_id = ?",
            params![policy_to_str(policy), folder_id],
        )?;
        Ok(())
    }

    pub fn update_folder_recursive(&self, folder_id: &str, recursive: bool) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE watched_folders SET recursive = ? WHERE folder_id = ?",
            params![i64::from(recursive), folder_id],
        )?;
        Ok(())
    }

    pub fn remove_watched_folder(&self, folder_id: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM watched_folders WHERE folder_id = ?",
            params![folder_id],
        )?;
        Ok(())
    }

    pub fn list_watched_folders(&self) -> AppResult<Vec<WatchedFolder>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT folder_id, path, display_name, policy, recursive, added_at_ms
             FROM watched_folders ORDER BY added_at_ms ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let pol: String = r.get(3)?;
            Ok(WatchedFolder {
                folder_id: r.get(0)?,
                path: r.get(1)?,
                display_name: r.get(2)?,
                policy: parse_policy(&pol),
                recursive: r.get::<_, i64>(4)? != 0,
                added_at_ms: r.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_sent_and_cleared_sets(&self) -> AppResult<(HashSet<String>, HashSet<String>)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT fingerprint_key, MAX(cleared_by_pixel) FROM sent_history GROUP BY fingerprint_key",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut sent = HashSet::new();
        let mut cleared = HashSet::new();
        for row in rows {
            let (fp, clr) = row?;
            sent.insert(fp.clone());
            if clr != 0 {
                cleared.insert(fp);
            }
        }
        Ok((sent, cleared))
    }

    pub fn get_sent_meta_map(&self) -> AppResult<HashMap<String, SentMeta>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT fingerprint_key, MIN(sent_at_ms), MAX(cleared_by_pixel) FROM sent_history GROUP BY fingerprint_key",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                SentMeta {
                    sent_at_ms: r.get(1)?,
                    cleared_by_pixel: r.get::<_, i64>(2)? != 0,
                },
            ))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (k, v) = row?;
            map.insert(k, v);
        }
        Ok(map)
    }

    pub fn mark_file_sent(
        &self,
        fingerprint_key: &str,
        target_device_id: &str,
        sha256: &str,
        display_name: &str,
        size_bytes: i64,
        date_taken_ms: i64,
    ) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = now_ms();
        conn.execute(
            "INSERT INTO sent_history (
                fingerprint_key, target_device_id, sha256, display_name, size_bytes, date_taken_ms, sent_at_ms, cleared_by_pixel
             ) VALUES (?, ?, ?, ?, ?, ?, ?, 0)
             ON CONFLICT(fingerprint_key, target_device_id) DO UPDATE SET
                sha256 = excluded.sha256,
                display_name = excluded.display_name,
                size_bytes = excluded.size_bytes,
                date_taken_ms = excluded.date_taken_ms",
            params![
                fingerprint_key,
                target_device_id,
                sha256,
                display_name,
                size_bytes,
                date_taken_ms,
                now
            ],
        )?;
        Ok(())
    }

    pub fn mark_fingerprints_sent_batch(
        &self,
        items: &[(String, String, i64, i64)],
        target_device_id: &str,
    ) -> AppResult<()> {
        if items.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = now_ms();
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO sent_history (
                    fingerprint_key, target_device_id, sha256, display_name, size_bytes, date_taken_ms, sent_at_ms, cleared_by_pixel
                 ) VALUES (?, ?, 'REMOTE_LEDGER_MATCH', ?, ?, ?, ?, 0)",
            )?;
            for (fp, name, size, taken_ms) in items {
                stmt.execute(params![fp, target_device_id, name, size, taken_ms, now])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn mark_fingerprints_cleared_batch(
        &self,
        fingerprints: &[String],
        target_device_id: &str,
    ) -> AppResult<()> {
        if fingerprints.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = now_ms();
        {
            let mut upd = tx.prepare(
                "UPDATE sent_history SET cleared_by_pixel = 1 WHERE fingerprint_key = ?",
            )?;
            let mut ins = tx.prepare(
                "INSERT OR IGNORE INTO sent_history (
                    fingerprint_key, target_device_id, sha256, display_name, size_bytes, date_taken_ms, sent_at_ms, cleared_by_pixel
                 ) VALUES (?, ?, 'REMOTE_LEDGER_CLEARED', ?, ?, ?, ?, 1)",
            )?;
            for fp in fingerprints {
                upd.execute(params![fp])?;
                let mut parts = fp.split('|');
                let name = parts.next().unwrap_or("media");
                let size = parts.next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                let sec = parts.next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                ins.execute(params![fp, target_device_id, name, size, sec * 1000, now])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_scan_cache_map(&self) -> AppResult<HashMap<String, (i64, i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT file_path, mtime_sec, size_bytes, date_taken_ms FROM scan_cache",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (k, v) = row?;
            out.insert(k, v);
        }
        Ok(out)
    }

    pub fn upsert_scan_cache_batch(&self, entries: &[(String, i64, i64, i64)]) -> AppResult<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO scan_cache (file_path, mtime_sec, size_bytes, date_taken_ms) VALUES (?, ?, ?, ?)",
            )?;
            for (path, mtime, size, taken) in entries {
                stmt.execute(params![path, mtime, size, taken])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn append_activity(&self, kind: &str, message: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO activity_log (timestamp_ms, kind, message) VALUES (?, ?, ?)",
            params![now_ms(), kind, message],
        )?;
        conn.execute(
            "DELETE FROM activity_log WHERE id NOT IN (SELECT id FROM activity_log ORDER BY id DESC LIMIT 200)",
            [],
        )?;
        Ok(())
    }

    pub fn list_recent_activity(&self, limit: usize) -> AppResult<Vec<ActivityEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp_ms, kind, message FROM activity_log ORDER BY id DESC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(ActivityEntry {
                id: r.get(0)?,
                timestamp_ms: r.get(1)?,
                kind: r.get(2)?,
                message: r.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

fn default_mac_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("/usr/sbin/scutil")
            .args(["--get", "ComputerName"])
            .output()
        {
            if out.status.success() {
                let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    std::env::var("HOSTNAME")
        .ok()
        .map(|s| s.trim_end_matches(".local").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Mac".to_string())
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn build_fingerprint_key(
    display_name: &str,
    size_bytes: i64,
    date_taken_epoch_ms: i64,
) -> String {
    let normalized_sec = date_taken_epoch_ms / 1000;
    format!(
        "{}|{}|{}",
        display_name.trim().to_lowercase(),
        size_bytes,
        normalized_sec
    )
}

pub fn normalize_folder_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed == "/" {
        return "/".to_string();
    }
    let without_trailing = trimmed.trim_end_matches(['\\', '/']);
    if without_trailing.is_empty() && trimmed.starts_with('/') {
        "/".to_string()
    } else if without_trailing.len() == 2 && without_trailing.ends_with(':') {
        format!("{without_trailing}\\")
    } else {
        without_trailing.to_string()
    }
}

fn theme_to_str(t: ThemeMode) -> &'static str {
    match t {
        ThemeMode::System => "SYSTEM",
        ThemeMode::Light => "LIGHT",
        ThemeMode::Dark => "DARK",
        ThemeMode::Matrix => "MATRIX",
    }
}
fn parse_theme(s: &str) -> ThemeMode {
    match s {
        "SYSTEM" => ThemeMode::System,
        "LIGHT" => ThemeMode::Light,
        "MATRIX" => ThemeMode::Matrix,
        _ => ThemeMode::Dark,
    }
}
fn policy_to_str(p: FolderPolicy) -> &'static str {
    match p {
        FolderPolicy::Auto => "AUTO",
        FolderPolicy::Choose => "CHOOSE",
        FolderPolicy::Skip => "SKIP",
    }
}
fn parse_policy(s: &str) -> FolderPolicy {
    match s {
        "AUTO" => FolderPolicy::Auto,
        "SKIP" => FolderPolicy::Skip,
        _ => FolderPolicy::Choose,
    }
}
fn cleanup_to_str(m: CleanupActionMode) -> &'static str {
    match m {
        CleanupActionMode::Trash => "TRASH",
        CleanupActionMode::ArchiveFolder => "ARCHIVE_FOLDER",
        CleanupActionMode::Permanent => "PERMANENT",
    }
}
fn parse_cleanup_mode(s: &str) -> CleanupActionMode {
    match s {
        "ARCHIVE_FOLDER" => CleanupActionMode::ArchiveFolder,
        "PERMANENT" => CleanupActionMode::Permanent,
        _ => CleanupActionMode::Trash,
    }
}
fn role_to_str(r: DeviceRole) -> &'static str {
    match r {
        DeviceRole::Sender => "SENDER",
        DeviceRole::Receiver => "RECEIVER",
    }
}
fn parse_role(s: &str) -> DeviceRole {
    match s {
        "SENDER" => DeviceRole::Sender,
        _ => DeviceRole::Receiver,
    }
}
fn save_mode_to_str(m: SaveLocationMode) -> &'static str {
    match m {
        SaveLocationMode::CameraFolder => "CAMERA_FOLDER",
        SaveLocationMode::SeparateByPhone => "SEPARATE_BY_PHONE",
        SaveLocationMode::MirrorOriginalFolders => "MIRROR_ORIGINAL_FOLDERS",
    }
}
fn parse_save_mode(s: &str) -> Option<SaveLocationMode> {
    match s {
        "CAMERA_FOLDER" => Some(SaveLocationMode::CameraFolder),
        "SEPARATE_BY_PHONE" => Some(SaveLocationMode::SeparateByPhone),
        "MIRROR_ORIGINAL_FOLDERS" => Some(SaveLocationMode::MirrorOriginalFolders),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folder_path_preserves_posix_root() {
        assert_eq!(normalize_folder_path("/"), "/");
        assert_eq!(
            normalize_folder_path("/Users/alice/Pictures/"),
            "/Users/alice/Pictures"
        );
        assert_eq!(normalize_folder_path("  /Users/alice/Photos Library.photoslibrary/ "), "/Users/alice/Photos Library.photoslibrary");
    }
}
