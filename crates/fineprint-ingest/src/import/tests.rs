// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the Scryfall import internals (REQ-ING-001, REQ-ING-002, REQ-NFR-004, R3,
//! R12). Every input is synthetic; nothing touches the network or the clock.

use std::collections::BTreeMap;
use std::ffi::OsString;

use fineprint_dsl::card::ColorBitmask;
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol, Variable};
use serde_json::{Value, json};

use super::cli::{parse_fetch_args, parse_import_args};
use super::fetch::{
    CachedFile, MIN_CURL_VERSION, Request, SourceMeta, check_curl_version, copy_stream_capped,
    curl_args, curl_failure_message, is_fresh, parse_bulk_data, parse_curl_version,
    parse_source_meta, part_path, read_stream_capped, render_source_meta, run_capped,
    run_capped_to_file, validate_atomic_cards, validate_download_uri, validate_scryfall_bulk,
};
use super::json_array::{ArrayError, parse_elements, split_array, validate_array};
use super::mtgjson::{cross_check, parse_atomic};
use super::normalize::{
    import_oracle_cards, import_rulings, normalize_card, oracle_text_hash, split_type_line,
};
use super::recompute::{
    cmc_matches, color_identity, cost_colors, cost_mana_value, face_colors, letters_mask,
    mask_letters, record_colors, record_mana_value, symbol_colors, symbol_mana_value,
    text_symbol_colors,
};
use super::{MAX_INPUT_BYTES, is_uuid_shape, read_capped};

/// A fixed-seed xorshift64 PRNG (deterministic, R3).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn bytes(&mut self, max_len: u64) -> Vec<u8> {
        let len = self.below(max_len);
        (0..len).map(|_| (self.next() & 0xff) as u8).collect()
    }
}

fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).unwrap()
}

/// REQ-ING-002 (CR 202.3, 202.3e, 202.3f, 202.3g; CR 202.2, 202.2d): per-symbol mana value and
/// colors for every symbol kind.
#[test]
fn symbol_tables() {
    use ManaSymbol as S;
    let cases = [
        (S::Generic(0), 0, 0),
        (S::Generic(7), 7, 0),
        (S::Colored(Color::Red), 1, 8),
        (S::Colorless, 1, 0),
        (S::Snow, 1, 0),
        (S::Variable(Variable::X), 0, 0),
        (S::Variable(Variable::Y), 0, 0),
        (S::Variable(Variable::Z), 0, 0),
        (S::Hybrid(Color::White, Color::Blue), 1, 3),
        (S::GenericHybrid(2, Color::Black), 2, 4),
        (S::Phyrexian(Color::Green), 1, 16),
        (S::HybridPhyrexian(Color::Red, Color::White), 1, 9),
    ];
    for (s, mv, bits) in cases {
        assert_eq!(symbol_mana_value(s), mv, "{s:?}");
        assert_eq!(symbol_colors(s).bits(), bits, "{s:?}");
    }
    assert_eq!(cost_mana_value(None), 0);
    assert_eq!(cost_colors(None), ColorBitmask::NONE);
    let big = ManaCostDSL::parse("{1000000}{1000000}").unwrap();
    assert_eq!(cost_mana_value(Some(&big)), 2_000_000);
    let c = ManaCostDSL::parse("{R}").unwrap();
    // REQ-ING-002 (CR 105.2, 202.2e, 204.2): cost and indicator colors are united.
    assert_eq!(face_colors(Some(&c), Some(mask(2))).bits(), 10);
    assert_eq!(face_colors(Some(&c), None).bits(), 8);
    assert_eq!(face_colors(None, Some(mask(4))).bits(), 4);
    assert_eq!(face_colors(None, None).bits(), 0);
}

/// REQ-ING-002 (CR 709.4, 709.4b, 716.4, 710.2, 712.6a): card-level colors by layout.
#[test]
fn record_colors_rules() {
    let (r, u, g) = (mask(8), mask(2), mask(16));
    assert_eq!(record_colors("split", &[r, u]).bits(), 10);
    assert_eq!(record_colors("split", &[r]).bits(), 8);
    for layout in [
        "adventure",
        "flip",
        "transform",
        "modal_dfc",
        "meld",
        "test_layout",
    ] {
        assert_eq!(record_colors(layout, &[g, u]).bits(), 16, "{layout}");
    }
    assert_eq!(record_colors("split", &[]).bits(), 0);
    assert_eq!(record_colors("transform", &[]).bits(), 0);
}

