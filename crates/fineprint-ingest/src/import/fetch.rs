// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `fineprint-ingest fetch`: download the Scryfall bulk files (and optionally MTGJSON
//! `AtomicCards`) into the git-ignored cache (REQ-ING-001, REQ-NFR-004).
//!
//! - One run makes at most ONE Scryfall API request, to [`BULK_DATA_URL`], and only when a
//!   needed file is stale; it then downloads each stale entry's `download_uri` (a file host).
//!   It never requests a per-card endpoint and never loops over cards.
//! - `download_uri` is untrusted ([`validate_download_uri`]): `https://` only, host
//!   `scryfall.com` or a subdomain of `scryfall.com` / `scryfall.io` except the API host
//!   `api.scryfall.com`, no credentials, no port, no curl glob characters. The cache file names
//!   are fixed constants.
//! - 24-hour cache: a file whose metadata says it was fetched less than [`CACHE_SECONDS`] ago
//!   (and not in the future) is skipped unless forced. The time is a parameter (`now`): only the
//!   CLI wrapper reads the system clock (R7).
//! - Concurrent runs: each group directory is created first and an exclusive lock on its
//!   [`LOCK_FILE`] is held from reading `source.json` to the end of the commit, so two runs on
//!   one cache serialize (the second sees the first's fresh metadata). The lock file is left in
//!   place. Each run stages into its own part files `{file}.{pid}.part`, created exclusively,
//!   and removes them on every failure path.
//! - Every download goes to a part file and is validated ([`validate_scryfall_bulk`],
//!   [`validate_atomic_cards`]); a failed or invalid download leaves the cache untouched. The
//!   commit then (1) atomically rewrites `source.json` without the entries of the files about
//!   to be replaced, (2) renames the staged files into place, (3) atomically writes the full new
//!   `source.json`. A crash or failure at any step leaves every cached file either absent from
//!   the metadata (so it is stale and refetched, never trusted) or consistent with it.
//! - HTTP goes through the [`Http`] trait; the production [`Curl`] runs the system `curl` (the
//!   standard library has no TLS; the same choice as `cargo xtask cr fetch`) with `--disable`
//!   (no `~/.curlrc`), `--globoff`, HTTPS only and no redirect following. Every response is
//!   streamed through this process and capped while it is read: the metadata in memory at
//!   [`MAX_METADATA_BYTES`], each bulk file into its part file at the request's byte cap
//!   ([`run_capped_to_file`]; curl writes to stdout, never `--output`). Past the cap curl is
//!   killed, so the cap does not depend on the curl or libcurl version. `--max-filesize` stays
//!   as a second line of defence; a curl older than [`MIN_CURL_VERSION`] or an unparseable
//!   version only prints one warning per process. Tests inject a fake and never touch the
//!   network.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdout, Command, Stdio};
use std::sync::OnceLock;

use fineprint_base::hash::sha256;
use serde_json::{Map, Value, json};

use super::json_array::validate_array;
use super::{
    ATOMIC_CARDS_FILE, MAX_INPUT_BYTES, MTGJSON_DIR, ORACLE_CARDS_FILE, RULINGS_FILE, SCRYFALL_DIR,
    SOURCE_FILE,
};

/// The Scryfall bulk-data metadata endpoint (the only API request a run makes).
pub const BULK_DATA_URL: &str = "https://api.scryfall.com/bulk-data";
/// The MTGJSON `AtomicCards` download (contacted only with `--mtgjson`).
pub const MTGJSON_ATOMIC_URL: &str = "https://mtgjson.com/api/v5/AtomicCards.json";
/// A cached file younger than this many seconds is fresh.
pub const CACHE_SECONDS: u64 = 86_400;
/// The largest bulk-data metadata response accepted.
pub const MAX_METADATA_BYTES: u64 = 1024 * 1024;
/// The `--max-time` for the metadata request, in seconds.
pub const METADATA_MAX_TIME: u64 = 60;
/// The `--max-time` for a bulk download, in seconds.
pub const DOWNLOAD_MAX_TIME: u64 = 3600;
/// The longest `updated_at` string kept.
const MAX_UPDATED_AT: usize = 256;
/// The longest download URI accepted.
const MAX_URI: usize = 2048;

