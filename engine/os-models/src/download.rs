//! Resumable, verified downloads. The engine's only network code path.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{RANGE, USER_AGENT};

use crate::catalog::CatalogEntry;
use crate::store::{ModelStore, Verify, sha256_file};

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

/// Download every missing file of `entry` into `store`, resuming partial
/// downloads, then verify size and SHA-256. Already-verified files are skipped.
pub fn download(
    store: &ModelStore,
    entry: &CatalogEntry,
    token: Option<&str>,
    progress: &mut dyn Progress,
) -> Result<(), DownloadError> {
    let dir = store.model_dir(entry);
    fs::create_dir_all(&dir)?;

    let missing: Vec<_> = entry.files.iter().filter(|f| !store.file_ok(entry, f, Verify::Size)).collect();
    let need: u64 = missing
        .iter()
        .map(|f| {
            let part = fs::metadata(dir.join(format!("{}.part", f.path))).map(|m| m.len()).unwrap_or(0);
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
        let part_path = dir.join(format!("{}.part", file.path));
        fetch_resumable(&client, &url, token, &part_path, file.size, &file.path, progress)?;

        let actual = fs::metadata(&part_path)?.len();
        if actual != file.size {
            return Err(DownloadError::Size { file: file.path.clone(), expected: file.size, actual });
        }
        let hash = sha256_file(&part_path)?;
        if !hash.eq_ignore_ascii_case(&file.sha256) {
            fs::remove_file(&part_path)?;
            return Err(DownloadError::Checksum { file: file.path.clone() });
        }
        fs::rename(&part_path, &final_path)?;
        tracing::info!(file = %file.path, "downloaded and verified");
    }
    Ok(())
}

fn fetch_resumable(
    client: &reqwest::blocking::Client,
    url: &str,
    token: Option<&str>,
    part_path: &Path,
    total: u64,
    name: &str,
    progress: &mut dyn Progress,
) -> Result<(), DownloadError> {
    let mut have = fs::metadata(part_path).map(|m| m.len()).unwrap_or(0);
    if have >= total {
        return Ok(());
    }
    let mut request = client.get(url).header(USER_AGENT, concat!("OpenBark/", env!("CARGO_PKG_VERSION")));
    if have > 0 {
        request = request.header(RANGE, format!("bytes={have}-"));
    }
    // Only Hugging Face needs a token (gated repos); never send it elsewhere.
    if let Some(token) = token.filter(|_| url.starts_with("https://huggingface.co/")) {
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
        s => return Err(DownloadError::Http { status: s.as_u16(), url: url.to_string() }),
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
