// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `cargo xtask cr fetch`: download the official CR text into the git-ignored `cr-cache/`
//! (REQ-NFR-007).
//!
//! Cache layout: `cr-cache/rules.txt` (the downloaded bytes, unchanged) and
//! `cr-cache/source.json` (`sourceUrl`, `effectiveDate` as an ISO date, `sha256` of the bytes,
//! `byteCount`). The text is never committed.
//!
//! xtask uses the standard library only, so HTTPS goes through the `curl` program
//! ([`Curl`]): arguments are passed separately (no shell), `--fail --location --silent
//! --show-error --max-time 120`, never `-k`/`--insecure`; curl applies the environment's proxy
//! and CA settings itself. Every download goes to a `.part` file that is renamed into place only
//! after validation, so a failed run never leaves a partial cache. The [`Http`] trait lets the
//! tests inject a fake client.
//!
//! Without `--url`, the URL is discovered from [`RULES_PAGE`]: the first
//! `https://media.wizards.com/...MagicCompRules....txt` link in the HTML, spaces encoded as `%20`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::json::{self, Json};
use super::sha256::sha256_hex;

/// The page that links the current CR text.
pub const RULES_PAGE: &str = "https://magic.wizards.com/en/rules";
/// Prefix of the CR download link.
const LINK_PREFIX: &str = "https://media.wizards.com/";
/// The line that carries the effective date.
const EFFECTIVE: &str = "These rules are effective as of ";

/// A minimal HTTP client: download `url` into the file `dest`.
pub trait Http {
    /// Download `url` to `dest`; an error is a human-readable reason.
    fn download(&self, url: &str, dest: &Path) -> Result<(), String>;
}

/// The real client: runs `curl`.
pub struct Curl;

/// The argument vector passed to `curl` for one download.
pub fn curl_args(url: &str, dest: &Path) -> Vec<String> {
    vec![
        "--fail".to_string(),
        "--location".to_string(),
        "--silent".to_string(),
        "--show-error".to_string(),
        "--max-time".to_string(),
        "120".to_string(),
        "--output".to_string(),
        dest.display().to_string(),
        "--url".to_string(),
        url.to_string(),
    ]
}

impl Http for Curl {
    fn download(&self, url: &str, dest: &Path) -> Result<(), String> {
        let out = Command::new("curl")
            .args(curl_args(url, dest))
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "curl not found: install curl (xtask uses it for HTTPS downloads)".to_string()
                } else {
                    format!("cannot run curl: {e}")
                }
            })?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "curl failed for {url}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

/// The recorded source of the cached text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The URL downloaded.
    pub source_url: String,
    /// ISO effective date.
    pub effective_date: String,
    /// Lowercase hex SHA-256 of the bytes.
    pub sha256: String,
    /// Size in bytes.
    pub byte_count: u64,
}

/// Render `source.json`.
pub fn render_source(s: &Source) -> String {
    format!(
        "{{\n  \"sourceUrl\": {},\n  \"effectiveDate\": {},\n  \"sha256\": {},\n  \"byteCount\": {}\n}}\n",
        json::quote(&s.source_url),
        json::quote(&s.effective_date),
        json::quote(&s.sha256),
        s.byte_count
    )
}

/// Read `source.json`.
pub fn read_source(text: &str) -> Result<Source, String> {
    let doc = json::parse(text)?;
    let field = |k: &str| {
        doc.get(k)
            .and_then(Json::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("missing or non-string `{k}`"))
    };
    let byte_count = doc
        .get("byteCount")
        .and_then(Json::as_num)
        .and_then(|n| n.parse::<u64>().ok())
        .ok_or("missing or invalid `byteCount`")?;
    Ok(Source {
        source_url: field("sourceUrl")?,
        effective_date: field("effectiveDate")?,
        sha256: field("sha256")?,
        byte_count,
    })
}

/// The first CR text link in `html`, with spaces encoded as `%20`.
pub fn discover_url(html: &str) -> Option<String> {
    for (i, _) in html.match_indices(LINK_PREFIX) {
        let rest = &html[i..];
        let end = rest
            .find(['"', '\'', '<', '>', '\n', '\r'])
            .unwrap_or(rest.len());
        let link = rest[..end].trim_end();
        if link.contains("MagicCompRules") && link.ends_with(".txt") {
            return Some(link.replace(' ', "%20"));
        }
    }
    None
}

