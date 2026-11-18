// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the `RulesVocabulary` loader (REQ-OBJ-001, REQ-SUB-001, REQ-DSL-014).
//! Synthetic vocabularies use invented names only (R5).

use serde_json::{Value, json};

use super::*;

fn s(x: &str) -> String {
    x.to_owned()
}

fn face(name: &str, card_types: &[&str], subtypes: &[&str]) -> TokenFace {
    TokenFace {
        name: s(name),
        colors: vec![],
        supertypes: vec![],
        card_types: card_types.iter().map(|c| s(c)).collect(),
        subtypes: subtypes.iter().map(|c| s(c)).collect(),
        power: None,
        toughness: None,
        abilities: vec![s("{T}: Quux.")],
    }
}

/// A small valid vocabulary with invented names.
fn sample() -> RulesVocabulary {
    RulesVocabulary {
        vocabulary_version: 1,
        cr_effective_date: s("2031-03-04"),
        card_types: vec![s("Alpha"), s("Beta"), s("Gamma")],
        supertypes: vec![s("Lofty")],
        subtype_groups: vec![
            SubtypeGroup {
                group: s("alpha"),
                card_types: vec![s("Alpha")],
                rule_ref: s("205.3g"),
                subtypes: vec![s("Zeta"), s("Mu’s-Kin")],
            },
            SubtypeGroup {
                group: s("beta"),
                card_types: vec![s("Beta"), s("Gamma")],
                rule_ref: s("205.3h"),
                subtypes: vec![s("Omicron Prime"), s("Kappa")],
            },
        ],
        keyword_abilities: vec![Keyword {
            id: s("GLEAM"),
            name: s("Gleam"),
            rule_ref: s("702.2"),
        }],
        keyword_actions: vec![Keyword {
            id: s("FROBNICATE"),
            name: s("Frobnicate"),
            rule_ref: s("701.2"),
        }],
        counter_kinds: vec![CounterKind {
            kind: s("gleam"),
            class: s("KEYWORD"),
            rule_ref: s("122.1b"),
        }],
        predefined_tokens: vec![
            PredefinedToken {
                letter: s("a"),
                rule_ref: s("111.10a"),
                name: s("Zeta"),
                faces: vec![face("Zeta", &["Alpha"], &["Zeta"])],
            },
            PredefinedToken {
                letter: s("c"),
                rule_ref: s("111.10c"),
                name: s("Eta"),
                faces: vec![
                    face("Eta", &["Alpha"], &[]),
                    TokenFace {
                        power: Some(0),
                        toughness: Some(0),
                        colors: vec![s("blue"), s("red")],
                        ..face("Eta Back", &["Alpha", "Beta"], &["Kappa"])
                    },
                ],
            },
        ],
    }
}

fn sample_value() -> Value {
    serde_json::to_value(sample()).unwrap()
}

fn load(v: &Value) -> Result<RulesVocabulary, VocabError> {
    RulesVocabulary::from_slice(&serde_json::to_vec(v).unwrap())
}

/// REQ-OBJ-001, REQ-SUB-001: a valid vocabulary loads and round-trips canonically.
#[test]
fn req_obj_001_sample_round_trip() {
    let v = sample();
    let bytes = v.to_canonical_bytes().unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
    let back = RulesVocabulary::from_slice(&bytes).unwrap();
    assert_eq!(back, v);
    assert_eq!(back.to_canonical_bytes().unwrap(), bytes);
    let text = std::str::from_utf8(&bytes).unwrap();
    assert_eq!(RulesVocabulary::from_str(text).unwrap(), v);
    assert_eq!(text.parse::<RulesVocabulary>().unwrap(), v);
    // canonical: canonicalize is the identity on the JSON part
    let json = &bytes[..bytes.len() - 1];
    assert_eq!(canonical::canonicalize(json).unwrap(), json);
}

