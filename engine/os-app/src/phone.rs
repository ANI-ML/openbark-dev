//! Calls through the clinic's own Twilio account (ADR-0023, docs/plans/telephony.md).
//!
//! Outgoing calls only, in this first release (ADR-0023 decision 2). The clinic
//! connects its own account in Settings; the engine keeps the key secret in
//! the keychain ([`os_connect::ConnectStore`]) and mints one-hour access
//! tokens for the phone window, which runs Twilio's Voice SDK. When the client
//! answers, the phone window streams both sides of the call here, and the call
//! is recorded on this computer as a two-channel visit
//! ([`os_engine::live::LiveSource::Call`]), announced first (ADR-0023 decision 1):
//! the caller reads the announcement, then says whether the client agreed.
//! "No" deletes what was recorded and the call goes on unrecorded.
//!
//! Who may do what (ADR-0023 section 10): only a veterinarian connects,
//! disconnects or changes the phone settings. Anyone can place a call. On a
//! shared OS account that's a convenience, not a security boundary.

use std::sync::Arc;
use std::time::Instant;

use chrono::Local;
use os_connect::settings::{check_api_key_secret, normalise_phone};
use os_connect::twilio::{AppChange, CallerIdOption};
use os_connect::{
    ConnectError, PhonePrefs, REMOVE_NOTE, Removed, Secret, SecretId, SecretStatus, TwilioClient,
    TwilioSettings,
};
use os_engine::live::{CallBlock, LiveOptions, LiveRecorder, LiveSource};
use os_engine::session::{CallDirection, CallInfo, ConsentOutcome, Session};
use os_engine::users::{Role, Who};
use serde::{Deserialize, Serialize};

use crate::{
    App, AppError, Consent, ConsentMethod, ConsentRule, Events, EventsSink, Holder, Need, Result, SessionView,
};

/// The phone settings as Settings shows them. Never the secret.
#[derive(Debug, Clone, Serialize)]
pub struct PhoneView {
    /// Twilio is connected on this computer (settings and secret saved).
    pub connected: bool,
    pub twilio: Option<TwilioView>,
    /// Where the secret is kept and its last four characters.
    pub secret: Option<SecretStatus>,
    pub prefs: PhonePrefs,
    /// Calls can be placed now: connected, set up, and the phone is on.
    pub ready: bool,
    /// The Linux desktop app can't make calls (no WebRTC in WebKitGTK); web
    /// mode can. The UI decides with this and its own transport.
    pub linux: bool,
    /// The outgoing Bin's text for the saved caller ID, for **Show the Twilio text**.
    pub outgoing_bin: Option<String>,
}

/// The saved Twilio settings, for display.
#[derive(Debug, Clone, Serialize)]
pub struct TwilioView {
    pub account_sid: String,
    /// "SK…4f2a".
    pub api_key_hint: String,
    pub api_key_sid: String,
    pub computer_name: String,
    pub identity: String,
    pub caller_id: Option<String>,
    pub outgoing_bin_url: Option<String>,
    pub twiml_app_sid: Option<String>,
}

/// What **Connect** sends. No `Debug`: it carries the secret.
#[derive(Deserialize)]
pub struct ConnectRequest {
    pub account_sid: String,
    pub api_key_sid: String,
    /// Empty keeps the secret already saved for this same key (changing the
    /// caller ID or the Bin URL without pasting the secret again).
    #[serde(default)]
    pub api_key_secret: String,
    pub computer_name: String,
    pub caller_id: String,
    pub outgoing_bin_url: String,
}

/// What **Connect** found and did.
#[derive(Debug, Clone, Serialize)]
pub struct Connected {
    pub phone: PhoneView,
    pub app_change: AppChange,
    pub caller_id: CallerIdOption,
}

/// What **Disconnect** removed, and the reminder to delete the key at Twilio.
#[derive(Debug, Clone, Serialize)]
pub struct Disconnected {
    pub removed: Removed,
    pub note: &'static str,
}

/// One line of **Check setup**.
#[derive(Debug, Clone, Serialize)]
pub struct SetupCheck {
    /// "key", "caller_id", "twiml_app", "phone_on", "trial".
    pub id: &'static str,
    /// `Some(true)` passed, `Some(false)` failed, `None` for a note.
    pub ok: Option<bool>,
    pub message: String,
}

