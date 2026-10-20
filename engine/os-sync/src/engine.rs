//! One sync run: pull what other devices sealed, merge it into the visit
//! folders, then seal and push what changed here.
//!
//! Every object pulled is checked before anything is written: its structure
//! (OBS1), its header against the row (id, kind, space, version: no
//! ciphertext moved or replayed under another name), its signature against
//! the roster, and its version against the last one seen (no rollback).
//! Visits are written through `os_engine::session` and the vault, under this
//! device's data key, holding the app's per-visit lock.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};

use os_engine::hlc::Hlc;
use os_engine::patients::{PatientRecord, PatientStore};
use os_engine::session::{self, AudioOn, Session, SessionMeta};

use crate::keys::{self, SymKey};
use crate::map::{self, Body, Commit, Stamped, VISIT_FIELDS};
use crate::merge::{self, FieldMerge};
use crate::seal::{self, Header};
use crate::state::{self, Conflict, Deferred, FieldSync, ObjSync, OutItem, Phase, Seen, State, VisitSync};
use crate::{Inner, RemoteRow, SyncError, SyncService, guard};

/// What the app lends sync for a run.
pub trait Host: Send + Sync {
    /// Hold the visit in folder `folder` while sync writes into it (the
    /// app's per-visit lock). `None`: it's busy; sync tries again later.
    fn lock_visit(&self, folder: &str) -> Option<Box<dyn std::any::Any>>;
}

/// No app around (tests, the CLI): every visit is free.
pub struct NoHost;

impl Host for NoHost {
    fn lock_visit(&self, _: &str) -> Option<Box<dyn std::any::Any>> {
        Some(Box::new(()))
    }
}

/// What one run did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    pub pulled: usize,
    pub applied: usize,
    pub adopted: usize,
    pub deferred: usize,
    pub pushed: usize,
    pub conflicts: usize,
}

const PULL_PAGE: usize = 200;
const PUSH_BATCH: usize = 50;

enum Applied {
    Done,
    Later,
}

/// Visits by id: their folders (rebuilt each run; visits are few).
type Index = BTreeMap<String, PathBuf>;

fn index(root: &std::path::Path) -> Index {
    session::list(root).into_iter().map(|s| (s.meta.id.clone(), s.dir)).collect()
}

