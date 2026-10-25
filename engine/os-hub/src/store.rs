//! The hub's store: what the Supabase tables held (ADR-0017 section 8),
//! as files in the hub's folder, every one sealed under the hub
//! computer's data key by the vault (ADR-0008).
//!
//! - `index.json`: accounts, devices, key envelopes, spaces, space keys,
//!   object and blob-part metadata, and the join codes' keyed hashes. Small,
//!   rewritten whole (temp file and rename) after every change.
//! - `objects/<space>/<object>.v<version>.obs`: one sealed OBS1 object body.
//! - `blobs/<space>/<object>.v<version>.p<part>.obb`: one sealed OBB1 part.
//!
//! Nothing here can be opened by the hub: the bodies, envelopes, wrapped
//! keys and the roster arrive sealed on the devices, under keys the hub
//! never holds. The vault adds encryption at rest on top.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::HubError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub amk_version: i64,
    pub kit_pub: String,
    pub kit_verifier: String,
    pub roster: String,
    pub roster_version: i64,
    #[serde(default)]
    pub rotate_needed: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    /// None while a new device waits in the hub's lobby to be approved.
    #[serde(default)]
    pub account: Option<String>,
    /// pending, approved or revoked.
    pub status: String,
    pub platform: String,
    pub x25519_pub: String,
    pub ed25519_pub: String,
    pub rebind_verifier: String,
    #[serde(default)]
    pub pair_mac: Option<String>,
    #[serde(default)]
    pub pair_blob: Option<String>,
    /// The hub's lookup tag for a pending device, derived from its 6 words.
    #[serde(default)]
    pub pair_tag: Option<String>,
    #[serde(default)]
    pub pending_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub approved_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub revoked_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_seen_at: Option<DateTime<Utc>>,
    /// The client address (or IPv6 /64) a lobby device asked from: each
    /// address may keep only a few waiting at once.
    #[serde(default)]
    pub lobby_peer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub account: String,
    /// A device id, or "kit".
    pub recipient: String,
    #[serde(default)]
    pub device_id: Option<String>,
    pub amk_version: i64,
    pub envelope: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Space {
    pub owner: String,
    pub key_version: i64,
    pub seq: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceKey {
    pub space: String,
    pub version: i64,
    pub amk_version: i64,
    pub wrapped: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Obj {
    pub kind: String,
    pub version: i64,
    pub seq: i64,
    #[serde(default)]
    pub wrapped_dek: Option<String>,
    #[serde(default)]
    pub key_version: Option<i64>,
    #[serde(default)]
    pub padded_size: Option<i64>,
    #[serde(default)]
    pub deleted: bool,
    /// The body's size in bytes (0 when deleted), for the quota.
    #[serde(default)]
    pub size: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobRec {
    pub size: u64,
    pub created_at: DateTime<Utc>,
}

/// A join code, as the hub keeps it: a keyed hash, never the code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeRec {
    pub hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Index {
    #[serde(default)]
    pub format: u32,
    /// The key join codes are hashed under (HMAC-SHA256), random per hub.
    #[serde(default)]
    pub code_key: String,
    #[serde(default)]
    pub codes: Vec<CodeRec>,
    #[serde(default)]
    pub accounts: BTreeMap<String, Account>,
    #[serde(default)]
    pub devices: BTreeMap<String, Device>,
    #[serde(default)]
    pub envelopes: Vec<Envelope>,
    #[serde(default)]
    pub spaces: BTreeMap<String, Space>,
    #[serde(default)]
    pub space_keys: Vec<SpaceKey>,
    /// Space id, then object id.
    #[serde(default)]
    pub objects: BTreeMap<String, BTreeMap<String, Obj>>,
    /// Space id, then `object/version/part`.
    #[serde(default)]
    pub blobs: BTreeMap<String, BTreeMap<String, BlobRec>>,
}

/// The blob map's key for one part.
pub fn blob_key(object: &str, version: i64, part: i64) -> String {
    format!("{object}/{version}/{part}")
}

pub fn split_blob_key(key: &str) -> Option<(&str, i64, i64)> {
    let mut it = key.split('/');
    let (o, v, p) = (it.next()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    Some((o, v, p))
}

/// Bytes counted toward the quota for each object the index lists (its
/// metadata), besides its body.
pub const OBJECT_OVERHEAD: u64 = 512;

pub struct Store {
    dir: PathBuf,
    pub ix: Index,
    /// Per space: seq to object id (rebuilt on load).
    pub by_seq: HashMap<String, BTreeMap<i64, String>>,
    /// The index as last saved, kept as `index.json.bak` at the next save.
    last_saved: Option<Vec<u8>>,
    /// Files and folders no longer referenced, removed only once the index
    /// that no longer names them is safely on disk.
    garbage: Vec<PathBuf>,
}

fn io(e: impl std::fmt::Display) -> HubError {
    HubError::Io(e.to_string())
}

/// Write a sealed file, then flush it to the disk.
fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), HubError> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(io)?;
    }
    os_engine::vault::write(path, bytes).map_err(io)?;
    std::fs::OpenOptions::new().write(true).open(path).and_then(|f| f.sync_all()).map_err(io)
}

/// Flush a folder's entries (the renames) to the disk, where the system allows it.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

fn read_index(path: &Path) -> std::io::Result<(Index, Vec<u8>)> {
    let bytes = os_engine::vault::read(path)?;
    let ix = serde_json::from_slice::<Index>(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok((ix, bytes))
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self, HubError> {
        std::fs::create_dir_all(dir).map_err(io)?;
        let path = dir.join("index.json");
        let (ix, last_saved) = match read_index(&path) {
            Ok((ix, bytes)) => (ix, Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
                Index {
                    format: 1,
                    code_key: os_sync::keys::b64(&os_sync::keys::random32().map_err(io)?),
                    ..Default::default()
                },
                None,
            ),
            // A damaged index: the copy from the save before.
            Err(e) => match read_index(&dir.join("index.json.bak")) {
                Ok((ix, bytes)) => {
                    tracing::warn!(error = %e, "the hub's index was damaged; using the previous copy");
                    (ix, Some(bytes))
                }
                Err(_) => return Err(HubError::Io(format!("the hub's index is damaged: {e}"))),
            },
        };
        if os_sync::keys::unb64_32(&ix.code_key).is_err() {
            return Err(HubError::Io("the hub's join code key is damaged".into()));
        }
        let mut s =
            Self { dir: dir.to_path_buf(), ix, by_seq: HashMap::new(), last_saved, garbage: Vec::new() };
        s.reindex();
        if !path.is_file() {
            s.save()?;
        }
        Ok(s)
    }

    pub fn reindex(&mut self) {
        self.by_seq.clear();
        for (space, objs) in &self.ix.objects {
            let m = self.by_seq.entry(space.clone()).or_default();
            for (id, o) in objs {
                m.insert(o.seq, id.clone());
            }
        }
    }

    /// Write the index (the previous one kept as `index.json.bak`), flushed
    /// to the disk, then remove what it no longer names.
    pub fn save(&mut self) -> Result<(), HubError> {
        let bytes = serde_json::to_vec(&self.ix).map_err(io)?;
        if let Some(prev) = &self.last_saved {
            write_synced(&self.dir.join("index.json.bak"), prev)?;
        }
        write_synced(&self.dir.join("index.json"), &bytes)?;
        sync_dir(&self.dir);
        self.last_saved = Some(bytes);
        for p in std::mem::take(&mut self.garbage) {
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
        Ok(())
    }

    /// The number of objects an account's spaces list (deleted ones too,
    /// until purged).
    pub fn object_count(&self, account: &str) -> usize {
        self.ix
            .spaces
            .iter()
            .filter(|(_, s)| s.owner == account)
            .map(|(id, _)| self.ix.objects.get(id).map_or(0, |m| m.len()))
            .sum()
    }

    fn object_path(&self, space: &str, id: &str, version: i64) -> PathBuf {
        self.dir.join("objects").join(space).join(format!("{id}.v{version}.obs"))
    }

    fn blob_path(&self, space: &str, object: &str, version: i64, part: i64) -> PathBuf {
        self.dir.join("blobs").join(space).join(format!("{object}.v{version}.p{part}.obb"))
    }

    pub fn write_object(&self, space: &str, id: &str, version: i64, body: &[u8]) -> Result<(), HubError> {
        write_synced(&self.object_path(space, id, version), body)
    }

    pub fn read_object(&self, space: &str, id: &str, version: i64) -> Result<Vec<u8>, HubError> {
        os_engine::vault::read(&self.object_path(space, id, version)).map_err(|e| HubError::Io(e.to_string()))
    }

    /// Remove an object version's file once the next index save succeeds.
    pub fn remove_object(&mut self, space: &str, id: &str, version: i64) {
        let p = self.object_path(space, id, version);
        self.garbage.push(p);
    }

    pub fn write_blob(
        &self,
        space: &str,
        object: &str,
        version: i64,
        part: i64,
        body: &[u8],
    ) -> Result<(), HubError> {
        write_synced(&self.blob_path(space, object, version, part), body)
    }

    pub fn read_blob(&self, space: &str, object: &str, version: i64, part: i64) -> Result<Vec<u8>, HubError> {
        os_engine::vault::read(&self.blob_path(space, object, version, part))
            .map_err(|e| HubError::Io(e.to_string()))
    }

    /// Remove a blob part's file once the next index save succeeds.
    pub fn remove_blob(&mut self, space: &str, object: &str, version: i64, part: i64) {
        let p = self.blob_path(space, object, version, part);
        self.garbage.push(p);
    }

    /// Remove every part of `object` whose version is below `below` (all of
    /// them for `i64::MAX`).
    pub fn drop_blobs(&mut self, space: &str, object: &str, below: i64) {
        let Some(parts) = self.ix.blobs.get_mut(space) else { return };
        let gone: Vec<String> = parts
            .keys()
            .filter(|k| split_blob_key(k).is_some_and(|(o, v, _)| o == object && v < below))
            .cloned()
            .collect();
        for k in gone {
            parts.remove(&k);
            if let Some((o, v, p)) = split_blob_key(&k) {
                let path = self.dir.join("blobs").join(space).join(format!("{o}.v{v}.p{p}.obb"));
                self.garbage.push(path);
            }
        }
    }

    /// Bytes an account holds: its live object bodies and blob parts.
    pub fn used_bytes(&self, account: &str) -> u64 {
        let mut total = 0u64;
        for (id, sp) in &self.ix.spaces {
            if sp.owner != account {
                continue;
            }
            total += self
                .ix
                .objects
                .get(id)
                .map(|m| m.values().map(|o| o.size + OBJECT_OVERHEAD).sum::<u64>())
                .unwrap_or(0);
            total += self.ix.blobs.get(id).map(|m| m.values().map(|b| b.size).sum::<u64>()).unwrap_or(0);
        }
        total
    }

    /// The account's personal space.
    pub fn space_of(&self, account: &str) -> Option<(&String, &Space)> {
        self.ix.spaces.iter().find(|(_, s)| s.owner == account)
    }

    /// Remove an account and everything it holds.
    pub fn delete_account(&mut self, account: &str) {
        let spaces: Vec<String> =
            self.ix.spaces.iter().filter(|(_, s)| s.owner == account).map(|(id, _)| id.clone()).collect();
        for sp in &spaces {
            self.ix.spaces.remove(sp);
            self.ix.objects.remove(sp);
            self.ix.blobs.remove(sp);
            self.by_seq.remove(sp);
            self.ix.space_keys.retain(|k| &k.space != sp);
            self.garbage.push(self.dir.join("objects").join(sp));
            self.garbage.push(self.dir.join("blobs").join(sp));
        }
        self.ix.devices.retain(|_, d| d.account.as_deref() != Some(account));
        self.ix.envelopes.retain(|e| e.account != account);
        self.ix.accounts.remove(account);
    }

    /// Housekeeping (the SQL `purge`, run hourly): deleted objects after a
    /// day, blob parts no committed version uses after a day, pending devices
    /// a day after they expired, join codes a day after they expired.
    pub fn purge(&mut self, now: DateTime<Utc>) -> (usize, usize, usize) {
        let day = chrono::Duration::days(1);
        let mut gone_objects: Vec<(String, String, i64)> = Vec::new();
        for (space, objs) in self.ix.objects.iter_mut() {
            objs.retain(|id, o| {
                let old = o.deleted && o.deleted_at.is_some_and(|t| t < now - day);
                if old {
                    gone_objects.push((space.clone(), id.clone(), o.seq));
                }
                !old
            });
        }
        for (space, _, seq) in &gone_objects {
            if let Some(m) = self.by_seq.get_mut(space) {
                m.remove(seq);
            }
        }
        let n_objects = gone_objects.len();
        let mut gone_blobs: Vec<(String, String)> = Vec::new();
        for (space, parts) in &self.ix.blobs {
            for (k, b) in parts {
                if b.created_at >= now - day {
                    continue;
                }
                let live = split_blob_key(k).is_some_and(|(o, v, _)| {
                    self.ix
                        .objects
                        .get(space)
                        .and_then(|m| m.get(o))
                        .is_some_and(|ob| ob.version == v && !ob.deleted)
                });
                if !live {
                    gone_blobs.push((space.clone(), k.clone()));
                }
            }
        }
        for (space, k) in &gone_blobs {
            if let Some(parts) = self.ix.blobs.get_mut(space) {
                parts.remove(k);
            }
            if let Some((o, v, p)) = split_blob_key(k) {
                self.remove_blob(space, o, v, p);
            }
        }
        let before = self.ix.devices.len();
        self.ix
            .devices
            .retain(|_, d| d.status != "pending" || d.pending_expires_at.is_none_or(|t| t >= now - day));
        let n_devices = before - self.ix.devices.len();
        self.ix.codes.retain(|c| c.expires_at >= now - day);
        (n_objects, gone_blobs.len(), n_devices)
    }
}
