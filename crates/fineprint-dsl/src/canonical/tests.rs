// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for RFC 8785 canonical JSON (REQ-DSL-001, REQ-NFR-006, REQ-CORE-006).

use std::collections::BTreeMap;

use fineprint_base::prng::Pcg64Dxsm;

use super::*;

fn canon(text: &str) -> String {
    String::from_utf8(canonicalize(text.as_bytes()).expect("canonicalize")).expect("utf8")
}

/// REQ-NFR-006: members sorted, whitespace removed, nested objects sorted too.
#[test]
fn req_nfr_006_sorts_members_and_strips_whitespace() {
    assert_eq!(
        canon("{ \"b\" : 1 ,\n \"a\" : [ 3 , { \"z\" : true , \"y\" : null } ] }"),
        r#"{"a":[3,{"y":null,"z":true}],"b":1}"#
    );
    assert_eq!(canon(" [ ] "), "[]");
    assert_eq!(canon("{}"), "{}");
    assert_eq!(canon("\"x\""), "\"x\"");
    assert_eq!(canon("false"), "false");
}

/// REQ-NFR-006: keys sort by UTF-16 code units: U+1F602 (surrogates D83D DE02) sorts before
/// U+FB33, the reverse of UTF-8 / code point order.
#[test]
fn req_nfr_006_utf16_key_order() {
    let input = "{\"\u{FB33}\":1,\"\u{1F602}\":2}";
    assert_eq!(canon(input), "{\"\u{1F602}\":2,\"\u{FB33}\":1}");
    // Plain ASCII order and prefix order.
    assert_eq!(canon(r#"{"ab":1,"a":2,"B":3}"#), r#"{"B":3,"a":2,"ab":1}"#);
}

/// REQ-NFR-006: ECMAScript number formatting rows derivable from the JCS rules.
#[test]
fn req_nfr_006_number_formatting() {
    let rows = [
        ("0", "0"),
        ("-0", "0"),
        ("1.0", "1"),
        ("1.5", "1.5"),
        ("-1.25", "-1.25"),
        ("1e2", "100"),
        ("1E2", "100"),
        ("1e-7", "1e-7"),
        ("2E-7", "2e-7"),
        ("0.000001", "0.000001"),
        ("123456789e-3", "123456.789"),
        ("9007199254740991", "9007199254740991"),
        ("-9007199254740991", "-9007199254740991"),
        ("5e-324", "5e-324"),
        ("0.1", "0.1"),
    ];
    for (input, expected) in rows {
        assert_eq!(canon(input), expected, "input {input}");
    }
}

/// REQ-NFR-006: minimal string escaping.
#[test]
fn req_nfr_006_string_escapes() {
    assert_eq!(canon(r#""\u0000\u001f""#), r#""\u0000\u001f""#);
    assert_eq!(
        canon(r#""\u0008\u0009\u000a\u000c\u000d""#),
        r#""\b\t\n\f\r""#
    );
    assert_eq!(canon(r#""\u000b\u0001""#), r#""\u000b\u0001""#);
    assert_eq!(canon(r#""\"\\\/""#), r#""\"\\/""#);
    assert_eq!(canon(r#""\u007fé€""#), "\"\u{7f}\u{e9}\u{20ac}\"");
    assert_eq!(canon(r#""😂""#), "\"\u{1F602}\"");
    assert_eq!(canon(r#""A""#), "\"A\"");
}

/// REQ-NFR-006: a lone surrogate escape is not valid I-JSON.
#[test]
fn req_nfr_006_lone_surrogate_rejected() {
    for input in [r#""\ud800""#, r#""\udc00x""#, r#"{"\ud83d":1}"#] {
        assert!(
            matches!(
                canonicalize(input.as_bytes()),
                Err(CanonicalError::Syntax(_))
            ),
            "{input}"
        );
    }
}

/// REQ-NFR-006: duplicate member names rejected at any depth, after string decoding.
#[test]
fn req_nfr_006_duplicate_keys_rejected() {
    let cases = [
        (r#"{"a":1,"a":2}"#, "a"),
        (r#"{"x":{"k":1,"k":1}}"#, "k"),
        (r#"[{"k":[{"q":0,"q":0}]}]"#, "q"),
        (r#"{"A":1,"A":2}"#, "A"),
    ];
    for (input, key) in cases {
        assert_eq!(
            canonicalize(input.as_bytes()),
            Err(CanonicalError::DuplicateKey(key.to_owned())),
            "{input}"
        );
    }
    // Same name in sibling objects or as a value is fine.
    assert!(canonicalize(br#"{"a":{"a":"a"},"b":{"a":1},"c":["a","a"]}"#).is_ok());
    assert!(canonicalize(br#"[{"a":1},{"a":1}]"#).is_ok());
}

/// REQ-NFR-006, REQ-CORE-006: integers beyond 2^53 − 1 are rejected; the limit itself passes.
#[test]
fn req_nfr_006_unsafe_integers_rejected() {
    assert_eq!(MAX_SAFE_INTEGER, (1i64 << 53) - 1);
    for ok in [
        "9007199254740991",
        "-9007199254740991",
        "[0,-0,1]",
        "{\"a\":123}",
    ] {
        assert!(canonicalize(ok.as_bytes()).is_ok(), "{ok}");
    }
    for bad in [
        "9007199254740992",
        "-9007199254740992",
        "[1,18446744073709551615]",
        "{\"a\":{\"b\":100000000000000000000000}}",
        "-9223372036854775808",
    ] {
        assert!(
            matches!(
                canonicalize(bad.as_bytes()),
                Err(CanonicalError::UnsafeInteger(_))
            ),
            "{bad}"
        );
    }
    // Non-integer numbers are allowed by the generic function.
    assert_eq!(canon("4503599627370495.5"), "4503599627370495.5");
    assert_eq!(canon("1.5e-300"), "1.5e-300");
    assert_eq!(canon("90071992547409.91e2"), "9007199254740991");
    // Any spelling whose value is an integer beyond the safe range is rejected too: JCS would
    // print it as an integer (or with a positive exponent), so accepting it would break
    // idempotence or silently round.
    for bad in [
        "1e20",
        "1E16",
        "9007199254740992.0",
        "9007199254740993.5",
        "-1.5e300",
        "1e21",
    ] {
        assert!(
            matches!(
                canonicalize(bad.as_bytes()),
                Err(CanonicalError::UnsafeInteger(_))
            ),
            "{bad}"
        );
    }
}

/// REQ-NFR-006: syntax errors, trailing bytes, depth limit, out-of-range doubles.
#[test]
fn req_nfr_006_syntax_errors() {
    let deep = "[".repeat(200) + &"]".repeat(200);
    for bad in [
        "",
        "{",
        "{\"a\"}",
        "[1,]",
        "01",
        "1 2",
        "{} x",
        "NaN",
        "1e400",
        "\"\u{1}\"",
        &deep,
    ] {
        assert!(
            matches!(canonicalize(bad.as_bytes()), Err(CanonicalError::Syntax(_))),
            "{bad:?}"
        );
    }
    assert!(matches!(
        canonicalize(&[0xff, 0xfe]),
        Err(CanonicalError::Syntax(_))
    ));
}

/// REQ-NFR-006: canonicalisation is idempotent.
#[test]
fn req_nfr_006_idempotent() {
    for input in [
        r#"{"z":[1.5,2e3,{"b":"é","a":null}],"y":"\n"}"#,
        "\"\u{1F602}\"",
        "[-0.0, 1E-7, 1.25e2, 0.000001]",
    ] {
        let once = canonicalize(input.as_bytes()).expect("first");
        let twice = canonicalize(&once).expect("second");
        assert_eq!(once, twice, "{input}");
    }
}

/// REQ-NFR-006: `to_canonical_vec` matches `canonicalize` of the serde_json text and rejects
/// unsafe integers and unserialisable values.
#[test]
fn req_nfr_006_to_canonical_vec() {
    let mut map = BTreeMap::new();
    map.insert("b", vec![1u32, 2]);
    map.insert("a", vec![]);
    assert_eq!(
        to_canonical_vec(&map).expect("ok"),
        br#"{"a":[],"b":[1,2]}"#.to_vec()
    );

    assert!(matches!(
        to_canonical_vec(&u64::MAX),
        Err(CanonicalError::UnsafeInteger(_))
    ));
    assert!(matches!(
        to_canonical_vec(&vec![i64::MIN]),
        Err(CanonicalError::UnsafeInteger(_))
    ));
    assert!(to_canonical_vec(&MAX_SAFE_INTEGER).is_ok());
    assert!(to_canonical_vec(&-MAX_SAFE_INTEGER).is_ok());

    let mut bad_keys = BTreeMap::new();
    bad_keys.insert(vec![1u8], 1u8);
    assert!(matches!(
        to_canonical_vec(&bad_keys),
        Err(CanonicalError::Serialize(_))
    ));
}

/// REQ-NFR-006: `parse_checked` returns the value for valid input.
#[test]
fn req_nfr_006_parse_checked() {
    let v = parse_checked(br#" {"a": [1, "x"]} "#).expect("ok");
    assert_eq!(v, serde_json::json!({"a": [1, "x"]}));
    assert!(parse_checked(br#"{"a":1,"a":1}"#).is_err());
}

/// REQ-NFR-006: every error has a readable Display.
#[test]
fn req_nfr_006_error_display() {
    let errors = [
        CanonicalError::Syntax("eof".into()),
        CanonicalError::DuplicateKey("k".into()),
        CanonicalError::UnsafeInteger("9007199254740992".into()),
        CanonicalError::Serialize("map key".into()),
    ];
    for (e, needle) in errors
        .iter()
        .zip(["eof", "\"k\"", "9007199254740992", "map key"])
    {
        assert!(e.to_string().contains(needle), "{e}");
        let _: &dyn std::error::Error = e;
    }
}

/// Fills `buf` with random bytes biased towards JSON punctuation.
fn random_json_like(rng: &mut Pcg64Dxsm, buf: &mut Vec<u8>) {
    const ALPHABET: &[u8] = b"{}[]\":,\\ \n0123456789-+.eEtrufalsn\"ab\x00\xff\xc3\xa9u";
    buf.clear();
    let len = (rng.next_u64() % 64) as usize;
    for _ in 0..len {
        let r = rng.next_u64();
        let byte = if r.is_multiple_of(4) {
            (r >> 8) as u8
        } else {
            ALPHABET[((r >> 8) % ALPHABET.len() as u64) as usize]
        };
        buf.push(byte);
    }
}

/// REQ-NFR-006, R12: bounded random bytes never panic, and accepted input is idempotent.
#[test]
fn req_nfr_006_random_bytes_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x00f1_e0b1_7c0a_0001);
    let mut buf = Vec::new();
    let mut accepted = 0u32;
    for _ in 0..4000 {
        random_json_like(&mut rng, &mut buf);
        if let Ok(once) = canonicalize(&buf) {
            accepted += 1;
            assert_eq!(canonicalize(&once).as_ref(), Ok(&once));
        }
    }
    assert!(accepted > 0, "the generator should produce some valid JSON");
}

// ---- Review round 1: checked serializer, depth budget, noncharacters ----

/// `[[...[1]...]]` with `depth` nested arrays, as text.
fn nested_text(depth: usize) -> String {
    "[".repeat(depth) + "1" + &"]".repeat(depth)
}

/// `depth` nested arrays around `1`, built iteratively.
fn nested_value(depth: usize) -> Value {
    let mut v = Value::from(1);
    for _ in 0..depth {
        v = Value::Array(vec![v]);
    }
    v
}

/// REQ-NFR-006, REQ-DSL-001: MAX_DEPTH is exactly the parser's limit, and export applies the
/// same budget, so nothing can be exported that cannot be loaded.
#[test]
fn req_nfr_006_max_depth_matches_parser_and_export() {
    let at_limit = nested_text(MAX_DEPTH);
    let canonical = canonicalize(at_limit.as_bytes()).expect("MAX_DEPTH is accepted");
    assert_eq!(canonical, at_limit.as_bytes());
    assert!(parse_checked(at_limit.as_bytes()).is_ok());
    assert!(matches!(
        canonicalize(nested_text(MAX_DEPTH + 1).as_bytes()),
        Err(CanonicalError::Syntax(_))
    ));
    // The same budget on export.
    assert_eq!(
        to_canonical_vec(&nested_value(MAX_DEPTH)).expect("export"),
        canonical
    );
    let too_deep = Err(CanonicalError::Serialize(format!(
        "nesting depth exceeds {MAX_DEPTH}"
    )));
    assert_eq!(to_canonical_vec(&nested_value(MAX_DEPTH + 1)), too_deep);
    // The case from the review: 130 nested arrays.
    assert_eq!(to_canonical_vec(&nested_value(130)), too_deep);
    // An object level counts like an array level.
    let mut obj = nested_value(MAX_DEPTH - 1);
    obj = serde_json::json!({ "k": obj });
    assert!(to_canonical_vec(&obj).is_ok());
    assert_eq!(to_canonical_vec(&vec![obj]), too_deep);
    // Bytes are an array and count as one level.
    assert_eq!(to_canonical_vec(&Bytes(b"\x01")).expect("bytes"), b"[1]");
}

/// Emits `self.0` nested sequences recursively, without materialising a Value.
struct Deep(usize);

impl Serialize for Deep {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(1))?;
        if self.0 > 1 {
            seq.serialize_element(&Deep(self.0 - 1))?;
        }
        seq.end()
    }
}

/// REQ-NFR-006, R12: a pathologically deep value is rejected before the serializer recurses
/// past MAX_DEPTH, so it cannot overflow the stack.
#[test]
fn req_nfr_006_deep_value_rejected_without_overflow() {
    let too_deep = Err(CanonicalError::Serialize(format!(
        "nesting depth exceeds {MAX_DEPTH}"
    )));
    assert_eq!(to_canonical_vec(&Deep(100_000)), too_deep);
    assert_eq!(to_canonical_vec(&Deep(MAX_DEPTH + 1)), too_deep);
    assert!(to_canonical_vec(&Deep(MAX_DEPTH)).is_ok());
}

/// A struct with a float field (test-only: REQ-CORE-006 forbids floats in rules code; these
/// values exist only to prove the serializer rejects non-finite ones).
#[allow(clippy::disallowed_types)]
#[derive(Serialize)]
struct FloatHolder {
    x: f64,
    y: f32,
}

/// REQ-NFR-006, REQ-CORE-006: NaN and infinities are rejected at any depth instead of turning
/// into `null`; explicit null and finite floats are still accepted.
#[test]
#[allow(clippy::disallowed_types)]
fn req_nfr_006_non_finite_floats_rejected() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let is_serialize = |r: Result<Vec<u8>, CanonicalError>| matches!(r, Err(CanonicalError::Serialize(m)) if m.contains("non-finite"));
        assert!(is_serialize(to_canonical_vec(&bad)), "{bad}");
        assert!(is_serialize(to_canonical_vec(&vec![1.0, bad])), "{bad}");
        assert!(
            is_serialize(to_canonical_vec(&FloatHolder { x: bad, y: 0.0 })),
            "{bad}"
        );
        #[allow(clippy::cast_possible_truncation)]
        let narrow = bad as f32;
        assert!(
            is_serialize(to_canonical_vec(&FloatHolder { x: 0.0, y: narrow })),
            "{bad}"
        );
    }
    assert_eq!(to_canonical_vec(&None::<i64>).expect("null"), b"null");
    assert_eq!(to_canonical_vec(&()).expect("unit"), b"null");
    assert_eq!(to_canonical_vec(&1.5).expect("1.5"), b"1.5");
    assert_eq!(
        to_canonical_vec(&FloatHolder { x: -0.0, y: 2.5 }).expect("finite"),
        br#"{"x":0,"y":2.5}"#
    );
}

/// Emits the member `k` twice, either as a map or as a struct.
struct TwiceK {
    as_struct: bool,
}

impl Serialize for TwiceK {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeStruct};
        if self.as_struct {
            let mut s = serializer.serialize_struct("TwiceK", 2)?;
            s.serialize_field("k", &1)?;
            s.serialize_field("k", &2)?;
            s.end()
        } else {
            let mut m = serializer.serialize_map(Some(2))?;
            m.serialize_entry("k", &1)?;
            m.serialize_entry("k", &2)?;
            m.end()
        }
    }
}

/// REQ-NFR-006: a Serialize impl emitting the same member name twice is rejected instead of
/// keeping the last value; ordinary unique-key maps still serialise.
#[test]
fn req_nfr_006_serializer_duplicate_members_rejected() {
    for as_struct in [false, true] {
        assert_eq!(
            to_canonical_vec(&TwiceK { as_struct }),
            Err(CanonicalError::DuplicateKey("k".into()))
        );
        assert_eq!(
            to_checked_value(&vec![TwiceK { as_struct }]),
            Err(CanonicalError::DuplicateKey("k".into()))
        );
    }
    let mut map = BTreeMap::new();
    map.insert("k".to_owned(), 1);
    map.insert("j".to_owned(), 2);
    assert_eq!(to_canonical_vec(&map).expect("ok"), br#"{"j":2,"k":1}"#);
}

/// Serialises as raw bytes.
struct Bytes(&'static [u8]);

impl Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

/// Every serde data-model shape, for the differential test.
#[derive(Serialize)]
enum Shape {
    Unit,
    Newtype(i32),
    Tuple(u8, String),
    Struct { a: Option<bool>, b: Vec<i64> },
}

#[derive(Serialize)]
struct UnitStruct;

#[derive(Serialize)]
struct NewtypeStruct(String);

#[derive(Serialize)]
struct TupleStruct(i8, i16, u16);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    first_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<u32>,
    present: Option<u32>,
    shapes: Vec<Shape>,
    nested: Value,
    #[serde(flatten)]
    extra: BTreeMap<String, u64>,
}

/// Asserts the checked serializer gives exactly what `serde_json::to_value` gives.
fn same_as_serde_json<T: Serialize + ?Sized>(value: &T) {
    assert_eq!(
        to_checked_value(value).expect("checked"),
        serde_json::to_value(value).expect("serde_json")
    );
}

/// REQ-NFR-006: differential test: for every input the old path accepted, the checked
/// serializer produces the identical Value (so canonical bytes and hashes are unchanged).
#[test]
fn req_nfr_006_checked_serializer_matches_serde_json() {
    let mut extra = BTreeMap::new();
    extra.insert("zz".to_owned(), 7u64);
    let record = Record {
        first_name: "é \u{1F602} \"q\"\n".into(),
        skipped: None,
        present: None,
        shapes: vec![
            Shape::Unit,
            Shape::Newtype(-3),
            Shape::Tuple(9, "t".into()),
            Shape::Struct {
                a: Some(true),
                b: vec![-MAX_SAFE_INTEGER, MAX_SAFE_INTEGER],
            },
        ],
        nested: serde_json::json!({"b": [1, -2, 1.5, null, {"c": "d"}], "a": false}),
        extra,
    };
    same_as_serde_json(&record);
    same_as_serde_json(&UnitStruct);
    same_as_serde_json(&NewtypeStruct("n".into()));
    same_as_serde_json(&TupleStruct(-1, -2, 3));
    same_as_serde_json(&(1u8, 'c', "s", true, ()));
    same_as_serde_json(&Bytes(b"\x00\xff"));
    same_as_serde_json(&[1u32, 2, 3]);
    same_as_serde_json(&Some(4i128));
    same_as_serde_json(&5u128);
    same_as_serde_json(&-6i128);
    same_as_serde_json(&1.5f32);
    same_as_serde_json(&0.1);
    same_as_serde_json(&-0.0);
    let mut int_keys = BTreeMap::new();
    int_keys.insert(-1i32, "a");
    int_keys.insert(10, "b");
    same_as_serde_json(&int_keys);
    let mut other_keys = BTreeMap::new();
    other_keys.insert('x', 1u8);
    same_as_serde_json(&other_keys);
    let mut bool_keys = BTreeMap::new();
    bool_keys.insert(true, 1u8);
    same_as_serde_json(&bool_keys);
    let mut big_keys = BTreeMap::new();
    big_keys.insert(u64::MAX, 1u8);
    same_as_serde_json(&big_keys);
    let mut variant_keys = BTreeMap::new();
    variant_keys.insert(NewtypeStruct("k".into()), 1u8);
    same_as_serde_json(&variant_keys);
    // Unsupported keys stay an error.
    let mut seq_keys = BTreeMap::new();
    seq_keys.insert((1u8, 2u8), 1u8);
    assert!(matches!(
        to_checked_value(&seq_keys),
        Err(CanonicalError::Serialize(_))
    ));
    // Unsafe integers of every width.
    for result in [
        to_checked_value(&(i128::from(MAX_SAFE_INTEGER) + 1)),
        to_checked_value(&-(i128::from(MAX_SAFE_INTEGER) + 1)),
        to_checked_value(&u128::MAX),
        to_checked_value(&(MAX_SAFE_INTEGER as u64 + 1)),
        to_checked_value(&i64::MIN),
    ] {
        assert!(
            matches!(result, Err(CanonicalError::UnsafeInteger(_))),
            "{result:?}"
        );
    }
}

impl Ord for NewtypeStruct {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for NewtypeStruct {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for NewtypeStruct {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for NewtypeStruct {}

/// REQ-NFR-006 (RFC 7493 §2.1): noncharacters in decoded strings and member names are rejected
/// on input (Syntax) and on export (Serialize), escaped or literal; ordinary Unicode passes.
#[test]
fn req_nfr_006_noncharacters_rejected() {
    let rejected = [
        (r#"{"note":"\ufdd0"}"#, "U+FDD0"),
        ("[\"a\u{FFFF}\"]", "U+FFFF"),
        (r#""\ud83f\udffe""#, "U+1FFFE"),
        (r#"{"\uffff":1}"#, "U+FFFF"),
        ("{\"k\u{FFFE}\":1}", "U+FFFE"),
        (r#""\ufdef""#, "U+FDEF"),
        ("\"\u{10FFFF}\"", "U+10FFFF"),
    ];
    for (input, code) in rejected {
        match canonicalize(input.as_bytes()) {
            Err(CanonicalError::Syntax(m)) => assert!(m.contains(code), "{m}"),
            other => panic!("{input}: {other:?}"),
        }
        assert!(parse_checked(input.as_bytes()).is_err(), "{input}");
        // The same content as a Rust value is refused on export.
        let value: Value = serde_json::from_str(input).expect("plain serde_json accepts it");
        match to_canonical_vec(&value) {
            Err(CanonicalError::Serialize(m)) => assert!(m.contains(code), "{m}"),
            other => panic!("{input}: {other:?}"),
        }
    }
    for ok in [
        r#""\ufdcf""#,
        r#""\ufdf0""#,
        r#""\ufffd""#,
        "\"\u{e9}\"",
        r#""\u00e9""#,
        "\"\u{1F602}\"",
        r#"{"\ud83d\ude02":"\uFDCF"}"#,
    ] {
        let canonical = canonicalize(ok.as_bytes()).expect(ok);
        let value: Value = serde_json::from_str(ok).expect("json");
        assert_eq!(to_canonical_vec(&value).as_ref(), Ok(&canonical), "{ok}");
    }
    // Typed values: a String, a char, a struct field and a map key.
    assert!(matches!(
        to_canonical_vec(&"x\u{FFFE}".to_owned()),
        Err(CanonicalError::Serialize(_))
    ));
    assert!(matches!(
        to_canonical_vec(&'\u{FDD0}'),
        Err(CanonicalError::Serialize(_))
    ));
    assert!(matches!(
        to_canonical_vec(&NewtypeStruct("\u{FFFE}".into())),
        Err(CanonicalError::Serialize(_))
    ));
    assert!(matches!(
        to_canonical_vec(&Shape::Tuple(1, "\u{FFFE}".into())),
        Err(CanonicalError::Serialize(_))
    ));
    let mut key = BTreeMap::new();
    key.insert("\u{FFFE}".to_owned(), 1);
    assert!(matches!(
        to_canonical_vec(&key),
        Err(CanonicalError::Serialize(m)) if m.contains("member name")
    ));
}

/// REQ-NFR-006: `parse_normalized` returns numbers in canonical form.
#[test]
fn req_nfr_006_parse_normalized() {
    assert_eq!(
        parse_normalized(b"[1.0, -0.0, 1e2, 1.5]").expect("ok"),
        serde_json::json!([1, 0, 100, 1.5])
    );
    assert!(parse_normalized(b"{\"a\":1,\"a\":2}").is_err());
}

/// REQ-NFR-006, R12: bounded random documents built from deep nesting, noncharacter escapes and
/// duplicate names never panic; whatever `canonicalize` accepts, export of the parsed value
/// accepts with the same bytes, and whatever it rejects, export rejects too.
#[test]
fn req_nfr_006_random_review_inputs_consistent() {
    const PIECES: &[&str] = &[
        "[",
        "]",
        "{",
        "}",
        "\"k\":",
        "\"\\ufdd0\"",
        "\"\\uffff\"",
        "\"\\ud83f\\udffe\"",
        "\"a\"",
        ",",
        "1",
        "1.0",
        "-0.0",
        "null",
        "\"\\ufdcf\"",
        "\"k\":1,\"k\":2",
    ];
    let mut rng = Pcg64Dxsm::from_u64_seed(0x00f1_e0b1_7c0a_0002);
    let mut accepted = 0u32;
    for i in 0..3000 {
        let mut text = String::new();
        if i % 7 == 0 {
            let depth = MAX_DEPTH - 2 + (rng.next_u64() % 5) as usize;
            text = nested_text(depth);
        } else {
            for _ in 0..(rng.next_u64() % 24) {
                text.push_str(PIECES[(rng.next_u64() % PIECES.len() as u64) as usize]);
            }
        }
        let result = canonicalize(text.as_bytes());
        // Plain serde_json keeps the last duplicate member, so only other outcomes compare.
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            let exported = to_canonical_vec(&value);
            match &result {
                Ok(bytes) => {
                    accepted += 1;
                    assert_eq!(exported.as_ref(), Ok(bytes), "{text}");
                }
                Err(CanonicalError::DuplicateKey(_)) => {}
                Err(_) => assert!(exported.is_err(), "{text}: {exported:?}"),
            }
        }
    }
    assert!(accepted > 0);
}

/// A struct with a member `h` and an optional nested struct of the same shape.
#[derive(Serialize)]
struct WithH {
    h: String,
    v: i64,
    inner: Option<Box<WithH>>,
}

/// Emits the member `h` twice (each with a noncharacter) and then `v`, as a map or a struct.
struct TwiceH {
    as_struct: bool,
}

impl Serialize for TwiceH {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeStruct};
        if self.as_struct {
            let mut s = serializer.serialize_struct("TwiceH", 3)?;
            s.serialize_field("h", "\u{FDD0}")?;
            s.serialize_field("h", "\u{FFFF}")?;
            s.serialize_field("v", &1)?;
            s.end()
        } else {
            let mut m = serializer.serialize_map(Some(3))?;
            m.serialize_entry("h", "\u{FDD0}")?;
            m.serialize_entry("h", "\u{FFFF}")?;
            m.serialize_entry("v", &1)?;
            m.end()
        }
    }
}

/// REQ-NFR-006, REQ-DSL-002, REQ-CORE-006: the skipped member is left out of the outermost
/// struct or map only, before its value is checked, and counts as absent for duplicate
/// detection; a nested member of the same name is serialised and checked as usual.
#[test]
fn req_nfr_006_checked_value_without_member_top_level_only() {
    let outer = |h: &str, inner: Option<WithH>| WithH {
        h: h.to_owned(),
        v: 1,
        inner: inner.map(Box::new),
    };
    let leaf = |h: &str| WithH {
        h: h.to_owned(),
        v: 2,
        inner: None,
    };
    // Top level: the stale value is never inspected, whatever it holds.
    for stale in ["", "\u{FDD0}", "\u{FFFF}", "not hex"] {
        assert_eq!(
            to_checked_value_without_member(&outer(stale, None), "h"),
            Ok(serde_json::json!({"v": 1, "inner": null})),
            "{stale:?}"
        );
    }
    // Nested: the member is kept and checked.
    assert_eq!(
        to_checked_value_without_member(&outer("\u{FDD0}", Some(leaf("x"))), "h"),
        Ok(serde_json::json!({"v": 1, "inner": {"h": "x", "v": 2, "inner": null}}))
    );
    assert!(matches!(
        to_checked_value_without_member(&outer("", Some(leaf("\u{FDD0}"))), "h"),
        Err(CanonicalError::Serialize(_))
    ));
    // Maps behave like structs, and nested maps keep the member.
    let mut map = BTreeMap::new();
    map.insert("h".to_owned(), BTreeMap::from([("h".to_owned(), 3)]));
    map.insert("w".to_owned(), BTreeMap::from([("h".to_owned(), 4)]));
    assert_eq!(
        to_checked_value_without_member(&map, "h"),
        Ok(serde_json::json!({"w": {"h": 4}}))
    );
    // A repeated skipped member is absent, not a duplicate; values are never checked.
    for as_struct in [false, true] {
        assert_eq!(
            to_checked_value_without_member(&TwiceH { as_struct }, "h"),
            Ok(serde_json::json!({"v": 1})),
        );
        assert_eq!(
            to_checked_value(&TwiceH { as_struct }),
            Err(CanonicalError::Serialize(
                "string contains the noncharacter U+FDD0".into()
            ))
        );
    }
    // Inside a sequence the struct is nested, so nothing is skipped.
    assert!(matches!(
        to_checked_value_without_member(&vec![outer("\u{FDD0}", None)], "h"),
        Err(CanonicalError::Serialize(_))
    ));
    // Without the member, the result equals the plain checked value with the member removed.
    let plain = outer("abc", Some(leaf("def")));
    let mut expected = to_checked_value(&plain).expect("plain");
    expected.as_object_mut().expect("object").remove("h");
    assert_eq!(to_checked_value_without_member(&plain, "h"), Ok(expected));
}
