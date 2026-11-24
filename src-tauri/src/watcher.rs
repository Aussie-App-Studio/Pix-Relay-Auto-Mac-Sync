use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::apple_photos::{is_apple_photos_library, resolve_apple_photos_originals_dir};
use crate::db::{Database, FolderPolicy};

pub struct FolderWatcherHandle {
    watcher: Mutex<Option<RecommendedWatcher>>,
    refresh_tx: mpsc::UnboundedSender<()>,
}

impl FolderWatcherHandle {
    pub fn start<F>(db: Database, on_debounced_change: F) -> Arc<Self>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel::<()>();
        let handle = Arc::new(Self {
            watcher: Mutex::new(None),
            refresh_tx: tx.clone(),
        });

        let cb = Arc::new(on_debounced_change);
        let cb_c = cb.clone();
        tauri::async_runtime::spawn(async move {
            while rx.recv().await.is_some() {
                // Coalesce bursty file writes (e.g. camera card or Photos import) over 3 seconds
                loop {
                    match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
                        Ok(Some(())) => continue,
                        _ => break,
                    }
                }
                cb_c();
            }
        });

        handle.reload_watches(&db);
        handle
    }

    pub fn reload_watches(&self, db: &Database) {
        let folders = db.list_watched_folders().unwrap_or_default();
        let tx = self.refresh_tx.clone();
        let Ok(mut watcher) = RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    if !matches!(ev.kind, notify::EventKind::Access(_)) {
                        let _ = tx.send(());
                    }
                }
            },
            Config::default(),
        ) else {
            return;
        };

        for f in folders {
            if f.policy != FolderPolicy::Auto {
                continue;
            }
            let path = Path::new(&f.path);
            if !path.exists() || !path.is_dir() {
                continue;
            }
            if is_apple_photos_library(path) {
                if let Some(orig) = resolve_apple_photos_originals_dir(path) {
                    let _ = watcher.watch(&orig, RecursiveMode::Recursive);
                }
            } else {
                let mode = if f.recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };
                let _ = watcher.watch(path, mode);
            }
        }

        *self.watcher.lock().unwrap() = Some(watcher);
    }
}
