//! Settings > Sync (ADR-0017, M3 phase 2): the app's side of encrypted sync.
//!
//! Off by default. The desktop app and web mode call these through Tauri
//! commands and the HTTP API; the WebView never talks to the network and
//! never sees a key. Answers are JSON (the os-sync types, serialised), so
//! the transports stay thin and phone builds without `sync` still compile.
//!
//! With a team set up, the member who turns sync on is linked: only visits
//! they recorded, drafted or signed sync (ADR-0017 section 3), and only they
//! or a veterinarian who manages the team can remove devices, turn it off or
//! delete the account.

use std::sync::Arc;

use serde_json::{Value, json};

#[cfg(feature = "sync")]
use crate::Need;
use crate::{App, AppError, Result};

/// Encrypted sync (and the clinic hub, ADR-0029) is a preview until the
/// independent security review is done: only builds made with
/// `OPENBARK_SYNC_PREVIEW` set at build time offer it (development,
/// TestFlight, the end-to-end tests' and, since 0.2.2, the release
/// installers). Nothing at run time can switch it on in a build without it.
#[cfg(feature = "sync")]
pub(crate) fn preview_enabled() -> bool {
    option_env!("OPENBARK_SYNC_PREVIEW").is_some()
}

/// The sync service for a visits folder, or why there is none.
#[cfg(feature = "sync")]
pub(crate) fn open_service(root: &std::path::Path) -> Option<Arc<os_sync::SyncService>> {
    if !preview_enabled() {
        return None;
    }
    match os_sync::SyncService::for_root(root) {
        Ok(s) => Some(Arc::new(s)),
        Err(e) => {
            tracing::warn!(error = %e, "encrypted sync is unavailable");
            None
        }
    }
}

#[cfg(feature = "sync")]
fn err(e: os_sync::SyncError) -> AppError {
    AppError(e.to_string())
}

/// How often desktops poll while open (ADR-0017 section 11): 30 s, or
/// `OPENBARK_SYNC_INTERVAL_SECS` (tests).
fn interval() -> std::time::Duration {
    let secs =
        std::env::var("OPENBARK_SYNC_INTERVAL_SECS").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(30);
    std::time::Duration::from_secs(secs.clamp(2, 3600))
}

#[cfg(feature = "sync")]
struct AppHost<'a>(&'a App);

#[cfg(feature = "sync")]
impl os_sync::Host for AppHost<'_> {
    fn lock_visit(&self, folder: &str) -> Option<Box<dyn std::any::Any>> {
        self.0.lock_visit(folder, crate::Holder::Saving).ok().map(|l| Box::new(l) as Box<dyn std::any::Any>)
    }
}

impl App {
    #[cfg(feature = "sync")]
    fn sync_service(&self) -> Result<&Arc<os_sync::SyncService>> {
        self.sync
            .as_ref()
            .ok_or_else(|| AppError("Encrypted sync couldn't start on this computer. Check the log.".into()))
    }

    /// Who may change sync: anyone single-user; with a team, the linked
    /// member, or a veterinarian who manages the team, or anyone while sync
    /// isn't linked yet.
    #[cfg(feature = "sync")]
    fn sync_actor(&self, by: Option<&str>, change: bool) -> Result<Option<String>> {
        let who = self.who(by, Need::Any)?;
        let Some(who) = who else { return Ok(None) };
        let id = who.id.clone();
        if !self.users.is_team() || !change {
            return Ok(id);
        }
        let linked = self.sync_service()?.status().linked_user;
        let manager = id.as_deref().and_then(|i| self.users.get(i)).is_some_and(|u| u.role.can_manage_team());
        if linked.is_none() || linked == id || manager {
            Ok(id)
        } else {
            Err(AppError(
                "Sync was turned on by another team member. Ask them, or a veterinarian, to change it."
                    .into(),
            ))
        }
    }

    /// Settings > Sync.
    pub fn sync_status(&self) -> Value {
        #[cfg(feature = "sync")]
        if let Some(s) = &self.sync {
            let mut v = serde_json::to_value(s.status()).unwrap_or(Value::Null);
            v["available"] = json!(true);
            v["kit"] = serde_json::to_value(s.kit()).unwrap_or(Value::Null);
            return v;
        }
        json!({"available": false, "phase": "off"})
    }

