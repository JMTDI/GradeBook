//! The opt-in check for a newer GradeBook, against the releases on GitHub.
//!
//! The check asks for one small JSON document and compares its version with this one. It
//! uses `curl` (or `wget`), which every supported system has, instead of a network library
//! of its own: the app stays small, and nothing is sent unless the person turns the check on
//! or presses the button.

use std::fmt;
use std::process::Command;
use std::sync::Arc;

use serde::Deserialize;

/// The GitHub repository the releases are in.
pub const REPOSITORY: &str = "JMTDI/GradeBook";

/// Seconds to wait for the answer before giving up.
const TIMEOUT_SECONDS: &str = "15";
/// The answer is a few kilobytes; anything much bigger isn't it.
const MAX_RESPONSE_BYTES: usize = 1_000_000;

pub fn latest_release_api() -> String {
    format!("https://api.github.com/repos/{REPOSITORY}/releases/latest")
}

/// The page to send people to for the download.
pub fn releases_page() -> String {
    format!("https://github.com/{REPOSITORY}/releases/latest")
}

/// A version like 1.2.0: three numbers, as the releases are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// `1.2.0` or `v1.2.0`. Anything else, such as `1.2.0-beta` or `1.2`, isn't a version.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let (major, minor, patch) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let number = |part: &str| -> Option<u64> {
            let plain = !part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit());
            plain.then(|| part.parse().ok()).flatten()
        };
        Some(Version {
            major: number(major)?,
            minor: number(minor)?,
            patch: number(patch)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The version of this build.
pub fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The release's page.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// This is the newest, or newer than any release (a build made from the source).
    UpToDate,
    Available(Release),
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The release's own page, but only if it really is a page of this repository: the answer
/// is what the browser gets opened on.
fn release_url(url: Option<String>) -> String {
    let prefix = format!("https://github.com/{REPOSITORY}/");
    match url {
        Some(url) if url.starts_with(&prefix) && !url.contains(char::is_whitespace) => url,
        _ => releases_page(),
    }
}

/// Compares `current` with the release in `json`, the answer of the GitHub API for the
/// latest release.
pub fn check(current: Version, json: &str) -> Result<Check, String> {
    let release: ApiRelease =
        serde_json::from_str(json).map_err(|e| format!("The answer wasn't a release description ({e})."))?;
    if release.draft || release.prerelease {
        return Ok(Check::UpToDate);
    }
    let version =
        Version::parse(&release.tag_name).ok_or_else(|| format!("\"{}\" isn't a version number.", release.tag_name))?;
    Ok(if version > current {
        Check::Available(Release {
            version,
            url: release_url(release.html_url),
        })
    } else {
        Check::UpToDate
    })
}

/// What the check runs on a background thread to get the description of the latest release.
pub type Fetcher = Arc<dyn Fn() -> Result<String, String> + Send + Sync>;

/// Fetches the latest release from GitHub.
pub fn github_fetcher() -> Fetcher {
    Arc::new(|| fetch(&latest_release_api()))
}

/// Downloads `url` (https, or http for a test on this computer) with `curl`, or with `wget`
/// where there is no curl.
pub fn fetch(url: &str) -> Result<String, String> {
    let secure = url.starts_with("https://");
    if !secure && !url.starts_with("http://") {
        return Err("Only web addresses can be fetched.".to_string());
    }
    let user_agent = format!("GradeBook/{}", current());

    let mut curl = Command::new(curl_program());
    if !secure {
        // Plain http is only for a server on this computer (the tests'), which a proxy named in
        // the environment would never reach.
        curl.args(["--noproxy", "*"]);
    }
    curl.args([
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--max-time",
        TIMEOUT_SECONDS,
        "--max-filesize",
        "1000000",
        "--proto",
        if secure { "=https" } else { "=http" },
        "--proto-redir",
        "=https",
        "--header",
        "Accept: application/vnd.github+json",
        "--user-agent",
        &user_agent,
        url,
    ]);
    let mut wget = Command::new("wget");
    wget.args(["--quiet", "--output-document=-", "--timeout=15", "--tries=1"]);
    if secure {
        wget.arg("--https-only");
    } else {
        wget.arg("--no-proxy");
    }
    wget.args([
        "--header=Accept: application/vnd.github+json",
        &format!("--user-agent={user_agent}"),
        url,
    ]);

    let mut missing = true;
    let mut failure = String::new();
    for command in [&mut curl, &mut wget] {
        no_window(command);
        match command.output() {
            Ok(output) if output.status.success() => {
                if output.stdout.len() > MAX_RESPONSE_BYTES {
                    return Err("The answer was much bigger than expected.".to_string());
                }
                return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
            }
            Ok(output) => {
                missing = false;
                let message = String::from_utf8_lossy(&output.stderr);
                failure = message
                    .lines()
                    .next()
                    .unwrap_or("The request failed.")
                    .trim()
                    .to_string();
                break;
            }
            Err(_) => continue,
        }
    }
    if missing {
        Err("Neither curl nor wget is installed, so GradeBook can't look for updates.".to_string())
    } else {
        Err(failure)
    }
}

#[cfg(windows)]
fn curl_program() -> std::path::PathBuf {
    // Windows 10 and 11 ship curl in the system folder.
    let system =
        std::env::var_os("SystemRoot").map(|root| std::path::Path::new(&root).join("System32").join("curl.exe"));
    match system {
        Some(path) if path.exists() => path,
        _ => "curl.exe".into(),
    }
}

#[cfg(not(windows))]
fn curl_program() -> std::path::PathBuf {
    "curl".into()
}

/// Keeps a console window from flashing up when the app, which has none, starts curl.
#[cfg(windows)]
fn no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn no_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn versions_are_three_numbers() {
        assert_eq!(
            v("1.2.3"),
            Version {
                major: 1,
                minor: 2,
                patch: 3
            }
        );
        assert_eq!(
            v("v10.0.7"),
            Version {
                major: 10,
                minor: 0,
                patch: 7
            }
        );
        assert_eq!(v(" v1.0.0 \n").to_string(), "1.0.0");
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1.2.3-beta",
            "v",
            "vv1.2.3",
            "-1.2.3",
            "1..3",
            "1.2.99999999999",
            "١.٢.٣",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(v("1.10.0") > v("1.9.9"));
        assert!(v("2.0.0") > v("1.99.99"));
        assert!(v("1.0.10") > v("1.0.9"));
        assert!(v("1.0.0") == v("v1.0.0"));
        assert!(current() >= v("1.0.0"), "this build's own version parses");
    }

    fn release(tag: &str, extra: &str) -> String {
        format!(
            r#"{{"tag_name": "{tag}", "html_url": "https://github.com/{REPOSITORY}/releases/tag/{tag}", "name": "GradeBook"{extra}}}"#
        )
    }

    #[test]
    fn a_newer_release_is_reported_with_its_page() {
        let found = check(v("1.0.0"), &release("v1.2.0", "")).unwrap();
        assert_eq!(
            found,
            Check::Available(Release {
                version: v("1.2.0"),
                url: format!("https://github.com/{REPOSITORY}/releases/tag/v1.2.0"),
            })
        );
    }

    #[test]
    fn the_same_or_an_older_release_is_up_to_date() {
        assert_eq!(check(v("1.2.0"), &release("v1.2.0", "")), Ok(Check::UpToDate));
        assert_eq!(check(v("2.0.0"), &release("v1.2.0", "")), Ok(Check::UpToDate));
    }

    #[test]
    fn drafts_and_pre_releases_are_ignored() {
        assert_eq!(
            check(v("1.0.0"), &release("v9.0.0", ", \"prerelease\": true")),
            Ok(Check::UpToDate)
        );
        assert_eq!(
            check(v("1.0.0"), &release("v9.0.0", ", \"draft\": true")),
            Ok(Check::UpToDate)
        );
    }

    #[test]
    fn an_answer_that_isnt_a_release_is_an_error() {
        for bad in ["", "not json", "{}", "[]", "{\"message\": \"Not Found\"}"] {
            assert!(check(v("1.0.0"), bad).is_err(), "{bad:?}");
        }
        let error = check(v("1.0.0"), &release("latest", "")).unwrap_err();
        assert!(error.contains("latest"), "{error}");
    }

    #[test]
    fn the_page_to_open_is_always_one_of_this_repository() {
        let json = |url: &str| format!(r#"{{"tag_name": "v2.0.0", "html_url": "{url}"}}"#);
        let page = |json: &str| match check(v("1.0.0"), json).unwrap() {
            Check::Available(release) => release.url,
            Check::UpToDate => panic!("newer"),
        };
        for elsewhere in [
            "https://evil.example/releases/tag/v2.0.0",
            "http://github.com/JMTDI/GradeBook/releases/tag/v2.0.0",
            "https://github.com/JMTDI/GradeBookEvil/releases",
            "https://github.com/other/GradeBook/releases/tag/v2.0.0",
            "file:///etc/passwd",
            "https://github.com/JMTDI/GradeBook/ x",
            "",
        ] {
            assert_eq!(page(&json(elsewhere)), releases_page(), "{elsewhere:?}");
        }
        assert_eq!(page(r#"{"tag_name": "v2.0.0"}"#), releases_page(), "no page given");
        let own = "https://github.com/JMTDI/GradeBook/releases/tag/v2.0.0";
        assert_eq!(page(&json(own)), own);
    }

    #[test]
    fn only_web_addresses_are_fetched() {
        for bad in [
            "file:///etc/passwd",
            "ftp://example.com/x",
            "-o /tmp/x",
            "",
            "example.com",
        ] {
            assert!(fetch(bad).unwrap_err().contains("web addresses"), "{bad:?}");
        }
    }

    /// Answers each of `responses` (status line and body) in turn on a local port, and hands
    /// back what the requests said.
    fn serve(responses: Vec<(&'static str, String)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/latest", listener.local_addr().unwrap().port());
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = stream.read(&mut chunk).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..n]);
                }
                requests.push(String::from_utf8_lossy(&request).to_string());
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(reply.as_bytes()).unwrap();
            }
            requests
        });
        (url, handle)
    }

    fn has_a_downloader() -> bool {
        let found = |program: &str| Command::new(program).arg("--version").output().is_ok();
        found(&curl_program().to_string_lossy()) || found("wget")
    }

    #[test]
    fn the_answer_is_fetched_over_http_and_failures_are_reported() {
        if !has_a_downloader() {
            eprintln!("skipped: neither curl nor wget is installed here");
            return;
        }
        let body = release("v3.1.4", "");
        let (url, server) = serve(vec![("200 OK", body.clone()), ("404 Not Found", "{}".to_string())]);

        assert_eq!(fetch(&url).unwrap(), body);
        let error = fetch(&url).unwrap_err();
        assert!(!error.is_empty(), "a missing page is an error");

        let requests = server.join().unwrap();
        let request = requests[0].to_lowercase();
        assert!(request.starts_with("get /latest "), "{request}");
        assert!(
            request.contains(&format!("user-agent: gradebook/{}", current())),
            "{request}"
        );
        assert!(request.contains("accept: application/vnd.github+json"), "{request}");
    }

    #[test]
    fn nothing_listening_is_an_error_not_a_hang() {
        if !has_a_downloader() {
            return;
        }
        // Bind and drop to find a port that is free, so nothing answers on it.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(fetch(&format!("http://127.0.0.1:{port}/latest")).is_err());
    }
}
