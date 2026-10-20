//! The Twilio REST calls for setup (ADR-0023 section 3; docs/network.md R9).
//!
//! All against `https://api.twilio.com/2010-04-01/Accounts/{AccountSid}/`,
//! with basic auth: the API key SID and its secret. Checked against Twilio's
//! API reference on 2026-10-18:
//! - Applications: https://www.twilio.com/docs/usage/api/applications
//! - IncomingPhoneNumbers: https://www.twilio.com/docs/phone-numbers/api/incomingphonenumber-resource
//! - OutgoingCallerIds: https://www.twilio.com/docs/voice/api/outgoing-caller-ids
//! - Calls: https://www.twilio.com/docs/voice/api/call-resource
//! - Errors: https://www.twilio.com/docs/usage/twilios-response
//!
//! The key is checked by listing Applications, not by reading the account: a
//! Standard key (the kind ADR-0023 asks for) can't read the Accounts resource
//! and gets error 20003 there, the same code as a wrong key. So the account
//! type (trial or upgraded) can't be read either; a trial account shows up as
//! error 21219 when a call is placed.
//!
//! The secret goes only to `https://api.twilio.com` (ADR-0023 section 10).
//! A loopback address is also allowed, for tests against a local mock.
//! Redirects are never followed, so the `Authorization` header can't be
//! carried anywhere else.

use std::fmt;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::{Method, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

use crate::ConnectError;
use crate::secrets::Secret;
use crate::settings::{TwilioSettings, check_bin_url, check_e164, check_sid};

/// Twilio's REST API.
pub const API_BASE: &str = "https://api.twilio.com";
/// The name of the TwiML App OpenBark makes in the clinic's account.
pub const APP_FRIENDLY_NAME: &str = "OpenBark";

const API_VERSION: &str = "2010-04-01";
const PAGE_SIZE: &str = "100";
/// Enough for 2,000 numbers or caller IDs; a clinic has a handful.
const MAX_PAGES: usize = 20;
/// How much of an unexpected Twilio message is kept in an error.
const MAX_MESSAGE: usize = 300;

/// A TwiML App.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwimlApp {
    pub sid: String,
    #[serde(default)]
    pub friendly_name: String,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub voice_url: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub voice_method: Option<String>,
}

/// What [`TwilioClient::ensure_app`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppChange {
    Created,
    Updated,
    Unchanged,
}

/// A number the clinic bought from Twilio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingNumber {
    pub sid: String,
    pub phone_number: String,
    #[serde(default)]
    pub friendly_name: String,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub voice_url: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub voice_application_sid: Option<String>,
    #[serde(default)]
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub voice: bool,
}

/// A number the clinic verified in Twilio, so calls can show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedCallerId {
    pub sid: String,
    pub phone_number: String,
    #[serde(default)]
    pub friendly_name: String,
}

/// Where a caller ID comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerIdKind {
    /// A number bought from Twilio: calls out, and later calls in.
    TwilioNumber,
    /// The clinic's own number, verified in Twilio: calls out only. Call-backs
    /// still reach the clinic's usual phones.
    Verified,
}

/// A number calls can come from, for the caller ID picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallerIdOption {
    pub phone_number: String,
    pub friendly_name: String,
    pub kind: CallerIdKind,
}

/// What [`TwilioClient::connect_outgoing`] found and did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutgoingSetup {
    pub app: TwimlApp,
    pub app_change: AppChange,
    pub caller_id: CallerIdOption,
}

/// A client for one Twilio account.
pub struct TwilioClient {
    http: Client,
    base: Url,
    account_sid: String,
    api_key_sid: String,
    secret: Secret,
}

impl fmt::Debug for TwilioClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioClient")
            .field("base", &self.base.as_str())
            .field("account_sid", &self.account_sid)
            .field("api_key_sid", &self.api_key_sid)
            .field("secret", &self.secret)
            .finish()
    }
}

impl TwilioClient {
    /// A client for `https://api.twilio.com`.
    pub fn new(account_sid: &str, api_key_sid: &str, secret: Secret) -> Result<Self, ConnectError> {
        Self::with_base_url(API_BASE, account_sid, api_key_sid, secret)
    }

    /// A client for [`api_base`]: `https://api.twilio.com`, unless
    /// `OPENBARK_TWILIO_API_BASE` names a loopback mock (demos and the
    /// end-to-end test). Any other value is refused, so the secret still goes
    /// only to Twilio or to this computer.
    pub fn for_account(account_sid: &str, api_key_sid: &str, secret: Secret) -> Result<Self, ConnectError> {
        Self::with_base_url(&api_base(), account_sid, api_key_sid, secret)
    }