/// The `User-Agent` sent with every request (Scryfall requires one).
#[must_use]
pub fn user_agent() -> String {
    format!("fineprint-ingest/{}", env!("CARGO_PKG_VERSION"))
}

/// One HTTP GET.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The URL.
    pub url: String,
    /// Request headers as `(name, value)`.
    pub headers: Vec<(String, String)>,
    /// The largest response body accepted, in bytes.
    pub max_bytes: u64,
    /// The time limit, in seconds.
    pub max_time_secs: u64,
}

impl Request {
    fn new(url: &str, max_bytes: u64, max_time_secs: u64) -> Request {
        Request {
            url: url.to_owned(),
            headers: vec![
                ("User-Agent".to_owned(), user_agent()),
                ("Accept".to_owned(), "application/json".to_owned()),
            ],
            max_bytes,
            max_time_secs,
        }
    }
}

/// A minimal HTTP client.
pub trait Http {
    /// GET `req.url` and return the body (a small JSON document).
    ///
    /// # Errors
    /// A human-readable reason.
    fn get_json(&self, req: &Request) -> Result<Vec<u8>, String>;

    /// GET `req.url` into the file `dest` (created or truncated).
    ///
    /// # Errors
    /// A human-readable reason.
    fn download_to_file(&self, req: &Request, dest: &Path) -> Result<(), String>;
}

/// The production client: runs the system `curl` (no shell; arguments passed separately).
#[derive(Clone, Copy, Debug, Default)]
pub struct Curl;

/// The oldest curl whose `--max-filesize` also stops a transfer whose size is unknown when it
/// starts. Advisory only: an older or unparseable `curl --version` prints one warning per
/// process; the bulk-download size cap is enforced in-process whatever the version (R12).
pub const MIN_CURL_VERSION: (u32, u32, u32) = (8, 4, 0);
/// The lock file inside each cache group directory (left in place after a run).
pub const LOCK_FILE: &str = ".lock";
/// The longest curl stderr excerpt quoted in an error, in bytes.
const MAX_STDERR_EXCERPT: usize = 200;

/// The `curl` arguments for `req`, writing to `dest` (or stdout when `None`): `--disable` first
/// (no `~/.curlrc`), then `--fail --silent --show-error --globoff`, HTTPS only, no redirect
/// following, a size and time limit, the headers; never `--insecure`. curl applies the proxy and
/// CA settings of the environment. `dest` is passed byte-exact (no lossy conversion).
#[must_use]
pub fn curl_args(req: &Request, dest: Option<&Path>) -> Vec<OsString> {
    let mut a: Vec<OsString> = [
        "--disable",
        "--fail",
        "--silent",
        "--show-error",
        "--globoff",
        "--proto",
        "=https",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    a.extend([
        OsString::from("--max-filesize"),
        OsString::from(req.max_bytes.to_string()),
        OsString::from("--max-time"),
        OsString::from(req.max_time_secs.to_string()),
    ]);
    for (k, v) in &req.headers {
        a.push(OsString::from("--header"));
        a.push(OsString::from(format!("{k}: {v}")));
    }
    if let Some(d) = dest {
        a.push(OsString::from("--output"));
        a.push(d.as_os_str().to_owned());
    }
    a.push(OsString::from("--url"));
    a.push(OsString::from(&req.url));
    a
}

/// The error text for a failed curl run: the role and exit status, plus the first line of
/// curl's stderr cut to 200 bytes on a character boundary with control characters replaced by
/// `?` (R12: never the URL, never unbounded network text).
#[must_use]
pub fn curl_failure_message(role: &str, exit: Option<i32>, stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut excerpt = String::new();
    for c in first.chars() {
        let c = if c.is_control() { '?' } else { c };
        if excerpt.len() + c.len_utf8() > MAX_STDERR_EXCERPT {
            break;
        }
        excerpt.push(c);
    }
    let status = exit.map_or_else(|| "no exit code".to_owned(), |n| format!("exit {n}"));
    if excerpt.is_empty() {
        format!("curl failed for {role} ({status})")
    } else {
        format!("curl failed for {role} ({status}): {excerpt}")
    }
}

/// Reads at most `cap` bytes from `r`.
///
/// # Errors
/// The read error, or a message naming the cap when `r` holds more than `cap` bytes (at most
/// `cap + 1` bytes are read).
pub fn read_stream_capped<R: Read>(r: R, cap: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    r.take(cap.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| format!("cannot read the response: {e}"))?;
    if u64::try_from(buf.len()).unwrap_or(u64::MAX) > cap {
        return Err(format!("the response is larger than the {cap}-byte cap"));
    }
    Ok(buf)
}

/// The buffer size of [`copy_stream_capped`].
const COPY_BUFFER: usize = 64 * 1024;

/// Copies `r` into `w` through a fixed 64 KiB buffer, reading at most `cap + 1` bytes and
/// writing at most `cap` bytes (R12: the cap holds while the bytes stream, whatever produces
/// them). Returns the number of bytes written. `w` is not flushed.
///
/// # Errors
/// A read or write error, or a message naming the cap when `r` holds more than `cap` bytes
/// (the bytes past the cap are never written).
pub fn copy_stream_capped<R: Read, W: Write>(r: R, w: &mut W, cap: u64) -> Result<u64, String> {
    let mut r = r.take(cap.saturating_add(1));
    let mut buf = vec![0u8; COPY_BUFFER];
    let mut written: u64 = 0;
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) => return Ok(written),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("cannot read the response: {e}")),
        };
        let room = usize::try_from(cap.saturating_sub(written)).unwrap_or(usize::MAX);
        if n > room {
            return Err(format!("the response is larger than the {cap}-byte cap"));
        }
        w.write_all(&buf[..n])
            .map_err(|e| format!("cannot write the download: {e}"))?;
        written = written.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
    }
}

