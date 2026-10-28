use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::models::lyrics::{LyricsCacheStats, LyricsDoc};

const MEMORY_CAPACITY: usize = 64;
const POSITIVE_TTL_SECS: u64 = 30 * 24 * 3_600;
const NEGATIVE_TTL_SECS: u64 = 24 * 3_600;
const DISK_BUDGET_BYTES: u64 = 50 * 1024 * 1024;
const DISK_MAX_FILES: usize = 5_000;

/// LRU mínimo: 64 entradas, evicción O(n) sobre 64 elementos = irrelevante.
struct TinyLru<V> {
    map: HashMap<String, (Arc<V>, u64)>,
    tick: u64,
    capacity: usize,
}

impl<V> TinyLru<V> {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            tick: 0,
            capacity: capacity.max(1),
        }
    }

    fn get(&mut self, key: &str) -> Option<Arc<V>> {
        self.tick += 1;
        let (value, stamp) = self.map.get_mut(key)?;
        *stamp = self.tick;
        Some(Arc::clone(value))
    }

    fn put(&mut self, key: String, value: Arc<V>) {
        self.tick += 1;
        if self.map.len() >= self.capacity && !self.map.contains_key(&key) {
            if let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, (_, stamp))| *stamp)
                .map(|(key, _)| key.clone())
            {
                self.map.remove(&oldest);
            }
        }
        self.map.insert(key, (value, self.tick));
    }

    fn len(&self) -> usize {
        self.map.len()
    }
}

#[derive(Serialize, Deserialize)]
struct DiskEntry {
    schema: u8,
    key: String,
    stored_at: u64,
    doc: LyricsDoc,
}

impl DiskEntry {
    fn ttl(&self) -> u64 {
        if self.doc.source == crate::models::lyrics::LyricsSource::None {
            NEGATIVE_TTL_SECS
        } else {
            POSITIVE_TTL_SECS
        }
    }

    fn is_fresh(&self) -> bool {
        now_secs().saturating_sub(self.stored_at) < self.ttl()
    }
}

pub struct LyricsCache {
    memory: Mutex<TinyLru<LyricsDoc>>,
    dir: PathBuf,
}

impl LyricsCache {
    pub fn new() -> Self {
        let dir = cache_root().join("lyrics");
        let _ = std::fs::create_dir_all(&dir);
        Self {
            memory: Mutex::new(TinyLru::new(MEMORY_CAPACITY)),
            dir,
        }
    }

    /// Lee memoria y, si falla, disco.
    pub async fn get(&self, key: &str) -> Option<Arc<LyricsDoc>> {
        if let Ok(mut memory) = self.memory.lock() {
            if let Some(hit) = memory.get(key) {
                return Some(hit);
            }
        }

        let path = self.path_for(key);
        let key_owned = key.to_string();
        let entry = tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(&path).ok()?;
            serde_json::from_slice::<DiskEntry>(&bytes).ok()
        })
        .await
        .ok()??;

        if entry.key != key_owned || !entry.is_fresh() {
            let _ = tokio::fs::remove_file(self.path_for(key)).await;
            return None;
        }

        let doc = Arc::new(entry.doc);
        if let Ok(mut memory) = self.memory.lock() {
            memory.put(key_owned, Arc::clone(&doc));
        }
        Some(doc)
    }

    pub async fn put(&self, key: String, doc: Arc<LyricsDoc>) {
        if let Ok(mut memory) = self.memory.lock() {
            memory.put(key.clone(), Arc::clone(&doc));
        }

        let path = self.path_for(&key);
        let entry = DiskEntry {
            schema: 1,
            key,
            stored_at: now_secs(),
            doc: (*doc).clone(),
        };

        let _ = tokio::task::spawn_blocking(move || {
            let Ok(bytes) = serde_json::to_vec(&entry) else {
                return;
            };
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        })
        .await;
    }

    pub async fn stats(&self) -> LyricsCacheStats {
        let memory_entries = self.memory.lock().map(|m| m.len()).unwrap_or(0);
        let dir = self.dir.clone();
        let (disk_entries, disk_bytes) = tokio::task::spawn_blocking(move || scan_dir(&dir))
            .await
            .unwrap_or((0, 0));
        LyricsCacheStats {
            memory_entries,
            disk_entries,
            disk_bytes,
        }
    }

    pub async fn clear(&self) -> u64 {
        if let Ok(mut memory) = self.memory.lock() {
            *memory = TinyLru::new(MEMORY_CAPACITY);
        }
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || {
            let mut removed = 0u64;
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if entry.path().extension().and_then(|e| e.to_str()) == Some("json")
                        && std::fs::remove_file(entry.path()).is_ok()
                    {
                        removed += 1;
                    }
                }
            }
            removed
        })
        .await
        .unwrap_or(0)
    }

    pub fn spawn_prune(&self) {
        let dir = self.dir.clone();
        std::thread::spawn(move || prune(&dir));
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{:016x}.json", fnv1a64(key.as_bytes())))
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn scan_dir(dir: &Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    files += 1;
                    bytes += meta.len();
                }
            }
        }
    }
    (files, bytes)
}

fn prune(dir: &Path) {
    let mut entries: Vec<(PathBuf, u64, u64)> = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(dir) else { return };

    for entry in read_dir.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        entries.push((path, modified, meta.len()));
    }

    let now = now_secs();
    for (path, modified, _) in &entries {
        if now.saturating_sub(*modified) > POSITIVE_TTL_SECS {
            let _ = std::fs::remove_file(path);
        }
    }

    entries.retain(|(path, _, _)| path.exists());
    entries.sort_by_key(|(_, modified, _)| *modified);

    let mut total: u64 = entries.iter().map(|(_, _, len)| *len).sum();
    let mut count = entries.len();
    for (path, _, len) in entries {
        if total <= DISK_BUDGET_BYTES && count <= DISK_MAX_FILES {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
            count = count.saturating_sub(1);
        }
    }
}

fn cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(dir).join("cadence");
    }

    #[cfg(target_os = "windows")]
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(dir).join("Cadence").join("cache");
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("com.xalejo.cadence");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join("cadence");
    }

    std::env::temp_dir().join("cadence-cache")
}
