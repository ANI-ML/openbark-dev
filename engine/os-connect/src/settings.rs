//! The clinic's Twilio settings that aren't secret (ADR-0023 section 10).
//!
//! The API key secret is not here: it lives in the keychain
//! ([`crate::secrets`]). Everything here can be shown in Settings.

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::ConnectError;
use crate::secrets::Secret;

/// Prefix of this computer's Voice identity, so OpenBark's clients stand out
/// in the clinic's Twilio logs.
pub const IDENTITY_PREFIX: &str = "openbark_";

/// Longest computer name accepted.
pub const MAX_COMPUTER_NAME: usize = 32;

/// The non-secret Twilio settings, saved by the engine (never in the
/// browser's storage).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwilioSettings {
    /// `AC` and 32 hex digits, from the Twilio console's home page.
    pub account_sid: String,
    /// `SK` and 32 hex digits: a Standard key made for this computer.
    pub api_key_sid: String,
    /// This computer's name, a place rather than a person ("front-desk").
    /// The Voice identity is made from it ([`identity_for`]).
    pub computer_name: String,
    /// The number calls come from, in E.164: a Twilio number or a verified
    /// caller ID. It goes into the outgoing Bin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_id: Option<String>,
    /// The URL of the clinic's "OpenBark outgoing" TwiML Bin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outgoing_bin_url: Option<String>,
    /// The URL of the "OpenBark incoming" Bin. Empty for calls out only, which
    /// is all the first release does (ADR-0023 section 15, decision 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incoming_bin_url: Option<String>,
    /// The "OpenBark" TwiML App, once setup has made it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twiml_app_sid: Option<String>,
}

impl TwilioSettings {
    /// Tidies what was typed: trims every field, drops empty optional ones,
    /// writes a caller ID without spaces or punctuation, and turns a
    /// computer name like "Front desk" into "front-desk".
    pub fn normalised(self) -> Self {
        let opt = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        Self {
            account_sid: self.account_sid.trim().to_string(),
            api_key_sid: self.api_key_sid.trim().to_string(),
            computer_name: normalise_computer_name(&self.computer_name),
            caller_id: opt(self.caller_id).map(|c| normalise_phone(&c)),
            outgoing_bin_url: opt(self.outgoing_bin_url),
            incoming_bin_url: opt(self.incoming_bin_url),
            twiml_app_sid: opt(self.twiml_app_sid),
        }
    }

    /// Checks every field. The error names the field and what to fix.
    pub fn validate(&self) -> Result<(), ConnectError> {
        check_sid(&self.account_sid, "AC", "Account SID")?;
        check_sid(&self.api_key_sid, "SK", "API key SID")?;
        check_computer_name(&self.computer_name)?;
        if let Some(c) = &self.caller_id {
            check_e164(c)?;
        }
        if let Some(u) = &self.outgoing_bin_url {
            check_bin_url(u, "Outgoing Bin URL")?;
        }
        if let Some(u) = &self.incoming_bin_url {
            check_bin_url(u, "Incoming Bin URL")?;
        }
        if let Some(a) = &self.twiml_app_sid {
            check_sid(a, "AP", "TwiML App SID")?;
        }
        Ok(())
    }

    /// This computer's Voice identity, for its access tokens and the incoming Bin.
    pub fn identity(&self) -> String {
        identity_for(&self.computer_name)
    }

    /// The key as Settings shows it: "SK…4f2a".
    pub fn api_key_hint(&self) -> String {
        let tail: String =
            self.api_key_sid.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        format!("SK…{tail}")
    }
}

/// The Voice identity for a computer name: `openbark_` and the name, with
/// dashes as underscores. Twilio allows only letters, digits and underscores
/// in Voice identities ("Voice tokens may only contain alpha-numeric and
/// underscore characters", https://www.twilio.com/docs/iam/access-tokens),
/// so "front-desk" becomes `openbark_front_desk`.
pub fn identity_for(computer_name: &str) -> String {
    format!("{IDENTITY_PREFIX}{}", computer_name.replace('-', "_"))
}

