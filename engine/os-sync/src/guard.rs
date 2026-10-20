//! The plaintext guard (ADR-0017 section 5): nothing leaves this device
//! that isn't ciphertext in one of sync's formats, or a short structural
//! value (an id, a version, a kind).
//!
//! Every request to the sync service passes [`check_request`] in the one
//! place requests are sent (`crate::supabase`). It walks the JSON body and
//! refuses it unless:
//! - every binary field decodes from base64 and has the exact shape its name
//!   promises: `body` an OBS1 object or an OBB1 blob part, envelopes OBE1,
//!   wrapped keys OBW1, public keys and verifiers 32 bytes;
//! - every other string is short (at most 64 characters) and contains no
//!   spaces: ids, kinds, versions, platform names;
//! - no field is named for something sync never sends (audio, recordings,
//!   transcripts, names).
//!
//! Sign-in (an email address and the 6-digit code) goes through
//! [`check_auth`] instead, which allows exactly those fields.
//!
//! Audio has a second wall: [`refuse_audio`] rejects any bytes that look
//! like a recording (WAV, the vault's OBA1, FLAC, Ogg, MP3, MP4/M4A) before
//! anything is sealed, and the mapping never reads `audio.wav` at all.

use serde_json::Value;

use crate::SyncError;
use crate::keys;
use crate::seal;