/// REQ-ING-002 (CR 202.3b, 202.3d): the record-level rule by layout.
#[test]
fn record_mana_value_rules() {
    let a = ManaCostDSL::parse("{2}{R}").unwrap();
    let b = ManaCostDSL::parse("{4}").unwrap();
    assert_eq!(record_mana_value("split", &[Some(&a), Some(&b)]), 7);
    assert_eq!(record_mana_value("split", &[Some(&a)]), 3);
    assert_eq!(record_mana_value("transform", &[Some(&a), Some(&b)]), 3);
    assert_eq!(record_mana_value("adventure", &[Some(&b), Some(&a)]), 4);
    assert_eq!(record_mana_value("normal", &[]), 0);
    assert_eq!(record_mana_value("split", &[None, None]), 0);
}

/// REQ-ING-002 (CR 903.4, 903.4c): reminder text is excluded; the scan never panics.
#[test]
fn text_symbols_and_identity() {
    assert_eq!(text_symbol_colors("{G}").bits(), 16);
    assert_eq!(text_symbol_colors("({G})").bits(), 0);
    assert_eq!(text_symbol_colors("(({G}){R}){U}").bits(), 2);
    assert_eq!(text_symbol_colors("{G({R})}").bits(), 0);
    assert_eq!(text_symbol_colors("}{W}{").bits(), 1);
    assert_eq!(text_symbol_colors("{{B}}").bits(), 4);
    assert_eq!(text_symbol_colors("\u{1F600}{\u{00e9}}{R}").bits(), 8);
    let id = color_identity(&[
        (mask(1), Some("{G}")),
        (mask(4), None),
        (mask(0), Some("({R})")),
    ]);
    assert_eq!(id.bits(), 1 | 4 | 16);
    assert_eq!(color_identity(&[]).bits(), 0);
}

/// REQ-ING-002: letters and masks, and the exact `*2` cmc rule.
#[test]
fn letters_and_cmc() {
    assert_eq!(
        letters_mask(["W", "U", "B", "R", "G"]).map(|m| m.bits()),
        Some(31)
    );
    assert_eq!(letters_mask([]).map(|m| m.bits()), Some(0));
    assert_eq!(letters_mask(["w"]), None);
    assert_eq!(letters_mask([""]), None);
    assert_eq!(letters_mask(["WU"]), None);
    assert_eq!(mask_letters(mask(31)), "WUBRG");
    assert_eq!(mask_letters(mask(0)), "");
    assert!(cmc_matches(3.0, 3));
    assert!(cmc_matches(0.0, 0));
    assert!(!cmc_matches(0.5, 0));
    assert!(!cmc_matches(2.9, 3));
    assert!(!cmc_matches(f64::NAN, 0));
    assert!(!cmc_matches(f64::INFINITY, 0));
    assert!(!cmc_matches(-2.0, 0));
    assert!(!cmc_matches(1e300, 0));
    assert!(!cmc_matches(4.0, u64::MAX));
}

/// REQ-ING-002: the type-line split and the Oracle text hash.
#[test]
fn type_line_and_hash() {
    let (s, t, sub) = split_type_line("World Elite Host Ongoing Test \u{2014} A \u{2014} B");
    assert_eq!(s, vec!["World", "Elite", "Host", "Ongoing"]);
    assert_eq!(t, vec!["Test"]);
    assert_eq!(sub, vec!["A", "\u{2014}", "B"]);
    let (s, t, sub) = split_type_line("");
    assert!(s.is_empty() && t.is_empty() && sub.is_empty());
    assert_eq!(
        oracle_text_hash(&[Some("a"), None, Some("b")]),
        fineprint_base::hash::sha256(b"a\n\nb").to_hex()
    );
}

/// REQ-ING-001: the oracle_id shape.
#[test]
fn uuid_shape() {
    assert!(is_uuid_shape("0123abcd-0000-4000-8000-00000000000f"));
    assert!(!is_uuid_shape("0123ABCD-0000-4000-8000-00000000000f"));
    assert!(!is_uuid_shape("0123abcd00000-4000-8000-00000000000f"));
    assert!(!is_uuid_shape("0123abcd-0000-4000-8000-00000000000"));
    assert!(!is_uuid_shape("0123abcd-0000-4000-8000-00000000000g"));
    assert!(!is_uuid_shape("0123abcd-0000-4000-8000-0000000000\u{e9}"));
    assert!(!is_uuid_shape(""));
}