    /// Before signing in: the hosted service (`url` empty) or a clinic's own.
    pub fn sync_set_server(&self, url: &str, key: &str, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, true)?;
            let server = (!url.trim().is_empty()).then(|| os_sync::supabase::Server {
                url: url.trim().to_string(),
                key: key.trim().to_string(),
            });
            self.sync_service()?.set_server(server).map_err(err)?;
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (url, key, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    pub fn sync_send_code(&self, email: &str, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, false)?;
            self.sync_service()?.send_code(email).map_err(err)?;
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (email, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// The emailed code: creates the account (then the Recovery Kit), asks
    /// to join it (then the pairing code), or carries on. `backfill_days`:
    /// how far back this computer's visits upload; `None` for all.
    pub fn sync_verify(
        &self,
        code: &str,
        device_name: &str,
        backfill_days: Option<u32>,
        by: Option<&str>,
    ) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            let linked = self.sync_actor(by, false)?;
            let step =
                self.sync_service()?.verify_code(code, device_name, backfill_days, linked).map_err(err)?;
            Ok(serde_json::to_value(step).unwrap_or(Value::Null))
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (code, device_name, backfill_days, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// Look for clinic hubs on the local network for a few seconds (mDNS).
    pub fn sync_find_hubs(&self) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_service()?;
            let found = os_sync::hub::browse(std::time::Duration::from_secs(3)).map_err(err)?;
            Ok(serde_json::to_value(found).unwrap_or(Value::Null))
        }
        #[cfg(not(feature = "sync"))]
        Err(AppError(NOT_BUILT.into()))
    }

    /// Join a clinic hub (ADR-0029): the QR code's text in `link`, or the
    /// 8-character code there with the hub's address and certificate check.
    /// `existing`: the person already syncs on another device, which
    /// approves this one; otherwise this device creates their account and
    /// Recovery Kit.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_join_hub(
        &self,
        link: &str,
        address: &str,
        check: &str,
        device_name: &str,
        backfill_days: Option<u32>,
        existing: bool,
        by: Option<&str>,
    ) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            let linked = self.sync_actor(by, false)?;
            let join = os_sync::hub::parse_join(link, address, check).map_err(err)?;
            let step = self
                .sync_service()?
                .join_hub(&join, device_name, backfill_days, linked, existing)
                .map_err(err)?;
            self.sync_soon();
            Ok(serde_json::to_value(step).unwrap_or(Value::Null))
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (link, address, check, device_name, backfill_days, existing, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// Joined through a clinic hub: the code the approving device showed.
    /// Only after it does this device sync.
    pub fn sync_confirm_pairing(&self, code: &str, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, false)?;
            self.sync_service()?.confirm_pairing(code).map_err(err)?;
            self.sync_soon();
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (code, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// Give up joining (waiting, or approved into an account the person
    /// didn't confirm): this device leaves it and starts again.
    pub fn sync_start_over(&self, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, false)?;
            self.sync_service()?.start_over().map_err(err)?;
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = by;
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// Two words of the Recovery Kit, typed back: `[(position, word)]`.
    pub fn sync_confirm_kit(&self, answers: Vec<(usize, String)>, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, false)?;
            self.sync_service()?.confirm_kit(&answers).map_err(err)?;
            self.sync_soon();
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (answers, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// A pending computer: a fresh 6-word code.
    pub fn sync_new_code(&self) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            let v = self.sync_service()?.new_pairing_code().map_err(err)?;
            Ok(serde_json::to_value(v).unwrap_or(Value::Null))
        }
        #[cfg(not(feature = "sync"))]
        Err(AppError(NOT_BUILT.into()))
    }

    /// Find the waiting device whose 6 words were typed (`approve` false),
    /// or add it (`approve` true).
    pub fn sync_pair(&self, words: &str, approve: bool, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, true)?;
            let s = self.sync_service()?;
            let v = if approve { s.approve_pairing(words) } else { s.check_pairing(words) }.map_err(err)?;
            Ok(serde_json::to_value(v).unwrap_or(Value::Null))
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (words, approve, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// A pending computer adds itself with the Recovery Kit's 24 words.
    pub fn sync_recover(&self, words: &str, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, false)?;
            self.sync_service()?.recover_with_kit(words).map_err(err)?;
            self.sync_soon();
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (words, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// Sync now (also what the 30-second poll does).
    pub fn sync_now(&self) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            let s = self.sync_service()?.clone();
            if !s.is_on() {
                return Ok(self.sync_status());
            }
            // Writes visits: not while folders change (ADR-0013).
            let _work = self.begin_work()?;
            let report = s.sync_once(&AppHost(self)).map_err(err)?;
            let mut v = self.sync_status();
            v["report"] = serde_json::to_value(report).unwrap_or(Value::Null);
            Ok(v)
        }
        #[cfg(not(feature = "sync"))]
        Err(AppError(NOT_BUILT.into()))
    }

    pub fn sync_remove_device(&self, device_id: &str, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, true)?;
            self.sync_service()?.remove_device(device_id).map_err(err)?;
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (device_id, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// Turn off sync on this computer (sign out; local visits stay).
    pub fn sync_off(&self, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, true)?;
            self.sync_service()?.turn_off().map_err(err)?;
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = by;
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// Delete the sync account on the server. Local visits are untouched.
    pub fn sync_delete_account(&self, confirm: &str, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.sync_actor(by, true)?;
            self.sync_service()?.delete_account(confirm).map_err(err)?;
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (confirm, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// Stop syncing one visit: the cloud copy goes, every local copy stays.
    pub fn sync_stop_visit(&self, id: &str, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "sync")]
        {
            self.who(by, Need::Notes)?;
            let s = self.open(id, None)?;
            self.sync_service()?.stop_visit(&s.meta.id, &AppHost(self)).map_err(err)?;
            Ok(self.sync_status())
        }
        #[cfg(not(feature = "sync"))]
        {
            let _ = (id, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// The sync state of each visit (by `session.json` id) for the visit list.
    pub(crate) fn sync_states(&self) -> std::collections::BTreeMap<String, String> {
        #[cfg(feature = "sync")]
        if let Some(s) = &self.sync {
            return s.visit_states();
        }
        Default::default()
    }

    /// Whether a visit not yet known to sync will upload.
    pub(crate) fn sync_would(&self, s: &os_engine::session::Session) -> bool {
        #[cfg(feature = "sync")]
        if let Some(sync) = &self.sync {
            return sync.would_sync(s);
        }
        let _ = s;
        false
    }

    /// Run a sync soon (after turning it on, after Stop and save), off this thread.
    #[cfg(feature = "sync")]
    pub(crate) fn sync_soon(&self) {
        self.sync_wake.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Poll every 30 seconds while the app is open (and at once after sync
    /// is turned on). Stops when the app is retired or dropped.
    pub fn start_sync(self: &Arc<Self>) {
        // Hub mode, if it was on (ADR-0029): listening again.
        self.start_hub();
        #[cfg(feature = "sync")]
        {
            if self.sync.is_none() {
                return;
            }
            let app = Arc::downgrade(self);
            let every = interval();
            let spawned = std::thread::Builder::new().name("openbark-sync".into()).spawn(move || {
                loop {
                    {
                        let Some(app) = app.upgrade() else { return };
                        if app.gate.retired.load(std::sync::atomic::Ordering::SeqCst) {
                            return;
                        }
                        app.sync_wake.store(false, std::sync::atomic::Ordering::SeqCst);
                        if let Err(e) = app.sync_now() {
                            tracing::debug!(error = %e.0, "sync run");
                        }
                    }
                    let mut slept = std::time::Duration::ZERO;
                    while slept < every {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        slept += std::time::Duration::from_millis(500);
                        let Some(app) = app.upgrade() else { return };
                        if app.sync_wake.load(std::sync::atomic::Ordering::SeqCst) {
                            break;
                        }
                    }
                }
            });
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "sync couldn't start");
            }
        }
        #[cfg(not(feature = "sync"))]
        let _ = interval;
    }
}

#[cfg(not(feature = "sync"))]
const NOT_BUILT: &str = "This build of OpenBark doesn't include encrypted sync.";