fn folder_name(dir: &std::path::Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn transcript_hash(s: &Session) -> Option<(Vec<u8>, String)> {
    let path = s.transcript_path();
    if !path.is_file() {
        return None;
    }
    let text = os_engine::vault::read(&path).ok()?;
    let info = serde_json::to_vec(&s.meta.transcription).unwrap_or_default();
    let h = map::hash_bytes(&[text.as_slice(), b"|", &info].concat());
    Some((text, h))
}

fn parse_hlc(s: &str) -> Option<Hlc> {
    s.parse().ok()
}

impl SyncService {
    /// One sync run. Does nothing while sync is off.
    pub fn sync_once(&self, host: &dyn Host) -> Result<SyncReport, SyncError> {
        let mut inner = self.lock();
        if !matches!(inner.state.phase, Phase::Active | Phase::Pending | Phase::KitCheck) {
            return Ok(SyncReport::default());
        }
        self.publish(&inner, true);
        let mut report = SyncReport::default();
        let result = self.run(&mut inner, host, &mut report);
        let result = match result {
            Err(SyncError::Revoked) => {
                self.handle_revoked(&mut inner);
                Err(SyncError::Revoked)
            }
            Err(e) if locked_out(&e) => {
                if self.was_revoked(&inner) {
                    self.handle_revoked(&mut inner);
                    Err(SyncError::Revoked)
                } else {
                    if matches!(e, SyncError::SignedOut) {
                        inner.state.phase = Phase::SignedOut;
                    }
                    Err(e)
                }
            }
            other => other,
        };
        match &result {
            Ok(_) => {
                inner.state.last_error = None;
                inner.state.last_sync_at = Some(Utc::now());
            }
            Err(SyncError::Offline) => inner.state.last_error = Some(SyncError::Offline.to_string()),
            Err(e) => inner.state.last_error = Some(e.to_string()),
        }
        let saved = self.persist(&mut inner);
        self.publish(&inner, false);
        saved?;
        result.map(|_| report)
    }

    /// Without a session: did the server revoke this device?
    fn was_revoked(&self, inner: &Inner) -> bool {
        let Ok(Some(device)) = inner.secrets.device_keys() else { return false };
        let args = json!({"device_id": self.me().id, "rebind_secret": keys::b64(device.rebind_secret())});
        matches!(self.client(&inner.state).rpc("device_status", args, None), Ok(Value::String(s)) if s == "revoked")
    }

    fn run(&self, inner: &mut Inner, host: &dyn Host, report: &mut SyncReport) -> Result<(), SyncError> {
        if inner.state.phase == Phase::Pending {
            if !self.poll_pending(inner)? {
                return Ok(());
            }
        }
        if inner.state.phase == Phase::KitCheck {
            self.load_devices(inner);
            return Ok(());
        }
        let acct = self.rpc(inner, "account_state", json!({}))?;
        match acct["device"]["status"].as_str() {
            Some("approved") => {}
            _ => {
                // Not bound to this session (signed in again elsewhere?): rebind.
                let device = self.device_keys(inner)?;
                let r = self.rpc(
                    inner,
                    "rebind_device",
                    json!({"device_id": self.me().id, "rebind_secret": keys::b64(device.rebind_secret())}),
                )?;
                if r["device"]["status"] != "approved" {
                    return Err(SyncError::Revoked);
                }
            }
        }
        let acct = self.rpc(inner, "account_state", json!({}))?;
        if acct["amk_version"].as_u64().unwrap_or_default() as u32 > inner.state.amk_version
            || inner.secrets.keys.as_ref().is_none_or(|k| k.space_keys.is_empty())
        {
            self.refresh_keys(inner)?;
        } else if inner.roster.is_none() || acct["roster_version"].as_i64() != Some(inner.state.roster_version) {
            self.refresh_roster(inner)?;
        }
        if acct["rotate_needed"].as_bool() == Some(true)
            && let Err(e) = self.rotate(inner)
        {
            tracing::warn!(error = %e, "key rotation will be tried again");
        }
        self.load_devices(inner);
        self.pull_all(inner, host, report)?;
        self.retry_deferred(inner, host, report);
        self.scan(inner)?;
        self.flush(inner, host, report)?;
        Ok(())
    }

    // ---- pull -------------------------------------------------------------

    fn pull_all(&self, inner: &mut Inner, host: &dyn Host, report: &mut SyncReport) -> Result<(), SyncError> {
        let space = inner.state.space_id.clone().ok_or(SyncError::NoKeys)?;
        let mut idx = index(&self.root);
        loop {
            let v = self.rpc(
                inner,
                "pull",
                json!({"space_id": space, "after": inner.state.cursor, "max_rows": PULL_PAGE}),
            )?;
            let rows: Vec<RemoteRow> = serde_json::from_value(v["rows"].clone()).unwrap_or_default();
            let n = rows.len();
            for row in rows {
                report.pulled += 1;
                let seq = row.seq;
                match self.apply_row(inner, host, &mut idx, &row, report) {
                    Ok(Applied::Done) => report.applied += 1,
                    Ok(Applied::Later) => {
                        report.deferred += 1;
                        inner.state.deferred.retain(|d| d.row.id != row.id);
                        inner.state.deferred.push(Deferred { row, tries: 0 });
                    }
                    Err(e) => inner.state.warn(format!("An object from the server was set aside: {e}")),
                }
                inner.state.cursor = inner.state.cursor.max(seq);
            }
            if n < PULL_PAGE {
                break;
            }
        }
        Ok(())
    }

    fn retry_deferred(&self, inner: &mut Inner, host: &dyn Host, report: &mut SyncReport) {
        let mut idx = index(&self.root);
        // A few passes: a run's signature can wait on the run, the run on its visit.
        for _ in 0..3 {
            let pending = std::mem::take(&mut inner.state.deferred);
            if pending.is_empty() {
                return;
            }
            let before = pending.len();
            for mut d in pending {
                match self.apply_row(inner, host, &mut idx, &d.row, report) {
                    Ok(Applied::Done) => report.applied += 1,
                    Ok(Applied::Later) => {
                        d.tries += 1;
                        if d.tries > 200 {
                            inner.state.warn(format!("A {} object never found its visit and was set aside.", d.row.kind));
                        } else {
                            inner.state.deferred.push(d);
                        }
                    }
                    Err(e) => inner.state.warn(format!("An object from the server was set aside: {e}")),
                }
            }
            if inner.state.deferred.len() == before {
                return;
            }
        }
    }

    /// Check and open one row. Errors here are integrity problems.
    fn open_row(&self, inner: &mut Inner, row: &RemoteRow) -> Result<(Header, Body), SyncError> {
        let space = inner.state.space_id.clone().ok_or(SyncError::NoKeys)?;
        let bytes = keys::unb64(row.body.as_deref().unwrap_or_default())?;
        let obj = seal::parse_obs1(&bytes)?;
        let h = &obj.header;
        if h.id != row.id || h.kind != row.kind || h.space != space {
            return Err(SyncError::Integrity("an object's sealed name doesn't match where the server put it".into()));
        }
        if h.ver as i64 != row.version {
            return Err(SyncError::Integrity(format!(
                "a {} object came back under another version (a replay?)",
                row.kind
            )));
        }
        if let Some(seen) = inner.state.seen.get(&row.id)
            && row.version < seen.version
        {
            return Err(SyncError::Integrity(format!("an older version of a {} object came back (a rollback?)", row.kind)));
        }
        let mut signer = inner.roster.as_ref().and_then(|r| r.signer(&h.dev, h.ctr));
        if signer.is_none() {
            self.refresh_roster(inner)?;
            signer = inner.roster.as_ref().and_then(|r| r.signer(&h.dev, h.ctr));
        }
        let signer = signer.ok_or_else(|| SyncError::Integrity("an object signed by a device the device list doesn't allow".into()))?;
        if !seal::verify_obs1(&obj, &signer) {
            return Err(SyncError::Integrity("an object's signature is wrong".into()));
        }
        let kv = row.key_version.ok_or_else(|| SyncError::Format("no key version".into()))?;
        if self.account_keys(inner)?.space_key(kv).is_err() {
            self.refresh_keys(inner)?;
        }
        let sk = self.account_keys(inner)?.space_key(kv)?;
        let wrapped = keys::unb64(row.wrapped_dek.as_deref().unwrap_or_default())?;
        let dek = SymKey(
            keys::unwrap(&sk, &keys::dek_aad(&space, &row.id, kv), &wrapped)?
                .try_into()
                .map_err(|_| SyncError::Format("bad object key".into()))?,
        );
        let plain = seal::open_obs1(&obj, &dek)?;
        let body: Body = serde_json::from_slice(&plain).map_err(|_| SyncError::Format("damaged object".into()))?;
        let header = h.clone();
        drop(obj);
        // Blobs need the DEK: keep it for this row.
        inner.last_dek = Some((row.id.clone(), row.version, dek));
        Ok((header, body))
    }

    fn apply_row(
        &self,
        inner: &mut Inner,
        host: &dyn Host,
        idx: &mut Index,
        row: &RemoteRow,
        report: &mut SyncReport,
    ) -> Result<Applied, SyncError> {
        if row.deleted {
            inner.state.seen.insert(row.id.clone(), Seen { version: row.version, dev: String::new(), ctr: 0 });
            return Ok(Applied::Done);
        }
        // This device's own object, already recorded.
        if inner.state.seen.get(&row.id).is_some_and(|s| s.version == row.version && s.dev == self.me().id) {
            return Ok(Applied::Done);
        }
        let (header, body) = self.open_row(inner, row)?;
        if let Some(h) = parse_hlc(&body.hlc) {
            inner.clock.observe(&h);
        }
        let out = match row.kind.as_str() {
            "visit" => self.apply_visit(inner, host, idx, row, &header, &body, report)?,
            "transcript" => self.apply_transcript(inner, host, idx, row, &body)?,
            "run" => self.apply_run(inner, host, idx, row, &body)?,
            "event" => self.apply_event(inner, host, idx, &body)?,
            "checks" => self.apply_checks(inner, host, idx, row, &body)?,
            "patient" => self.apply_patient(inner, row, &body)?,
            "tombstone" => {
                if let Some(v) = body.data["session_id"].as_str() {
                    inner.state.stopped.insert(v.to_string());
                    state::audit(&self.paths, "visit_stopped_elsewhere", json!({"visit": v}));
                }
                Applied::Done
            }
            _ => Applied::Done,
        };
        if matches!(out, Applied::Done) {
            inner.state.seen.insert(row.id.clone(), Seen { version: row.version, dev: header.dev, ctr: header.ctr });
        }
        Ok(out)
    }

    /// The visit a row belongs to, held: `Ok(None)` when it isn't here (yet)
    /// or is busy.
    fn held_visit(
        &self,
        inner: &Inner,
        host: &dyn Host,
        idx: &Index,
        session_id: &str,
    ) -> Option<(Session, Box<dyn std::any::Any>)> {
        let dir = idx.get(session_id)?;
        if inner.state.dropped.contains(session_id) {
            return None;
        }
        let guard = host.lock_visit(&folder_name(dir))?;
        let s = Session::open(dir).ok()?;
        Some((s, guard))
    }

    fn skip_visit(&self, inner: &Inner, session_id: &str) -> bool {
        inner.state.stopped.contains(session_id) || inner.state.dropped.contains(session_id)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_visit(
        &self,
        inner: &mut Inner,
        host: &dyn Host,
        idx: &mut Index,
        row: &RemoteRow,
        header: &Header,
        body: &Body,
        report: &mut SyncReport,
    ) -> Result<Applied, SyncError> {
        let vb: map::VisitBody = serde_json::from_value(body.data.clone()).map_err(|e| SyncError::Format(e.to_string()))?;
        if self.skip_visit(inner, &vb.session_id) {
            return Ok(Applied::Done);
        }
        let from = self.name_of(inner, &header.dev).unwrap_or_else(|| "another device".into());
        if !idx.contains_key(&vb.session_id) {
            // A visit made on another device: a new folder, under this
            // device's data key. Its audio stays where it was recorded.
            let mut meta: SessionMeta = serde_json::from_value(json!({
                "id": vb.session_id, "app_version": vb.app_version, "created_at": vb.created_at,
                "source": "", "audio_sha256": "", "audio_secs": 0.0, "transcription": null, "runs": [],
            }))
            .map_err(|e| SyncError::Format(e.to_string()))?;
            for (f, st) in &vb.fields {
                map::apply_field(&mut meta, f, &st.v)?;
            }
            meta.audio_on = Some(AudioOn {
                device_id: vb.audio_on.clone(),
                device_name: self.name_of(inner, &vb.audio_on).unwrap_or_default(),
            });
            let s = Session::adopt(&self.root, meta)?;
            let values = map::field_values(&s.meta);
            let vs = inner.state.visits.entry(vb.session_id.clone()).or_default();
            vs.adopted = true;
            vs.header.version = Some(row.version);
            for f in VISIT_FIELDS {
                if let Some(st) = vb.fields.get(f) {
                    vs.fields.insert(f.into(), FieldSync { hash: map::hash_value(&values[f]), hlc: st.t.clone() });
                }
            }
            idx.insert(vb.session_id.clone(), s.dir.clone());
            report.adopted += 1;
            return Ok(Applied::Done);
        }
        let Some((mut s, _guard)) = self.held_visit(inner, host, idx, &vb.session_id) else { return Ok(Applied::Later) };
        let mut vs = inner.state.visits.get(&vb.session_id).cloned().unwrap_or_default();
        let mut changed = false;
        for f in VISIT_FIELDS {
            let Some(remote) = vb.fields.get(f) else { continue };
            let Some(remote_hlc) = parse_hlc(&remote.t) else { continue };
            let local_v = map::field_values(&s.meta)[f].clone();
            let local_h = map::hash_value(&local_v);
            let synced = vs.fields.get(f).cloned();
            let synced_hlc = synced.as_ref().and_then(|x| parse_hlc(&x.hlc));
            if local_h == map::hash_value(&remote.v) {
                let t = match &synced_hlc {
                    Some(s) if *s > remote_hlc => s.to_string(),
                    _ => remote.t.clone(),
                };
                vs.fields.insert(f.into(), FieldSync { hash: local_h, hlc: t });
                continue;
            }
            let local_changed = synced.as_ref().is_none_or(|x| x.hash != local_h);
            let local_hlc = if local_changed {
                merge::local_stamp(&mut inner.clock, map::edit_time(&s.meta, f), synced_hlc.as_ref())
            } else {
                synced_hlc.clone().unwrap_or(Hlc { wall_ms: 0, counter: 0, device: String::new() })
            };
            match merge::merge_field(synced_hlc.as_ref(), local_changed, &local_hlc, &remote_hlc) {
                FieldMerge::KeepLocal => {}
                FieldMerge::TakeRemote | FieldMerge::ConflictTakeRemote => {
                    if local_changed {
                        self.log_conflict(inner, &vb.session_id, f, remote.v.clone(), local_v.clone(), &from, "this device");
                    }
                    map::apply_field(&mut s.meta, f, &remote.v)?;
                    changed = true;
                    let after = map::hash_value(&map::field_values(&s.meta)[f]);
                    vs.fields.insert(f.into(), FieldSync { hash: after, hlc: remote.t.clone() });
                }
                FieldMerge::ConflictKeepLocal => {
                    self.log_conflict(inner, &vb.session_id, f, local_v.clone(), remote.v.clone(), "this device", &from);
                    // Recorded as the other's value: the next scan sees the
                    // local one as changed and pushes it with a later stamp.
                    vs.fields.insert(f.into(), FieldSync { hash: map::hash_value(&remote.v), hlc: remote.t.clone() });
                }
            }
        }
        if s.meta.audio_on.is_none() && vb.audio_on != self.me().id && !s.has_audio() {
            s.meta.audio_on = Some(AudioOn {
                device_id: vb.audio_on.clone(),
                device_name: self.name_of(inner, &vb.audio_on).unwrap_or_default(),
            });
            changed = true;
        }
        vs.header.version = Some(row.version);
        if changed {
            s.save()?;
        }
        inner.state.visits.insert(vb.session_id.clone(), vs);
        Ok(Applied::Done)
    }

    #[allow(clippy::too_many_arguments)]
    fn log_conflict(
        &self,
        inner: &mut Inner,
        visit: &str,
        field: &str,
        kept: Value,
        other: Value,
        kept_from: &str,
        other_from: &str,
    ) {
        inner.state.conflicts.push(Conflict {
            visit: visit.into(),
            field: field.into(),
            kept,
            other,
            kept_from: kept_from.into(),
            other_from: other_from.into(),
            at: Utc::now(),
        });
        if inner.state.conflicts.len() > 500 {
            inner.state.conflicts.remove(0);
        }
    }

    /// A blob's plaintext, for the row just opened.
    fn fetch_blob(&self, inner: &mut Inner, row: &RemoteRow, hashes: &[String]) -> Result<Vec<u8>, SyncError> {
        let space = inner.state.space_id.clone().ok_or(SyncError::NoKeys)?;
        let (_, _, dek) = inner
            .last_dek
            .clone()
            .filter(|(id, v, _)| *id == row.id && *v == row.version)
            .ok_or(SyncError::NoKeys)?;
        let v = self.rpc(
            inner,
            "get_blobs",
            json!({"space_id": space, "object_id": row.id, "object_version": row.version}),
        )?;
        let parts: Vec<Vec<u8>> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| keys::unb64(p["body"].as_str().unwrap_or_default()))
            .collect::<Result<_, _>>()?;
        let roster = inner.roster.clone();
        let signer = move |dev: &str| roster.as_ref().and_then(|r| r.get(dev)).and_then(|e| e.ed_pub());
        let plain = seal::open_blob(&parts, hashes, &row.id, &space, row.version as u32, &dek, &signer)?;
        guard::refuse_audio(&plain).map_err(|_| SyncError::Integrity("a blob looked like a recording".into()))?;
        Ok(plain)
    }

    fn apply_transcript(
        &self,
        inner: &mut Inner,
        host: &dyn Host,
        idx: &Index,
        row: &RemoteRow,
        body: &Body,
    ) -> Result<Applied, SyncError> {
        let tb: map::TranscriptBody = serde_json::from_value(body.data.clone()).map_err(|e| SyncError::Format(e.to_string()))?;
        if self.skip_visit(inner, &tb.session_id) {
            return Ok(Applied::Done);
        }
        let Some((mut s, _guard)) = self.held_visit(inner, host, idx, &tb.session_id) else { return Ok(Applied::Later) };
        let mut vs = inner.state.visits.get(&tb.session_id).cloned().unwrap_or_default();
        let remote_hlc = parse_hlc(&body.hlc);
        let synced_hlc = parse_hlc(&vs.transcript.hlc);
        if remote_hlc.is_some() && synced_hlc.is_some() && remote_hlc <= synced_hlc {
            vs.transcript.version = Some(row.version.max(vs.transcript.version.unwrap_or_default()));
            inner.state.visits.insert(tb.session_id.clone(), vs);
            return Ok(Applied::Done);
        }
        let local = transcript_hash(&s);
        let local_changed = local.as_ref().is_some_and(|(_, h)| *h != vs.transcript.hash);
        if local_changed && s.has_audio() {
            // Transcribed again here, where the audio is: this device's
            // version wins and is pushed next.
            vs.transcript.version = Some(row.version);
            vs.transcript.hlc = body.hlc.clone();
            inner.state.visits.insert(tb.session_id.clone(), vs);
            return Ok(Applied::Done);
        }
        let text = self.fetch_blob(inner, row, &tb.parts)?;
        let path = s.transcript_path();
        os_engine::vault::write(&path, &text).map_err(|e| SyncError::Io(e.to_string()))?;
        s.meta.transcription = tb.info.clone();
        s.save()?;
        vs.transcript = ObjSync {
            version: Some(row.version),
            hash: transcript_hash(&s).map(|(_, h)| h).unwrap_or_default(),
            hlc: body.hlc.clone(),
        };
        inner.state.visits.insert(tb.session_id.clone(), vs);
        Ok(Applied::Done)
    }

    fn apply_run(
        &self,
        inner: &mut Inner,
        host: &dyn Host,
        idx: &Index,
        row: &RemoteRow,
        body: &Body,
    ) -> Result<Applied, SyncError> {
        let rb: map::RunBody = serde_json::from_value(body.data.clone()).map_err(|e| SyncError::Format(e.to_string()))?;
        if self.skip_visit(inner, &rb.session_id) {
            return Ok(Applied::Done);
        }
        let Some((mut s, _guard)) = self.held_visit(inner, host, idx, &rb.session_id) else { return Ok(Applied::Later) };
        let rid = rb.run.id().to_string();
        if !s.meta.runs.iter().any(|r| r.id() == rid) {
            let files = map::run_files_from(&self.fetch_blob(inner, row, &rb.parts)?)?;
            let leaf = map::safe_run_leaf(&rb.run.dir).unwrap_or_else(|| format!("{:03}_sync", s.meta.runs.len() + 1));
            let mut name = leaf.clone();
            let mut n = 2;
            while s.dir.join("runs").join(&name).exists() || s.meta.runs.iter().any(|r| r.dir == format!("runs/{name}")) {
                name = format!("{leaf}-{n}");
                n += 1;
            }
            let dir = s.dir.join("runs").join(&name);
            std::fs::create_dir_all(&dir).map_err(|e| SyncError::Io(e.to_string()))?;
            for (file, bytes) in files {
                os_engine::vault::write(&dir.join(file), &bytes).map_err(|e| SyncError::Io(e.to_string()))?;
            }
            let mut run = map::strip_run(&rb.run);
            run.dir = format!("runs/{name}");
            s.meta.runs.push(run);
            s.meta.runs.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id().cmp(b.id())));
            self.settle(inner, &mut s.meta);
            s.save()?;
        }
        inner.state.visits.entry(rb.session_id.clone()).or_default().runs.insert(rid);
        Ok(Applied::Done)
    }

    fn settle(&self, inner: &Inner, meta: &mut SessionMeta) -> bool {
        let names = |tag: Option<&str>| tag.and_then(|t| self.name_of(inner, t));
        merge::settle(meta, &names)
    }

    fn apply_event(&self, inner: &mut Inner, host: &dyn Host, idx: &Index, body: &Body) -> Result<Applied, SyncError> {
        let eb: map::EventBody = serde_json::from_value(body.data.clone()).map_err(|e| SyncError::Format(e.to_string()))?;
        if self.skip_visit(inner, &eb.session_id) {
            return Ok(Applied::Done);
        }
        let Some((mut s, _guard)) = self.held_visit(inner, host, idx, &eb.session_id) else { return Ok(Applied::Later) };
        let Some(at) = s.meta.runs.iter().position(|r| r.id() == eb.run_id) else { return Ok(Applied::Later) };
        let vs = inner.state.visits.entry(eb.session_id.clone()).or_default();
        let mut changed = false;
        if let Some(sign) = &eb.sign {
            let run = &mut s.meta.runs[at];
            // Signing is one-way: a signed run never changes.
            if run.signed_at.is_none() {
                run.signed_at = Some(sign.at);
                run.signed_by = Some(sign.by.clone());
                run.signer = sign.signer.clone();
                changed = true;
            }
            vs.signs.insert(eb.run_id.clone());
        }
        if let Some(a) = &eb.addendum {
            let run = &mut s.meta.runs[at];
            if !run.addenda.iter().any(|x| x.id == a.id) {
                run.addenda.push(a.clone());
                run.addenda.sort_by(|x, y| x.at.cmp(&y.at).then_with(|| x.id.cmp(&y.id)));
                changed = true;
            }
            vs.addenda.insert(a.id.clone());
        }
        if changed {
            self.settle(inner, &mut s.meta);
            s.save()?;
        }
        Ok(Applied::Done)
    }

    fn apply_checks(
        &self,
        inner: &mut Inner,
        host: &dyn Host,
        idx: &Index,
        row: &RemoteRow,
        body: &Body,
    ) -> Result<Applied, SyncError> {
        let cb: map::ChecksBody = serde_json::from_value(body.data.clone()).map_err(|e| SyncError::Format(e.to_string()))?;
        if self.skip_visit(inner, &cb.session_id) {
            return Ok(Applied::Done);
        }
        let Some((mut s, _guard)) = self.held_visit(inner, host, idx, &cb.session_id) else { return Ok(Applied::Later) };
        let Some(at) = s.meta.runs.iter().position(|r| r.id() == cb.run_id) else { return Ok(Applied::Later) };
        let mut vs = inner.state.visits.get(&cb.session_id).cloned().unwrap_or_default();
        let synced = vs.checks.get(&cb.run_id).cloned();
        let remote_hlc = parse_hlc(&body.hlc).ok_or_else(|| SyncError::Format("no clock".into()))?;
        let synced_hlc = synced.as_ref().and_then(|x| parse_hlc(&x.hlc));
        let local_h = map::hash_value(&serde_json::to_value(&s.meta.runs[at].chart_checks).expect("serialises"));
        let remote_h = map::hash_value(&serde_json::to_value(&cb.checks).expect("serialises"));
        let local_changed = synced.as_ref().map_or(!s.meta.runs[at].chart_checks.is_empty(), |x| x.hash != local_h);
        let local_hlc = merge::local_stamp(&mut inner.clock, None, synced_hlc.as_ref());
        let signed = s.meta.runs[at].signed_at.is_some();
        match merge::merge_field(synced_hlc.as_ref(), local_changed && local_h != remote_h, &local_hlc, &remote_hlc) {
            FieldMerge::TakeRemote | FieldMerge::ConflictTakeRemote if !signed => {
                s.meta.runs[at].chart_checks = cb.checks.clone();
                s.save()?;
                vs.checks.insert(cb.run_id.clone(), ObjSync { version: Some(row.version), hash: remote_h, hlc: body.hlc.clone() });
            }
            FieldMerge::ConflictKeepLocal => {
                vs.checks.insert(cb.run_id.clone(), ObjSync { version: Some(row.version), hash: remote_h, hlc: body.hlc.clone() });
            }
            _ => {
                let e = vs.checks.entry(cb.run_id.clone()).or_default();
                e.version = Some(row.version);
            }
        }
        inner.state.visits.insert(cb.session_id.clone(), vs);
        Ok(Applied::Done)
    }

    fn apply_patient(&self, inner: &mut Inner, row: &RemoteRow, body: &Body) -> Result<Applied, SyncError> {
        let pb: map::PatientBody = serde_json::from_value(body.data.clone()).map_err(|e| SyncError::Format(e.to_string()))?;
        let store = PatientStore::for_sessions(&self.root);
        let merged = store.merge_in(vec![pb.record.clone()])?;
        let now = merged.iter().find(|r| r.id == pb.record.id);
        let hash = now.map(|r| map::hash_value(&serde_json::to_value(r).expect("serialises"))).unwrap_or_default();
        inner.state.patients.insert(pb.record.id.clone(), ObjSync { version: Some(row.version), hash, hlc: body.hlc.clone() });
        Ok(Applied::Done)
    }

    // ---- scan and push ----------------------------------------------------

    /// Whether a visit is in what this device syncs: visits that came from
    /// another device always; else from `since`, and with a team only the
    /// linked member's (recorded, drafted or signed by them).
    pub(crate) fn in_scope(st: &State, s: &Session) -> bool {
        if st.visits.get(&s.meta.id).is_some_and(|v| v.adopted) {
            return true;
        }
        if let Some(since) = st.since
            && s.meta.created_at.with_timezone(&Utc) < since
        {
            return false;
        }
        match &st.linked_user {
            None => true,
            Some(user) => {
                let me = |w: &Option<os_engine::users::Who>| w.as_ref().and_then(|w| w.id.as_deref()) == Some(user);
                me(&s.meta.recorded_by) || s.meta.runs.iter().any(|r| me(&r.by) || me(&r.signer))
            }
        }
    }

    fn scan(&self, inner: &mut Inner) -> Result<(), SyncError> {
        if inner.state.phase != Phase::Active {
            return Ok(());
        }
        let naming = self.account_keys(inner)?.naming()?;
        let sessions = session::list(&self.root);
        let present: BTreeSet<String> = sessions.iter().map(|s| s.meta.id.clone()).collect();
        // Restored from Deleted visits: sync it again. Deleted here: never
        // brought back by a pull (ADR-0017 section 10: "Delete from this
        // device" removes this device's copy only).
        inner.state.dropped.retain(|id| !present.contains(id));
        let gone: Vec<String> = inner.state.visits.keys().filter(|id| !present.contains(*id)).cloned().collect();
        for id in gone {
            inner.state.dropped.insert(id);
        }
        let mut patients: BTreeSet<String> = BTreeSet::new();
        for s in &sessions {
            if self.skip_visit(inner, &s.meta.id) || !Self::in_scope(&inner.state, s) {
                continue;
            }
            patients.extend(map::visit_patients(&s.meta));
            if let Err(e) = self.scan_visit(inner, s, &naming) {
                inner.state.warn(format!("A visit couldn't be prepared for sync: {e}"));
            }
        }
        let since = inner.state.since;
        let team = inner.state.linked_user.is_some();
        for rec in PatientStore::for_sessions(&self.root).list().unwrap_or_default() {
            let linked = patients.contains(&rec.id) || rec.aliases.iter().any(|a| patients.contains(a));
            let new_here = !team && since.is_none_or(|t| rec.created_at.with_timezone(&Utc) >= t);
            if linked || new_here || inner.state.patients.contains_key(&rec.id) {
                self.scan_patient(inner, &rec, &naming)?;
            }
        }
        Ok(())
    }

    fn queued(st: &State, id: &str, hash: &str) -> bool {
        st.outbox.iter().any(|o| o.id == id && o.hash == hash)
    }

    fn scan_visit(&self, inner: &mut Inner, s: &Session, naming: &SymKey) -> Result<(), SyncError> {
        let sid = s.meta.id.clone();
        let vs: VisitSync = inner.state.visits.get(&sid).cloned().unwrap_or_default();
        // The header, field by field.
        let values = map::field_values(&s.meta);
        let mut fields = vs.fields.clone();
        let mut changed = vs.header.version.is_none();
        for f in VISIT_FIELDS {
            let h = map::hash_value(&values[f]);
            if fields.get(f).is_some_and(|x| x.hash == h) {
                continue;
            }
            let synced = fields.get(f).and_then(|x| parse_hlc(&x.hlc));
            let stamp = merge::local_stamp(&mut inner.clock, map::edit_time(&s.meta, f), synced.as_ref());
            fields.insert(f.into(), FieldSync { hash: h, hlc: stamp.to_string() });
            changed = true;
        }
        let vid = keys::object_id(naming, "visit", &sid);
        let fields_hash = map::hash_value(&serde_json::to_value(&fields).expect("serialises"));
        if changed && !Self::queued(&inner.state, &vid, &fields_hash) {
            let body = map::VisitBody {
                session_id: sid.clone(),
                created_at: s.meta.created_at,
                app_version: s.meta.app_version.clone(),
                audio_on: s.meta.audio_on.as_ref().map(|a| a.device_id.clone()).unwrap_or_else(|| self.me().id.clone()),
                fields: VISIT_FIELDS
                    .iter()
                    .map(|f| (f.to_string(), Stamped { v: values[*f].clone(), t: fields[*f].hlc.clone() }))
                    .collect(),
            };
            let hlc = fields.values().filter_map(|x| parse_hlc(&x.hlc)).max().map(|h| h.to_string()).unwrap_or_default();
            self.enqueue(
                inner,
                "visit",
                &vid,
                vs.header.version,
                serde_json::to_value(&body).expect("serialises"),
                hlc,
                None,
                fields_hash.clone(),
                Commit::Header { visit: sid.clone(), fields: fields.clone(), hash: fields_hash },
            )?;
        }
        // The transcript.
        if let Some((text, h)) = transcript_hash(s)
            && h != vs.transcript.hash
        {
            let tid = keys::object_id(naming, "transcript", &sid);
            if !Self::queued(&inner.state, &tid, &h) {
                let synced = parse_hlc(&vs.transcript.hlc);
                let at = s.meta.transcription.as_ref().map(|t| t.created_at);
                let stamp = merge::local_stamp(&mut inner.clock, at, synced.as_ref()).to_string();
                let body = json!({"session_id": sid, "info": s.meta.transcription, "hash": h, "parts": []});
                self.enqueue(
                    inner,
                    "transcript",
                    &tid,
                    vs.transcript.version,
                    body,
                    stamp.clone(),
                    Some(text),
                    h.clone(),
                    Commit::Transcript { visit: sid.clone(), hash: h, hlc: stamp },
                )?;
            }
        }
        // Runs (immutable), signatures, addenda, chart confirmations.
        for run in &s.meta.runs {
            let rid = run.id().to_string();
            let local = format!("{sid}/{rid}");
            if !vs.runs.contains(&rid) {
                let oid = keys::object_id(naming, "run", &local);
                if !inner.state.outbox.iter().any(|o| o.id == oid) {
                    let (files, blob) = map::run_files(&s.dir.join(&run.dir))?;
                    let body = json!({"session_id": sid, "run": map::strip_run(run), "files": files, "parts": []});
                    let stamp = inner.clock.now().to_string();
                    self.enqueue(inner, "run", &oid, None, body, stamp, Some(blob), rid.clone(), Commit::Run {
                        visit: sid.clone(),
                        run_id: rid.clone(),
                    })?;
                }
            }
            if let (Some(at), false) = (run.signed_at, vs.signs.contains(&rid)) {
                let oid = keys::object_id(naming, "sign", &local);
                if !inner.state.outbox.iter().any(|o| o.id == oid) {
                    let ev = map::EventBody {
                        session_id: sid.clone(),
                        run_id: rid.clone(),
                        sign: Some(map::SignEvent { at, by: run.signed_by.clone().unwrap_or_default(), signer: run.signer.clone() }),
                        addendum: None,
                    };
                    let stamp = inner.clock.now().to_string();
                    self.enqueue(inner, "event", &oid, None, serde_json::to_value(&ev).expect("json"), stamp, None, rid.clone(), Commit::Sign {
                        visit: sid.clone(),
                        run_id: rid.clone(),
                    })?;
                }
            }
            for a in &run.addenda {
                if vs.addenda.contains(&a.id) {
                    continue;
                }
                let oid = keys::object_id(naming, "addendum", &format!("{sid}/{}", a.id));
                if inner.state.outbox.iter().any(|o| o.id == oid) {
                    continue;
                }
                let ev = map::EventBody { session_id: sid.clone(), run_id: rid.clone(), sign: None, addendum: Some(a.clone()) };
                let stamp = inner.clock.now().to_string();
                self.enqueue(inner, "event", &oid, None, serde_json::to_value(&ev).expect("json"), stamp, None, a.id.clone(), Commit::Addendum {
                    visit: sid.clone(),
                    id: a.id.clone(),
                })?;
            }
            if !run.chart_checks.is_empty() || vs.checks.contains_key(&rid) {
                let h = map::hash_value(&serde_json::to_value(&run.chart_checks).expect("serialises"));
                let synced = vs.checks.get(&rid).cloned().unwrap_or_default();
                if synced.hash != h {
                    let oid = keys::object_id(naming, "checks", &local);
                    if !Self::queued(&inner.state, &oid, &h) {
                        let at = run.chart_checks.iter().map(|c| c.at).max();
                        let stamp = merge::local_stamp(&mut inner.clock, at, parse_hlc(&synced.hlc).as_ref()).to_string();
                        let body = map::ChecksBody { session_id: sid.clone(), run_id: rid.clone(), checks: run.chart_checks.clone() };
                        self.enqueue(
                            inner,
                            "checks",
                            &oid,
                            synced.version,
                            serde_json::to_value(&body).expect("json"),
                            stamp.clone(),
                            None,
                            h.clone(),
                            Commit::Checks { visit: sid.clone(), run_id: rid.clone(), hash: h, hlc: stamp },
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn scan_patient(&self, inner: &mut Inner, rec: &PatientRecord, naming: &SymKey) -> Result<(), SyncError> {
        let value = serde_json::to_value(rec).expect("serialises");
        let h = map::hash_value(&value);
        let synced = inner.state.patients.get(&rec.id).cloned().unwrap_or_default();
        if synced.hash == h {
            return Ok(());
        }
        let oid = keys::object_id(naming, "patient", &rec.id);
        if Self::queued(&inner.state, &oid, &h) {
            return Ok(());
        }
        let stamp = merge::local_stamp(&mut inner.clock, Some(rec.updated_at.unwrap_or(rec.created_at)), parse_hlc(&synced.hlc).as_ref())
            .to_string();
        self.enqueue(
            inner,
            "patient",
            &oid,
            synced.version,
            json!({"record": value}),
            stamp.clone(),
            None,
            h.clone(),
            Commit::Patient { id: rec.id.clone(), hash: h, hlc: stamp },
        )
    }

    /// Seal an object (and its blob) and put it in the outbox, replacing
    /// anything queued for the same object.
    #[allow(clippy::too_many_arguments)]
    fn enqueue(
        &self,
        inner: &mut Inner,
        kind: &str,
        id: &str,
        base: Option<i64>,
        mut data: Value,
        hlc: String,
        blob: Option<Vec<u8>>,
        hash: String,
        commit: Commit,
    ) -> Result<(), SyncError> {
        let space = inner.state.space_id.clone().ok_or(SyncError::NoKeys)?;
        let kv = inner.state.key_version;
        let sk = self.account_keys(inner)?.space_key(kv)?;
        let device = self.device_keys(inner)?;
        let version = base.unwrap_or(0) + 1;
        let dek = SymKey::random()?;
        let mut blobs = Vec::new();
        if let Some(b) = blob {
            guard::refuse_audio(&b)?;
            let mut ctr = inner.state.counter;
            let sealed = seal::seal_blob(id, &space, version as u32, &b, &dek, &device, &self.me().id, &mut || {
                ctr += 1;
                ctr
            })?;
            inner.state.counter = ctr;
            data["parts"] = json!(sealed.hashes);
            blobs = sealed.parts.iter().map(|p| keys::b64(p)).collect();
        }
        inner.state.counter += 1;
        let header = Header {
            v: 1,
            id: id.to_string(),
            space: space.clone(),
            kind: kind.to_string(),
            ver: version as u32,
            dev: self.me().id.clone(),
            ctr: inner.state.counter,
        };
        let plain = serde_json::to_vec(&Body { hlc, data }).expect("serialises");
        let sealed = seal::seal_obs1(&header, &plain, &dek, &device)?;
        let padded = seal::parse_obs1(&sealed)?.ct.len();
        let wrapped = keys::wrap(&sk, &keys::dek_aad(&space, id, kv), &dek.0)?;
        inner.state.outbox.retain(|o| o.id != id);
        inner.state.outbox.push(OutItem {
            id: id.to_string(),
            kind: kind.to_string(),
            op: "put".into(),
            base_version: base,
            version,
            body: keys::b64(&sealed),
            wrapped_dek: keys::b64(&wrapped),
            key_version: kv,
            padded_size: padded,
            blobs,
            hash,
            commit,
        });
        Ok(())
    }

    fn flush(&self, inner: &mut Inner, host: &dyn Host, report: &mut SyncReport) -> Result<(), SyncError> {
        let space = inner.state.space_id.clone().ok_or(SyncError::NoKeys)?;
        let items = inner.state.outbox.clone();
        for batch in items.chunks(PUSH_BATCH) {
            let mut ready = Vec::new();
            for it in batch {
                let mut ok = true;
                for (part, body) in it.blobs.iter().enumerate() {
                    let r = self.rpc(
                        inner,
                        "put_blob",
                        json!({"space_id": space, "object_id": it.id, "object_version": it.version, "part": part, "body": body}),
                    );
                    match r {
                        Ok(_) => {}
                        Err(SyncError::Refused(c)) if c == "committed" => {
                            // Pushed before, reply lost: an immutable run is
                            // done; anything else is sealed again next run.
                            inner.state.outbox.retain(|o| o.id != it.id);
                            if it.kind == "run" {
                                self.commit(inner, &it.commit, it.version);
                            }
                            ok = false;
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
                if ok {
                    ready.push(it.clone());
                }
            }
            if ready.is_empty() {
                continue;
            }
            let payload: Vec<Value> = ready
                .iter()
                .map(|it| {
                    if it.op == "delete" {
                        json!({"op": "delete", "id": it.id})
                    } else {
                        json!({
                            "op": "put", "id": it.id, "kind": it.kind, "base_version": it.base_version,
                            "body": it.body, "wrapped_dek": it.wrapped_dek, "key_version": it.key_version,
                            "padded_size": it.padded_size,
                        })
                    }
                })
                .collect();
            let results = self.rpc(inner, "push", json!({"space_id": space, "items": payload}))?;
            for (it, res) in ready.iter().zip(results.as_array().into_iter().flatten()) {
                inner.state.outbox.retain(|o| o.id != it.id);
                match res["status"].as_str() {
                    Some("ok") | Some("exists") => {
                        let v = res["version"].as_i64().unwrap_or(it.version);
                        self.commit(inner, &it.commit, v);
                        if it.op == "put" {
                            let ctr = seal::parse_obs1(&keys::unb64(&it.body)?).map(|o| o.header.ctr).unwrap_or_default();
                            if res["status"] == "ok" {
                                inner.state.seen.insert(it.id.clone(), Seen { version: v, dev: self.me().id.clone(), ctr });
                            }
                        }
                        report.pushed += 1;
                    }
                    Some("conflict") => {
                        report.conflicts += 1;
                        if let Ok(row) = serde_json::from_value::<RemoteRow>(res["current"].clone()) {
                            let mut idx = index(&self.root);
                            match self.apply_row(inner, host, &mut idx, &row, report) {
                                Ok(Applied::Done) => {}
                                Ok(Applied::Later) => inner.state.deferred.push(Deferred { row, tries: 0 }),
                                Err(e) => inner.state.warn(format!("A conflicting object was set aside: {e}")),
                            }
                        }
                    }
                    Some("stale_key") => {
                        self.refresh_keys(inner)?;
                    }
                    Some("deleted") => {
                        if let Some(v) = commit_visit(&it.commit) {
                            inner.state.stopped.insert(v);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn commit(&self, inner: &mut Inner, c: &Commit, version: i64) {
        let st = &mut inner.state;
        match c {
            Commit::Header { visit, fields, .. } => {
                let vs = st.visits.entry(visit.clone()).or_default();
                vs.fields = fields.clone();
                vs.header.version = Some(version);
            }
            Commit::Transcript { visit, hash, hlc } => {
                st.visits.entry(visit.clone()).or_default().transcript =
                    ObjSync { version: Some(version), hash: hash.clone(), hlc: hlc.clone() };
            }
            Commit::Run { visit, run_id } => {
                st.visits.entry(visit.clone()).or_default().runs.insert(run_id.clone());
            }
            Commit::Sign { visit, run_id } => {
                st.visits.entry(visit.clone()).or_default().signs.insert(run_id.clone());
            }
            Commit::Addendum { visit, id } => {
                st.visits.entry(visit.clone()).or_default().addenda.insert(id.clone());
            }
            Commit::Checks { visit, run_id, hash, hlc } => {
                st.visits
                    .entry(visit.clone())
                    .or_default()
                    .checks
                    .insert(run_id.clone(), ObjSync { version: Some(version), hash: hash.clone(), hlc: hlc.clone() });
            }
            Commit::Patient { id, hash, hlc } => {
                st.patients.insert(id.clone(), ObjSync { version: Some(version), hash: hash.clone(), hlc: hlc.clone() });
            }
            Commit::Tombstone { visit } => {
                st.stopped.insert(visit.clone());
                state::audit(&self.paths, "visit_stopped", json!({"visit": visit}));
            }
            Commit::Delete => {}
        }
    }

    /// Stop syncing one visit: a tombstone, and the cloud copies deleted.
    /// Every local copy stays (ADR-0017 section 10).
    pub fn stop_visit(&self, session_id: &str, host: &dyn Host) -> Result<(), SyncError> {
        let mut inner = self.lock();
        if inner.state.phase != Phase::Active {
            return Err(SyncError::User("Sync isn't on.".into()));
        }
        let naming = self.account_keys(&inner)?.naming()?;
        let vs = inner.state.visits.get(session_id).cloned().unwrap_or_default();
        let mut ids = vec![
            keys::object_id(&naming, "visit", session_id),
            keys::object_id(&naming, "transcript", session_id),
        ];
        for r in &vs.runs {
            ids.push(keys::object_id(&naming, "run", &format!("{session_id}/{r}")));
            ids.push(keys::object_id(&naming, "checks", &format!("{session_id}/{r}")));
        }
        for r in &vs.signs {
            ids.push(keys::object_id(&naming, "sign", &format!("{session_id}/{r}")));
        }
        for a in &vs.addenda {
            ids.push(keys::object_id(&naming, "addendum", &format!("{session_id}/{a}")));
        }
        inner.state.outbox.retain(|o| !ids.contains(&o.id) && commit_visit(&o.commit).as_deref() != Some(session_id));
        let tid = keys::object_id(&naming, "tombstone", session_id);
        let stamp = inner.clock.now().to_string();
        self.enqueue(
            &mut inner,
            "tombstone",
            &tid,
            None,
            json!({"session_id": session_id, "objects": ids}),
            stamp,
            None,
            session_id.to_string(),
            Commit::Tombstone { visit: session_id.to_string() },
        )?;
        for id in &ids {
            inner.state.outbox.push(OutItem {
                id: id.clone(),
                kind: String::new(),
                op: "delete".into(),
                base_version: None,
                version: 0,
                body: String::new(),
                wrapped_dek: String::new(),
                key_version: 0,
                padded_size: 0,
                blobs: Vec::new(),
                hash: String::new(),
                commit: Commit::Delete,
            });
        }
        let mut report = SyncReport::default();
        let r = self.flush(&mut inner, host, &mut report);
        let saved = self.persist(&mut inner);
        self.publish(&inner, false);
        r?;
        saved
    }

    /// Each known visit's sync state, by visit id: synced, pending, stopped.
    pub fn visit_states(&self) -> BTreeMap<String, String> {
        let inner = match self.inner.try_lock() {
            Ok(i) => i,
            Err(_) => return self.snapshot_states(),
        };
        let st = &inner.state;
        let mut out = BTreeMap::new();
        if st.phase != Phase::Active {
            return out;
        }
        let waiting: BTreeSet<String> = st.outbox.iter().filter_map(|o| commit_visit(&o.commit)).collect();
        for (id, vs) in &st.visits {
            let s = if st.stopped.contains(id) {
                "stopped"
            } else if waiting.contains(id) {
                "pending"
            } else if vs.header.version.is_some() {
                "synced"
            } else {
                "pending"
            };
            out.insert(id.clone(), s.to_string());
        }
        for id in &st.stopped {
            out.insert(id.clone(), "stopped".into());
        }
        *self.states_cache.lock().expect("cache") = out.clone();
        out
    }

    fn snapshot_states(&self) -> BTreeMap<String, String> {
        self.states_cache.lock().expect("cache").clone()
    }

    /// Whether this visit would sync (for the visit list, before its first push).
    pub fn would_sync(&self, s: &Session) -> bool {
        match self.inner.try_lock() {
            Ok(inner) => inner.state.phase == Phase::Active && Self::in_scope(&inner.state, s),
            Err(_) => false,
        }
    }
}

/// The server refused this device, or its sign-in ended: maybe revoked.
fn locked_out(e: &SyncError) -> bool {
    matches!(e, SyncError::SignedOut) || matches!(e, SyncError::Refused(c) if c == "not_approved")
}

fn commit_visit(c: &Commit) -> Option<String> {
    match c {
        Commit::Header { visit, .. }
        | Commit::Transcript { visit, .. }
        | Commit::Run { visit, .. }
        | Commit::Sign { visit, .. }
        | Commit::Addendum { visit, .. }
        | Commit::Checks { visit, .. }
        | Commit::Tombstone { visit } => Some(visit.clone()),
        Commit::Patient { .. } | Commit::Delete => None,
    }
}