/// Runs `cmd` with stdin closed and stdout and stderr piped, hands stdout to `consume` while the
/// child runs, and kills (and waits) the child as soon as `consume` fails (R12). stderr is
/// drained on its own thread (first 4 KiB kept) so neither pipe can block the child.
fn run_streaming<T>(
    mut cmd: Command,
    role: &str,
    consume: impl FnOnce(ChildStdout) -> Result<T, String>,
) -> Result<T, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "curl not found: install curl (fineprint-ingest fetch uses it for HTTPS downloads)"
                .to_owned()
        } else {
            format!("cannot run curl: {e}")
        }
    })?;
    let stderr = child.stderr.take().map(|mut e| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let _ = (&mut e).take(4096).read_to_end(&mut kept);
            let _ = std::io::copy(&mut e, &mut std::io::sink());
            kept
        })
    });
    let out = match child.stdout.take() {
        Some(o) => consume(o),
        None => Err("cannot read the response: no stdout".to_owned()),
    };
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            // The stderr thread is not joined: it ends when the pipe closes.
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{e} for {role}; the transfer was stopped"));
        }
    };
    let status = child.wait().map_err(|e| format!("cannot run curl: {e}"))?;
    let err = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
    if status.success() {
        Ok(out)
    } else {
        Err(curl_failure_message(role, status.code(), &err))
    }
}

/// Runs `cmd` with piped stdout and stderr, reading at most `cap` bytes of stdout while it
/// runs; past the cap the child is killed (R12). `role` names the transfer in errors.
///
/// # Errors
/// A spawn failure, the cap, or [`curl_failure_message`] for a non-zero exit.
pub fn run_capped(cmd: Command, cap: u64, role: &str) -> Result<Vec<u8>, String> {
    run_streaming(cmd, role, |o| read_stream_capped(o, cap))
}

/// Runs `cmd` and streams its stdout into the existing file `dest` (opened for writing and
/// truncated, never created), writing at most `cap` bytes through [`copy_stream_capped`]; as
/// soon as the child produces more than `cap` bytes it is killed and waited (R12). The cap is
/// enforced by this process, whatever the child is. The file is flushed at the end. On any
/// error `dest` may hold a partial body (at most `cap` bytes) and must not be trusted: the
/// caller removes it. `role` names the transfer in errors.
///
/// # Errors
/// A spawn, open, read or write failure, the cap, or [`curl_failure_message`] for a non-zero
/// exit.
pub fn run_capped_to_file(cmd: Command, cap: u64, role: &str, dest: &Path) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(dest)
        .map_err(|e| format!("cannot open the part file for {role}: {e}"))?;
    run_streaming(cmd, role, |o| {
        copy_stream_capped(o, &mut file, cap)?;
        file.flush()
            .map_err(|e| format!("cannot write the download: {e}"))
    })
}

