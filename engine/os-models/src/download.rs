//! Resumable, verified downloads. The engine's only network code path for
//! model files (Hub browsing is in [`crate::hub`]).

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{RANGE, USER_AGENT};

use crate::catalog::{CatalogEntry, Source};
use crate::store::{ModelStore, Verify, hash_file, hashes_match, part_path, safe_rel_path};

/// The only host a Hugging Face token is ever sent to.
const HF_ORIGIN: &str = "https://huggingface.co/";

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("{0} has no download URL")]
    NoUrl(String),
    #[error("not enough disk space: need {need} bytes, {available} available")]
    DiskSpace { need: u64, available: u64 },
    #[error("HTTP {status} for {url}")]
    Http { status: u16, url: String },
    #[error("size mismatch for {file}: expected {expected}, got {actual}")]
    Size { file: String, expected: u64, actual: u64 },
    #[error("checksum mismatch for {file} (download deleted)")]
    Checksum { file: String },
    /// 401 or 403 with `X-Error-Code: GatedRepo`.
    #[error("This model is gated on Hugging Face. Request access on its page, then add your token.")]
    Gated { repo: String },
    /// 404, or 401 without `GatedRepo` (the Hub's answer for a private repo).
    #[error("Hugging Face couldn't find {url}. It may have been removed or made private.")]
    NotFound { url: String },
    #[error("{0} was added from this computer, so it can't be downloaded.")]
    Local(String),
    #[error(transparent)]
    UnsafePath(#[from] crate::store::UnsafePath),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Progress callback: (file name, bytes done, bytes total).
pub trait Progress {
    fn update(&mut self, file: &str, done: u64, total: u64);
}

impl<F: FnMut(&str, u64, u64)> Progress for F {
    fn update(&mut self, file: &str, done: u64, total: u64) {
        self(file, done, total)
    }
}

/// The token to send with one request: only for a gated entry, and only to
/// `https://huggingface.co/` (ADR-0025 §8). The Hub then redirects file
/// downloads to its storage hosts (`cas-bridge.xethub.hf.co`,
/// `us.aws.cdn.hf.co`, ...); the token never follows, because reqwest
/// 0.13.5 strips `Authorization` (and cookies) whenever a redirect changes
/// host, port or scheme: `src/redirect.rs`, `remove_sensitive_headers`,
/// called for every redirect it follows.
fn token_for<'a>(entry: &CatalogEntry, url: &str, token: Option<&'a str>) -> Option<&'a str> {
    token.filter(|t| entry.gated && !t.is_empty() && url.starts_with(HF_ORIGIN))
}

/// Download every missing file of `entry` into `store`, resuming partial
/// downloads, then verify size and hash (SHA-256, or git blob SHA-1 for small
/// Hub files). Already-verified files are skipped.
pub fn download(
    store: &ModelStore,
    entry: &CatalogEntry,
    token: Option<&str>,
    progress: &mut dyn Progress,
) -> Result<(), DownloadError> {
    if entry.is_local() {
        return Err(DownloadError::Local(entry.display_name.clone()));
    }
    // It came with the app (ADR-0018 section 10): nothing to fetch, and its
    // folder is read-only.
    if store.is_bundled(entry) {
        return Ok(());
    }
    // Every path is checked before anything is written.
    for file in &entry.files {
        safe_rel_path(&file.path)?;
    }
    let dir = store.model_dir(entry);
    fs::create_dir_all(&dir)?;

    let missing: Vec<_> = entry.files.iter().filter(|f| !store.file_ok(entry, f, Verify::Size)).collect();
    let need: u64 = missing
        .iter()
        .map(|f| {
            let part =
                fs::metadata(part_path(&store.file_path(entry, &f.path))).map(|m| m.len()).unwrap_or(0);
            f.size.saturating_sub(part)
        })
        .sum();
    if let Some(available) = available_space(&dir) {
        // Keep 1 GiB headroom for the OS and the encrypted store.
        if need + (1 << 30) > available {
            return Err(DownloadError::DiskSpace { need, available });
        }
    }

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(None)
        .build()?;

    for file in &entry.files {
        let final_path = store.file_path(entry, &file.path);
        if store.file_ok(entry, file, Verify::Size) {
            // Size matches: confirm the hash once, re-download if it is wrong.
            if store.file_ok(entry, file, Verify::Full) {
                progress.update(&file.path, file.size, file.size);
                continue;
            }
            fs::remove_file(&final_path)?;
        }
        let url = entry.url_for(file).ok_or_else(|| DownloadError::NoUrl(file.path.clone()))?;
        let part = part_path(&final_path);
        if let Some(parent) = part.parent() {
            fs::create_dir_all(parent)?;
        }
        fetch_resumable(
            &client,
            entry,
            &url,
            token_for(entry, &url, token),
            &part,
            file.size,
            &file.path,
            progress,
        )?;

        let actual = fs::metadata(&part)?.len();
        if actual != file.size {
            // Never resumed again: the next try starts this file afresh.
            fs::remove_file(&part)?;
            return Err(DownloadError::Size { file: file.path.clone(), expected: file.size, actual });
        }
        let hashes = hash_file(&part)?;
        if !hashes_match(file, &hashes) {
            fs::remove_file(&part)?;
            return Err(DownloadError::Checksum { file: file.path.clone() });
        }
        fs::rename(&part, &final_path)?;
        tracing::info!(file = %file.path, "downloaded and verified");
    }
    Ok(())
}

