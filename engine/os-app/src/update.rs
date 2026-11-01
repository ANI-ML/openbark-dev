//! When the version check runs (ADR-0035, docs/network.md R16). The request
//! itself is `os_models::update`; this decides whether to make it.
//!
//! - The automatic check runs only when the interface asks for it, and the
//!   interface asks only while the person's setting is on (`updateCheck` in
//!   the app's settings: "ask", "on" or "off"; "ask" checks nothing). It
//!   reaches openbark.io at most once in 24 hours: the time of the last
//!   request is kept in a small file, so a restart doesn't check again and,
//!   in web mode, neither does a second browser tab.
//! - **Check for updates** always asks, whatever the setting: pressing it is
//!   the person's choice.
//! - The time is written before the request goes out, so a failing or slow
//!   answer still counts as the day's check.
//! - Nothing is downloaded or installed. Never on the iPhone, where the App
//!   Store and TestFlight handle updates.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use os_models::update::{self as release, Release, UpdateError};
use serde::{Deserialize, Serialize};

use crate::{App, AppError, Result};

/// The automatic check waits this long between requests.
pub const INTERVAL_SECS: u64 = 24 * 60 * 60;
/// Moves the check's small state file (tests, demos).
pub const FILE_ENV: &str = "OPENBARK_UPDATE_FILE";
const FILE: &str = "update-check.json";

const ON_PHONE: &str = "On the iPhone, the App Store and TestFlight handle updates.";
const NO_ANSWER: &str = "OpenBark couldn't reach openbark.io. Check the internet connection and try again.";

/// One check at a time, across every App in this process (web mode swaps in
/// a fresh App when a folder changes).
static CHECKING: Mutex<()> = Mutex::new(());

/// What the check knows, for the notice and Settings, About.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateView {
    /// The version running.
    pub current: String,
    /// This call asked openbark.io (false: under 24 hours since the last check).
    pub checked: bool,
    /// The newest published version seen, now or at the last check.
    pub latest: Option<String>,
    /// `latest` is newer than the version running.
    pub newer: bool,
    /// The release notes page for **What's new** (checked).
    pub notes: Option<String>,
    /// What **Download** opens: the website's download section.
    pub download: String,
    /// When openbark.io was last asked, in seconds since 1970.
    pub checked_at: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    checked_at: Option<u64>,
    #[serde(default)]
    latest: Option<Release>,
}

/// The automatic check is due: never checked, 24 hours since the last
/// check, or the clock was set back past it.
pub fn due(last: Option<u64>, now: u64) -> bool {
    match last {
        None => true,
        Some(at) if at > now => true,
        Some(at) => now - at >= INTERVAL_SECS,
    }
}

