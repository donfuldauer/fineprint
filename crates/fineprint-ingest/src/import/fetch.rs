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
//!   `scryfall.com` or a subdomain of `scryfall.com` / `scryfall.io`, no credentials, no port.
//!   The cache file names are fixed constants.
//! - 24-hour cache: a file whose metadata says it was fetched less than [`CACHE_SECONDS`] ago
//!   (and not in the future) is skipped unless forced. The time is a parameter (`now`): only the
//!   CLI wrapper reads the system clock (R7).
//! - Every download goes to a `.part` file, is validated ([`validate_scryfall_bulk`],
//!   [`validate_atomic_cards`]) and only then renamed into place; the group's `source.json` is
//!   written atomically after. A failed or invalid download leaves the cache untouched.
//! - HTTP goes through the [`Http`] trait; the production [`Curl`] runs the system `curl` (the
//!   standard library has no TLS; the same choice as `cargo xtask cr fetch`). Tests inject a
//!   fake and never touch the network.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// The `curl` arguments for `req`, writing to `dest` (or stdout when `None`): `--fail
/// --location --silent --show-error`, HTTPS only (also on redirects), a size and time limit,
/// the headers; never `--insecure`. curl applies the proxy and CA settings of the environment.
#[must_use]
pub fn curl_args(req: &Request, dest: Option<&Path>) -> Vec<String> {
    let mut a: Vec<String> = [
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    a.extend([
        "--max-filesize".to_owned(),
        req.max_bytes.to_string(),
        "--max-time".to_owned(),
        req.max_time_secs.to_string(),
    ]);
    for (k, v) in &req.headers {
        a.push("--header".to_owned());
        a.push(format!("{k}: {v}"));
    }
    if let Some(d) = dest {
        a.push("--output".to_owned());
        a.push(d.display().to_string());
    }
    a.push("--url".to_owned());
    a.push(req.url.clone());
    a
}

fn run_curl(args: Vec<String>, url: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("curl").args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "curl not found: install curl (fineprint-ingest fetch uses it for HTTPS downloads)"
                .to_owned()
        } else {
            format!("cannot run curl: {e}")
        }
    })?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "curl failed for {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

impl Http for Curl {
    fn get_json(&self, req: &Request) -> Result<Vec<u8>, String> {
        run_curl(curl_args(req, None), &req.url)
    }

    fn download_to_file(&self, req: &Request, dest: &Path) -> Result<(), String> {
        run_curl(curl_args(req, Some(dest)), &req.url).map(|_| ())
    }
}

/// Whether `uri` is an acceptable `download_uri` (R12): `https://`, no credentials, no port, no
/// whitespace, control characters or backslashes, and a host that is `scryfall.com` or ends in
/// `.scryfall.com` or `.scryfall.io` with non-empty labels of `a-z 0-9 -`.
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

fn part_path(dir: &Path, file: &str) -> PathBuf {
    dir.join(format!("{file}.part"))
}

/// Downloads `req` to the `.part` file of `file`, validates it with `check`, and returns the
/// staged result (the `.part` file is removed on any failure).
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

/// Renames the staged files into place and writes the group's `source.json` atomically.
fn commit(dir: &Path, mut meta: SourceMeta, staged: Vec<Staged>) -> Result<(), String> {
    for (i, s) in staged.iter().enumerate() {
        if let Err(e) = fs::rename(&s.part, dir.join(s.file)) {
            discard(staged.get(i..).unwrap_or_default());
            return Err(format!("cannot write the cache in {}: {e}", dir.display()));
        }
    }
    for s in staged {
        meta.insert(s.file.to_owned(), s.meta);
    }
    crate::vocab::cli::write_atomic(&dir.join(SOURCE_FILE), &render_source_meta(&meta))
        .map_err(|e| format!("cannot write {}: {e}", dir.join(SOURCE_FILE).display()))
}

fn scryfall_bulk_check(bytes: &[u8]) -> Result<Option<String>, String> {
    validate_scryfall_bulk(bytes).map(|()| None)
}

/// Runs a fetch at time `now` (seconds since the Unix epoch). `Ok` is the summary line; `Err`
/// is an input or network error (exit 2), after which the cache is as it was before the failing
/// group (a group's files are committed together).
///
/// # Errors
/// A human-readable reason that never quotes downloaded content.
pub fn fetch(http: &dyn Http, opts: &FetchOptions, now: u64) -> Result<String, String> {
    let mut done = Vec::new();
    let sdir = opts.cache.join(SCRYFALL_DIR);
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
        fs::create_dir_all(&sdir).map_err(|e| format!("cannot create {}: {e}", sdir.display()))?;
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
    if opts.mtgjson {
        let mdir = opts.cache.join(MTGJSON_DIR);
        let mmeta = read_source_meta(&mdir);
        if !opts.force && is_fresh(&mmeta, &mdir, ATOMIC_CARDS_FILE, now) {
            done.push("mtgjson fresh".to_owned());
        } else {
            fs::create_dir_all(&mdir)
                .map_err(|e| format!("cannot create {}: {e}", mdir.display()))?;
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