/// Fields that carry sealed bytes, and the format each must be.
fn sealed_kind(field: &str) -> Option<Sealed> {
    Some(match field {
        "body" => Sealed::ObjectOrBlob,
        "roster" => Sealed::Object,
        "envelope" | "device_envelope" | "kit_envelope" => Sealed::Envelope,
        "space_key" | "wrapped_dek" => Sealed::Wrapped,
        "pair_blob" => Sealed::Wrapped,
        "x25519_pub" | "ed25519_pub" | "kit_pub" | "pair_mac" | "rebind_verifier" | "kit_verifier"
        | "rebind_secret" | "kit_token" => Sealed::Key32,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sealed {
    ObjectOrBlob,
    Object,
    Envelope,
    Wrapped,
    Key32,
}

/// Field names that must never appear in a request, whatever they hold.
const NEVER: [&str; 10] = [
    "audio",
    "recording",
    "wav",
    "pcm",
    "transcript_text",
    "note_text",
    "patient_name",
    "name",
    "email_body",
    "file",
];

fn refuse(why: String) -> SyncError {
    SyncError::Guard(why)
}

/// Check one sealed value's bytes against the format its field promises.
fn check_sealed(field: &str, kind: Sealed, bytes: &[u8]) -> Result<(), SyncError> {
    refuse_audio(bytes).map_err(|_| refuse(format!("{field} looks like a recording")))?;
    let ok = match kind {
        Sealed::ObjectOrBlob => seal::parse_obs1(bytes).is_ok() || seal::parse_obb1(bytes).is_ok(),
        Sealed::Object => seal::parse_obs1(bytes).is_ok(),
        Sealed::Envelope => bytes.len() == 109 && bytes.starts_with(keys::ENVELOPE_MAGIC),
        Sealed::Wrapped => bytes.len() >= 44 && bytes.len() <= 1024 && bytes.starts_with(keys::WRAP_MAGIC),
        Sealed::Key32 => bytes.len() == 32,
    };
    if ok { Ok(()) } else { Err(refuse(format!("{field} isn't sealed in the expected format"))) }
}

fn check_value(field: &str, v: &Value) -> Result<(), SyncError> {
    if NEVER.iter().any(|n| field.eq_ignore_ascii_case(n)) {
        return Err(refuse(format!("a request carried a field named {field}")));
    }
    match v {
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
        Value::String(s) => match sealed_kind(field) {
            Some(kind) => {
                let bytes = keys::unb64(s).map_err(|_| refuse(format!("{field} isn't base64")))?;
                check_sealed(field, kind, &bytes)
            }
            None => {
                if s.len() > 64 || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    Err(refuse(format!("{field} holds free text, which sync never sends")))
                } else {
                    Ok(())
                }
            }
        },
        Value::Array(items) => items.iter().try_for_each(|i| check_value(field, i)),
        Value::Object(map) => map.iter().try_for_each(|(k, v)| check_value(k, v)),
    }
}

/// Refuse a sync request body unless it is ciphertext and structure only.
pub fn check_request(body: &Value) -> Result<(), SyncError> {
    match body {
        Value::Object(map) => map.iter().try_for_each(|(k, v)| check_value(k, v)),
        _ => Err(refuse("a request body must be a JSON object".into())),
    }
}

/// Sign-in bodies: only these fields, short values.
pub fn check_auth(body: &Value) -> Result<(), SyncError> {
    const ALLOWED: [&str; 6] = ["email", "token", "type", "create_user", "refresh_token", "confirm"];
    let Value::Object(map) = body else { return Err(refuse("a sign-in body must be a JSON object".into())) };
    for (k, v) in map {
        if !ALLOWED.contains(&k.as_str()) {
            return Err(refuse(format!("a sign-in request carried {k}")));
        }
        match v {
            Value::String(s) if s.len() <= 320 && !s.contains('\n') => {}
            Value::Bool(_) => {}
            _ => return Err(refuse(format!("{k} isn't a short value"))),
        }
    }
    Ok(())
}

/// Refuse bytes that look like a recording, before anything is sealed.
pub fn refuse_audio(bytes: &[u8]) -> Result<(), SyncError> {
    let head = &bytes[..bytes.len().min(16)];
    let starts = |m: &[u8]| head.starts_with(m);
    let audio = starts(b"RIFF")
        || starts(b"OBA1")
        || starts(b"fLaC")
        || starts(b"OggS")
        || starts(b"ID3")
        || (head.len() >= 2 && head[0] == 0xFF && (head[1] & 0xE0) == 0xE0 && !starts(b"\xff\xfe"))
        || (head.len() >= 8 && &head[4..8] == b"ftyp")
        || starts(b"caff")
        || starts(b"FORM");
    if audio { Err(refuse("audio never syncs".into())) } else { Ok(()) }
}

/// Refuse a file the mapping was asked to send if it's a recording: by name
/// (`audio.wav` and any audio extension) or by its bytes.
pub fn refuse_audio_file(name: &str, bytes: &[u8]) -> Result<(), SyncError> {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or_default();
    if lower.starts_with("audio")
        || ["wav", "wave", "m4a", "mp3", "aac", "flac", "ogg", "opus", "caf", "aiff", "tmp"].contains(&ext)
    {
        return Err(refuse(format!("{name} is a recording; audio never syncs")));
    }
    refuse_audio(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{DeviceKeys, SymKey};
    use serde_json::json;

    fn obs1() -> String {
        let h = seal::Header {
            v: 1,
            id: uuid::Uuid::new_v4().to_string(),
            space: uuid::Uuid::new_v4().to_string(),
            kind: "visit".into(),
            ver: 1,
            dev: uuid::Uuid::now_v7().to_string(),
            ctr: 1,
        };
        keys::b64(
            &seal::seal_obs1(&h, b"{}", &SymKey::random().unwrap(), &DeviceKeys::generate().unwrap())
                .unwrap(),
        )
    }

    #[test]
    fn sealed_objects_and_structure_pass() {
        let w = keys::b64(&keys::wrap(&SymKey::random().unwrap(), b"dek", &[1; 32]).unwrap());
        let body = json!({
            "space_id": uuid::Uuid::new_v4().to_string(),
            "items": [{"op": "put", "id": uuid::Uuid::new_v4().to_string(), "kind": "visit", "base_version": null,
                       "body": obs1(), "wrapped_dek": w, "key_version": 1, "padded_size": 1024}]
        });
        check_request(&body).unwrap();
    }

    #[test]
    fn plaintext_names_audio_and_free_text_are_refused() {
        let id = uuid::Uuid::new_v4().to_string();
        // A patient's name where a body should be.
        let plain = keys::b64(b"{\"name\":\"Maple Okonkwo\"}");
        assert!(check_request(&json!({"space_id": id, "items": [{"body": plain}]})).is_err());
        // An encrypted recording from disk (OBA1) or a WAV.
        let mut oba1 = b"OBA1".to_vec();
        oba1.extend_from_slice(&[0; 200]);
        assert!(check_request(&json!({"items": [{"body": keys::b64(&oba1)}]})).is_err());
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend_from_slice(&[0; 64]);
        assert!(check_request(&json!({"items": [{"body": keys::b64(&wav)}]})).is_err());
        // Free text in a structural field, or a forbidden field name.
        assert!(check_request(&json!({"kind": "Maple Okonkwo, cat, vomiting"})).is_err());
        assert!(check_request(&json!({"audio": "x"})).is_err());
        assert!(check_request(&json!({"name": "Front desk"})).is_err());
        // Keys must be exactly 32 bytes.
        assert!(check_request(&json!({"x25519_pub": keys::b64(&[1; 31])})).is_err());
        check_request(&json!({"x25519_pub": keys::b64(&[1; 32])})).unwrap();
        // Not a JSON object at all.
        assert!(check_request(&json!("hello")).is_err());
    }

    #[test]
    fn sign_in_allows_only_its_fields() {
        check_auth(&json!({"email": "vet@example.org", "create_user": true})).unwrap();
        check_auth(&json!({"email": "vet@example.org", "token": "123456", "type": "email"})).unwrap();
        assert!(check_auth(&json!({"email": "vet@example.org", "data": {"clinic": "x"}})).is_err());
    }

    #[test]
    fn recordings_are_refused_by_name_and_by_bytes() {
        assert!(refuse_audio_file("audio.wav", b"OBK1....").is_err());
        assert!(refuse_audio_file("audio.wav.tmp", b"").is_err());
        assert!(refuse_audio_file("call.m4a", b"").is_err());
        assert!(refuse_audio_file("note.json", b"RIFF....WAVE").is_err());
        assert!(refuse_audio_file("note.json", b"\0\0\0\x20ftypM4A ").is_err());
        assert!(refuse_audio_file("note.json", b"OBA1\0\0\x01\0").is_err());
        refuse_audio_file("note.json", b"{\"note\":{}}").unwrap();
        refuse_audio_file("note.md", "## Plan\n".as_bytes()).unwrap();
    }
}
