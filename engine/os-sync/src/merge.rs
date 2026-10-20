//! Merging (ADR-0017 section 9). On the device, deterministic, and never
//! destroying local data.
//!
//! - **Header fields:** last writer wins, field by field, by hybrid logical
//!   clock. A field changed here and on another device since they last met
//!   is a conflict: the later change is kept and both values go into the
//!   local conflict log.
//! - **Runs:** both devices' runs are kept. Signing is one-way: the run
//!   signed first is the record ([`settle`]); a later signature, or a draft
//!   made after the record was signed, is set aside beside it and offered as
//!   an addendum. Nothing is dropped.
//! - **Patients:** `os_engine::patients::merge` (same id: changed last wins;
//!   same animal and family: lowest id, the others as aliases).

use chrono::{DateTime, Local};
use os_engine::hlc::Hlc;
use os_engine::session::{SessionMeta, SetAside, SetAsideReason};

/// How one header field merges when the other device's value arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldMerge {
    /// Nothing new from the other device.
    KeepLocal,
    /// Take the other device's value (this device hadn't changed it).
    TakeRemote,
    /// Both changed it: the other device's change is later.
    ConflictTakeRemote,
    /// Both changed it: this device's change is later.
    ConflictKeepLocal,
}

/// Decide one field. `synced`: the clock timestamp this device last pushed
/// or pulled for it; `local_changed`: whether the value here differs from
/// what was last synced; `local_hlc`: the timestamp the local change gets.
pub fn merge_field(synced: Option<&Hlc>, local_changed: bool, local_hlc: &Hlc, remote: &Hlc) -> FieldMerge {
    if synced.is_some_and(|s| remote <= s) {
        return FieldMerge::KeepLocal;
    }
    if !local_changed {
        return FieldMerge::TakeRemote;
    }
    if remote > local_hlc { FieldMerge::ConflictTakeRemote } else { FieldMerge::ConflictKeepLocal }
}

/// The timestamp a change made on this device gets: when the value says it
/// was made (patient details and consent carry their time), else now; and
/// always after what this device last synced for the field.
pub fn local_stamp(
    clock: &mut os_engine::hlc::Clock,
    edited: Option<DateTime<Local>>,
    synced: Option<&Hlc>,
) -> Hlc {
    let now = clock.now();
    let Some(at) = edited else { return now };
    let device = now.device.clone();
    let mut stamp = Hlc { wall_ms: at.timestamp_millis().max(0) as u64, counter: 0, device };
    if let Some(s) = synced
        && stamp <= *s
    {
        stamp = Hlc { wall_ms: s.wall_ms, counter: s.counter.saturating_add(1), device: stamp.device };
    }
    // Never after the clock's own now: a value can't claim a future time.
    if stamp > now { now } else { stamp }
}