/// REQ-OBJ-001, REQ-DSL-014, REQ-SUB-001: lookup helpers.
#[test]
fn req_obj_001_lookups() {
    let v = sample();
    assert!(v.is_card_type("Alpha"));
    assert!(!v.is_card_type("alpha"));
    assert!(v.is_supertype("Lofty"));
    assert!(!v.is_supertype("Alpha"));
    assert_eq!(
        v.subtypes_of("Beta").collect::<Vec<_>>(),
        ["Omicron Prime", "Kappa"]
    );
    assert_eq!(
        v.subtypes_of("Gamma").collect::<Vec<_>>(),
        v.subtypes_of("Beta").collect::<Vec<_>>()
    );
    assert_eq!(v.subtypes_of("Delta").count(), 0);
    assert!(v.is_subtype_of("Alpha", "Mu’s-Kin"));
    assert!(!v.is_subtype_of("Alpha", "Kappa"));
    assert_eq!(v.keyword_ability("GLEAM").unwrap().rule_ref, "702.2");
    assert!(v.keyword_ability("FROBNICATE").is_none());
    assert_eq!(v.keyword_action("FROBNICATE").unwrap().name, "Frobnicate");
    assert!(v.keyword_action("GLEAM").is_none());
    assert_eq!(v.token("a").unwrap().name, "Zeta");
    assert_eq!(v.token("Eta").unwrap().letter, "c");
    assert!(v.token("b").is_none());
    assert_eq!(v.counter_kind("gleam").unwrap().class, "KEYWORD");
    assert!(v.counter_kind("shine").is_none());
}

/// REQ-OBJ-001: the union over groups has no repeats when two groups share a subtype.
#[test]
fn req_obj_001_subtypes_of_is_a_union() {
    let mut v = sample();
    v.subtype_groups.push(SubtypeGroup {
        group: s("gamma"),
        card_types: vec![s("Gamma")],
        rule_ref: s("205.3i"),
        subtypes: vec![s("Kappa"), s("Rho")],
    });
    assert_eq!(
        v.subtypes_of("Gamma").collect::<Vec<_>>(),
        ["Omicron Prime", "Kappa", "Rho"]
    );
}

fn expect_err(mutate: impl FnOnce(&mut Value)) -> VocabError {
    let mut v = sample_value();
    mutate(&mut v);
    load(&v).expect_err("mutated vocabulary must be rejected")
}

type Mutation = Box<dyn Fn(&mut Value)>;

/// REQ-OBJ-001 (R12): the loader rejects duplicate keys, unknown fields, wrong versions and
/// missing members.
#[test]
fn req_obj_001_loader_rejects_shape() {
    let dup = br#"{"vocabularyVersion":1,"vocabularyVersion":1}"#;
    assert!(matches!(
        RulesVocabulary::from_slice(dup),
        Err(VocabError::Json(CanonicalError::DuplicateKey(_)))
    ));
    assert!(matches!(
        expect_err(|v| v["extra"] = json!(1)),
        VocabError::Shape(_)
    ));
    assert!(matches!(
        expect_err(|v| v["predefinedTokens"][0]["faces"][0]["extra"] = json!("x")),
        VocabError::Shape(_)
    ));
    for bad in [json!(2), json!(0), json!("1"), json!(1.5), json!(null)] {
        assert!(matches!(
            expect_err(|v| v["vocabularyVersion"] = bad),
            VocabError::Version(_)
        ));
    }
    assert!(matches!(
        expect_err(|v| {
            v.as_object_mut().unwrap().remove("vocabularyVersion");
        }),
        VocabError::Version(_)
    ));
    assert!(matches!(
        RulesVocabulary::from_slice(b"[1]"),
        Err(VocabError::Shape(_))
    ));
    assert!(matches!(
        expect_err(|v| {
            v.as_object_mut().unwrap().remove("cardTypes");
        }),
        VocabError::Shape(_)
    ));
    assert!(matches!(
        expect_err(|v| v["cardTypes"] = json!("Alpha")),
        VocabError::Shape(_)
    ));
}

