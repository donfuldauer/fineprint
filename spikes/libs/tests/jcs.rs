// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! T2 and T6 (REQ-NFR-006): RFC 8785 JCS vectors, both candidates, and bounded random input.
//!
//! Provenance of the hard-coded expectations:
//! - `APPENDIX_B`: the "Number Serialization Samples" table of the JCS specification, copied
//!   from the RFC author's source (cyberphone/ietf-json-canon,
//!   draft-rundgren-json-canonicalization-scheme.xml); every row was also re-checked with
//!   Node.js 22 `JSON.stringify` (ECMAScript reference behaviour).
//! - `REFERENCE_FILES`: input/output pairs of the RFC author's reference test data
//!   (cyberphone/json-canonicalization `testdata/input` and `testdata/output`), verbatim.
//!   `weird` is the property-sorting example (UTF-16 code-unit order).
//! - `NODE_RANDOM`: NOT published. 32 finite doubles drawn from NumPy PCG64DXSM(8785) bits,
//!   formatted by Node.js 22 `JSON.stringify`.

// non-rules crate: REQ-CORE-006 applies to the rules crates only
#![allow(clippy::disallowed_types)]

use fineprint_spike_libs::jcs::canonicalize;
use fineprint_spike_libs::prng::Pcg64Dxsm;

const APPENDIX_B: [(u64, &str); 23] = [
    (0x0000000000000000, "0"),
    (0x8000000000000000, "0"),
    (0x0000000000000001, "5e-324"),
    (0x8000000000000001, "-5e-324"),
    (0x7fefffffffffffff, "1.7976931348623157e+308"),
    (0xffefffffffffffff, "-1.7976931348623157e+308"),
    (0x4340000000000000, "9007199254740992"),
    (0xc340000000000000, "-9007199254740992"),
    (0x4430000000000000, "295147905179352830000"),
    (0x44b52d02c7e14af5, "9.999999999999997e+22"),
    (0x44b52d02c7e14af6, "1e+23"),
    (0x44b52d02c7e14af7, "1.0000000000000001e+23"),
    (0x444b1ae4d6e2ef4e, "999999999999999700000"),
    (0x444b1ae4d6e2ef4f, "999999999999999900000"),
    (0x444b1ae4d6e2ef50, "1e+21"),
    (0x3eb0c6f7a0b5ed8c, "9.999999999999997e-7"),
    (0x3eb0c6f7a0b5ed8d, "0.000001"),
    (0x41b3de4355555553, "333333333.3333332"),
    (0x41b3de4355555554, "333333333.33333325"),
    (0x41b3de4355555555, "333333333.3333333"),
    (0x41b3de4355555556, "333333333.3333334"),
    (0x41b3de4355555557, "333333333.33333343"),
    (0xbecbf647612f3696, "-0.0000033333333333333333"),
];

/// Appendix B rows that MUST fail: NaN and Infinity.
const APPENDIX_B_INVALID: [u64; 2] = [0x7fffffffffffffff, 0x7ff0000000000000];

const NODE_RANDOM: [(u64, &str); 32] = [
    (0xb60d247452378dae, "-2.492504898358219e-48"),
    (0xfb28ee5b86713cd0, "-1.853647177432394e+285"),
    (0x293f45afa88af524, "5.2013992591013346e-110"),
    (0x137049a8b9a2091b, "4.724802801687416e-215"),
    (0xe7e88d95ca1038c3, "-3.5006846718604783e+192"),
    (0x291f39861fd3795f, "1.298374322693651e-110"),
    (0x5fd0a7f4a6654470, "3.4893981057516876e+153"),
    (0xba935b7337b6566f, "-1.5636643308367093e-26"),
    (0x2da7b174752e74d5, "9.304964086780834e-89"),
    (0x87bfef1c8852da9c, "-2.3612264353401064e-271"),
    (0x335c581539084955, "2.7560250703997612e-61"),
    (0xb35132b0d5f8176d, "-1.6722419368731585e-61"),
    (0xc34c00e184dcfdd3, "-15764535889230758"),
    (0x12f6aeca01321026, "2.570272830467421e-217"),
    (0x14a39e80f6588a7b, "2.983824855564293e-209"),
    (0xdf5555907da35a4a, "-1.7458821940729004e+151"),
    (0x2cabb1c36fe6e19a, "1.6595988896488078e-93"),
    (0xcb0fdcdb54c45675, "-3.8148030442930173e+53"),
    (0x2428c126b445085d, "1.7028878622890797e-134"),
    (0x90ec07e8762f7cbb, "-3.697687306295254e-227"),
    (0x02fe9c1364de0ba4, "2.995455761059581e-294"),
    (0x6a5a7272cd2b0a8b, "2.0729773052283062e+204"),
    (0x167f88713ab9045b, "2.574707397068409e-200"),
    (0x2e13dbd50b9be951, "9.98280298915044e-87"),
    (0xcb29d9bee7d47d01, "-1.237996315193283e+54"),
    (0x648611bfbdd39ce0, "1.746693357483493e+176"),
    (0x883a283696400965, "-4.951231063093988e-269"),
    (0x5bed6a0ea88bf164, "6.68107569852958e+134"),
    (0x2512dbe0a5a87259, "4.251069067279374e-130"),
    (0x9dd029cf9c114e63, "-4.385640882842958e-165"),
    (0xc465aaa6e1311630, "-3.1974271118513935e+21"),
    (0x8810310b54d75f43, "-7.662193246070677e-270"),
];

