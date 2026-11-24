use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::apple_photos::is_apple_photos_library;
use crate::db::{now_ms, CleanupActionMode, Database};
use crate::error::{AppError, AppResult};
use crate::scanner::{scan_folder_items, MediaSyncStatus};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPreviewRequest {
    pub folder_ids: Option<Vec<String>>,
    pub include_on_pixel_unconfirmed: bool,
    pub min_age_days: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderCleanupBreakdown {
    pub folder_id: String,
    pub display_name: String,
    pub path: String,
    pub cleared_count: usize,
    pub cleared_bytes: i64,
    pub on_pixel_count: usize,
    pub on_pixel_bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPreview {
    pub eligible_count: usize,
    pub eligible_bytes: i64,
    pub cleared_count: usize,
    pub cleared_bytes: i64,
    pub on_pixel_unconfirmed_count: usize,
    pub on_pixel_unconfirmed_bytes: i64,
    pub per_folder: Vec<FolderCleanupBreakdown>,
    pub sample_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExecuteRequest {
    pub folder_ids: Option<Vec<String>>,
    pub include_on_pixel_unconfirmed: bool,
    pub min_age_days: u32,
    pub mode: CleanupActionMode,
    pub archive_root: Option<String>,
    pub remove_empty_folders: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExecuteResult {
    pub cleaned_count: usize,
    pub freed_bytes: i64,
    pub failed_count: usize,
    pub removed_empty_dirs: usize,
    pub message: String,
}

struct EligibleFile {
    file_path: String,
    folder_root: String,
    folder_name: String,
    size_bytes: i64,
    is_cleared: bool,
}

fn collect_eligible_files(
    db: &Database,
    folder_ids: Option<&[String]>,
    include_on_pixel_unconfirmed: bool,
    min_age_days: u32,
) -> AppResult<(Vec<EligibleFile>, Vec<FolderCleanupBreakdown>)> {
    let settings = db.get_settings()?;
    let folders = db.list_watched_folders()?;
    let sent_meta = db.get_sent_meta_map()?;
    let cutoff_ms = if min_age_days == 0 {
        i64::MAX
    } else {
        now_ms() - (min_age_days as i64) * 86_400_000
    };

    let mut eligible = Vec::new();
    let mut breakdowns = Vec::new();

    for folder in folders {
        if let Some(filter) = folder_ids {
            if !filter.contains(&folder.folder_id) {
                continue;
            }
        }
        // Never modify or delete files inside an Apple Photos Library bundle
        if is_apple_photos_library(Path::new(&folder.path)) {
            continue;
        }
        let (items, exists) = scan_folder_items(db, &folder, settings.include_raw_files)?;
        if !exists {
            continue;
        }

        let mut cleared_count = 0usize;
        let mut cleared_bytes = 0i64;
        let mut on_pixel_count = 0usize;
        let mut on_pixel_bytes = 0i64;

        for item in items {
            let Some(meta) = sent_meta.get(&item.fingerprint_key) else {
                continue;
            };
            if min_age_days > 0 && meta.sent_at_ms > cutoff_ms {
                continue;
            }
            match item.sync_status {
                MediaSyncStatus::Unsent => continue,
                MediaSyncStatus::ClearedByPixel => {
                    cleared_count += 1;
                    cleared_bytes += item.size_bytes;
                    eligible.push(EligibleFile {
                        file_path: item.file_path,
                        folder_root: folder.path.clone(),
                        folder_name: folder.display_name.clone(),
                        size_bytes: item.size_bytes,
                        is_cleared: true,
                    });
                }
                MediaSyncStatus::OnPixel => {
                    if include_on_pixel_unconfirmed {
                        on_pixel_count += 1;
                        on_pixel_bytes += item.size_bytes;
                        eligible.push(EligibleFile {
                            file_path: item.file_path,
                            folder_root: folder.path.clone(),
                            folder_name: folder.display_name.clone(),
                            size_bytes: item.size_bytes,
                            is_cleared: false,
                        });
                    }
                }
            }
        }

        if cleared_count > 0 || on_pixel_count > 0 {
            breakdowns.push(FolderCleanupBreakdown {
                folder_id: folder.folder_id,
                display_name: folder.display_name,
                path: folder.path,
                cleared_count,
                cleared_bytes,
                on_pixel_count,
                on_pixel_bytes,
            });
        }
    }

    Ok((eligible, breakdowns))
}

pub fn preview_free_up_space(db: &Database, req: &CleanupPreviewRequest) -> AppResult<CleanupPreview> {
    let (eligible, breakdowns) = collect_eligible_files(
        db,
        req.folder_ids.as_deref(),
        req.include_on_pixel_unconfirmed,
        req.min_age_days,
    )?;

    let mut cleared_count = 0usize;
    let mut cleared_bytes = 0i64;
    let mut on_pixel_count = 0usize;
    let mut on_pixel_bytes = 0i64;

    for f in &eligible {
        if f.is_cleared {
            cleared_count += 1;
            cleared_bytes += f.size_bytes;
        } else {
            on_pixel_count += 1;
            on_pixel_bytes += f.size_bytes;
        }
    }

    let sample_paths = eligible.iter().take(20).map(|f| f.file_path.clone()).collect();

    Ok(CleanupPreview {
        eligible_count: eligible.len(),
        eligible_bytes: cleared_bytes + on_pixel_bytes,
        cleared_count,
        cleared_bytes,
        on_pixel_unconfirmed_count: on_pixel_count,
        on_pixel_unconfirmed_bytes: on_pixel_bytes,
        per_folder: breakdowns,
        sample_paths,
    })
}

pub fn execute_free_up_space(db: &Database, req: &CleanupExecuteRequest) -> AppResult<CleanupExecuteResult> {
    if req.mode == CleanupActionMode::ArchiveFolder {
        let root_ok = req
            .archive_root
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !root_ok {
            return Err(AppError::Message(
                "Choose an archive destination folder first.".to_string(),
            ));
        }
    }

    let (eligible, _) = collect_eligible_files(
        db,
        req.folder_ids.as_deref(),
        req.include_on_pixel_unconfirmed,
        req.min_age_days,
    )?;

    if eligible.is_empty() {
        return Ok(CleanupExecuteResult {
            cleaned_count: 0,
            freed_bytes: 0,
            failed_count: 0,
            removed_empty_dirs: 0,
            message: "No eligible uploaded files matched your filter.".to_string(),
        });
    }

    let mut verified: Vec<EligibleFile> = Vec::with_capacity(eligible.len());
    let mut failed_count = 0usize;
    for f in eligible {
        match std::fs::metadata(&f.file_path) {
            Ok(meta) if meta.is_file() && meta.len() as i64 == f.size_bytes => {
                verified.push(f);
            }
            _ => failed_count += 1,
        }
    }

    let mut cleaned_count = 0usize;
    let mut freed_bytes = 0i64;
    let mut touched_roots: BTreeSet<String> = BTreeSet::new();

    match req.mode {
        CleanupActionMode::Trash => {
            let paths: Vec<PathBuf> = verified.iter().map(|f| PathBuf::from(&f.file_path)).collect();
            if trash::delete_all(&paths).is_ok() {
                for f in &verified {
                    cleaned_count += 1;
                    freed_bytes += f.size_bytes;
                    touched_roots.insert(f.folder_root.clone());
                }
            } else {
                for f in &verified {
                    if trash::delete(&f.file_path).is_ok() {
                        cleaned_count += 1;
                        freed_bytes += f.size_bytes;
                        touched_roots.insert(f.folder_root.clone());
                    } else {
                        failed_count += 1;
                    }
                }
            }
        }
        CleanupActionMode::ArchiveFolder => {
            let archive_base = PathBuf::from(req.archive_root.as_deref().unwrap_or_default());
            std::fs::create_dir_all(&archive_base)?;
            for f in &verified {
                let src = Path::new(&f.file_path);
                let rel = src
                    .strip_prefix(&f.folder_root)
                    .unwrap_or_else(|_| Path::new(src.file_name().unwrap_or_default()));
                let dest = archive_base.join(&f.folder_name).join(rel);
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let moved = match std::fs::rename(src, &dest) {
                    Ok(()) => true,
                    Err(_) => {
                        if std::fs::copy(src, &dest).is_ok() && std::fs::remove_file(src).is_ok() {
                            true
                        } else {
                            let _ = std::fs::remove_file(&dest);
                            false
                        }
                    }
                };
                if moved {
                    cleaned_count += 1;
                    freed_bytes += f.size_bytes;
                    touched_roots.insert(f.folder_root.clone());
                } else {
                    failed_count += 1;
                }
            }
        }
        CleanupActionMode::Permanent => {
            for f in &verified {
                if std::fs::remove_file(&f.file_path).is_ok() {
                    cleaned_count += 1;
                    freed_bytes += f.size_bytes;
                    touched_roots.insert(f.folder_root.clone());
                } else {
                    failed_count += 1;
                }
            }
        }
    }

    let mut removed_empty_dirs = 0usize;
    if req.remove_empty_folders {
        for root_str in touched_roots {
            removed_empty_dirs += prune_empty_subfolders(Path::new(&root_str));
        }
    }

    let action_word = match req.mode {
        CleanupActionMode::Trash => "moved to Trash",
        CleanupActionMode::ArchiveFolder => "moved to archive",
        CleanupActionMode::Permanent => "permanently deleted",
    };
    let msg = format!("{cleaned_count} files {action_word}");
    let _ = db.append_activity("CLEANUP", &msg);

    Ok(CleanupExecuteResult {
        cleaned_count,
        freed_bytes,
        failed_count,
        removed_empty_dirs,
        message: msg,
    })
}

fn prune_empty_subfolders(root: &Path) -> usize {
    if !root.exists() || !root.is_dir() {
        return 0;
    }
    let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .map(|e| e.into_path())
        .collect();
    dirs.sort_by(|a, b| b.components().count().cmp(&a.components().count()));

    let mut removed = 0usize;
    for dir in dirs {
        if let Ok(mut entries) = std::fs::read_dir(&dir) {
            if entries.next().is_none() && std::fs::remove_dir(&dir).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{build_fingerprint_key, FolderPolicy};
    use std::fs;
    use std::time::UNIX_EPOCH;

    #[test]
    fn cleanup_never_touches_unsent_or_unconfirmed_when_only_cleared_requested() {
        let tmp = std::env::temp_dir().join(format!("pixrelay_clean_{}", uuid::Uuid::new_v4()));
        let photos = tmp.join("Photos");
        let archive = tmp.join("Archive");
        fs::create_dir_all(&photos).unwrap();

        let f_unsent = photos.join("unsent.jpg");
        let f_on_pixel = photos.join("on_pixel.jpg");
        let f_cleared = photos.join("cleared.jpg");
        fs::write(&f_unsent, vec![1u8; 100]).unwrap();
        fs::write(&f_on_pixel, vec![2u8; 200]).unwrap();
        fs::write(&f_cleared, vec![3u8; 300]).unwrap();

        let db = Database::open_path(&tmp.join("test.db")).unwrap();
        let folder = db
            .add_watched_folder(&photos.to_string_lossy(), "Photos", FolderPolicy::Auto, true)
            .unwrap();

        for (path, mark_cleared) in [(&f_on_pixel, false), (&f_cleared, true)] {
            let meta = fs::metadata(path).unwrap();
            let mtime_ms = meta
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                * 1000;
            let fp = build_fingerprint_key(
                &path.file_name().unwrap().to_string_lossy(),
                meta.len() as i64,
                mtime_ms,
            );
            db.mark_file_sent(
                &fp,
                "pixel-1",
                "sha",
                &path.file_name().unwrap().to_string_lossy(),
                meta.len() as i64,
                mtime_ms,
            )
            .unwrap();
            if mark_cleared {
                db.mark_fingerprints_cleared_batch(&[fp], "pixel-1").unwrap();
            }
        }

        let preview = preview_free_up_space(
            &db,
            &CleanupPreviewRequest {
                folder_ids: Some(vec![folder.folder_id.clone()]),
                include_on_pixel_unconfirmed: false,
                min_age_days: 0,
            },
        )
        .unwrap();
        assert_eq!(preview.cleared_count, 1);
        assert_eq!(preview.eligible_count, 1);
        assert_eq!(preview.eligible_bytes, 300);

        let res = execute_free_up_space(
            &db,
            &CleanupExecuteRequest {
                folder_ids: Some(vec![folder.folder_id]),
                include_on_pixel_unconfirmed: false,
                min_age_days: 0,
                mode: CleanupActionMode::ArchiveFolder,
                archive_root: Some(archive.to_string_lossy().to_string()),
                remove_empty_folders: false,
            },
        )
        .unwrap();
        assert_eq!(res.cleaned_count, 1);
        assert_eq!(res.freed_bytes, 300);
        assert!(f_unsent.exists());
        assert!(f_on_pixel.exists());
        assert!(!f_cleared.exists());
        assert!(archive.join("Photos").join("cleared.jpg").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cleanup_strictly_protects_apple_photos_library() {
        let tmp = std::env::temp_dir().join(format!("pixrelay_clean_ap_{}", uuid::Uuid::new_v4()));
        let lib = tmp.join("Photos Library.photoslibrary");
        let orig = lib.join("originals");
        fs::create_dir_all(&orig).unwrap();

        let f_cleared = orig.join("IMG_1001.jpg");
        fs::write(&f_cleared, vec![7u8; 400]).unwrap();
        let meta = fs::metadata(&f_cleared).unwrap();
        let mtime_ms = meta
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            * 1000;
        let fp = build_fingerprint_key("IMG_1001.jpg", meta.len() as i64, mtime_ms);

        let db = Database::open_path(&tmp.join("mac.db")).unwrap();
        let folder = db
            .add_watched_folder(&lib.to_string_lossy(), "Photos Library", FolderPolicy::Auto, true)
            .unwrap();
        db.mark_file_sent(&fp, "pixel-1", "sha", "IMG_1001.jpg", meta.len() as i64, mtime_ms)
            .unwrap();
        db.mark_fingerprints_cleared_batch(&[fp], "pixel-1").unwrap();

        let preview = preview_free_up_space(
            &db,
            &CleanupPreviewRequest {
                folder_ids: Some(vec![folder.folder_id.clone()]),
                include_on_pixel_unconfirmed: true,
                min_age_days: 0,
            },
        )
        .unwrap();
        assert_eq!(preview.eligible_count, 0);
        assert_eq!(preview.eligible_bytes, 0);

        let res = execute_free_up_space(
            &db,
            &CleanupExecuteRequest {
                folder_ids: Some(vec![folder.folder_id]),
                include_on_pixel_unconfirmed: true,
                min_age_days: 0,
                mode: CleanupActionMode::Permanent,
                archive_root: None,
                remove_empty_folders: true,
            },
        )
        .unwrap();
        assert_eq!(res.cleaned_count, 0);
        assert!(f_cleared.exists());

        let _ = fs::remove_dir_all(&tmp);
    }
}