/// A failed response, in the words the user sees.
fn status_error(
    entry: &CatalogEntry,
    url: &str,
    status: StatusCode,
    error_code: Option<&str>,
) -> DownloadError {
    let gated = error_code.is_some_and(|c| c.eq_ignore_ascii_case("GatedRepo"));
    match status.as_u16() {
        401 | 403 if gated => DownloadError::Gated {
            repo: match &entry.source {
                Source::Huggingface { repo, .. } => repo.clone(),
                _ => entry.id.clone(),
            },
        },
        401 | 404 => DownloadError::NotFound { url: url.to_string() },
        s => DownloadError::Http { status: s, url: url.to_string() },
    }
}

#[allow(clippy::too_many_arguments)]
fn fetch_resumable(
    client: &reqwest::blocking::Client,
    entry: &CatalogEntry,
    url: &str,
    token: Option<&str>,
    part_path: &Path,
    total: u64,
    name: &str,
    progress: &mut dyn Progress,
) -> Result<(), DownloadError> {
    let mut have = fs::metadata(part_path).map(|m| m.len()).unwrap_or(0);
    if have > total {
        // Longer than the file: left from another revision. Start again.
        fs::remove_file(part_path)?;
        have = 0;
    }
    if have == total {
        return Ok(());
    }
    let mut request = client.get(url).header(USER_AGENT, concat!("OpenBark/", env!("CARGO_PKG_VERSION")));
    if have > 0 {
        request = request.header(RANGE, format!("bytes={have}-"));
    }
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let mut response = request.send()?;
    let status = response.status();
    let append = match status {
        StatusCode::PARTIAL_CONTENT => true,
        StatusCode::OK => {
            have = 0; // Server ignored the range; start over.
            false
        }
        StatusCode::RANGE_NOT_SATISFIABLE => return Ok(()),
        s => {
            let code = response.headers().get("x-error-code").and_then(|v| v.to_str().ok());
            return Err(status_error(entry, url, s, code));
        }
    };
    let mut out =
        OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(part_path)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = response.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        have += n as u64;
        progress.update(name, have, total);
    }
    out.flush()?;
    Ok(())
}