const REFERENCE_FILES: [(&str, &[u8], &[u8]); 6] = [
    (
        "arrays",
        b"[\n  56,\n  {\n    \"d\": true,\n    \"10\": null,\n    \"1\": [ ]\n  }\n]\n",
        b"[56,{\"1\":[],\"10\":null,\"d\":true}]",
    ),
    (
        "french",
        b"{\n  \"peach\": \"This sorting order\",\n  \"p\xc3\xa9ch\xc3\xa9\": \"is wrong according to French\",\n  \"p\xc3\xaache\": \"but canonicalization MUST\",\n  \"sin\":   \"ignore locale\"\n}\n",
        b"{\"peach\":\"This sorting order\",\"p\xc3\xa9ch\xc3\xa9\":\"is wrong according to French\",\"p\xc3\xaache\":\"but canonicalization MUST\",\"sin\":\"ignore locale\"}",
    ),
    (
        "structures",
        b"{\n  \"1\": {\"f\": {\"f\": \"hi\",\"F\": 5} ,\"\\n\": 56.0},\n  \"10\": { },\n  \"\": \"empty\",\n  \"a\": { },\n  \"111\": [ {\"e\": \"yes\",\"E\": \"no\" } ],\n  \"A\": { }\n}",
        b"{\"\":\"empty\",\"1\":{\"\\n\":56,\"f\":{\"F\":5,\"f\":\"hi\"}},\"10\":{},\"111\":[{\"E\":\"no\",\"e\":\"yes\"}],\"A\":{},\"a\":{}}",
    ),
    (
        "unicode",
        b"{\n  \"Unnormalized Unicode\":\"A\\u030a\"\n}\n",
        b"{\"Unnormalized Unicode\":\"A\xcc\x8a\"}",
    ),
    (
        "values",
        b"{\n  \"numbers\": [333333333.33333329, 1E30, 4.50, 2e-3, 0.000000000000000000000000001],\n  \"string\": \"\\u20ac$\\u000F\\u000aA'\\u0042\\u0022\\u005c\\\\\\\"\\/\",\n  \"literals\": [null, true, false]\n}",
        b"{\"literals\":[null,true,false],\"numbers\":[333333333.3333333,1e+30,4.5,0.002,1e-27],\"string\":\"\xe2\x82\xac$\\u000f\\nA'B\\\"\\\\\\\\\\\"/\"}",
    ),
    (
        "weird",
        b"{\n  \"\\u20ac\": \"Euro Sign\",\n  \"\\r\": \"Carriage Return\",\n  \"\\u000a\": \"Newline\",\n  \"1\": \"One\",\n  \"\\u0080\": \"Control\\u007f\",\n  \"\\ud83d\\ude02\": \"Smiley\",\n  \"\\u00f6\": \"Latin Small Letter O With Diaeresis\",\n  \"\\ufb33\": \"Hebrew Letter Dalet With Dagesh\",\n  \"</script>\": \"Browser Challenge\"\n}\n",
        b"{\"\\n\":\"Newline\",\"\\r\":\"Carriage Return\",\"1\":\"One\",\"</script>\":\"Browser Challenge\",\"\xc2\x80\":\"Control\x7f\",\"\xc3\xb6\":\"Latin Small Letter O With Diaeresis\",\"\xe2\x82\xac\":\"Euro Sign\",\"\xf0\x9f\x98\x82\":\"Smiley\",\"\xef\xac\xb3\":\"Hebrew Letter Dalet With Dagesh\"}",
    ),
];