fn run_curl(args: Vec<OsString>, cap: u64, role: &str) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("curl");
    cmd.args(args);
    run_capped(cmd, cap, role)
}

/// Parses the version of `curl --version` output (`curl 8.4.0 (...) ...`; a suffix such as
/// `-DEV` is ignored). `None` when the first line has another shape.
#[must_use]
pub fn parse_curl_version(output: &str) -> Option<(u32, u32, u32)> {
    let mut words = output.lines().next()?.split_whitespace();
    if words.next()? != "curl" {
        return None;
    }
    let mut parts = words.next()?.split('.');
    let mut num = |required: bool| -> Option<u32> {
        match parts.next() {
            None if !required => Some(0),
            None => None,
            Some(p) => {
                let digits: String = p.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            }
        }
    };
    Some((num(true)?, num(true)?, num(false)?))
}

/// The advisory verdict on `curl --version` output: `Ok(None)` for [`MIN_CURL_VERSION`] or
/// later, `Ok(Some(warning))` for an older or unparseable version. The warning names 8.4.0 and
/// says the in-process cap still applies: the download size cap never depends on this verdict
/// (R12; the frontend version need not be the linked libcurl's).
///
/// # Errors
/// Never for a version (the `Result` is kept for API stability).
pub fn check_curl_version(output: &str) -> Result<Option<String>, String> {
    match parse_curl_version(output) {
        None => Ok(Some(
            "warning: cannot parse the curl version; curl 8.4.0 or later is recommended as a \
             second line of defence (--max-filesize), and the in-process download size cap \
             still applies"
                .to_owned(),
        )),
        Some(v) if v < MIN_CURL_VERSION => Ok(Some(format!(
            "warning: curl {}.{}.{} is older than 8.4.0, so --max-filesize may not stop a \
             response of unknown size; the in-process download size cap still applies",
            v.0, v.1, v.2
        ))),
        Some(_) => Ok(None),
    }
}

/// Checks the system curl version once per process and prints at most one warning on stderr
/// (see [`check_curl_version`]). Advisory only: it never fails and never decides the cap.
fn curl_version_advisory() {
    static CHECKED: OnceLock<()> = OnceLock::new();
    CHECKED.get_or_init(|| {
        let mut cmd = Command::new("curl");
        cmd.args(["--disable", "--version"]);
        let warning = match run_capped(cmd, 64 * 1024, "curl --version") {
            Ok(out) => check_curl_version(&String::from_utf8_lossy(&out)).unwrap_or_else(Some),
            Err(_) => Some(
                "warning: cannot read the curl version; the in-process download size cap still \
                 applies"
                    .to_owned(),
            ),
        };
        if let Some(w) = warning {
            eprintln!("{w}");
        }
    });
}

/// The role label of a download into `dest` (`{file}.{pid}.part`): the cache file name.
fn download_role(dest: &Path) -> &'static str {
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned());
    let name = name.as_deref().unwrap_or("");
    [ORACLE_CARDS_FILE, RULINGS_FILE, ATOMIC_CARDS_FILE]
        .into_iter()
        .find(|f| name.starts_with(f))
        .unwrap_or("download")
}

impl Http for Curl {
    fn get_json(&self, req: &Request) -> Result<Vec<u8>, String> {
        run_curl(curl_args(req, None), req.max_bytes, "bulk-data metadata")
    }

    fn download_to_file(&self, req: &Request, dest: &Path) -> Result<(), String> {
        // R12: curl writes to stdout (no `--output`); this process streams the body into the
        // part file and kills curl past `req.max_bytes`. `--max-filesize` stays in the arguments
        // as a second line of defence only; the version check is advisory.
        curl_version_advisory();
        let mut cmd = Command::new("curl");
        cmd.args(curl_args(req, None));
        run_capped_to_file(cmd, req.max_bytes, download_role(dest), dest)
    }
}