/// Parse the effective date (`These rules are effective as of <Month D, YYYY>.`) as ISO.
pub fn effective_date(text: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    for line in text.lines() {
        let Some(pos) = line.find(EFFECTIVE) else {
            continue;
        };
        let rest = line[pos + EFFECTIVE.len()..].trim_end();
        let Some(rest) = rest.strip_suffix('.') else {
            continue;
        };
        let mut parts = rest.split(' ');
        let (Some(month), Some(day), Some(year), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Some(m) = MONTHS.iter().position(|n| *n == month) else {
            continue;
        };
        let Some(day) = day.strip_suffix(',') else {
            continue;
        };
        let day_ok = (1..=2).contains(&day.len()) && day.bytes().all(|b| b.is_ascii_digit());
        let year_ok = year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit());
        if !(day_ok && year_ok) {
            continue;
        }
        let d: u32 = day.parse().ok()?;
        if !(1..=31).contains(&d) {
            continue;
        }
        return Some(format!("{year}-{:02}-{d:02}", m + 1));
    }
    None
}

/// Options for [`fetch`].
#[derive(Debug, Clone)]
pub struct FetchOptions {
    /// Explicit text URL (skips discovery).
    pub url: Option<String>,
    /// Re-download even when cached.
    pub force: bool,
    /// The cache directory.
    pub cache: PathBuf,
}

fn download_bytes(http: &dyn Http, url: &str, part: &Path) -> Result<Vec<u8>, String> {
    let result = http.download(url, part).and_then(|()| {
        fs::read(part).map_err(|e| format!("cannot read the download {}: {e}", part.display()))
    });
    let _ = fs::remove_file(part);
    result
}

/// Validate downloaded CR bytes; returns the text's effective date.
pub fn validate(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("the download is empty".to_string());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "the download is not valid UTF-8")?;
    effective_date(text).ok_or_else(|| {
        format!("the download has no `{EFFECTIVE}<Month D, YYYY>.` line (not a CR text?)")
    })
}

