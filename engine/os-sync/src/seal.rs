//! The sync object formats (ADR-0017 section 5).
//!
//! **OBS1**, small objects (visit headers, events, patient records, the
//! roster):
//!
//! ```text
//! "OBS1" | 0x01 | header length (u16 BE) | header (JSON) | nonce (24)
//!        | XChaCha20-Poly1305 ciphertext of the padded body | Ed25519 signature (64)
//! ```
//!
//! - The header names the object: id, space, kind, version, the device that
//!   sealed it and that device's counter. It is the associated data, so the
//!   server can't move a ciphertext to another id, kind, space or version.
//! - The body is padded so the ciphertext is exactly 1, 4 or 8 KiB (or a
//!   whole number of 8 KiB steps, up to 64 KiB).
//! - The signature covers every byte before it, by the device's Ed25519 key.
//!   Clients accept it only from keys in the roster.
//!
//! **OBB1**, blob parts (transcripts and note runs; never audio). The OBA1
//! chunk layout of `os_engine::vault`, under its own magic so a sync blob can
//! never be mistaken for an encrypted recording (OBA1 on disk):
//!
//! ```text
//! "OBB1" | 0x01 | header length (u16 BE) | header (JSON)
//!        | chunks: nonce (24) | ciphertext (64 KiB + 16) ... | Ed25519 signature (64)
//! ```
//!
//! Each chunk binds the header, its index and whether it is the last as
//! associated data, under a key derived from the object's DEK and the part
//! number. The plaintext is padded to whole 64 KiB chunks.
//!
//! [`parse_obs1`] and [`parse_obb1`] check the structure without any key:
//! the plaintext guard (`crate::guard`) uses them on every request body.

use serde::{Deserialize, Serialize};

use crate::SyncError;
use crate::keys::{self, DeviceKeys, SymKey};

pub const OBS1: &[u8; 4] = b"OBS1";
pub const OBB1: &[u8; 4] = b"OBB1";
const VERSION: u8 = 1;
const NONCE: usize = 24;
const TAG: usize = 16;
const SIG: usize = 64;
const MAX_HEADER: usize = 1024;
/// Ciphertext sizes for OBS1 bodies.
pub const BUCKETS: [usize; 3] = [1024, 4096, 8192];
pub const MAX_OBS1_BODY: usize = 64 * 1024;
/// Plaintext per blob chunk, as the vault's audio chunks.
pub const CHUNK: usize = 64 * 1024;
/// Chunks per blob part: 1 MiB of plaintext.
pub const CHUNKS_PER_PART: usize = 16;

/// What an OBS1 object says about itself, in the clear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub v: u8,
    pub id: String,
    pub space: String,
    pub kind: String,
    pub ver: u32,
    pub dev: String,
    pub ctr: u64,
}

/// What an OBB1 blob part says about itself, in the clear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobHeader {
    pub v: u8,
    /// The object this part belongs to, and its version.
    pub id: String,
    pub space: String,
    pub ver: u32,
    pub part: u32,
    pub parts: u32,
    pub dev: String,
    pub ctr: u64,
    pub chunks: u32,
}

/// An OBS1 object's parts, checked for shape but not opened.
#[derive(Debug, Clone)]
pub struct Obs1<'a> {
    pub header: Header,
    /// Everything up to the end of the header: the associated data.
    pub aad: &'a [u8],
    pub nonce: &'a [u8],
    pub ct: &'a [u8],
    /// Everything before the signature.
    pub signed: &'a [u8],
    pub sig: &'a [u8],
}

fn bad(why: &str) -> SyncError {
    SyncError::Format(format!("not a sealed object: {why}"))
}

