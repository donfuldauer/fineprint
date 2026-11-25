// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint-ingest fetch` and `import` (REQ-ING-001, REQ-ING-002,
//! REQ-NFR-004; #87 FP-085). Every record is synthetic and Scryfall- or MTGJSON-shaped, built
//! in code with invented names and text (R5); no `.json` fixture is committed. The fetch tests
//! use the library with a fake `Http` and a fake time: nothing here touches the network, runs
//! curl or reads the clock for a rules value (R7). The CLI paths run through the built binary
//! with a scratch directory as the cache.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use fineprint_base::hash::sha256;
use fineprint_ingest::import::fetch::{
    BULK_DATA_URL, CachedFile, FetchOptions, Http, MTGJSON_ATOMIC_URL, Request, SourceMeta, fetch,
    parse_source_meta, render_source_meta, user_agent,
};
use fineprint_ingest::import::normalize::{
    Diagnostic, NormalizedCardRecord, RejectReason, import_oracle_cards, normalize_card,
};
use fineprint_ingest::import::{DEFAULT_CACHE, cli};
use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_fineprint-ingest");
const T0: u64 = 1_800_000_000;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A new, empty scratch directory created exclusively under Cargo's private
/// `CARGO_TARGET_TMPDIR`. The leaf name combines the pid, a counter and the clock (for
/// uniqueness only, R7); `create_dir` fails if the leaf exists, which is retried.
fn scratch(name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(root).expect("create the scratch root");
    for _ in 0..16 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let leaf = root.join(format!(
            "fp-spec-ingest-import-{name}-{}-{n}-{nanos}",
            std::process::id()
        ));
        match std::fs::create_dir(&leaf) {
            Ok(()) => return leaf,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => panic!("cannot create scratch dir {}: {e}", leaf.display()),
        }
    }
    panic!("no fresh scratch dir under {}", root.display());
}

fn run_in(dir: Option<&Path>, args: &[&str]) -> Output {
    let mut c = Command::new(BIN);
    c.args(args);
    if let Some(d) = dir {
        c.current_dir(d);
    }
    c.output().unwrap()
}

