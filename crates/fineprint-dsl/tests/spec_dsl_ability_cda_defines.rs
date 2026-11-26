// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the `defines` member of a layer-4 characteristic-defining CHAR effect
//! (REQ-DSL-008, SRS §14.4.3; REQ-OBJ-002 for its use by copy exceptions, CR 707.9d, 604.3a,
//! 109.3): on a layer-4 effect, `modification.defines` is a non-empty array of distinct strings
//! from exactly `SUPERTYPES`, `CARD_TYPES`, `SUBTYPES`; valid ones round-trip byte-identically
//! (canonical JSON), invalid ones are rejected by decode (errors, never panics); on other
//! layers it stays an ordinary opaque param. Black-box: written from the work-item brief only.
//! Synthetic data only (R5); pseudo-random inputs come from a hand-written LCG (R3).

use std::fmt::Debug;

use fineprint_dsl::ability::{AbilityAST, StaticEffect};
use fineprint_dsl::canonical::{canonicalize, to_canonical_vec};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const PARTS: [&str; 3] = ["SUPERTYPES", "CARD_TYPES", "SUBTYPES"];

// ---------------------------------------------------------------- helpers (local copies)

fn text(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

/// Decodes `s` as `T` and checks it re-serialises to the same JSON value and to the canonical
/// bytes of `s`.
fn accept<T>(s: &str) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let t: T = match serde_json::from_str::<T>(s) {
        Ok(t) => t,
        Err(e) => panic!("expected {s} to decode, got error: {e}"),
    };
    let t2: T = serde_json::from_slice(s.as_bytes()).unwrap();
    assert_eq!(t2, t);
    let original: Value = serde_json::from_str(s).unwrap();
    assert_eq!(
        serde_json::to_value(&t).unwrap(),
        original,
        "to_value of {s}"
    );
    let canon = canonicalize(s.as_bytes()).unwrap();
    assert_eq!(
        to_canonical_vec(&t).unwrap(),
        canon,
        "canonical bytes of {s}"
    );
    let from_canon: T = serde_json::from_slice(&canon).unwrap();
    assert_eq!(from_canon, t);
    assert_eq!(
        to_canonical_vec(&from_canon).unwrap(),
        canon,
        "canonical bytes are stable for {s}"
    );
    t
}

fn reject<T: DeserializeOwned + Debug>(s: &str) {
    let r = serde_json::from_str::<T>(s);
    assert!(r.is_err(), "expected rejection of {s}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<T>(s.as_bytes()).is_err(),
        "expected rejection (from_slice) of {s}"
    );
    if canonicalize(s.as_bytes()).is_ok() {
        let v: Value = serde_json::from_str(s).unwrap();
        assert!(
            serde_json::from_value::<T>(v).is_err(),
            "expected rejection (from_value) of {s}"
        );
    }
}

fn stat(effects: Value) -> Value {
    json!({"abilityId":"test.cda","kind":"STATIC","effects":effects})
}

/// A CHAR effect of `layer` on its own object whose modification is `ANY_OP` plus `defines`
/// (`None`: absent), with `isCDA` as given (`None`: absent).
fn effect(layer: &str, defines: Option<Value>, is_cda: Option<bool>) -> Value {
    let mut m = json!({"op":"ANY_OP"});
    if let Some(d) = defines {
        m.as_object_mut().unwrap().insert("defines".into(), d);
    }
    let mut e = json!({"type":"CHAR","layer":layer,"affected":{"ref":"SELF"},"modification":m});
    if let Some(b) = is_cda {
        e.as_object_mut().unwrap().insert("isCDA".into(), json!(b));
    }
    e
}

/// Accepts the effect standalone, inside a STATIC ability, and through the card-level
/// ability placeholder's typed view (`fineprint_dsl::card::AbilityAST::to_real`).
fn accept_effect(e: &Value) -> StaticEffect {
    let ability = stat(json!([e]));
    accept::<AbilityAST>(&text(&ability));
    let canon: Value =
        serde_json::from_slice(&canonicalize(text(&ability).as_bytes()).unwrap()).unwrap();
    if let Err(err) = fineprint_dsl::card::AbilityAST(canon).to_real() {
        panic!("card-level to_real rejected {e}: {err}");
    }
    accept::<StaticEffect>(&text(e))
}

/// Rejects the effect standalone, inside a STATIC ability, and through the card-level typed
/// view.
fn reject_effect(e: &Value) {
    reject::<StaticEffect>(&text(e));
    let ability = stat(json!([e]));
    reject::<AbilityAST>(&text(&ability));
    assert!(
        fineprint_dsl::card::AbilityAST(ability).to_real().is_err(),
        "card-level to_real accepted {e}"
    );
}

