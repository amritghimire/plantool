use crate::registry::Registry;
use notify::{RecursiveMode, Watcher};
use plantool_core::DocKind;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct WatchHandle {
    _watcher: Option<notify::RecommendedWatcher>,
}

pub fn start(registry: Arc<Registry>, home: PathBuf) -> WatchHandle {
    let sessions_dir = crate::store::sessions_dir(&home);
    let _ = std::fs::create_dir_all(&sessions_dir);
    let pending: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));

    let pending_for_watcher = pending.clone();
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            let mut p = pending_for_watcher.lock().unwrap_or_else(|e| e.into_inner());
            for path in ev.paths {
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    p.insert(path);
                }
            }
        }
    })
    .ok()
    .and_then(|mut w| w.watch(&sessions_dir, RecursiveMode::Recursive).ok().map(|_| w));
    if watcher.is_none() {
        tracing::warn!("file watcher unavailable; relying on polling");
    }

    let reg = registry.clone();
    let pending_for_flush = pending.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let paths: Vec<PathBuf> = {
                let mut p = pending_for_flush.lock().unwrap_or_else(|e| e.into_inner());
                p.drain().collect()
            };
            for path in paths {
                if let Some((s, kind)) = reg.capture_path(&path) {
                    if let Err(e) = s.capture_doc(kind) {
                        tracing::warn!("capture {}: {e}", path.display());
                    }
                }
            }
        }
    });

    let reg = registry;
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            for s in reg.all() {
                for kind in DocKind::ALL {
                    let _ = s.capture_doc(kind);
                }
            }
        }
    });

    WatchHandle { _watcher: watcher }
}