type Canonicalizer = fn(&serde_json::Value) -> Option<Vec<u8>>;

fn chosen(value: &serde_json::Value) -> Option<Vec<u8>> {
    serde_json_canonicalizer::to_vec(value).ok()
}

fn serde_jcs_candidate(value: &serde_json::Value) -> Option<Vec<u8>> {
    serde_jcs::to_vec(value).ok()
}

const CANDIDATES: [(&str, Canonicalizer); 2] = [
    ("serde_json_canonicalizer", chosen),
    ("serde_jcs", serde_jcs_candidate),
];

fn number(bits: u64) -> serde_json::Value {
    serde_json::Value::from(f64::from_bits(bits))
}

#[test]
fn req_nfr_006_appendix_b_number_table() {
    for (name, canonicalize_value) in CANDIDATES {
        for (bits, expected) in APPENDIX_B.iter().chain(NODE_RANDOM.iter()) {
            assert_eq!(
                canonicalize_value(&number(*bits)).as_deref(),
                Some(expected.as_bytes()),
                "{name} {bits:016x}"
            );
        }
    }
    for bits in APPENDIX_B_INVALID {
        assert!(serde_json_canonicalizer::to_vec(&f64::from_bits(bits)).is_err());
        assert!(serde_jcs::to_vec(&f64::from_bits(bits)).is_err());
    }
}

#[test]
fn req_nfr_006_reference_files_including_utf16_key_order_and_escaping() {
    for (name, input, expected) in REFERENCE_FILES {
        assert_eq!(
            canonicalize(input).ok().as_deref(),
            Some(expected),
            "{name}"
        );
        let value: serde_json::Value = serde_json::from_slice(input).unwrap_or_default();
        assert_eq!(
            serde_jcs_candidate(&value).as_deref(),
            Some(expected),
            "serde_jcs {name}"
        );
    }
}

/// U+1F602 (UTF-16 D83D DE02) sorts before U+FB33 in UTF-16 code-unit order, but after it in
/// UTF-8 byte order and code-point order: the expected output proves UTF-16 ordering.
#[test]
fn req_nfr_006_key_order_is_utf16_not_utf8() {
    let smiley = "\u{1F602}";
    let dalet = "\u{FB33}";
    assert!(dalet.as_bytes() < smiley.as_bytes());
    assert!(smiley.encode_utf16().lt(dalet.encode_utf16()));
    let input = format!("{{\"{dalet}\":2,\"{smiley}\":1}}");
    let expected = format!("{{\"{smiley}\":1,\"{dalet}\":2}}");
    assert_eq!(
        canonicalize(input.as_bytes()).ok(),
        Some(expected.into_bytes())
    );
}