fn id_like(s: &str) -> bool {
    s.len() <= 64 && !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn kind_ok(s: &str) -> bool {
    matches!(s, "visit" | "run" | "event" | "checks" | "transcript" | "patient" | "tombstone" | "roster")
}

/// Check an OBS1 object's structure (no key needed).
pub fn parse_obs1(bytes: &[u8]) -> Result<Obs1<'_>, SyncError> {
    if bytes.len() < 7 || &bytes[..4] != OBS1 {
        return Err(bad("magic"));
    }
    if bytes[4] != VERSION {
        return Err(bad("version"));
    }
    let hlen = u16::from_be_bytes([bytes[5], bytes[6]]) as usize;
    if hlen == 0 || hlen > MAX_HEADER || bytes.len() < 7 + hlen + NONCE + TAG + SIG {
        return Err(bad("lengths"));
    }
    let header: Header = serde_json::from_slice(&bytes[7..7 + hlen]).map_err(|_| bad("header"))?;
    if header.v != VERSION
        || !id_like(&header.id)
        || !id_like(&header.space)
        || !id_like(&header.dev)
        || !kind_ok(&header.kind)
    {
        return Err(bad("header fields"));
    }
    let body = 7 + hlen;
    let sig_at = bytes.len() - SIG;
    let ct = &bytes[body + NONCE..sig_at];
    if !(BUCKETS.contains(&ct.len()) || (ct.len() % 8192 == 0 && ct.len() <= MAX_OBS1_BODY)) {
        return Err(bad("padding"));
    }
    Ok(Obs1 {
        header,
        aad: &bytes[..body],
        nonce: &bytes[body..body + NONCE],
        ct,
        signed: &bytes[..sig_at],
        sig: &bytes[sig_at..],
    })
}

/// The ciphertext size a body of `len` bytes pads to.
fn padded_ct_len(len: usize) -> Result<usize, SyncError> {
    let need = len + 4 + TAG;
    if let Some(b) = BUCKETS.iter().find(|b| **b >= need) {
        return Ok(*b);
    }
    let steps = need.div_ceil(8192) * 8192;
    if steps > MAX_OBS1_BODY {
        return Err(SyncError::Format("object too large for OBS1; it goes in a blob".into()));
    }
    Ok(steps)
}

/// Seal `body` as an OBS1 object under `dek`, signed by `device`.
pub fn seal_obs1(header: &Header, body: &[u8], dek: &SymKey, device: &DeviceKeys) -> Result<Vec<u8>, SyncError> {
    let hbytes = serde_json::to_vec(header).expect("header serialises");
    if hbytes.len() > MAX_HEADER {
        return Err(SyncError::Format("header too long".into()));
    }
    let ct_len = padded_ct_len(body.len())?;
    let mut plain = Vec::with_capacity(ct_len - TAG);
    plain.extend_from_slice(&(body.len() as u32).to_le_bytes());
    plain.extend_from_slice(body);
    plain.resize(ct_len - TAG, 0);
    let mut out = Vec::with_capacity(7 + hbytes.len() + NONCE + ct_len + SIG);
    out.extend_from_slice(OBS1);
    out.push(VERSION);
    out.extend_from_slice(&(hbytes.len() as u16).to_be_bytes());
    out.extend_from_slice(&hbytes);
    let aad = out.clone();
    let nonce = keys::random::<NONCE>()?;
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&keys::aead_seal(&dek.0, &nonce, &aad, &plain)?);
    let sig = device.sign(&out);
    out.extend_from_slice(&sig);
    Ok(out)
}

/// Check an OBS1 object's signature against `ed_pub`.
pub fn verify_obs1(obj: &Obs1<'_>, ed_pub: &[u8; 32]) -> bool {
    keys::verify(ed_pub, obj.signed, obj.sig)
}

/// Open an OBS1 object's body (check the signature first).
pub fn open_obs1(obj: &Obs1<'_>, dek: &SymKey) -> Result<Vec<u8>, SyncError> {
    let plain = keys::aead_open(&dek.0, obj.nonce, obj.aad, obj.ct)?;
    let len = u32::from_le_bytes(plain[..4].try_into().map_err(|_| bad("body"))?) as usize;
    if 4 + len > plain.len() || plain[4 + len..].iter().any(|b| *b != 0) {
        return Err(bad("body padding"));
    }
    Ok(plain[4..4 + len].to_vec())
}