/// REQ-OBJ-001, REQ-SUB-001 (R12): malformed entries and duplicates are `Invalid`.
#[test]
fn req_obj_001_loader_rejects_values() {
    let tok = |v: &mut Value, k: &str, x: Value| v["predefinedTokens"][0][k] = x;
    let face0 = |v: &mut Value, k: &str, x: Value| v["predefinedTokens"][0]["faces"][0][k] = x;
    let cases: Vec<(&str, Mutation)> = vec![
        (
            "letter two chars",
            Box::new(move |v| tok(v, "letter", json!("ab"))),
        ),
        (
            "letter upper",
            Box::new(move |v| tok(v, "letter", json!("A"))),
        ),
        (
            "letter non-ascii",
            Box::new(move |v| tok(v, "letter", json!("é"))),
        ),
        (
            "letter empty",
            Box::new(move |v| tok(v, "letter", json!(""))),
        ),
        (
            "duplicate letters",
            Box::new(|v| v["predefinedTokens"][1]["letter"] = json!("a")),
        ),
        (
            "duplicate token names",
            Box::new(|v| v["predefinedTokens"][1]["name"] = json!("Zeta")),
        ),
        ("zero faces", Box::new(move |v| tok(v, "faces", json!([])))),
        (
            "three faces",
            Box::new(|v| {
                let f = v["predefinedTokens"][0]["faces"][0].clone();
                v["predefinedTokens"][0]["faces"] = json!([f.clone(), f.clone(), f]);
            }),
        ),
        (
            "empty token name",
            Box::new(move |v| tok(v, "name", json!(""))),
        ),
        (
            "empty face name",
            Box::new(move |v| face0(v, "name", json!(""))),
        ),
        (
            "bad token ruleRef",
            Box::new(move |v| tok(v, "ruleRef", json!("x"))),
        ),
        (
            "padded name",
            Box::new(|v| v["cardTypes"][0] = json!(" Alpha")),
        ),
        (
            "control char",
            Box::new(|v| v["supertypes"][0] = json!("Lo\u{7}fty")),
        ),
        (
            "empty card type",
            Box::new(|v| v["cardTypes"][0] = json!("")),
        ),
        ("no card types", Box::new(|v| v["cardTypes"] = json!([]))),
        (
            "duplicate card type",
            Box::new(|v| v["cardTypes"][1] = json!("Alpha")),
        ),
        (
            "duplicate supertype",
            Box::new(|v| v["supertypes"] = json!(["Lofty", "Lofty"])),
        ),
        (
            "duplicate subtype in group",
            Box::new(|v| v["subtypeGroups"][0]["subtypes"] = json!(["Zeta", "Zeta"])),
        ),
        (
            "empty subtype list",
            Box::new(|v| v["subtypeGroups"][0]["subtypes"] = json!([])),
        ),
        (
            "empty subtype name",
            Box::new(|v| v["subtypeGroups"][0]["subtypes"][0] = json!("")),
        ),
        (
            "empty group",
            Box::new(|v| v["subtypeGroups"][0]["group"] = json!("")),
        ),
        (
            "duplicate group",
            Box::new(|v| v["subtypeGroups"][1]["group"] = json!("alpha")),
        ),
        (
            "group unknown card type",
            Box::new(|v| v["subtypeGroups"][0]["cardTypes"] = json!(["Delta"])),
        ),
        (
            "group without card types",
            Box::new(|v| v["subtypeGroups"][0]["cardTypes"] = json!([])),
        ),
        (
            "bad group ruleRef",
            Box::new(|v| v["subtypeGroups"][0]["ruleRef"] = json!("205")),
        ),
        (
            "bad keyword id",
            Box::new(|v| v["keywordAbilities"][0]["id"] = json!("gleam")),
        ),
        (
            "keyword id double underscore",
            Box::new(|v| v["keywordAbilities"][0]["id"] = json!("A__B")),
        ),
        (
            "keyword id collision across lists",
            Box::new(|v| v["keywordAbilities"][0]["id"] = json!("FROBNICATE")),
        ),
        (
            "duplicate keyword name",
            Box::new(|v| {
                let k = v["keywordActions"][0].clone();
                v["keywordActions"] =
                    json!([k, {"id":"OTHER","name":"Frobnicate","ruleRef":"701.3"}]);
            }),
        ),
        (
            "empty keyword name",
            Box::new(|v| v["keywordActions"][0]["name"] = json!("")),
        ),
        (
            "bad keyword ruleRef",
            Box::new(|v| v["keywordActions"][0]["ruleRef"] = json!("701.x")),
        ),
        (
            "empty counter kind",
            Box::new(|v| v["counterKinds"][0]["kind"] = json!("")),
        ),
        (
            "bad counter class",
            Box::new(|v| v["counterKinds"][0]["class"] = json!("keyword")),
        ),
        (
            "duplicate counter kind",
            Box::new(|v| {
                let c = v["counterKinds"][0].clone();
                v["counterKinds"] = json!([c.clone(), c]);
            }),
        ),
        (
            "bad counter ruleRef",
            Box::new(|v| v["counterKinds"][0]["ruleRef"] = json!("122.1abc")),
        ),
        (
            "unknown color",
            Box::new(move |v| face0(v, "colors", json!(["purple"]))),
        ),
        (
            "colors out of order",
            Box::new(move |v| face0(v, "colors", json!(["red", "blue"]))),
        ),
        (
            "colors repeated",
            Box::new(move |v| face0(v, "colors", json!(["red", "red"]))),
        ),
        (
            "unknown face supertype",
            Box::new(move |v| face0(v, "supertypes", json!(["Humble"]))),
        ),
        (
            "unknown face card type",
            Box::new(move |v| face0(v, "cardTypes", json!(["Delta"]))),
        ),
        (
            "face without card types",
            Box::new(move |v| face0(v, "cardTypes", json!([]))),
        ),
        (
            "face subtype of another card type",
            Box::new(move |v| face0(v, "subtypes", json!(["Kappa"]))),
        ),
        (
            "power without toughness",
            Box::new(move |v| face0(v, "power", json!(1))),
        ),
        (
            "empty ability",
            Box::new(move |v| face0(v, "abilities", json!([""]))),
        ),
        (
            "bad date shape",
            Box::new(|v| v["crEffectiveDate"] = json!("2031-3-04")),
        ),
        (
            "bad date month",
            Box::new(|v| v["crEffectiveDate"] = json!("2031-13-04")),
        ),
        (
            "bad date day",
            Box::new(|v| v["crEffectiveDate"] = json!("2031-02-29")),
        ),
        (
            "bad date century",
            Box::new(|v| v["crEffectiveDate"] = json!("2100-02-29")),
        ),
        (
            "bad date digits",
            Box::new(|v| v["crEffectiveDate"] = json!("20x1-02-01")),
        ),
        (
            "bad date zero day",
            Box::new(|v| v["crEffectiveDate"] = json!("2031-02-00")),
        ),
    ];
    for (what, mutate) in cases {
        let mut v = sample_value();
        mutate(&mut v);
        match load(&v) {
            Err(VocabError::Invalid { path, reason }) => {
                assert!(!path.is_empty() && !reason.is_empty(), "{what}");
            }
            other => panic!("{what}: expected Invalid, got {other:?}"),
        }
    }
}