/// R12, REQ-ING-001: the top-level array splitter.
#[test]
fn array_splitter() {
    assert_eq!(split_array(b" [ ] ").unwrap().len(), 0);
    assert_eq!(
        split_array(b"[1, \"a,]\\\"\" , {\"x\":[1,2]}]").unwrap(),
        vec![&b"1"[..], &b"\"a,]\\\"\""[..], &b"{\"x\":[1,2]}"[..]]
    );
    assert_eq!(split_array(b"[1,]").unwrap(), vec![&b"1"[..], &b""[..]]);
    assert_eq!(split_array(b"{}"), Err(ArrayError::NotArray));
    assert_eq!(split_array(b""), Err(ArrayError::NotArray));
    assert_eq!(split_array(b"[1"), Err(ArrayError::Malformed));
    assert_eq!(split_array(b"[1]x"), Err(ArrayError::Malformed));
    assert_eq!(split_array(b"[]x"), Err(ArrayError::Malformed));
    assert_eq!(split_array(b"[1}]"), Err(ArrayError::Malformed));
    assert_eq!(split_array(b"[\"abc]"), Err(ArrayError::Malformed));
    assert_eq!(split_array(&[b'[', 0xff, b']']), Err(ArrayError::NotUtf8));
    assert_eq!(validate_array(b"[1,{}]"), Ok(2));
    assert_eq!(validate_array(b"[1,,2]"), Err(ArrayError::Malformed));
    assert_eq!(validate_array(b"[[}]"), Err(ArrayError::Malformed));
    let parts = parse_elements(b"[1, tru]").unwrap();
    assert_eq!(parts, vec![Ok(json!(1)), Err(())]);
    for e in [
        ArrayError::NotUtf8,
        ArrayError::NotArray,
        ArrayError::Malformed,
    ] {
        assert!(!e.to_string().is_empty());
    }
}

/// REQ-ING-001, R12: download URI validation, including the curl glob characters (F35) and the
/// excluded API host `api.scryfall.com` (F37).
#[test]
fn download_uri_rules() {
    for ok in [
        "https://scryfall.com/a",
        "https://data.scryfall.io/a/b.json?x=1",
        "https://a-b.c.scryfall.com",
        "https://data.scryfall.io#frag",
        "https://c2.scryfall.com/x.json",
        "https://xapi.scryfall.com/a",
        "https://api.scryfall.io/a",
    ] {
        assert_eq!(validate_download_uri(ok), Ok(()), "{ok}");
    }
    let long = format!("https://data.scryfall.io/{}", "a".repeat(3000));
    for bad in [
        "http://scryfall.com/a",
        "https://scryfall.io/a",
        "https://xscryfall.com/a",
        "https://scryfall.com.evil.example/a",
        "https://a@scryfall.com/a",
        "https://scryfall.com:443/a",
        "https://scryfall.com /a",
        "https://scryfall.com\n/a",
        "https://a..scryfall.io/a",
        "https://a_b.scryfall.io/a",
        "https://",
        "HTTPS://scryfall.com/a",
        long.as_str(),
        // F35: curl URL globbing characters.
        "https://data.scryfall.io/a[1-9].json",
        "https://data.scryfall.io/{a,b}.json",
        "https://data.scryfall.io/a]",
        "https://data.scryfall.io/a?x={",
        "https://data.scryfall.io/a#}",
        // F37: the API host is never a download host.
        "https://api.scryfall.com/bulk-data",
        "https://api.scryfall.com",
        "https://api.scryfall.com?x=1",
        "https://API.SCRYFALL.COM/bulk-data",
    ] {
        assert!(validate_download_uri(bad).is_err(), "{bad}");
    }
    assert_eq!(
        validate_download_uri("https://api.scryfall.com/x"),
        Err("the download URI names the Scryfall API host")
    );
}

fn lossy(a: &[OsString]) -> Vec<String> {
    a.iter().map(|x| x.to_string_lossy().into_owned()).collect()
}

/// REQ-ING-001, REQ-NFR-004, R12: the curl arguments: `--disable` first (F36: no `~/.curlrc`),
/// `--globoff` (F35), HTTPS only, no redirect following (F15: neither `--location` nor
/// `--proto-redir`), fail on errors, bounded size and time, the headers, never insecure.
#[test]
fn curl_arguments() {
    let req = Request {
        url: "https://api.scryfall.com/bulk-data".to_owned(),
        headers: vec![("Accept".to_owned(), "application/json".to_owned())],
        max_bytes: MAX_INPUT_BYTES,
        max_time_secs: 60,
    };
    let a = lossy(&curl_args(&req, Some(std::path::Path::new("x.part"))));
    assert_eq!(a.first().map(String::as_str), Some("--disable"));
    let joined = a.join(" ");
    for want in [
        "--fail",
        "--silent",
        "--show-error",
        "--globoff",
        "--proto =https",
        "--max-filesize 4294967296",
        "--max-time 60",
        "--header Accept: application/json",
        "--output x.part",
    ] {
        assert!(joined.contains(want), "{want}");
    }
    for absent in ["--location", "-L", "--proto-redir", "--insecure", "-k"] {
        assert!(!a.iter().any(|x| x == absent), "{absent}");
    }
    assert_eq!(a.last().map(String::as_str), Some(req.url.as_str()));
    assert!(!joined.contains("--insecure"));
    let stdout_args = lossy(&curl_args(&req, None));
    assert!(!stdout_args.iter().any(|x| x == "--output"));
    assert_eq!(stdout_args.first().map(String::as_str), Some("--disable"));
}