fn state_path() -> PathBuf {
    match std::env::var_os(FILE_ENV) {
        Some(p) => PathBuf::from(p),
        None => os_models::locations::app_data_dir().join(FILE),
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn load(path: &Path) -> State {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(path: &Path, state: &State) {
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(path, serde_json::to_vec(state).unwrap_or_default()));
    if let Err(e) = written {
        tracing::warn!(error = %e, "couldn't save the update check's time");
    }
}

fn view(current: &str, state: &State, checked: bool) -> UpdateView {
    // Re-checked on the way out: the file is only a cache.
    let latest = state.latest.as_ref().and_then(|r| release::Version::parse(&r.version).map(|v| (v, r)));
    UpdateView {
        current: current.to_string(),
        checked,
        latest: latest.map(|(v, _)| v.to_string()),
        newer: latest.is_some_and(|(v, _)| release::is_newer(&v.to_string(), current)),
        notes: latest.map(|(_, r)| release::safe_notes(&r.notes)),
        download: release::DOWNLOAD_PAGE.to_string(),
        checked_at: state.checked_at,
    }
}

/// One check against the state kept in `path`. `manual`: the person pressed
/// **Check for updates**, so the 24 hours don't apply. `fetch` is the request.
fn run(
    path: &Path,
    now: u64,
    manual: bool,
    current: &str,
    fetch: impl FnOnce() -> std::result::Result<Option<Release>, UpdateError>,
) -> Result<UpdateView> {
    let mut state = load(path);
    if !manual && !due(state.checked_at, now) {
        return Ok(view(current, &state, false));
    }
    state.checked_at = Some(now);
    save(path, &state);
    match fetch() {
        Ok(latest) => {
            state.latest = latest;
            save(path, &state);
            Ok(view(current, &state, true))
        }
        Err(e) => {
            tracing::info!(error = %e, manual, "the update check got no answer");
            Err(AppError(NO_ANSWER.into()))
        }
    }
}

impl App {
    /// The version check: `manual` when the person pressed **Check for
    /// updates**; otherwise at most one request in 24 hours (the interface
    /// asks only while the daily check is on). Never on the iPhone.
    pub fn check_for_update(&self, manual: bool) -> Result<UpdateView> {
        if cfg!(target_os = "ios") {
            return Err(AppError(ON_PHONE.into()));
        }
        let _one = CHECKING.lock().unwrap_or_else(|p| p.into_inner());
        run(&state_path(), now_secs(), manual, env!("CARGO_PKG_VERSION"), release::fetch)
    }

    /// What the last check found, without asking openbark.io (Settings, About).
    pub fn update_status(&self) -> UpdateView {
        view(env!("CARGO_PKG_VERSION"), &load(&state_path()), false)
    }

    /// The page for **What's new** (`notes`) or **Download** (`download`), from
    /// the last check: only addresses `os_models::update` accepted.
    pub fn release_link(&self, which: &str) -> Result<String> {
        match which {
            "download" => Ok(release::DOWNLOAD_PAGE.to_string()),
            "notes" => Ok(self.update_status().notes.unwrap_or_else(|| release::NOTES_FALLBACK.to_string())),
            _ => Err(AppError(format!("Unknown release page: {which}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const DAY: u64 = INTERVAL_SECS;
    const T0: u64 = 1_793_491_200;

    fn rel(version: &str) -> Release {
        Release {
            version: version.into(),
            notes: format!("https://github.com/ANI-ML/openbark-releases/releases/tag/v{version}"),
        }
    }

    #[test]
    fn the_automatic_check_is_due_once_a_day() {
        assert!(due(None, T0), "never checked");
        assert!(!due(Some(T0), T0));
        assert!(!due(Some(T0), T0 + DAY - 1), "23:59:59 later");
        assert!(due(Some(T0), T0 + DAY), "24 hours later");
        assert!(due(Some(T0), T0 + 3 * DAY));
        assert!(due(Some(T0 + DAY), T0), "the clock was set back");
    }

    #[test]
    fn the_automatic_check_asks_at_most_once_in_24_hours() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let asked = Cell::new(0);
        let fetch = || {
            asked.set(asked.get() + 1);
            Ok(Some(rel("0.2.10")))
        };
        // At start, never checked: asks.
        let first = run(&path, T0, false, "0.2.9", fetch).unwrap();
        assert!(first.checked && first.newer);
        assert_eq!(first.latest.as_deref(), Some("0.2.10"));
        // Hourly while open: doesn't ask again, but still reports what it found.
        for hour in 1..24 {
            let later = run(&path, T0 + hour * 3600, false, "0.2.9", fetch).unwrap();
            assert!(!later.checked, "hour {hour}");
            assert!(later.newer);
            assert_eq!(later.latest.as_deref(), Some("0.2.10"));
        }
        assert_eq!(asked.get(), 1);
        // A restart within the day doesn't ask either: the time is in the file.
        let restarted = run(&path, T0 + 2 * 3600, false, "0.2.9", || panic!("asked within the day")).unwrap();
        assert!(!restarted.checked);
        // 24 hours on: asks again.
        assert!(run(&path, T0 + DAY, false, "0.2.9", fetch).unwrap().checked);
        assert_eq!(asked.get(), 2);
    }

    #[test]
    fn check_for_updates_always_asks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let asked = Cell::new(0);
        let fetch = || {
            asked.set(asked.get() + 1);
            Ok(Some(rel("0.2.4")))
        };
        assert!(run(&path, T0, false, "0.2.4", fetch).unwrap().checked);
        let pressed = run(&path, T0 + 60, true, "0.2.4", fetch).unwrap();
        assert!(pressed.checked);
        assert!(!pressed.newer, "the same version: the latest");
        assert!(run(&path, T0 + 120, true, "0.2.4", fetch).unwrap().checked);
        assert_eq!(asked.get(), 3);
    }

    #[test]
    fn a_failed_check_still_counts_as_the_days_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let err = run(&path, T0, false, "0.2.4", || Err(UpdateError::Network("offline".into()))).unwrap_err();
        assert_eq!(err.0, NO_ANSWER);
        let later = run(&path, T0 + 3600, false, "0.2.4", || panic!("asked again within the day")).unwrap();
        assert!(!later.checked);
        assert_eq!(later.checked_at, Some(T0));
        assert_eq!(later.latest, None);
    }

    #[test]
    fn only_a_newer_version_is_announced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        for (latest, current, newer) in [
            ("0.2.10", "0.2.9", true),
            ("0.2.9", "0.2.10", false),
            ("0.2.4", "0.2.4", false),
            ("1.0.0", "0.9.9", true),
        ] {
            let v = run(&path, T0, true, current, || Ok(Some(rel(latest)))).unwrap();
            assert_eq!(v.newer, newer, "{latest} over {current}");
            assert_eq!(v.current, current);
        }
    }

    #[test]
    fn an_unpublished_release_clears_the_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        assert!(run(&path, T0, true, "0.2.4", || Ok(Some(rel("0.2.5")))).unwrap().newer);
        // release.json says published: false (os_models reads that as None).
        let v = run(&path, T0 + 60, true, "0.2.4", || Ok(None)).unwrap();
        assert!(!v.newer);
        assert_eq!(v.latest, None);
        assert_eq!(v.notes, None);
    }

    #[test]
    fn a_tampered_state_file_cant_announce_garbage_or_open_a_strange_page() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let bad = State {
            checked_at: Some(T0),
            latest: Some(Release { version: "<b>9.9.9</b>".into(), notes: "https://evil.example/".into() }),
        };
        save(&path, &bad);
        let v = run(&path, T0 + 60, false, "0.2.4", || panic!("not due")).unwrap();
        assert_eq!(v.latest, None);
        assert!(!v.newer);
        let odd = Release { version: "0.3.0".into(), notes: "javascript:alert(1)".into() };
        let odd = State { checked_at: Some(T0), latest: Some(odd) };
        save(&path, &odd);
        let v = run(&path, T0 + 60, false, "0.2.4", || panic!("not due")).unwrap();
        assert_eq!(v.notes.as_deref(), Some(release::NOTES_FALLBACK));
        std::fs::write(&path, b"not json").unwrap();
        assert!(
            run(&path, T0 + 60, false, "0.2.4", || Ok(None)).unwrap().checked,
            "unreadable: as if never checked"
        );
    }
}