/// Whether `uri` is an acceptable `download_uri` (R12): `https://`, no credentials, no port, no
/// whitespace, control characters, backslashes or curl glob characters (`[ ] { }`), and a host
/// that is `scryfall.com` or ends in `.scryfall.com` or `.scryfall.io` with non-empty labels of
/// `a-z 0-9 -`, except the API host `api.scryfall.com` (downloads never hit the API).
///
/// # Errors
/// A fixed reason (the URI itself is not repeated).
pub fn validate_download_uri(uri: &str) -> Result<(), &'static str> {
    if uri.len() > MAX_URI {
        return Err("the download URI is too long");
    }
    if uri
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return Err("the download URI contains whitespace, control characters or backslashes");
    }
    if uri.contains(['[', ']', '{', '}']) {
        return Err("the download URI contains curl glob characters");
    }
    let Some(rest) = uri.strip_prefix("https://") else {
        return Err("the download URI is not https");
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..end).unwrap_or("");
    if authority.contains('@') {
        return Err("the download URI carries credentials");
    }
    if authority.contains(':') {
        return Err("the download URI names a port");
    }
    let host = authority;
    if host == "api.scryfall.com" {
        return Err("the download URI names the Scryfall API host");
    }
    let labels_ok = !host.is_empty()
        && host.split('.').all(|l| {
            !l.is_empty()
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    let allowed =
        host == "scryfall.com" || host.ends_with(".scryfall.com") || host.ends_with(".scryfall.io");
    if labels_ok && allowed {
        Ok(())
    } else {
        Err("the download URI host is not scryfall.com, *.scryfall.com or *.scryfall.io")
    }
}

/// One entry of the bulk-data metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BulkEntry {
    /// The download URI (still to be validated).
    pub download_uri: String,
    /// The `updated_at` string, verbatim.
    pub updated_at: String,
}

/// Parses the bulk-data metadata (`{"object":"list","data":[...]}`) and returns the entries for
/// `oracle_cards` and `rulings` (the first entry of each type).
///
/// # Errors
/// A fixed reason when the document has the wrong shape or lacks one of the two types.
pub fn parse_bulk_data(bytes: &[u8]) -> Result<(BulkEntry, BulkEntry), String> {
    let v: Value =
        serde_json::from_slice(bytes).map_err(|_| "the bulk-data metadata is not JSON")?;
    let data = v
        .get("data")
        .and_then(Value::as_array)
        .ok_or("the bulk-data metadata has no `data` array")?;
    let find = |ty: &str| -> Result<BulkEntry, String> {
        let e = data
            .iter()
            .find(|e| e.get("type").and_then(Value::as_str) == Some(ty))
            .ok_or_else(|| format!("the bulk-data metadata has no `{ty}` entry"))?;
        let field = |k: &str| {
            e.get(k)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("the `{ty}` entry has no string `{k}`"))
        };
        let updated_at = field("updated_at")?;
        if updated_at.len() > MAX_UPDATED_AT {
            return Err(format!("the `{ty}` entry's `updated_at` is too long"));
        }
        Ok(BulkEntry {
            download_uri: field("download_uri")?,
            updated_at,
        })
    };
    Ok((find("oracle_cards")?, find("rulings")?))
}

/// The cache metadata of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedFile {
    /// The URL the file was downloaded from.
    pub source_url: String,
    /// The source's `updated_at` (verbatim), when known.
    pub updated_at: Option<String>,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    /// The file size.
    pub byte_count: u64,
    /// When it was fetched (seconds since the Unix epoch).
    pub fetched_at_epoch_seconds: u64,
}

/// A group's `source.json`: file name → metadata.
pub type SourceMeta = BTreeMap<String, CachedFile>;

/// The version of `source.json`.
pub const SOURCE_FORMAT_VERSION: u64 = 1;

