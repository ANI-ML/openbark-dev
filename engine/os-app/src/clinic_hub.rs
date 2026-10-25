//! Settings > Clinic hub (ADR-0029): this computer as the clinic's sync hub.
//!
//! Desktop builds only (the `hub` feature; phones never link the server),
//! and only where encrypted sync is offered at all (the preview gate in
//! sync.rs). Off by default. When it's on, the listener runs on one port
//! (default 47110) for the allowed client ranges, the hub advertises itself
//! on the local network, and its screen shows a join code. The hub's store
//! lives in `<visits>/.openbark/hub` (or `OPENBARK_HUB_DIR`), sealed under
//! this computer's data key.
//!
//! With a team, only a veterinarian who manages the team changes it.

use serde_json::{Value, json};

#[cfg(feature = "hub")]
use crate::Need;
use crate::{App, AppError, Result};

/// The hub's folder for a visits folder.
pub fn hub_dir(sessions_root: &std::path::Path) -> std::path::PathBuf {
    match std::env::var_os("OPENBARK_HUB_DIR").filter(|d| !d.is_empty()) {
        Some(d) => std::path::PathBuf::from(d),
        None => sessions_root.join(os_engine::HOUSEKEEPING_DIR).join("hub"),
    }
}

/// The hub's store as its server role sees it, opened with this computer's
/// data key (`openbark hub inspect`): structure and ciphertext only.
pub fn hub_inspect(dir: &std::path::Path) -> Result<Value> {
    #[cfg(feature = "hub")]
    {
        os_hub::Hub::inspect(dir).map_err(|e| AppError(e.to_string()))
    }
    #[cfg(not(feature = "hub"))]
    {
        let _ = dir;
        Err(AppError(NOT_BUILT.into()))
    }
}

#[cfg(not(feature = "hub"))]
const NOT_BUILT: &str = "This build of OpenBark can't be a clinic hub.";

/// The hub while this app runs.
#[cfg(feature = "hub")]
pub(crate) struct HubRuntime {
    hub: std::sync::Arc<os_hub::Hub>,
    identity: os_hub::tls::Identity,
    listener: Option<os_hub::Listener>,
    mdns: Option<os_hub::Advertiser>,
    settings: os_hub::Settings,
    error: Option<String>,
}

#[cfg(feature = "hub")]
impl HubRuntime {
    fn open(dir: &std::path::Path, settings: os_hub::Settings) -> Result<Self> {
        let identity = os_hub::tls::load_or_create(dir).map_err(|e| AppError(e.to_string()))?;
        let hub = os_hub::Hub::open(dir, &settings, os_hub::Limits::from_env())
            .map_err(|e| AppError(e.to_string()))?;
        Ok(Self {
            hub: std::sync::Arc::new(hub),
            identity,
            listener: None,
            mdns: None,
            settings,
            error: None,
        })
    }