fn available_space(path: &Path) -> Option<u64> {
    let path = fs::canonicalize(path).ok()?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{FileSpec, Layout, Slot};
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// A tiny HTTP/1.1 server on 127.0.0.1 (no internet in unit tests).
    /// `/files/<name>` serves `files[name]` (honouring `Range`), `/gated/...`
    /// answers 401 with `X-Error-Code: GatedRepo`, `/private/...` a bare 401,
    /// anything else 404. Every request's headers are recorded.
    struct Stub {
        base: String,
        requests: Arc<Mutex<Vec<String>>>,
    }

    fn stub(files: Vec<(&'static str, Vec<u8>)>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let _ = serve(stream, &files, &log);
            }
        });
        Stub { base, requests }
    }

    fn serve(
        mut stream: TcpStream,
        files: &[(&str, Vec<u8>)],
        log: &Mutex<Vec<String>>,
    ) -> std::io::Result<()> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 || line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        log.lock().unwrap().push(head.clone());
        let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
        let range = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("range: bytes=")
                    .map(|r| r.trim_end_matches('-').to_string())
            })
            .and_then(|r| r.trim().parse::<usize>().ok());
        let reply = |status: &str, extra: &str, body: &[u8]| -> Vec<u8> {
            let mut out = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
                body.len()
            )
            .into_bytes();
            out.extend_from_slice(body);
            out
        };
        let bytes = if let Some(name) = path.strip_prefix("/files/") {
            match files.iter().find(|(n, _)| *n == name) {
                Some((_, body)) => match range {
                    Some(from) if from < body.len() => reply(
                        "206 Partial Content",
                        &format!("Content-Range: bytes {from}-{}/{}\r\n", body.len() - 1, body.len()),
                        &body[from..],
                    ),
                    _ => reply("200 OK", "", body),
                },
                None => reply("404 Not Found", "X-Error-Code: EntryNotFound\r\n", b""),
            }
        } else if path.starts_with("/gated/") {
            reply("401 Unauthorized", "X-Error-Code: GatedRepo\r\n", b"")
        } else if path.starts_with("/private/") {
            reply("401 Unauthorized", "", b"")
        } else {
            reply("404 Not Found", "", b"")
        };
        stream.write_all(&bytes)?;
        stream.flush()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(bytes))
    }

    fn url_entry(base: &str, files: &[(&str, &str, &[u8])]) -> CatalogEntry {
        CatalogEntry {
            id: "hf:someone/repo/model".into(),
            display_name: "Some model".into(),
            slot: Slot::Llm,
            license: "mit".into(),
            attribution: None,
            source: Source::Url,
            files: files
                .iter()
                .map(|(path, url_path, body)| FileSpec {
                    path: (*path).into(),
                    size: body.len() as u64,
                    sha256: sha256_hex(body),
                    git_sha1: None,
                    url: Some(format!("{base}{url_path}")),
                })
                .collect(),
            layout: Layout::LlmGguf { model: files[0].0.into() },
            gated: false,
            mobile: false,
        }
    }

    fn no_progress() -> impl FnMut(&str, u64, u64) {
        |_: &str, _: u64, _: u64| {}
    }

    #[test]
    fn nested_parts_download_into_their_folders() {
        let a = b"GGUF part one".to_vec();
        let b = b"GGUF part two, longer".to_vec();
        let s = stub(vec![("a", a.clone()), ("b", b.clone())]);
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let entry = url_entry(
            &s.base,
            &[
                ("Q4_K_M/m-00001-of-00002.gguf", "/files/a", &a),
                ("Q4_K_M/m-00002-of-00002.gguf", "/files/b", &b),
            ],
        );
        download(&store, &entry, None, &mut no_progress()).unwrap();
        let dir = tmp.path().join("hf_someone/repo/model/Q4_K_M");
        assert_eq!(fs::read(dir.join("m-00001-of-00002.gguf")).unwrap(), a);
        assert_eq!(fs::read(dir.join("m-00002-of-00002.gguf")).unwrap(), b);
        assert!(!dir.join("m-00001-of-00002.gguf.part").exists());
        assert!(store.is_installed(&entry, Verify::Full));
    }

    #[test]
    fn a_partial_download_resumes() {
        let body: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let s = stub(vec![("m", body.clone())]);
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let entry = url_entry(&s.base, &[("m.gguf", "/files/m", &body)]);
        let part = part_path(&store.file_path(&entry, "m.gguf"));
        fs::create_dir_all(part.parent().unwrap()).unwrap();
        fs::write(&part, &body[..1234]).unwrap();
        download(&store, &entry, None, &mut no_progress()).unwrap();
        assert_eq!(fs::read(store.file_path(&entry, "m.gguf")).unwrap(), body);
        assert!(
            s.requests.lock().unwrap().iter().any(|r| r.to_ascii_lowercase().contains("range: bytes=1234-"))
        );
    }

    /// A `.part` longer than the file (left by another revision) used to fail
    /// every retry with a size error. It is thrown away and fetched afresh.
    #[test]
    fn a_partial_download_longer_than_the_file_starts_again() {
        let body = b"GGUF the current revision".to_vec();
        let s = stub(vec![("m", body.clone())]);
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let entry = url_entry(&s.base, &[("m.gguf", "/files/m", &body)]);
        let part = part_path(&store.file_path(&entry, "m.gguf"));
        fs::create_dir_all(part.parent().unwrap()).unwrap();
        fs::write(&part, vec![9u8; body.len() + 100]).unwrap();
        download(&store, &entry, None, &mut no_progress()).unwrap();
        assert_eq!(fs::read(store.file_path(&entry, "m.gguf")).unwrap(), body);
        assert!(!part.exists());
        let requests = s.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].to_ascii_lowercase().contains("range:"), "fetched from the start");
    }

    #[test]
    fn small_files_verify_by_git_sha1() {
        let s = stub(vec![("c", b"abc".to_vec())]);
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let mut entry = url_entry(&s.base, &[("config.json", "/files/c", b"abc")]);
        entry.files[0].sha256 = String::new();
        entry.files[0].git_sha1 = Some("f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f".into());
        download(&store, &entry, None, &mut no_progress()).unwrap();
        assert!(store.is_installed(&entry, Verify::Full));

        // A wrong git SHA-1 deletes the download.
        let tmp2 = tempfile::tempdir().unwrap();
        let store2 = ModelStore::new(tmp2.path());
        entry.files[0].git_sha1 = Some("0".repeat(40));
        let err = download(&store2, &entry, None, &mut no_progress()).unwrap_err();
        assert!(matches!(err, DownloadError::Checksum { .. }), "{err}");
        assert!(!part_path(&store2.file_path(&entry, "config.json")).exists());
    }

    #[test]
    fn unsafe_paths_are_refused_before_anything_is_written() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path().join("store"));
        for bad in ["../escape.gguf", "/abs.gguf", "a\\b.gguf", "C:x.gguf", "a//b.gguf"] {
            let entry = url_entry("http://127.0.0.1:9", &[(bad, "/files/x", b"x")]);
            let err = download(&store, &entry, None, &mut no_progress()).unwrap_err();
            assert!(matches!(err, DownloadError::UnsafePath(_)), "{bad}: {err}");
        }
        assert!(!tmp.path().join("store").exists(), "nothing was created");
        assert!(!tmp.path().join("escape.gguf").exists());
    }

    #[test]
    fn local_models_cannot_be_downloaded() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let mut entry = url_entry("http://127.0.0.1:9", &[("m.gguf", "/files/m", b"x")]);
        entry.source = Source::Local { path: None };
        let err = download(&store, &entry, None, &mut no_progress()).unwrap_err();
        assert!(matches!(err, DownloadError::Local(_)));
        assert_eq!(err.to_string(), "Some model was added from this computer, so it can't be downloaded.");
    }

    #[test]
    fn a_bundled_model_is_never_downloaded_again() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundle");
        // Port 9 (discard): any request would fail.
        let entry = url_entry("http://127.0.0.1:9", &[("vad.onnx", "/files/vad", b"weights")]);
        let dir = bundled.join(crate::dir_name(&entry.id));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("vad.onnx"), b"weights").unwrap();
        let store = ModelStore::new(tmp.path().join("store")).with_bundled(Some(bundled));
        download(&store, &entry, None, &mut no_progress()).unwrap();
        assert!(!tmp.path().join("store").exists(), "nothing is written to the store");
        // And it can't be removed.
        let err = crate::remove::remove_files(&store, &entry).unwrap_err();
        assert_eq!(err.to_string(), "Some model comes with the app, so it can't be removed.");
        assert!(dir.join("vad.onnx").is_file());
    }

    #[test]
    fn gated_and_missing_answers_become_plain_errors() {
        let s = stub(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let mut entry = url_entry(&s.base, &[("m.gguf", "/gated/m.gguf", b"x")]);
        entry.source = Source::Url;
        let err = download(&store, &entry, None, &mut no_progress()).unwrap_err();
        assert!(matches!(err, DownloadError::Gated { .. }), "{err}");
        assert_eq!(
            err.to_string(),
            "This model is gated on Hugging Face. Request access on its page, then add your token."
        );

        let entry = url_entry(&s.base, &[("m.gguf", "/private/m.gguf", b"x")]);
        let err = download(&store, &entry, None, &mut no_progress()).unwrap_err();
        assert!(matches!(err, DownloadError::NotFound { .. }), "{err}");

        let entry = url_entry(&s.base, &[("m.gguf", "/files/none", b"x")]);
        let err = download(&store, &entry, None, &mut no_progress()).unwrap_err();
        assert!(matches!(err, DownloadError::NotFound { .. }), "{err}");
        assert!(err.to_string().starts_with("Hugging Face couldn't find http://127.0.0.1:"));
    }

    #[test]
    fn gated_errors_name_the_repo() {
        let mut entry = url_entry("http://x", &[("m.gguf", "/m", b"x")]);
        entry.source = Source::Huggingface {
            repo: "google/gemma-3-1b-it-qat-q4_0-gguf".into(),
            revision: "0".repeat(40),
        };
        match status_error(&entry, "u", StatusCode::FORBIDDEN, Some("GatedRepo")) {
            DownloadError::Gated { repo } => assert_eq!(repo, "google/gemma-3-1b-it-qat-q4_0-gguf"),
            e => panic!("{e}"),
        }
        assert!(matches!(
            status_error(&entry, "u", StatusCode::FORBIDDEN, None),
            DownloadError::Http { status: 403, .. }
        ));
        assert!(matches!(
            status_error(&entry, "u", StatusCode::NOT_FOUND, None),
            DownloadError::NotFound { .. }
        ));
        assert!(matches!(
            status_error(&entry, "u", StatusCode::UNAUTHORIZED, None),
            DownloadError::NotFound { .. }
        ));
        assert!(matches!(
            status_error(&entry, "u", StatusCode::INTERNAL_SERVER_ERROR, None),
            DownloadError::Http { status: 500, .. }
        ));
    }

    /// The status line and `X-Error-Code` of a saved live `resolve` answer.
    fn live_answer(name: &str) -> (StatusCode, Option<String>) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hub").join(name);
        let text = String::from_utf8(fs::read(&path).unwrap()).unwrap();
        let status = text.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).unwrap();
        let code = text.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("x-error-code").then(|| v.trim().to_string())
        });
        (StatusCode::from_u16(status).unwrap(), code)
    }

    /// What the Hub really answered on 2026-10-19: a gated file without a
    /// token, and a file that isn't there.
    #[test]
    fn live_hub_answers_map_to_plain_errors() {
        let mut entry = url_entry("http://x", &[("m.gguf", "/m", b"x")]);
        entry.source = Source::Huggingface {
            repo: "google/gemma-3-1b-it-qat-q4_0-gguf".into(),
            revision: "0".repeat(40),
        };
        let (status, code) = live_answer("hdr_resolve_gated.txt");
        assert_eq!((status.as_u16(), code.as_deref()), (401, Some("GatedRepo")));
        assert!(matches!(status_error(&entry, "u", status, code.as_deref()), DownloadError::Gated { .. }));
        let (status, code) = live_answer("hdr_resolve_missing.txt");
        assert_eq!((status.as_u16(), code.as_deref()), (404, Some("EntryNotFound")));
        assert!(matches!(status_error(&entry, "u", status, code.as_deref()), DownloadError::NotFound { .. }));
    }

    #[test]
    fn the_token_goes_only_to_hugging_face_for_gated_entries() {
        let mut entry = url_entry("http://x", &[("m.gguf", "/m", b"x")]);
        let hf = "https://huggingface.co/google/gemma/resolve/abc/m.gguf";
        assert_eq!(token_for(&entry, hf, Some("hf_secret")), None, "not gated: no token");
        entry.gated = true;
        assert_eq!(token_for(&entry, hf, Some("hf_secret")), Some("hf_secret"));
        assert_eq!(token_for(&entry, hf, None), None);
        for elsewhere in [
            "http://huggingface.co/x",
            "https://huggingface.co.evil.example/x",
            "https://cas-bridge.xethub.hf.co/x",
            "https://github.com/x",
            "https://huggingface.co:444/x",
        ] {
            assert_eq!(token_for(&entry, elsewhere, Some("hf_secret")), None, "{elsewhere}");
        }
    }

    #[test]
    fn no_token_reaches_another_host_even_for_a_gated_entry() {
        let body = b"weights".to_vec();
        let s = stub(vec![("m", body.clone())]);
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let mut entry = url_entry(&s.base, &[("m.gguf", "/files/m", &body)]);
        entry.gated = true;
        download(&store, &entry, Some("hf_secret_token"), &mut no_progress()).unwrap();
        let log = s.requests.lock().unwrap();
        assert!(!log.is_empty());
        for r in log.iter() {
            assert!(!r.to_ascii_lowercase().contains("authorization"), "{r}");
            assert!(!r.contains("hf_secret_token"));
        }
    }
}