/// Mark the runs of a visit that met a signature from another device.
/// For each animal's medical-record runs: the run signed first is the
/// record; any other signed run is set aside as signed later; an unsigned
/// run drafted after the record is set aside as a draft made after the note
/// was signed elsewhere. Earlier drafts stay as history. Returns whether
/// anything changed. `name_of` gives a device name for a run's device tag.
pub fn settle(meta: &mut SessionMeta, name_of: &dyn Fn(Option<&str>) -> Option<String>) -> bool {
    let mut changed = false;
    let pets: Vec<Option<String>> = {
        let mut p: Vec<Option<String>> = meta.runs.iter().map(|r| r.pet.clone()).collect();
        p.sort();
        p.dedup();
        p
    };
    for pet in pets {
        let record = meta
            .runs
            .iter()
            .filter(|r| r.document.is_record() && r.pet == pet && r.signed_at.is_some())
            .min_by(|a, b| a.signed_at.cmp(&b.signed_at).then_with(|| a.id().cmp(b.id())))
            .map(|r| (r.id().to_string(), r.signed_at.expect("signed"), r.created_at, r.device.clone()));
        for run in meta.runs.iter_mut().filter(|r| r.document.is_record() && r.pet == pet) {
            let want = match &record {
                Some((rid, signed_at, created_at, device)) if run.id() != rid => {
                    let reason = if run.signed_at.is_some() {
                        Some(SetAsideReason::SignedLater)
                    } else if run.created_at > *created_at {
                        Some(SetAsideReason::SignedElsewhere)
                    } else {
                        None
                    };
                    reason.map(|reason| SetAside {
                        reason,
                        record_run: rid.clone(),
                        signed_at: *signed_at,
                        signed_on: name_of(device.as_deref()),
                    })
                }
                _ => None,
            };
            if run.set_aside != want {
                run.set_aside = want;
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use os_engine::session::RunInfo;

    fn h(ms: u64, dev: &str) -> Hlc {
        Hlc { wall_ms: ms, counter: 0, device: dev.into() }
    }

    #[test]
    fn fields_merge_last_writer_wins_and_log_conflicts() {
        let base = h(100, "a");
        // Nothing new.
        assert_eq!(merge_field(Some(&base), false, &h(150, "a"), &h(90, "b")), FieldMerge::KeepLocal);
        // The other device changed it; this one didn't.
        assert_eq!(merge_field(Some(&base), false, &h(150, "a"), &h(120, "b")), FieldMerge::TakeRemote);
        // Both changed it: the later change wins, whichever side.
        assert_eq!(merge_field(Some(&base), true, &h(150, "a"), &h(120, "b")), FieldMerge::ConflictKeepLocal);
        assert_eq!(merge_field(Some(&base), true, &h(110, "a"), &h(120, "b")), FieldMerge::ConflictTakeRemote);
        // Never synced before.
        assert_eq!(merge_field(None, false, &h(1, "a"), &h(1, "b")), FieldMerge::TakeRemote);
    }

    #[test]
    fn a_local_stamp_follows_the_edit_time_but_never_goes_back() {
        let mut clock = os_engine::hlc::Clock::new("a");
        let edited = Local.timestamp_millis_opt(1_000_000).unwrap();
        let s = local_stamp(&mut clock, Some(edited), None);
        assert_eq!(s.wall_ms, 1_000_000);
        let synced = h(2_000_000, "b");
        let s2 = local_stamp(&mut clock, Some(edited), Some(&synced));
        assert!(s2 > synced, "an edit is after what this device last saw");
        let future = Local::now() + Duration::days(3);
        let s3 = local_stamp(&mut clock, Some(future), None);
        assert!(s3.wall_ms < (Local::now() + Duration::days(1)).timestamp_millis() as u64);
    }

    fn run(id: &str, at_min: i64, signed_min: Option<i64>, device: &str) -> RunInfo {
        let base = Local.with_ymd_and_hms(2026, 10, 20, 9, 0, 0).unwrap();
        serde_json::from_value(serde_json::json!({
            "dir": format!("runs/{id}"), "run_id": id, "device": device, "llm": "m", "template": "soap@1",
            "created_at": base + Duration::minutes(at_min), "faithfulness_passed": true, "total_ms": 1,
            "signed_at": signed_min.map(|m| base + Duration::minutes(m)), "signed_by": signed_min.map(|_| "Dr P"),
        }))
        .unwrap()
    }

    fn meta(runs: Vec<RunInfo>) -> SessionMeta {
        let mut m: SessionMeta = serde_json::from_value(serde_json::json!({
            "id": "v", "app_version": "0", "created_at": Local::now(), "source": "s", "audio_sha256": "",
            "audio_secs": 0.0, "transcription": null, "runs": []
        }))
        .unwrap();
        m.runs = runs;
        m
    }

    #[test]
    fn the_first_signature_is_the_record_and_nothing_is_dropped() {
        // A drafted at 9:00; B drafted at 9:05 and signed at 9:06; A, offline,
        // redrafted at 9:10 and signed at 9:12.
        let mut m = meta(vec![run("r1", 0, None, "aaaa"), run("r2", 5, Some(6), "bbbb"), run("r3", 10, Some(12), "aaaa")]);
        let names = |tag: Option<&str>| tag.map(|t| format!("device {t}"));
        assert!(settle(&mut m, &names));
        assert_eq!(m.runs.len(), 3);
        assert_eq!(m.runs[0].set_aside, None, "an earlier draft stays history");
        assert_eq!(m.runs[1].set_aside, None, "the first signature is the record");
        let aside = m.runs[2].set_aside.clone().unwrap();
        assert_eq!(aside.reason, SetAsideReason::SignedLater);
        assert_eq!(aside.record_run, "r2");
        assert_eq!(aside.signed_on.as_deref(), Some("device bbbb"));
        // An unsigned draft made after the signature elsewhere.
        let mut m2 = meta(vec![run("r2", 5, Some(6), "bbbb"), run("r4", 8, None, "aaaa")]);
        settle(&mut m2, &names);
        assert_eq!(m2.runs[1].set_aside.as_ref().unwrap().reason, SetAsideReason::SignedElsewhere);
        // Settling again changes nothing, and the record is what the visit shows.
        assert!(!settle(&mut m2, &names));
        let s = os_engine::session::Session { dir: std::path::PathBuf::from("x"), meta: m2, pet: None };
        assert_eq!(s.latest_record().unwrap().id(), "r2");
        assert!(s.is_signed());
    }
}
