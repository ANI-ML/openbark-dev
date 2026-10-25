//! This device's identity in its visits folder (ADR-0017 sections 6 and 9).
//!
//! A random UUIDv7, made the first time it's needed and kept in
//! `<visits>/.openbark/device-id`. Its short tag (4 hex characters of its
//! SHA-256) goes into run folder names (`runs/004_a3f9_<model>`), so drafts
//! made offline on two devices never collide, and into clock timestamps
//! ([`crate::hlc`]). The id isn't secret and never leaves the device except
//! inside encrypted sync objects.

use std::path::Path;

use sha2::{Digest, Sha256};

const FILE: &str = "device-id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// UUIDv7.
    pub id: String,
    /// 4 lowercase hex characters.
    pub tag: String,
}

impl Device {
    pub fn from_id(id: &str) -> Self {
        let hash = Sha256::digest(id.as_bytes());
        Self { id: id.to_string(), tag: hex::encode(&hash[..2]) }
    }
}

/// The device for the visits folder `root`, created on first use. If the
/// folder can't be written, a fresh id for this run only (runs still get a
/// tag; it just isn't remembered).
pub fn for_root(root: &Path) -> Device {
    let dir = root.join(crate::HOUSEKEEPING_DIR);
    let path = dir.join(FILE);
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(id) = uuid::Uuid::parse_str(text.trim())
    {
        return Device::from_id(&id.to_string());
    }
    let id = uuid::Uuid::now_v7().to_string();
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &id)) {
        tracing::warn!(error = %e, "couldn't save this device's id; run folders get a one-off tag");
    }
    Device::from_id(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let a = for_root(dir.path());
        let b = for_root(dir.path());
        assert_eq!(a, b);
        assert_eq!(a.tag.len(), 4);
        assert!(a.tag.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        let other = tempfile::tempdir().unwrap();
        assert_ne!(for_root(other.path()).id, a.id);
        assert_eq!(Device::from_id(&a.id).tag, a.tag);
    }
}
