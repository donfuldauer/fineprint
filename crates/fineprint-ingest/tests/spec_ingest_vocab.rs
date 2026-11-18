// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint-ingest vocab` (REQ-SUB-001, REQ-OBJ-001, REQ-DSL-014; #15 FP-014),
//! run through the built binary. The synthetic CR is assembled in code from invented words
//! (R5); no line of this file starts with a rule number. The tests against the real CR run only
//! when the git-ignored `cr-cache/rules.txt` exists, unless `FINEPRINT_REQUIRE_CR_CACHE=1`
//! makes a missing cache a failure (the `cr` workflow sets it); everything else needs no cache.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use fineprint_dsl::vocab::RulesVocabulary;

const BIN: &str = env!("CARGO_BIN_EXE_fineprint-ingest");

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A new, empty scratch directory created exclusively under Cargo's private
/// `CARGO_TARGET_TMPDIR` (never a shared, predictable temp path). The leaf name combines the
/// pid, a process-wide counter and the clock (for uniqueness only, R7); `create_dir` fails if
/// the leaf exists, which is retried a bounded number of times with a new name. Any other
/// error panics.
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
            "fp-spec-ingest-vocab-{name}-{}-{n}-{nanos}",
            std::process::id()
        ));
        match std::fs::create_dir(&leaf) {
            Ok(()) => return leaf,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => panic!("cannot create scratch dir {}: {e}", leaf.display()),
        }
    }
    panic!(
        "no fresh scratch dir under {} after 16 tries",
        root.display()
    );
}

fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

/// `(rule, text)` pairs of a small invented CR body.
fn synthetic_rules() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "111.10a",
            "A Zeta token is a colorless Zeta alpha token with \u{201c}{T}: Quux.\u{201d}",
        ),
        (
            "111.10b",
            "A Kappa token is a 1/2 green Kappa beta token named Kappa Prime with glow.",
        ),
        (
            "122.1b",
            "A keyword counter does invented things. The keywords that a keyword counter can be are glow and gleam.",
        ),
        ("205.2a", "The card types are alpha and beta."),
        (
            "205.3g",
            "Alphas have invented subtypes; these subtypes are called alpha types. The alpha types are Zeta and Eta.",
        ),
        (
            "205.3h",
            "Betas have invented subtypes; these subtypes are called beta types. The beta types are Kappa, Rho-Tau, and Omicron Prime.",
        ),
        ("205.4a", "The supertypes are lofty."),
        ("701.1.", "Invented introduction words."),
        ("701.2.", "Frobnicate"),
        ("702.1.", "Invented introduction words."),
        ("702.2.", "Glow"),
        ("702.3.", "Gleam"),
    ]
}

fn render(rules: &[(&str, &str)], line_end: &str, bom: bool) -> Vec<u8> {
    let mut lines = vec![
        "Invented Title".to_owned(),
        "These rules are effective as of July 9, 2030.".to_owned(),
        "1. Game Concepts".to_owned(),
        "Glossary".to_owned(),
        String::new(),
        "1. Game Concepts".to_owned(),
        String::new(),
    ];
    for (n, t) in rules {
        lines.push(format!("{n} {t}"));
        lines.push(String::new());
    }
    lines.push("Glossary".to_owned());
    lines.push("Glow".to_owned());
    let mut text = lines.join(line_end);
    text.push_str(line_end);
    if bom {
        text.insert(0, '\u{feff}');
    }
    text.into_bytes()
}

