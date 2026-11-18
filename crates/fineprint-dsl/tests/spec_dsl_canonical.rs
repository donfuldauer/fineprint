// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::canonical` (RFC 8785 / JCS canonical JSON).
//! Covers REQ-DSL-001 and REQ-NFR-006 (canonical JSON as defined by RFC 8785), plus
//! REQ-CORE-006 (no floats in the test code; integer-only random documents).
//! Black-box: written from the work-item brief only. Expected bytes are hand-derived (ECMAScript
//! number forms were checked independently with `node -e 'JSON.stringify(JSON.parse(s))'`), and
//! random documents are checked against an independent reference canonicaliser below.

use std::collections::BTreeMap;

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::canonical::{CanonicalError, MAX_SAFE_INTEGER, canonicalize, to_canonical_vec};
use serde_json::{Value, json};

// ---------------------------------------------------------------- helpers

fn canon_ok(input: &str) -> String {
    match canonicalize(input.as_bytes()) {
        Ok(bytes) => String::from_utf8(bytes).expect("canonical output is UTF-8"),
        Err(e) => panic!("canonicalize({input:?}) failed: {e:?}"),
    }
}

fn assert_canon(input: &str, expected: &str) {
    let got = canon_ok(input);
    assert_eq!(got, expected, "canonicalize({input:?})");
    // Idempotence on every vector.
    assert_eq!(canon_ok(&got), got, "idempotence for {input:?}");
}

fn kind(e: &CanonicalError) -> &'static str {
    match e {
        CanonicalError::Syntax(_) => "Syntax",
        CanonicalError::DuplicateKey(_) => "DuplicateKey",
        CanonicalError::UnsafeInteger(_) => "UnsafeInteger",
        CanonicalError::Serialize(_) => "Serialize",
    }
}

fn is_syntax(r: &Result<Vec<u8>, CanonicalError>) -> bool {
    matches!(r, Err(CanonicalError::Syntax(_)))
}

fn is_unsafe(r: &Result<Vec<u8>, CanonicalError>) -> bool {
    matches!(r, Err(CanonicalError::UnsafeInteger(_)))
}

/// Independent reference canonicaliser for documents with no non-integer numbers:
/// members sorted by UTF-16 code units, minimal string escapes, no whitespace.
fn oracle(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else {
                panic!("oracle only handles integers");
            }
        }
        Value::String(s) => oracle_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                oracle(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                oracle_string(k, out);
                out.push(':');
                oracle(&map[k.as_str()], out);
            }
            out.push('}');
        }
    }
}

fn oracle_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn oracle_bytes(v: &Value) -> Vec<u8> {
    let mut s = String::new();
    oracle(v, &mut s);
    s.into_bytes()
}

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_u64() % n
}

const STRING_POOL: &[char] = &[
    'a',
    'b',
    'A',
    'Z',
    '0',
    '9',
    ' ',
    '"',
    '\\',
    '/',
    '\u{0}',
    '\u{1}',
    '\u{8}',
    '\t',
    '\n',
    '\u{b}',
    '\u{c}',
    '\r',
    '\u{1f}',
    '\u{7f}',
    '\u{80}',
    'é',
    'あ',
    '\u{FB33}',
    '\u{1F602}',
    '\u{FFFD}',
    '\u{10000}',
];

fn random_string(rng: &mut Pcg64Dxsm, max_len: u64) -> String {
    let n = below(rng, max_len + 1);
    (0..n)
        .map(|_| STRING_POOL[below(rng, STRING_POOL.len() as u64) as usize])
        .collect()
}

fn random_int(rng: &mut Pcg64Dxsm) -> i64 {
    match below(rng, 6) {
        0 => MAX_SAFE_INTEGER,
        1 => -MAX_SAFE_INTEGER,
        2 => 0,
        3 => below(rng, 100) as i64 - 50,
        _ => (below(rng, 2 * MAX_SAFE_INTEGER as u64 + 1) as i64) - MAX_SAFE_INTEGER,
    }
}

