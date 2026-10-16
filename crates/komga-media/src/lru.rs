//! LRU cache primitive shared by the container handle caches (pdf/zip/rar).
//!
//! Why a hand-rolled one: the caches hold native handles whose count must stay bounded,
//! eviction order is the only contract, and the whole thing is fifty lines — a crate
//! dependency buys nothing here.

use crate::error::MediaError;
use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct Lru<K, V> {
    entries: HashMap<K, Entry<V>>,
    tick: u64,
    cap: usize,
}

struct Entry<V> {
    value: V,
    last_use: u64,
}

impl<K: Eq + Hash, V> Lru<K, V> {
    pub fn new(cap: usize) -> Self {
        Lru {
            entries: HashMap::new(),
            tick: 0,
            cap,
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.entries.get_mut(key).map(|entry| {
            self.tick += 1;
            entry.last_use = self.tick;
            &entry.value
        })
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries.get_mut(key).map(|entry| {
            self.tick += 1;
            entry.last_use = self.tick;
            &mut entry.value
        })
    }

    /// Returns the cached value, or inserts the freshly created one, evicting the
    /// least-recently-used entry when over capacity.
    pub fn get_or_insert_with<E>(
        &mut self,
        key: &K,
        create: impl FnOnce() -> std::result::Result<V, E>,
    ) -> std::result::Result<&V, E>
    where
        K: Clone,
    {
        if self.entries.contains_key(key) {
            self.tick += 1;
            let entry = self.entries.get_mut(key).expect("contains_key");
            entry.last_use = self.tick;
            return Ok(&entry.value);
        }
        let value = create()?;
        self.insert((*key).clone(), value);
        Ok(&self.entries.get(key).expect("just inserted").value)
    }

    pub fn insert(&mut self, key: K, value: V)
    where
        K: Clone,
    {
        if self.entries.len() >= self.cap {
            let evict = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_use)
                .map(|(key, _)| (*key).clone());
            if let Some(evict) = evict {
                self.entries.remove(&evict);
            }
        }
        self.tick += 1;
        self.entries.insert(
            key,
            Entry {
                value,
                last_use: self.tick,
            },
        );
    }

    /// Not used on Windows: the RAR handle cache is Unix-only, and it is the only
    /// caller that hands handles back out of the cache.
    #[cfg(any(unix, test))]
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.entries.remove(key).map(|entry| entry.value)
    }
}

/// Identity of a book file for caching: path plus size and mtime, so a replaced file
/// (different size or mtime) is a different key and reopens instead of serving the stale
/// handle. Same-size-same-mtime replacement within timestamp resolution is undetectable
/// by stat.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct FileKey {
    path: PathBuf,
    size: u64,
    mtime: Option<SystemTime>,
}

pub fn file_key(path: &Path) -> std::io::Result<FileKey> {
    let metadata = std::fs::metadata(path)?;
    Ok(FileKey {
        path: path.to_path_buf(),
        size: metadata.len(),
        mtime: metadata.modified().ok(),
    })
}

/// `file_key` does I/O that can fail; callers surface the same error a fresh open would.
pub fn key_error(path: &Path, e: std::io::Error) -> MediaError {
    match e.kind() {
        std::io::ErrorKind::NotFound => MediaError::NoSuchFile(path.display().to_string()),
        _ => MediaError::Other(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used() {
        let mut lru = Lru::new(2);
        lru.insert("a", 1);
        lru.insert("b", 2);
        assert_eq!(lru.get(&"a"), Some(&1));
        lru.insert("c", 3); // "b" untouched longest, evicted
        assert_eq!(lru.get(&"b"), None);
        assert_eq!(lru.get(&"a"), Some(&1));
        assert_eq!(lru.get(&"c"), Some(&3));
    }

    #[test]
    fn insert_replaces_existing_key() {
        let mut lru = Lru::new(2);
        lru.insert("a", 1);
        lru.insert("a", 2);
        assert_eq!(lru.get(&"a"), Some(&2));
        assert_eq!(lru.len(), 1);
    }

    #[test]
    fn get_or_insert_creates_once_and_caches() {
        let mut lru = Lru::new(2);
        let calls = std::cell::Cell::new(0);
        for _ in 0..2 {
            let value = lru
                .get_or_insert_with(&"a", || {
                    calls.set(calls.get() + 1);
                    Ok::<_, std::convert::Infallible>(7)
                })
                .unwrap();
            assert_eq!(*value, 7);
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn remove_returns_the_value() {
        let mut lru = Lru::new(2);
        lru.insert("a", 1);
        assert_eq!(lru.remove(&"a"), Some(1));
        assert!(lru.is_empty());
    }
}