/// A token for the phone window's `Device`. Lives one hour.
#[derive(Serialize)]
pub struct PhoneToken {
    pub token: String,
    pub identity: String,
    pub expires_at: u64,
}

/// What the main window asks the phone window to call.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CallRequest {
    /// The patient whose owner is being called, if any.
    #[serde(default)]
    pub patient_id: Option<String>,
    /// A number typed on the dial pad, if any; otherwise the owner's number.
    #[serde(default)]
    pub number: Option<String>,
}

/// The call the phone window shows: who, which number, and how it's recorded.
#[derive(Debug, Clone, Serialize)]
pub struct CallTarget {
    pub patient_id: Option<String>,
    /// "Bella", the animal.
    pub patient_name: String,
    /// "Patel", the family.
    pub last_name: String,
    /// E.164, or `None` when there's none on file (the dial pad is shown).
    pub number: Option<String>,
    pub record_calls: bool,
    pub announcement: String,
    /// The caller, as the visit will record them.
    pub by: Option<String>,
    /// The clinic's side on the recording: "Vet", "Tech" or "Clinic".
    pub clinic_label: String,
}

/// The call being recorded now.
pub(crate) struct CallState {
    id: String,
    /// When the client answered (not when the engine was ready).
    started: Instant,
    outcome: Option<ConsentOutcome>,
    /// Set only when the caller confirms the announcement was read ("I've
    /// read it"). Without it the recording isn't kept (ADR-0023 decision 1).
    announced_at: Option<f32>,
    who: Option<Who>,
    /// Set once the recording is deleted (declined, or never announced): the
    /// call may go on, unrecorded.
    discarded: Option<ConsentOutcome>,
}

#[cfg(test)]
impl CallState {
    /// A call being recorded into the visit `id`, for tests that need no
    /// speech models.
    pub(crate) fn for_test(id: &str, announced: bool) -> Self {
        Self {
            id: id.to_string(),
            started: Instant::now(),
            outcome: None,
            announced_at: announced.then_some(0.0),
            who: None,
            discarded: None,
        }
    }
}

/// The label a call's visit folder is made from when no patient is chosen.
/// Never the number: folder names and visit ids show in logs, replies and
/// events, and the number is only kept encrypted in `session.json`.
pub(crate) const CALL_LABEL: &str = "call";

/// The sample rates the phone window may stream at.
pub const CALL_SAMPLE_RATES: std::ops::RangeInclusive<u32> = 8_000..=192_000;

/// Refuses a sample rate no browser produces (and that would stop the recorder).
pub fn check_call_sample_rate(rate: u32) -> Result<()> {
    if CALL_SAMPLE_RATES.contains(&rate) {
        Ok(())
    } else {
        Err(AppError(format!(
            "The call's audio came at {rate} Hz, which OpenBark can't record. Try the call again."
        )))
    }
}

/// The end-to-end demo only (the `mock-twilio` feature, never in builds
/// clinics get): calls to the number in `OPENBARK_TEST_FAIL_CALLS_TO` get a
/// block the recorder can't use two seconds in, so its worker fails as it
/// would on a real fault, to test that a decline still deletes what it wrote.
#[cfg(feature = "mock-twilio")]
mod fault {
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    use os_engine::live::CallBlock;

    pub fn maybe_fail(number: &str, blocks: Receiver<CallBlock>) -> Receiver<CallBlock> {
        if std::env::var("OPENBARK_TEST_FAIL_CALLS_TO").ok().as_deref() != Some(number) {
            return blocks;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let start = Instant::now();
            let mut failed = false;
            for block in blocks {
                if !failed && start.elapsed() > Duration::from_secs(2) {
                    failed = true;
                    let bad = CallBlock { clinic: vec![0.0; 160], client: vec![0.0; 160], sample_rate: 0 };
                    if tx.send(bad).is_err() {
                        break;
                    }
                }
                if tx.send(block).is_err() {
                    break;
                }
            }
        });
        rx
    }
}

/// The longest gap accepted between the answer and the engine starting.
const MAX_SINCE_ANSWER: f32 = 600.0;

