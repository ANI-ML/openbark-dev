//! The keys of ADR-0017 section 4, and the two small sealed formats that
//! carry them.
//!
//! | Key | Lives | Opens |
//! |---|---|---|
//! | Device keys (X25519, Ed25519) | this device's keychain, never exported | envelopes addressed to it; signs what it seals |
//! | Account Master Key (AMK) | the server keeps only envelopes: one per approved device, one for the Recovery Kit | the space keys and the roster |
//! | Recovery Kit | printed (24 words) | an X25519 key that opens the AMK's kit envelope |
//! | Space key, versioned | wrapped under the AMK | the object keys |
//! | Object key (DEK) | wrapped under a space key version, beside each object | one object and its blob parts |
//!
//! Formats:
//! - **OBE1**, an envelope to an X25519 public key: `OBE1` ‖ version ‖
//!   ephemeral public key (32) ‖ nonce (24) ‖ XChaCha20-Poly1305 ciphertext.
//!   The key is HKDF-SHA256 over the X25519 shared secret (and, when pairing,
//!   a key from the 6-word code), salted with both public keys; the purpose,
//!   key version and recipient are bound as associated data.
//! - **OBW1**, a key wrapped under a symmetric key: `OBW1` ‖ nonce (24) ‖
//!   ciphertext, with its purpose bound as associated data.

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::SyncError;

pub const ENVELOPE_MAGIC: &[u8; 4] = b"OBE1";
pub const WRAP_MAGIC: &[u8; 4] = b"OBW1";
const NONCE: usize = 24;
const TAG: usize = 16;

/// 32 random bytes from the operating system.
pub fn random32() -> Result<[u8; 32], SyncError> {
    random::<32>()
}

pub fn random<const N: usize>() -> Result<[u8; N], SyncError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| SyncError::Crypto(format!("no secure random numbers: {e}")))?;
    Ok(b)
}

/// HKDF-SHA256, 32 bytes out.
pub fn hkdf(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut out = [0u8; 32];
    hk.expand(info, &mut out).expect("32 bytes is a valid HKDF length");
    out
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn hmac(key: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as hmac::KeyInit>::new_from_slice(key).expect("any key length");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn unb64(text: &str) -> Result<Vec<u8>, SyncError> {
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| SyncError::Format("not base64".into()))
}

pub fn unb64_32(text: &str) -> Result<[u8; 32], SyncError> {
    unb64(text)?.try_into().map_err(|_| SyncError::Format("not a 32-byte value".into()))
}

/// Constant-time comparison of two 32-byte values.
pub fn eq32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A 256-bit symmetric key (the AMK, a space key, a DEK). Never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct SymKey(pub [u8; 32]);

impl std::fmt::Debug for SymKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SymKey(..)")
    }
}

impl SymKey {
    pub fn random() -> Result<Self, SyncError> {
        Ok(Self(random32()?))
    }

    /// A key for one purpose, derived from this one.
    pub fn derive(&self, info: &str) -> SymKey {
        SymKey(hkdf(&self.0, b"openbark", info.as_bytes()))
    }
}

