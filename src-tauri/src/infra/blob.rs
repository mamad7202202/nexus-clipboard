//! Content-addressed blob store for binary payloads (images, oversized text).
//!
//! Files are keyed by their BLAKE3 hash and sharded two levels deep so no
//! directory ever holds more than a few thousand entries — which matters once
//! the history reaches millions of items. Because the key *is* the content
//! hash, identical screenshots copied a hundred times occupy one file.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/ab/cd/abcd...bin`
    fn path_for(&self, key: &str) -> Result<PathBuf> {
        // Keys come from our own hashing, but this is the one place a bad key
        // could escape the store directory, so validate defensively.
        if key.len() < 4 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::invalid("malformed blob key"));
        }
        Ok(self
            .root
            .join(&key[0..2])
            .join(&key[2..4])
            .join(format!("{key}.bin")))
    }

    /// Store `data` and return its key. Writing the same content twice is a
    /// no-op, so callers never need to check first.
    pub fn put(&self, data: &[u8]) -> Result<String> {
        let key = crate::util::hash_bytes(data);
        let path = self.path_for(&key)?;

        if path.exists() {
            return Ok(key);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write to a temporary sibling and rename, so a crash mid-write can
        // never leave a truncated blob under a valid key.
        let tmp = path.with_extension("tmp");
        {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(data)?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &path)?;

        Ok(key)
    }

    pub fn get(&self, key: &str) -> Result<Vec<u8>> {
        let path = self.path_for(key)?;
        if !path.exists() {
            return Err(Error::NotFound);
        }
        Ok(fs::read(path)?)
    }

    pub fn exists(&self, key: &str) -> bool {
        self.path_for(key).map(|p| p.exists()).unwrap_or(false)
    }

    pub fn size_of(&self, key: &str) -> Result<u64> {
        let path = self.path_for(key)?;
        Ok(fs::metadata(path)?.len())
    }

    pub fn remove(&self, key: &str) -> Result<()> {
        let path = self.path_for(key)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Total bytes held by the store.
    pub fn total_bytes(&self) -> Result<u64> {
        let mut total = 0u64;
        for entry in walk(&self.root)? {
            total += entry.1;
        }
        Ok(total)
    }

    /// Delete every blob not present in `referenced`. Returns how many files
    /// were removed and how many bytes that reclaimed.
    pub fn gc(&self, referenced: &[String]) -> Result<(usize, u64)> {
        let live: std::collections::HashSet<&str> =
            referenced.iter().map(|s| s.as_str()).collect();

        let mut removed = 0usize;
        let mut freed = 0u64;

        for (path, size) in walk(&self.root)? {
            let key = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if key.is_empty() || live.contains(key.as_str()) {
                continue;
            }
            if fs::remove_file(&path).is_ok() {
                removed += 1;
                freed += size;
            }
        }

        Ok((removed, freed))
    }
}

/// Recursively collect `(path, size)` for every `.bin` file under `root`.
fn walk(root: &Path) -> Result<Vec<(PathBuf, u64)>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            // A directory disappearing under us during GC is benign.
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "bin") {
                out.push((path, meta.len()));
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (BlobStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("nexus-blob-{}", uuid::Uuid::new_v4()));
        (BlobStore::new(&dir).unwrap(), dir)
    }

    #[test]
    fn put_get_round_trip() {
        let (store, dir) = temp_store();
        let key = store.put(b"hello blob").unwrap();
        assert_eq!(store.get(&key).unwrap(), b"hello blob");
        assert!(store.exists(&key));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn identical_content_shares_one_file() {
        let (store, dir) = temp_store();
        let a = store.put(b"same").unwrap();
        let b = store.put(b"same").unwrap();
        assert_eq!(a, b);
        assert_eq!(walk(store.root()).unwrap().len(), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn gc_removes_only_unreferenced() {
        let (store, dir) = temp_store();
        let keep = store.put(b"keep me").unwrap();
        let drop = store.put(b"drop me").unwrap();

        let (removed, _) = store.gc(&[keep.clone()]).unwrap();
        assert_eq!(removed, 1);
        assert!(store.exists(&keep));
        assert!(!store.exists(&drop));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_path_traversal_keys() {
        let (store, dir) = temp_store();
        assert!(store.get("../../etc/passwd").is_err());
        assert!(store.get("..").is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_blob_is_not_found() {
        let (store, dir) = temp_store();
        let err = store.get(&"a".repeat(64)).unwrap_err();
        assert!(matches!(err, Error::NotFound));
        fs::remove_dir_all(dir).ok();
    }
}
