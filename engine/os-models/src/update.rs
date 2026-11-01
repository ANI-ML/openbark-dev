//! The version check (ADR-0035, docs/network.md R16): read the newest
//! published version number from the website's `release.json`. It downloads
//! and installs nothing. os-app decides when it runs: only once the person
//! has turned the daily check on, or when they press **Check for updates**.
//!
//! The request is a plain GET of one fixed address with
//! `User-Agent: OpenBark/<version>`: no query, no cookies (reqwest is built
//! without a cookie store), no token, no id. reqwest adds the standard
//! `Accept: */*`, and HTTP itself needs `Host`. One redirect is followed, and
//! only to the same file on `www.openbark.io` (the bare domain redirects
//! there).

use std::io::Read;
use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::{Client, Request};
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};

/// The one address the check reads.
pub const RELEASE_URL: &str = "https://www.openbark.io/release.json";
/// What **Download** opens: the website's download section, which offers the
/// installer for the computer it's opened on. Nothing is downloaded by the app.
pub const DOWNLOAD_PAGE: &str = "https://www.openbark.io/#download";
/// What **What's new** opens when `release.json` names no acceptable page.
pub const NOTES_FALLBACK: &str = "https://www.openbark.io/history";
/// The `User-Agent` header, the only header the check sets.
pub const AGENT: &str = concat!("OpenBark/", env!("CARGO_PKG_VERSION"));

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TIMEOUT: Duration = Duration::from_secs(10);
/// `release.json` is about 1 KB; anything much larger isn't it.
const BODY_CAP: u64 = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("couldn't reach openbark.io: {0}")]
    Network(String),
    #[error("openbark.io answered HTTP {0}")]
    Http(u16),
    #[error("openbark.io sent something that isn't the release file")]
    Invalid,
}

/// A version number, `MAJOR.MINOR.PATCH` (a leading `v` is allowed).
/// Compared number by number, so 0.2.10 is newer than 0.2.9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// `None` for anything else, pre-release tags included ("0.3.0-beta.1"):
    /// such a release is never announced.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let version = Self {
            major: number(parts.next()?)?,
            minor: number(parts.next()?)?,
            patch: number(parts.next()?)?,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn number(part: &str) -> Option<u64> {
    if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

/// `latest` is a newer version than `current`. False when either isn't a
/// version number.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (Version::parse(latest), Version::parse(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// The newest published release, as far as the check needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// Written back from the parsed numbers ("0.2.10"), never the raw text.
    pub version: String,
    /// The release notes page, checked by [`safe_notes`].
    pub notes: String,
}

/// Read `release.json`. `Ok(None)` when it doesn't announce a release: its
/// `published` isn't `true`, or its version isn't a version number.
pub fn parse_release(body: &[u8]) -> Result<Option<Release>, UpdateError> {
    let json: serde_json::Value = serde_json::from_slice(body).map_err(|_| UpdateError::Invalid)?;
    if !json.is_object() {
        return Err(UpdateError::Invalid);
    }
    if json.get("published") != Some(&serde_json::Value::Bool(true)) {
        return Ok(None);
    }
    let Some(version) = json.get("version").and_then(|v| v.as_str()).and_then(Version::parse) else {
        return Ok(None);
    };
    let notes = safe_notes(json.get("notes").and_then(|v| v.as_str()).unwrap_or(""));
    Ok(Some(Release { version: version.to_string(), notes }))
}

/// The notes page to open: an https page on www.openbark.io or in the
/// public releases repository on GitHub, else [`NOTES_FALLBACK`].
pub fn safe_notes(url: &str) -> String {
    match Url::parse(url) {
        Ok(u) if allowed_link(&u) => u.to_string(),
        _ => NOTES_FALLBACK.to_string(),
    }
}

fn allowed_link(u: &Url) -> bool {
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() || u.port().is_some() {
        return false;
    }
    match u.host_str() {
        Some("www.openbark.io") => true,
        Some("github.com") => u.path().starts_with("/ANI-ML/openbark-releases/"),
        _ => false,
    }
}

/// A redirect is followed only once, and only to [`RELEASE_URL`] itself
/// (`openbark.io` redirects to `www.openbark.io`). `previous` starts with the
/// address first asked for, as reqwest passes it.
pub fn redirect_allowed(previous: &[Url], next: &Url) -> bool {
    previous.len() == 1 && next.as_str() == RELEASE_URL
}

/// The client: short timeouts, the redirect rule above, no cookie store.
pub fn client() -> Result<Client, UpdateError> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if redirect_allowed(attempt.previous(), attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|e| UpdateError::Network(e.to_string()))
}

/// The request: a GET of `url` with `User-Agent: OpenBark/<version>`, the
/// only header OpenBark sets.
pub fn request(client: &Client, url: &str) -> Result<Request, UpdateError> {
    client.get(url).header(USER_AGENT, AGENT).build().map_err(|e| UpdateError::Network(e.to_string()))
}