/// REQ-OBJ-001: accepted edge values (leap days, all colors, a face supertype).
#[test]
fn req_obj_001_loader_accepts_edges() {
    let mut v = sample_value();
    v["crEffectiveDate"] = json!("2032-02-29");
    v["predefinedTokens"][0]["faces"][0]["colors"] =
        json!(["white", "blue", "black", "red", "green"]);
    v["predefinedTokens"][0]["faces"][0]["supertypes"] = json!(["Lofty"]);
    assert!(load(&v).is_ok());
    let mut v = sample_value();
    v["crEffectiveDate"] = json!("2000-02-29");
    assert!(load(&v).is_ok());
}

/// REQ-OBJ-001: validate() catches what a hand-built value can break, and an out-of-range
/// integer cannot be serialised silently.
#[test]
fn req_obj_001_validate_and_serialise_errors() {
    let mut v = sample();
    v.vocabulary_version = 9;
    assert!(matches!(v.validate(), Err(VocabError::Version(_))));
    let mut v = sample();
    v.predefined_tokens[1].faces[1].power = Some(i64::MAX);
    assert!(matches!(v.validate(), Err(VocabError::Invalid { .. })));
    assert!(matches!(v.to_canonical_bytes(), Err(VocabError::Json(_))));
}

/// REQ-OBJ-001: error messages name the problem.
#[test]
fn req_obj_001_error_display() {
    let cases = [
        VocabError::Json(CanonicalError::Syntax(s("x"))),
        VocabError::Version(s("2")),
        VocabError::Shape(s("unknown field")),
        VocabError::Invalid {
            path: s("cardTypes[0]"),
            reason: s("must not be empty"),
        },
    ];
    let texts: Vec<String> = cases.iter().map(ToString::to_string).collect();
    assert!(texts[0].contains("JSON"));
    assert!(texts[1].contains("vocabularyVersion 2"));
    assert!(texts[2].contains("unknown field"));
    assert!(texts[3].contains("cardTypes[0]") && texts[3].contains("must not be empty"));
    let e: VocabError = CanonicalError::Syntax(s("y")).into();
    assert_eq!(e, VocabError::Json(CanonicalError::Syntax(s("y"))));
}

