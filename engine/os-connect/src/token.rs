//! Voice SDK access tokens (ADR-0023 sections 1 and 10).
//!
//! The engine is the token server: the API key secret stays in Rust and the
//! phone window only ever holds a token that expires within an hour.
//!
//! Format, per https://www.twilio.com/docs/iam/access-tokens and twilio-node's
//! `AccessToken` (checked 2026-10-18):
//! - a JWT signed with HS256 using the API key secret;
//! - header `{"alg":"HS256","typ":"JWT","cty":"twilio-fpa;v=1"}`;
//! - claims `jti` (key SID, a dash, the issue time), `grants` (`identity`,
//!   and `voice` with `outgoing.application_sid` and, when allowed,
//!   `incoming.allow`), `iat`, `exp`, `iss` (the key SID) and `sub` (the
//!   account SID). Tokens live at most 24 hours; OpenBark uses one hour.
//!
//! Fields are written in twilio-node's order, so a token minted here is
//! byte-for-byte the one Twilio's helper library makes for the same inputs
//! (`fixtures/twilio-node-access-tokens.json`).

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use sha2::Sha256;

use crate::ConnectError;
use crate::secrets::Secret;
use crate::settings::check_sid;

/// How long a token lives: one hour (ADR-0023 section 10). The phone window
/// asks for a fresh one when the SDK warns that it will expire.
pub const TOKEN_TTL_SECS: u64 = 3600;

/// Twilio's ceiling.
pub const MAX_TTL_SECS: u64 = 24 * 3600;

/// Longest identity Twilio accepts for a client (https://www.twilio.com/docs/voice/twiml/client).
const MAX_IDENTITY: usize = 256;

/// What the token lets the phone window do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceGrant {
    /// The "OpenBark" TwiML App (`AP...`), whose Voice URL is the outgoing Bin.
    pub outgoing_application_sid: String,
    /// Whether this computer may receive calls. Off in the first release,
    /// which makes calls only (ADR-0023 section 15, decision 2).
    pub incoming_allow: bool,
}

/// Everything a token is made from.
#[derive(Debug, Clone)]
pub struct TokenRequest<'a> {
    pub account_sid: &'a str,
    pub api_key_sid: &'a str,
    pub api_key_secret: &'a Secret,
    /// This computer's identity ([`crate::settings::identity_for`]).
    pub identity: &'a str,
    pub grant: VoiceGrant,
    pub ttl_secs: u64,
}

/// A minted token. It is a credential for an hour, so `Debug` leaves it out.
#[derive(Clone, PartialEq, Eq)]
pub struct AccessToken {
    jwt: String,
    pub identity: String,
    /// Seconds since 1970 (UTC).
    pub issued_at: u64,
    /// Seconds since 1970 (UTC).
    pub expires_at: u64,
}

impl AccessToken {
    /// The JWT, for the phone window's `Device` only. Never log it.
    pub fn jwt(&self) -> &str {
        &self.jwt
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessToken")
            .field("identity", &self.identity)
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .field("jwt", &"<withheld>")
            .finish()
    }
}

#[derive(Serialize)]
struct Header {
    alg: &'static str,
    typ: &'static str,
    cty: &'static str,
}

#[derive(Serialize)]
struct Claims<'a> {
    jti: String,
    grants: Grants<'a>,
    iat: u64,
    exp: u64,
    iss: &'a str,
    sub: &'a str,
}

#[derive(Serialize)]
struct Grants<'a> {
    identity: &'a str,
    voice: Voice<'a>,
}

#[derive(Serialize)]
struct Voice<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    incoming: Option<Incoming>,
    outgoing: Outgoing<'a>,
}

#[derive(Serialize)]
struct Incoming {
    allow: bool,
}

#[derive(Serialize)]
struct Outgoing<'a> {
    application_sid: &'a str,
}