/// Ask openbark.io for the newest release.
pub fn fetch() -> Result<Option<Release>, UpdateError> {
    fetch_from(RELEASE_URL)
}

fn fetch_from(url: &str) -> Result<Option<Release>, UpdateError> {
    let client = client()?;
    let response = client.execute(request(&client, url)?).map_err(|e| UpdateError::Network(e.to_string()))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(UpdateError::Http(status));
    }
    let mut body = Vec::new();
    response.take(BODY_CAP + 1).read_to_end(&mut body).map_err(|e| UpdateError::Network(e.to_string()))?;
    if body.len() as u64 > BODY_CAP {
        return Err(UpdateError::Invalid);
    }
    parse_release(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::mpsc;

    #[test]
    fn versions_compare_number_by_number() {
        assert!(is_newer("0.2.10", "0.2.9"));
        assert!(!is_newer("0.2.9", "0.2.10"));
        assert!(is_newer("0.3.0", "0.2.99"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("v0.2.5", "0.2.4"));
        assert!(is_newer("0.2.5", " 0.2.4 "));
    }

    #[test]
    fn an_equal_or_older_version_is_not_newer() {
        assert!(!is_newer("0.2.4", "0.2.4"));
        assert!(!is_newer("v0.2.4", "0.2.4"));
        assert!(!is_newer("0.2.3", "0.2.4"));
        assert!(!is_newer("0.1.99", "0.2.0"));
    }

    #[test]
    fn anything_that_isnt_a_version_number_is_never_newer() {
        for text in
            ["", "0.3", "0.3.0.1", "0.3.0-beta.1", "1.x.0", "-1.0.0", "1..0", "١.٠.٠", "9999999999.0.0"]
        {
            assert!(!is_newer(text, "0.2.4"), "{text:?}");
        }
        assert!(!is_newer("0.3.0", "not a version"));
        assert_eq!(Version::parse("0.2.10").map(|v| v.to_string()).as_deref(), Some("0.2.10"));
        assert_eq!(Version::parse("v1.02.3").map(|v| v.to_string()).as_deref(), Some("1.2.3"));
    }

    const PUBLISHED: &str = r#"{"product":"OpenBark","version":"0.2.10","published":true,
        "notes":"https://github.com/ANI-ML/openbark-releases/releases/tag/v0.2.10","files":{}}"#;

    #[test]
    fn a_published_release_is_read() {
        let release = parse_release(PUBLISHED.as_bytes()).unwrap().unwrap();
        assert_eq!(release.version, "0.2.10");
        assert_eq!(release.notes, "https://github.com/ANI-ML/openbark-releases/releases/tag/v0.2.10");
    }

    #[test]
    fn an_unpublished_release_is_ignored() {
        for body in [
            r#"{"version":"0.3.0","published":false}"#,
            r#"{"version":"0.3.0"}"#,
            r#"{"version":"0.3.0","published":"true"}"#,
            r#"{"version":"0.3.0","published":1}"#,
            r#"{"version":"0.3.0","published":null}"#,
        ] {
            assert_eq!(parse_release(body.as_bytes()).unwrap(), None, "{body}");
        }
    }

    #[test]
    fn a_release_without_a_version_number_is_ignored() {
        for body in [
            r#"{"published":true}"#,
            r#"{"version":"0.3.0-rc.1","published":true}"#,
            r#"{"version":3,"published":true}"#,
            r#"{"version":"<b>9.9.9</b>","published":true}"#,
        ] {
            assert_eq!(parse_release(body.as_bytes()).unwrap(), None, "{body}");
        }
    }

    #[test]
    fn something_other_than_the_release_file_is_an_error() {
        for body in ["<html>Not found</html>", "[1,2,3]", "\"0.3.0\"", ""] {
            assert!(matches!(parse_release(body.as_bytes()), Err(UpdateError::Invalid)), "{body:?}");
        }
    }

    #[test]
    fn only_trusted_notes_pages_are_kept() {
        let ok = [
            "https://github.com/ANI-ML/openbark-releases/releases/tag/v0.3.0",
            "https://www.openbark.io/history",
        ];
        for url in ok {
            assert_eq!(safe_notes(url), url);
        }
        for url in [
            "",
            "http://github.com/ANI-ML/openbark-releases/releases/tag/v0.3.0",
            "https://github.com/someone-else/openbark-releases/releases",
            "https://evil.example/ANI-ML/openbark-releases/",
            "https://www.openbark.io.evil.example/history",
            "https://user@www.openbark.io/history",
            "https://www.openbark.io:8443/history",
            "javascript:alert(1)",
            "file:///etc/passwd",
        ] {
            assert_eq!(safe_notes(url), NOTES_FALLBACK, "{url:?}");
        }
        let body = r#"{"version":"0.3.0","published":true,"notes":"https://evil.example/"}"#;
        assert_eq!(parse_release(body.as_bytes()).unwrap().unwrap().notes, NOTES_FALLBACK);
    }

    #[test]
    fn only_one_redirect_to_the_release_file_is_followed() {
        let bare = Url::parse("https://openbark.io/release.json").unwrap();
        let www = Url::parse(RELEASE_URL).unwrap();
        assert!(redirect_allowed(std::slice::from_ref(&bare), &www));
        assert!(
            redirect_allowed(std::slice::from_ref(&www), &www),
            "a same-address redirect is still one hop"
        );
        // A second hop, another host, another file, a query or plain http: not followed.
        assert!(!redirect_allowed(&[bare.clone(), www.clone()], &www));
        for next in [
            "https://evil.example/release.json",
            "https://www.openbark.io/other.json",
            "https://www.openbark.io/release.json?id=1",
            "http://www.openbark.io/release.json",
        ] {
            assert!(!redirect_allowed(std::slice::from_ref(&bare), &Url::parse(next).unwrap()), "{next}");
        }
    }

    #[test]
    fn the_request_is_a_plain_get_with_only_the_user_agent() {
        let request = request(&client().unwrap(), RELEASE_URL).unwrap();
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(request.url().as_str(), RELEASE_URL);
        assert_eq!(request.url().query(), None);
        assert_eq!(request.url().fragment(), None);
        assert!(request.url().username().is_empty() && request.url().password().is_none());
        let names: Vec<&str> = request.headers().keys().map(|k| k.as_str()).collect();
        assert_eq!(names, ["user-agent"]);
        assert_eq!(request.headers()[USER_AGENT], AGENT);
        assert_eq!(AGENT, format!("OpenBark/{}", env!("CARGO_PKG_VERSION")));
        assert!(request.body().is_none());
    }

    /// A one-shot HTTP server on loopback: answers with `reply` and hands
    /// back the request exactly as it arrived.
    fn serve_once(reply: String) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/release.json", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else { return };
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 1024];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => seen.extend_from_slice(&buf[..n]),
                }
            }
            let _ = stream.write_all(reply.as_bytes());
            let _ = tx.send(String::from_utf8_lossy(&seen).into_owned());
        });
        (url, rx)
    }

    fn ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn what_goes_on_the_wire_is_the_get_the_user_agent_and_the_protocols_own_lines() {
        let (url, seen) = serve_once(ok(PUBLISHED));
        let release = fetch_from(&url).unwrap().unwrap();
        assert_eq!(release.version, "0.2.10");
        let raw = seen.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut lines = raw.split("\r\n").filter(|l| !l.is_empty());
        assert_eq!(lines.next(), Some("GET /release.json HTTP/1.1"), "no query string: {raw}");
        let mut names = Vec::new();
        for line in lines {
            let (name, value) = line.split_once(':').unwrap();
            let name = name.trim().to_ascii_lowercase();
            if name == "user-agent" {
                assert_eq!(value.trim(), AGENT);
            }
            if name == "accept" {
                assert_eq!(value.trim(), "*/*");
            }
            names.push(name);
        }
        names.sort();
        // Host (HTTP needs it), reqwest's Accept: */*, and ours: no cookie, id or token.
        assert_eq!(names, ["accept", "host", "user-agent"], "{raw}");
    }

    #[test]
    fn an_unpublished_release_on_the_server_announces_nothing() {
        let (url, _seen) = serve_once(ok(r#"{"version":"9.9.9","published":false}"#));
        assert_eq!(fetch_from(&url).unwrap(), None);
    }

    #[test]
    fn a_server_error_or_a_redirect_elsewhere_is_a_failure() {
        let (url, _seen) =
            serve_once("HTTP/1.1 500 Oops\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        assert!(matches!(fetch_from(&url), Err(UpdateError::Http(500))));
        let redirect = "HTTP/1.1 302 Found\r\nLocation: https://evil.example/release.json\r\n\
                        Content-Length: 0\r\nConnection: close\r\n\r\n";
        let (url, _seen) = serve_once(redirect.into());
        assert!(matches!(fetch_from(&url), Err(UpdateError::Http(302))));
    }

    #[test]
    fn an_oversized_answer_is_refused() {
        let big =
            format!("{{\"version\":\"0.3.0\",\"published\":true,\"pad\":\"{}\"}}", "x".repeat(70 * 1024));
        let (url, _seen) = serve_once(ok(&big));
        assert!(matches!(fetch_from(&url), Err(UpdateError::Invalid)));
    }

    #[test]
    fn nothing_listening_fails_quietly_and_quickly() {
        // A port that was just free: connection refused, an error, no panic.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let started = std::time::Instant::now();
        assert!(matches!(
            fetch_from(&format!("http://127.0.0.1:{port}/release.json")),
            Err(UpdateError::Network(_))
        ));
        assert!(started.elapsed() < TIMEOUT + Duration::from_secs(2));
    }
}