/// REQ-DSL-014, REQ-OBJ-001: identifier and rule-reference shapes.
#[test]
fn req_dsl_014_identifier_shapes() {
    for ok in ["FLY", "A_B", "X2", "A1_B2_C3"] {
        assert!(is_upper_snake(ok), "{ok}");
    }
    for bad in ["", "_A", "A_", "A__B", "a", "A-B", "Ä"] {
        assert!(!is_upper_snake(bad), "{bad}");
    }
    for ok in ["111.10a", "702.9", "205.3g", "702.188ab"] {
        assert!(is_rule_ref(ok), "{ok}");
    }
    for bad in [
        "702", "702.", "70.9", "702.9abc", "702.9A", "a02.9", "702-9",
    ] {
        assert!(!is_rule_ref(bad), "{bad}");
    }
    assert_eq!(days_in_month(2031, 0), 0);
    assert_eq!(days_in_month(2031, 4), 30);
}

/// REQ-SUB-001, REQ-OBJ-001, REQ-DSL-014: the bundled vocabulary loads and equals its file.
#[test]
fn req_sub_001_bundled_round_trip() {
    let v = RulesVocabulary::bundled().unwrap();
    let again = RulesVocabulary::bundled().unwrap();
    assert!(std::ptr::eq(v, again));
    assert_eq!(&RulesVocabulary::from_slice(BUNDLED_JSON).unwrap(), v);
    assert_eq!(v.to_canonical_bytes().unwrap(), BUNDLED_JSON);
    assert_eq!(RulesVocabulary::bundled_bytes(), BUNDLED_JSON);
}

/// Deterministic test PRNG (splitmix64; fixed seed, no OS randomness).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
    fn byte(&mut self) -> u8 {
        self.next().to_le_bytes()[0]
    }
}

/// REQ-OBJ-001 (R12): 2,000 random byte strings never panic the loader.
#[test]
fn req_obj_001_random_bytes_never_panic() {
    let mut rng = Rng(0x0015_0f14);
    for _ in 0..2_000 {
        let len = rng.below(256);
        let bytes: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
        let _ = RulesVocabulary::from_slice(&bytes);
    }
}

/// REQ-OBJ-001, REQ-SUB-001 (R12): every mutation of the bundled JSON either loads to a
/// vocabulary that re-serialises to canonical bytes or is an error (2,000 mutations).
#[test]
fn req_obj_001_mutations_load_or_error() {
    let base = BUNDLED_JSON.to_vec();
    let mut rng = Rng(0x00f0_0014);
    let alphabet = b"{}[]\":,0123456789-aZ \\n";
    let mut loaded = 0;
    for _ in 0..2_000 {
        let mut bytes = base.clone();
        for _ in 0..=rng.below(3) {
            let i = rng.below(bytes.len());
            match rng.below(3) {
                0 => bytes[i] = alphabet[rng.below(alphabet.len())],
                1 => {
                    bytes.remove(i);
                }
                _ => bytes.insert(i, alphabet[rng.below(alphabet.len())]),
            }
        }
        if let Ok(v) = RulesVocabulary::from_slice(&bytes) {
            loaded += 1;
            let out = v.to_canonical_bytes().unwrap();
            let json = &out[..out.len() - 1];
            assert_eq!(canonical::canonicalize(json).unwrap(), json);
            assert_eq!(RulesVocabulary::from_slice(&out).unwrap(), v);
        }
    }
    assert!(loaded > 0, "some mutations (inside names) must still load");
}