    /// (Re)start the listener and the advertisement for the current settings.
    fn start(&mut self) {
        self.mdns = None;
        self.listener = None;
        match os_hub::Listener::start(self.hub.clone(), &self.identity, self.settings.port) {
            Ok(l) => {
                self.error = None;
                self.listener = Some(l);
                // OPENBARK_HUB_MDNS=0 turns advertising off (tests; networks that forbid multicast).
                if std::env::var("OPENBARK_HUB_MDNS").ok().is_none_or(|v| v != "0") {
                    match os_hub::Advertiser::start(self.settings.port, &self.identity.fingerprint()) {
                        Ok(a) => self.mdns = Some(a),
                        Err(e) => tracing::warn!(error = %e, "the hub isn't advertised on the local network"),
                    }
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn stop(&mut self) {
        self.mdns = None;
        if let Some(l) = self.listener.take() {
            l.stop();
        }
    }
}

impl App {
    /// Whether this install may be a clinic hub: a desktop build with the
    /// server, where encrypted sync is offered.
    pub fn clinic_hub_available(&self) -> bool {
        #[cfg(feature = "hub")]
        {
            crate::sync::preview_enabled() && !os_models::platform::Profile::current().is_mobile()
        }
        #[cfg(not(feature = "hub"))]
        false
    }

    #[cfg(feature = "hub")]
    fn hub_lock(&self) -> std::sync::MutexGuard<'_, Option<HubRuntime>> {
        self.hub.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Settings > Clinic hub. The live join code only for whoever may manage
    /// the hub (`by`: with a team, a veterinarian who manages it; anyone
    /// single-user): anyone else at the screen sees the hub without it.
    pub fn clinic_hub_status(&self, by: Option<&str>) -> Value {
        #[cfg(feature = "hub")]
        if self.clinic_hub_available() {
            let manager = self.who(by, Need::Team).is_ok();
            let dir = hub_dir(&self.sessions_root);
            let guard = self.hub_lock();
            let settings =
                guard.as_ref().map(|r| r.settings.clone()).unwrap_or_else(|| os_hub::Settings::load(&dir));
            let ranges = settings.ranges().unwrap_or_default();
            let addresses: Vec<String> =
                os_hub::net::lan_addresses(&ranges).iter().map(|a| a.to_string()).collect();
            let mut v = json!({
                "available": true,
                "on": settings.on,
                "listening": guard.as_ref().is_some_and(|r| r.listener.is_some()),
                "port": settings.port,
                "allowed": settings.allowed,
                "quota_mb": settings.quota_mb,
                "addresses": addresses,
                "error": guard.as_ref().and_then(|r| r.error.clone()),
                "join": null,
                "devices": [],
                "can_manage": manager,
            });
            if let Some(rt) = guard.as_ref() {
                let fp = rt.identity.fingerprint();
                let host = addresses.first().cloned().unwrap_or_else(|| "127.0.0.1".into());
                let fp_hex = rt.identity.fingerprint_hex();
                v["fingerprint"] = json!(fp_hex);
                if let Some(code) = rt.hub.shown_code().filter(|_| manager) {
                    // The check is keyed by this code: typed with it, it proves
                    // the certificate is this hub's.
                    let check = rt.identity.check_code(&code.code);
                    let link = os_sync::hub::join_link(&host, settings.port, &fp, &code.code);
                    v["join"] = json!({
                        "code": format!("{}-{}", &code.code[..4], &code.code[4..]),
                        "check": check,
                        "address": os_sync::hub::HubLink { host: host.clone(), port: settings.port, fingerprint: fp_hex.clone() }.address(),
                        "expires_at": code.expires_at,
                        "used": code.used,
                        "link": link,
                        "qr_svg": os_sync::keys::qr_svg(&link),
                    });
                }
                v["devices"] = serde_json::to_value(rt.hub.devices()).unwrap_or(Value::Null);
                let (accounts, bytes) = rt.hub.usage();
                v["accounts"] = json!(accounts);
                v["used_mb"] = json!((bytes as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0);
            }
            return v;
        }
        json!({"available": false, "on": false})
    }

    /// Change the hub's settings; `on` turns hub mode on or off.
    pub fn clinic_hub_settings(
        &self,
        on: Option<bool>,
        port: Option<u16>,
        allowed: Option<Vec<String>>,
        quota_mb: Option<u64>,
        by: Option<&str>,
    ) -> Result<Value> {
        #[cfg(feature = "hub")]
        {
            if !self.clinic_hub_available() {
                return Err(AppError("This computer can't be a clinic hub.".into()));
            }
            self.who(by, Need::Team)?;
            let dir = hub_dir(&self.sessions_root);
            let mut guard = self.hub_lock();
            let mut settings =
                guard.as_ref().map(|r| r.settings.clone()).unwrap_or_else(|| os_hub::Settings::load(&dir));
            let before = settings.clone();
            if let Some(p) = port {
                settings.port = p;
            }
            if let Some(a) = allowed {
                settings.allowed =
                    a.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            }
            if let Some(q) = quota_mb {
                settings.quota_mb = q;
            }
            if let Some(o) = on {
                settings.on = o;
            }
            settings.ranges().map_err(|e| AppError(e.to_string()))?;
            settings.save(&dir).map_err(|e| AppError(e.to_string()))?;
            if settings.on && guard.is_none() {
                *guard = Some(HubRuntime::open(&dir, settings.clone())?);
            }
            if let Some(rt) = guard.as_mut() {
                rt.hub.apply(&settings).map_err(|e| AppError(e.to_string()))?;
                let restart =
                    settings.on && (!before.on || before.port != settings.port || rt.listener.is_none());
                rt.settings = settings.clone();
                if !settings.on {
                    rt.stop();
                } else if restart {
                    rt.start();
                }
            }
            drop(guard);
            Ok(self.clinic_hub_status(by))
        }
        #[cfg(not(feature = "hub"))]
        {
            let _ = (on, port, allowed, quota_mb, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// A new join code on the hub's screen (single use, 30 minutes).
    pub fn clinic_hub_join_code(&self, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "hub")]
        {
            self.who(by, Need::Team)?;
            {
                let guard = self.hub_lock();
                let rt = guard
                    .as_ref()
                    .filter(|r| r.settings.on)
                    .ok_or_else(|| AppError("Turn on hub mode first.".into()))?;
                rt.hub.new_join_code().map_err(|e| AppError(e.to_string()))?;
            }
            Ok(self.clinic_hub_status(by))
        }
        #[cfg(not(feature = "hub"))]
        {
            let _ = by;
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// Remove a device from the hub (refused at once; its account's keys
    /// rotate at the next sync of one of its other devices).
    pub fn clinic_hub_remove_device(&self, device_id: &str, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "hub")]
        {
            self.who(by, Need::Team)?;
            {
                let guard = self.hub_lock();
                let rt = guard.as_ref().ok_or_else(|| AppError("Turn on hub mode first.".into()))?;
                rt.hub.remove_device(device_id).map_err(|e| AppError(e.to_string()))?;
            }
            Ok(self.clinic_hub_status(by))
        }
        #[cfg(not(feature = "hub"))]
        {
            let _ = (device_id, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// The hub computer's own device joins its hub (over this computer's
    /// loopback address, with a join code made here). `existing`: the person
    /// already syncs on another device, which then approves this one.
    pub fn clinic_hub_join_self(&self, device_name: &str, existing: bool, by: Option<&str>) -> Result<Value> {
        #[cfg(feature = "hub")]
        {
            // It makes a join code: only whoever may manage the hub.
            self.who(by, Need::Team)?;
            let link = {
                let guard = self.hub_lock();
                let rt = guard
                    .as_ref()
                    .filter(|r| r.listener.is_some())
                    .ok_or_else(|| AppError("Turn on hub mode first.".into()))?;
                let code = rt.hub.new_join_code().map_err(|e| AppError(e.to_string()))?;
                os_sync::hub::join_link("127.0.0.1", rt.settings.port, &rt.identity.fingerprint(), &code.code)
            };
            self.sync_join_hub(&link, "", "", device_name, None, existing, by)
        }
        #[cfg(not(feature = "hub"))]
        {
            let _ = (device_name, existing, by);
            Err(AppError(NOT_BUILT.into()))
        }
    }

    /// At start: hub mode on means listening again (the certificate and the
    /// store carry over).
    pub(crate) fn start_hub(&self) {
        #[cfg(feature = "hub")]
        {
            if !self.clinic_hub_available() {
                return;
            }
            let dir = hub_dir(&self.sessions_root);
            let settings = os_hub::Settings::load(&dir);
            if !settings.on {
                return;
            }
            let mut guard = self.hub_lock();
            if guard.is_some() {
                return;
            }
            match HubRuntime::open(&dir, settings) {
                Ok(mut rt) => {
                    rt.start();
                    *guard = Some(rt);
                }
                Err(e) => tracing::warn!(error = %e.0, "the clinic hub couldn't start"),
            }
        }
    }
}