/// When the call ends: how long it lasted and Twilio's call SID.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CallEnd {
    #[serde(default)]
    pub duration_secs: Option<f32>,
    #[serde(default)]
    pub call_sid: Option<String>,
}

/// Starting a recorded call, when the client answers.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CallStart {
    pub number: String,
    #[serde(default)]
    pub patient_id: Option<String>,
    #[serde(default)]
    pub call_sid: Option<String>,
    /// The phone window's sample rate, checked before recording starts.
    #[serde(default)]
    pub sample_rate: Option<u32>,
    /// Seconds since the client answered: the window keeps the audio from
    /// the answer while the engine gets ready, so times count from there.
    #[serde(default)]
    pub since_answer_secs: Option<f32>,
}

/// The pending call target, set by the main window.
pub(crate) struct Pending {
    target: CallTarget,
    by: Option<String>,
}

const NOT_CONNECTED: &str =
    "Twilio isn't connected on this computer. A veterinarian can connect it in Settings > Phone calls.";
const PHONE_OFF: &str = "The phone is off on this computer. Turn it on in Settings > Phone calls.";
const NO_CALL: &str = "No call is being recorded.";

/// Twilio's call SIDs are `CA` and 32 hex digits; anything else isn't kept.
fn valid_call_sid(sid: Option<String>) -> Option<String> {
    sid.map(|s| s.trim().to_string()).filter(|s| os_connect::settings::check_sid(s, "CA", "call SID").is_ok())
}

