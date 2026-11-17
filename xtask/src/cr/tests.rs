// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Command-level tests for `cargo xtask cr` (REQ-DOC-001, REQ-NFR-007, REQ-NFR-008). All
//! hermetic: synthetic CR text with invented prose, temp roots without git, no network.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::fetch::tests::TempDir;
use super::fetch::{Source, render_source};
use super::rules::tests::synthetic_cr;
use super::scan::{SRS_PATH, ScanFile, scan_files};
use super::*;

const SRC: &str = "crates/demo/src/lib.rs";

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_string()).collect()
}

/// A temp root with `docs/SRS.md`, source files and a cache holding `cr`.
struct Fixture {
    tmp: TempDir,
}

impl Fixture {
    fn new(srs: &str, sources: &[(&str, &str)], cr: &str) -> Fixture {
        let f = Fixture {
            tmp: TempDir::new("root"),
        };
        f.write(SRS_PATH, srs);
        for (p, c) in sources {
            f.write(p, c);
        }
        f.set_cr(cr, "2026-11-06");
        f
    }

    fn root(&self) -> &Path {
        &self.tmp.0
    }

    fn cache(&self) -> PathBuf {
        self.root().join("cr-cache")
    }

    fn index_file(&self) -> PathBuf {
        self.root().join(INDEX_PATH)
    }