fn aead(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

pub fn aead_seal(key: &[u8; 32], nonce: &[u8; NONCE], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, SyncError> {
    aead(key)
        .encrypt(&XNonce::from(*nonce), Payload { msg: plain, aad })
        .map_err(|_| SyncError::Crypto("encryption failed".into()))
}

pub fn aead_open(key: &[u8; 32], nonce: &[u8], aad: &[u8], ct: &[u8]) -> Result<Vec<u8>, SyncError> {
    let nonce = XNonce::try_from(nonce).map_err(|_| SyncError::Format("bad nonce".into()))?;
    aead(key)
        .decrypt(&nonce, Payload { msg: ct, aad })
        .map_err(|_| SyncError::Crypto("can't be opened with this key".into()))
}

/// OBW1: `plain` sealed under `key`, `aad` bound in.
pub fn wrap(key: &SymKey, aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, SyncError> {
    let nonce = random::<NONCE>()?;
    let ct = aead_seal(&key.0, &nonce, &[WRAP_MAGIC.as_slice(), aad].concat(), plain)?;
    Ok([WRAP_MAGIC.as_slice(), &nonce, &ct].concat())
}

pub fn unwrap(key: &SymKey, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, SyncError> {
    if !sealed.starts_with(WRAP_MAGIC) || sealed.len() < 4 + NONCE + TAG {
        return Err(SyncError::Format("not a wrapped key".into()));
    }
    aead_open(&key.0, &sealed[4..4 + NONCE], &[WRAP_MAGIC.as_slice(), aad].concat(), &sealed[4 + NONCE..])
}

/// This device's own keys. The secrets never leave the device; only the
/// public halves and the rebind verifier go to the server.
pub struct DeviceKeys {
    x: StaticSecret,
    ed: SigningKey,
    /// Proves this device to the server when it signs in again, or asks
    /// whether it was revoked. Only its SHA-256 is stored there.
    rebind: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct DeviceKeysText {
    x25519: String,
    ed25519: String,
    rebind: String,
}

impl DeviceKeys {
    pub fn generate() -> Result<Self, SyncError> {
        Ok(Self {
            x: StaticSecret::from(random32()?),
            ed: SigningKey::from_bytes(&random32()?),
            rebind: random32()?,
        })
    }

    pub fn x_pub(&self) -> [u8; 32] {
        PublicKey::from(&self.x).to_bytes()
    }

    pub fn ed_pub(&self) -> [u8; 32] {
        self.ed.verifying_key().to_bytes()
    }

    pub fn x_secret(&self) -> &StaticSecret {
        &self.x
    }

    pub fn rebind_secret(&self) -> &[u8; 32] {
        &self.rebind
    }

    pub fn rebind_verifier(&self) -> [u8; 32] {
        sha256(&[&self.rebind])
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.ed.sign(msg).to_bytes()
    }

    pub fn to_text(&self) -> String {
        serde_json::to_string(&DeviceKeysText {
            x25519: b64(&self.x.to_bytes()),
            ed25519: b64(&self.ed.to_bytes()),
            rebind: b64(&self.rebind),
        })
        .expect("keys serialise")
    }

    pub fn from_text(text: &str) -> Result<Self, SyncError> {
        let t: DeviceKeysText =
            serde_json::from_str(text).map_err(|_| SyncError::Format("damaged device keys".into()))?;
        Ok(Self {
            x: StaticSecret::from(unb64_32(&t.x25519)?),
            ed: SigningKey::from_bytes(&unb64_32(&t.ed25519)?),
            rebind: unb64_32(&t.rebind)?,
        })
    }
}

/// Check an Ed25519 signature.
pub fn verify(ed_pub: &[u8; 32], msg: &[u8], sig: &[u8]) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(ed_pub) else { return false };
    let Ok(sig) = <[u8; 64]>::try_from(sig) else { return false };
    key.verify(msg, &Signature::from_bytes(&sig)).is_ok()
}

/// A device's public keys as one 32-byte fingerprint (carried in the
/// pairing QR code).
pub fn fingerprint(device_id: &str, x_pub: &[u8; 32], ed_pub: &[u8; 32]) -> [u8; 32] {
    sha256(&[b"openbark device v1", device_id.as_bytes(), x_pub, ed_pub])
}

/// OBE1: a 32-byte key sealed to an X25519 public key. `aad` names what it
/// is (purpose, version, recipient); `bind` (the pairing key) must be known
/// to open it too.
pub fn seal_to(
    recipient: &[u8; 32],
    plain: &[u8; 32],
    aad: &[u8],
    bind: Option<&[u8; 32]>,
) -> Result<Vec<u8>, SyncError> {
    let eph = StaticSecret::from(random32()?);
    let eph_pub = PublicKey::from(&eph).to_bytes();
    let shared = eph.diffie_hellman(&PublicKey::from(*recipient));
    let key = envelope_key(shared.as_bytes(), &eph_pub, recipient, aad, bind)?;
    let nonce = random::<NONCE>()?;
    let ct = aead_seal(&key, &nonce, &[ENVELOPE_MAGIC.as_slice(), &[1], aad].concat(), plain)?;
    Ok([ENVELOPE_MAGIC.as_slice(), &[1], &eph_pub, &nonce, &ct].concat())
}

/// Open an OBE1 envelope addressed to `secret`.
pub fn open_envelope(
    secret: &StaticSecret,
    sealed: &[u8],
    aad: &[u8],
    bind: Option<&[u8; 32]>,
) -> Result<[u8; 32], SyncError> {
    if sealed.len() != 5 + 32 + NONCE + 32 + TAG || !sealed.starts_with(ENVELOPE_MAGIC) || sealed[4] != 1 {
        return Err(SyncError::Format("not a key envelope".into()));
    }
    let eph_pub: [u8; 32] = sealed[5..37].try_into().expect("32 bytes");
    let me = PublicKey::from(secret).to_bytes();
    let shared = secret.diffie_hellman(&PublicKey::from(eph_pub));
    let key = envelope_key(shared.as_bytes(), &eph_pub, &me, aad, bind)?;
    let plain = aead_open(&key, &sealed[37..37 + NONCE], &[ENVELOPE_MAGIC.as_slice(), &[1], aad].concat(), &sealed[37 + NONCE..])?;
    plain.try_into().map_err(|_| SyncError::Format("bad envelope contents".into()))
}

fn envelope_key(
    shared: &[u8; 32],
    eph_pub: &[u8; 32],
    recipient: &[u8; 32],
    aad: &[u8],
    bind: Option<&[u8; 32]>,
) -> Result<[u8; 32], SyncError> {
    if shared.iter().all(|b| *b == 0) {
        return Err(SyncError::Crypto("a low-order key was offered".into()));
    }
    let ikm = [shared.as_slice(), bind.map(|b| b.as_slice()).unwrap_or_default()].concat();
    let salt = [eph_pub.as_slice(), recipient].concat();
    Ok(hkdf(&ikm, &salt, &[b"openbark envelope v1|".as_slice(), aad].concat()))
}

/// What an AMK envelope is bound to: the version and its recipient (a
/// device id, or "kit").
pub fn amk_aad(version: u32, recipient: &str) -> Vec<u8> {
    format!("amk|{version}|{recipient}").into_bytes()
}

/// The space key and the naming key, as wrapped under the AMK.
#[derive(Clone, Serialize, Deserialize)]
pub struct SpaceSecret {
    pub key: String,
    pub naming: String,
}

impl std::fmt::Debug for SpaceSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SpaceSecret(..)")
    }
}

pub fn space_key_aad(space: &str, version: u32, amk_version: u32) -> Vec<u8> {
    format!("space-key|{space}|{version}|{amk_version}").into_bytes()
}

pub fn wrap_space_key(
    amk: &SymKey,
    space: &str,
    version: u32,
    amk_version: u32,
    key: &SymKey,
    naming: &SymKey,
) -> Result<Vec<u8>, SyncError> {
    let text = serde_json::to_vec(&SpaceSecret { key: b64(&key.0), naming: b64(&naming.0) }).expect("serialises");
    wrap(&amk.derive("space-key"), &space_key_aad(space, version, amk_version), &text)
}

pub fn unwrap_space_key(
    amk: &SymKey,
    space: &str,
    version: u32,
    amk_version: u32,
    wrapped: &[u8],
) -> Result<(SymKey, SymKey), SyncError> {
    let text = unwrap(&amk.derive("space-key"), &space_key_aad(space, version, amk_version), wrapped)?;
    let s: SpaceSecret = serde_json::from_slice(&text).map_err(|_| SyncError::Format("damaged space key".into()))?;
    Ok((SymKey(unb64_32(&s.key)?), SymKey(unb64_32(&s.naming)?)))
}

pub fn dek_aad(space: &str, object: &str, key_version: u32) -> Vec<u8> {
    format!("dek|{space}|{object}|{key_version}").into_bytes()
}

/// An object's id on the server: derived from what it is on the device
/// (`kind` and the visit, run or patient id) under the naming key, so the
/// server sees a random-looking UUID and two devices name the same thing
/// alike. Nothing on the server uses a visit folder's name.
pub fn object_id(naming: &SymKey, kind: &str, local_id: &str) -> String {
    let h = hmac(&naming.0, &[b"openbark object id v1|", kind.as_bytes(), b"|", local_id.as_bytes()]);
    let mut b: [u8; 16] = h[..16].try_into().expect("16 bytes");
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    uuid::Uuid::from_bytes(b).to_string()
}

// ---- the Recovery Kit -------------------------------------------------------

/// The Recovery Kit: 256 random bits, written as 24 BIP-39 words.
#[derive(Clone)]
pub struct Kit {
    seed: [u8; 32],
}

impl Kit {
    pub fn generate() -> Result<Self, SyncError> {
        Ok(Self { seed: random32()? })
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self { seed }
    }

    pub fn seed(&self) -> &[u8; 32] {
        &self.seed
    }

    pub fn words(&self) -> Vec<String> {
        bip39::Mnemonic::from_entropy(&self.seed)
            .expect("32 bytes is a valid entropy length")
            .words()
            .map(String::from)
            .collect()
    }

    /// The kit from its 24 words (any case, extra spaces, numbering or the
    /// first four letters of each word). The last word's checksum catches a
    /// mistyped word.
    pub fn from_words(text: &str) -> Result<Self, SyncError> {
        let words = normalise_words(text)?;
        if words.len() != 24 {
            return Err(SyncError::User(format!(
                "A Recovery Kit has 24 words; {} were entered.",
                words.len()
            )));
        }
        let m = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &words.join(" "))
            .map_err(|_| SyncError::User("These words aren't a Recovery Kit. Check each word against the printed kit.".into()))?;
        let entropy = m.to_entropy();
        Ok(Self { seed: entropy.try_into().map_err(|_| SyncError::User("That isn't a 24-word kit.".into()))? })
    }

    pub fn x_secret(&self) -> StaticSecret {
        StaticSecret::from(hkdf(&self.seed, b"openbark", b"kit x25519 v1"))
    }

    pub fn x_pub(&self) -> [u8; 32] {
        PublicKey::from(&self.x_secret()).to_bytes()
    }

    /// Proves the kit to the server when a new device approves itself; its
    /// SHA-256 is stored there. Derived on its own label, so it says nothing
    /// about the kit's private key.
    pub fn token(&self) -> [u8; 32] {
        hkdf(&self.seed, b"openbark", b"kit token v1")
    }

    pub fn verifier(&self) -> [u8; 32] {
        sha256(&[&self.token()])
    }
}

/// The BIP-39 English word list.
fn word_list() -> &'static [&'static str; 2048] {
    bip39::Language::English.word_list()
}