/// Run a fetch with `http`. `Ok` is the one-line summary, `Err` an input error (exit 2).
pub fn fetch(http: &dyn Http, opts: &FetchOptions) -> Result<String, String> {
    let rules = opts.cache.join("rules.txt");
    let source = opts.cache.join("source.json");
    if rules.exists() && !opts.force {
        let date = fs::read_to_string(&source)
            .ok()
            .and_then(|t| read_source(&t).ok())
            .map_or_else(|| "unknown".to_string(), |s| s.effective_date);
        return Ok(format!(
            "cr fetch: cached ({}, effective {date}); use --force to download again",
            rules.display()
        ));
    }
    fs::create_dir_all(&opts.cache)
        .map_err(|e| format!("cannot create {}: {e}", opts.cache.display()))?;
    let url = match &opts.url {
        Some(u) => u.clone(),
        None => {
            let page = download_bytes(http, RULES_PAGE, &opts.cache.join("rules-page.html.part"))?;
            discover_url(&String::from_utf8_lossy(&page)).ok_or_else(|| {
                format!("no CR text link found on {RULES_PAGE}; pass the text URL with --url <url>")
            })?
        }
    };
    let part = opts.cache.join("rules.txt.part");
    let bytes = download_bytes(http, &url, &part)?;
    let date = validate(&bytes).map_err(|e| format!("{e} ({url}); the cache is unchanged"))?;
    let src = Source {
        source_url: url,
        effective_date: date,
        sha256: sha256_hex(&bytes),
        byte_count: bytes.len() as u64,
    };
    let source_part = opts.cache.join("source.json.part");
    let write = || -> std::io::Result<()> {
        fs::write(&part, &bytes)?;
        fs::write(&source_part, render_source(&src))?;
        fs::rename(&part, &rules)?;
        fs::rename(&source_part, &source)
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&part);
        let _ = fs::remove_file(&source_part);
        return Err(format!(
            "cannot write the cache in {}: {e}",
            opts.cache.display()
        ));
    }
    Ok(format!(
        "cr fetch: downloaded {} bytes from {} (effective {}, sha256 {})",
        src.byte_count, src.source_url, src.effective_date, src.sha256
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A fake response: the bytes, or `Err` with partial bytes written before a failure.
    pub(crate) type Response = Result<Vec<u8>, Vec<u8>>;

    /// A fake client: url -> response bytes (or an error after writing partial bytes).
    pub(crate) struct Fake {
        pub(crate) responses: BTreeMap<String, Response>,
        pub(crate) calls: RefCell<Vec<String>>,
    }

    impl Fake {
        pub(crate) fn new(list: &[(&str, Response)]) -> Fake {
            Fake {
                responses: list
                    .iter()
                    .map(|(u, r)| ((*u).to_string(), r.clone()))
                    .collect(),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl Http for Fake {
        fn download(&self, url: &str, dest: &Path) -> Result<(), String> {
            self.calls.borrow_mut().push(url.to_string());
            match self.responses.get(url) {
                Some(Ok(b)) => fs::write(dest, b).map_err(|e| e.to_string()),
                Some(Err(partial)) => {
                    let _ = fs::write(dest, partial);
                    Err("simulated network failure".to_string())
                }
                None => Err(format!("no fake response for {url}")),
            }
        }
    }

    /// A unique, cleaned-up temp dir.
    pub(crate) struct TempDir(pub(crate) PathBuf);

    impl TempDir {
        pub(crate) fn new(tag: &str) -> TempDir {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "fineprint-xtask-cr-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const TEXT_URL: &str = "https://media.wizards.com/2099/downloads/MagicCompRules%2020990101.txt";

    fn cr_bytes() -> Vec<u8> {
        "\u{feff}Synthetic rules\r\n\r\nThese rules are effective as of January 1, 2099.\r\n"
            .as_bytes()
            .to_vec()
    }

    fn page() -> Vec<u8> {
        concat!(
            "<a href=\"https://media.wizards.com/2099/other.pdf\">x</a>",
            "<a href=\"https://media.wizards.com/2099/downloads/MagicCompRules 20990101.txt\">t</a>",
            "<a href=\"https://media.wizards.com/2099/downloads/MagicCompRules 2.txt\">u</a>"
        )
        .as_bytes()
        .to_vec()
    }

    fn opts(dir: &Path, url: Option<&str>, force: bool) -> FetchOptions {
        FetchOptions {
            url: url.map(str::to_string),
            force,
            cache: dir.join("cr-cache"),
        }
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .map(|r| {
                r.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// REQ-NFR-007: URL discovery, source.json fields, cached short-circuit and --force.
    #[test]
    fn req_nfr_007_fetch_discover_cache_force() {
        let tmp = TempDir::new("fetch");
        let fake = Fake::new(&[(RULES_PAGE, Ok(page())), (TEXT_URL, Ok(cr_bytes()))]);
        let o = opts(&tmp.0, None, false);
        let msg = fetch(&fake, &o).unwrap();
        assert!(msg.contains("downloaded"), "{msg}");
        assert_eq!(*fake.calls.borrow(), [RULES_PAGE, TEXT_URL]);
        assert_eq!(fs::read(o.cache.join("rules.txt")).unwrap(), cr_bytes());
        let src = read_source(&fs::read_to_string(o.cache.join("source.json")).unwrap()).unwrap();
        assert_eq!(
            src,
            Source {
                source_url: TEXT_URL.to_string(),
                effective_date: "2099-01-01".to_string(),
                sha256: sha256_hex(&cr_bytes()),
                byte_count: cr_bytes().len() as u64,
            }
        );
        assert_eq!(listing(&o.cache), ["rules.txt", "source.json"]);
        // cached: no download
        let msg = fetch(&fake, &o).unwrap();
        assert!(
            msg.contains("cached") && msg.contains("2099-01-01"),
            "{msg}"
        );
        assert_eq!(fake.calls.borrow().len(), 2);
        // --force with --url downloads again, skipping discovery
        let fake2 = Fake::new(&[(TEXT_URL, Ok(cr_bytes()))]);
        fetch(&fake2, &opts(&tmp.0, Some(TEXT_URL), true)).unwrap();
        assert_eq!(*fake2.calls.borrow(), [TEXT_URL]);
        // cached with an unreadable source.json still short-circuits
        fs::write(o.cache.join("source.json"), "{").unwrap();
        assert!(fetch(&fake2, &o).unwrap().contains("unknown"));
    }

    /// REQ-NFR-007: bad downloads are rejected and leave the cache untouched, with no partial
    /// file.
    #[test]
    fn req_nfr_007_fetch_rejections_leave_cache() {
        let tmp = TempDir::new("reject");
        let good = Fake::new(&[(TEXT_URL, Ok(cr_bytes()))]);
        let o = opts(&tmp.0, Some(TEXT_URL), true);
        fetch(&good, &o).unwrap();
        let before = fs::read(o.cache.join("rules.txt")).unwrap();
        let src_before = fs::read(o.cache.join("source.json")).unwrap();
        for (resp, needle) in [
            (Ok(Vec::new()), "empty"),
            (Ok(vec![0xff, 0xfe, 0x00]), "UTF-8"),
            (Ok(b"plain text without a date".to_vec()), "effective"),
            (Err(b"partial bytes".to_vec()), "simulated"),
        ] {
            let fake = Fake::new(&[(TEXT_URL, resp)]);
            let err = fetch(&fake, &o).unwrap_err();
            assert!(err.contains(needle), "{err}");
            assert_eq!(fs::read(o.cache.join("rules.txt")).unwrap(), before);
            assert_eq!(fs::read(o.cache.join("source.json")).unwrap(), src_before);
            assert_eq!(listing(&o.cache), ["rules.txt", "source.json"]);
        }
        // a failed first download leaves no cache at all
        let tmp2 = TempDir::new("reject2");
        let fake = Fake::new(&[(TEXT_URL, Err(b"partial".to_vec()))]);
        let o2 = opts(&tmp2.0, Some(TEXT_URL), false);
        assert!(fetch(&fake, &o2).is_err());
        assert!(listing(&o2.cache).is_empty());
        // discovery failure suggests --url
        let fake = Fake::new(&[(RULES_PAGE, Ok(b"<html>nothing</html>".to_vec()))]);
        let err = fetch(&fake, &opts(&tmp2.0, None, false)).unwrap_err();
        assert!(err.contains("--url"), "{err}");
        let fake = Fake::new(&[]);
        assert!(fetch(&fake, &opts(&tmp2.0, None, false)).is_err());
    }

    /// REQ-NFR-007: link discovery and effective-date parsing.
    #[test]
    fn req_nfr_007_discover_and_date() {
        assert_eq!(
            discover_url(&String::from_utf8(page()).unwrap()).as_deref(),
            Some(TEXT_URL)
        );
        assert_eq!(discover_url("https://media.wizards.com/x.pdf"), None);
        assert_eq!(
            discover_url("'https://media.wizards.com/MagicCompRules.txt'").as_deref(),
            Some("https://media.wizards.com/MagicCompRules.txt")
        );
        let d = |s: &str| effective_date(s);
        assert_eq!(
            d("x\nThese rules are effective as of November 6, 2026.\n").as_deref(),
            Some("2026-11-06")
        );
        assert_eq!(
            d("These rules are effective as of February 28, 2030.\r\n").as_deref(),
            Some("2030-02-28")
        );
        for bad in [
            "These rules are effective as of November 6, 2026",
            "These rules are effective as of Nov 6, 2026.",
            "These rules are effective as of November 6 2026.",
            "These rules are effective as of November 0, 2026.",
            "These rules are effective as of November 32, 2026.",
            "These rules are effective as of November 6, 26.",
            "These rules are effective as of November 123, 2026.",
            "These rules are effective as of November 6, 2026, really.",
            "",
        ] {
            assert_eq!(d(bad), None, "{bad:?}");
        }
    }

    /// REQ-NFR-007: the real curl invocation keeps TLS verification and fails on HTTP errors.
    #[test]
    fn req_nfr_007_curl_args() {
        let args = curl_args("https://example.invalid/a%20b.txt", Path::new("out.part"));
        for need in [
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            "120",
            "--output",
            "out.part",
        ] {
            assert!(args.iter().any(|a| a == need), "{need}");
        }
        assert!(!args.iter().any(|a| a == "-k" || a == "--insecure"));
        assert_eq!(
            args.last().map(String::as_str),
            Some("https://example.invalid/a%20b.txt")
        );
    }

    /// REQ-NFR-007: source.json round trip and errors.
    #[test]
    fn req_nfr_007_source_json() {
        let s = Source {
            source_url: "u\"".to_string(),
            effective_date: "2026-11-06".to_string(),
            sha256: "ab".to_string(),
            byte_count: 7,
        };
        assert_eq!(read_source(&render_source(&s)).unwrap(), s);
        assert!(read_source("{}").is_err());
        assert!(read_source("{\"byteCount\": 1}").is_err());
        assert!(read_source("x").is_err());
    }
}
