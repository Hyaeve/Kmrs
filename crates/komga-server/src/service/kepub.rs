//! `KepubConverter.kt`: kepubify availability (the `komga.kobo.kepubify-path` config
//! property, probed) and the 5-minute cache of converted files (`KoboController.cachedKepub`).

use crate::config::ServerConfig;
use komga_media::kepubify;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

pub struct KepubConverter {
    /// probe results by path string; a path is only probed once, like `configureKepubify`
    probed: RwLock<HashMap<String, Option<PathBuf>>>,
    cache: Mutex<HashMap<String, CacheEntry>>,
    /// converted files live in our own temp subdirectory so stale ones can be cleaned at
    /// startup (the Java side relies on `deleteOnExit` instead)
    tmp_dir: PathBuf,
}

struct CacheEntry {
    path: PathBuf,
    last_access: Instant,
}

/// Default converted-files directory; stale files are cleaned at startup (the Java side
/// relies on `deleteOnExit` instead).
pub fn default_tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("kmrs-kepub");
    let _ = std::fs::create_dir_all(&dir);
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
    dir
}

impl KepubConverter {
    pub fn new(tmp_dir: PathBuf) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&tmp_dir);
        Arc::new(Self {
            probed: RwLock::new(HashMap::new()),
            cache: Mutex::new(HashMap::new()),
            tmp_dir,
        })
    }

    /// Effective kepubify path: the `komga.kobo.kepubify-path` config property, probed
    /// (`configureKepubify`).
    pub fn kepubify_path(&self, config: &ServerConfig) -> Option<PathBuf> {
        config
            .kepubify_path
            .as_deref()
            .and_then(|p| self.probe(&p.to_string_lossy()))
    }

    pub fn is_available(&self, config: &ServerConfig) -> bool {
        self.kepubify_path(config).is_some()
    }

    fn probe(&self, path: &str) -> Option<PathBuf> {
        if let Some(cached) = self.probed.read().unwrap().get(path) {
            return cached.clone();
        }
        let result = kepubify::probe(path);
        self.probed
            .write()
            .unwrap()
            .insert(path.to_string(), result.clone());
        result
    }

    /// `cachedKepub`: Caffeine `expireAfterAccess(5 min)`; evicted temp files are deleted.
    /// A cached path that no longer exists is treated as a miss.
    pub fn cached_or_convert(
        &self,
        key: &str,
        convert: impl FnOnce(&Path) -> Option<PathBuf>,
    ) -> Option<PathBuf> {
        let mut cache = self.cache.lock().unwrap();
        let expired: Vec<String> = cache
            .iter()
            .filter(|(_, e)| e.last_access.elapsed() > CACHE_TTL)
            .map(|(k, _)| k.clone())
            .collect();
        for k in expired {
            if let Some(e) = cache.remove(&k) {
                let _ = std::fs::remove_file(&e.path);
            }
        }
        if let Some(entry) = cache.get_mut(key) {
            if entry.path.is_file() {
                entry.last_access = Instant::now();
                return Some(entry.path.clone());
            }
        }
        let converted = convert(&self.tmp_dir)?;
        cache.insert(
            key.to_string(),
            CacheEntry {
                path: converted.clone(),
                last_access: Instant::now(),
            },
        );
        Some(converted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ServerConfig {
        ServerConfig::from_env()
    }

    fn executable_script(path: &Path, content: &str) {
        std::fs::write(path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn unavailable_without_any_path() {
        let converter = KepubConverter::new(tempfile::tempdir().unwrap().keep());
        assert!(!converter.is_available(&config()));
    }

    #[test]
    fn config_path_is_probed() {
        let dir = tempfile::tempdir().unwrap();
        let valid = dir.path().join("kepubify");
        executable_script(&valid, "#!/bin/sh\nexit 0\n");

        let converter = KepubConverter::new(tempfile::tempdir().unwrap().keep());
        let mut config = config();
        config.kepubify_path = Some(valid.clone());
        assert_eq!(converter.kepubify_path(&config), Some(valid));
        assert!(converter.is_available(&config));

        config.kepubify_path = Some(PathBuf::from("/does/not/exist"));
        assert_eq!(converter.kepubify_path(&config), None);
    }

    #[test]
    fn cached_or_convert_reuses_and_expires() {
        let dir = tempfile::tempdir().unwrap();
        let converter = KepubConverter::new(dir.path().to_path_buf());
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let make = |tmp: &Path| {
            let n = calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = tmp.join(format!("book-{n}.kepub.epub"));
            std::fs::write(&path, b"x").unwrap();
            Some(path)
        };
        let calls = || calls.load(std::sync::atomic::Ordering::Relaxed);
        let first = converter.cached_or_convert("k1", make).unwrap();
        assert_eq!(calls(), 1);
        let second = converter.cached_or_convert("k1", make).unwrap();
        assert_eq!(first, second);
        assert_eq!(calls(), 1, "cache hit skips conversion");

        // a deleted cached file is a miss
        std::fs::remove_file(&first).unwrap();
        let third = converter.cached_or_convert("k1", make).unwrap();
        assert_eq!(calls(), 2);
        assert!(third.exists());

        // expiry evicts and deletes the file
        {
            let mut cache = converter.cache.lock().unwrap();
            cache.get_mut("k1").unwrap().last_access =
                Instant::now() - CACHE_TTL - Duration::from_secs(1);
        }
        let fourth = converter.cached_or_convert("k2", make).unwrap();
        assert!(!third.exists(), "expired temp file was deleted");
        assert!(fourth.exists());
        let _ = dir;
    }
}
