use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::UNIX_EPOCH;

use chrono::{Datelike, Local, TimeZone};
use serde::{Deserialize, Serialize};
use walkdir::{DirEntry, WalkDir};

use crate::apple_photos::{
    is_apple_photos_library, load_apple_photos_metadata_map, resolve_apple_photos_originals_dir,
    ApplePhotosMeta,
};
use crate::db::{build_fingerprint_key, Database, FolderPolicy, WatchedFolder};
use crate::error::AppResult;
use crate::protocol::client::TransferCandidate;

const STANDARD_IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "heic", "heif", "gif", "bmp"];
const STANDARD_VIDEO_EXTS: &[&str] = &["mp4", "mov", "mkv", "webm", "3gp", "m4v", "avi"];
const RAW_EXTS: &[&str] = &["dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2", "pef"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MediaSyncStatus {
    Unsent,
    OnPixel,
    ClearedByPixel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DateGroupingMode {
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacMediaItem {
    pub id: String,
    pub folder_id: String,
    pub file_path: String,
    pub display_name: String,
    pub bucket_name: String,
    pub relative_path: String,
    pub size_bytes: i64,
    pub date_taken_epoch_ms: i64,
    pub date_modified_epoch_sec: i64,
    pub mime_type: String,
    pub is_video: bool,
    pub is_raw: bool,
    pub fingerprint_key: String,
    pub sync_status: MediaSyncStatus,
}

impl MacMediaItem {
    pub fn to_transfer_candidate(&self) -> TransferCandidate {
        TransferCandidate {
            file_path: self.file_path.clone(),
            display_name: self.display_name.clone(),
            bucket_name: self.bucket_name.clone(),
            relative_path: self.relative_path.clone(),
            size_bytes: self.size_bytes,
            date_taken_epoch_ms: self.date_taken_epoch_ms,
            date_modified_epoch_sec: self.date_modified_epoch_sec,
            mime_type: self.mime_type.clone(),
            is_video: self.is_video,
            fingerprint_key: self.fingerprint_key.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSummary {
    pub folder_id: String,
    pub path: String,
    pub display_name: String,
    pub policy: FolderPolicy,
    pub recursive: bool,
    pub exists_on_disk: bool,
    pub is_apple_photos_library: bool,
    pub total_count: usize,
    pub unsent_count: usize,
    pub on_pixel_count: usize,
    pub cleared_count: usize,
    pub total_bytes: i64,
    pub unsent_bytes: i64,
    pub reclaimable_cleared_bytes: i64,
    pub reclaimable_all_sent_bytes: i64,
    pub percent_uploaded: f64,
    pub is_100_percent_uploaded: bool,
    pub latest_date_taken_ms: i64,
    pub cover_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDateGroup {
    pub group_key: String,
    pub group_title: String,
    pub total_count: usize,
    pub unsent_count: usize,
    pub items: Vec<MacMediaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub folders: Vec<FolderSummary>,
    pub total_media_count: usize,
    pub total_unsent_count: usize,
    pub total_cleared_count: usize,
    pub total_reclaimable_cleared_bytes: i64,
    pub total_reclaimable_all_sent_bytes: i64,
}

fn is_ignored_dir(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    let lower = name.to_lowercase();
    lower.starts_with('.')
        || lower == ".spotlight-v100"
        || lower == ".trashes"
        || lower == ".fseventsd"
        || lower == ".temporaryitems"
        || lower == "$recycle.bin"
        || lower == "system volume information"
        || lower == "node_modules"
        || lower == "__macosx"
}

fn is_ignored_filename(name: &str) -> bool {
    name.starts_with("._") || name.eq_ignore_ascii_case(".DS_Store")
}

pub fn classify_media_extension(path: &Path, include_raw: bool) -> Option<(bool, bool, &'static str)> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    if STANDARD_IMAGE_EXTS.contains(&ext.as_str()) {
        let mime = match ext.as_str() {
            "png" => "image/png",
            "webp" => "image/webp",
            "heic" | "heif" => "image/heic",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        };
        return Some((false, false, mime));
    }
    if STANDARD_VIDEO_EXTS.contains(&ext.as_str()) {
        let mime = match ext.as_str() {
            "mov" => "video/quicktime",
            "mkv" => "video/x-matroska",
            "webm" => "video/webm",
            "3gp" => "video/3gpp",
            _ => "video/mp4",
        };
        return Some((true, false, mime));
    }
    if include_raw && RAW_EXTS.contains(&ext.as_str()) {
        let mime = match ext.as_str() {
            "dng" => "image/x-adobe-dng",
            _ => "image/x-raw",
        };
        return Some((false, true, mime));
    }
    None
}

fn read_exif_date_taken_ms(path: &Path) -> Option<i64> {
    let file = File::open(path).ok()?;
    let mut buf_reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut buf_reader).ok()?;
    let field = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .or_else(|| exif.get_field(exif::Tag::DateTimeDigitized, exif::In::PRIMARY))
        .or_else(|| exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY))?;
    let val_str = field.display_value().to_string();
    let clean = val_str.trim().trim_matches('"');
    let parsed = chrono::NaiveDateTime::parse_from_str(clean, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(clean, "%Y:%m:%d %H:%M:%S"))
        .ok()?;
    let dt = Local.from_local_datetime(&parsed).earliest()?;
    Some(dt.timestamp_millis())
}

#[allow(clippy::too_many_arguments)]
pub fn scan_single_file(
    file_path: &Path,
    folder: &WatchedFolder,
    include_raw: bool,
    is_photos_library: bool,
    apple_meta: Option<&HashMap<String, ApplePhotosMeta>>,
    sent_set: &HashSet<String>,
    cleared_set: &HashSet<String>,
    cache_map: &HashMap<String, (i64, i64, i64)>,
    new_cache_entries: &mut Vec<(String, i64, i64, i64)>,
) -> Option<MacMediaItem> {
    let raw_file_name = file_path.file_name().map(|n| n.to_string_lossy().to_string())?;
    if is_ignored_filename(&raw_file_name) {
        return None;
    }
    let (is_video, is_raw, mime) = classify_media_extension(file_path, include_raw)?;
    let meta = std::fs::metadata(file_path).ok()?;
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }
    let size_bytes = meta.len() as i64;
    let mtime_sec = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    let ap_entry = apple_meta.and_then(|map| map.get(&raw_file_name.to_lowercase()));

    let path_str = file_path.to_string_lossy().to_string();
    let date_taken_ms = match cache_map.get(&path_str) {
        Some(&(cached_mtime, cached_size, cached_taken))
            if cached_mtime == mtime_sec && cached_size == size_bytes =>
        {
            cached_taken
        }
        _ => {
            let exif_ms = if !is_video {
                read_exif_date_taken_ms(file_path)
            } else {
                None
            };
            let taken = exif_ms
                .or_else(|| ap_entry.and_then(|m| m.date_taken_epoch_ms))
                .unwrap_or(mtime_sec * 1000);
            new_cache_entries.push((path_str.clone(), mtime_sec, size_bytes, taken));
            taken
        }
    };

    let display_name = ap_entry
        .and_then(|m| m.original_filename.clone())
        .unwrap_or(raw_file_name);

    let (bucket_name, relative_path) = if is_photos_library {
        (folder.display_name.clone(), folder.display_name.clone())
    } else {
        let root = Path::new(&folder.path);
        let rel_dir = file_path
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let rel = if rel_dir.is_empty() {
            folder.display_name.clone()
        } else {
            format!("{}/{}", folder.display_name, rel_dir)
        };
        let bucket = file_path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| folder.display_name.clone());
        (bucket, rel)
    };

    let fp_key = build_fingerprint_key(&display_name, size_bytes, date_taken_ms);
    let sync_status = if cleared_set.contains(&fp_key) {
        MediaSyncStatus::ClearedByPixel
    } else if sent_set.contains(&fp_key) {
        MediaSyncStatus::OnPixel
    } else {
        MediaSyncStatus::Unsent
    };

    Some(MacMediaItem {
        id: path_str.clone(),
        folder_id: folder.folder_id.clone(),
        file_path: path_str,
        display_name,
        bucket_name,
        relative_path,
        size_bytes,
        date_taken_epoch_ms: date_taken_ms,
        date_modified_epoch_sec: mtime_sec,
        mime_type: mime.to_string(),
        is_video,
        is_raw,
        fingerprint_key: fp_key,
        sync_status,
    })
}

pub fn scan_folder_items(
    db: &Database,
    folder: &WatchedFolder,
    include_raw: bool,
) -> AppResult<(Vec<MacMediaItem>, bool)> {
    let root = Path::new(&folder.path);
    if !root.exists() || !root.is_dir() {
        return Ok((Vec::new(), false));
    }

    let is_photos_lib = is_apple_photos_library(root);
    let (scan_root, apple_meta_opt) = if is_photos_lib {
        let Some(orig_dir) = resolve_apple_photos_originals_dir(root) else {
            return Ok((Vec::new(), false));
        };
        let meta_map = load_apple_photos_metadata_map(root);
        (orig_dir, Some(meta_map))
    } else {
        (root.to_path_buf(), None)
    };

    let (sent_set, cleared_set) = db.get_sent_and_cleared_sets()?;
    let cache_map = db.load_scan_cache_map()?;
    let mut new_cache = Vec::new();
    let mut items = Vec::new();

    let walker = if is_photos_lib || folder.recursive {
        WalkDir::new(&scan_root)
    } else {
        WalkDir::new(&scan_root).max_depth(1)
    };

    for entry in walker.into_iter().filter_entry(|e| e.depth() == 0 || !is_ignored_dir(e)) {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(item) = scan_single_file(
            entry.path(),
            folder,
            include_raw,
            is_photos_lib,
            apple_meta_opt.as_ref(),
            &sent_set,
            &cleared_set,
            &cache_map,
            &mut new_cache,
        ) {
            items.push(item);
        }
    }

    if !new_cache.is_empty() {
        let _ = db.upsert_scan_cache_batch(&new_cache);
    }
    items.sort_by(|a, b| b.date_taken_epoch_ms.cmp(&a.date_taken_epoch_ms));
    Ok((items, true))
}

pub fn summarize_folder(folder: &WatchedFolder, items: &[MacMediaItem], exists_on_disk: bool) -> FolderSummary {
    let total_count = items.len();
    let mut unsent_count = 0usize;
    let mut on_pixel_count = 0usize;
    let mut cleared_count = 0usize;
    let mut total_bytes = 0i64;
    let mut unsent_bytes = 0i64;
    let mut reclaimable_cleared_bytes = 0i64;
    let mut reclaimable_all_sent_bytes = 0i64;
    let mut latest_date_taken_ms = 0i64;

    for item in items {
        total_bytes += item.size_bytes;
        if item.date_taken_epoch_ms > latest_date_taken_ms {
            latest_date_taken_ms = item.date_taken_epoch_ms;
        }
        match item.sync_status {
            MediaSyncStatus::Unsent => {
                unsent_count += 1;
                unsent_bytes += item.size_bytes;
            }
            MediaSyncStatus::OnPixel => {
                on_pixel_count += 1;
                reclaimable_all_sent_bytes += item.size_bytes;
            }
            MediaSyncStatus::ClearedByPixel => {
                cleared_count += 1;
                reclaimable_cleared_bytes += item.size_bytes;
                reclaimable_all_sent_bytes += item.size_bytes;
            }
        }
    }

    let uploaded_count = on_pixel_count + cleared_count;
    let percent_uploaded = if total_count == 0 {
        0.0
    } else {
        (uploaded_count as f64 / total_count as f64) * 100.0
    };
    let is_100_percent_uploaded = total_count > 0 && unsent_count == 0;
    let cover_path = items.first().map(|i| i.file_path.clone());
    let is_photos_lib = is_apple_photos_library(Path::new(&folder.path));

    FolderSummary {
        folder_id: folder.folder_id.clone(),
        path: folder.path.clone(),
        display_name: folder.display_name.clone(),
        policy: folder.policy,
        recursive: folder.recursive,
        exists_on_disk,
        is_apple_photos_library: is_photos_lib,
        total_count,
        unsent_count,
        on_pixel_count,
        cleared_count,
        total_bytes,
        unsent_bytes,
        reclaimable_cleared_bytes,
        reclaimable_all_sent_bytes,
        percent_uploaded,
        is_100_percent_uploaded,
        latest_date_taken_ms,
        cover_path,
    }
}

pub fn scan_library_snapshot(db: &Database) -> AppResult<(LibrarySnapshot, Vec<MacMediaItem>)> {
    let settings = db.get_settings()?;
    let folders = db.list_watched_folders()?;
    let mut summaries = Vec::with_capacity(folders.len());
    let mut all_items = Vec::new();

    for folder in &folders {
        let (items, exists) = scan_folder_items(db, folder, settings.include_raw_files)?;
        summaries.push(summarize_folder(folder, &items, exists));
        all_items.extend(items);
    }

    let total_media_count = all_items.len();
    let total_unsent_count = summaries.iter().map(|f| f.unsent_count).sum();
    let total_cleared_count = summaries.iter().map(|f| f.cleared_count).sum();
    let total_reclaimable_cleared_bytes = summaries.iter().map(|f| f.reclaimable_cleared_bytes).sum();
    let total_reclaimable_all_sent_bytes = summaries.iter().map(|f| f.reclaimable_all_sent_bytes).sum();

    all_items.sort_by(|a, b| b.date_taken_epoch_ms.cmp(&a.date_taken_epoch_ms));

    Ok((
        LibrarySnapshot {
            folders: summaries,
            total_media_count,
            total_unsent_count,
            total_cleared_count,
            total_reclaimable_cleared_bytes,
            total_reclaimable_all_sent_bytes,
        },
        all_items,
    ))
}

pub fn group_items_by_date(
    items: Vec<MacMediaItem>,
    mode: DateGroupingMode,
) -> Vec<MediaDateGroup> {
    let mut grouped: BTreeMap<String, (String, Vec<MacMediaItem>)> = BTreeMap::new();

    for item in items {
        let dt = Local
            .timestamp_millis_opt(item.date_taken_epoch_ms)
            .single()
            .unwrap_or_else(Local::now);
        let (key, title) = match mode {
            DateGroupingMode::Day => (
                dt.format("%Y-%m-%d").to_string(),
                dt.format("%a, %b %-d, %Y").to_string(),
            ),
            DateGroupingMode::Week => {
                let iso_week = dt.iso_week();
                let days_from_mon = dt.weekday().num_days_from_monday() as i64;
                let week_start = dt - chrono::Duration::days(days_from_mon);
                let week_end = week_start + chrono::Duration::days(6);
                (
                    format!("{}-W{:02}", iso_week.year(), iso_week.week()),
                    format!(
                        "Week of {} – {}",
                        week_start.format("%b %-d"),
                        week_end.format("%b %-d, %Y")
                    ),
                )
            }
            DateGroupingMode::Month => (
                dt.format("%Y-%m").to_string(),
                dt.format("%B %Y").to_string(),
            ),
        };
        grouped
            .entry(key)
            .or_insert_with(|| (title, Vec::new()))
            .1
            .push(item);
    }

    grouped
        .into_iter()
        .rev()
        .map(|(group_key, (group_title, items))| {
            let total_count = items.len();
            let unsent_count = items
                .iter()
                .filter(|i| i.sync_status == MediaSyncStatus::Unsent)
                .count();
            MediaDateGroup {
                group_key,
                group_title,
                total_count,
                unsent_count,
                items,
            }
        })
        .collect()
}

pub fn scan_standalone_files(db: &Database, paths: &[String]) -> AppResult<Vec<MacMediaItem>> {
    let settings = db.get_settings()?;
    let (sent_set, cleared_set) = db.get_sent_and_cleared_sets()?;
    let cache_map = db.load_scan_cache_map()?;
    let mut new_cache = Vec::new();
    let mut out = Vec::new();

    for raw in paths {
        let p = Path::new(raw);
        if !p.exists() || !p.is_file() {
            continue;
        }
        let parent_name = p
            .parent()
            .and_then(|d| d.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Mac Drop".to_string());
        let dummy_folder = WatchedFolder {
            folder_id: "dropped".to_string(),
            path: p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default(),
            display_name: parent_name,
            policy: FolderPolicy::Choose,
            recursive: false,
            added_at_ms: 0,
        };
        if let Some(item) = scan_single_file(
            p,
            &dummy_folder,
            settings.include_raw_files,
            false,
            None,
            &sent_set,
            &cleared_set,
            &cache_map,
            &mut new_cache,
        ) {
            out.push(item);
        }
    }
    if !new_cache.is_empty() {
        let _ = db.upsert_scan_cache_batch(&new_cache);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::fs;

    #[test]
    fn scanner_handles_recursive_raw_and_skips_appledouble() {
        let tmp = std::env::temp_dir().join(format!("pixrelay_scan_{}", uuid::Uuid::new_v4()));
        let sub = tmp.join("Japan");
        fs::create_dir_all(&sub).unwrap();
        fs::write(tmp.join("IMG_001.jpg"), vec![1u8; 64]).unwrap();
        fs::write(tmp.join("._IMG_001.jpg"), vec![9u8; 32]).unwrap();
        fs::write(tmp.join(".DS_Store"), vec![8u8; 32]).unwrap();
        fs::write(sub.join("VID_002.mp4"), vec![2u8; 128]).unwrap();
        fs::write(sub.join("RAW_003.ARW"), vec![3u8; 256]).unwrap();

        let db = Database::open_path(&tmp.join("test.db")).unwrap();
        let folder = db
            .add_watched_folder(
                &tmp.to_string_lossy(),
                "Trips",
                FolderPolicy::Auto,
                true,
            )
            .unwrap();

        let (items, exists) = scan_folder_items(&db, &folder, false).unwrap();
        assert!(exists);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| !i.display_name.starts_with("._")));

        let (items_raw, _) = scan_folder_items(&db, &folder, true).unwrap();
        assert_eq!(items_raw.len(), 3);

        let non_rec = WatchedFolder {
            recursive: false,
            ..folder
        };
        let (items_top, _) = scan_folder_items(&db, &non_rec, false).unwrap();
        assert_eq!(items_top.len(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scanner_scans_apple_photos_originals_only_and_enriches_metadata() {
        let tmp = std::env::temp_dir().join(format!("pixrelay_apscan_{}", uuid::Uuid::new_v4()));
        let lib = tmp.join("Photos Library.photoslibrary");
        let orig_sub = lib.join("originals").join("B");
        let deriv_sub = lib.join("resources").join("derivatives").join("B");
        let db_dir = lib.join("database");
        fs::create_dir_all(&orig_sub).unwrap();
        fs::create_dir_all(&deriv_sub).unwrap();
        fs::create_dir_all(&db_dir).unwrap();

        fs::write(orig_sub.join("ABCD-1234.heic"), vec![1u8; 256]).unwrap();
        // Derivative thumbnail inside resources/ must NOT be scanned
        fs::write(deriv_sub.join("ABCD-1234_thumb.jpg"), vec![2u8; 64]).unwrap();

        {
            let conn = Connection::open(db_dir.join("Photos.sqlite")).unwrap();
            conn.execute_batch(
                "
                CREATE TABLE ZASSET (Z_PK INTEGER PRIMARY KEY, ZFILENAME TEXT, ZDATECREATED REAL);
                CREATE TABLE ZADDITIONALASSETATTRIBUTES (Z_PK INTEGER PRIMARY KEY, ZASSET INTEGER, ZORIGINALFILENAME TEXT);
                INSERT INTO ZASSET (Z_PK, ZFILENAME, ZDATECREATED) VALUES (1, 'ABCD-1234.heic', 757382400.0);
                INSERT INTO ZADDITIONALASSETATTRIBUTES (Z_PK, ZASSET, ZORIGINALFILENAME) VALUES (5, 1, 'IMG_9001.HEIC');
                ",
            )
            .unwrap();
        }

        let db = Database::open_path(&tmp.join("mac.db")).unwrap();
        let folder = db
            .add_watched_folder(
                &lib.to_string_lossy(),
                "Photos Library",
                FolderPolicy::Auto,
                true,
            )
            .unwrap();

        let (items, exists) = scan_folder_items(&db, &folder, false).unwrap();
        assert!(exists);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].display_name, "IMG_9001.HEIC");
        assert_eq!(items[0].date_taken_epoch_ms, 1_735_689_600_000);
        assert_eq!(items[0].bucket_name, "Photos Library");
        assert_eq!(items[0].relative_path, "Photos Library");

        let summary = summarize_folder(&folder, &items, exists);
        assert!(summary.is_apple_photos_library);

        let _ = fs::remove_dir_all(&tmp);
    }
}