/// Mints a token issued at `now` (seconds since 1970).
pub fn mint(req: &TokenRequest<'_>, now: u64) -> Result<AccessToken, ConnectError> {
    check_sid(req.account_sid, "AC", "Account SID")?;
    check_sid(req.api_key_sid, "SK", "API key SID")?;
    check_sid(&req.grant.outgoing_application_sid, "AP", "TwiML App SID")?;
    check_identity(req.identity)?;
    if req.api_key_secret.is_empty() {
        return Err(ConnectError::Invalid("The API key secret is missing. Connect Twilio again.".into()));
    }
    if req.ttl_secs == 0 || req.ttl_secs > MAX_TTL_SECS {
        return Err(ConnectError::Invalid(format!(
            "A token lives between 1 second and {MAX_TTL_SECS} seconds (24 hours)."
        )));
    }

    let header = Header { alg: "HS256", typ: "JWT", cty: "twilio-fpa;v=1" };
    let exp = now + req.ttl_secs;
    let claims = Claims {
        jti: format!("{}-{now}", req.api_key_sid),
        grants: Grants {
            identity: req.identity,
            voice: Voice {
                incoming: req.grant.incoming_allow.then_some(Incoming { allow: true }),
                outgoing: Outgoing { application_sid: &req.grant.outgoing_application_sid },
            },
        },
        iat: now,
        exp,
        iss: req.api_key_sid,
        sub: req.account_sid,
    };

    let encode = |json: serde_json::Result<Vec<u8>>| {
        json.map(|bytes| URL_SAFE_NO_PAD.encode(bytes)).map_err(|e| ConnectError::Invalid(e.to_string()))
    };
    let signing_input =
        format!("{}.{}", encode(serde_json::to_vec(&header))?, encode(serde_json::to_vec(&claims))?);
    let signature = sign(req.api_key_secret, signing_input.as_bytes())?;
    Ok(AccessToken {
        jwt: format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(signature)),
        identity: req.identity.to_string(),
        issued_at: now,
        expires_at: exp,
    })
}

/// Mints a token issued now.
pub fn mint_now(req: &TokenRequest<'_>) -> Result<AccessToken, ConnectError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ConnectError::Invalid("This computer's clock is set before 1970.".into()))?
        .as_secs();
    mint(req, now)
}

/// HMAC-SHA256 with the API key secret (its UTF-8 bytes, as twilio-node uses).
fn sign(secret: &Secret, message: &[u8]) -> Result<Vec<u8>, ConnectError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.expose().as_bytes())
        .map_err(|_| ConnectError::Invalid("The API key secret can't be used to sign.".into()))?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().to_vec())
}

