use std::collections::VecDeque;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::UNIX_EPOCH;

use image::codecs::jpeg::JpegEncoder;
use image::ColorType;
use percent_encoding::percent_decode_str;

use crate::db::Database;
use crate::protocol::crypto::sha256_hex;

const MAX_CACHE_BYTES: u64 = 250 * 1024 * 1024;

pub struct ThumbnailService {
    cache_dir: PathBuf,
    db: Database,
    pool: ThumbThreadPool,
}

impl ThumbnailService {
    pub fn new(db: Database) -> Self {
        let base = dirs::cache_dir()
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let cache_dir = base.join("Pix Relay Auto Mac Sync").join("thumb_cache");
        let _ = std::fs::create_dir_all(&cache_dir);
        let service = Self {
            cache_dir: cache_dir.clone(),
            db,
            pool: ThumbThreadPool::new(4),
        };
        std::thread::spawn(move || prune_cache_if_needed(&cache_dir));
        service
    }

    pub fn handle_protocol_uri(&self, uri_str: &str) -> (u16, &'static str, Vec<u8>) {
        let Some(raw_path) = parse_thumb_path_from_uri(uri_str) else {
            return (400, "text/plain", b"Bad thumbnail URI".to_vec());
        };
        let path = PathBuf::from(&raw_path);
        if !path.is_file() || !self.is_path_allowed(&path) {
            return (404, "image/svg+xml", fallback_svg().into_bytes());
        }

        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => return (404, "image/svg+xml", fallback_svg().into_bytes()),
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let key = sha256_hex(format!("{}|{}|{}", raw_path, mtime, meta.len()).as_bytes());
        let cache_file = self.cache_dir.join(format!("{key}.jpg"));

        if cache_file.exists() {
            if let Ok(bytes) = std::fs::read(&cache_file) {
                return (200, "image/jpeg", bytes);
            }
        }

        let path_c = path.clone();
        let cache_c = cache_file.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.pool.execute(move || {
            let generated = generate_thumbnail_jpeg(&path_c);
            if let Some(ref bytes) = generated {
                let _ = std::fs::write(&cache_c, bytes);
            }
            let _ = tx.send(generated);
        });

        match rx.recv() {
            Ok(Some(bytes)) => (200, "image/jpeg", bytes),
            _ => (200, "image/svg+xml", fallback_svg().into_bytes()),
        }
    }

    fn is_path_allowed(&self, path: &Path) -> bool {
        let Ok(canon) = std::fs::canonicalize(path) else {
            return false;
        };
        let folders = self.db.list_watched_folders().unwrap_or_default();
        for f in folders {
            if let Ok(root_canon) = std::fs::canonicalize(&f.path) {
                if canon.starts_with(&root_canon) {
                    return true;
                }
            }
        }
        false
    }
}

pub fn parse_thumb_path_from_uri(uri_str: &str) -> Option<String> {
    let encoded_part = if let Some(rest) = uri_str.strip_prefix("pixthumb://localhost/") {
        rest
    } else if let Some(rest) = uri_str.strip_prefix("http://pixthumb.localhost/") {
        rest
    } else if let Some(rest) = uri_str.strip_prefix("https://pixthumb.localhost/") {
        rest
    } else if let Some(rest) = uri_str.strip_prefix("pixthumb://") {
        rest.trim_start_matches("localhost/")
    } else {
        return None;
    };
    let clean = encoded_part.split('?').next().unwrap_or(encoded_part);
    let decoded = percent_decode_str(clean).decode_utf8().ok()?.to_string();
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn fallback_svg() -> String {
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128"><rect width="128" height="128" fill="#181D25"/><rect x="34" y="38" width="60" height="52" rx="8" fill="#282F3C"/><circle cx="50" cy="54" r="6" fill="#56C0FF"/><path d="M40 82 L56 64 L68 75 L79 61 L88 82 Z" fill="#56C0FF" opacity="0.7"/></svg>"##.to_string()
}

fn generate_thumbnail_jpeg(path: &Path) -> Option<Vec<u8>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let is_video = matches!(
        ext.as_str(),
        "mp4" | "mov" | "mkv" | "webm" | "3gp" | "m4v" | "avi"
    );
    let is_fast_image = matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "bmp");

    if is_fast_image {
        if let Some(bytes) = generate_image_crate_thumbnail(path, 256) {
            return Some(bytes);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if !is_video {
            if let Some(bytes) = generate_macos_sips_thumbnail(path, 256) {
                return Some(bytes);
            }
        }
        if let Some(bytes) = generate_macos_qlmanage_thumbnail(path, 256) {
            return Some(bytes);
        }
    }

    if !is_video {
        if let Some(bytes) = generate_image_crate_thumbnail(path, 256) {
            return Some(bytes);
        }
    }
    None
}

fn generate_image_crate_thumbnail(path: &Path, max_dim: u32) -> Option<Vec<u8>> {
    let img = image::open(path).ok()?;
    encode_dynamic_image_to_jpeg(img, max_dim, 80)
}