/// Renders a group's `source.json` (`{"formatVersion":1,"files":{...}}`, sorted members).
#[must_use]
pub fn render_source_meta(meta: &SourceMeta) -> Vec<u8> {
    let files: Map<String, Value> = meta
        .iter()
        .map(|(k, f)| {
            (
                k.clone(),
                json!({
                    "sourceUrl": f.source_url,
                    "updatedAt": f.updated_at,
                    "sha256": f.sha256,
                    "byteCount": f.byte_count,
                    "fetchedAtEpochSeconds": f.fetched_at_epoch_seconds,
                }),
            )
        })
        .collect();
    let v = json!({ "formatVersion": SOURCE_FORMAT_VERSION, "files": files });
    let mut s = serde_json::to_vec_pretty(&v).unwrap_or_default();
    s.push(b'\n');
    s
}

/// Parses a group's `source.json`; entries with a wrong shape are skipped.
///
/// # Errors
/// A fixed reason when the document is not an object with a `files` object.
pub fn parse_source_meta(bytes: &[u8]) -> Result<SourceMeta, &'static str> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| "source.json is not JSON")?;
    let files = v
        .get("files")
        .and_then(Value::as_object)
        .ok_or("source.json has no `files` object")?;
    let mut out = SourceMeta::new();
    for (k, f) in files {
        let s = |key: &str| f.get(key).and_then(Value::as_str).map(str::to_owned);
        let n = |key: &str| f.get(key).and_then(Value::as_u64);
        let (Some(source_url), Some(sha256), Some(byte_count), Some(fetched)) = (
            s("sourceUrl"),
            s("sha256"),
            n("byteCount"),
            n("fetchedAtEpochSeconds"),
        ) else {
            continue;
        };
        out.insert(
            k.clone(),
            CachedFile {
                source_url,
                updated_at: s("updatedAt"),
                sha256,
                byte_count,
                fetched_at_epoch_seconds: fetched,
            },
        );
    }
    Ok(out)
}

/// Reads a group's `source.json`, or an empty map when missing or invalid.
#[must_use]
pub fn read_source_meta(group_dir: &Path) -> SourceMeta {
    super::read_capped(&group_dir.join(SOURCE_FILE), MAX_METADATA_BYTES)
        .ok()
        .and_then(|b| parse_source_meta(&b).ok())
        .unwrap_or_default()
}

/// Whether a cached file is fresh at `now`: it exists, has metadata, and was fetched at or
/// before `now` and less than [`CACHE_SECONDS`] ago (a future `fetchedAt` is stale).
#[must_use]
pub fn is_fresh(meta: &SourceMeta, group_dir: &Path, file: &str, now: u64) -> bool {
    meta.get(file).is_some_and(|m| {
        m.fetched_at_epoch_seconds <= now
            && now - m.fetched_at_epoch_seconds < CACHE_SECONDS
            && group_dir.join(file).is_file()
    })
}

fn validate_common(bytes: &[u8]) -> Result<&str, String> {
    if bytes.is_empty() {
        return Err("the download is empty".to_owned());
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_INPUT_BYTES {
        return Err("the download is larger than 4 GiB".to_owned());
    }
    std::str::from_utf8(bytes).map_err(|_| "the download is not valid UTF-8".to_owned())
}

/// Validates a Scryfall bulk file: non-empty, at most 4 GiB, UTF-8, a JSON array whose every
/// element parses.
///
/// # Errors
/// A fixed reason.
pub fn validate_scryfall_bulk(bytes: &[u8]) -> Result<(), String> {
    validate_common(bytes)?;
    validate_array(bytes)
        .map(|_| ())
        .map_err(|e| format!("the download is invalid: {e}"))
}

/// Validates an MTGJSON `AtomicCards` file (non-empty, at most 4 GiB, UTF-8, a JSON object
/// with a `data` member) and returns its `meta.date` string, if any.
///
/// # Errors
/// A fixed reason.
pub fn validate_atomic_cards(bytes: &[u8]) -> Result<Option<String>, String> {
    validate_common(bytes)?;
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|_| "the download is not well-formed JSON".to_owned())?;
    if !v.get("data").is_some_and(Value::is_object) {
        return Err("the download is not an object with a `data` object".to_owned());
    }
    Ok(v.get("meta")
        .and_then(|m| m.get("date"))
        .and_then(Value::as_str)
        .filter(|s| s.len() <= MAX_UPDATED_AT)
        .map(str::to_owned))
}