/// Twilio: "Voice tokens may only contain alpha-numeric and underscore characters."
fn check_identity(identity: &str) -> Result<(), ConnectError> {
    let ok = (1..=MAX_IDENTITY).contains(&identity.len())
        && identity.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(ConnectError::Invalid(
            "This computer's identity may only use letters, digits and underscores.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::tests::{ACCOUNT, APP, KEY};
    use serde_json::{Value, json};

    const FIXTURE: &str = include_str!("../fixtures/twilio-node-access-tokens.json");

    /// The fixture, put back together. It stores each SID without its prefix and
    /// each token in pieces, so secret scanners don't flag made-up test data.
    struct Fixture {
        account_sid: String,
        api_key_sid: String,
        api_key_secret: String,
        outgoing_application_sid: String,
        identity: String,
        ttl: u64,
        now: u64,
        outgoing_only: String,
        with_incoming: String,
    }

    fn fixture() -> Fixture {
        let f: Value = serde_json::from_str(FIXTURE).unwrap();
        let i = &f["inputs"];
        let text = |v: &Value| v.as_str().unwrap().to_string();
        let header: String = f["jwt_header_pieces"].as_array().unwrap().iter().map(text).collect();
        let jwt = |name: &str| {
            let t = &f[name];
            [header.clone(), text(&t["payload"]), text(&t["signature"])].join(".")
        };
        Fixture {
            account_sid: format!("AC{}", text(&i["account_sid_after_AC"])),
            api_key_sid: format!("SK{}", text(&i["api_key_sid_after_SK"])),
            api_key_secret: text(&i["api_key_secret"]),
            outgoing_application_sid: format!("AP{}", text(&i["outgoing_application_sid_after_AP"])),
            identity: text(&i["identity"]),
            ttl: i["ttl"].as_u64().unwrap(),
            now: i["now"].as_u64().unwrap(),
            outgoing_only: jwt("outgoing_only"),
            with_incoming: jwt("with_incoming"),
        }
    }

    fn request<'a>(f: &'a Fixture, secret: &'a Secret, incoming: bool) -> TokenRequest<'a> {
        TokenRequest {
            account_sid: &f.account_sid,
            api_key_sid: &f.api_key_sid,
            api_key_secret: secret,
            identity: &f.identity,
            grant: VoiceGrant {
                outgoing_application_sid: f.outgoing_application_sid.clone(),
                incoming_allow: incoming,
            },
            ttl_secs: f.ttl,
        }
    }

    fn decode(part: &str) -> Value {
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    }

    #[test]
    fn matches_the_token_twilios_own_library_makes() {
        let f = fixture();
        let secret = Secret::new(f.api_key_secret.as_str());
        let out = mint(&request(&f, &secret, false), f.now).unwrap();
        assert_eq!(out.jwt(), f.outgoing_only);
        let both = mint(&request(&f, &secret, true), f.now).unwrap();
        assert_eq!(both.jwt(), f.with_incoming);
    }

    #[test]
    fn header_claims_and_signature_are_as_twilio_documents() {
        let f = fixture();
        let secret = Secret::new(f.api_key_secret.as_str());
        let now = 1_792_324_800;
        let token = mint(&request(&f, &secret, false), now).unwrap();
        assert_eq!(token.issued_at, now);
        assert_eq!(token.expires_at, now + 3600);
        assert_eq!(token.identity, "openbark_front_desk");

        let parts: Vec<&str> = token.jwt().split('.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(decode(parts[0]), json!({"alg": "HS256", "typ": "JWT", "cty": "twilio-fpa;v=1"}));
        let claims = decode(parts[1]);
        assert_eq!(
            claims,
            json!({
                "jti": format!("{KEY}-1792324800"),
                "grants": {
                    "identity": "openbark_front_desk",
                    "voice": {"outgoing": {"application_sid": APP}}
                },
                "iat": 1792324800,
                "exp": 1792328400,
                "iss": KEY,
                "sub": ACCOUNT
            })
        );
        assert!(claims["grants"]["voice"].get("incoming").is_none(), "calls out only by default");

        // The signature checks out with the secret, and not with another.
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let sig = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.expose().as_bytes()).unwrap();
        mac.update(signing_input.as_bytes());
        mac.verify_slice(&sig).unwrap();
        let mut other = Hmac::<Sha256>::new_from_slice(b"another-secret-entirely-000000000").unwrap();
        other.update(signing_input.as_bytes());
        assert!(other.verify_slice(&sig).is_err());
        // The token never carries the secret.
        assert!(!token.jwt().contains(secret.expose()));
        assert!(!format!("{token:?}").contains(parts[2]), "Debug withholds the JWT");
    }

    #[test]
    fn bad_inputs_are_refused() {
        let f = fixture();
        let secret = Secret::new("fixture-secret-not-a-real-key-000");
        let base = || request(&f, &secret, false);
        let mut r = base();
        r.identity = "openbark-front-desk";
        assert!(mint(&r, 0).unwrap_err().to_string().contains("underscores"), "Twilio rejects dashes");
        let mut r = base();
        r.identity = "";
        assert!(mint(&r, 0).is_err());
        let mut r = base();
        r.account_sid = KEY;
        assert!(mint(&r, 0).is_err());
        let mut r = base();
        r.grant.outgoing_application_sid = "APnope".into();
        assert!(mint(&r, 0).is_err());
        let mut r = base();
        r.ttl_secs = MAX_TTL_SECS + 1;
        assert!(mint(&r, 0).is_err());
        let empty = Secret::new("");
        let mut r = base();
        r.api_key_secret = &empty;
        assert!(mint(&r, 0).is_err());
    }

    #[test]
    fn minting_now_uses_the_clock() {
        let f = fixture();
        let secret = Secret::new("fixture-secret-not-a-real-key-000");
        let t = mint_now(&request(&f, &secret, false)).unwrap();
        assert_eq!(t.expires_at - t.issued_at, TOKEN_TTL_SECS);
        assert!(t.issued_at > 1_760_000_000);
    }
}