/// Words typed by a person: lower case, split on spaces, commas, hyphens
/// and numbering; each word may be just its first four letters (unique in
/// the BIP-39 list).
pub fn normalise_words(text: &str) -> Result<Vec<String>, SyncError> {
    let list = word_list();
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || matches!(c, ',' | '-' | '.' | ';')) {
        let w = raw.trim().trim_matches(|c: char| !c.is_ascii_alphabetic()).to_ascii_lowercase();
        if w.is_empty() || w.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let found = list
            .iter()
            .find(|l| **l == w)
            .or_else(|| if w.len() >= 4 { list.iter().find(|l| l.starts_with(&w[..4]) && l.len() >= 4) } else { None })
            .ok_or_else(|| SyncError::User(format!("\"{w}\" isn't one of the words OpenBark uses. Check it.")))?;
        out.push(found.to_string());
    }
    Ok(out)
}

// ---- pairing -----------------------------------------------------------------

/// The 6-word pairing code a new device shows (66 random bits), and the
/// keys derived from it. The approving device types the words: it can then
/// check the new device's keys (the HMAC), and the new device can check the
/// envelope came from a device that saw its screen (the binding). A slow
/// key derivation (Argon2id, salted with the device id) keeps the server
/// from guessing the code in the 10 minutes a request lasts.
#[derive(Clone)]
pub struct PairingCode {
    pub words: Vec<String>,
}

