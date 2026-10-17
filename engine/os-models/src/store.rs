use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::catalog::{CatalogEntry, FileSpec};

/// How thoroughly to check an installed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// Size only (fast; used on every run).
    Size,
    /// Size and SHA-256 (used after download and by `models verify`).
    Full,
}

/// Where models live on disk: `<root>/<model-id>/<file>`.
#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
}

impl ModelStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `OPENBARK_MODELS_DIR`, else the platform data directory.
    pub fn default_location() -> Self {
        if let Some(dir) = std::env::var_os("OPENBARK_MODELS_DIR") {
            return Self::new(dir);
        }
        let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
        Self::new(base.join("OpenBark").join("models"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn model_dir(&self, entry: &CatalogEntry) -> PathBuf {
        self.root.join(&entry.id)
    }

    pub fn file_path(&self, entry: &CatalogEntry, name: &str) -> PathBuf {
        self.model_dir(entry).join(name)
    }

    pub fn file_ok(&self, entry: &CatalogEntry, file: &FileSpec, verify: Verify) -> bool {
        let path = self.file_path(entry, &file.path);
        let Ok(meta) = std::fs::metadata(&path) else { return false };
        if meta.len() != file.size {
            return false;
        }
        verify == Verify::Size || sha256_file(&path).is_ok_and(|h| h.eq_ignore_ascii_case(&file.sha256))
    }

    pub fn is_installed(&self, entry: &CatalogEntry, verify: Verify) -> bool {
        entry.files.iter().all(|f| self.file_ok(entry, f, verify))
    }
}

pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut reader = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