/// REQ-ING-001, REQ-NFR-004 (F33): a non-UTF-8 destination path reaches curl byte-exact.
#[cfg(unix)]
#[test]
fn curl_output_path_is_byte_exact() {
    use std::os::unix::ffi::OsStrExt;
    let raw: &[u8] = b"/tmp/cache-\xff\xfe/oracle-cards.json.1.part";
    let dest = std::path::Path::new(std::ffi::OsStr::from_bytes(raw));
    let req = Request {
        url: "https://data.scryfall.io/a.json".to_owned(),
        headers: Vec::new(),
        max_bytes: 1,
        max_time_secs: 1,
    };
    let a = curl_args(&req, Some(dest));
    let i = a.iter().position(|x| x == "--output").unwrap();
    assert_eq!(a[i + 1].as_bytes(), raw);
}

/// REQ-ING-001, R12 (F34): a curl failure names the role and exit status and at most 200 bytes
/// of the first stderr line with control characters replaced; never the URL.
#[test]
fn curl_failure_messages() {
    assert_eq!(
        curl_failure_message("rulings.json", Some(22), b""),
        "curl failed for rulings.json (exit 22)"
    );
    assert_eq!(
        curl_failure_message(
            "bulk-data metadata",
            Some(6),
            b"\ncurl: (6) Could not resolve host\nsecond line\n"
        ),
        "curl failed for bulk-data metadata (exit 6): curl: (6) Could not resolve host"
    );
    assert_eq!(
        curl_failure_message("AtomicCards.json", None, b"a\x1b[31mb\tc"),
        "curl failed for AtomicCards.json (no exit code): a?[31mb?c"
    );
    // A long line is cut to 200 bytes on a character boundary.
    let long = "é".repeat(150);
    let m = curl_failure_message("oracle-cards.json", Some(28), long.as_bytes());
    let excerpt = m.split_once(": ").unwrap().1;
    assert_eq!(excerpt.len(), 200);
    assert!(excerpt.chars().all(|c| c == 'é'));
    // Invalid UTF-8 never panics.
    let m = curl_failure_message("oracle-cards.json", Some(1), &[0xff; 500]);
    assert!(m.len() < 300, "{m}");
}

/// REQ-NFR-004, R12 (F12): the stream reader keeps at most `cap` bytes and names the cap on
/// overflow.
#[test]
fn stream_reader_is_capped() {
    assert_eq!(read_stream_capped(&b""[..], 4).unwrap(), b"");
    assert_eq!(read_stream_capped(&b"abcd"[..], 4).unwrap(), b"abcd");
    let e = read_stream_capped(&b"abcde"[..], 4).unwrap_err();
    assert!(e.contains("4-byte cap"), "{e}");
    // An endless reader stops at cap + 1 bytes.
    let e = read_stream_capped(std::io::repeat(b'y'), 1024 * 1024).unwrap_err();
    assert!(e.contains("1048576-byte cap"), "{e}");
}

/// REQ-NFR-004, R12 (F12): a child producing endless output is stopped at the cap and killed.
#[cfg(unix)]
#[test]
fn capped_command_is_killed_past_the_cap() {
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", "yes"]);
    let e = run_capped(cmd, 1024, "test.role").unwrap_err();
    assert!(
        e.contains("1024-byte cap") && e.contains("test.role"),
        "{e}"
    );
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", "printf abc"]);
    assert_eq!(run_capped(cmd, 1024, "test.role").unwrap(), b"abc");
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", "echo 'oops' >&2; exit 3"]);
    assert_eq!(
        run_capped(cmd, 1024, "test.role").unwrap_err(),
        "curl failed for test.role (exit 3): oops"
    );
}