pub const PAIRING_WORDS: usize = 6;

impl PairingCode {
    pub fn generate() -> Result<Self, SyncError> {
        let r = random::<9>()?;
        let mut bits: u128 = 0;
        for b in r {
            bits = (bits << 8) | b as u128;
        }
        let list = word_list();
        let words = (0..PAIRING_WORDS)
            .map(|i| list[((bits >> (72 - 11 * (i + 1))) & 0x7ff) as usize].to_string())
            .collect();
        Ok(Self { words })
    }

    pub fn parse(text: &str) -> Result<Self, SyncError> {
        let words = normalise_words(text)?;
        if words.len() != PAIRING_WORDS {
            return Err(SyncError::User(format!(
                "Type the {PAIRING_WORDS} words shown on the new device; {} were entered.",
                words.len()
            )));
        }
        Ok(Self { words })
    }

    pub fn text(&self) -> String {
        self.words.join(" ")
    }

    /// The pairing key for `device_id` (Argon2id, 19 MiB, 2 passes).
    pub fn key(&self, device_id: &str) -> Result<[u8; 32], SyncError> {
        use argon2::{Algorithm, Argon2, Params, Version};
        let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(|e| SyncError::Crypto(e.to_string()))?;
        let salt = [b"openbark pair v1|".as_slice(), device_id.as_bytes()].concat();
        let mut out = [0u8; 32];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(self.text().as_bytes(), &salt, &mut out)
            .map_err(|e| SyncError::Crypto(e.to_string()))?;
        Ok(out)
    }
}