/// REQ-SUB-001, REQ-OBJ-001, REQ-DSL-014: the binary is deterministic, CRLF/BOM-invariant,
/// writes canonical bytes with one trailing newline, and `--check` agrees.
#[test]
fn req_sub_001_binary_determinism() {
    let dir = scratch("det");
    let rules = synthetic_rules();
    let mut outputs = Vec::new();
    for (i, (eol, bom)) in [
        ("\n", false),
        ("\n", false),
        ("\r\n", false),
        ("\r\n", true),
    ]
    .into_iter()
    .enumerate()
    {
        let cr = dir.join(format!("rules{i}.txt"));
        std::fs::write(&cr, render(&rules, eol, bom)).unwrap();
        let out = dir.join(format!("vocab{i}.json"));
        let o = run(&[
            "vocab".as_ref(),
            "--cr".as_ref(),
            cr.as_os_str(),
            "--out".as_ref(),
            out.as_os_str(),
        ]);
        assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(String::from_utf8_lossy(&o.stdout).lines().count(), 1);
        outputs.push(std::fs::read(&out).unwrap());
    }
    assert!(outputs.windows(2).all(|w| w[0] == w[1]));
    let bytes = &outputs[0];
    assert!(bytes.ends_with(b"}\n"));
    assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
    let v = RulesVocabulary::from_slice(bytes).unwrap();
    assert_eq!(&v.to_canonical_bytes().unwrap(), bytes);
    assert_eq!(v.cr_effective_date, "2030-07-09");
    assert_eq!(
        v.subtypes_of("Beta").collect::<Vec<_>>(),
        ["Kappa", "Rho-Tau", "Omicron Prime"]
    );
    assert_eq!(v.keyword_ability("GLOW").unwrap().rule_ref, "702.2");
    // stdout form
    let cr = dir.join("rules0.txt");
    let o = run(&[
        "vocab".as_ref(),
        "--cr".as_ref(),
        cr.as_os_str(),
        "--out".as_ref(),
        "-".as_ref(),
    ]);
    assert_eq!(code(&o), 0);
    assert_eq!(&o.stdout, bytes);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-OBJ-001, REQ-SUB-001: `--check` exit codes on synthetic input: 0 identical, 1 after one
/// changed byte (message names the section, never CR text), 2 for a missing or unparseable
/// `--cr` and for usage errors.
#[test]
fn req_obj_001_check_exit_codes_synthetic() {
    let dir = scratch("check");
    let cr = dir.join("rules.txt");
    std::fs::write(&cr, render(&synthetic_rules(), "\n", false)).unwrap();
    let out = dir.join("vocab.json");
    let gen_args = [
        "vocab".as_ref(),
        "--cr".as_ref(),
        cr.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
    ];
    assert_eq!(code(&run(&gen_args)), 0);
    let mut check_args = gen_args.to_vec();
    check_args.push("--check".as_ref());
    let o = run(&check_args);
    assert_eq!(code(&o), 0);
    assert!(String::from_utf8_lossy(&o.stdout).contains("matches"));
    // alter one byte inside a name
    let mut bytes = std::fs::read(&out).unwrap();
    let at = bytes.windows(5).position(|w| w == b"Zeta\"").unwrap();
    bytes[at] = b'Y';
    std::fs::write(&out, &bytes).unwrap();
    let o = run(&check_args);
    assert_eq!(code(&o), 1);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("first differing section"), "{err}");
    assert!(!err.contains("invented"), "{err}");
    // missing and unparseable --cr
    let missing = dir.join("absent.txt");
    let o = run(&[
        "vocab".as_ref(),
        "--cr".as_ref(),
        missing.as_os_str(),
        "--check".as_ref(),
        "--out".as_ref(),
        out.as_os_str(),
    ]);
    assert_eq!(code(&o), 2);
    let junk = dir.join("junk.txt");
    std::fs::write(&junk, b"not a rules document\n").unwrap();
    let o = run(&[
        "vocab".as_ref(),
        "--cr".as_ref(),
        junk.as_os_str(),
        "--check".as_ref(),
        "--out".as_ref(),
        out.as_os_str(),
    ]);
    assert_eq!(code(&o), 2);
    assert!(String::from_utf8_lossy(&o.stderr).contains("marker"));
    std::fs::write(&junk, b"\xff\xfe\x00").unwrap();
    assert_eq!(
        code(&run(&["vocab".as_ref(), "--cr".as_ref(), junk.as_os_str()])),
        2
    );
    // usage errors
    assert_eq!(code(&run(&[])), 2);
    assert_eq!(code(&run(&["bogus".as_ref()])), 2);
    assert_eq!(code(&run(&["vocab".as_ref()])), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-SUB-001, REQ-OBJ-001: each missing required rule exits 2 and names what is missing.
#[test]
fn req_sub_001_missing_rules_exit_2() {
    let dir = scratch("missing");
    let cr = dir.join("rules.txt");
    let all = synthetic_rules();
    for (drop, needle) in [
        (vec!["111.10a", "111.10b"], "111.10a"),
        (vec!["205.2a"], "205.2a"),
        (vec!["205.4a"], "205.4a"),
        (vec!["205.3g", "205.3h"], "205.3g"),
        (vec!["122.1b"], "122.1b"),
    ] {
        let rules: Vec<(&str, &str)> = all
            .iter()
            .copied()
            .filter(|(n, _)| !drop.contains(n))
            .collect();
        std::fs::write(&cr, render(&rules, "\n", false)).unwrap();
        let o = run(&[
            "vocab".as_ref(),
            "--cr".as_ref(),
            cr.as_os_str(),
            "--out".as_ref(),
            "-".as_ref(),
        ]);
        assert_eq!(code(&o), 2, "{needle}");
        assert!(o.stdout.is_empty(), "{needle}: no partial output");
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(needle),
            "{needle}"
        );
    }
    let text = String::from_utf8(render(&all, "\n", false)).unwrap();
    for (from, needle) in [
        (
            "These rules are effective as of July 9, 2030.",
            "effective-date",
        ),
        ("1. Game Concepts", "START"),
        ("Glossary", "END"),
    ] {
        std::fs::write(&cr, text.replace(from, "Invented")).unwrap();
        let o = run(&[
            "vocab".as_ref(),
            "--cr".as_ref(),
            cr.as_os_str(),
            "--out".as_ref(),
            "-".as_ref(),
        ]);
        assert_eq!(code(&o), 2, "{needle}");
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(needle),
            "{needle}"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-DSL-014: a keyword id collision exits 1.
#[test]
fn req_dsl_014_collision_exit_1() {
    let dir = scratch("collide");
    let cr = dir.join("rules.txt");
    let mut rules = synthetic_rules();
    rules.push(("702.4.", "Frob-nicate"));
    rules.push(("702.5.", "Frobnicate"));
    std::fs::write(&cr, render(&rules, "\n", false)).unwrap();
    let o = run(&[
        "vocab".as_ref(),
        "--cr".as_ref(),
        cr.as_os_str(),
        "--out".as_ref(),
        "-".as_ref(),
    ]);
    assert_eq!(code(&o), 1);
    assert!(String::from_utf8_lossy(&o.stderr).contains("FROBNICATE"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-SUB-001, REQ-OBJ-001, REQ-DSL-014 (CI check): with the cached CR, `--check` exits 0 on
/// the committed file, 1 on a copy with one altered byte, and regeneration equals the file.
/// Runs only when `cr-cache/rules.txt` exists (the CI test jobs do not fetch the CR); with
/// `FINEPRINT_REQUIRE_CR_CACHE=1` (set by the `cr` workflow after its fetch) a missing or
/// unreadable cache fails the test instead.
#[test]
fn req_sub_001_real_cr_check_when_cached() {
    let root = workspace_root();
    let cr = root.join("cr-cache/rules.txt");
    let required = std::env::var_os("FINEPRINT_REQUIRE_CR_CACHE").is_some_and(|v| v == "1");
    if !cr.is_file() {
        assert!(
            !required,
            "FINEPRINT_REQUIRE_CR_CACHE=1 but {} is missing or not a file; fetch it with \
             `cargo xtask cr fetch`",
            cr.display()
        );
        return;
    }
    if let Err(e) = std::fs::read(&cr) {
        panic!(
            "{} exists but is unreadable ({e}); fetch it again with `cargo xtask cr fetch --force`",
            cr.display()
        );
    }
    let committed = root.join("crates/fineprint-dsl/data/vocab.json");
    let o = run(&[
        "vocab".as_ref(),
        "--cr".as_ref(),
        cr.as_os_str(),
        "--out".as_ref(),
        committed.as_os_str(),
        "--check".as_ref(),
    ]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    let regenerated =
        fineprint_ingest::vocab::generate_bytes(&std::fs::read(&cr).unwrap()).unwrap();
    assert_eq!(regenerated, std::fs::read(&committed).unwrap());
    assert_eq!(regenerated, RulesVocabulary::bundled_bytes());
    // CRLF and LF forms of the real text give the same bytes
    let text = std::fs::read(&cr).unwrap();
    let lf: Vec<u8> = String::from_utf8(text)
        .unwrap()
        .replace("\r\n", "\n")
        .into_bytes();
    assert_eq!(
        fineprint_ingest::vocab::generate_bytes(&lf).unwrap(),
        regenerated
    );
    let dir = scratch("real");
    let copy = dir.join("vocab.json");
    let mut bytes = regenerated.clone();
    let at = bytes.windows(9).position(|w| w == b"Treasure\"").unwrap();
    bytes[at] = b't';
    std::fs::write(&copy, &bytes).unwrap();
    let o = run(&[
        "vocab".as_ref(),
        "--cr".as_ref(),
        cr.as_os_str(),
        "--out".as_ref(),
        copy.as_os_str(),
        "--check".as_ref(),
    ]);
    assert_eq!(code(&o), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}
