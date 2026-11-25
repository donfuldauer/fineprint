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
    CachedFile, Request, SourceMeta, curl_args, is_fresh, parse_bulk_data, parse_source_meta,
    render_source_meta, validate_atomic_cards, validate_download_uri, validate_scryfall_bulk,
};
use super::json_array::{ArrayError, parse_elements, split_array, validate_array};
use super::mtgjson::{cross_check, parse_atomic};
use super::normalize::{
    import_oracle_cards, import_rulings, normalize_card, oracle_text_hash, split_type_line,
};
use super::recompute::{
    cmc_matches, color_identity, cost_colors, cost_mana_value, face_colors, letters_mask,
    mask_letters, record_mana_value, symbol_colors, symbol_mana_value, text_symbol_colors,
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
    assert_eq!(face_colors(Some(&c), Some(mask(2))).bits(), 2);
    assert_eq!(face_colors(Some(&c), None).bits(), 8);
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

/// REQ-ING-001, R12: download URI validation.
#[test]
fn download_uri_rules() {
    for ok in [
        "https://scryfall.com/a",
        "https://data.scryfall.io/a/b.json?x=1",
        "https://a-b.c.scryfall.com",
        "https://data.scryfall.io#frag",
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
    ] {
        assert!(validate_download_uri(bad).is_err(), "{bad}");
    }
}

/// REQ-ING-001, REQ-NFR-004: the curl arguments: HTTPS only, fail on errors, bounded size and
/// time, the headers, never insecure.
#[test]
fn curl_arguments() {
    let req = Request {
        url: "https://api.scryfall.com/bulk-data".to_owned(),
        headers: vec![("Accept".to_owned(), "application/json".to_owned())],
        max_bytes: MAX_INPUT_BYTES,
        max_time_secs: 60,
    };
    let a = curl_args(&req, Some(std::path::Path::new("x.part")));
    let joined = a.join(" ");
    for want in [
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--proto =https",
        "--proto-redir =https",
        "--max-filesize 4294967296",
        "--max-time 60",
        "--header Accept: application/json",
        "--output x.part",
    ] {
        assert!(joined.contains(want), "{want}");
    }
    assert_eq!(a.last().map(String::as_str), Some(req.url.as_str()));
    assert!(!joined.contains("--insecure") && !a.iter().any(|x| x == "-k"));
    assert!(!curl_args(&req, None).iter().any(|x| x == "--output"));
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
