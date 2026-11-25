// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Gate tests for the `fineprint-ingest fetch` commit order and concurrent-run safety
//! (REQ-ING-001, REQ-NFR-004, R12; gate 02-state-objects F13, F14). Every body is a tiny
//! synthetic JSON document with neutral names (R5); the client is a fake `Http` and the time is
//! a fixed value: nothing here touches the network or reads the clock for a result (R7).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use fineprint_base::hash::sha256;
use fineprint_ingest::import::fetch::{
    BULK_DATA_URL, FetchOptions, Http, Request, SourceMeta, fetch, is_fresh, parse_source_meta,
};
use serde_json::json;

const T0: u64 = 1_800_000_000;
const ORACLE_URI: &str = "https://data.scryfall.io/oracle-cards/test.json";
const RULINGS_URI: &str = "https://data.scryfall.io/rulings/test.json";
const ORACLE: &str = "oracle-cards.json";
const RULINGS: &str = "rulings.json";

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
            "fp-spec-ingest-fetch-gate-{name}-{}-{n}-{nanos}",
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

fn bytes_of(v: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

fn bulk_meta() -> Vec<u8> {
    bytes_of(&json!({
        "object": "list",
        "data": [
            {"type": "oracle_cards", "updated_at": "u1", "download_uri": ORACLE_URI},
            {"type": "rulings", "updated_at": "u2", "download_uri": RULINGS_URI},
        ]
    }))
}

fn oracle_body(generation: u32) -> Vec<u8> {
    bytes_of(&json!([{"object": "card", "name": "test.card_a", "generation": generation}]))
}

fn rulings_body(generation: u32) -> Vec<u8> {
    bytes_of(&json!([{"object": "ruling", "comment": "test.ruling", "generation": generation}]))
}

/// A fake client serving one generation of bodies and counting requests in shared counters.
struct Fake<'a> {
    generation: u32,
    metadata: &'a AtomicUsize,
    downloads: &'a AtomicUsize,
}

impl Http for Fake<'_> {
    fn get_json(&self, req: &Request) -> Result<Vec<u8>, String> {
        self.metadata.fetch_add(1, Ordering::SeqCst);
        if req.url == BULK_DATA_URL {
            Ok(bulk_meta())
        } else {
            Err("no fake route".to_owned())
        }
    }

    fn download_to_file(&self, req: &Request, dest: &Path) -> Result<(), String> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        let body = match req.url.as_str() {
            ORACLE_URI => oracle_body(self.generation),
            RULINGS_URI => rulings_body(self.generation),
            _ => return Err("no fake route".to_owned()),
        };
        std::fs::write(dest, body).map_err(|e| e.to_string())
    }
}

fn opts(cache: &Path, force: bool) -> FetchOptions {
    FetchOptions {
        cache: cache.to_path_buf(),
        force,
        mtgjson: false,
    }
}

fn meta_of(dir: &Path) -> SourceMeta {
    parse_source_meta(&std::fs::read(dir.join("source.json")).unwrap()).unwrap()
}

/// Every metadata entry matches the file on disk (sha256 and byte count).
fn assert_consistent(dir: &Path) {
    for (file, m) in meta_of(dir) {
        let bytes = std::fs::read(dir.join(&file)).unwrap();
        assert_eq!(m.sha256, sha256(&bytes).to_hex(), "{file}");
        assert_eq!(m.byte_count, bytes.len() as u64, "{file}");
    }
}

fn names(dir: &Path) -> BTreeMap<String, bool> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                e.path().is_dir(),
            )
        })
        .collect()
}

