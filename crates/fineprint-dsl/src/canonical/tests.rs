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