/// "Front desk " → "front-desk".
pub fn normalise_computer_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join("-").to_lowercase()
}

/// "+1 (555) 555-0100" → "+15555550100". Anything else is left for
/// [`check_e164`] to refuse.
pub fn normalise_phone(number: &str) -> String {
    number.chars().filter(|c| !matches!(c, ' ' | '-' | '(' | ')' | '.')).collect()
}

/// A Twilio SID: a two-letter prefix and 32 hex digits.
pub fn check_sid(value: &str, prefix: &str, label: &str) -> Result<(), ConnectError> {
    let ok =
        value.len() == 34 && value.starts_with(prefix) && value[2..].bytes().all(|b| b.is_ascii_hexdigit());
    if ok {
        Ok(())
    } else {
        Err(ConnectError::Invalid(format!(
            "The {label} should be {prefix} followed by 32 letters and digits. Copy it again from your Twilio console."
        )))
    }
}

/// A phone number in E.164: "+", then up to 15 digits, not starting with 0.
pub fn check_e164(value: &str) -> Result<(), ConnectError> {
    let digits = value.strip_prefix('+').unwrap_or("");
    let ok = (2..=15).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.starts_with('0');
    if ok {
        Ok(())
    } else {
        Err(ConnectError::Invalid(format!(
            "{value} isn't a full phone number. Write it with + and the country code, for example +15555550100."
        )))
    }
}

/// A TwiML Bin URL: HTTPS, on a Twilio host, under `/twiml/`. Catches the
/// usual slip of pasting the console page's address instead.
pub fn check_bin_url(value: &str, label: &str) -> Result<(), ConnectError> {
    let bad = || {
        ConnectError::Invalid(format!(
            "The {label} should be the Bin's own URL, which starts with https://handler.twilio.com/twiml/. Copy it from the Bin's page in your Twilio console."
        ))
    };
    let url = Url::parse(value).map_err(|_| bad())?;
    let host = url.host_str().unwrap_or("");
    let twilio_host = host == "twilio.com" || host.ends_with(".twilio.com");
    if url.scheme() != "https"
        || !twilio_host
        || !url.path().starts_with("/twiml/")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(bad());
    }
    Ok(())
}

/// A computer name: 1 to 32 lowercase letters, digits, dashes or
/// underscores, starting and ending with a letter or digit.
pub fn check_computer_name(name: &str) -> Result<(), ConnectError> {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let ok = (1..=MAX_COMPUTER_NAME).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && edge(name.chars().next())
        && edge(name.chars().last());
    if ok {
        Ok(())
    } else {
        Err(ConnectError::Invalid(format!(
            "Name this computer after a place, in lowercase letters, digits and dashes, up to {MAX_COMPUTER_NAME} characters. For example: front-desk."
        )))
    }
}