/// Options for [`fetch`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchOptions {
    /// The cache directory.
    pub cache: PathBuf,
    /// Download even when fresh.
    pub force: bool,
    /// Also download MTGJSON `AtomicCards`.
    pub mtgjson: bool,
}

/// A download validator: `Ok` carries an `updatedAt` read from the file, if any.
type Validator = dyn Fn(&[u8]) -> Result<Option<String>, String>;

/// A validated download waiting to be committed.
struct Staged {
    file: &'static str,
    part: PathBuf,
    meta: CachedFile,
}

/// This run's part file for `file` in `dir`: `{file}.{pid}.part`.
#[must_use]
pub fn part_path(dir: &Path, file: &str) -> PathBuf {
    dir.join(format!("{file}.{}.part", std::process::id()))
}

/// Creates the part file exclusively. The group lock is held, so an existing file of this
/// name is a leftover of a crashed run with the same pid: it is removed and creation retried.
fn create_part(part: &Path) -> Result<(), String> {
    let open = || {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(part)
    };
    let created = match open() {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(part);
            open()
        }
        other => other,
    };
    created
        .map(drop)
        .map_err(|e| format!("cannot create {}: {e}", part.display()))
}

/// Creates `dir` and takes the exclusive lock on its [`LOCK_FILE`], held until the returned
/// file is dropped.
fn lock_group(dir: &Path) -> Result<fs::File, String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = dir.join(LOCK_FILE);
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    file.lock()
        .map_err(|e| format!("cannot lock {}: {e}", path.display()))?;
    Ok(file)
}

/// Downloads `req` to this run's part file of `file`, validates it with `check`, and returns
/// the staged result (the part file is removed on any failure).
fn stage(
    http: &dyn Http,
    dir: &Path,
    file: &'static str,
    req: &Request,
    updated_at: Option<String>,
    now: u64,
    check: &Validator,
) -> Result<Staged, String> {
    let part = part_path(dir, file);
    create_part(&part)?;
    let result = (|| {
        http.download_to_file(req, &part)?;
        let bytes = super::read_capped(&part, MAX_INPUT_BYTES)
            .map_err(|e| format!("cannot read the download: {e}"))?;
        let from_file = check(&bytes).map_err(|e| format!("{e} ({file})"))?;
        Ok(CachedFile {
            source_url: req.url.clone(),
            updated_at: updated_at.or(from_file),
            sha256: sha256(&bytes).to_hex(),
            byte_count: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            fetched_at_epoch_seconds: now,
        })
    })();
    match result {
        Ok(meta) => Ok(Staged { file, part, meta }),
        Err(e) => {
            let _ = fs::remove_file(&part);
            Err(e)
        }
    }
}

fn discard(staged: &[Staged]) {
    for s in staged {
        let _ = fs::remove_file(&s.part);
    }
}

/// Commits the staged files (the group lock is held): (1) atomically writes `source.json`
/// without the entries of the files about to be replaced, (2) renames the staged files into
/// place, (3) atomically writes the full new `source.json`. After a failure at any step every
/// file is either absent from the metadata (stale, refetched next run) or consistent with it,
/// and this run's remaining part files are removed.
fn commit(dir: &Path, mut meta: SourceMeta, staged: Vec<Staged>) -> Result<(), String> {
    let source = dir.join(SOURCE_FILE);
    let write = |m: &SourceMeta| {
        crate::vocab::cli::write_atomic(&source, &render_source_meta(m))
            .map_err(|e| format!("cannot write {}: {e}", source.display()))
    };
    let before = meta.len();
    for s in &staged {
        meta.remove(s.file);
    }
    if meta.len() != before
        && let Err(e) = write(&meta)
    {
        discard(&staged);
        return Err(e);
    }
    for (i, s) in staged.iter().enumerate() {
        if let Err(e) = fs::rename(&s.part, dir.join(s.file)) {
            discard(staged.get(i..).unwrap_or_default());
            return Err(format!("cannot write the cache in {}: {e}", dir.display()));
        }
    }
    for s in staged {
        meta.insert(s.file.to_owned(), s.meta);
    }
    write(&meta)
}