fn random_value(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    let kinds = if depth == 0 { 4 } else { 6 };
    match below(rng, kinds) {
        0 => Value::Null,
        1 => Value::Bool(below(rng, 2) == 0),
        2 => json!(random_int(rng)),
        3 => Value::String(random_string(rng, 8)),
        4 => {
            let n = below(rng, 5);
            Value::Array((0..n).map(|_| random_value(rng, depth - 1)).collect())
        }
        _ => {
            let n = below(rng, 6);
            let mut map = serde_json::Map::new();
            for _ in 0..n {
                let k = random_string(rng, 4);
                map.insert(k, random_value(rng, depth - 1));
            }
            Value::Object(map)
        }
    }
}

// ---------------------------------------------------------------- key ordering, structure

/// REQ-DSL-001 / REQ-NFR-006: object members are sorted by UTF-16 code units.
#[test]
fn req_nfr_006_keys_sorted_by_utf16_code_units() {
    assert_canon(
        r#"{"a":1,"B":2,"aa":3,"10":4,"9":5,"":6}"#,
        r#"{"":6,"10":4,"9":5,"B":2,"a":1,"aa":3}"#,
    );
    // U+1F602 is the surrogate pair D83D DE02, which sorts BEFORE U+FB33 in UTF-16 order
    // (the reverse of UTF-8 / code-point order).
    let input = "{\"\u{FB33}\":1,\"\u{1F602}\":2}";
    let expected = "{\"\u{1F602}\":2,\"\u{FB33}\":1}";
    assert_canon(input, expected);
    // Same input written with escapes.
    assert_canon(r#"{"\ufb33":1,"\ud83d\ude02":2}"#, expected);
    // A key that is a prefix sorts first; case sorts by code unit.
    assert_canon(
        r#"{"ab":1,"a":2,"b":3,"A":4}"#,
        r#"{"A":4,"a":2,"ab":1,"b":3}"#,
    );
}

/// REQ-NFR-006: nested objects are sorted recursively, arrays keep order, whitespace stripped.
#[test]
fn req_nfr_006_nested_objects_arrays_and_whitespace() {
    assert_canon(
        " { \"z\" : [ 3 , 1 , 2 , { \"y\" : true , \"x\" : null } ] ,\n\t\"m\" : { \"d\" : \"v\" , \"c\" : [ ] , \"b\" : { } } } \r\n",
        r#"{"m":{"b":{},"c":[],"d":"v"},"z":[3,1,2,{"x":null,"y":true}]}"#,
    );
    assert_canon("[ ]", "[]");
    assert_canon("{ }", "{}");
    assert_canon(" true ", "true");
    assert_canon("false", "false");
    assert_canon("null", "null");
    assert_canon("\"s\"", "\"s\"");
    assert_canon("[[[]],[{}],\"\"]", "[[[]],[{}],\"\"]");
}

// ---------------------------------------------------------------- numbers

/// REQ-NFR-006: numbers are serialised in ECMAScript form (vectors checked with node).
#[test]
fn req_nfr_006_ecmascript_number_forms() {
    let vectors: &[(&str, &str)] = &[
        ("0", "0"),
        ("-0", "0"),
        ("-0.0", "0"),
        ("1.0", "1"),
        ("1e2", "100"),
        ("1E2", "100"),
        ("0.000001", "0.000001"),
        ("1e-6", "0.000001"),
        ("1e-7", "1e-7"),
        ("-1e-7", "-1e-7"),
        ("-1.5", "-1.5"),
        ("0.1", "0.1"),
        ("4.35", "4.35"),
        ("5e-324", "5e-324"),
        ("2.5E-3", "0.0025"),
        ("100.50", "100.5"),
        ("123e-2", "1.23"),
        ("333333333.33333329", "333333333.3333333"),
        ("42", "42"),
        ("-42", "-42"),
        ("-0.5", "-0.5"),
        ("1.5e-300", "1.5e-300"),
        ("1e15", "1000000000000000"),
        ("4503599627370495.5", "4503599627370495.5"),
        // Integral values at the safe limit are accepted whatever their spelling.
        ("9007199254740991.0", "9007199254740991"),
        ("-9007199254740991e0", "-9007199254740991"),
        ("9.007199254740991e15", "9007199254740991"),
    ];
    for (input, expected) in vectors {
        assert_canon(input, expected);
        assert_canon(&format!("[{input}]"), &format!("[{expected}]"));
        assert_canon(
            &format!("{{\"n\":{input}}}"),
            &format!("{{\"n\":{expected}}}"),
        );
    }
}

/// REQ-NFR-006 / REQ-CORE-006: a number whose VALUE is integral with magnitude above 2^53-1 is
/// UnsafeInteger whatever its spelling (exponent, fraction digits), at every position
/// (docs/DECISIONS.md "#11 FP-010 — Safe integer range for hashed JSON").
#[test]
fn req_nfr_006_unsafe_integer_values_in_any_spelling() {
    let unsafe_values = [
        "9007199254740992.0",
        "9007199254740992e0",
        "9.007199254740992e15",
        "1e16",
        "1e20",
        "1e21",
        "1E21",
        "1.2345678901234568e20",
        "123456789012345680000.0",
        "1.5e300",
        "1.7976931348623157e308",
        "9007199254740993.5",
    ];
    for v in unsafe_values {
        for sign in ["", "-"] {
            let n = format!("{sign}{v}");
            let docs = [
                n.clone(),
                format!("[1.5,{n}]"),
                format!("{{\"k\":{n}}}"),
                format!("{{\"a\":[{{\"b\":{n}}}]}}"),
            ];
            for d in &docs {
                let r = canonicalize(d.as_bytes());
                assert!(is_unsafe(&r), "expected UnsafeInteger for {d}: {r:?}");
            }
        }
    }
}

/// REQ-NFR-006 / REQ-CORE-006: integers are limited to +-(2^53-1) at every position.
#[test]
fn req_nfr_006_unsafe_integers_rejected_everywhere() {
    assert_eq!(MAX_SAFE_INTEGER, 9_007_199_254_740_991);
    for ok in ["9007199254740991", "-9007199254740991"] {
        assert_canon(ok, ok);
        assert_canon(&format!("[1,{ok}]"), &format!("[1,{ok}]"));
        assert_canon(
            &format!("{{\"a\":{{\"b\":[{ok}]}}}}"),
            &format!("{{\"a\":{{\"b\":[{ok}]}}}}"),
        );
    }
    for bad in [
        "9007199254740992",
        "-9007199254740992",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        "123456789012345678901234567890",
    ] {
        let docs = [
            bad.to_string(),
            format!("[0,{bad}]"),
            format!("{{\"k\":{bad}}}"),
            format!("{{\"a\":{{\"b\":[1,2,{{\"c\":{bad}}}]}}}}"),
            format!("[[[[{bad}]]]]"),
        ];
        for d in &docs {
            let r = canonicalize(d.as_bytes());
            assert!(is_unsafe(&r), "expected UnsafeInteger for {d}: {r:?}");
        }
    }
}

// ---------------------------------------------------------------- strings

/// REQ-NFR-006: strings are minimally escaped; `/`, DEL and non-ASCII are raw.
#[test]
fn req_nfr_006_string_escapes() {
    let vectors: &[(&str, &str)] = &[
        (r#""\u0041""#, r#""A""#),
        (r#""\/""#, r#""/""#),
        (r#""/""#, r#""/""#),
        ("\"\\u007f\"", "\"\u{7f}\""),
        ("\"\u{7f}\"", "\"\u{7f}\""),
        (r#""\u00e9""#, "\"é\""),
        (r#""\u3042""#, "\"あ\""),
        (r#""\ud83d\ude02""#, "\"\u{1F602}\""),
        (r#""\u0000""#, r#""\u0000""#),
        (r#""\u0001""#, r#""\u0001""#),
        (r#""\u000b""#, r#""\u000b""#),
        (r#""\u000B""#, r#""\u000b""#),
        (r#""\u001F""#, r#""\u001f""#),
        (r#""\u0008""#, r#""\b""#),
        (r#""\u0009""#, r#""\t""#),
        (r#""\u000a""#, r#""\n""#),
        (r#""\u000c""#, r#""\f""#),
        (r#""\u000d""#, r#""\r""#),
        (r#""\b\t\n\f\r""#, r#""\b\t\n\f\r""#),
        (r#""\"""#, r#""\"""#),
        (r#""\u0022""#, r#""\"""#),
        (r#""\\""#, r#""\\""#),
        (r#""\u005c""#, r#""\\""#),
        (r#""\u0020""#, r#"" ""#),
        ("\"\u{2028}\u{2029}\"", "\"\u{2028}\u{2029}\""),
        (r#""\u2028""#, "\"\u{2028}\""),
        (r#""\ufeff""#, "\"\u{feff}\""),
    ];
    for (input, expected) in vectors {
        assert_canon(input, expected);
    }
}

/// REQ-NFR-006: lone surrogates are not valid Unicode text and are rejected.
#[test]
fn req_nfr_006_lone_surrogate_rejected() {
    for d in [
        r#""\ud800""#,
        r#""\udc00""#,
        r#""a\ud800b""#,
        r#"{"\ud800":1}"#,
        r#"["\ude02\ud83d"]"#,
    ] {
        assert!(canonicalize(d.as_bytes()).is_err(), "{d}");
    }
    // Invalid UTF-8 is not JSON text either.
    assert!(canonicalize(b"\"\xff\"").is_err());
    assert!(canonicalize(b"\"\xed\xa0\x80\"").is_err());
}

// ---------------------------------------------------------------- rejections

/// REQ-DSL-001 / REQ-NFR-006: duplicate keys (compared after decoding) are rejected at any depth.
#[test]
fn req_dsl_001_duplicate_keys_rejected() {
    let cases: &[(&str, &str)] = &[
        (r#"{"a":1,"a":2}"#, "a"),
        (r#"{"a":1,"a":1}"#, "a"),
        (r#"{"a":1,"\u0061":2}"#, "a"),
        (r#"{"\u0061":1,"b":0,"a":2}"#, "a"),
        (r#"{"x":{"k":1,"k":2}}"#, "k"),
        (r#"[0,{"k":1,"z":[{"q":1,"q":null}]}]"#, "q"),
        (r#"{"\u00e9":1,"é":2}"#, "é"),
        (r#"{"\ud83d\ude02":1,"😂":2}"#, "😂"),
        (r#"{"":1,"":2}"#, ""),
    ];
    for (d, key) in cases {
        match canonicalize(d.as_bytes()) {
            Err(CanonicalError::DuplicateKey(k)) => assert_eq!(&k, key, "{d}"),
            other => panic!("expected DuplicateKey for {d}, got {other:?}"),
        }
    }
    // Same key in sibling objects is not a duplicate.
    assert_canon(r#"[{"a":1},{"a":2}]"#, r#"[{"a":1},{"a":2}]"#);
    assert_canon(r#"{"a":{"a":1}}"#, r#"{"a":{"a":1}}"#);
}

/// REQ-NFR-006: invalid JSON, trailing bytes, empty and truncated input are Syntax errors.
#[test]
fn req_nfr_006_syntax_errors() {
    let bad: &[&str] = &[
        "",
        " ",
        "{",
        "[",
        "[1,",
        "{\"a\":",
        "{\"a\":}",
        "{\"a\" 1}",
        "{a:1}",
        "{'a':1}",
        "[1,]",
        "{\"a\":1,}",
        "\"abc",
        "nul",
        "tru",
        "01",
        "+1",
        ".5",
        "1.",
        "1e",
        "--1",
        "NaN",
        "Infinity",
        "{\"a\":1} x",
        "{\"a\":1}{}",
        "1 2",
        "[] []",
        "\"a\"\"b\"",
        "\"\\x\"",
        "\"\\u12\"",
        "\"tab\tinside\"",
        "\"nl\ninside\"",
        "/* c */ 1",
    ];
    for d in bad {
        let r = canonicalize(d.as_bytes());
        assert!(is_syntax(&r), "expected Syntax for {d:?}: {r:?}");
    }
    // Every strict prefix of a valid document is truncated, hence rejected.
    let doc = r#"{"a":[1,{"b":"c"},true,null],"d":-12}"#;
    for cut in 0..doc.len() {
        let r = canonicalize(&doc.as_bytes()[..cut]);
        assert!(is_syntax(&r), "prefix {cut}: {r:?}");
    }
}

/// REQ-NFR-006: deep nesting returns an error instead of overflowing the stack.
#[test]
fn req_nfr_006_deep_nesting_is_error_not_overflow() {
    let n = 10_000;
    let arrays = format!("{}{}", "[".repeat(n), "]".repeat(n));
    let r = canonicalize(arrays.as_bytes());
    assert!(is_syntax(&r), "{:?}", r.as_ref().map(|v| v.len()));
    let mut objects = String::new();
    for _ in 0..n {
        objects.push_str("{\"a\":");
    }
    objects.push('1');
    objects.push_str(&"}".repeat(n));
    let r = canonicalize(objects.as_bytes());
    assert!(is_syntax(&r), "{:?}", r.as_ref().map(|v| v.len()));
    // Unterminated deep nesting too.
    assert!(canonicalize("[".repeat(n).as_bytes()).is_err());
}

/// REQ-NFR-006: `CanonicalError` implements Display and std::error::Error with useful text.
#[test]
fn req_nfr_006_error_display() {
    let errs = [
        canonicalize(b"{").unwrap_err(),
        canonicalize(br#"{"a":1,"a":2}"#).unwrap_err(),
        canonicalize(b"9007199254740992").unwrap_err(),
    ];
    for e in &errs {
        let s = e.to_string();
        assert!(!s.is_empty());
        let _: &dyn std::error::Error = e;
        assert_eq!(e.clone(), *e);
    }
    let s = CanonicalError::Serialize("x".into()).to_string();
    assert!(!s.is_empty());
}

// ---------------------------------------------------------------- to_canonical_vec

/// REQ-DSL-001 / REQ-NFR-006: `to_canonical_vec` equals canonicalize(serde_json::to_vec(value)).
#[test]
fn req_dsl_001_to_canonical_vec_matches_canonicalize() {
    let values = [
        json!(null),
        json!({"b": [1, 2, {"z": "é", "a": "\u{1F602}"}], "a": true, "": -5}),
        json!(["x", "\u{0}\u{1f}\u{7f}/", [], {}]),
        json!(MAX_SAFE_INTEGER),
        json!(9_007_199_254_740_992_i64),
        json!(u64::MAX),
        json!({"n": {"m": -9_007_199_254_740_992_i64}}),
    ];
    for v in &values {
        let a = to_canonical_vec(v);
        let b = canonicalize(&serde_json::to_vec(v).unwrap());
        match (&a, &b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y, "{v}"),
            // Error texts may legitimately differ between the two paths; the kind may not.
            (Err(x), Err(y)) => assert_eq!(kind(x), kind(y), "{v}"),
            _ => panic!("to_canonical_vec and canonicalize disagree for {v}: {a:?} vs {b:?}"),
        }
    }
    assert_eq!(
        to_canonical_vec(&values[1]).unwrap(),
        "{\"\":-5,\"a\":true,\"b\":[1,2,{\"a\":\"\u{1F602}\",\"z\":\"é\"}]}".as_bytes()
    );
    assert!(matches!(
        to_canonical_vec(&values[4]),
        Err(CanonicalError::UnsafeInteger(_))
    ));

    let mut m: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    m.insert("zeta".into(), vec![3, -1]);
    m.insert("Alpha".into(), vec![]);
    m.insert("\u{FB33}".into(), vec![1]);
    m.insert("\u{1F602}".into(), vec![2]);
    let got = to_canonical_vec(&m).unwrap();
    assert_eq!(got, canonicalize(&serde_json::to_vec(&m).unwrap()).unwrap());
    assert_eq!(
        got,
        "{\"Alpha\":[],\"zeta\":[3,-1],\"\u{1F602}\":[2],\"\u{FB33}\":[1]}".as_bytes()
    );
    let tuple = (1_i64, "two", true, Option::<u8>::None, vec!["a", "b"]);
    assert_eq!(
        to_canonical_vec(&tuple).unwrap(),
        br#"[1,"two",true,null,["a","b"]]"#.to_vec()
    );
}

// ---------------------------------------------------------------- property tests

/// REQ-DSL-001 / REQ-NFR-006: random documents canonicalise to the reference encoding,
/// independent of input formatting, and canonicalisation is idempotent.
#[test]
fn req_dsl_001_random_documents_match_reference_and_are_idempotent() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F1E_2D3C_4B5A_6978);
    for i in 0..600 {
        let v = random_value(&mut rng, 4);
        let expected = oracle_bytes(&v);
        let compact = serde_json::to_vec(&v).unwrap();
        let pretty = serde_json::to_vec_pretty(&v).unwrap();
        let a = canonicalize(&compact).unwrap_or_else(|e| panic!("doc {i}: {e:?}"));
        let b = canonicalize(&pretty).unwrap_or_else(|e| panic!("doc {i}: {e:?}"));
        assert_eq!(
            a,
            expected,
            "doc {i}: {}",
            String::from_utf8_lossy(&compact)
        );
        assert_eq!(b, expected, "doc {i} pretty");
        assert_eq!(canonicalize(&a).unwrap(), a, "idempotence doc {i}");
        assert_eq!(
            to_canonical_vec(&v).unwrap(),
            expected,
            "to_canonical_vec doc {i}"
        );
        // The canonical bytes still decode to the same value.
        let back: Value = serde_json::from_slice(&a).unwrap();
        assert_eq!(back, v);
    }
}

/// REQ-NFR-006 (R12 untrusted input): a bounded random-bytes run never panics, and any
/// accepted input yields valid, idempotent canonical JSON.
#[test]
fn req_nfr_006_random_bytes_never_panic() {
    const ALPHABET: &[u8] = b"{}[]\":,0123456789-+.eEtrufalsn \\u/\t\nab\x00\xff\xc3\xa9";
    let mut rng = Pcg64Dxsm::from_u64_seed(0xDEAD_BEEF_0000_0010);
    let mut accepted = 0usize;
    for _ in 0..3000 {
        let len = below(&mut rng, 64) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                if below(&mut rng, 4) == 0 {
                    rng.next_u64() as u8
                } else {
                    ALPHABET[below(&mut rng, ALPHABET.len() as u64) as usize]
                }
            })
            .collect();
        if let Ok(out) = canonicalize(&bytes) {
            accepted += 1;
            assert!(serde_json::from_slice::<Value>(&out).is_ok());
            assert_eq!(canonicalize(&out).unwrap(), out);
        }
    }
    // Not asserted further: the count only shows the run exercised both paths.
    let _ = accepted;
}

/// REQ-NFR-006 (R12): randomly mutated valid documents (byte flips, insertions, truncation)
/// never panic; accepted results are idempotent.
#[test]
fn req_nfr_006_mutated_documents_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x5EED_0000_0000_0011);
    for _ in 0..1500 {
        let v = random_value(&mut rng, 3);
        let mut bytes = serde_json::to_vec(&v).unwrap();
        let edits = 1 + below(&mut rng, 3);
        for _ in 0..edits {
            match below(&mut rng, 4) {
                0 if !bytes.is_empty() => {
                    let i = below(&mut rng, bytes.len() as u64) as usize;
                    bytes[i] ^= 1 << below(&mut rng, 8);
                }
                1 if !bytes.is_empty() => {
                    let i = below(&mut rng, bytes.len() as u64 + 1) as usize;
                    bytes.truncate(i);
                }
                2 => {
                    let i = below(&mut rng, bytes.len() as u64 + 1) as usize;
                    let b = b"{}[]\",:9e-\\"[below(&mut rng, 11) as usize];
                    bytes.insert(i, b);
                }
                _ if !bytes.is_empty() => {
                    let i = below(&mut rng, bytes.len() as u64) as usize;
                    bytes.remove(i);
                }
                _ => {}
            }
        }
        if let Ok(out) = canonicalize(&bytes) {
            assert_eq!(canonicalize(&out).unwrap(), out);
        }
    }
}