/// REQ-NFR-004, R12 (F12; gate R5, brief clause R5-3): the `curl --version` parser and the
/// ADVISORY verdict: 8.4.0 or later passes silently, an older or unparsable version is a
/// warning naming 8.4.0 and saying the in-process cap still applies; never an error.
#[test]
fn curl_version_check() {
    assert_eq!(MIN_CURL_VERSION, (8, 4, 0));
    let line = |v: &str| format!("curl {v} (x86_64-pc-linux-gnu) libcurl/{v}\nRelease-Date: x\n");
    assert_eq!(parse_curl_version(&line("8.4.0")), Some((8, 4, 0)));
    assert_eq!(parse_curl_version(&line("8.11.1-DEV")), Some((8, 11, 1)));
    assert_eq!(parse_curl_version(&line("9.0")), Some((9, 0, 0)));
    for bad in [
        "",
        "libcurl 8.4.0",
        "curl",
        "curl x.y.z",
        "curl 8",
        "\ncurl 8.4.0",
    ] {
        assert_eq!(parse_curl_version(bad), None, "{bad:?}");
    }
    for ok in ["8.4.0", "8.4.1", "8.10.0", "9.0.0"] {
        assert_eq!(check_curl_version(&line(ok)), Ok(None), "{ok}");
    }
    for old in ["8.3.99", "7.88.1", "8.0.0"] {
        let w = check_curl_version(&line(old)).unwrap().unwrap();
        assert!(
            w.starts_with("warning:")
                && w.contains("8.4.0")
                && w.contains("in-process")
                && w.contains("cap"),
            "{w}"
        );
    }
    for bad in ["garbage", "", "curl x.y.z", "libcurl 8.4.0"] {
        let w = check_curl_version(bad).unwrap().unwrap();
        assert!(
            w.starts_with("warning:")
                && w.contains("8.4.0")
                && w.contains("in-process")
                && w.contains("cap"),
            "{w}"
        );
    }
}

/// A new, empty scratch directory created exclusively under the private root
/// `<workspace>/target/fp-import-tests`. The leaf name combines the pid, a counter and the
/// clock (for uniqueness only, R7); an existing leaf is retried with a new name.
fn scratch(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fp-import-tests");
    std::fs::create_dir_all(&root).expect("create the scratch root");
    for _ in 0..16 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let leaf = root.join(format!("{tag}-{}-{n}-{nanos}", std::process::id()));
        match std::fs::create_dir(&leaf) {
            Ok(()) => return leaf,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => panic!("cannot create scratch dir {}: {e}", leaf.display()),
        }
    }
    panic!("no fresh scratch dir under {}", root.display());
}

/// A part file that already exists (as `create_part` makes it), holding stale bytes that the
/// download must truncate.
fn existing_part(dir: &std::path::Path) -> std::path::PathBuf {
    let p = dir.join("test.part");
    std::fs::write(&p, b"stale bytes that must be truncated").unwrap();
    p
}

fn sh(script: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", script]);
    cmd
}