#[test]
fn req_nfr_006_string_escaping() {
    let input = br#"["\u0000\u001f\u007f\b\t\n\f\r\"\\\/\u00e9\u20ac\ud83d\ude02"]"#;
    let expected =
        b"[\"\\u0000\\u001f\x7f\\b\\t\\n\\f\\r\\\"\\\\/\xc3\xa9\xe2\x82\xac\xf0\x9f\x98\x82\"]";
    assert_eq!(canonicalize(input).ok().as_deref(), Some(&expected[..]));
    // A lone surrogate is not valid Unicode and must be rejected.
    assert!(canonicalize(br#"["\ud800"]"#).is_err());
}

/// Evidence for DECISIONS.md: integers beyond 2^53 are serialised as the nearest double
/// (ECMAScript behaviour), i.e. silently rounded; both candidates agree.
#[test]
fn req_nfr_006_large_integers_are_rounded_like_ecmascript() {
    let cases = [
        (
            b"9007199254740993".as_slice(),
            b"9007199254740992".as_slice(),
        ),
        (b"18446744073709551615", b"18446744073709552000"),
        (b"-9223372036854775808", b"-9223372036854776000"),
        (b"18446744073709551616", b"18446744073709552000"),
    ];
    for (input, expected) in cases {
        assert_eq!(canonicalize(input).ok().as_deref(), Some(expected));
        let value: serde_json::Value = serde_json::from_slice(input).unwrap_or_default();
        assert_eq!(serde_jcs_candidate(&value).as_deref(), Some(expected));
    }
    // Out-of-range literals are rejected by the parser rather than becoming Infinity.
    assert!(canonicalize(b"1e400").is_err());
}

/// R12: bounded random inputs never panic; accepted inputs canonicalise idempotently.
#[test]
fn req_nfr_006_random_inputs_never_panic() {
    const TOKENS: [&[u8]; 16] = [
        b"{",
        b"}",
        b"[",
        b"]",
        b":",
        b",",
        b"\"k\"",
        b"\"\\u00e9\"",
        b"\"\\ud83d\\ude02\"",
        b"1",
        b"-0.5e3",
        b"1e21",
        b"true",
        b"null",
        b" ",
        b"\"\\ud800\"",
    ];
    let mut rng = Pcg64Dxsm::new(12, 34);
    let mut accepted = 0;
    for round in 0..4000 {
        let len = (rng.next_u64() % 24) as usize;
        let mut input = Vec::new();
        for _ in 0..len {
            let pick = rng.next_u64();
            if round % 2 == 0 {
                input.extend_from_slice(TOKENS[(pick % 16) as usize]);
            } else {
                input.push(pick as u8);
            }
        }
        if let Ok(canonical) = canonicalize(&input) {
            accepted += 1;
            assert_eq!(canonicalize(&canonical).ok(), Some(canonical));
        }
    }
    assert!(accepted > 0);
}

/// REQ-NFR-006: RFC 8785 requires I-JSON input (no duplicate member names); a plain duplicate
/// is rejected instead of silently keeping the last value.
#[test]
fn req_nfr_006_rejects_duplicate_keys_unescaped() {
    assert!(canonicalize(br#"{"a":1,"a":2}"#).is_err());
    assert!(canonicalize(br#"{"a":1,"b":2,"a":3}"#).is_err());
}

/// REQ-NFR-006: names are compared after JSON string decoding, so different escape spellings of
/// the same name are duplicates.
#[test]
fn req_nfr_006_rejects_duplicate_keys_escaped() {
    assert!(canonicalize(br#"{"\u0061":1,"a":2}"#).is_err());
    assert!(canonicalize(br#"{"a":1,"\u0061":2}"#).is_err());
    assert!(canonicalize(br#"{"/":1,"\/":2}"#).is_err());
    assert!(canonicalize("{\"\u{1F602}\":1,\"\\ud83d\\ude02\":2}".as_bytes()).is_err());
    assert!(canonicalize("{\"\\ud83d\\ude02\":1,\"\u{1F602}\":2}".as_bytes()).is_err());
}

/// REQ-NFR-006: duplicates are rejected in every nested object, including inside arrays.
#[test]
fn req_nfr_006_rejects_duplicate_keys_nested() {
    assert!(canonicalize(br#"{"x":{"a":1,"a":2}}"#).is_err());
    assert!(canonicalize(br#"[{"a":1,"a":2}]"#).is_err());
    assert!(canonicalize(br#"{"x":[1,{"y":{"z":[{"b":true,"b":false}]}}]}"#).is_err());
    // The first object is valid; the duplicate sits in a later array element.
    assert!(canonicalize(br#"[{"a":1},{"c":{"d":null,"d":null}}]"#).is_err());
}

/// REQ-NFR-006: the same name in different objects, or names differing only in case, are not
/// duplicates.
#[test]
fn req_nfr_006_accepts_same_key_in_different_objects() {
    assert_eq!(
        canonicalize(br#"{"a":{"a":1},"b":{"a":2}}"#)
            .ok()
            .as_deref(),
        Some(&br#"{"a":{"a":1},"b":{"a":2}}"#[..])
    );
    assert_eq!(
        canonicalize(br#"[{"a":1},{"a":2}]"#).ok().as_deref(),
        Some(&br#"[{"a":1},{"a":2}]"#[..])
    );
    assert_eq!(
        canonicalize(br#"{"a":1,"A":2}"#).ok().as_deref(),
        Some(&br#"{"A":2,"a":1}"#[..])
    );
}

/// REQ-NFR-006 / R12: very deep nesting is rejected by the recursion limit, not a stack overflow.
#[test]
fn req_nfr_006_deep_nesting_is_rejected_without_crashing() {
    const DEPTH: usize = 10_000;
    let mut arrays = Vec::new();
    arrays.resize(DEPTH, b'[');
    arrays.resize(2 * DEPTH, b']');
    assert!(canonicalize(&arrays).is_err());
    let mut objects = b"{\"k\":".repeat(DEPTH);
    objects.extend_from_slice(b"1");
    objects.extend(std::iter::repeat_n(b'}', DEPTH));
    assert!(canonicalize(&objects).is_err());
    // Trailing characters after a valid document are still rejected.
    assert!(canonicalize(br#"{"a":1} x"#).is_err());
}