/// An OBB1 part's parts, checked for shape but not opened.
#[derive(Debug, Clone)]
pub struct Obb1<'a> {
    pub header: BlobHeader,
    pub hbytes: &'a [u8],
    pub chunks: Vec<(&'a [u8], &'a [u8])>,
    pub signed: &'a [u8],
    pub sig: &'a [u8],
}

const CHUNK_CT: usize = CHUNK + TAG;

/// Check an OBB1 part's structure (no key needed).
pub fn parse_obb1(bytes: &[u8]) -> Result<Obb1<'_>, SyncError> {
    if bytes.len() < 7 || &bytes[..4] != OBB1 {
        return Err(bad("magic"));
    }
    if bytes[4] != VERSION {
        return Err(bad("version"));
    }
    let hlen = u16::from_be_bytes([bytes[5], bytes[6]]) as usize;
    if hlen == 0 || hlen > MAX_HEADER || bytes.len() < 7 + hlen + SIG {
        return Err(bad("lengths"));
    }
    let hbytes = &bytes[7..7 + hlen];
    let header: BlobHeader = serde_json::from_slice(hbytes).map_err(|_| bad("header"))?;
    if header.v != VERSION
        || !id_like(&header.id)
        || !id_like(&header.space)
        || !id_like(&header.dev)
        || header.chunks == 0
        || header.chunks as usize > CHUNKS_PER_PART
        || header.parts == 0
        || header.part >= header.parts
    {
        return Err(bad("header fields"));
    }
    let start = 7 + hlen;
    let sig_at = bytes.len() - SIG;
    if sig_at - start != header.chunks as usize * (NONCE + CHUNK_CT) {
        return Err(bad("chunks"));
    }
    let chunks = (0..header.chunks as usize)
        .map(|i| {
            let at = start + i * (NONCE + CHUNK_CT);
            (&bytes[at..at + NONCE], &bytes[at + NONCE..at + NONCE + CHUNK_CT])
        })
        .collect();
    Ok(Obb1 { header, hbytes, chunks, signed: &bytes[..sig_at], sig: &bytes[sig_at..] })
}

fn blob_key(dek: &SymKey, part: u32) -> SymKey {
    dek.derive(&format!("blob|{part}"))
}

fn chunk_aad(hbytes: &[u8], index: usize, last: bool) -> Vec<u8> {
    [OBB1.as_slice(), &[VERSION], hbytes, &(index as u32).to_be_bytes(), &[last as u8]].concat()
}

/// A sealed blob: its parts, and each part's SHA-256 for the manifest.
pub struct SealedBlob {
    pub parts: Vec<Vec<u8>>,
    pub hashes: Vec<String>,
}

/// Seal `plain` as OBB1 parts of object `id` version `ver`. The plaintext
/// is prefixed with its length and padded to whole 64 KiB chunks.
#[allow(clippy::too_many_arguments)]
pub fn seal_blob(
    id: &str,
    space: &str,
    ver: u32,
    plain: &[u8],
    dek: &SymKey,
    device: &DeviceKeys,
    device_id: &str,
    next_ctr: &mut dyn FnMut() -> u64,
) -> Result<SealedBlob, SyncError> {
    let mut padded = Vec::with_capacity(plain.len() + 8 + CHUNK);
    padded.extend_from_slice(&(plain.len() as u64).to_le_bytes());
    padded.extend_from_slice(plain);
    let total_chunks = padded.len().div_ceil(CHUNK).max(1);
    padded.resize(total_chunks * CHUNK, 0);
    let per_part = CHUNK * CHUNKS_PER_PART;
    let parts_n = padded.len().div_ceil(per_part);
    if parts_n > 64 {
        return Err(SyncError::Format("blob too large to sync".into()));
    }
    let mut parts = Vec::new();
    let mut hashes = Vec::new();
    for (p, piece) in padded.chunks(per_part).enumerate() {
        let chunks = piece.len() / CHUNK;
        let header = BlobHeader {
            v: VERSION,
            id: id.to_string(),
            space: space.to_string(),
            ver,
            part: p as u32,
            parts: parts_n as u32,
            dev: device_id.to_string(),
            ctr: next_ctr(),
            chunks: chunks as u32,
        };
        let hbytes = serde_json::to_vec(&header).expect("header serialises");
        let key = blob_key(dek, p as u32);
        let mut out = Vec::with_capacity(7 + hbytes.len() + chunks * (NONCE + CHUNK_CT) + SIG);
        out.extend_from_slice(OBB1);
        out.push(VERSION);
        out.extend_from_slice(&(hbytes.len() as u16).to_be_bytes());
        out.extend_from_slice(&hbytes);
        for (i, chunk) in piece.chunks(CHUNK).enumerate() {
            let nonce = keys::random::<NONCE>()?;
            out.extend_from_slice(&nonce);
            out.extend_from_slice(&keys::aead_seal(&key.0, &nonce, &chunk_aad(&hbytes, i, i + 1 == chunks), chunk)?);
        }
        let sig = device.sign(&out);
        out.extend_from_slice(&sig);
        hashes.push(hex::encode(keys::sha256(&[&out])));
        parts.push(out);
    }
    Ok(SealedBlob { parts, hashes })
}

