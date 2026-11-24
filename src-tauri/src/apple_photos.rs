use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::db::{normalize_folder_path, Database};

const CORE_DATA_EPOCH_OFFSET_SECS: f64 = 978_307_200.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplePhotosLibraryInfo {
    pub path: String,
    pub display_name: String,
    pub already_added: bool,
    pub readable: bool,
    pub needs_permission: bool,
}

#[derive(Debug, Clone)]
pub struct ApplePhotosMeta {
    pub original_filename: Option<String>,
    pub date_taken_epoch_ms: Option<i64>,
}

pub fn is_apple_photos_library(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        ancestor
            .file_name()
            .and_then(|n| n.to_str())
            .map(|name| name.to_lowercase().ends_with(".photoslibrary"))
            .unwrap_or(false)
    })
}

pub fn resolve_apple_photos_originals_dir(bundle_path: &Path) -> Option<PathBuf> {
    let originals = bundle_path.join("originals");
    if originals.is_dir() {
        return Some(originals);
    }
    let masters = bundle_path.join("Masters");
    if masters.is_dir() {
        return Some(masters);
    }
    None
}

pub fn detect_apple_photos_libraries(db: &Database) -> Vec<ApplePhotosLibraryInfo> {
    let Some(pictures) = dirs::picture_dir() else {
        return Vec::new();
    };
    detect_apple_photos_libraries_in_dir(&pictures, db)
}