/// Every non-empty ordered selection of distinct parts (3 + 6 + 6 = 15).
fn all_valid_defines() -> Vec<Vec<&'static str>> {
    let mut out = Vec::new();
    for a in PARTS {
        out.push(vec![a]);
        for b in PARTS {
            if b == a {
                continue;
            }
            out.push(vec![a, b]);
            for c in PARTS {
                if c != a && c != b {
                    out.push(vec![a, b, c]);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- tests

/// REQ-DSL-008, REQ-OBJ-002, CR 707.9d, 109.3: every non-empty array of distinct part names, in
/// every order, is accepted on a layer-4 CDA and re-serialises byte-identically; different
/// arrays (including different orders) decode to different values.
#[test]
fn req_dsl_008_valid_defines_round_trip() {
    let all = all_valid_defines();
    assert_eq!(all.len(), 15);
    let mut seen: Vec<StaticEffect> = Vec::new();
    for d in &all {
        let e = accept_effect(&effect("4", Some(json!(d)), Some(true)));
        assert!(!seen.contains(&e), "{d:?}");
        seen.push(e);
    }
    // The brief's example effect, as literal text.
    let s = r#"{"type":"CHAR","layer":"4","affected":{"ref":"SELF"},"modification":{"op":"ANY_OP","defines":["SUBTYPES"]},"isCDA":true}"#;
    let e: StaticEffect = accept(s);
    assert_eq!(
        String::from_utf8(to_canonical_vec(&e).unwrap()).unwrap(),
        String::from_utf8(canonicalize(s.as_bytes()).unwrap()).unwrap()
    );
}

/// REQ-DSL-008: `defines` is a layer-4 member regardless of the `isCDA` flag's spelling
/// (true, false or absent): a valid array is accepted and an invalid one rejected in each case.
#[test]
fn req_dsl_008_defines_on_layer_4_with_any_is_cda() {
    for is_cda in [Some(true), Some(false), None] {
        accept_effect(&effect("4", Some(json!(["CARD_TYPES"])), is_cda));
        reject_effect(&effect("4", Some(json!(["NOPE"])), is_cda));
        reject_effect(&effect("4", Some(json!([])), is_cda));
    }
}

/// REQ-DSL-008; R12: on a layer-4 effect, an empty array, a duplicate, an unknown name, a
/// wrong-case name, a non-string element and a non-array value are rejected by decode (an
/// error, never a panic). `null` is rejected as for every param.
#[test]
fn req_dsl_008_invalid_defines_rejected() {
    let bad = [
        json!([]),
        json!(["SUBTYPES", "SUBTYPES"]),
        json!(["SUPERTYPES", "CARD_TYPES", "SUPERTYPES"]),
        json!(["SUPERTYPES", "CARD_TYPES", "SUBTYPES", "SUBTYPES"]),
        json!(["NOPE"]),
        json!(["SUBTYPES", "NOPE"]),
        json!(["TYPES"]),
        json!(["COLORS"]),
        json!(["POWER"]),
        json!(["SUBTYPE"]),
        json!(["CARDTYPES"]),
        json!(["CARD TYPES"]),
        json!(["CARD-TYPES"]),
        json!([""]),
        json!([" SUBTYPES"]),
        json!(["SUBTYPES "]),
        json!(["subtypes"]),
        json!(["Subtypes"]),
        json!(["SUPERtypes"]),
        json!(["card_types"]),
        json!([1]),
        json!(["SUBTYPES", 1]),
        json!([true]),
        json!([["SUBTYPES"]]),
        json!([{"SUBTYPES": true}]),
        json!([null]),
        json!("SUBTYPES"),
        json!("SUPERTYPES,SUBTYPES"),
        json!(3),
        json!(true),
        json!({"SUBTYPES": true}),
        json!({}),
        json!(null),
    ];
    for d in bad {
        reject_effect(&effect("4", Some(d), Some(true)));
    }
}

/// REQ-DSL-008, SRS §14.4.3: on a non-layer-4 CHAR effect, `defines` is an ordinary opaque
/// param: accepted with any (non-null, integer-only) JSON value, and round-trips.
#[test]
fn req_dsl_008_defines_is_opaque_on_other_layers() {
    let values = [
        json!(["SUBTYPES"]),
        json!(["NOPE"]),
        json!([]),
        json!(["SUBTYPES", "SUBTYPES"]),
        json!(["subtypes"]),
        json!("SUBTYPES"),
        json!(1),
        json!(false),
        json!({"a": 1}),
        json!([1, "x", false]),
    ];
    for layer in ["1a", "1b", "2", "3", "5", "6", "7a", "7b", "7c", "7d"] {
        for v in &values {
            accept_effect(&effect(layer, Some(v.clone()), Some(true)));
            accept_effect(&effect(layer, Some(v.clone()), None));
        }
    }
}

/// REQ-DSL-008: existing behaviour is unchanged: a layer-4 effect without `defines` decodes and
/// round-trips (with `isCDA` true, false or absent), and differs from one with `defines`.
#[test]
fn req_dsl_008_layer_4_without_defines_unchanged() {
    let plain = accept_effect(&effect("4", None, Some(true)));
    accept_effect(&effect("4", None, Some(false)));
    accept_effect(&effect("4", None, None));
    let all = accept_effect(&effect("4", Some(json!(PARTS)), Some(true)));
    assert_ne!(plain, all);
    // Other params next to `defines` stay opaque.
    let mut e = effect("4", Some(json!(["SUBTYPES"])), Some(true));
    e["modification"]
        .as_object_mut()
        .unwrap()
        .insert("subtype".into(), json!("test_kind"));
    accept_effect(&e);
    // `defines` outside the modification is an unknown effect member, as before.
    let mut e = effect("4", None, Some(true));
    e.as_object_mut()
        .unwrap()
        .insert("defines".into(), json!(["SUBTYPES"]));
    reject_effect(&e);
}

/// REQ-DSL-008; R12: duplicate `defines` keys in the text are rejected (as for every param).
#[test]
fn req_dsl_008_duplicate_defines_key_rejected() {
    reject::<StaticEffect>(
        r#"{"type":"CHAR","layer":"4","affected":{"ref":"SELF"},"modification":{"op":"ANY_OP","defines":["SUBTYPES"],"defines":["SUBTYPES"]},"isCDA":true}"#,
    );
}

/// A 64-bit LCG (Knuth MMIX constants), local copy.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg(seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// REQ-DSL-008: property (500 seeds). A random array of up to 4 strings drawn from the three
/// part names and near misses is accepted on a layer-4 effect exactly when it is non-empty,
/// every element is one of the three names and no element repeats; accepted ones round-trip.
#[test]
fn req_dsl_008_property_defines_validity() {
    let pool = [
        "SUPERTYPES",
        "CARD_TYPES",
        "SUBTYPES",
        "subtypes",
        "SUBTYPE",
        "TYPES",
        "",
        "Card_Types",
    ];
    for seed in 0..500u64 {
        let mut rng = Lcg::new(seed);
        let n = rng.below(5) as usize;
        let d: Vec<&str> = (0..n)
            .map(|_| pool[rng.below(pool.len() as u64) as usize])
            .collect();
        let distinct = d.iter().enumerate().all(|(i, x)| !d[..i].contains(x));
        let valid = !d.is_empty() && distinct && d.iter().all(|x| PARTS.contains(x));
        let e = effect("4", Some(json!(d)), Some(true));
        if valid {
            accept_effect(&e);
        } else {
            reject_effect(&e);
        }
    }
}

/// R12: bounded random byte mutations of a valid layer-4 effect with `defines` never panic the
/// decoders (the result may be accepted or rejected).
#[test]
fn req_dsl_008_random_bytes_do_not_panic() {
    let base = text(&effect(
        "4",
        Some(json!(["SUPERTYPES", "CARD_TYPES", "SUBTYPES"])),
        Some(true),
    ));
    let ability = text(&stat(json!([effect(
        "4",
        Some(json!(["SUBTYPES"])),
        Some(true)
    )])));
    let alphabet = b"[]{},:\"SUBTYPESCARD_ 01nul";
    for seed in 0..1000u64 {
        let mut rng = Lcg::new(seed);
        for src in [&base, &ability] {
            let mut bytes = src.as_bytes().to_vec();
            for _ in 0..=rng.below(4) {
                let i = rng.below(bytes.len() as u64) as usize;
                match rng.below(3) {
                    0 => bytes[i] = alphabet[rng.below(alphabet.len() as u64) as usize],
                    1 => {
                        bytes.remove(i);
                    }
                    _ => bytes.insert(i, rng.below(256) as u8),
                }
            }
            let _ = serde_json::from_slice::<StaticEffect>(&bytes);
            let _ = serde_json::from_slice::<AbilityAST>(&bytes);
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                let _ = fineprint_dsl::card::AbilityAST(v).to_real();
            }
        }
    }
}
