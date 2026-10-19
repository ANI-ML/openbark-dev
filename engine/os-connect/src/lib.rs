//! The clinic's own Twilio account (ADR-0023).
//!
//! The clinic signs up with Twilio, pays Twilio, and pastes its own keys into
//! OpenBark. This crate is the engine's side of that:
//!
//! - [`settings`]: the non-secret settings (account SID, API key SID, caller
//!   ID, Bin URLs, TwiML App SID, this computer's name) and their checks;
//! - [`secrets`]: the API key secret, kept in the OS keychain or, without
//!   one, sealed under the local data key in `connections.bin` (ADR-0008);
//! - [`store`]: where both are saved;
//! - [`token`]: Voice SDK access tokens, minted here so the secret never
//!   leaves the engine (ADR-0023 section 10);
//! - [`twilio`]: the REST calls for setup (docs/network.md R9);
//! - [`twiml`]: the TwiML Bin text the clinic pastes into its console.
//!
//! Nothing here runs until the clinic connects Twilio in Settings. ANI.ML runs
//! no relay and no token server: the only parties are this computer and the
//! clinic's Twilio account.
//!
//! The REST client is blocking (as `os-models` is). From async code, call it
//! inside `spawn_blocking`.

pub mod secrets;
pub mod settings;
pub mod store;
pub mod token;
pub mod twilio;
pub mod twiml;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod redaction_tests;
#[cfg(test)]
mod test_support;

pub use secrets::{Keychain, OsKeychain, Secret, SecretId, SecretLocation, SecretStatus};
pub use settings::TwilioSettings;
pub use store::{ConnectStore, Removed, TwilioCredentials};
pub use token::{AccessToken, TOKEN_TTL_SECS, VoiceGrant};
pub use twilio::TwilioClient;

/// Everything that can go wrong. The messages are written for the clinic:
/// each says what happened and where to fix it. None of them ever holds a
/// secret.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// Something typed doesn't fit; the message says what to fix.
    #[error("{0}")]
    Invalid(String),
    /// Keeping or reading the settings or a secret on this computer failed.
    #[error("{0}")]
    Storage(String),
    /// Twilio error 20003, or HTTP 401.
    #[error(
        "Twilio didn't accept this key. Check the SID and secret, or make a new key in your Twilio console."
    )]
    WrongKey,
    /// Twilio error 20005: suspended, closed, or out of funds.
    #[error(
        "Twilio refused the request: your account needs attention. Check billing in your Twilio console."
    )]
    AccountNotActive,
    /// Twilio error 21219: a trial account calling a number it hasn't verified.
    #[error(
        "Your Twilio account is a trial, and trials can't connect calls through OpenBark. Upgrade it in your Twilio console."
    )]
    TrialAccount,
    /// Twilio error 21215.
    #[error(
        "Twilio blocked a call to this country. Turn it on in your Twilio console under Voice > Settings > Geo permissions."
    )]
    GeoPermission,
    /// Twilio error 21210.
    #[error(
        "Twilio hasn't verified this caller ID. Verify it in your Twilio console under Phone Numbers > Verified Caller IDs, or choose one of your Twilio numbers."
    )]
    CallerIdNotVerified,
    /// The chosen caller ID is neither a number in the account nor a verified caller ID.
    #[error(
        "{0} isn't one of this Twilio account's numbers or verified caller IDs. Buy it or verify it in your Twilio console, or choose another."
    )]
    CallerIdNotInAccount(String),
    /// Twilio error 20404, or HTTP 404.
    #[error("Twilio couldn't find {0} in this account.")]
    NotFound(String),
    /// Twilio error 20429, or HTTP 429.
    #[error("Twilio is getting too many requests from this account. Wait a minute and try again.")]
    RateLimited,
    /// Any other error Twilio returned.
    #[error("Twilio returned an error (HTTP {status}{}): {message}", code.map(|c| format!(", code {c}")).unwrap_or_default())]
    Twilio { status: u16, code: Option<u32>, message: String },
    /// Twilio couldn't be reached at all.
    #[error("Can't reach Twilio. Check this computer's internet connection. ({0})")]
    Network(String),
    /// Twilio answered with something this version doesn't understand.
    #[error("Twilio sent a reply OpenBark didn't expect: {0}")]
    BadResponse(String),
}