    /// A client for another base URL: only `https://api.twilio.com` or a
    /// loopback address (a local mock in tests).
    pub fn with_base_url(
        base: &str,
        account_sid: &str,
        api_key_sid: &str,
        secret: Secret,
    ) -> Result<Self, ConnectError> {
        check_sid(account_sid, "AC", "Account SID")?;
        check_sid(api_key_sid, "SK", "API key SID")?;
        crate::settings::check_api_key_secret(&secret)?;
        let base = Url::parse(base).map_err(|_| refused_base())?;
        let host = base.host_str().unwrap_or("");
        let loopback = matches!(host, "127.0.0.1" | "localhost" | "[::1]");
        let twilio = base.scheme() == "https" && host == "api.twilio.com" && base.port().is_none();
        if !(twilio || loopback && matches!(base.scheme(), "http" | "https"))
            || !base.username().is_empty()
            || base.password().is_some()
        {
            return Err(refused_base());
        }
        let mut builder = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("OpenBark/", env!("CARGO_PKG_VERSION")));
        if loopback {
            builder = builder.no_proxy();
        }
        let http = builder.build().map_err(|e| ConnectError::Network(e.without_url().to_string()))?;
        Ok(Self {
            http,
            base,
            account_sid: account_sid.to_string(),
            api_key_sid: api_key_sid.to_string(),
            secret,
        })
    }

    /// The account's URL for `rest` (which starts with `/`, or is empty).
    fn url(&self, rest: &str) -> Url {
        let path = format!("/{API_VERSION}/Accounts/{}{rest}", self.account_sid);
        self.base.join(&path).expect("a path on a valid base URL")
    }

    /// Sends one request and returns the body of a 2xx reply. `what` names
    /// the thing asked for, for a "couldn't find" message.
    fn send(
        &self,
        method: Method,
        url: Url,
        form: &[(&str, &str)],
        what: &str,
    ) -> Result<String, ConnectError> {
        let path = url.path().to_string();
        let mut req = self
            .http
            .request(method.clone(), url)
            .basic_auth(&self.api_key_sid, Some(self.secret.expose()))
            .header(ACCEPT, "application/json");
        if method == Method::POST {
            req = req.header(CONTENT_TYPE, "application/x-www-form-urlencoded").body(form_body(form));
        }
        let res = req.send().map_err(|e| ConnectError::Network(e.without_url().to_string()))?;
        let status = res.status().as_u16();
        let body = res.text().map_err(|e| ConnectError::Network(e.without_url().to_string()))?;
        tracing::debug!(%method, %path, status, "twilio");
        if (200..300).contains(&status) { Ok(body) } else { Err(api_error(status, &body, what)) }
    }

    fn send_json<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        form: &[(&str, &str)],
        what: &str,
    ) -> Result<T, ConnectError> {
        let body = self.send(method, url, form, what)?;
        serde_json::from_str(&body).map_err(|e| ConnectError::BadResponse(e.to_string()))
    }

    /// Every item of a list, following `next_page_uri`.
    fn list<T: DeserializeOwned>(
        &self,
        rest: &str,
        key: &str,
        filters: &[(&str, &str)],
    ) -> Result<Vec<T>, ConnectError> {
        let mut url = self.url(rest);
        url.query_pairs_mut().extend_pairs(filters).append_pair("PageSize", PAGE_SIZE);
        let prefix = format!("/{API_VERSION}/Accounts/{}/", self.account_sid);
        let mut out = Vec::new();
        for _ in 0..MAX_PAGES {
            let mut page: serde_json::Value = self.send_json(Method::GET, url, &[], "that list")?;
            let items = page
                .get_mut(key)
                .map(serde_json::Value::take)
                .ok_or_else(|| ConnectError::BadResponse(format!("the reply has no \"{key}\" list")))?;
            let items: Vec<T> =
                serde_json::from_value(items).map_err(|e| ConnectError::BadResponse(e.to_string()))?;
            out.extend(items);
            match page.get("next_page_uri").and_then(serde_json::Value::as_str) {
                Some(next) if !next.is_empty() => {
                    // Only ever follow a page of the same account's resources.
                    if !next.starts_with(&prefix) {
                        return Err(ConnectError::BadResponse("a next page outside this account".into()));
                    }
                    url = self.base.join(next).map_err(|e| ConnectError::BadResponse(e.to_string()))?;
                }
                _ => return Ok(out),
            }
        }
        Err(ConnectError::BadResponse(format!("more than {MAX_PAGES} pages of {key}")))
    }

    /// Checks the key: lists one TwiML App, which a Standard key may do.
    /// A wrong SID or secret gives [`ConnectError::WrongKey`].
    pub fn check_key(&self) -> Result<(), ConnectError> {
        let mut url = self.url("/Applications.json");
        url.query_pairs_mut().append_pair("PageSize", "1");
        let page: serde_json::Value = self.send_json(Method::GET, url, &[], "this account")?;
        if page.get("applications").is_some_and(serde_json::Value::is_array) {
            Ok(())
        } else {
            Err(ConnectError::BadResponse("the reply has no \"applications\" list".into()))
        }
    }

    /// The numbers the clinic bought from Twilio.
    pub fn incoming_numbers(&self) -> Result<Vec<IncomingNumber>, ConnectError> {
        self.list("/IncomingPhoneNumbers.json", "incoming_phone_numbers", &[])
    }

    /// The clinic's verified caller IDs.
    pub fn verified_caller_ids(&self) -> Result<Vec<VerifiedCallerId>, ConnectError> {
        self.list("/OutgoingCallerIds.json", "outgoing_caller_ids", &[])
    }

    /// Every number calls can come from: Twilio numbers that can make calls,
    /// then verified caller IDs, without repeats.
    pub fn caller_ids(&self) -> Result<Vec<CallerIdOption>, ConnectError> {
        let mut out: Vec<CallerIdOption> = self
            .incoming_numbers()?
            .into_iter()
            .filter(|n| n.capabilities.voice)
            .map(|n| CallerIdOption {
                phone_number: n.phone_number,
                friendly_name: n.friendly_name,
                kind: CallerIdKind::TwilioNumber,
            })
            .collect();
        for v in self.verified_caller_ids()? {
            if !out.iter().any(|o| o.phone_number == v.phone_number) {
                out.push(CallerIdOption {
                    phone_number: v.phone_number,
                    friendly_name: v.friendly_name,
                    kind: CallerIdKind::Verified,
                });
            }
        }
        Ok(out)
    }

    /// Checks that `number` is one of the account's numbers or verified
    /// caller IDs, so Twilio will let calls show it.
    pub fn verify_caller_id(&self, number: &str) -> Result<CallerIdOption, ConnectError> {
        check_e164(number)?;
        self.caller_ids()?
            .into_iter()
            .find(|o| o.phone_number == number)
            .ok_or_else(|| ConnectError::CallerIdNotInAccount(number.to_string()))
    }

    /// One TwiML App, or `None` if it no longer exists.
    pub fn fetch_app(&self, sid: &str) -> Result<Option<TwimlApp>, ConnectError> {
        check_sid(sid, "AP", "TwiML App SID")?;
        match self.send_json(
            Method::GET,
            self.url(&format!("/Applications/{sid}.json")),
            &[],
            "the TwiML App",
        ) {
            Ok(app) => Ok(Some(app)),
            Err(ConnectError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The first TwiML App named "OpenBark", if any.
    pub fn find_app(&self) -> Result<Option<TwimlApp>, ConnectError> {
        let apps: Vec<TwimlApp> =
            self.list("/Applications.json", "applications", &[("FriendlyName", APP_FRIENDLY_NAME)])?;
        Ok(apps.into_iter().find(|a| a.friendly_name == APP_FRIENDLY_NAME))
    }

    /// Makes sure the "OpenBark" TwiML App exists and its Voice URL is the
    /// outgoing Bin. Uses `known_sid` (from settings) first, then looks the
    /// app up by name, then creates it.
    pub fn ensure_app(
        &self,
        known_sid: Option<&str>,
        outgoing_bin_url: &str,
    ) -> Result<(TwimlApp, AppChange), ConnectError> {
        check_bin_url(outgoing_bin_url, "Outgoing Bin URL")?;
        let existing = match known_sid {
            Some(sid) => match self.fetch_app(sid)? {
                Some(app) => Some(app),
                None => self.find_app()?,
            },
            None => self.find_app()?,
        };
        let form = [("VoiceUrl", outgoing_bin_url), ("VoiceMethod", "POST")];
        match existing {
            Some(app)
                if app.voice_url.as_deref() == Some(outgoing_bin_url)
                    && app.voice_method.as_deref().is_none_or(|m| m.eq_ignore_ascii_case("POST")) =>
            {
                Ok((app, AppChange::Unchanged))
            }
            Some(app) => {
                check_sid(&app.sid, "AP", "TwiML App SID")?;
                let url = self.url(&format!("/Applications/{}.json", app.sid));
                let updated = self.send_json(Method::POST, url, &form, "the TwiML App")?;
                Ok((updated, AppChange::Updated))
            }
            None => {
                let create = [("FriendlyName", APP_FRIENDLY_NAME), form[0], form[1]];
                let created = self.send_json(Method::POST, self.url("/Applications.json"), &create, "")?;
                Ok((created, AppChange::Created))
            }
        }
    }

    /// Points one Twilio number's calls at the incoming Bin. Clears the
    /// number's `VoiceApplicationSid`, because Twilio ignores the Voice URL
    /// while one is set. Only after the clinic confirms (ADR-0023 section 3);
    /// not used until incoming calls ship.
    pub fn set_number_voice_url(
        &self,
        number_sid: &str,
        incoming_bin_url: &str,
    ) -> Result<IncomingNumber, ConnectError> {
        check_sid(number_sid, "PN", "number SID")?;
        check_bin_url(incoming_bin_url, "Incoming Bin URL")?;
        let form = [("VoiceUrl", incoming_bin_url), ("VoiceMethod", "POST"), ("VoiceApplicationSid", "")];
        let url = self.url(&format!("/IncomingPhoneNumbers/{number_sid}.json"));
        self.send_json(Method::POST, url, &form, "that number")
    }

    /// Ends a live call (`Status=completed`): the caller's side of an
    /// answered incoming call, when OpenBark hangs up (ADR-0023 section 6).
    pub fn end_call(&self, call_sid: &str) -> Result<(), ConnectError> {
        check_sid(call_sid, "CA", "call SID")?;
        let url = self.url(&format!("/Calls/{call_sid}.json"));
        self.send(Method::POST, url, &[("Status", "completed")], "that call").map(|_| ())
    }

    /// Setup for calls out (the first release): checks the key, checks the
    /// chosen caller ID, and makes or updates the "OpenBark" TwiML App with
    /// the outgoing Bin. Saves the app's SID into `settings`.
    pub fn connect_outgoing(&self, settings: &mut TwilioSettings) -> Result<OutgoingSetup, ConnectError> {
        settings.validate()?;
        if settings.account_sid != self.account_sid || settings.api_key_sid != self.api_key_sid {
            return Err(ConnectError::Invalid(
                "These settings are for another Twilio account or key.".into(),
            ));
        }
        let caller_id = settings.caller_id.clone().ok_or_else(|| {
            ConnectError::Invalid("Choose the number calls will come from (the caller ID).".into())
        })?;
        let bin = settings.outgoing_bin_url.clone().ok_or_else(|| {
            ConnectError::Invalid("Paste the URL of your \"OpenBark outgoing\" Bin.".into())
        })?;
        self.check_key()?;
        let caller_id = self.verify_caller_id(&caller_id)?;
        let (app, app_change) = self.ensure_app(settings.twiml_app_sid.as_deref(), &bin)?;
        check_sid(&app.sid, "AP", "TwiML App SID")
            .map_err(|_| ConnectError::BadResponse("the TwiML App has no valid SID".into()))?;
        settings.twiml_app_sid = Some(app.sid.clone());
        Ok(OutgoingSetup { app, app_change, caller_id })
    }
}

/// Where the REST calls go: `OPENBARK_TWILIO_API_BASE` if set (only a
/// loopback address is then accepted, by [`TwilioClient::with_base_url`]),
/// otherwise [`API_BASE`].
pub fn api_base() -> String {
    // Only debug builds and the `mock` feature (tests and the demo) read it:
    // the builds clinics get always talk to api.twilio.com.
    #[cfg(any(debug_assertions, feature = "mock"))]
    if let Some(base) = std::env::var(API_BASE_ENV).ok().filter(|v| !v.trim().is_empty()) {
        return base;
    }
    API_BASE.to_string()
}

/// Points the REST calls at a local mock (loopback only).
pub const API_BASE_ENV: &str = "OPENBARK_TWILIO_API_BASE";

fn refused_base() -> ConnectError {
    ConnectError::Invalid("OpenBark only sends the Twilio key to https://api.twilio.com.".into())
}

/// `application/x-www-form-urlencoded`, without another crate: `Url`'s
/// query serializer is that encoding.
fn form_body(pairs: &[(&str, &str)]) -> String {
    let mut url = Url::parse("http://form.invalid/").expect("a static URL");
    url.query_pairs_mut().extend_pairs(pairs);
    url.query().unwrap_or("").to_string()
}

/// Twilio's error body: `{"code": 20003, "message": "...", "more_info": "...", "status": 401}`.
#[derive(Deserialize, Default)]
struct ApiErrorBody {
    code: Option<u32>,
    message: Option<String>,
}

/// Maps a Twilio error to its plain cause (docs/plans/telephony.md, "Errors").
fn api_error(status: u16, body: &str, what: &str) -> ConnectError {
    let e: ApiErrorBody = serde_json::from_str(body).unwrap_or_default();
    match (e.code, status) {
        (Some(20003), _) | (None, 401) => ConnectError::WrongKey,
        (Some(20005), _) => ConnectError::AccountNotActive,
        (Some(21219), _) => ConnectError::TrialAccount,
        (Some(21215), _) => ConnectError::GeoPermission,
        (Some(21210), _) => ConnectError::CallerIdNotVerified,
        (Some(20404), _) | (None, 404) => {
            ConnectError::NotFound(if what.is_empty() { "that" } else { what }.to_string())
        }
        (Some(20429), _) | (_, 429) => ConnectError::RateLimited,
        (code, status) => {
            let message: String = e.message.unwrap_or_else(|| "no details".to_string());
            let message = message.chars().take(MAX_MESSAGE).collect();
            ConnectError::Twilio { status, code, message }
        }
    }
}

/// Twilio writes `""` for "not set".
fn empty_as_none<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.filter(|s| !s.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{MockServer, Reply};
    use crate::settings::tests::{ACCOUNT, APP, BIN_IN, BIN_OUT, KEY, sample};
    use base64::Engine as _;

    const SECRET: &str = "mock-secret-0123456789abcdefWXYZ";
    // Made-up SIDs, built from pieces so secret scanners don't flag the source.
    const PN: &str = concat!("PN", "0123456789abcdef", "0123456789abcdef");
    const PN2: &str = concat!("PN", "fedcba9876543210", "fedcba9876543210");
    const AP2: &str = concat!("AP", "fedcba9876543210", "fedcba9876543210");
    const CA: &str = concat!("CA", "0123456789abcdef", "0123456789abcdef");
    const VERIFIED1: &str = concat!("PN", "0000000000000000", "0000000000000001");
    const VERIFIED2: &str = concat!("PN", "0000000000000000", "0000000000000002");
    const OTHER_ACCOUNT: &str = concat!("AC", "ffffffffffffffff", "ffffffffffffffff");
    const OTHER_KEY: &str = concat!("SK", "ffffffffffffffff", "ffffffffffffffff");

    fn client(server: &MockServer) -> TwilioClient {
        TwilioClient::with_base_url(&server.base, ACCOUNT, KEY, Secret::new(SECRET)).unwrap()
    }

    fn acct(rest: &str) -> String {
        format!("/2010-04-01/Accounts/{ACCOUNT}{rest}")
    }

    fn err(status: u16, code: u32, message: &str) -> Reply {
        Reply::json(
            status,
            format!(
                r#"{{"code":{code},"message":"{message}","more_info":"https://www.twilio.com/docs/errors/{code}","status":{status}}}"#
            ),
        )
    }

    fn apps(list: &str) -> Reply {
        Reply::ok(format!(r#"{{"applications":[{list}],"next_page_uri":null,"page":0,"page_size":100}}"#))
    }

    fn app_json(sid: &str, url: &str) -> String {
        format!(r#"{{"sid":"{sid}","friendly_name":"OpenBark","voice_url":"{url}","voice_method":"POST"}}"#)
    }

    #[test]
    fn requests_use_basic_auth_and_the_secret_goes_nowhere_else() {
        let server = MockServer::start(|_| apps(""));
        client(&server).check_key().unwrap();
        let reqs = server.requests();
        assert_eq!(reqs.len(), 1);
        let r = &reqs[0];
        assert_eq!(r.method, "GET");
        assert_eq!(r.target, format!("{}?PageSize=1", acct("/Applications.json")));
        let expected = base64::engine::general_purpose::STANDARD.encode(format!("{KEY}:{SECRET}").as_bytes());
        assert_eq!(r.header("authorization").as_deref(), Some(format!("Basic {expected}").as_str()));
        assert_eq!(r.header("accept").as_deref(), Some("application/json"));
        assert!(r.header("user-agent").unwrap().starts_with("OpenBark/"));
        assert!(!r.target.contains(SECRET) && !r.body.contains(SECRET));
    }

    #[test]
    fn a_wrong_key_says_so() {
        let server = MockServer::start(|_| err(401, 20003, "Authenticate"));
        let e = client(&server).check_key().unwrap_err();
        assert!(matches!(e, ConnectError::WrongKey), "{e:?}");
        assert!(e.to_string().starts_with("Twilio didn't accept this key."));
    }

    #[test]
    fn twilio_errors_map_to_plain_causes() {
        type Check = fn(&ConnectError) -> bool;
        let cases: Vec<(Reply, Check)> = vec![
            (err(403, 20003, "Permission denied"), |e| matches!(e, ConnectError::WrongKey)),
            (Reply::json(401, "not json".into()), |e| matches!(e, ConnectError::WrongKey)),
            (err(401, 20005, "Account not active"), |e| matches!(e, ConnectError::AccountNotActive)),
            (err(400, 21219, "'To' phone number not verified"), |e| matches!(e, ConnectError::TrialAccount)),
            (err(400, 21215, "Geo permission"), |e| matches!(e, ConnectError::GeoPermission)),
            (err(400, 21210, "'From' not verified"), |e| matches!(e, ConnectError::CallerIdNotVerified)),
            (err(404, 20404, "Not found"), |e| matches!(e, ConnectError::NotFound(w) if w == "that call")),
            (Reply::json(404, r#"{"status":404,"message":"nope"}"#.into()), |e| {
                matches!(e, ConnectError::NotFound(_))
            }),
            (err(429, 20429, "Too many requests"), |e| matches!(e, ConnectError::RateLimited)),
            (
                err(500, 20500, "Internal server error"),
                |e| matches!(e, ConnectError::Twilio { status: 500, code: Some(20500), message } if message == "Internal server error"),
            ),
        ];
        for (reply, check) in cases {
            let status = reply.status;
            let server = MockServer::start(move |_| reply.clone());
            let e = client(&server).end_call(CA).unwrap_err();
            assert!(check(&e), "HTTP {status}: {e:?}");
        }
        let long = "x".repeat(5000);
        let server = MockServer::start(move |_| err(400, 12345, &long));
        let e = client(&server).end_call(CA).unwrap_err().to_string();
        assert!(e.contains("code 12345") && e.len() < 500, "{e}");
    }

    #[test]
    fn caller_ids_merge_numbers_and_verified_ids_across_pages() {
        let page2 = acct("/IncomingPhoneNumbers.json?PageSize=100&Page=1&PageToken=PA123");
        let page2_target = page2.clone();
        let server = MockServer::start(move |r| {
            if r.target == page2_target {
                Reply::ok(format!(
                    r#"{{"incoming_phone_numbers":[{{"sid":"{PN2}","phone_number":"+15555550111","friendly_name":"Fax","voice_url":"","voice_application_sid":"","capabilities":{{"voice":false,"sms":true}}}}],"next_page_uri":null}}"#
                ))
            } else if r.target.starts_with(&acct("/IncomingPhoneNumbers.json")) {
                Reply::ok(format!(
                    r#"{{"incoming_phone_numbers":[{{"sid":"{PN}","phone_number":"+15555550100","friendly_name":"Main","voice_url":"https://demo.twilio.com/welcome/voice/","voice_application_sid":"","capabilities":{{"voice":true,"sms":true}}}}],"next_page_uri":"{page2}"}}"#
                ))
            } else if r.target.starts_with(&acct("/OutgoingCallerIds.json")) {
                Reply::ok(format!(
                    r#"{{"outgoing_caller_ids":[{{"sid":"{VERIFIED1}","phone_number":"+15555550100","friendly_name":"Dup"}},{{"sid":"{VERIFIED2}","phone_number":"+15555550142","friendly_name":"Front desk line"}}],"next_page_uri":null}}"#
                ))
            } else {
                Reply::json(404, "{}".into())
            }
        });
        let c = client(&server);
        let numbers = c.incoming_numbers().unwrap();
        assert_eq!(numbers.len(), 2, "both pages read");
        assert_eq!(numbers[0].voice_application_sid, None, "\"\" means not set");
        let ids = c.caller_ids().unwrap();
        assert_eq!(
            ids,
            vec![
                CallerIdOption {
                    phone_number: "+15555550100".into(),
                    friendly_name: "Main".into(),
                    kind: CallerIdKind::TwilioNumber
                },
                CallerIdOption {
                    phone_number: "+15555550142".into(),
                    friendly_name: "Front desk line".into(),
                    kind: CallerIdKind::Verified
                },
            ],
            "voice-capable numbers first, no repeats, the fax line left out"
        );
        assert_eq!(c.verify_caller_id("+15555550142").unwrap().kind, CallerIdKind::Verified);
        let e = c.verify_caller_id("+15555550199").unwrap_err();
        assert!(matches!(e, ConnectError::CallerIdNotInAccount(_)), "{e:?}");
        assert!(e.to_string().contains("+15555550199"));
    }

    #[test]
    fn a_next_page_outside_the_account_is_not_followed() {
        let server = MockServer::start(|_| {
            Reply::ok(format!(
                r#"{{"outgoing_caller_ids":[],"next_page_uri":"/2010-04-01/Accounts/{OTHER_ACCOUNT}/OutgoingCallerIds.json?Page=1"}}"#
            ))
        });
        let e = client(&server).verified_caller_ids().unwrap_err();
        assert!(matches!(e, ConnectError::BadResponse(_)), "{e:?}");
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn the_app_is_created_when_missing() {
        let server = MockServer::start(|r| match r.method.as_str() {
            "GET" => apps(""),
            _ => Reply::json(201, app_json(APP, BIN_OUT)),
        });
        let (app, change) = client(&server).ensure_app(None, BIN_OUT).unwrap();
        assert_eq!((app.sid.as_str(), change), (APP, AppChange::Created));
        let reqs = server.requests();
        assert_eq!(
            reqs[0].target,
            format!("{}?FriendlyName=OpenBark&PageSize=100", acct("/Applications.json"))
        );
        assert_eq!(reqs[1].method, "POST");
        assert_eq!(reqs[1].target, acct("/Applications.json"));
        assert_eq!(reqs[1].header("content-type").as_deref(), Some("application/x-www-form-urlencoded"));
        assert_eq!(
            reqs[1].body,
            concat!(
                "FriendlyName=OpenBark&VoiceUrl=https%3A%2F%2Fhandler.twilio.com%2Ftwiml%2F",
                "EH",
                "0123456789abcdef",
                "0123456789abcdef",
                "&VoiceMethod=POST"
            )
        );
    }

    #[test]
    fn an_app_already_right_is_left_alone() {
        let server = MockServer::start(|_| apps(&app_json(APP, BIN_OUT)));
        let (app, change) = client(&server).ensure_app(None, BIN_OUT).unwrap();
        assert_eq!((app.sid.as_str(), change), (APP, AppChange::Unchanged));
        assert_eq!(server.requests().len(), 1, "no write");
    }

    #[test]
    fn an_app_with_another_url_is_updated() {
        let server = MockServer::start(|r| match r.method.as_str() {
            "GET" => Reply::ok(app_json(APP, "https://handler.twilio.com/twiml/EHold")),
            _ => Reply::ok(app_json(APP, BIN_OUT)),
        });
        let (app, change) = client(&server).ensure_app(Some(APP), BIN_OUT).unwrap();
        assert_eq!((app.voice_url.as_deref(), change), (Some(BIN_OUT), AppChange::Updated));
        let reqs = server.requests();
        assert_eq!(reqs[0].target, acct(&format!("/Applications/{APP}.json")));
        assert_eq!(reqs[1].target, acct(&format!("/Applications/{APP}.json")));
        assert!(reqs[1].body.starts_with("VoiceUrl=https%3A%2F%2Fhandler.twilio.com"));
    }

    #[test]
    fn a_deleted_app_is_found_by_name_or_made_again() {
        let server = MockServer::start(|r| {
            if r.target.contains(&format!("/Applications/{APP}.json")) {
                err(404, 20404, "The requested resource was not found")
            } else {
                apps(&app_json(AP2, BIN_OUT))
            }
        });
        let (app, change) = client(&server).ensure_app(Some(APP), BIN_OUT).unwrap();
        assert_eq!((app.sid.as_str(), change), (AP2, AppChange::Unchanged));
    }

    #[test]
    fn a_numbers_voice_url_is_set_and_its_app_cleared() {
        let server = MockServer::start(|_| {
            Reply::ok(format!(
                r#"{{"sid":"{PN}","phone_number":"+15555550100","friendly_name":"Main","voice_url":"{BIN_IN}","voice_application_sid":"","capabilities":{{"voice":true}}}}"#
            ))
        });
        let n = client(&server).set_number_voice_url(PN, BIN_IN).unwrap();
        assert_eq!((n.voice_url.as_deref(), n.voice_application_sid), (Some(BIN_IN), None));
        let r = &server.requests()[0];
        assert_eq!(
            (r.method.as_str(), r.target.as_str()),
            ("POST", acct(&format!("/IncomingPhoneNumbers/{PN}.json")).as_str())
        );
        assert_eq!(
            r.body,
            concat!(
                "VoiceUrl=https%3A%2F%2Fhandler.twilio.com%2Ftwiml%2F",
                "EH",
                "fedcba9876543210",
                "fedcba9876543210",
                "&VoiceMethod=POST&VoiceApplicationSid="
            )
        );
    }

    #[test]
    fn a_call_is_ended_with_status_completed() {
        let server = MockServer::start(|_| Reply::ok(format!(r#"{{"sid":"{CA}","status":"completed"}}"#)));
        client(&server).end_call(CA).unwrap();
        let r = &server.requests()[0];
        assert_eq!(
            (r.target.as_str(), r.body.as_str()),
            (acct(&format!("/Calls/{CA}.json")).as_str(), "Status=completed")
        );
        assert!(client(&server).end_call("CA123").is_err(), "a bad SID never reaches Twilio");
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn connecting_for_calls_out_checks_everything_and_saves_the_app() {
        let server = MockServer::start(|r| {
            if r.method == "POST" {
                Reply::json(201, app_json(APP, BIN_OUT))
            } else if r.target.contains("/IncomingPhoneNumbers.json") {
                Reply::ok(r#"{"incoming_phone_numbers":[],"next_page_uri":null}"#.into())
            } else if r.target.contains("/OutgoingCallerIds.json") {
                Reply::ok(format!(
                    r#"{{"outgoing_caller_ids":[{{"sid":"{VERIFIED2}","phone_number":"+15555550100","friendly_name":"Clinic"}}],"next_page_uri":null}}"#
                ))
            } else {
                apps("")
            }
        });
        let mut settings = sample();
        let setup = client(&server).connect_outgoing(&mut settings).unwrap();
        assert_eq!(setup.app_change, AppChange::Created);
        assert_eq!(setup.caller_id.kind, CallerIdKind::Verified);
        assert_eq!(settings.twiml_app_sid.as_deref(), Some(APP));

        let mut no_caller = sample();
        no_caller.caller_id = None;
        assert!(
            client(&server).connect_outgoing(&mut no_caller).unwrap_err().to_string().contains("caller ID")
        );
        let mut other_key = sample();
        other_key.api_key_sid = OTHER_KEY.into();
        assert!(client(&server).connect_outgoing(&mut other_key).is_err());
    }

    #[test]
    fn the_key_only_goes_to_twilio() {
        let s = || Secret::new(SECRET);
        TwilioClient::new(ACCOUNT, KEY, s()).unwrap();
        TwilioClient::with_base_url("http://127.0.0.1:9", ACCOUNT, KEY, s()).unwrap();
        TwilioClient::with_base_url("http://[::1]:9", ACCOUNT, KEY, s()).unwrap();
        for base in [
            "http://api.twilio.com",
            "https://api.twilio.com.example.net",
            "https://api.twilio.com:8443",
            "https://example.com",
            "https://user:pw@api.twilio.com",
            "ftp://127.0.0.1",
            "not a url",
        ] {
            let e = TwilioClient::with_base_url(base, ACCOUNT, KEY, s()).unwrap_err();
            assert!(e.to_string().contains("only sends the Twilio key"), "{base}: {e}");
        }
        assert!(TwilioClient::new("AC1", KEY, s()).is_err());
    }

    #[test]
    fn redirects_are_not_followed() {
        let server = MockServer::start(|_| Reply::redirect("http://127.0.0.1:9/elsewhere"));
        let e = client(&server).check_key().unwrap_err();
        assert!(matches!(e, ConnectError::Twilio { status: 302, .. }), "{e:?}");
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn no_network_says_so() {
        // A loopback port that was free a moment ago: nothing listens there.
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let c = TwilioClient::with_base_url(&format!("http://{free}"), ACCOUNT, KEY, Secret::new(SECRET))
            .unwrap();
        let e = c.check_key().unwrap_err();
        assert!(matches!(e, ConnectError::Network(_)), "{e:?}");
        assert!(e.to_string().starts_with("Can't reach Twilio."));
    }

    #[test]
    fn form_bodies_are_url_encoded() {
        assert_eq!(form_body(&[("A", "b c&d=e"), ("Empty", "")]), "A=b+c%26d%3De&Empty=");
    }
}