    fn write(&self, rel: &str, content: &str) {
        let p = self.root().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    fn set_cr(&self, cr: &str, date: &str) {
        fs::create_dir_all(self.cache()).unwrap();
        fs::write(self.cache().join("rules.txt"), cr).unwrap();
        let src = Source {
            source_url: "https://example.invalid/rules.txt".to_string(),
            effective_date: date.to_string(),
            sha256: sha256::sha256_hex(cr.as_bytes()),
            byte_count: cr.len() as u64,
        };
        fs::write(self.cache().join("source.json"), render_source(&src)).unwrap();
    }

    fn index(&self) -> Outcome {
        index_at(self.root(), &self.cache(), &self.index_file(), INDEX_PATH)
    }

    fn check(&self) -> Outcome {
        check_at(self.root(), &self.cache(), &self.index_file(), INDEX_PATH)
    }
}

fn srs_text() -> String {
    format!(
        "# Synthetic SRS\nRules baseline CR-2026-11-06.\nSee CR {a}.1 and {a}.2.\n| {a}.1a |\n| 31 | Mandatory loops cited {x}; y |\n",
        a = 100,
        x = "729.4"
    )
}

/// REQ-DOC-001 acceptance ("The index covers every CR citation in docs/SRS.md"): with a CR
/// that contains every rule the scanner extracts from a synthetic SRS, the index keys equal the
/// scanner's set exactly.
#[test]
fn req_doc_001_index_covers_every_srs_citation_synthetic() {
    let srs = format!(
        "CR-2026-11-06\nCR {a}.4b, {b}\nCR {c}–{d}, {e}\n**CR {a}.2a–m**\n(CR {a}.3, {f}.3)\n(702.191)\n| {g}.3, {g}.6e |\nCR {a}.6 – default\nCRs {h} and {i}\n",
        a = 104,
        b = 733,
        c = 100,
        d = 101,
        e = 800,
        f = 704,
        g = 712,
        h = 500,
        i = 502
    );
    let scan = scan_files(&[ScanFile {
        path: SRS_PATH.to_string(),
        bytes: srs.clone().into_bytes(),
    }]);
    let found: Vec<String> = scan.cited.keys().map(|r| r.as_str().to_string()).collect();
    assert_eq!(
        found,
        s(&[
            "100", "101", "104.2a", "104.2m", "104.3", "104.4b", "104.6", "500", "502", "702.191",
            "704.3", "712.3", "712.6e", "733", "800"
        ])
    );
    let extra: Vec<(String, String)> = found
        .iter()
        .map(|n| (n.clone(), format!("Synthetic text for {}", n.len())))
        .collect();
    let extra_ref: Vec<(&str, &str)> = extra
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let fx = Fixture::new(&srs, &[], &synthetic_cr(&extra_ref));
    let out = fx.index();
    assert_eq!(out.code, 0, "{out:?}");
    let idx = index::read(&fs::read_to_string(fx.index_file()).unwrap()).unwrap();
    let keys: Vec<String> = idx
        .citations
        .keys()
        .map(|r| r.as_str().to_string())
        .collect();
    assert_eq!(keys, found);
}

/// REQ-DOC-001 acceptance ("The index covers every CR citation in docs/SRS.md"), on the real
/// repository: the committed index's rules cited from docs/SRS.md plus its excluded rules
/// equal exactly the scanner's SRS set. Needs no CR text and no network.
#[test]
fn req_doc_001_committed_index_covers_real_srs() {
    let top = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let srs = fs::read(top.join(SRS_PATH)).unwrap();
    let scan = scan_files(&[ScanFile {
        path: SRS_PATH.to_string(),
        bytes: srs,
    }]);
    let mut want: BTreeSet<String> = scan.cited.keys().map(|r| r.as_str().to_string()).collect();
    want.extend(scan.excluded.iter().map(|e| e.rule.as_str().to_string()));
    let idx = index::read(&fs::read_to_string(top.join(INDEX_PATH)).unwrap()).unwrap();
    let mut got: BTreeSet<String> = idx
        .citations
        .iter()
        .filter(|(_, e)| e.locations.iter().any(|l| l.starts_with("docs/SRS.md:")))
        .map(|(r, _)| r.as_str().to_string())
        .collect();
    got.extend(idx.excluded.iter().map(|e| e.rule.as_str().to_string()));
    assert_eq!(got, want);
    assert_eq!(idx.baseline.id, "CR-2026-11-06");
    assert!(!scan.excluded.is_empty());
}

/// REQ-DOC-001 acceptance ("A fabricated citation in a scratch commit fails the check"): a
/// fabricated rule in a source comment fails `check` both when it is missing from the CR and
/// when it is in the CR but not in the index; removing the comment passes again.
#[test]
fn req_doc_001_fabricated_citation_fails_check() {
    let fab = format!("{}.9z", 999);
    let clean = "fn demo() {}\n";
    let dirty = format!("fn demo() {{}}\n// CR {fab}\n");
    let fx = Fixture::new(&srs_text(), &[(SRC, clean)], &synthetic_cr(&[]));
    assert_eq!(fx.index().code, 0);
    assert_eq!(fx.check().code, 0);

    fx.write(SRC, &dirty);
    let out = fx.check();
    assert_eq!(out.code, 1, "{out:?}");
    let want = format!("{SRC}:2: CR {fab} not found in the fetched CR text");
    assert!(out.stderr.contains(&want), "{out:?}");
    // index refuses to write while the citation is missing
    let before = fs::read(fx.index_file()).unwrap();
    assert_eq!(fx.index().code, 1);
    assert_eq!(fs::read(fx.index_file()).unwrap(), before);

    // in the CR but not indexed
    fx.set_cr(
        &synthetic_cr(&[(&fab, "Synthetic fabricated entry")]),
        "2026-11-06",
    );
    let out = fx.check();
    assert_eq!(out.code, 1, "{out:?}");
    assert!(
        out.stderr
            .iter()
            .any(|l| l.starts_with(&format!("{SRC}:2: CR {fab} is cited but not in"))),
        "{out:?}"
    );

    fx.write(SRC, clean);
    assert_eq!(fx.check().code, 0);
}

/// REQ-DOC-001, REQ-NFR-007: check semantics (hash change, index problems, cache missing,
/// uncited warning, moved lines, effective-date warning).
#[test]
fn req_doc_001_check_semantics() {
    let fx = Fixture::new(
        &srs_text(),
        &[(SRC, &format!("// CR {}.1\n", 101))],
        &synthetic_cr(&[]),
    );
    assert_eq!(fx.check().code, 1, "index missing");
    let out = fx.index();
    assert_eq!(out.code, 0, "{out:?}");
    assert!(out.stdout[0].contains("1 excluded"), "{out:?}");
    let ok = fx.check();
    assert_eq!(ok.code, 0, "{ok:?}");
    assert!(ok.stderr.is_empty(), "{ok:?}");

    // moving citing lines does not fail the check
    fx.write(SRS_PATH, &format!("\n\n\n{}", srs_text()));
    fx.write(SRC, &format!("\n\n// CR {}.1\n", 101));
    assert_eq!(fx.check().code, 0);

    // indexed but no longer cited: warning only
    fx.write(SRC, "fn x() {}\n");
    let out = fx.check();
    assert_eq!(out.code, 0, "{out:?}");
    assert!(
        out.stderr
            .iter()
            .any(|l| l.starts_with("warning:") && l.contains("CR 101.1 "))
    );
    fx.write(SRC, &format!("// CR {}.1\n", 101));

    // effective date differs: warning only
    fx.set_cr(&synthetic_cr(&[]), "2027-01-01");
    let out = fx.check();
    assert_eq!(out.code, 0, "{out:?}");
    assert!(
        out.stderr
            .iter()
            .any(|l| l.starts_with("warning:") && l.contains("2027-01-01"))
    );

    // changed rule text: exit 1 naming the rule
    fx.set_cr(&synthetic_cr(&[]).replace("zephyr", "zebra"), "2026-11-06");
    let out = fx.check();
    assert_eq!(out.code, 1, "{out:?}");
    assert!(
        out.stderr.iter().any(|l| l.contains(&format!(
            "CR {}.1: rule text changed since the verified baseline",
            100
        ))),
        "{out:?}"
    );
    fx.set_cr(&synthetic_cr(&[]), "2026-11-06");
    assert_eq!(fx.check().code, 0);

    // garbled index and unknown schema version
    let good = fs::read_to_string(fx.index_file()).unwrap();
    fs::write(fx.index_file(), "{ not json").unwrap();
    assert_eq!(fx.check().code, 1);
    fs::write(
        fx.index_file(),
        good.replace("\"schemaVersion\": 1", "\"schemaVersion\": 9"),
    )
    .unwrap();
    let out = fx.check();
    assert_eq!(out.code, 1);
    assert!(
        out.stderr
            .iter()
            .any(|l| l.starts_with("docs/cr-citations.json:1: unknown schemaVersion"))
    );
    fs::write(fx.index_file(), &good).unwrap();

    // cache missing or corrupted: exit 2 for both commands
    fs::write(fx.cache().join("rules.txt"), "tampered").unwrap();
    assert_eq!(fx.check().code, 2);
    fs::remove_dir_all(fx.cache()).unwrap();
    assert_eq!(fx.check().code, 2);
    assert_eq!(fx.index().code, 2);
}

/// REQ-NFR-007, REQ-DOC-001: `index` output is byte-deterministic, carries no prose, records
/// the SRS exclusion, and refuses an SRS that does not name the fetched baseline.
#[test]
fn req_nfr_007_index_file_deterministic() {
    let fx = Fixture::new(
        &srs_text(),
        &[(SRC, &format!("/* CR {}.1a */\n", 100))],
        &synthetic_cr(&[]),
    );
    assert_eq!(fx.index().code, 0);
    let first = fs::read(fx.index_file()).unwrap();
    assert_eq!(fx.index().code, 0);
    assert_eq!(fs::read(fx.index_file()).unwrap(), first);
    let text = String::from_utf8(first).unwrap();
    for word in ["zephyr", "quokka", "Synthetic", "Alpha", "Beta", "Example"] {
        assert!(!text.contains(word), "{word}");
    }
    assert!(text.contains("\"location\": \"docs/SRS.md:5\""));
    assert!(text.contains(&format!(
        "\"locations\": [\n        \"{SRC}:1\",\n        \"docs/SRS.md:4\"\n      ]"
    )));
    assert!(!text.contains(&fx.root().display().to_string()));
    // an SRS that does not name the baseline id blocks the index
    fx.write(
        SRS_PATH,
        &srs_text().replace("CR-2026-11-06", "CR-2000-01-01"),
    );
    let before = fs::read(fx.index_file()).unwrap();
    let out = fx.index();
    assert_eq!(out.code, 1, "{out:?}");
    assert_eq!(fs::read(fx.index_file()).unwrap(), before);
    // an unwritable output is an input error
    fx.write(SRS_PATH, &srs_text());
    let out = index_at(
        fx.root(),
        &fx.cache(),
        &fx.root().join("no/such/dir/x.json"),
        "x",
    );
    assert_eq!(out.code, 2);
    // a root that does not exist
    let gone = fx.root().join("missing-root");
    assert_eq!(index_at(&gone, &fx.cache(), &fx.index_file(), "x").code, 2);
    assert_eq!(check_at(&gone, &fx.cache(), &fx.index_file(), "x").code, 2);
}

/// REQ-NFR-007, REQ-NFR-008: the command entry point with explicit flags (no network: the
/// fetch is served from the cache).
#[test]
fn req_nfr_008_run_with_flags() {
    let fx = Fixture::new(&srs_text(), &[], &synthetic_cr(&[]));
    let root = fx.root().display().to_string();
    let cache = fx.cache().display().to_string();
    let file = fx.index_file().display().to_string();
    let args = |v: &[&str]| s(v);
    assert_eq!(
        run(&args(&[
            "index", "--root", &root, "--cache", &cache, "--out", &file
        ])),
        0
    );
    assert_eq!(
        run(&args(&[
            "check", "--root", &root, "--cache", &cache, "--index", &file
        ])),
        0
    );
    assert_eq!(run(&args(&["fetch", "--cache", &cache])), 0);
    assert_eq!(
        run(&args(&[
            "fetch",
            "--force",
            "--cache",
            &cache,
            "--url",
            "file:///dev/null/x"
        ])),
        2
    );
    fs::remove_file(fx.cache().join("rules.txt")).unwrap();
    assert_eq!(
        run(&args(&[
            "check", "--root", &root, "--cache", &cache, "--index", &file
        ])),
        2
    );
}

/// REQ-NFR-008: usage errors exit 2.
#[test]
fn req_nfr_008_usage_errors() {
    for a in [
        &[][..],
        &["nope"][..],
        &["fetch", "--bogus"][..],
        &["index", "--out"][..],
        &["check", "--force"][..],
        &["fetch", "--url"][..],
        &["check", "extra"][..],
    ] {
        assert_eq!(run(&s(a)), 2, "{a:?}");
    }
    assert_eq!(
        parse_flags(
            &s(&["--url", "u", "--force", "--cache", "c"]),
            &["--url", "--cache"],
            true
        ),
        Ok(Flags {
            url: Some("u".into()),
            force: true,
            cache: Some("c".into()),
            root: None,
            file: None
        })
    );
}

/// R12, REQ-NFR-007, REQ-DOC-001: the CR parser, citation scanner and JSON readers survive
/// bounded random input (fixed-seed xorshift).
#[test]
fn req_nfr_007_parsers_fuzz_no_panic() {
    let alphabet: Vec<char> = "0123456789.  ,;/-\u{2013}CRsand\"'#/*`r\\\n\r\t{}[]:Examplez1.GaeConcptsGlossary\u{feff}\u{e9}"
        .chars()
        .collect();
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for case in 0..600 {
        let len = (next() % 400) as usize;
        let text: String = if case % 2 == 0 {
            let bytes: Vec<u8> = (0..len).map(|_| (next() & 0xff) as u8).collect();
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            (0..len)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                .collect()
        };
        let wrapped = format!("{}\n{text}\n{}\n", rules::START_MARKER, rules::END_MARKER);
        let _ = rules::parse_cr(&text);
        let _ = rules::parse_cr(&wrapped);
        let _ = scan::srs_line(&text);
        let _ = scan::comment_lines("a.rs", &text);
        let _ = scan::comment_lines("a.py", &text);
        let _ = scan::comment_lines("a.ts", &text);
        let files = [
            ScanFile {
                path: SRS_PATH.into(),
                bytes: text.clone().into_bytes(),
            },
            ScanFile {
                path: "x.rs".into(),
                bytes: text.clone().into_bytes(),
            },
        ];
        let _ = scan_files(&files);
        let _ = json::parse(&text);
        let _ = index::read(&text);
        let _ = fetch::read_source(&text);
        let _ = fetch::discover_url(&format!("https://media.wizards.com/{text}"));
        let _ = fetch::effective_date(&format!("These rules are effective as of {text}"));
        let _ = fetch::validate(text.as_bytes());
    }
}