/// REQ-NFR-004, REQ-ING-001, R12 (R5) T1: an endless producer streamed into a part file is
/// stopped at the cap and killed (the call returns); the error names the cap and the role and
/// the file holds at most `cap` bytes.
#[cfg(unix)]
#[test]
fn capped_download_to_file_kills_an_endless_producer() {
    let dir = scratch("capped-endless");
    let part = existing_part(&dir);
    let e = run_capped_to_file(sh("yes"), 1024, "test.role", &part).unwrap_err();
    assert!(
        e.contains("1024-byte cap") && e.contains("test.role") && e.contains("stopped"),
        "{e}"
    );
    let len = std::fs::metadata(&part).unwrap().len();
    assert!(len <= 1024, "{len}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// REQ-NFR-004, REQ-ING-001, R12 (R5) T2: output exactly at the cap or under it is written
/// byte for byte (the stale content is truncated); a non-zero exit is a curl failure message.
#[cfg(unix)]
#[test]
fn capped_download_to_file_writes_exact_bytes() {
    let dir = scratch("capped-exact");
    let part = existing_part(&dir);
    // Exactly at the cap: 1024 bytes.
    run_capped_to_file(
        sh("head -c 1024 /dev/zero | tr '\\0' 'a'"),
        1024,
        "test.role",
        &part,
    )
    .unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), vec![b'a'; 1024]);
    // One byte over the cap is refused, and no byte past the cap is written.
    let e = run_capped_to_file(
        sh("head -c 1025 /dev/zero | tr '\\0' 'b'"),
        1024,
        "test.role",
        &part,
    )
    .unwrap_err();
    assert!(e.contains("1024-byte cap"), "{e}");
    assert!(std::fs::metadata(&part).unwrap().len() <= 1024);
    // Under the cap.
    run_capped_to_file(sh("printf abc"), 1024, "test.role", &part).unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), b"abc");
    // A non-zero exit.
    assert_eq!(
        run_capped_to_file(sh("echo 'oops' >&2; exit 3"), 1024, "test.role", &part).unwrap_err(),
        "curl failed for test.role (exit 3): oops"
    );
    // The part file is never created by the download: a missing one is an error.
    let missing = dir.join("missing.part");
    assert!(run_capped_to_file(sh("printf abc"), 1024, "test.role", &missing).is_err());
    assert!(!missing.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// REQ-NFR-004, R12 (R5): the streaming copy never writes more than the cap, whatever the
/// chunking, and reads at most `cap + 1` bytes of an endless reader.
#[test]
fn copy_stream_is_capped() {
    let mut out = Vec::new();
    assert_eq!(copy_stream_capped(&b"abcd"[..], &mut out, 4), Ok(4));
    assert_eq!(out, b"abcd");
    let mut out = Vec::new();
    assert_eq!(copy_stream_capped(&b""[..], &mut out, 0), Ok(0));
    let mut out = Vec::new();
    let e = copy_stream_capped(&b"abcde"[..], &mut out, 4).unwrap_err();
    assert!(e.contains("4-byte cap"), "{e}");
    assert!(out.len() <= 4);
    // An endless reader, a cap larger than the 64 KiB buffer and not a multiple of it.
    let cap = 200 * 1024 + 7;
    let mut out = Vec::new();
    let e = copy_stream_capped(std::io::repeat(b'y'), &mut out, cap).unwrap_err();
    assert!(e.contains(&format!("{cap}-byte cap")), "{e}");
    assert!(u64::try_from(out.len()).unwrap() <= cap);
    // Exactly the cap across several buffers.
    let body = vec![b'z'; usize::try_from(cap).unwrap()];
    let mut out = Vec::new();
    assert_eq!(copy_stream_capped(&body[..], &mut out, cap), Ok(cap));
    assert_eq!(out, body);
}

/// REQ-NFR-004, REQ-ING-001, R12 (R5) T3: the download arguments write to stdout (no
/// `--output`) and keep `--max-filesize` as a second line of defence.
#[test]
fn download_arguments_stream_to_stdout() {
    let req = Request {
        url: "https://data.scryfall.io/a.json".to_owned(),
        headers: Vec::new(),
        max_bytes: 12_345,
        max_time_secs: 1,
    };
    let a = lossy(&curl_args(&req, None));
    assert!(!a.iter().any(|x| x == "--output" || x == "-o"));
    assert!(a.join(" ").contains("--max-filesize 12345"));
}

/// REQ-NFR-004, REQ-ING-001, R12 (R5) T4: the cap does not depend on the version verdict:
/// for every banner (current, older, garbage) the verdict is never an error and the same
/// streaming download is capped.
#[cfg(unix)]
#[test]
fn download_cap_does_not_depend_on_the_curl_version() {
    let dir = scratch("capped-version");
    for banner in [
        "curl 8.4.0 (x) libcurl/8.4.0",
        "curl 8.4.0 (x) libcurl/8.3.0",
        "curl 7.88.1 (x) libcurl/7.88.1",
        "garbage",
        "",
    ] {
        assert!(check_curl_version(banner).is_ok(), "{banner:?}");
        let part = existing_part(&dir);
        let e = run_capped_to_file(sh("yes"), 4096, "test.role", &part).unwrap_err();
        assert!(e.contains("4096-byte cap"), "{banner:?}: {e}");
        assert!(std::fs::metadata(&part).unwrap().len() <= 4096);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// REQ-ING-001 (F14): each run stages into its own `{file}.{pid}.part`.
#[test]
fn part_names_are_per_run() {
    let p = part_path(std::path::Path::new("d"), "rulings.json");
    assert_eq!(
        p,
        std::path::Path::new("d").join(format!("rulings.json.{}.part", std::process::id()))
    );
}

/// REQ-ING-001: bulk-data and source.json parsing, and the freshness rule.
#[test]
fn metadata_parsing_and_freshness() {
    let doc = json!({"data": [
        {"type": "rulings", "updated_at": "r", "download_uri": "https://data.scryfall.io/r"},
        {"type": "oracle_cards", "updated_at": "o", "download_uri": "https://data.scryfall.io/o"},
        {"type": "oracle_cards", "updated_at": "second", "download_uri": "https://x"},
    ]});
    let (o, r) = parse_bulk_data(&serde_json::to_vec(&doc).unwrap()).unwrap();
    assert_eq!(o.updated_at, "o");
    assert_eq!(r.download_uri, "https://data.scryfall.io/r");
    let long = json!({"data": [
        {"type": "oracle_cards", "updated_at": "x".repeat(300), "download_uri": "https://x"},
        {"type": "rulings", "updated_at": "r", "download_uri": "https://x"},
    ]});
    assert!(parse_bulk_data(&serde_json::to_vec(&long).unwrap()).is_err());

    let mut meta = SourceMeta::new();
    meta.insert(
        "f.json".to_owned(),
        CachedFile {
            source_url: "https://data.scryfall.io/f".to_owned(),
            updated_at: None,
            sha256: "00".to_owned(),
            byte_count: 2,
            fetched_at_epoch_seconds: 100,
        },
    );
    let bytes = render_source_meta(&meta);
    assert_eq!(parse_source_meta(&bytes).unwrap(), meta);
    let partial = json!({"files": {"a": {"sourceUrl": "x"}, "b": 3}});
    assert!(
        parse_source_meta(&serde_json::to_vec(&partial).unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(parse_source_meta(b"[]").is_err());
    assert!(parse_source_meta(b"x").is_err());

    // Freshness needs the file on disk; a missing directory is never fresh.
    let nowhere = std::path::Path::new("/nonexistent-fineprint-dir");
    assert!(!is_fresh(&meta, nowhere, "f.json", 101));
    assert!(!is_fresh(&meta, nowhere, "g.json", 101));
}

/// REQ-ING-001, REQ-NFR-004: the download validators.
#[test]
fn download_validators() {
    assert!(validate_scryfall_bulk(b"[]").is_ok());
    assert!(validate_scryfall_bulk(b"").is_err());
    assert!(validate_scryfall_bulk(&[0xc3]).is_err());
    assert!(validate_scryfall_bulk(b"{}").is_err());
    assert_eq!(validate_atomic_cards(b"{\"data\":{}}"), Ok(None));
    assert_eq!(
        validate_atomic_cards(b"{\"data\":{},\"meta\":{\"date\":\"d\"}}"),
        Ok(Some("d".to_owned()))
    );
    assert!(validate_atomic_cards(b"{\"data\":[]}").is_err());
    assert!(validate_atomic_cards(b"{").is_err());
}

/// R12: `read_capped` refuses a file above the cap.
#[test]
fn read_capped_refuses_large_files() {
    let path = std::env::temp_dir().join(format!(
        "fineprint-ingest-import-cap-{}.bin",
        std::process::id()
    ));
    std::fs::write(&path, b"0123456789").unwrap();
    assert_eq!(read_capped(&path, 10).unwrap().len(), 10);
    let err = read_capped(&path, 3).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    let _ = std::fs::remove_file(&path);
    assert!(read_capped(&path, 3).is_err());
}

/// REQ-ING-002: CLI option parsing (an option is never a value).
#[test]
fn cli_argument_parsing() {
    let os = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
    let a = parse_fetch_args(&os(&["--force", "--mtgjson", "--cache", "c"])).unwrap();
    assert!(a.force && a.mtgjson);
    assert_eq!(a.cache, std::path::PathBuf::from("c"));
    assert_eq!(
        parse_fetch_args(&[]).unwrap().cache,
        std::path::PathBuf::from("ingest-cache")
    );
    let a = parse_import_args(&os(&["--strict", "--out", "-", "--mtgjson-report", "r"])).unwrap();
    assert!(a.strict);
    assert_eq!(a.out, Some(OsString::from("-")));
    for bad in [
        vec!["--out", "--help"],
        vec!["--mtgjson-report"],
        vec!["--mtgjson-report", "a", "--mtgjson-report", "b"],
        vec!["x"],
    ] {
        assert!(parse_import_args(&os(&bad)).is_err(), "{bad:?}");
    }
}

/// REQ-ING-001: the rulings import keeps source order per id.
#[test]
fn rulings_unit() {
    let card = json!({"oracle_id": "00000000-0000-4000-8000-000000000001", "name": "t", "layout": "normal"});
    let recs = import_oracle_cards(&serde_json::to_vec(&json!([card])).unwrap())
        .unwrap()
        .records;
    let r = json!([
        {"oracle_id": "00000000-0000-4000-8000-000000000001", "source": "s", "published_at": "p", "comment": "c1"},
        {"oracle_id": "00000000-0000-4000-8000-000000000001", "source": "s", "published_at": "p", "comment": "c2"},
        {"oracle_id": "BAD", "source": "s", "published_at": "p", "comment": "c"},
    ]);
    let out = import_rulings(&serde_json::to_vec(&r).unwrap(), &recs).unwrap();
    let list = &out.rulings["00000000-0000-4000-8000-000000000001"];
    assert_eq!(
        list.iter().map(|x| x.comment.as_str()).collect::<Vec<_>>(),
        vec!["c1", "c2"]
    );
    assert_eq!(out.invalid, 1);
    assert!(import_rulings(b"{}", &recs).is_err());
}

const FIELDS: [&str; 30] = [
    "oracle_id",
    "name",
    "layout",
    "mana_cost",
    "cmc",
    "type_line",
    "oracle_text",
    "power",
    "toughness",
    "loyalty",
    "defense",
    "colors",
    "color_indicator",
    "color_identity",
    "keywords",
    "legalities",
    "game_changer",
    "digital",
    "games",
    "all_parts",
    "card_faces",
    "id",
    "component",
    "identifiers",
    "scryfallOracleId",
    "manaValue",
    "types",
    "data",
    "type",
    "download_uri",
];

const STRINGS: [&str; 14] = [
    "",
    "00000000-0000-4000-8000-000000000001",
    "00000000-0000-4000-8000-000000000002",
    "{2}{W}",
    "{Q}",
    "W",
    "G",
    "normal",
    "split",
    "Creature \u{2014} Test",
    "A // B",
    "(x {G}) {R}",
    "\u{1F600}{",
    "https://data.scryfall.io/x",
];

fn pick(rng: &mut Rng, l: &[&str]) -> String {
    let i = usize::try_from(rng.below(l.len() as u64)).unwrap();
    l[i].to_owned()
}

/// A random JSON value with keys from [`FIELDS`] and strings from [`STRINGS`] (structurally
/// valid JSON with arbitrary types).
fn random_value(rng: &mut Rng, depth: u32) -> Value {
    match rng.below(if depth == 0 { 5 } else { 7 }) {
        0 => Value::Null,
        1 => Value::Bool(rng.below(2) == 0),
        2 => json!(rng.below(10)),
        3 => json!(rng.below(20) as f64 / 2.0 - 1.0),
        4 => Value::String(pick(rng, &STRINGS)),
        5 => Value::Array(
            (0..rng.below(4))
                .map(|_| random_value(rng, depth - 1))
                .collect(),
        ),
        _ => Value::Object(
            (0..rng.below(8))
                .map(|_| (pick(rng, &FIELDS), random_value(rng, depth - 1)))
                .collect(),
        ),
    }
}

/// R12, REQ-ING-001, REQ-ING-002: every parser entry point survives 2,500 bounded random byte
/// strings (lengths 0..4096, fixed-seed xorshift64) and 2,500 structurally valid JSON values
/// with wrong types, with no panic and a clean error or rejection.
#[test]
fn parsers_survive_random_input() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let empty = BTreeMap::new();
    for i in 0..2_500u32 {
        let mut b = rng.bytes(4096);
        // Bias a share of the inputs towards JSON-looking prefixes.
        match i % 5 {
            0 => b.insert(0, b'['),
            1 => b.insert(0, b'{'),
            _ => {}
        }
        let _ = split_array(&b);
        let _ = validate_array(&b);
        let _ = import_oracle_cards(&b);
        let _ = import_rulings(&b, &empty);
        let _ = parse_atomic(&b);
        let _ = parse_bulk_data(&b);
        let _ = parse_source_meta(&b);
        let _ = validate_scryfall_bulk(&b);
        let _ = validate_atomic_cards(&b);
        let s = String::from_utf8_lossy(&b);
        let _ = validate_download_uri(&s);
        let _ = parse_curl_version(&s);
        let _ = check_curl_version(&s);
        let _ = curl_failure_message("test.role", Some(1), &b);
        let _ = text_symbol_colors(&s);
        let _ = split_type_line(&s);
        let args: Vec<OsString> = s.split(['\0', ' ']).map(OsString::from).collect();
        let _ = parse_fetch_args(&args);
        let _ = parse_import_args(&args);
    }
    let mut rng = Rng(0x0123_4567_89ab_cdef);
    for _ in 0..2_500 {
        let v = random_value(&mut rng, 4);
        let _ = normalize_card(&v);
        let bytes = serde_json::to_vec(&Value::Array(vec![v.clone(), v.clone()])).unwrap();
        let out = import_oracle_cards(&bytes).unwrap();
        let _ = import_rulings(&bytes, &out.records);
        let one = serde_json::to_vec(&v).unwrap();
        let _ = parse_bulk_data(&one);
        let _ = parse_source_meta(&one);
        let _ = validate_atomic_cards(&one);
        if let Ok(a) = parse_atomic(&one) {
            let _ = cross_check(&out.records, &a, &Value::Null);
        }
        let wrapped = serde_json::to_vec(&json!({"data": {"n": [v.clone()], "m": v}})).unwrap();
        if let Ok(a) = parse_atomic(&wrapped) {
            let _ = cross_check(&out.records, &a, &Value::Null);
        }
    }
}

/// R3: a normalized record's JSON is a pure function of the input.
#[test]
fn normalization_is_deterministic() {
    let mut rng = Rng(42);
    for _ in 0..200 {
        let v = random_value(&mut rng, 4);
        let a = normalize_card(&v).map(|r| r.to_json_value().to_string());
        let b = normalize_card(&v).map(|r| r.to_json_value().to_string());
        assert_eq!(a, b);
    }
}