pub fn detect_apple_photos_libraries_in_dir(
    pictures_dir: &Path,
    db: &Database,
) -> Vec<ApplePhotosLibraryInfo> {
    if !pictures_dir.is_dir() {
        return Vec::new();
    }

    let watched_paths: std::collections::HashSet<String> = db
        .list_watched_folders()
        .unwrap_or_default()
        .into_iter()
        .map(|f| normalize_folder_path(&f.path).to_lowercase())
        .collect();

    let mut found: Vec<ApplePhotosLibraryInfo> = Vec::new();
    let Ok(entries) = std::fs::read_dir(pictures_dir) else {
        return found;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !fname.to_lowercase().ends_with(".photoslibrary") {
            continue;
        }
        let norm_path = normalize_folder_path(&path.to_string_lossy());
        let already_added = watched_paths.contains(&norm_path.to_lowercase());
        let display_name = fname
            .strip_suffix(".photoslibrary")
            .or_else(|| fname.strip_suffix(".PHOTOSLIBRARY"))
            .unwrap_or(fname)
            .to_string();

        let (readable, needs_permission) = check_library_readability(&path);
        found.push(ApplePhotosLibraryInfo {
            path: norm_path,
            display_name,
            already_added,
            readable,
            needs_permission,
        });
    }

    // Prioritize "Photos Library.photoslibrary" first
    found.sort_by(|a, b| {
        let a_default = a.display_name.eq_ignore_ascii_case("Photos Library");
        let b_default = b.display_name.eq_ignore_ascii_case("Photos Library");
        b_default
            .cmp(&a_default)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    found
}

fn check_library_readability(bundle_path: &Path) -> (bool, bool) {
    let target = resolve_apple_photos_originals_dir(bundle_path).unwrap_or_else(|| bundle_path.to_path_buf());
    match std::fs::read_dir(&target) {
        Ok(mut iter) => {
            // Try stepping one entry to catch TCC permission errors surfaced during listing
            match iter.next() {
                Some(Err(e)) if e.kind() == std::io::ErrorKind::PermissionDenied => (false, true),
                _ => (true, false),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => (false, true),
        Err(_) => (false, false),
    }
}

pub fn core_data_sec_to_epoch_ms(core_data_secs: f64) -> Option<i64> {
    if !core_data_secs.is_finite() {
        return None;
    }
    let unix_secs = core_data_secs + CORE_DATA_EPOCH_OFFSET_SECS;
    if unix_secs <= 0.0 {
        return None;
    }
    Some((unix_secs * 1000.0).round() as i64)
}

/// Strictly read-only, immutable lookup against Apple Photos' SQLite catalog.
/// Never locks or creates -wal/-shm sidecar files next to Photos.sqlite.
pub fn load_apple_photos_metadata_map(bundle_path: &Path) -> HashMap<String, ApplePhotosMeta> {
    let db_path = bundle_path.join("database").join("Photos.sqlite");
    if !db_path.is_file() {
        return HashMap::new();
    }

    let uri = build_readonly_immutable_uri(&db_path);
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let Ok(conn) = Connection::open_with_flags(&uri, flags) else {
        return HashMap::new();
    };

    let sql = "
        SELECT
            LOWER(a.ZFILENAME),
            attr.ZORIGINALFILENAME,
            a.ZDATECREATED
        FROM ZASSET a
        LEFT JOIN ZADDITIONALASSETATTRIBUTES attr
            ON a.Z_PK = attr.ZASSET
        WHERE a.ZFILENAME IS NOT NULL
    ";

    let Ok(mut stmt) = conn.prepare(sql) else {
        return HashMap::new();
    };

    let Ok(rows) = stmt.query_map([], |row| {
        let zfilename: String = row.get(0)?;
        let original_filename: Option<String> = row.get(1)?;
        let zdate_created: Option<f64> = row.get(2)?;
        Ok((zfilename, original_filename, zdate_created))
    }) else {
        return HashMap::new();
    };

    let mut out = HashMap::new();
    for row in rows.flatten() {
        let (key, orig, zdate) = row;
        let clean_orig = orig.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let date_ms = zdate.and_then(core_data_sec_to_epoch_ms);
        if clean_orig.is_some() || date_ms.is_some() {
            out.insert(
                key,
                ApplePhotosMeta {
                    original_filename: clean_orig,
                    date_taken_epoch_ms: date_ms,
                },
            );
        }
    }
    out
}

fn build_readonly_immutable_uri(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let clean = raw.trim_start_matches("//?/");
    let encoded: String = clean
        .chars()
        .map(|c| match c {
            '?' => "%3F".to_string(),
            '#' => "%23".to_string(),
            '%' => "%25".to_string(),
            other => other.to_string(),
        })
        .collect();
    if encoded.starts_with('/') {
        format!("file:{encoded}?mode=ro&immutable=1")
    } else {
        format!("file:///{encoded}?mode=ro&immutable=1")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn apple_photos_bundle_and_originals_resolution() {
        let tmp = std::env::temp_dir().join(format!("pixrelay_ap_{}", Uuid::new_v4()));
        let lib = tmp.join("Photos Library.photoslibrary");
        let orig = lib.join("originals").join("A");
        fs::create_dir_all(&orig).unwrap();

        assert!(is_apple_photos_library(&lib));
        assert!(is_apple_photos_library(&orig));
        assert!(!is_apple_photos_library(&tmp));

        assert_eq!(
            resolve_apple_photos_originals_dir(&lib).unwrap(),
            lib.join("originals")
        );

        // Older Masters fallback
        let legacy_lib = tmp.join("Legacy.photoslibrary");
        fs::create_dir_all(legacy_lib.join("Masters")).unwrap();
        assert_eq!(
            resolve_apple_photos_originals_dir(&legacy_lib).unwrap(),
            legacy_lib.join("Masters")
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn loads_synthetic_photos_sqlite_read_only_and_handles_missing_db() {
        let tmp = std::env::temp_dir().join(format!("pixrelay_apsql_{}", Uuid::new_v4()));
        let lib = tmp.join("Photos Library.photoslibrary");
        let db_dir = lib.join("database");
        fs::create_dir_all(&db_dir).unwrap();

        // Missing DB gracefully returns empty map
        assert!(load_apple_photos_metadata_map(&lib).is_empty());

        let sqlite_path = db_dir.join("Photos.sqlite");
        {
            let conn = Connection::open(&sqlite_path).unwrap();
            conn.execute_batch(
                "
                CREATE TABLE ZASSET (Z_PK INTEGER PRIMARY KEY, ZFILENAME TEXT, ZDATECREATED REAL);
                CREATE TABLE ZADDITIONALASSETATTRIBUTES (Z_PK INTEGER PRIMARY KEY, ZASSET INTEGER, ZORIGINALFILENAME TEXT);
                INSERT INTO ZASSET (Z_PK, ZFILENAME, ZDATECREATED) VALUES (1, '1A2B3C4D-1111.heic', 757382400.0);
                INSERT INTO ZADDITIONALASSETATTRIBUTES (Z_PK, ZASSET, ZORIGINALFILENAME) VALUES (10, 1, 'IMG_4242.HEIC');
                ",
            )
            .unwrap();
        }

        let map = load_apple_photos_metadata_map(&lib);
        let meta = map.get("1a2b3c4d-1111.heic").expect("metadata row loaded");
        assert_eq!(meta.original_filename.as_deref(), Some("IMG_4242.HEIC"));
        // Core Data 757382400s + 978307200s = 1735689600s (2025-01-01T00:00:00Z)
        assert_eq!(meta.date_taken_epoch_ms, Some(1_735_689_600_000));

        // Verify immutable read-only open created no WAL/SHM sidecar files
        assert!(!db_dir.join("Photos.sqlite-wal").exists());
        assert!(!db_dir.join("Photos.sqlite-shm").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detects_libraries_in_pictures_dir() {
        let tmp = std::env::temp_dir().join(format!("pixrelay_apdet_{}", Uuid::new_v4()));
        let pics = tmp.join("Pictures");
        let lib_a = pics.join("Vacation.photoslibrary");
        let lib_main = pics.join("Photos Library.photoslibrary");
        fs::create_dir_all(lib_a.join("originals")).unwrap();
        fs::create_dir_all(lib_main.join("originals")).unwrap();

        let db = Database::open_path(&tmp.join("mac.db")).unwrap();
        db.add_watched_folder(&lib_a.to_string_lossy(), "Vacation", crate::db::FolderPolicy::Auto, true)
            .unwrap();

        let detected = detect_apple_photos_libraries_in_dir(&pics, &db);
        assert_eq!(detected.len(), 2);
        // Primary "Photos Library" comes first
        assert_eq!(detected[0].display_name, "Photos Library");
        assert!(!detected[0].already_added);
        assert!(detected[0].readable);

        assert_eq!(detected[1].display_name, "Vacation");
        assert!(detected[1].already_added);

        let _ = fs::remove_dir_all(&tmp);
    }
}