fn encode_dynamic_image_to_jpeg(
    img: image::DynamicImage,
    max_dim: u32,
    quality: u8,
) -> Option<Vec<u8>> {
    let thumb = img.thumbnail(max_dim, max_dim).to_rgb8();
    let mut out = Vec::new();
    let mut writer = BufWriter::new(&mut out);
    let encoder = JpegEncoder::new_with_quality(&mut writer, quality);
    image::ImageEncoder::write_image(
        encoder,
        thumb.as_raw(),
        thumb.width(),
        thumb.height(),
        ColorType::Rgb8.into(),
    )
    .ok()?;
    drop(writer);
    Some(out)
}

#[cfg(target_os = "macos")]
struct TempDirGuard(PathBuf);

#[cfg(target_os = "macos")]
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(target_os = "macos")]
fn generate_macos_sips_thumbnail(path: &Path, max_dim: u32) -> Option<Vec<u8>> {
    let tmp_dir = std::env::temp_dir().join(format!("pixthumb_sips_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_dir).ok()?;
    let _guard = TempDirGuard(tmp_dir.clone());
    let out_jpg = tmp_dir.join("thumb.jpg");

    let status = std::process::Command::new("/usr/bin/sips")
        .args(["-Z", &max_dim.to_string(), "-s", "format", "jpeg", "-s", "formatOptions", "80"])
        .arg(path)
        .arg("--out")
        .arg(&out_jpg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if !status.success() || !out_jpg.is_file() {
        return None;
    }
    std::fs::read(&out_jpg).ok().filter(|b| !b.is_empty())
}

#[cfg(target_os = "macos")]
fn generate_macos_qlmanage_thumbnail(path: &Path, max_dim: u32) -> Option<Vec<u8>> {
    let tmp_dir = std::env::temp_dir().join(format!("pixthumb_ql_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_dir).ok()?;
    let _guard = TempDirGuard(tmp_dir.clone());

    let status = std::process::Command::new("/usr/bin/qlmanage")
        .args(["-t", "-s", &max_dim.to_string(), "-o"])
        .arg(&tmp_dir)
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if !status.success() {
        return None;
    }

    let png_path = std::fs::read_dir(&tmp_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("png"))?;

    let img = image::open(&png_path).ok()?;
    encode_dynamic_image_to_jpeg(img, max_dim, 80)
}

fn prune_cache_if_needed(cache_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let mut total = 0u64;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                let len = meta.len();
                total += len;
                let modified = meta.modified().unwrap_or(UNIX_EPOCH);
                files.push((entry.path(), len, modified));
            }
        }
    }
    if total <= MAX_CACHE_BYTES {
        return;
    }
    files.sort_by_key(|f| f.2);
    for (p, len, _) in files {
        if total <= MAX_CACHE_BYTES * 8 / 10 {
            break;
        }
        if std::fs::remove_file(p).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

struct ThumbThreadPool {
    queue: Arc<(Mutex<VecDeque<Box<dyn FnOnce() + Send>>>, Condvar)>,
}

impl ThumbThreadPool {
    fn new(threads: usize) -> Self {
        let queue = Arc::new((
            Mutex::new(VecDeque::<Box<dyn FnOnce() + Send>>::new()),
            Condvar::new(),
        ));
        for i in 0..threads {
            let q = queue.clone();
            std::thread::Builder::new()
                .name(format!("pixthumb-{i}"))
                .spawn(move || loop {
                    let task = {
                        let (lock, cv) = &*q;
                        let mut guard = lock.lock().unwrap();
                        while guard.is_empty() {
                            guard = cv.wait(guard).unwrap();
                        }
                        guard.pop_front()
                    };
                    if let Some(job) = task {
                        job();
                    }
                })
                .ok();
        }
        Self { queue }
    }

    fn execute<F: FnOnce() + Send + 'static>(&self, f: F) {
        let (lock, cv) = &*self.queue;
        let mut guard = lock.lock().unwrap();
        if guard.len() > 64 {
            guard.pop_front();
        }
        guard.push_back(Box::new(f));
        cv.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_macos_posix_and_apple_photos_thumb_uris() {
        let posix = "/Users/alice/Pictures/Photos Library.photoslibrary/originals/A/IMG_001.HEIC";
        let uri = format!(
            "pixthumb://localhost/{}",
            percent_encoding::utf8_percent_encode(posix, percent_encoding::NON_ALPHANUMERIC)
        );
        assert_eq!(parse_thumb_path_from_uri(&uri).as_deref(), Some(posix));

        let http_uri = "http://pixthumb.localhost/%2FUsers%2Falice%2FPictures%2Ftrip.jpg?v=1";
        assert_eq!(
            parse_thumb_path_from_uri(http_uri).as_deref(),
            Some("/Users/alice/Pictures/trip.jpg")
        );
    }
}