/// REQ-ING-001, REQ-NFR-004, R12 (F13): when a rename fails mid-commit, the metadata never
/// claims the newly renamed file as fresh (its entry was removed before any rename), a re-run
/// refetches it, and the successful run ends with the full metadata, consistent with the files
/// and no part file left behind.
#[test]
fn fetch_commit_failure_never_leaves_a_trusted_new_file() {
    let cache = scratch("commit");
    let dir = cache.join("scryfall");
    let (m, d) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let fake = |generation| Fake {
        generation,
        metadata: &m,
        downloads: &d,
    };
    fetch(&fake(1), &opts(&cache, false), T0).unwrap();
    assert_eq!(meta_of(&dir).len(), 2);
    assert_consistent(&dir);

    // The second rename (rulings) fails: its destination is a non-empty directory.
    std::fs::remove_file(dir.join(RULINGS)).unwrap();
    std::fs::create_dir(dir.join(RULINGS)).unwrap();
    std::fs::write(dir.join(RULINGS).join("keep"), b"x").unwrap();
    let err = fetch(&fake(2), &opts(&cache, true), T0 + 10).unwrap_err();
    assert!(!err.is_empty());
    // The first rename landed the generation-2 oracle file, but nothing claims it.
    assert_eq!(std::fs::read(dir.join(ORACLE)).unwrap(), oracle_body(2));
    let meta = meta_of(&dir);
    assert!(!meta.contains_key(ORACLE), "{meta:?}");
    assert!(!meta.contains_key(RULINGS), "{meta:?}");
    assert!(!is_fresh(&meta, &dir, ORACLE, T0 + 20));
    assert_consistent(&dir);
    assert!(!names(&dir).keys().any(|n| n.ends_with(".part")));

    // A non-forced re-run refetches both files and ends with the full, consistent metadata.
    std::fs::remove_dir_all(dir.join(RULINGS)).unwrap();
    let before = d.load(Ordering::SeqCst);
    fetch(&fake(3), &opts(&cache, false), T0 + 20).unwrap();
    assert_eq!(d.load(Ordering::SeqCst) - before, 2, "both files refetched");
    let meta = meta_of(&dir);
    assert_eq!(
        meta.keys().map(String::as_str).collect::<Vec<_>>(),
        [ORACLE, RULINGS]
    );
    assert!(meta.values().all(|f| f.fetched_at_epoch_seconds == T0 + 20));
    assert_eq!(std::fs::read(dir.join(ORACLE)).unwrap(), oracle_body(3));
    assert_eq!(std::fs::read(dir.join(RULINGS)).unwrap(), rulings_body(3));
    assert_consistent(&dir);
    assert!(!names(&dir).keys().any(|n| n.ends_with(".part")));
}

/// REQ-ING-001, REQ-NFR-004 (F14): two concurrent fetches on one cache at the same fixed time
/// serialize on the group lock: both succeed, the cache is consistent, and the needed files are
/// downloaded exactly once in total (the second run sees the first's fresh metadata). The lock
/// file stays in place.
#[test]
fn concurrent_fetches_serialize_on_the_group_lock() {
    let cache = scratch("concurrent");
    let (m, d) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let results: Vec<Result<String, String>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let (m, d, cache) = (&m, &d, &cache);
                s.spawn(move || {
                    let fake = Fake {
                        generation: 1,
                        metadata: m,
                        downloads: d,
                    };
                    fetch(&fake, &opts(cache, false), T0)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for r in &results {
        assert!(r.is_ok(), "{r:?}");
    }
    assert_eq!(m.load(Ordering::SeqCst), 1, "one metadata request");
    assert_eq!(d.load(Ordering::SeqCst), 2, "each needed file once");
    let dir = cache.join("scryfall");
    assert_eq!(meta_of(&dir).len(), 2);
    assert_consistent(&dir);
    assert_eq!(std::fs::read(dir.join(ORACLE)).unwrap(), oracle_body(1));
    assert_eq!(std::fs::read(dir.join(RULINGS)).unwrap(), rulings_body(1));
    let listed = names(&dir);
    assert!(listed.contains_key(".lock"), "{listed:?}");
    assert!(!listed.keys().any(|n| n.ends_with(".part")), "{listed:?}");
}