/// The clinic's side of the recording, from the caller's role.
fn clinic_label(who: Option<&Who>) -> String {
    match who.and_then(|w| w.role) {
        Some(Role::Dvm) => "Vet",
        Some(Role::Rvt) => "Tech",
        _ => "Clinic",
    }
    .to_string()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl App {
    /// A REST client for the clinic's account: `api.twilio.com`, or the
    /// loopback mock tests set in `twilio_base` (or `OPENBARK_TWILIO_API_BASE`).
    fn twilio_client(&self, account: &str, key: &str, secret: Secret) -> Result<TwilioClient> {
        Ok(match &self.twilio_base {
            Some(base) => TwilioClient::with_base_url(base, account, key, secret)?,
            None => TwilioClient::for_account(account, key, secret)?,
        })
    }

    /// The phone settings for Settings and the Call buttons.
    pub fn phone(&self) -> Result<PhoneView> {
        let twilio = self.connect.load_twilio()?;
        let secret = self.connect.secret_status(SecretId::TwilioApiKeySecret)?;
        let prefs = self.connect.load_phone()?;
        let connected = twilio.is_some() && secret.is_some();
        let set_up = twilio.as_ref().is_some_and(|t| t.twiml_app_sid.is_some() && t.caller_id.is_some());
        let outgoing_bin = twilio
            .as_ref()
            .and_then(|t| t.caller_id.as_deref())
            .and_then(|c| os_connect::twiml::outgoing_bin(c).ok());
        Ok(PhoneView {
            connected,
            ready: connected && set_up && prefs.enabled,
            twilio: twilio.map(|t| TwilioView {
                api_key_hint: t.api_key_hint(),
                identity: t.identity(),
                account_sid: t.account_sid,
                api_key_sid: t.api_key_sid,
                computer_name: t.computer_name,
                caller_id: t.caller_id,
                outgoing_bin_url: t.outgoing_bin_url,
                twiml_app_sid: t.twiml_app_sid,
            }),
            secret,
            prefs,
            linux: cfg!(target_os = "linux"),
            outgoing_bin,
        })
    }

    /// The outgoing Bin's text for a caller ID typed in the Connect sheet,
    /// before anything is saved. Makes no network call.
    pub fn phone_bin_text(&self, caller_id: &str) -> Result<String> {
        Ok(os_connect::twiml::outgoing_bin(&normalise_phone(caller_id))?)
    }

    /// **Connect**: checks the key with Twilio, checks the caller ID, makes
    /// or updates the "OpenBark" TwiML App (docs/network.md R9), and only then
    /// saves the secret (keychain) and the settings. Veterinarians only.
    pub fn phone_connect(&self, req: ConnectRequest, by: Option<&str>) -> Result<Connected> {
        self.who(by, Need::Phone)?;
        let saved = self.connect.load_twilio()?;
        let mut settings = TwilioSettings {
            account_sid: req.account_sid,
            api_key_sid: req.api_key_sid,
            computer_name: req.computer_name,
            caller_id: Some(req.caller_id),
            outgoing_bin_url: Some(req.outgoing_bin_url),
            incoming_bin_url: None,
            twiml_app_sid: None,
        }
        .normalised();
        // Keep the TwiML App this account already has.
        if let Some(old) = &saved
            && old.account_sid == settings.account_sid
        {
            settings.twiml_app_sid = old.twiml_app_sid.clone();
        }
        settings.validate()?;
        let secret = if req.api_key_secret.trim().is_empty() {
            let same_key = saved.as_ref().is_some_and(|s| s.api_key_sid == settings.api_key_sid);
            match self.connect.load_secret(SecretId::TwilioApiKeySecret)? {
                Some((secret, _)) if same_key => secret,
                _ => return Err(AppError("Paste the API key secret.".into())),
            }
        } else {
            let secret = Secret::new(req.api_key_secret);
            check_api_key_secret(&secret)?;
            secret
        };
        let client = self.twilio_client(&settings.account_sid, &settings.api_key_sid, secret.clone())?;
        let setup = client.connect_outgoing(&mut settings)?;
        self.connect.save_secret(SecretId::TwilioApiKeySecret, &secret)?;
        self.connect.save_twilio(settings)?;
        tracing::info!(change = ?setup.app_change, "Twilio connected");
        Ok(Connected { phone: self.phone()?, app_change: setup.app_change, caller_id: setup.caller_id })
    }

    /// **Disconnect**: removes the key from this computer and turns the
    /// phone off. Veterinarians only.
    pub fn phone_disconnect(&self, by: Option<&str>) -> Result<Disconnected> {
        self.who(by, Need::Phone)?;
        if self.call.lock().map_err(|_| AppError("call lock poisoned".into()))?.is_some() {
            return Err(AppError("A call is being recorded. Hang up first.".into()));
        }
        let removed = self.connect.remove_twilio()?;
        Ok(Disconnected { removed, note: REMOVE_NOTE })
    }

    /// Phone on or off, record calls, the announcement. Veterinarians only.
    pub fn phone_set_prefs(&self, prefs: PhonePrefs, by: Option<&str>) -> Result<PhoneView> {
        self.who(by, Need::Phone)?;
        if prefs.enabled && self.connect.twilio_credentials()?.is_none() {
            return Err(AppError("Connect Twilio before turning the phone on.".into()));
        }
        self.connect.save_phone(prefs)?;
        self.phone()
    }

    /// **Check setup**: every check, each saying what's wrong and where to
    /// fix it (docs/network.md R9). Veterinarians only.
    pub fn phone_check(&self, by: Option<&str>) -> Result<Vec<SetupCheck>> {
        self.who(by, Need::Phone)?;
        let creds = self.connect.twilio_credentials()?.ok_or_else(|| AppError(NOT_CONNECTED.into()))?;
        let mut out = Vec::new();
        let line = |id, r: std::result::Result<String, ConnectError>| match r {
            Ok(message) => SetupCheck { id, ok: Some(true), message },
            Err(e) => SetupCheck { id, ok: Some(false), message: e.to_string() },
        };
        let client = self.twilio_client(
            &creds.settings.account_sid,
            &creds.settings.api_key_sid,
            creds.secret.clone(),
        )?;
        let key = client.check_key().map(|()| "Twilio accepts this computer's key.".to_string());
        let key_ok = key.is_ok();
        out.push(line("key", key));
        if key_ok {
            let s = &creds.settings;
            let caller = match &s.caller_id {
                Some(c) => client.verify_caller_id(c).map(|o| format!("Calls show {}.", o.phone_number)),
                None => Err(ConnectError::Invalid("Choose the number calls will come from.".into())),
            };
            out.push(line("caller_id", caller));
            let app = match (&s.twiml_app_sid, &s.outgoing_bin_url) {
                (Some(sid), Some(bin)) => match client.fetch_app(sid) {
                    Ok(Some(app)) if app.voice_url.as_deref() == Some(bin.as_str()) => {
                        Ok("The OpenBark TwiML App uses your outgoing Bin.".to_string())
                    }
                    Ok(Some(_)) => Err(ConnectError::Invalid(
                        "The OpenBark TwiML App no longer uses your outgoing Bin. Choose Connect again to fix it."
                            .into(),
                    )),
                    Ok(None) => Err(ConnectError::Invalid(
                        "The OpenBark TwiML App was deleted in your Twilio console. Choose Connect again to make it."
                            .into(),
                    )),
                    Err(e) => Err(e),
                },
                _ => Err(ConnectError::Invalid("Twilio isn't set up yet. Choose Connect.".into())),
            };
            out.push(line("twiml_app", app));
        }
        let prefs = self.connect.load_phone()?;
        out.push(SetupCheck {
            id: "phone_on",
            ok: Some(prefs.enabled),
            message: if prefs.enabled {
                "The phone is on on this computer.".into()
            } else {
                "The phone is off on this computer. Turn it on to make calls.".into()
            },
        });
        // A Standard key can't read the account's type, so this is a note.
        out.push(SetupCheck {
            id: "trial",
            ok: None,
            message:
                "Real calls need an upgraded Twilio account. Trials can't connect calls through OpenBark."
                    .into(),
        });
        Ok(out)
    }

    /// A one-hour access token for the phone window (ADR-0023 section 10).
    /// The secret stays here; only the token leaves the engine.
    pub fn phone_token(&self) -> Result<PhoneToken> {
        let creds = self.connect.twilio_credentials()?.ok_or_else(|| AppError(NOT_CONNECTED.into()))?;
        if !self.connect.load_phone()?.enabled {
            return Err(AppError(PHONE_OFF.into()));
        }
        let token = creds.voice_token(now_secs())?;
        Ok(PhoneToken {
            token: token.jwt().to_string(),
            identity: token.identity.clone(),
            expires_at: token.expires_at,
        })
    }

    /// The main window chooses who to call, then opens the phone window,
    /// which reads it with [`App::phone_pending`]. Anyone can call.
    pub fn phone_prepare(&self, req: CallRequest, by: Option<&str>) -> Result<CallTarget> {
        let who = self.who(by, Need::Any)?;
        let prefs = self.connect.load_phone()?;
        if self.connect.twilio_credentials()?.is_none() {
            return Err(AppError(NOT_CONNECTED.into()));
        }
        if !prefs.enabled {
            return Err(AppError(PHONE_OFF.into()));
        }
        let record = match req.patient_id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => Some(
                self.patients
                    .get(id)
                    .ok_or_else(|| AppError("That patient isn't in this computer's records.".into()))?,
            ),
            None => None,
        };
        let typed = req.number.as_deref().map(str::trim).filter(|n| !n.is_empty());
        let number = match typed {
            Some(n) => Some(os_engine::patients::normalise_phone(n)?),
            None => record.as_ref().and_then(|r| r.owner_phone.clone()),
        };
        let target = CallTarget {
            patient_id: record.as_ref().map(|r| r.id.clone()),
            patient_name: record.as_ref().map(|r| r.name.clone()).unwrap_or_default(),
            last_name: record.as_ref().map(|r| r.last_name.clone()).unwrap_or_default(),
            number,
            record_calls: prefs.record_calls,
            announcement: prefs.announcement,
            by: who.as_ref().map(|w| w.name.clone()),
            clinic_label: clinic_label(who.as_ref()),
        };
        *self.pending_call.lock().map_err(|_| AppError("call lock poisoned".into()))? =
            Some(Pending { target: target.clone(), by: by.map(String::from) });
        Ok(target)
    }

    /// What the phone window should call, as the main window chose it.
    pub fn phone_pending(&self) -> Result<Option<CallTarget>> {
        let pending = self.pending_call.lock().map_err(|_| AppError("call lock poisoned".into()))?;
        let Some(p) = pending.as_ref() else { return Ok(None) };
        // The preferences may have changed since; show the current ones.
        let prefs = self.connect.load_phone()?;
        Ok(Some(CallTarget {
            record_calls: prefs.record_calls,
            announcement: prefs.announcement,
            ..p.target.clone()
        }))
    }

    /// The client answered and the call is recorded: start a two-channel
    /// recording fed by `blocks` (ADR-0023 section 7). The caller is the one
    /// the main window prepared the call for.
    pub fn start_call(
        &self,
        req: CallStart,
        blocks: std::sync::mpsc::Receiver<CallBlock>,
        events: Arc<dyn Events>,
    ) -> Result<String> {
        let prefs = self.connect.load_phone()?;
        if self.connect.twilio_credentials()?.is_none() {
            return Err(AppError(NOT_CONNECTED.into()));
        }
        if !prefs.enabled {
            return Err(AppError(PHONE_OFF.into()));
        }
        if !prefs.record_calls {
            return Err(AppError("Recording calls is off in Settings > Phone calls.".into()));
        }
        let number = os_engine::patients::normalise_phone(&req.number)?;
        if let Some(rate) = req.sample_rate {
            check_call_sample_rate(rate)?;
        }
        let since_answer = req
            .since_answer_secs
            .filter(|s| s.is_finite() && *s > 0.0)
            .map(|s| s.min(MAX_SINCE_ANSWER))
            .unwrap_or(0.0);
        let answered = Instant::now()
            .checked_sub(std::time::Duration::from_secs_f32(since_answer))
            .unwrap_or_else(Instant::now);
        let answered_at = Local::now() - chrono::Duration::milliseconds((since_answer * 1000.0) as i64);
        let by = self
            .pending_call
            .lock()
            .map_err(|_| AppError("call lock poisoned".into()))?
            .as_ref()
            .and_then(|p| p.by.clone());
        let who = self.who(by.as_deref(), Need::Any)?;
        let mut call_slot = self.call.lock().map_err(|_| AppError("call lock poisoned".into()))?;
        if call_slot.is_some() {
            return Err(AppError("A call is already being recorded.".into()));
        }
        let patient = match req.patient_id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => self.patients.get(id).map(|r| os_engine::session::PatientInfo {
                name: r.name,
                last_name: r.last_name,
                species: r.species,
                patient_id: Some(r.id),
                ..Default::default()
            }),
            None => None,
        };
        // The caller reads the announcement (ADR-0023 decision 1): verbal, under
        // either rule. The client's answer is added when it's known.
        let consent = Consent {
            rule: ConsentRule::AllParties,
            method: ConsentMethod::Verbal,
            confirmed_by: who
                .as_ref()
                .map(|w| w.name.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "Clinic".into()),
            confirmed_at: None,
            script: Some(prefs.announcement.clone()),
            outcome: None,
            // Set when the caller confirms the announcement was read.
            announced_at_secs: None,
        };
        consent.check()?;
        let call = CallInfo {
            direction: CallDirection::Outgoing,
            number: number.clone(),
            started_at: answered_at,
            duration_secs: 0.0,
            call_sid: valid_call_sid(req.call_sid),
            clinic_label: clinic_label(who.as_ref()),
            declined_at_secs: None,
        };
        let work = self.begin_work()?;
        let mut slot = self.recorder.lock().map_err(|_| AppError("recorder lock poisoned".into()))?;
        if slot.is_some() {
            return Err(AppError("A recording is already running. Stop it first.".into()));
        }
        let mut options = LiveOptions::microphone();
        options.label_speakers = true;
        options.patient = patient.filter(|p| !p.is_empty());
        options.consent = Some(consent);
        options.recorded_by = who.clone();
        options.call = Some(call);
        let label = CALL_LABEL.to_string();
        #[cfg(feature = "mock-twilio")]
        let blocks = fault::maybe_fail(&number, blocks);
        let recorder = LiveRecorder::start(
            &self.store,
            &self.sessions_root,
            LiveSource::Call { label, blocks },
            options,
            Box::new(EventsSink(events)),
        )?;
        let id = crate::folder_name(&recorder.session_dir);
        *slot = Some(recorder);
        *self.recording_work.lock().map_err(|_| AppError("recorder lock poisoned".into()))? = Some(work);
        *self.recording_lock.lock().map_err(|_| AppError("recorder lock poisoned".into()))? =
            self.lock_visit(&id, Holder::Recording).ok();
        *call_slot = Some(CallState {
            id: id.clone(),
            started: answered,
            outcome: None,
            announced_at: None,
            who,
            discarded: None,
        });
        Ok(id)
    }

    /// "I've read it": the caller read the announcement to the client. Until
    /// this arrives the recording isn't kept (ADR-0023 decision 1). `at` is when
    /// the announcement started, in seconds into the recording (0: it is
    /// shown when the client answers).
    pub fn call_announced(&self, at: Option<f32>) -> Result<()> {
        let mut call = self.call.lock().map_err(|_| AppError("call lock poisoned".into()))?;
        let state = call.as_mut().ok_or_else(|| AppError(NO_CALL.into()))?;
        if state.discarded.is_some() {
            return Err(AppError("The recording of this call was already deleted.".into()));
        }
        let at = at.filter(|a| a.is_finite()).unwrap_or(0.0).clamp(0.0, 3600.0);
        state.announced_at.get_or_insert(at);
        Ok(())
    }

    /// **Did the client agree?** Yes keeps recording; no deletes what was
    /// recorded and the call goes on unrecorded (returns the visit, which now
    /// says the client declined).
    pub fn call_consent(
        &self,
        outcome: ConsentOutcome,
        announced_at: Option<f32>,
    ) -> Result<Option<SessionView>> {
        if outcome == ConsentOutcome::Declined {
            return self.decline_call().map(Some);
        }
        if outcome == ConsentOutcome::NotAnnounced {
            return Err(AppError("That isn't one of the client's answers.".into()));
        }
        if announced_at.is_some() {
            self.call_announced(announced_at)?;
        }
        let mut call = self.call.lock().map_err(|_| AppError("call lock poisoned".into()))?;
        let state = call.as_mut().ok_or_else(|| AppError(NO_CALL.into()))?;
        if state.discarded.is_some() {
            return Err(AppError("The recording of this call was already deleted.".into()));
        }
        if state.announced_at.is_none() {
            return Err(AppError("Read the announcement to the client first.".into()));
        }
        state.outcome = Some(outcome);
        Ok(None)
    }

    /// **Stop and delete the recording** (or "No, stop recording"): the
    /// recording so far is deleted, and the visit records that the client
    /// declined. The call itself goes on.
    pub fn decline_call(&self) -> Result<SessionView> {
        let (id, who, elapsed) = {
            let mut call = self.call.lock().map_err(|_| AppError("call lock poisoned".into()))?;
            let state = call.as_mut().ok_or_else(|| AppError(NO_CALL.into()))?;
            if state.discarded.is_some() {
                return Err(AppError("The recording of this call was already deleted.".into()));
            }
            state.discarded = Some(ConsentOutcome::Declined);
            state.outcome = Some(ConsentOutcome::Declined);
            (state.id.clone(), state.who.clone(), state.started.elapsed().as_secs_f32())
        };
        self.discard_recording(&id, who, elapsed, ConsentOutcome::Declined)
    }

    /// Deletes a call's recording, whatever state the recorder is in: stopped
    /// cleanly, failed, or already gone. The visit stays, saying why the
    /// recording was deleted and when.
    fn discard_recording(
        &self,
        id: &str,
        who: Option<Who>,
        elapsed: f32,
        outcome: ConsentOutcome,
    ) -> Result<SessionView> {
        let recorder = self.recorder.lock().map_err(|_| AppError("recorder lock poisoned".into()))?.take();
        let _work = self.recording_work.lock().map_err(|_| AppError("recorder lock poisoned".into()))?.take();
        let _lock = self.recording_lock.lock().map_err(|_| AppError("recorder lock poisoned".into()))?.take();
        let dir = match recorder {
            Some(recorder) => {
                let dir = recorder.session_dir.clone();
                // A recorder that failed still had its folder: delete what it wrote.
                if let Err(e) = recorder.discard() {
                    tracing::warn!(visit = %id, error = %e, "the call's recorder had failed; deleting what it wrote");
                }
                dir
            }
            None => self.session_dir(id)?,
        };
        let mut s = Session::open(&dir)?;
        s.forget_recording(&who.unwrap_or_else(|| Who::named("Clinic")))?;
        s.set_consent_outcome(outcome, None)?;
        if let Some(mut call) = s.meta.call.clone() {
            call.declined_at_secs = Some(elapsed);
            call.duration_secs = call.duration_secs.max(elapsed);
            s.set_call(call)?;
        }
        self.link_visit(&mut s)?;
        tracing::info!(visit = %id, ?outcome, "call recording deleted");
        Ok(crate::view(&s, None))
    }

    /// The phone window closed or crashed (desktop): the call is over. An
    /// announced recording is saved with what it has; one never announced is
    /// deleted, as at any hang-up.
    pub fn phone_window_closed(&self) -> Result<Option<SessionView>> {
        self.end_call(CallEnd::default())
    }

    /// Hang up: a recorded call is saved (transcript complete) and returned,
    /// ready to draft with the Phone call template. A declined call's visit
    /// gets its final length. A call that was never recorded saves nothing
    /// and returns `None`.
    pub fn end_call(&self, end: CallEnd) -> Result<Option<SessionView>> {
        let Some(state) = self.call.lock().map_err(|_| AppError("call lock poisoned".into()))?.take() else {
            return Ok(None);
        };
        let duration = end
            .duration_secs
            .filter(|d| d.is_finite() && *d >= 0.0)
            .unwrap_or_else(|| state.started.elapsed().as_secs_f32());
        let sid = valid_call_sid(end.call_sid);
        let finish = |s: &mut Session| -> Result<()> {
            if let Some(mut call) = s.meta.call.clone() {
                call.duration_secs = duration.max(call.declined_at_secs.unwrap_or(0.0));
                call.call_sid = sid.clone().or(call.call_sid);
                s.set_call(call)?;
            }
            Ok(())
        };
        if state.discarded.is_some() {
            let mut s = Session::open(&self.session_dir(&state.id)?)?;
            finish(&mut s)?;
            return Ok(Some(crate::view(&s, None)));
        }
        // Never announced ("I've read it" wasn't confirmed): the recording
        // isn't kept (ADR-0023 decision 1), whether the caller hung up, the
        // client did, or the window closed.
        if state.announced_at.is_none() {
            let elapsed = state.started.elapsed().as_secs_f32();
            self.discard_recording(&state.id, state.who.clone(), elapsed, ConsentOutcome::NotAnnounced)?;
            let mut s = Session::open(&self.session_dir(&state.id)?)?;
            finish(&mut s)?;
            return Ok(Some(crate::view(&s, None)));
        }
        let recorder = self.recorder.lock().map_err(|_| AppError("recorder lock poisoned".into()))?.take();
        let _work = self.recording_work.lock().map_err(|_| AppError("recorder lock poisoned".into()))?.take();
        let _lock = self.recording_lock.lock().map_err(|_| AppError("recorder lock poisoned".into()))?.take();
        // A recorder that failed keeps what it wrote; the visit still gets the
        // call's details and the client's answer.
        let transcript = match recorder.map(|r| r.stop()) {
            Some(Ok(result)) => Some(result.transcript),
            Some(Err(e)) => {
                tracing::warn!(visit = %state.id, error = %e, "the call's recorder failed");
                None
            }
            None => None,
        };
        let mut s = Session::open(&self.session_dir(&state.id)?)?;
        // An announced call with no answer to "Did the client agree?": the
        // client stayed on the line after hearing it.
        s.set_consent_outcome(state.outcome.unwrap_or(ConsentOutcome::StayedOnLine), state.announced_at)?;
        finish(&mut s)?;
        self.link_visit(&mut s)?;
        Ok(Some(crate::view(&s, transcript)))
    }

    /// Set or clear a patient's owner phone number (ADR-0023 section 8).
    pub fn set_owner_phone(&self, id: &str, phone: &str) -> Result<os_engine::patients::PatientRecord> {
        let _work = self.begin_work()?;
        Ok(self.patients.set_owner_phone(id, phone)?)
    }

    /// A call is being recorded (or was, and the client declined) and not
    /// yet hung up.
    pub fn call_active(&self) -> bool {
        self.call.lock().map(|c| c.is_some()).unwrap_or(false)
    }
}