/// A Twilio API key secret: something was pasted, and it has no spaces inside.
pub fn check_api_key_secret(secret: &Secret) -> Result<(), ConnectError> {
    let v = secret.expose();
    if v.is_empty() || v.chars().any(|c| c.is_whitespace() || c.is_control()) || v.len() > 256 {
        return Err(ConnectError::Invalid(
            "Paste the API key secret Twilio showed when you made the key. If you no longer have it, make a new key."
                .into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const ACCOUNT: &str = "AC0123456789abcdef0123456789abcdef";
    pub const KEY: &str = "SK0123456789abcdef0123456789abcdef";
    pub const APP: &str = "AP0123456789abcdef0123456789abcdef";
    pub const BIN_OUT: &str = "https://handler.twilio.com/twiml/EH0123456789abcdef0123456789abcdef";
    pub const BIN_IN: &str = "https://handler.twilio.com/twiml/EHfedcba9876543210fedcba9876543210";

    pub fn sample() -> TwilioSettings {
        TwilioSettings {
            account_sid: ACCOUNT.into(),
            api_key_sid: KEY.into(),
            computer_name: "front-desk".into(),
            caller_id: Some("+15555550100".into()),
            outgoing_bin_url: Some(BIN_OUT.into()),
            incoming_bin_url: None,
            twiml_app_sid: None,
        }
    }

    #[test]
    fn typed_values_are_tidied_then_checked() {
        let typed = TwilioSettings {
            account_sid: format!("  {ACCOUNT}\n"),
            api_key_sid: format!("{KEY} "),
            computer_name: " Front  Desk ".into(),
            caller_id: Some("+1 (555) 555-0100".into()),
            outgoing_bin_url: Some(format!(" {BIN_OUT} ")),
            incoming_bin_url: Some("   ".into()),
            twiml_app_sid: Some(String::new()),
        };
        let s = typed.normalised();
        assert_eq!(s, sample());
        s.validate().unwrap();
        assert_eq!(s.identity(), "openbark_front_desk");
        assert_eq!(s.api_key_hint(), "SK…cdef");
    }

    #[test]
    fn each_field_is_checked() {
        let bad = |f: fn(&mut TwilioSettings)| {
            let mut s = sample();
            f(&mut s);
            s.validate().unwrap_err().to_string()
        };
        assert!(bad(|s| s.account_sid = KEY.into()).contains("Account SID"));
        assert!(bad(|s| s.api_key_sid = "SK123".into()).contains("API key SID"));
        assert!(bad(|s| s.api_key_sid = format!("SK{}", "z".repeat(32))).contains("API key SID"));
        assert!(bad(|s| s.computer_name = "Exam room's PC".into()).contains("front-desk"));
        assert!(bad(|s| s.computer_name = "-desk".into()).contains("front-desk"));
        assert!(bad(|s| s.computer_name = "a".repeat(33)).contains("front-desk"));
        assert!(bad(|s| s.caller_id = Some("5555550100".into())).contains("country code"));
        assert!(bad(|s| s.caller_id = Some("+05555550100".into())).contains("country code"));
        assert!(bad(|s| s.caller_id = Some(format!("+1{}", "5".repeat(15)))).contains("country code"));
        assert!(bad(|s| s.twiml_app_sid = Some(ACCOUNT.into())).contains("TwiML App SID"));
    }

    #[test]
    fn bin_urls_must_be_the_bins_own_address() {
        check_bin_url(BIN_OUT, "Outgoing Bin URL").unwrap();
        for wrong in [
            "http://handler.twilio.com/twiml/EH0123", // not HTTPS
            "https://console.twilio.com/us1/develop/twiml-bins/twiml-bins/EH01", // the console page
            "https://handler.twilio.com.example.net/twiml/EH0123", // not Twilio's host
            "https://example.com/twiml/EH0123",
            "https://user:pw@handler.twilio.com/twiml/EH0123",
            "handler.twilio.com/twiml/EH0123",
        ] {
            let e = check_bin_url(wrong, "Outgoing Bin URL").unwrap_err().to_string();
            assert!(e.contains("https://handler.twilio.com/twiml/"), "{wrong}: {e}");
        }
    }

    #[test]
    fn identities_only_use_what_twilio_allows() {
        for name in ["front-desk", "exam_2", "lab3"] {
            check_computer_name(name).unwrap();
            let id = identity_for(name);
            assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "{id}");
        }
        assert_eq!(identity_for("exam-room-2"), "openbark_exam_room_2");
    }

    #[test]
    fn secrets_must_be_pasted_whole() {
        check_api_key_secret(&Secret::new("abcdefghijklmnopqrstuvwxyz012345")).unwrap();
        assert!(check_api_key_secret(&Secret::new("")).is_err());
        assert!(check_api_key_secret(&Secret::new("abc def")).is_err());
    }

    #[test]
    fn settings_save_as_plain_json_fields_without_the_secret() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert_eq!(
            json,
            format!(
                r#"{{"account_sid":"{ACCOUNT}","api_key_sid":"{KEY}","computer_name":"front-desk","caller_id":"+15555550100","outgoing_bin_url":"{BIN_OUT}"}}"#
            )
        );
        let back: TwilioSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sample());
    }
}