fn scryfall_bulk_check(bytes: &[u8]) -> Result<Option<String>, String> {
    validate_scryfall_bulk(bytes).map(|()| None)
}

/// Runs a fetch at time `now` (seconds since the Unix epoch). `Ok` is the summary line; `Err`
/// is an input or network error (exit 2). A failure before a group's commit leaves that group
/// unchanged; a failure during the commit leaves every file either absent from the group's
/// metadata (refetched next run) or consistent with it. Each group is locked while it is read,
/// staged and committed, so concurrent runs on one cache serialize.
///
/// # Errors
/// A human-readable reason that never quotes downloaded content.
pub fn fetch(http: &dyn Http, opts: &FetchOptions, now: u64) -> Result<String, String> {
    let mut done = Vec::new();
    let sdir = opts.cache.join(SCRYFALL_DIR);
    let slock = lock_group(&sdir)?;
    let smeta = read_source_meta(&sdir);
    let wanted: Vec<&'static str> = [ORACLE_CARDS_FILE, RULINGS_FILE]
        .into_iter()
        .filter(|f| opts.force || !is_fresh(&smeta, &sdir, f, now))
        .collect();
    if wanted.is_empty() {
        done.push("scryfall fresh".to_owned());
    } else {
        let body = http.get_json(&Request::new(
            BULK_DATA_URL,
            MAX_METADATA_BYTES,
            METADATA_MAX_TIME,
        ))?;
        if u64::try_from(body.len()).unwrap_or(u64::MAX) > MAX_METADATA_BYTES {
            return Err("the bulk-data metadata is too large".to_owned());
        }
        let (oracle, rulings) = parse_bulk_data(&body)?;
        let entry = |f: &str| {
            if f == ORACLE_CARDS_FILE {
                &oracle
            } else {
                &rulings
            }
        };
        for f in &wanted {
            validate_download_uri(&entry(f).download_uri)
                .map_err(|e| format!("{e} ({f}); the cache is unchanged"))?;
        }
        let mut staged = Vec::new();
        for f in &wanted {
            let e = entry(f);
            let req = Request::new(&e.download_uri, MAX_INPUT_BYTES, DOWNLOAD_MAX_TIME);
            match stage(
                http,
                &sdir,
                f,
                &req,
                Some(e.updated_at.clone()),
                now,
                &scryfall_bulk_check,
            ) {
                Ok(s) => staged.push(s),
                Err(err) => {
                    discard(&staged);
                    return Err(format!("{err}; the cache is unchanged"));
                }
            }
        }
        let summary: Vec<String> = staged
            .iter()
            .map(|s| format!("{} {} bytes", s.file, s.meta.byte_count))
            .collect();
        commit(&sdir, smeta, staged)?;
        done.push(format!("scryfall downloaded {}", summary.join(", ")));
    }
    drop(slock);
    if opts.mtgjson {
        let mdir = opts.cache.join(MTGJSON_DIR);
        let _mlock = lock_group(&mdir)?;
        let mmeta = read_source_meta(&mdir);
        if !opts.force && is_fresh(&mmeta, &mdir, ATOMIC_CARDS_FILE, now) {
            done.push("mtgjson fresh".to_owned());
        } else {
            let req = Request::new(MTGJSON_ATOMIC_URL, MAX_INPUT_BYTES, DOWNLOAD_MAX_TIME);
            let s = stage(
                http,
                &mdir,
                ATOMIC_CARDS_FILE,
                &req,
                None,
                now,
                &validate_atomic_cards,
            )
            .map_err(|e| format!("{e}; the mtgjson cache is unchanged"))?;
            let bytes = s.meta.byte_count;
            commit(&mdir, mmeta, vec![s])?;
            done.push(format!(
                "mtgjson downloaded {ATOMIC_CARDS_FILE} {bytes} bytes"
            ));
        }
    }
    Ok(format!(
        "fetch: {} ({})",
        done.join("; "),
        opts.cache.display()
    ))
}