/// The HMAC a pending device posts over its id and public keys.
pub fn pair_mac(pair_key: &[u8; 32], device_id: &str, x_pub: &[u8; 32], ed_pub: &[u8; 32]) -> [u8; 32] {
    hmac(pair_key, &[b"openbark pair mac v1|", device_id.as_bytes(), x_pub, ed_pub])
}

/// The key the pending device's name is sealed under.
pub fn pair_blob_key(pair_key: &[u8; 32]) -> SymKey {
    SymKey(hkdf(pair_key, b"openbark", b"pair blob v1"))
}

/// Bound into the AMK envelope for the new device.
pub fn pair_bind(pair_key: &[u8; 32]) -> [u8; 32] {
    hkdf(pair_key, b"openbark", b"pair bind v1")
}

/// The pairing QR code's text: `openbark-pair:1:<device id>:<fingerprint>:<words>`.
pub fn pairing_uri(device_id: &str, fingerprint: &[u8; 32], code: &PairingCode) -> String {
    let fp = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(fingerprint);
    format!("openbark-pair:1:{device_id}:{fp}:{}", code.words.join("-"))
}

/// An SVG QR code for `text` (medium error correction).
pub fn qr_svg(text: &str) -> String {
    match qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M) {
        Ok(code) => code
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(220, 220)
            .quiet_zone(true)
            .build(),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_envelope_opens_only_for_its_recipient_purpose_and_binding() {
        let a = DeviceKeys::generate().unwrap();
        let b = DeviceKeys::generate().unwrap();
        let amk = random32().unwrap();
        let bind = random32().unwrap();
        let env = seal_to(&a.x_pub(), &amk, b"amk|1|dev-a", Some(&bind)).unwrap();
        assert_eq!(env.len(), 5 + 32 + 24 + 32 + 16);
        assert_eq!(open_envelope(a.x_secret(), &env, b"amk|1|dev-a", Some(&bind)).unwrap(), amk);
        assert!(open_envelope(b.x_secret(), &env, b"amk|1|dev-a", Some(&bind)).is_err(), "another device");
        assert!(open_envelope(a.x_secret(), &env, b"amk|2|dev-a", Some(&bind)).is_err(), "another version");
        assert!(open_envelope(a.x_secret(), &env, b"amk|1|dev-a", None).is_err(), "without the pairing key");
        let mut bad = env.clone();
        bad[60] ^= 1;
        assert!(open_envelope(a.x_secret(), &bad, b"amk|1|dev-a", Some(&bind)).is_err(), "changed bytes");
        // A low-order public key gives an all-zero secret: refused.
        assert!(seal_to(&[0u8; 32], &amk, b"x", None).is_err());
    }

    #[test]
    fn device_keys_round_trip_and_sign() {
        let k = DeviceKeys::generate().unwrap();
        let back = DeviceKeys::from_text(&k.to_text()).unwrap();
        assert_eq!(back.x_pub(), k.x_pub());
        assert_eq!(back.ed_pub(), k.ed_pub());
        assert_eq!(back.rebind_verifier(), k.rebind_verifier());
        let sig = k.sign(b"header");
        assert!(verify(&k.ed_pub(), b"header", &sig));
        assert!(!verify(&k.ed_pub(), b"headex", &sig));
        assert!(!format!("{:?}", SymKey([7; 32])).contains('7'));
    }

    #[test]
    fn the_kit_is_24_words_and_forgives_typing() {
        let kit = Kit::generate().unwrap();
        let words = kit.words();
        assert_eq!(words.len(), 24);
        let typed = words
            .iter()
            .enumerate()
            .map(|(i, w)| format!("{}. {}", i + 1, if i % 2 == 0 { w.to_uppercase() } else { w[..4.min(w.len())].to_string() }))
            .collect::<Vec<_>>()
            .join("\n");
        let back = Kit::from_words(&typed).unwrap();
        assert_eq!(back.seed(), kit.seed());
        assert_eq!(back.x_pub(), kit.x_pub());
        assert_ne!(kit.token(), *kit.seed());
        // One word changed: the checksum catches it (almost always; retry a
        // different word if the swap happens to keep it valid).
        let mut changed = words.clone();
        changed[3] = if changed[3] == "abandon" { "ability".into() } else { "abandon".into() };
        let mut wrong = Kit::from_words(&changed.join(" "));
        if wrong.is_ok() {
            changed[23] = if changed[23] == "zoo" { "zone".into() } else { "zoo".into() };
            wrong = Kit::from_words(&changed.join(" "));
        }
        assert!(wrong.is_err() || wrong.unwrap().seed() != kit.seed());
        assert!(Kit::from_words("abandon ability").is_err());
        assert!(Kit::from_words(&format!("{} notaword", words[..23].join(" "))).is_err());
    }

    #[test]
    fn the_pairing_code_binds_keys_and_resists_a_swap() {
        let code = PairingCode::generate().unwrap();
        assert_eq!(code.words.len(), 6);
        let typed = PairingCode::parse(&code.words.join("-").to_uppercase()).unwrap();
        assert_eq!(typed.text(), code.text());
        let dev = DeviceKeys::generate().unwrap();
        let key = code.key("dev-1").unwrap();
        assert_eq!(typed.key("dev-1").unwrap(), key);
        assert_ne!(code.key("dev-2").unwrap(), key, "salted with the device id");
        let mac = pair_mac(&key, "dev-1", &dev.x_pub(), &dev.ed_pub());
        let other = DeviceKeys::generate().unwrap();
        assert_ne!(pair_mac(&key, "dev-1", &other.x_pub(), &dev.ed_pub()), mac, "swapped keys don't match");
        let uri = pairing_uri("dev-1", &fingerprint("dev-1", &dev.x_pub(), &dev.ed_pub()), &code);
        assert!(uri.starts_with("openbark-pair:1:dev-1:"));
        assert!(qr_svg(&uri).starts_with("<?xml"));
    }

    #[test]
    fn object_ids_are_stable_random_looking_and_keyed() {
        let n1 = SymKey([1; 32]);
        let n2 = SymKey([2; 32]);
        let a = object_id(&n1, "visit", "0199-session");
        assert_eq!(a, object_id(&n1, "visit", "0199-session"));
        assert_ne!(a, object_id(&n1, "run", "0199-session"));
        assert_ne!(a, object_id(&n2, "visit", "0199-session"));
        assert_eq!(uuid::Uuid::parse_str(&a).unwrap().get_version_num(), 4);
    }

    #[test]
    fn wrapped_keys_are_bound_to_their_purpose() {
        let amk = SymKey::random().unwrap();
        let (k, n) = (SymKey::random().unwrap(), SymKey::random().unwrap());
        let w = wrap_space_key(&amk, "space", 1, 1, &k, &n).unwrap();
        assert!(w.starts_with(WRAP_MAGIC));
        let (k2, n2) = unwrap_space_key(&amk, "space", 1, 1, &w).unwrap();
        assert_eq!((k2, n2), (k, n));
        assert!(unwrap_space_key(&amk, "space", 2, 1, &w).is_err());
        assert!(unwrap_space_key(&SymKey::random().unwrap(), "space", 1, 1, &w).is_err());
    }
}