/// Open the parts of a blob, checking each against the manifest's hashes,
/// its header (object, space, version, order) and its signature.
pub fn open_blob(
    parts: &[Vec<u8>],
    hashes: &[String],
    id: &str,
    space: &str,
    ver: u32,
    dek: &SymKey,
    signer: &dyn Fn(&str) -> Option<[u8; 32]>,
) -> Result<Vec<u8>, SyncError> {
    if parts.len() != hashes.len() || parts.is_empty() {
        return Err(SyncError::Integrity("a blob part is missing".into()));
    }
    let mut padded = Vec::new();
    for (p, (bytes, want)) in parts.iter().zip(hashes).enumerate() {
        if hex::encode(keys::sha256(&[bytes])) != *want {
            return Err(SyncError::Integrity("a blob part doesn't match its manifest".into()));
        }
        let b = parse_obb1(bytes)?;
        let h = &b.header;
        if h.id != id || h.space != space || h.ver != ver || h.part as usize != p || h.parts as usize != parts.len() {
            return Err(SyncError::Integrity("a blob part belongs elsewhere".into()));
        }
        let ed = signer(&h.dev).ok_or_else(|| SyncError::Integrity("a blob part from an unknown device".into()))?;
        if !keys::verify(&ed, b.signed, b.sig) {
            return Err(SyncError::Integrity("a blob part's signature is wrong".into()));
        }
        let key = blob_key(dek, p as u32);
        let n = b.chunks.len();
        for (i, (nonce, ct)) in b.chunks.iter().enumerate() {
            padded.extend(keys::aead_open(&key.0, nonce, &chunk_aad(b.hbytes, i, i + 1 == n), ct)?);
        }
    }
    let len = u64::from_le_bytes(padded[..8].try_into().map_err(|_| bad("blob"))?) as usize;
    if 8 + len > padded.len() {
        return Err(bad("blob length"));
    }
    Ok(padded[8..8 + len].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(kind: &str) -> Header {
        Header {
            v: 1,
            id: uuid::Uuid::new_v4().to_string(),
            space: uuid::Uuid::new_v4().to_string(),
            kind: kind.into(),
            ver: 1,
            dev: uuid::Uuid::now_v7().to_string(),
            ctr: 7,
        }
    }

    #[test]
    fn obs1_round_trips_and_pads_to_buckets() {
        let dev = DeviceKeys::generate().unwrap();
        let dek = SymKey::random().unwrap();
        for (len, bucket) in [(0usize, 1024usize), (900, 1024), (1100, 4096), (5000, 8192), (9000, 16384)] {
            let body = vec![b'x'; len];
            let h = header("visit");
            let sealed = seal_obs1(&h, &body, &dek, &dev).unwrap();
            let obj = parse_obs1(&sealed).unwrap();
            assert_eq!(obj.header, h);
            assert_eq!(obj.ct.len(), bucket, "{len} bytes");
            assert!(verify_obs1(&obj, &dev.ed_pub()));
            assert_eq!(open_obs1(&obj, &dek).unwrap(), body);
        }
        assert!(seal_obs1(&header("visit"), &vec![0; 70_000], &dek, &dev).is_err());
    }

    #[test]
    fn obs1_binds_its_header_and_signature() {
        let dev = DeviceKeys::generate().unwrap();
        let dek = SymKey::random().unwrap();
        let sealed = seal_obs1(&header("patient"), b"{\"name\":\"Maple\"}", &dek, &dev).unwrap();
        // The server moves the ciphertext under another id: the header no
        // longer matches the associated data, and the signature fails.
        let obj = parse_obs1(&sealed).unwrap();
        let mut h2 = obj.header.clone();
        h2.id = uuid::Uuid::new_v4().to_string();
        let h2b = serde_json::to_vec(&h2).unwrap();
        let mut moved = Vec::new();
        moved.extend_from_slice(OBS1);
        moved.push(1);
        moved.extend_from_slice(&(h2b.len() as u16).to_be_bytes());
        moved.extend_from_slice(&h2b);
        moved.extend_from_slice(&sealed[obj.aad.len()..]);
        let m = parse_obs1(&moved).unwrap();
        assert!(!verify_obs1(&m, &dev.ed_pub()));
        assert!(open_obs1(&m, &dek).is_err());
        // Another device's key doesn't verify it.
        assert!(!verify_obs1(&obj, &DeviceKeys::generate().unwrap().ed_pub()));
        // Plain text never parses.
        assert!(parse_obs1(b"{\"name\":\"Maple\"}").is_err());
        assert!(parse_obs1(b"OBS1").is_err());
    }

    #[test]
    fn blobs_round_trip_across_parts_and_detect_tampering() {
        let dev = DeviceKeys::generate().unwrap();
        let dek = SymKey::random().unwrap();
        let mut ctr = 0;
        let mut next = || {
            ctr += 1;
            ctr
        };
        let plain: Vec<u8> = (0..(CHUNK * CHUNKS_PER_PART + 5000)).map(|i| (i % 251) as u8).collect();
        let id = uuid::Uuid::new_v4().to_string();
        let blob = seal_blob(&id, "space", 2, &plain, &dek, &dev, "dev-a", &mut next).unwrap();
        assert_eq!(blob.parts.len(), 2);
        for p in &blob.parts {
            assert!(parse_obb1(p).is_ok());
        }
        let ed = dev.ed_pub();
        let signer = |d: &str| (d == "dev-a").then_some(ed);
        assert_eq!(open_blob(&blob.parts, &blob.hashes, &id, "space", 2, &dek, &signer).unwrap(), plain);
        // Reordered, missing, replayed under another version, or from an unknown device.
        let swapped = vec![blob.parts[1].clone(), blob.parts[0].clone()];
        let swapped_h = vec![blob.hashes[1].clone(), blob.hashes[0].clone()];
        assert!(open_blob(&swapped, &swapped_h, &id, "space", 2, &dek, &signer).is_err());
        assert!(open_blob(&blob.parts[..1], &blob.hashes[..1], &id, "space", 2, &dek, &signer).is_err());
        assert!(open_blob(&blob.parts, &blob.hashes, &id, "space", 3, &dek, &signer).is_err());
        assert!(open_blob(&blob.parts, &blob.hashes, &id, "space", 2, &dek, &|_| None).is_err());
        // A small blob is one 64 KiB chunk.
        let small = seal_blob(&id, "space", 1, b"tiny", &dek, &dev, "dev-a", &mut || 1).unwrap();
        assert_eq!(parse_obb1(&small.parts[0]).unwrap().chunks.len(), 1);
        // An encrypted recording (OBA1) is never a blob.
        let mut oba1 = small.parts[0].clone();
        oba1[..4].copy_from_slice(b"OBA1");
        assert!(parse_obb1(&oba1).is_err());
    }
}