fn run(args: &[&str]) -> Output {
    run_in(None, args)
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// A synthetic oracle id with the lowercase UUID shape.
fn oid(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

/// A synthetic one-face card; `extra` members override the defaults.
fn card(n: u32, extra: Value) -> Value {
    let mut v = json!({
        "object": "card",
        "id": format!("ffffffff-ffff-4fff-8fff-{n:012x}"),
        "oracle_id": oid(n),
        "name": format!("test.card_{n}"),
        "layout": "normal",
        "mana_cost": "{1}{G}",
        "cmc": 2.0,
        "type_line": "Creature \u{2014} Test Folk",
        "oracle_text": "Test ability.",
        "power": "2",
        "toughness": "2",
        "colors": ["G"],
        "color_identity": ["G"],
        "keywords": [],
        "legalities": {"testformat": "legal"},
        "games": ["paper"],
    });
    if let (Value::Object(m), Value::Object(e)) = (&mut v, extra) {
        for (k, x) in e {
            if x == json!("__remove__") {
                m.remove(&k);
            } else {
                m.insert(k, x);
            }
        }
    }
    v
}

fn norm(v: &Value) -> NormalizedCardRecord {
    normalize_card(v).unwrap()
}

fn diags(r: &NormalizedCardRecord) -> Vec<&'static str> {
    r.diagnostics.iter().map(|d| d.as_str()).collect()
}

fn bytes_of(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

/// Writes `<cache>/scryfall/oracle-cards.json` (and rulings, and a matching `source.json`).
fn write_cache(cache: &Path, cards: &Value, rulings: Option<&Value>, with_meta: bool) {
    let dir = cache.join("scryfall");
    std::fs::create_dir_all(&dir).unwrap();
    let mut meta = SourceMeta::new();
    let mut put = |file: &str, v: &Value, updated: &str| {
        let b = bytes_of(v);
        std::fs::write(dir.join(file), &b).unwrap();
        meta.insert(
            file.to_owned(),
            CachedFile {
                source_url: "https://data.scryfall.io/test/file.json".to_owned(),
                updated_at: Some(updated.to_owned()),
                sha256: sha256(&b).to_hex(),
                byte_count: b.len() as u64,
                fetched_at_epoch_seconds: T0,
            },
        );
    };
    put("oracle-cards.json", cards, "2026-01-02T03:04:05.000+00:00");
    if let Some(r) = rulings {
        put("rulings.json", r, "2026-01-02T09:00:00.000+00:00");
    }
    if with_meta {
        std::fs::write(dir.join("source.json"), render_source_meta(&meta)).unwrap();
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn list_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(dir).unwrap().to_path_buf());
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------------------------
// Fetch (library API, fake Http, fake time)
// ---------------------------------------------------------------------------------------------

/// A fake response body, or a failure after writing partial bytes.
type Response = Result<Vec<u8>, Vec<u8>>;

struct Fake {
    routes: BTreeMap<String, Response>,
    calls: RefCell<Vec<Request>>,
}

impl Fake {
    fn new(routes: &[(&str, Response)]) -> Fake {
        Fake {
            routes: routes
                .iter()
                .map(|(u, r)| ((*u).to_owned(), r.clone()))
                .collect(),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn urls(&self) -> Vec<String> {
        self.calls.borrow().iter().map(|r| r.url.clone()).collect()
    }
}

impl Http for Fake {
    fn get_json(&self, req: &Request) -> Result<Vec<u8>, String> {
        self.calls.borrow_mut().push(req.clone());
        match self.routes.get(&req.url) {
            Some(Ok(b)) => Ok(b.clone()),
            _ => Err("simulated failure".to_owned()),
        }
    }

    fn download_to_file(&self, req: &Request, dest: &Path) -> Result<(), String> {
        self.calls.borrow_mut().push(req.clone());
        match self.routes.get(&req.url) {
            Some(Ok(b)) => std::fs::write(dest, b).map_err(|e| e.to_string()),
            Some(Err(partial)) => {
                let _ = std::fs::write(dest, partial);
                Err("simulated network failure".to_owned())
            }
            None => Err("no fake route".to_owned()),
        }
    }
}

const ORACLE_URI: &str = "https://data.scryfall.io/oracle-cards/oracle-cards-test.json";
const RULINGS_URI: &str = "https://data.scryfall.io/rulings/rulings-test.json";

fn bulk_meta(oracle_uri: &str, rulings_uri: &str) -> Vec<u8> {
    bytes_of(&json!({
        "object": "list",
        "has_more": false,
        "data": [
            {"object": "bulk_data", "type": "oracle_cards", "updated_at": "2026-01-02T03:04:05.000+00:00", "download_uri": oracle_uri},
            {"object": "bulk_data", "type": "rulings", "updated_at": "2026-01-02T09:00:00.000+00:00", "download_uri": rulings_uri},
            {"object": "bulk_data", "type": "default_cards", "updated_at": "x", "download_uri": "https://data.scryfall.io/default-cards/d.json"},
        ]
    }))
}

fn oracle_body() -> Vec<u8> {
    bytes_of(&json!([card(1, json!({})), card(2, json!({}))]))
}

fn rulings_body() -> Vec<u8> {
    bytes_of(&json!([
        {"object": "ruling", "oracle_id": oid(1), "source": "wotc", "published_at": "2026-01-01", "comment": "Test ruling one."}
    ]))
}

fn good_fake() -> Fake {
    Fake::new(&[
        (BULK_DATA_URL, Ok(bulk_meta(ORACLE_URI, RULINGS_URI))),
        (ORACLE_URI, Ok(oracle_body())),
        (RULINGS_URI, Ok(rulings_body())),
        (
            MTGJSON_ATOMIC_URL,
            Ok(bytes_of(
                &json!({"meta": {"date": "2026-01-03"}, "data": {}}),
            )),
        ),
    ])
}

fn opts(cache: &Path, force: bool, mtgjson: bool) -> FetchOptions {
    FetchOptions {
        cache: cache.to_path_buf(),
        force,
        mtgjson,
    }
}

fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    list_files(dir)
        .into_iter()
        .map(|f| {
            let b = std::fs::read(dir.join(&f)).unwrap();
            (f, b)
        })
        .collect()
}

/// REQ-ING-001, REQ-NFR-004: the first fetch makes exactly one bulk-data metadata request and
/// downloads the two needed files (never a per-card endpoint, never `default_cards`), sending
/// the required `User-Agent` and `Accept` headers, and records `updatedAt` verbatim, the sha256,
/// the byte count and the fake time.
#[test]
fn fetch_first_run_one_metadata_request_two_downloads() {
    let cache = scratch("fetch-first");
    let http = good_fake();
    let summary = fetch(&http, &opts(&cache, false, false), T0).unwrap();
    assert!(summary.starts_with("fetch: "), "{summary}");
    let urls = http.urls();
    assert_eq!(urls, vec![BULK_DATA_URL, ORACLE_URI, RULINGS_URI]);
    let api_calls = urls
        .iter()
        .filter(|u| u.starts_with("https://api.scryfall.com/"))
        .count();
    assert_eq!(api_calls, 1);
    assert!(!urls.iter().any(|u| u.contains("/cards/")));
    for r in http.calls.borrow().iter() {
        assert!(r.headers.contains(&("User-Agent".to_owned(), user_agent())));
        assert!(user_agent().starts_with("fineprint-ingest/"));
        assert!(
            r.headers
                .contains(&("Accept".to_owned(), "application/json".to_owned()))
        );
        assert!(r.max_bytes > 0 && r.max_time_secs > 0);
    }
    let dir = cache.join("scryfall");
    assert_eq!(
        std::fs::read(dir.join("oracle-cards.json")).unwrap(),
        oracle_body()
    );
    assert_eq!(
        std::fs::read(dir.join("rulings.json")).unwrap(),
        rulings_body()
    );
    let meta = parse_source_meta(&std::fs::read(dir.join("source.json")).unwrap()).unwrap();
    let o = &meta["oracle-cards.json"];
    assert_eq!(
        o.updated_at.as_deref(),
        Some("2026-01-02T03:04:05.000+00:00")
    );
    assert_eq!(o.sha256, sha256(&oracle_body()).to_hex());
    assert_eq!(o.byte_count, oracle_body().len() as u64);
    assert_eq!(o.fetched_at_epoch_seconds, T0);
    assert_eq!(o.source_url, ORACLE_URI);
    let r = &meta["rulings.json"];
    assert_eq!(r.sha256, sha256(&rulings_body()).to_hex());
    assert_eq!(
        r.updated_at.as_deref(),
        Some("2026-01-02T09:00:00.000+00:00")
    );
    assert!(
        !cache.join("mtgjson").exists(),
        "MTGJSON untouched without the flag"
    );
    assert!(
        list_files(&cache)
            .iter()
            .all(|f| !f.to_string_lossy().ends_with(".part"))
    );
}

/// REQ-ING-001: the 24-hour cache. +86399 s makes zero requests (not even the metadata one);
/// +86400 s refetches; `force` refetches inside the window; a `fetchedAt` in the future is
/// stale.
#[test]
fn fetch_respects_the_24_hour_cache() {
    let cache = scratch("fetch-cache");
    fetch(&good_fake(), &opts(&cache, false, false), T0).unwrap();

    let http = good_fake();
    let s = fetch(&http, &opts(&cache, false, false), T0 + 86_399).unwrap();
    assert!(http.urls().is_empty(), "{:?}", http.urls());
    assert!(s.contains("fresh"));

    let http = good_fake();
    fetch(&http, &opts(&cache, true, false), T0 + 10).unwrap();
    assert_eq!(http.urls().len(), 3, "force refetches within the window");

    let http = good_fake();
    fetch(&http, &opts(&cache, false, false), T0 + 10 + 86_400).unwrap();
    assert_eq!(http.urls().len(), 3, "a 24-hour-old file is stale");

    // A future fetchedAt counts as stale.
    let dir = cache.join("scryfall");
    let mut meta = parse_source_meta(&std::fs::read(dir.join("source.json")).unwrap()).unwrap();
    for f in meta.values_mut() {
        f.fetched_at_epoch_seconds = T0 + 1_000_000;
    }
    std::fs::write(dir.join("source.json"), render_source_meta(&meta)).unwrap();
    let http = good_fake();
    fetch(&http, &opts(&cache, false, false), T0 + 200_000).unwrap();
    assert_eq!(http.urls().len(), 3);

    // Only the stale file is downloaded (one metadata request, one download).
    let mut meta = parse_source_meta(&std::fs::read(dir.join("source.json")).unwrap()).unwrap();
    meta.get_mut("rulings.json")
        .unwrap()
        .fetched_at_epoch_seconds = T0;
    std::fs::write(dir.join("source.json"), render_source_meta(&meta)).unwrap();
    let http = good_fake();
    fetch(&http, &opts(&cache, false, false), T0 + 200_000).unwrap();
    assert_eq!(http.urls(), vec![BULK_DATA_URL, RULINGS_URI]);

    // A missing data file is stale even with fresh metadata.
    std::fs::remove_file(dir.join("rulings.json")).unwrap();
    let http = good_fake();
    fetch(&http, &opts(&cache, false, false), T0 + 200_001).unwrap();
    assert_eq!(http.urls(), vec![BULK_DATA_URL, RULINGS_URI]);
}

/// REQ-ING-001, R12: `download_uri` is untrusted. Non-https, foreign or look-alike hosts,
/// credentials and ports are rejected before any download and the cache is untouched.
#[test]
fn fetch_rejects_untrusted_download_uris() {
    let cache = scratch("fetch-uri");
    fetch(&good_fake(), &opts(&cache, false, false), T0).unwrap();
    let before = snapshot(&cache);
    for bad in [
        "http://data.scryfall.io/oracle.json",
        "https://scryfall.com.evil.example/oracle.json",
        "https://evilscryfall.io/oracle.json",
        "https://evil.example/oracle.json",
        "https://user:pw@data.scryfall.io/oracle.json",
        "https://data.scryfall.io@evil.example/oracle.json",
        "https://data.scryfall.io:8443/oracle.json",
        "https://DATA.SCRYFALL.IO/oracle.json",
        "https://.scryfall.io/oracle.json",
        "ftp://data.scryfall.io/oracle.json",
        "https://data.scryfall.io\\@evil.example/x",
        "",
    ] {
        let http = Fake::new(&[
            (BULK_DATA_URL, Ok(bulk_meta(bad, RULINGS_URI))),
            (bad, Ok(oracle_body())),
            (RULINGS_URI, Ok(rulings_body())),
        ]);
        let err = fetch(&http, &opts(&cache, true, false), T0 + 5).unwrap_err();
        assert!(err.contains("unchanged"), "{err}");
        assert_eq!(http.urls(), vec![BULK_DATA_URL], "{bad}: no download");
        assert_eq!(snapshot(&cache), before, "{bad}: cache untouched");
    }
    for good in [
        "https://scryfall.com/x.json",
        "https://c2.scryfall.com/x.json",
        "https://data.scryfall.io/x.json",
    ] {
        let http = Fake::new(&[
            (BULK_DATA_URL, Ok(bulk_meta(good, RULINGS_URI))),
            (good, Ok(oracle_body())),
            (RULINGS_URI, Ok(rulings_body())),
        ]);
        fetch(&http, &opts(&cache, true, false), T0 + 5).unwrap();
    }
}

/// REQ-ING-001, REQ-NFR-004: a failed or invalid download (empty, invalid UTF-8, not a JSON
/// array, malformed, a failing client, a bad metadata document) leaves the previous cache
/// byte-identical and no `.part` file behind.
#[test]
fn fetch_failed_download_leaves_cache_untouched() {
    let cache = scratch("fetch-fail");
    fetch(&good_fake(), &opts(&cache, false, false), T0).unwrap();
    let before = snapshot(&cache);
    let bodies: Vec<Response> = vec![
        Ok(Vec::new()),
        Ok(vec![0xff, 0xfe, 0x5b, 0x5d]),
        Ok(b"{\"data\": []}".to_vec()),
        Ok(b"[{\"a\": 1},".to_vec()),
        Ok(b"[1,,2]".to_vec()),
        Ok(b"[1] trailing".to_vec()),
        Err(b"[{\"partial".to_vec()),
    ];
    for body in bodies {
        // The rulings download fails: the oracle file already staged must not land either.
        let http = Fake::new(&[
            (BULK_DATA_URL, Ok(bulk_meta(ORACLE_URI, RULINGS_URI))),
            (ORACLE_URI, Ok(oracle_body())),
            (RULINGS_URI, body.clone()),
        ]);
        let err = fetch(&http, &opts(&cache, true, false), T0 + 7).unwrap_err();
        assert!(err.contains("unchanged"), "{err}");
        assert_eq!(snapshot(&cache), before, "{body:?}");
    }
    for meta in [
        b"not json".to_vec(),
        bytes_of(&json!({"data": [{"type": "oracle_cards"}]})),
        bytes_of(
            &json!({"data": [{"type": "oracle_cards", "download_uri": ORACLE_URI, "updated_at": "x"}]}),
        ),
        bytes_of(&json!({"object": "list"})),
    ] {
        let http = Fake::new(&[(BULK_DATA_URL, Ok(meta))]);
        assert!(fetch(&http, &opts(&cache, true, false), T0 + 7).is_err());
        assert_eq!(snapshot(&cache), before);
    }
    let http = Fake::new(&[]);
    assert!(fetch(&http, &opts(&cache, true, false), T0 + 7).is_err());
    assert_eq!(snapshot(&cache), before);
}

/// REQ-ING-001: MTGJSON is contacted only with `--mtgjson`; its body must be an object with a
/// `data` object; `meta.date` becomes `updatedAt`.
#[test]
fn fetch_mtgjson_only_with_flag() {
    let cache = scratch("fetch-mtgjson");
    let http = good_fake();
    fetch(&http, &opts(&cache, false, false), T0).unwrap();
    assert!(!http.urls().iter().any(|u| u.contains("mtgjson")));

    let http = good_fake();
    fetch(&http, &opts(&cache, false, true), T0 + 1).unwrap();
    assert_eq!(http.urls(), vec![MTGJSON_ATOMIC_URL], "scryfall is fresh");
    let meta =
        parse_source_meta(&std::fs::read(cache.join("mtgjson/source.json")).unwrap()).unwrap();
    assert_eq!(
        meta["AtomicCards.json"].updated_at.as_deref(),
        Some("2026-01-03")
    );
    let http = good_fake();
    fetch(&http, &opts(&cache, false, true), T0 + 2).unwrap();
    assert!(http.urls().is_empty(), "both groups fresh");

    // An invalid AtomicCards body: the Scryfall group is fresh, the MTGJSON group untouched.
    let cache = scratch("fetch-mtgjson-bad");
    fetch(&good_fake(), &opts(&cache, false, false), T0).unwrap();
    let before = snapshot(&cache);
    for body in [
        b"[]".to_vec(),
        b"{\"meta\": {}}".to_vec(),
        b"{\"data\": []}".to_vec(),
        Vec::new(),
    ] {
        let http = Fake::new(&[(MTGJSON_ATOMIC_URL, Ok(body))]);
        let err = fetch(&http, &opts(&cache, false, true), T0 + 5).unwrap_err();
        assert!(err.contains("unchanged"), "{err}");
        assert_eq!(http.urls(), vec![MTGJSON_ATOMIC_URL]);
        assert_eq!(snapshot(&cache), before);
    }
}

/// REQ-ING-001: the `fetch` command line with an injected client and time: usage errors exit 2
/// before any request, success exits 0 with one summary line, a failure exits 2.
#[test]
fn fetch_cli_with_injected_client() {
    let cache = scratch("fetch-cli");
    let os = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
    for bad in [
        vec!["--cache"],
        vec!["--cache", "--force"],
        vec!["--bogus"],
        vec!["--cache", "a", "--cache", "b"],
    ] {
        let http = good_fake();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            cli::run_fetch_with(&os(&bad), &http, T0, &mut out, &mut err),
            2
        );
        assert!(http.urls().is_empty());
        assert!(String::from_utf8_lossy(&err).contains("usage:"));
    }
    let http = good_fake();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = os(&["--cache", p(&cache), "--mtgjson"]);
    assert_eq!(cli::run_fetch_with(&args, &http, T0, &mut out, &mut err), 0);
    assert_eq!(String::from_utf8_lossy(&out).lines().count(), 1);
    let http = Fake::new(&[]);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = os(&["--cache", p(&cache), "--force"]);
    assert_eq!(cli::run_fetch_with(&args, &http, T0, &mut out, &mut err), 2);
    // The binary's usage errors never reach the network either.
    assert_eq!(code(&run(&["fetch", "--bogus"])), 2);
    assert_eq!(code(&run(&["fetch", "--cache"])), 2);
}

// ---------------------------------------------------------------------------------------------
// Import through the binary
// ---------------------------------------------------------------------------------------------

fn import(cache: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["import", "--cache", p(cache)];
    args.extend_from_slice(extra);
    run(&args)
}

fn output(cache: &Path) -> Value {
    read_json(&cache.join("normalized/cards.json"))
}

/// REQ-ING-001: records are keyed by `oracle_id` and sorted whatever the input order; a
/// duplicate is kept once and reported; an invalid id shape and a missing id are rejected; the
/// output is byte-identical across two runs (R3).
#[test]
fn import_keys_order_duplicates_and_rejections() {
    let cache = scratch("import-keys");
    let cards = json!([
        card(3, json!({})),
        card(1, json!({})),
        card(2, json!({})),
        card(1, json!({"name": "test.duplicate"})),
        card(
            4,
            json!({"oracle_id": "00000000-0000-4000-8000-00000000000G"})
        ),
        card(
            5,
            json!({"oracle_id": "00000000-0000-4000-8000-0000000000AB"})
        ),
        card(6, json!({"oracle_id": "__remove__"})),
        card(7, json!({"oracle_id": 7})),
    ]);
    write_cache(&cache, &cards, None, true);
    let o = import(&cache, &[]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let first = std::fs::read(cache.join("normalized/cards.json")).unwrap();
    let v = output(&cache);
    let ids: Vec<&str> = v["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["cardDefId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![oid(1), oid(2), oid(3)]);
    assert_eq!(v["records"][0]["name"], "test.card_1", "the first is kept");
    assert_eq!(
        v["rejected"],
        json!([
            {"index": 3, "oracleId": oid(1), "reason": "duplicateOracleId"},
            {"index": 4, "oracleId": null, "reason": "oracleIdShape"},
            {"index": 5, "oracleId": null, "reason": "oracleIdShape"},
            {"index": 6, "oracleId": null, "reason": "notGameplayObject"},
            {"index": 7, "oracleId": null, "reason": "fieldType:oracle_id"},
        ])
    );
    assert_eq!(v["formatVersion"], 1);
    let o = import(&cache, &[]);
    assert_eq!(code(&o), 0);
    assert_eq!(
        std::fs::read(cache.join("normalized/cards.json")).unwrap(),
        first
    );
    let text = String::from_utf8(first).unwrap();
    let order: Vec<usize> = [
        "{\"formatVersion\":",
        "\n\"sources\":",
        "\n\"records\":",
        "\n\"rulings\":",
        "\n\"rejected\":",
        "\n\"warnings\":",
    ]
    .iter()
    .map(|k| text.find(k).unwrap())
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "fixed member order");
}

/// REQ-ING-001: the output records each source's `updatedAt`, recomputed sha256 and byte
/// count; a tampered cache file yields a warning and the recomputed hash; a missing
/// `source.json` is a warning, not an error.
#[test]
fn import_records_source_provenance() {
    let cache = scratch("import-prov");
    let cards = json!([card(1, json!({}))]);
    let rulings = json!([]);
    write_cache(&cache, &cards, Some(&rulings), true);
    assert_eq!(code(&import(&cache, &[])), 0);
    let v = output(&cache);
    let s = &v["sources"]["oracleCards"];
    assert_eq!(s["updatedAt"], "2026-01-02T03:04:05.000+00:00");
    assert_eq!(s["sha256"], sha256(&bytes_of(&cards)).to_hex());
    assert_eq!(s["byteCount"], bytes_of(&cards).len());
    assert_eq!(
        v["sources"]["rulings"]["updatedAt"],
        "2026-01-02T09:00:00.000+00:00"
    );
    assert_eq!(v["warnings"], json!([]));

    // Tamper with the cached file after its metadata was written.
    let tampered = bytes_of(&json!([card(1, json!({})), card(2, json!({}))]));
    std::fs::write(cache.join("scryfall/oracle-cards.json"), &tampered).unwrap();
    assert_eq!(code(&import(&cache, &[])), 0);
    let v = output(&cache);
    assert_eq!(
        v["sources"]["oracleCards"]["sha256"],
        sha256(&tampered).to_hex()
    );
    assert_eq!(v["warnings"], json!(["sourceHashMismatch:oracleCards"]));

    std::fs::remove_file(cache.join("scryfall/source.json")).unwrap();
    assert_eq!(code(&import(&cache, &[])), 0);
    let v = output(&cache);
    assert_eq!(v["sources"]["oracleCards"]["updatedAt"], Value::Null);
    assert_eq!(v["warnings"], json!(["sourceMetadataMissing"]));
    assert_eq!(
        v["sources"]["rulings"]["sha256"],
        sha256(&bytes_of(&rulings)).to_hex()
    );
}

// ---------------------------------------------------------------------------------------------
// Field mapping and recomputation (library API)
// ---------------------------------------------------------------------------------------------

fn two_face() -> Value {
    json!({
        "object": "card",
        "id": "ffffffff-ffff-4fff-8fff-0000000000aa",
        "oracle_id": oid(10),
        "name": "test.front // test.back",
        "layout": "transform",
        "cmc": 3.0,
        "type_line": "Creature \u{2014} Test Folk // Legendary Planeswalker \u{2014} Testwalker",
        "color_identity": ["G", "W"],
        "keywords": ["Zeta", "Alpha"],
        "legalities": {"testformat": "legal", "otherformat": "not_legal"},
        "games": ["arena"],
        "all_parts": [
            {"object": "related_card", "id": "ffffffff-ffff-4fff-8fff-0000000000bb", "component": "token", "name": "test.token", "uri": "https://api.scryfall.com/cards/x"}
        ],
        "card_faces": [
            {"object": "card_face", "name": "test.front", "mana_cost": "{2}{G}", "type_line": "Creature \u{2014} Test Folk", "oracle_text": "Test ability.", "power": "*", "toughness": "1+*", "colors": ["G"]},
            {"object": "card_face", "name": "test.back", "mana_cost": "", "type_line": "Legendary Planeswalker \u{2014} Testwalker", "oracle_text": "Test text. {T}: Add {W}.", "loyalty": "X", "color_indicator": ["W"], "colors": ["W"]}
        ]
    })
}

/// REQ-ING-002: the SRS §16.3 mapping for a two-face card: per-face names, parsed costs and raw
/// cost text, type-line split, raw Oracle text, P/T/loyalty kept as strings, color masks,
/// computed colors, identity over both faces, front-face mana value, sorted keywords, raw
/// legalities and all_parts (id/component/name only), digitalOnly, gameChanger default, the
/// Oracle text hash; the printing `id` is never used.
#[test]
fn mapping_two_face_card() {
    let r = norm(&two_face());
    assert_eq!(r.card_def_id, oid(10));
    let j = r.to_json_value();
    assert_eq!(
        j,
        json!({
            "cardDefId": oid(10),
            "name": "test.front // test.back",
            "layout": "transform",
            "faces": [
                {"name": "test.front", "manaCost": "{2}{G}", "manaCostText": "{2}{G}", "supertypes": [], "cardTypes": ["Creature"], "subtypes": ["Test", "Folk"], "oracleText": "Test ability.", "power": "*", "toughness": "1+*", "loyalty": null, "defense": null, "colors": 16, "colorIndicator": null, "computedColors": 16},
                {"name": "test.back", "manaCost": null, "manaCostText": "", "supertypes": ["Legendary"], "cardTypes": ["Planeswalker"], "subtypes": ["Testwalker"], "oracleText": "Test text. {T}: Add {W}.", "power": null, "toughness": null, "loyalty": "X", "defense": null, "colors": 1, "colorIndicator": 1, "computedColors": 1}
            ],
            "colorIdentity": 17,
            "sourceColorIdentity": 17,
            "manaValue": 3,
            "keywords": ["Alpha", "Zeta"],
            "allParts": [{"id": "ffffffff-ffff-4fff-8fff-0000000000bb", "component": "token", "name": "test.token"}],
            "legalities": {"otherformat": "not_legal", "testformat": "legal"},
            "gameChanger": false,
            "digitalOnly": true,
            "oracleTextHash": sha256(b"Test ability.\nTest text. {T}: Add {W}.").to_hex(),
            "diagnostics": [],
        })
    );
    let text = serde_json::to_string(&j).unwrap();
    assert!(
        !text.contains("0000000000aa"),
        "the printing id is not used"
    );
    assert!(j["manaValue"].is_u64(), "an integer");
}

/// REQ-ING-002: a one-face card without `card_faces`, a land with an empty cost (no
/// diagnostic), a non-land normal card with an empty cost (`emptyManaCostUnexpected`), and the
/// `digitalOnly` and `gameChanger` rules.
#[test]
fn mapping_one_face_land_and_flags() {
    let r = norm(&card(1, json!({"defense": "4", "game_changer": true})));
    assert_eq!(r.faces.len(), 1);
    let f = &r.faces[0];
    assert_eq!(f.name, "test.card_1");
    assert_eq!(f.power.as_deref(), Some("2"));
    assert_eq!(f.defense.as_deref(), Some("4"));
    assert_eq!(r.mana_value, 2);
    assert!(r.game_changer);
    assert!(!r.digital_only, "games [paper]");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    assert_eq!(r.oracle_text_hash, sha256(b"Test ability.").to_hex());

    let land = norm(&card(
        2,
        json!({"mana_cost": "", "cmc": 0.0, "type_line": "Land", "colors": [], "color_identity": [], "power": "__remove__", "toughness": "__remove__"}),
    ));
    assert!(land.diagnostics.is_empty(), "{:?}", land.diagnostics);
    assert_eq!(land.faces[0].mana_cost, None);
    assert_eq!(land.faces[0].mana_cost_text.as_deref(), Some(""));
    assert_eq!(land.faces[0].power, None);

    let odd = norm(&card(
        3,
        json!({"mana_cost": "", "cmc": 0.0, "colors": [], "color_identity": []}),
    ));
    assert_eq!(diags(&odd), vec!["emptyManaCostUnexpected"]);
    let absent = norm(&card(
        4,
        json!({"mana_cost": "__remove__", "cmc": 0.0, "colors": [], "color_identity": []}),
    ));
    assert_eq!(diags(&absent), vec!["emptyManaCostUnexpected"]);
    assert_eq!(absent.faces[0].mana_cost_text, None);

    assert!(norm(&card(5, json!({"digital": true}))).digital_only);
    assert!(norm(&card(6, json!({"games": ["arena"]}))).digital_only);
    assert!(norm(&card(7, json!({"games": ["arena", "mtgo"]}))).digital_only);
    assert!(!norm(&card(8, json!({"games": ["paper", "arena"]}))).digital_only);
    assert!(!norm(&card(9, json!({"games": []}))).digital_only);
    assert!(!norm(&card(10, json!({"games": "__remove__"}))).digital_only);
    assert!(!norm(&card(11, json!({"digital": false, "games": ["paper"]}))).digital_only);
    assert!(!norm(&card(12, json!({"game_changer": "__remove__"}))).game_changer);
}

fn mv_of(cost: &str) -> u64 {
    norm(&card(1, json!({"mana_cost": cost, "cmc": "__remove__"}))).mana_value
}

fn colors_of(cost: &str) -> u8 {
    norm(&card(1, json!({"mana_cost": cost}))).faces[0]
        .computed_colors
        .bits()
}

/// REQ-ING-002 (CR 202.3, 202.3a, 202.3e, 202.3f, 202.3g; colors CR 202.2, 202.2d): the
/// recomputed mana value and colors of every symbol kind.
#[test]
fn recompute_every_symbol_kind() {
    for (cost, mv) in [
        ("{2}{W}", 3),
        ("{0}", 0),
        ("", 0),
        ("{X}{X}{R}", 1),
        ("{X}{Y}{Z}", 0),
        ("{2/W}{2/U}", 4),
        ("{W/U}", 1),
        ("{G/P}", 1),
        ("{W/U/P}", 1),
        ("{S}", 1),
        ("{C}", 1),
        ("{10}", 10),
    ] {
        assert_eq!(mv_of(cost), mv, "{cost}");
    }
    for (cost, bits) in [
        ("{W/U}", 1 | 2),
        ("{2/G}", 16),
        ("{R/P}", 8),
        ("{B/G/P}", 4 | 16),
        ("{C}", 0),
        ("{S}", 0),
        ("{X}", 0),
        ("{3}", 0),
        ("", 0),
        ("{W}{U}{B}{R}{G}", 31),
    ] {
        assert_eq!(colors_of(cost), bits, "{cost}");
    }
    // A color indicator replaces the cost colors of a face.
    let r = norm(&card(
        1,
        json!({"mana_cost": "{R}", "cmc": 1.0, "color_indicator": ["U"], "colors": ["U"], "color_identity": ["U"]}),
    ));
    assert_eq!(r.faces[0].computed_colors.bits(), 2);
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
}

fn faced(layout: &str, costs: [&str; 2], cmc: u64) -> NormalizedCardRecord {
    norm(&json!({
        "oracle_id": oid(20),
        "name": "test.a // test.b",
        "layout": layout,
        "cmc": cmc,
        "card_faces": [
            {"name": "test.a", "mana_cost": costs[0], "type_line": "Instant"},
            {"name": "test.b", "mana_cost": costs[1], "type_line": "Sorcery"},
        ]
    }))
}

/// REQ-ING-002 (CR 202.3b, 202.3c, 202.3d): the record-level mana value by layout: `split`
/// sums both halves; `transform`, `modal_dfc`, `meld` and `reversible_card` use the front face;
/// `adventure` and `flip` the first face.
#[test]
fn recompute_mana_value_by_layout() {
    let split = faced("split", ["{1}{R}", "{3}{U}{U}"], 7);
    assert_eq!(split.mana_value, 7);
    assert!(split.diagnostics.is_empty(), "{:?}", split.diagnostics);
    for layout in [
        "transform",
        "modal_dfc",
        "meld",
        "reversible_card",
        "test_layout",
    ] {
        let r = faced(layout, ["{1}{R}", "{3}{U}{U}"], 2);
        assert_eq!(r.mana_value, 2, "{layout}");
        assert!(!diags(&r).contains(&"manaValueMismatch"), "{layout}");
    }
    for layout in ["adventure", "flip"] {
        assert_eq!(faced(layout, ["{4}{G}", "{1}{G}"], 5).mana_value, 5);
    }
    // Back face without a cost: the front face counts.
    assert_eq!(faced("transform", ["{2}{B}", ""], 3).mana_value, 3);
}

/// REQ-ING-002: the float `cmc` is validation only: an equal value gives no flag, a different
/// one `manaValueMismatch`, half values use the `*2` integer rule, a missing or non-numeric
/// `cmc` is `cmcMissing`; `cmc` never appears in the record and `manaValue` is an integer.
#[test]
fn cmc_is_validation_only() {
    let with = |cost: &str, cmc: Value| {
        norm(&card(
            1,
            json!({"mana_cost": cost, "cmc": cmc, "colors": [], "color_identity": []}),
        ))
    };
    assert!(with("{3}", json!(3.0)).diagnostics.is_empty());
    assert!(with("{3}", json!(3)).diagnostics.is_empty());
    assert_eq!(diags(&with("{3}", json!(2.0))), vec!["manaValueMismatch"]);
    assert_eq!(diags(&with("{3}", json!(3.5))), vec!["manaValueMismatch"]);
    assert_eq!(diags(&with("{3}", json!(2.9))), vec!["manaValueMismatch"]);
    assert_eq!(diags(&with("{0}", json!(0.5))), vec!["manaValueMismatch"]);
    assert_eq!(diags(&with("{0}", json!(-1.0))), vec!["manaValueMismatch"]);
    assert_eq!(diags(&with("{0}", json!(1e300))), vec!["manaValueMismatch"]);
    for bad in [
        json!("NaN"),
        json!("3"),
        json!(null),
        json!([3]),
        json!("__remove__"),
    ] {
        let r = with("{3}", bad.clone());
        assert_eq!(diags(&r), vec!["cmcMissing"], "{bad}");
        assert_eq!(r.mana_value, 3);
    }
    let r = with("{3}", json!(2.0));
    assert_eq!(
        r.mana_value, 3,
        "a mismatch never changes the recomputed value"
    );
    let j = r.to_json_value();
    assert!(j["manaValue"].is_u64());
    assert!(j.get("cmc").is_none());
    let text = serde_json::to_string(&j).unwrap();
    assert!(!text.contains("2.0"), "{text}");
}

/// REQ-ING-002 (CR 903.4, 903.4c, 903.4d): color identity from cost symbols, the indicator, mana
/// symbols in rules text (not in reminder text, nested or unclosed parentheses included) and the
/// back face; `colorIdentityMismatch` and `colorsMismatch` never change the recomputed values.
#[test]
fn color_identity_and_cross_checks() {
    let id = |text: &str| {
        norm(&card(1, json!({"mana_cost": "{1}", "oracle_text": text, "colors": [], "color_identity": "__remove__"})))
            .color_identity
            .bits()
    };
    assert_eq!(id("Test. {T}: Add {G}."), 16);
    assert_eq!(id("Test (reminder {T}: Add {G}.)"), 0);
    assert_eq!(id("Test (outer (inner) still {R}) and {U}."), 2);
    assert_eq!(id("Test (unclosed {W} {B}"), 0);
    assert_eq!(id("Test) stray close {B}"), 4);
    assert_eq!(
        id("{W/U} and {2/R} and {B/P} and {Q} and {T} and {"),
        1 | 2 | 8 | 4
    );
    assert_eq!(id("\u{2014}\u{1F600}{G}\u{00e9}{"), 16);
    assert_eq!(id(""), 0);
    let ind = norm(&card(
        1,
        json!({"mana_cost": "", "cmc": 0.0, "type_line": "Land", "color_indicator": ["B"], "colors": ["B"], "color_identity": ["B"]}),
    ));
    assert_eq!(ind.color_identity.bits(), 4);
    assert!(ind.diagnostics.is_empty(), "{:?}", ind.diagnostics);

    let r = norm(&two_face());
    assert_eq!(r.color_identity.bits(), 16 | 1, "the back face counts");

    let mut v = two_face();
    v["color_identity"] = json!(["G"]);
    let r = norm(&v);
    assert_eq!(diags(&r), vec!["colorIdentityMismatch"]);
    assert_eq!(r.color_identity.bits(), 17, "recomputed value unchanged");
    assert_eq!(r.source_color_identity.map(|m| m.bits()), Some(16));

    let r = norm(&card(1, json!({"colors": ["R"]})));
    assert_eq!(diags(&r), vec!["colorsMismatch"]);
    assert_eq!(r.faces[0].computed_colors.bits(), 16);
    assert_eq!(r.faces[0].colors.map(|m| m.bits()), Some(8));

    let mut v = two_face();
    v["card_faces"][0]["colors"] = json!(["U"]);
    assert_eq!(diags(&norm(&v)), vec!["colorsMismatch"]);
}

/// REQ-ING-002: the type line splits on the em dash into supertypes, card types and subtypes
/// (a split only); no dash gives no subtypes; a missing or empty line is `typeLineEmpty`; a
/// combined `A // B` line is split per face when the counts agree, else
/// `typeLineFaceMismatch`.
#[test]
fn type_line_split() {
    let r = norm(&card(
        1,
        json!({"type_line": "Legendary Test Creature \u{2014} Test Folk"}),
    ));
    let f = &r.faces[0];
    assert_eq!(f.supertypes, vec!["Legendary"]);
    assert_eq!(f.card_types, vec!["Test", "Creature"]);
    assert_eq!(f.subtypes, vec!["Test", "Folk"]);
    let r = norm(&card(1, json!({"type_line": "Basic Snow Land"})));
    assert_eq!(r.faces[0].supertypes, vec!["Basic", "Snow"]);
    assert_eq!(r.faces[0].card_types, vec!["Land"]);
    assert!(r.faces[0].subtypes.is_empty());
    let r = norm(&card(1, json!({"type_line": "Creature - Test Folk"})));
    assert_eq!(
        r.faces[0].card_types,
        vec!["Creature", "-", "Test", "Folk"],
        "a hyphen is no dash"
    );
    for line in [json!(""), json!("   "), json!("__remove__")] {
        let r = norm(&card(1, json!({"type_line": line})));
        assert!(diags(&r).contains(&"typeLineEmpty"), "{line}");
    }

    let combined = |line: &str, faces: usize| {
        let fs: Vec<Value> = (0..faces)
            .map(|i| json!({"name": format!("test.f{i}"), "mana_cost": "{1}"}))
            .collect();
        norm(
            &json!({"oracle_id": oid(30), "name": "test.combined", "layout": "split", "type_line": line, "card_faces": fs, "cmc": 2.0}),
        )
    };
    let r = combined("Instant // Legendary Sorcery \u{2014} Test", 2);
    assert_eq!(r.faces[0].card_types, vec!["Instant"]);
    assert_eq!(r.faces[1].supertypes, vec!["Legendary"]);
    assert_eq!(r.faces[1].card_types, vec!["Sorcery"]);
    assert_eq!(r.faces[1].subtypes, vec!["Test"]);
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let r = combined("Instant // Sorcery // Instant", 2);
    assert!(diags(&r).contains(&"typeLineFaceMismatch"));
    let r = combined("Instant", 2);
    assert_eq!(r.faces[1].card_types, vec!["Instant"]);
    // A face's own type line wins over the combined line.
    let mut v = two_face();
    v["type_line"] = json!("Instant // Sorcery // Instant");
    assert!(norm(&v).diagnostics.is_empty());
}

const MARK: &str = "SECRETMARK";

/// REQ-ING-002, R12: wrong-type fields reject only that record with a fixed reason naming the
/// field; the other records still import; reasons never contain input text.
#[test]
fn wrong_types_reject_only_that_record() {
    let cases: Vec<(Value, &str)> = vec![
        (card(2, json!({"colors": MARK})), "fieldType:colors"),
        (card(3, json!({"colors": [MARK]})), "unknownColor:colors"),
        (card(4, json!({"colors": ["W", 1]})), "fieldType:colors"),
        (card(5, json!({"mana_cost": 3})), "fieldType:mana_cost"),
        (
            card(6, json!({"legalities": [MARK]})),
            "fieldType:legalities",
        ),
        (
            card(7, json!({"legalities": {"x": 1}})),
            "fieldType:legalities",
        ),
        (json!(MARK), "notObject"),
        (json!([MARK]), "notObject"),
        (card(8, json!({"mana_cost": "{Q}"})), "manaCostParse"),
        (
            card(9, json!({"mana_cost": format!("{{{MARK}}}")})),
            "manaCostParse",
        ),
        (
            card(10, json!({"color_identity": ["X"]})),
            "unknownColor:color_identity",
        ),
        (
            card(11, json!({"color_indicator": "W"})),
            "fieldType:color_indicator",
        ),
        (card(12, json!({"keywords": [1]})), "fieldType:keywords"),
        (card(13, json!({"name": 1})), "fieldType:name"),
        (card(14, json!({"name": "__remove__"})), "missingField:name"),
        (
            card(15, json!({"layout": "__remove__"})),
            "missingField:layout",
        ),
        (
            card(16, json!({"card_faces": MARK})),
            "fieldType:card_faces",
        ),
        (card(17, json!({"card_faces": []})), "fieldType:card_faces"),
        (
            card(18, json!({"card_faces": [MARK]})),
            "fieldType:card_faces",
        ),
        (
            card(19, json!({"card_faces": [{"name": 2}]})),
            "fieldType:card_faces.name",
        ),
        (
            card(20, json!({"card_faces": [{"mana_cost": "{1}"}]})),
            "missingField:card_faces.name",
        ),
        (
            card(21, json!({"card_faces": [{"name": "x", "colors": ["Q"]}]})),
            "unknownColor:card_faces.colors",
        ),
        (card(22, json!({"all_parts": MARK})), "fieldType:all_parts"),
        (
            card(23, json!({"all_parts": [MARK]})),
            "fieldType:all_parts",
        ),
        (
            card(24, json!({"all_parts": [{"id": 5}]})),
            "fieldType:all_parts.id",
        ),
        (card(25, json!({"digital": MARK})), "fieldType:digital"),
        (
            card(26, json!({"game_changer": 1})),
            "fieldType:game_changer",
        ),
        (card(27, json!({"games": [1]})), "fieldType:games"),
        (
            card(28, json!({"oracle_text": {"x": MARK}})),
            "fieldType:oracle_text",
        ),
        (card(29, json!({"power": 2})), "fieldType:power"),
        (
            card(30, json!({"type_line": ["Land"]})),
            "fieldType:type_line",
        ),
        (card(31, json!({"colors": ["WU"]})), "unknownColor:colors"),
    ];
    let mut arr = vec![card(1, json!({}))];
    arr.extend(cases.iter().map(|(v, _)| v.clone()));
    arr.push(card(99, json!({})));
    let out = import_oracle_cards(&bytes_of(&Value::Array(arr))).unwrap();
    assert_eq!(
        out.records.keys().cloned().collect::<Vec<_>>(),
        vec![oid(1), oid(99)]
    );
    assert_eq!(out.rejected.len(), cases.len());
    for (i, ((_, reason), rej)) in cases.iter().zip(&out.rejected).enumerate() {
        assert_eq!(rej.index, i + 1);
        assert_eq!(rej.reason.as_string(), *reason, "case {}", i + 1);
        assert!(!rej.to_json_value().to_string().contains(MARK));
    }
    assert_eq!(out.rejected[0].oracle_id.as_deref(), Some(oid(2).as_str()));
    assert_eq!(
        normalize_card(&json!(1)).unwrap_err(),
        RejectReason::NotObject
    );
    // An element that is not JSON at all inside a well-formed array.
    let out = import_oracle_cards(b"[{\"a\": tru}, 5]").unwrap();
    assert_eq!(out.rejected.len(), 2);
    assert!(import_oracle_cards(b"{\"not\": \"array\"}").is_err());
    assert_eq!(Diagnostic::TypeLineEmpty.as_str(), "typeLineEmpty");
}

/// REQ-ING-002, REQ-ING-001: rulings are attached by `oracle_id` in source order under the
/// separate `rulings` member; unknown ids and invalid elements are counted as warnings; an
/// absent rulings file is fine.
#[test]
fn rulings_attach_in_source_order() {
    let cache = scratch("import-rulings");
    let cards = json!([card(1, json!({})), card(2, json!({}))]);
    let ruling = |id: String, c: &str| json!({"object": "ruling", "oracle_id": id, "source": "wotc", "published_at": "2026-01-01", "comment": c});
    let rulings = json!([
        ruling(oid(2), "Test ruling B1."),
        ruling(oid(1), "Test ruling A1."),
        ruling(oid(2), "Test ruling B2."),
        ruling(oid(77), "Test ruling orphan."),
        {"oracle_id": oid(1), "source": 1},
        "not a ruling",
    ]);
    write_cache(&cache, &cards, Some(&rulings), true);
    assert_eq!(code(&import(&cache, &[])), 0);
    let v = output(&cache);
    assert_eq!(
        v["rulings"],
        json!({
            oid(1): [{"source": "wotc", "publishedAt": "2026-01-01", "comment": "Test ruling A1."}],
            oid(2): [
                {"source": "wotc", "publishedAt": "2026-01-01", "comment": "Test ruling B1."},
                {"source": "wotc", "publishedAt": "2026-01-01", "comment": "Test ruling B2."}
            ],
        })
    );
    assert_eq!(
        v["warnings"],
        json!(["rulingsUnknownOracleId:1", "rulingsInvalid:2"])
    );
    assert!(v["records"][0].get("rulings").is_none(), "not embedded");

    let cache = scratch("import-no-rulings");
    write_cache(&cache, &cards, None, true);
    assert_eq!(code(&import(&cache, &[])), 0);
    let v = output(&cache);
    assert_eq!(v["rulings"], json!({}));
    assert_eq!(v["sources"]["rulings"], Value::Null);

    // A rulings file that is not an array is an input error.
    std::fs::write(cache.join("scryfall/rulings.json"), b"{}").unwrap();
    assert_eq!(code(&import(&cache, &[])), 2);
}

// ---------------------------------------------------------------------------------------------
// MTGJSON cross-validation
// ---------------------------------------------------------------------------------------------

fn atomic_entry(n: u32, extra: Value) -> Value {
    let mut v = json!({
        "name": format!("test.card_{n}"),
        "manaValue": 2.0,
        "colors": ["G"],
        "colorIdentity": ["G"],
        "types": ["Creature"],
        "supertypes": [],
        "subtypes": ["Folk", "Test"],
        "power": "2",
        "toughness": "2",
        "text": "Test ability.",
        "identifiers": {"scryfallOracleId": oid(n)},
    });
    if let (Value::Object(m), Value::Object(e)) = (&mut v, extra) {
        for (k, x) in e {
            m.insert(k, x);
        }
    }
    v
}

fn write_atomic(cache: &Path, data: Value) {
    let dir = cache.join("mtgjson");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("AtomicCards.json"),
        bytes_of(&json!({"meta": {"date": "2026-01-03", "version": "test"}, "data": data})),
    )
    .unwrap();
}

/// REQ-ING-001: the MTGJSON cross-check reports each disagreeing characteristic per
/// `oracle_id` and field (no names or text), counts and lists ids found in only one source, is
/// deterministic and leaves the normalized records unchanged; without the MTGJSON file the flag
/// is exit 2 naming `fetch --mtgjson`.
#[test]
fn mtgjson_cross_check_report() {
    let cache = scratch("import-mtgjson");
    let cards = json!([
        card(1, json!({})),
        card(2, json!({})),
        card(3, json!({})),
        card(4, json!({})),
    ]);
    write_cache(&cache, &cards, None, true);
    let report = cache.join("report.json");
    let o = import(&cache, &["--mtgjson-report", p(&report)]);
    assert_eq!(code(&o), 2);
    assert!(stderr(&o).contains("fetch --mtgjson"), "{}", stderr(&o));

    assert_eq!(code(&import(&cache, &[])), 0);
    let plain = std::fs::read(cache.join("normalized/cards.json")).unwrap();
    write_atomic(
        &cache,
        json!({
            "test.card_1": [atomic_entry(1, json!({}))],
            "test.card_2": [atomic_entry(2, json!({"manaValue": 5.0, "colors": ["R"], "colorIdentity": ["R", "G"], "power": "*", "toughness": 2, "types": ["Test", "Creature"]}))],
            "test.card_3": [atomic_entry(3, json!({"manaValue": "two", "loyalty": "3"}))],
            "test.card_5": [atomic_entry(5, json!({}))],
            "test.broken": [{"identifiers": {}}, 7, atomic_entry(6, json!({"identifiers": {"scryfallOracleId": "bad"}}))],
            "test.notlist": {"x": 1},
        }),
    );
    let o = import(&cache, &["--mtgjson-report", p(&report)]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let first = std::fs::read(&report).unwrap();
    assert_eq!(
        std::fs::read(cache.join("normalized/cards.json")).unwrap(),
        plain,
        "the records are unchanged by the report"
    );
    let r: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(r["matched"], 3);
    assert_eq!(r["agreed"], 1);
    assert_eq!(
        r["onlyInScryfall"],
        json!({"count": 1, "oracleIds": [oid(4)]})
    );
    assert_eq!(
        r["onlyInMtgjson"],
        json!({"count": 1, "oracleIds": [oid(5)]})
    );
    assert_eq!(r["invalidEntries"], 4);
    assert_eq!(
        r["disagreements"],
        json!([
            {"oracleId": oid(2), "fields": [
                {"field": "manaValue", "scryfall": 2, "mtgjson": 5.0},
                {"field": "colors", "scryfall": "G", "mtgjson": "R"},
                {"field": "colorIdentity", "scryfall": "G", "mtgjson": "RG"},
                {"field": "types", "scryfall": "list(1)", "mtgjson": "list(2)"},
                {"field": "power", "scryfall": "string", "mtgjson": "string"},
                {"field": "toughness", "scryfall": "string", "mtgjson": "number"},
            ]},
            {"oracleId": oid(3), "fields": [
                {"field": "manaValue", "scryfall": 2, "mtgjson": "string"},
                {"field": "loyalty", "scryfall": "absent", "mtgjson": "string"},
            ]},
        ])
    );
    assert_eq!(r["source"]["updatedAt"], Value::Null);
    let text = String::from_utf8(first.clone()).unwrap();
    assert!(!text.contains("test.card_"), "no names in the report");
    assert!(!text.contains("Test ability"), "no text in the report");

    // Deterministic, and to stdout with the summary on stderr.
    assert_eq!(code(&import(&cache, &["--mtgjson-report", p(&report)])), 0);
    assert_eq!(std::fs::read(&report).unwrap(), first);
    let o = import(&cache, &["--mtgjson-report", "-"]);
    assert_eq!(code(&o), 0);
    assert_eq!(o.stdout, first);
    assert!(stderr(&o).contains("MTGJSON report to stdout"));

    // An unusable AtomicCards file is an input error.
    std::fs::write(cache.join("mtgjson/AtomicCards.json"), b"[]").unwrap();
    assert_eq!(code(&import(&cache, &["--mtgjson-report", p(&report)])), 2);
}

// ---------------------------------------------------------------------------------------------
// CLI, paths and determinism
// ---------------------------------------------------------------------------------------------

/// REQ-NFR-004: `.gitignore` lists `ingest-cache/`; the default cache is `ingest-cache`; the
/// CLI with no `--out` writes only under the cache directory, and with no `--cache` only under
/// `ingest-cache/` of the current directory.
#[test]
fn default_paths_stay_in_the_ignored_cache() {
    let gi = std::fs::read_to_string(workspace_root().join(".gitignore")).unwrap();
    assert!(gi.lines().any(|l| l.trim() == "ingest-cache/"));
    assert_eq!(DEFAULT_CACHE, "ingest-cache");

    let root = scratch("paths");
    let cache = root.join("ingest-cache");
    write_cache(&cache, &json!([card(1, json!({}))]), None, true);
    let before = list_files(&root);
    let o = run_in(Some(&root), &["import"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let mut after = list_files(&root);
    after.retain(|f| !before.contains(f));
    assert_eq!(
        after,
        vec![PathBuf::from("ingest-cache/normalized/cards.json")]
    );
    let summary = String::from_utf8_lossy(&o.stdout);
    assert_eq!(summary.lines().count(), 1, "{summary}");
    assert!(summary.contains("1 records, 0 rejected"), "{summary}");
}

/// REQ-ING-002: usage errors exit 2; a missing Oracle Cards file exits 2 naming `fetch`;
/// `--strict` exits 1 with rejections or diagnostics and 0 without; no flag exits 0 despite
/// diagnostics; `--out -` writes the JSON to stdout and the summary to stderr.
#[test]
fn import_cli_exit_codes() {
    let cache = scratch("import-cli");
    for bad in [
        vec!["import", "--bogus"],
        vec!["import", "--out"],
        vec!["import", "--out", "--strict"],
        vec!["import", "--cache", "--out"],
        vec!["import", "--mtgjson-report", "--cache"],
        vec!["import", "--out", "a", "--out", "b"],
        vec!["import", "--out", "-", "--mtgjson-report", "-"],
        vec!["bogus"],
        vec![],
    ] {
        let o = run(&bad);
        assert_eq!(code(&o), 2, "{bad:?}");
        assert!(stderr(&o).contains("usage:"), "{bad:?}");
    }
    let o = import(&cache, &[]);
    assert_eq!(code(&o), 2);
    assert!(
        stderr(&o).contains("fineprint-ingest fetch"),
        "{}",
        stderr(&o)
    );

    write_cache(&cache, &json!([card(1, json!({}))]), None, true);
    assert_eq!(code(&import(&cache, &["--strict"])), 0);
    write_cache(&cache, &json!([card(1, json!({"cmc": 9.0}))]), None, true);
    assert_eq!(code(&import(&cache, &[])), 0);
    assert_eq!(code(&import(&cache, &["--strict"])), 1);
    write_cache(&cache, &json!([card(1, json!({})), 5]), None, true);
    assert_eq!(code(&import(&cache, &[])), 0);
    assert_eq!(code(&import(&cache, &["--strict"])), 1);

    let o = import(&cache, &["--out", "-"]);
    assert_eq!(code(&o), 0);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["records"].as_array().unwrap().len(), 1);
    assert!(stderr(&o).starts_with("import: wrote stdout"));

    let out = cache.join("explicit.json");
    let o = import(&cache, &["--out", p(&out)]);
    assert_eq!(code(&o), 0);
    assert!(out.is_file());
    let o = import(&cache, &["--out", p(&cache.join("missing-dir/x.json"))]);
    assert_eq!(code(&o), 2);

    // An Oracle Cards file that is not an array is an input error.
    std::fs::write(cache.join("scryfall/oracle-cards.json"), b"{}").unwrap();
    assert_eq!(code(&import(&cache, &[])), 2);
}

/// R3, REQ-ING-001: two imports of the same cache (input in a different order) produce
/// byte-identical normalized output.
#[test]
fn import_is_deterministic_across_input_order() {
    let a = scratch("det-a");
    let b = scratch("det-b");
    let cards: Vec<Value> = (1..=20)
        .map(|n| card(n, json!({"keywords": ["Zeta", "Alpha"]})))
        .collect();
    let mut rev = cards.clone();
    rev.reverse();
    write_cache(&a, &Value::Array(cards), None, false);
    write_cache(&b, &Value::Array(rev), None, false);
    assert_eq!(code(&import(&a, &[])), 0);
    assert_eq!(code(&import(&b, &[])), 0);
    let va = output(&a);
    let vb = output(&b);
    assert_eq!(va["records"], vb["records"]);
    let ra = std::fs::read(a.join("normalized/cards.json")).unwrap();
    assert_eq!(code(&import(&a, &[])), 0);
    assert_eq!(std::fs::read(a.join("normalized/cards.json")).unwrap(), ra);
}
