// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for card definitions (REQ-DSL-001, REQ-DSL-002, REQ-OBJ-001, REQ-NFR-006).
//! All cards are synthetic (R5).

use std::collections::BTreeMap;

use fineprint_base::prng::Pcg64Dxsm;
use serde_json::{Value, json};

use super::*;
use crate::canonical::{MAX_SAFE_INTEGER, canonicalize};
use crate::mana::{ManaCostDSL, ManaSymbol};

/// `v` decoded as a real value expression (panics on invalid test content).
fn vexpr(v: Value) -> ValueExpressionAST {
    serde_json::from_value(v).expect("valid value expression")
}

/// `items` decoded as a real token stream (panics on invalid test content).
fn text_of(items: Vec<Value>) -> TextTokenStream {
    serde_json::from_value(Value::Array(items)).expect("valid token stream")
}

/// `v` decoded as a real ability (panics on invalid test content).
fn ability_of(v: Value) -> AbilityAST {
    serde_json::from_value(v).expect("valid ability")
}

/// `v` decoded as a partner ability (panics on invalid test content).
fn partner_of(v: Value) -> PartnerAbilityDSL {
    serde_json::from_value(v).expect("valid partner ability")
}

/// A synthetic face (R5).
fn face(name: &str, cost: Option<&str>) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: FaceKind::Main,
        name: name.to_owned(),
        mana_cost: cost.map(|c| ManaCostDSL::parse(c).expect("cost")),
        color_indicator: None,
        supertypes: Vec::new(),
        card_types: vec!["Creature".to_owned()],
        subtypes: vec!["Bird".to_owned()],
        power: Some(ValueExpressionAST::Literal(2)),
        toughness: Some(ValueExpressionAST::Literal(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: text_of(vec![
            json!({"id": "t1", "kind": "LITERAL", "text": "Flying"}),
        ]),
        abilities: vec![ability_of(
            json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "FLYING"}),
        )],
    }
}

/// A synthetic unsealed definition (R5).
fn definition() -> CardDefinitionDSL {
    let mut legalities = BTreeMap::new();
    legalities.insert("test_format".to_owned(), Legality::Legal);
    legalities.insert("other_format".to_owned(), Legality::NotLegal);
    CardDefinitionDSL {
        card_def_id: "test.flier_2_2".to_owned(),
        dsl_version: DSL_VERSION.to_owned(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "0".repeat(64),
            origin: Origin::Parser,
            parser_version: Some("0.1.0".to_owned()),
            override_source: None,
            verified_by: None,
            rules_baseline: "test-baseline".to_owned(),
        },
        support_status: SupportStatus::AutoParsed,
        known_gaps: None,
        layout: LayoutBehavior::Normal,
        faces: vec![face("test.flier_2_2", Some("{1}{W}"))],
        meld_partner: None,
        color_identity: ColorBitmask::WHITE,
        commander_eligibility: None,
        legalities,
        digital_only: false,
        related_tokens: None,
    }
}

fn sealed() -> CardDefinitionDSL {
    definition().seal().expect("seal")
}

fn json_bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).expect("ser")
}

/// A valid partner ability: a synthetic KEYWORD ability with id `id` (R5).
fn partner(id: &str) -> PartnerAbilityDSL {
    partner_of(json!({"abilityId": id, "kind": "KEYWORD", "keyword": "PARTNER"}))
}

/// A valid value expression of `levels` nested `MATH` nodes around `leaf`, built iteratively
/// by moving (`json!` would serialise the inner value recursively). Each `MATH` node opens two
/// JSON objects.
fn math_chain(levels: usize, leaf: Value) -> Value {
    let mut v = leaf;
    for _ in 0..levels {
        let mut body = serde_json::Map::new();
        body.insert("op".to_owned(), json!("ADD"));
        body.insert("lhs".to_owned(), v);
        body.insert("rhs".to_owned(), json!("X"));
        let mut node = serde_json::Map::new();
        node.insert("MATH".to_owned(), Value::Object(body));
        v = Value::Object(node);
    }
    v
}

/// The typed form of [`math_chain`]: `levels` nested `ADD` nodes around `leaf`, built
/// iteratively, with `X` as each right operand.
fn typed_math_chain(levels: usize, leaf: ValueExpressionAST) -> ValueExpressionAST {
    let mut v = leaf;
    for _ in 0..levels {
        v = ValueExpressionAST::Math {
            op: crate::expr::MathOp::Add,
            lhs: Box::new(v),
            rhs: Box::new(ValueExpressionAST::X),
        };
    }
    v
}

/// Generic enum check: ALL / as_str / Display agree with serde and every value round-trips.
fn check_enum<T>(all: &[T], as_str: fn(T) -> &'static str)
where
    T: Copy
        + PartialEq
        + std::fmt::Debug
        + std::fmt::Display
        + Serialize
        + for<'de> Deserialize<'de>,
{
    let mut seen = std::collections::BTreeSet::new();
    for &v in all {
        let json = serde_json::to_value(v).expect("ser");
        assert_eq!(json, Value::String(as_str(v).to_owned()));
        assert_eq!(v.to_string(), as_str(v));
        assert_eq!(serde_json::from_value::<T>(json).expect("de"), v);
        assert!(seen.insert(as_str(v)), "duplicate string {}", as_str(v));
    }
    assert!(serde_json::from_value::<T>(json!("NOT_A_VARIANT")).is_err());
    assert!(serde_json::from_value::<T>(json!(as_str(all[0]).to_lowercase())).is_err());
}

/// REQ-DSL-002 (SRS §4.2, §14.2): all enum variants serialise as the SRS strings.
#[test]
fn req_dsl_002_enums_round_trip() {
    check_enum(&LayoutBehavior::ALL, LayoutBehavior::as_str);
    check_enum(&FaceKind::ALL, FaceKind::as_str);
    check_enum(&SupportStatus::ALL, SupportStatus::as_str);
    check_enum(&Origin::ALL, Origin::as_str);
    check_enum(&Legality::ALL, Legality::as_str);
    let layouts: Vec<&str> = LayoutBehavior::ALL.iter().map(|l| l.as_str()).collect();
    assert_eq!(
        layouts.join(" "),
        "NORMAL SPLIT ROOM FLIP LEVELER DFC_NONMODAL DFC_MODAL MELD SAGA ADVENTURER CLASS \
         ATTRACTION PROTOTYPE CASE OMEN STATION PREPARATION MUTATE TOKEN DFC_TOKEN PLANAR SCHEME \
         VANGUARD CONSPIRACY DUNGEON"
    );
    let kinds: Vec<&str> = FaceKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        kinds.join(" "),
        "MAIN BACK LEFT_HALF RIGHT_HALF ADVENTURE OMEN PREPARE_SPELL PROTOTYPE FLIPPED MELD_RESULT"
    );
    assert_eq!(
        SupportStatus::ALL.map(SupportStatus::as_str),
        ["VERIFIED", "AUTO_PARSED", "PARTIAL", "UNSUPPORTED"]
    );
    assert_eq!(
        Origin::ALL.map(Origin::as_str),
        ["PARSER", "CLIENT_OVERRIDE", "MIXED"]
    );
    assert_eq!(
        Legality::ALL.map(Legality::as_str),
        ["LEGAL", "BANNED", "RESTRICTED", "NOT_LEGAL"]
    );
}

/// REQ-DSL-002: ColorBitmask is an integer 0..=31; 32, -1 and non-integers are rejected.
#[test]
fn req_dsl_002_color_bitmask() {
    for bits in 0u8..=31 {
        let mask = ColorBitmask::new(bits).expect("valid");
        assert_eq!(mask.bits(), bits);
        let json = serde_json::to_value(mask).expect("ser");
        assert_eq!(json, json!(bits));
        assert_eq!(
            serde_json::from_value::<ColorBitmask>(json).expect("de"),
            mask
        );
    }
    for bits in 32u8..=255 {
        assert_eq!(ColorBitmask::new(bits), None);
    }
    for bad in [
        json!(32),
        json!(-1),
        json!(256),
        json!(1.5),
        json!("1"),
        json!(null),
    ] {
        assert!(
            serde_json::from_value::<ColorBitmask>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    assert_eq!(ColorBitmask::NONE.bits(), 0);
    let all = [
        ColorBitmask::WHITE,
        ColorBitmask::BLUE,
        ColorBitmask::BLACK,
        ColorBitmask::RED,
        ColorBitmask::GREEN,
    ];
    assert_eq!(all.map(ColorBitmask::bits), [1, 2, 4, 8, 16]);
}

/// REQ-DSL-001: dslVersion parsing.
#[test]
fn req_dsl_001_version_parse() {
    assert_eq!(
        DslVersion::parse("1.0"),
        Ok(DslVersion {
            major: 1,
            minor: 0,
            patch: 0
        })
    );
    assert_eq!(
        DslVersion::parse("1.7.3"),
        Ok(DslVersion {
            major: 1,
            minor: 7,
            patch: 3
        })
    );
    assert_eq!(
        DslVersion::parse("0.0.0"),
        Ok(DslVersion {
            major: 0,
            minor: 0,
            patch: 0
        })
    );
    assert_eq!(
        DslVersion::parse("18446744073709551615.10.0").map(|v| v.major),
        Ok(u64::MAX)
    );
    assert_eq!(DslVersion::parse("1.7.3").expect("ok").to_string(), "1.7.3");
    assert_eq!(DslVersion::parse("2.0").expect("ok").to_string(), "2.0.0");
    for bad in [
        "",
        "1",
        "1.",
        ".1",
        "01.0",
        "1.00",
        "1.0.01",
        "v1.0",
        "1.0-beta",
        "1.0+build",
        "+1.0",
        "-1.0",
        " 1.0",
        "1.0 ",
        "1.0.0.0",
        "1..0",
        "a.b",
        "18446744073709551616.0",
        "１.0",
    ] {
        assert_eq!(
            DslVersion::parse(bad),
            Err(DslVersionError::Malformed(bad.to_owned())),
            "{bad}"
        );
    }
    assert!(DslVersion::parse("1.0").expect("a") < DslVersion::parse("1.0.1").expect("b"));
}

/// REQ-DSL-001 (SRS §14.1 item 5): a different major version is rejected with a clear error.
#[test]
fn req_dsl_001_version_check() {
    assert_eq!(DSL_VERSION, "1.0");
    assert_eq!(DSL_MAJOR, 1);
    assert_eq!(
        check_dsl_version(DSL_VERSION).map(|v| v.major),
        Ok(DSL_MAJOR)
    );
    assert!(check_dsl_version("1.7.3").is_ok());
    assert!(check_dsl_version("1.999").is_ok());
    let err = check_dsl_version("2.0").expect_err("major 2");
    assert_eq!(
        err,
        DslVersionError::MajorMismatch {
            found: 2,
            supported: 1
        }
    );
    let text = err.to_string();
    assert!(
        text.contains("dslVersion") && text.contains('2') && text.contains('1'),
        "{text}"
    );
    assert_eq!(
        check_dsl_version("0.9"),
        Err(DslVersionError::MajorMismatch {
            found: 0,
            supported: 1
        })
    );
    assert!(matches!(
        check_dsl_version("1"),
        Err(DslVersionError::Malformed(_))
    ));
    assert!(
        DslVersionError::Malformed("x".into())
            .to_string()
            .contains("\"x\"")
    );
}

/// REQ-DSL-002: JSON member names are exactly the SRS names; optional members are omitted.
#[test]
fn req_dsl_002_json_member_names() {
    let v = serde_json::to_value(sealed()).expect("ser");
    let keys: Vec<&str> = v
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "cardDefId",
            "colorIdentity",
            "definitionHash",
            "digitalOnly",
            "dslVersion",
            "faces",
            "layout",
            "legalities",
            "source",
            "supportStatus"
        ]
    );
    let face_keys: Vec<&str> = v["faces"][0]
        .as_object()
        .expect("face")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        face_keys,
        [
            "abilities",
            "cardTypes",
            "faceKind",
            "manaCost",
            "name",
            "power",
            "subtypes",
            "supertypes",
            "text",
            "toughness"
        ]
    );
    assert_eq!(v["source"]["parserVersion"], json!("0.1.0"));
    assert!(v["source"].get("overrideSource").is_none());

    let mut full = definition();
    full.known_gaps = Some(vec!["gap".into()]);
    full.meld_partner = Some("test.partner".into());
    full.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: true,
        partner_abilities: vec![partner("p1")],
    });
    full.related_tokens = Some(vec!["test.token_1_1".into()]);
    full.source.override_source = Some("test.client".into());
    full.source.verified_by = Some(vec!["a".repeat(64)]);
    full.faces[0].color_indicator = Some(ColorBitmask::RED);
    full.faces[0].loyalty = Some(3);
    full.faces[0].defense = Some(4);
    full.faces[0].hand_modifier = Some(-1);
    full.faces[0].life_modifier = Some(5);
    let v = serde_json::to_value(&full).expect("ser");
    for key in [
        "knownGaps",
        "meldPartner",
        "commanderEligibility",
        "relatedTokens",
    ] {
        assert!(v.get(key).is_some(), "{key}");
    }
    assert!(v["commanderEligibility"].get("partnerAbilities").is_some());
    for key in [
        "overrideSource",
        "verifiedBy",
        "rulesBaseline",
        "oracleTextHash",
    ] {
        assert!(v["source"].get(key).is_some(), "{key}");
    }
    for key in [
        "colorIndicator",
        "loyalty",
        "defense",
        "handModifier",
        "lifeModifier",
    ] {
        assert!(v["faces"][0].get(key).is_some(), "{key}");
    }
    assert_eq!(
        serde_json::from_value::<CardDefinitionDSL>(v).expect("de"),
        full
    );
}

/// REQ-OBJ-001, REQ-DSL-002: a face without mana cost omits `manaCost`; `{0}` is present and
/// gives different canonical JSON and a different definitionHash.
#[test]
fn req_obj_001_no_mana_cost_vs_zero_in_definition() {
    let mut none = definition();
    none.faces[0].mana_cost = None;
    let mut zero = definition();
    zero.faces[0].mana_cost = Some(ManaCostDSL::new(vec![ManaSymbol::Generic(0)]).expect("ok"));
    let none = none.seal().expect("seal");
    let zero = zero.seal().expect("seal");
    let none_json = String::from_utf8(none.to_canonical_json().expect("json")).expect("utf8");
    let zero_json = String::from_utf8(zero.to_canonical_json().expect("json")).expect("utf8");
    assert!(!none_json.contains("manaCost"), "{none_json}");
    assert!(zero_json.contains(r#""manaCost":"{0}""#), "{zero_json}");
    assert_ne!(none.definition_hash, zero.definition_hash);
    assert_eq!(CardDefinitionDSL::from_json(none_json.as_bytes()), Ok(none));
    assert_eq!(CardDefinitionDSL::from_json(zero_json.as_bytes()), Ok(zero));
}

/// REQ-DSL-002, REQ-NFR-006: seal / compute_hash / verify_hash.
#[test]
fn req_dsl_002_seal_and_verify() {
    let def = sealed();
    assert_eq!(def.definition_hash.len(), 64);
    assert!(
        def.definition_hash
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    );
    assert_eq!(def.verify_hash(), Ok(()));

    // compute_hash ignores the current definitionHash value.
    let mut other = def.clone();
    other.definition_hash = "anything".into();
    assert_eq!(other.compute_hash(), def.compute_hash());
    assert_eq!(other.clone().seal().expect("seal"), def);
    assert_eq!(
        other.verify_hash(),
        Err(CardDefinitionError::HashMismatch {
            declared: "anything".into(),
            computed: def.definition_hash.clone()
        })
    );

    // The hash is SHA-256 of the canonical JSON without the definitionHash member.
    let mut v = serde_json::to_value(&def).expect("ser");
    v.as_object_mut().expect("obj").remove("definitionHash");
    let bytes = canonicalize(&json_bytes(&v)).expect("jcs");
    assert_eq!(
        fineprint_base::hash::sha256(&bytes).to_hex(),
        def.definition_hash
    );

    // Uppercase hex is a mismatch.
    let mut upper = def.clone();
    upper.definition_hash = def.definition_hash.to_uppercase();
    assert!(matches!(
        upper.verify_hash(),
        Err(CardDefinitionError::HashMismatch { .. })
    ));

    // Any content change changes the hash.
    let mut changed = def.clone();
    changed.digital_only = true;
    assert_ne!(changed.compute_hash().expect("hash"), def.definition_hash);
}

/// REQ-DSL-001, REQ-NFR-006: from_json accepts the canonical form and any reordering or
/// whitespace, with the same value and hash.
#[test]
fn req_dsl_001_from_json_reorder_and_whitespace_invariant() {
    let def = sealed();
    let canonical = def.to_canonical_json().expect("json");
    assert_eq!(canonicalize(&canonical).expect("stable"), canonical);
    assert_eq!(CardDefinitionDSL::from_json(&canonical), Ok(def.clone()));
    let pretty = serde_json::to_vec_pretty(&def).expect("pretty");
    assert_eq!(CardDefinitionDSL::from_json(&pretty), Ok(def.clone()));
    // serde field order (declaration order) differs from canonical order.
    let declared = serde_json::to_vec(&def).expect("ser");
    assert_ne!(declared, canonical);
    let parsed = CardDefinitionDSL::from_json(&declared).expect("ok");
    assert_eq!(parsed.compute_hash().expect("hash"), def.definition_hash);
    // Reversed member order with extra whitespace.
    let v = serde_json::to_value(&def).expect("ser");
    let members: Vec<String> = v
        .as_object()
        .expect("obj")
        .iter()
        .rev()
        .map(|(k, val)| format!("\n  {} :\t{}", serde_json::to_string(k).expect("k"), val))
        .collect();
    let reversed = format!("{{{} }}", members.join(" ,"));
    assert_eq!(CardDefinitionDSL::from_json(reversed.as_bytes()), Ok(def));
}

/// REQ-DSL-001, REQ-NFR-006: from_json rejections in check order.
#[test]
fn req_dsl_001_from_json_rejections() {
    let def = sealed();
    let good = serde_json::to_value(&def).expect("ser");
    let text = serde_json::to_string(&good).expect("ser");

    // (a) canonical-level: invalid JSON, duplicate key, unsafe integer.
    assert!(matches!(
        CardDefinitionDSL::from_json(b"{"),
        Err(CardDefinitionError::Canonical(CanonicalError::Syntax(_)))
    ));
    let dup = text.replacen("{", "{\"layout\":\"NORMAL\",", 1);
    assert_eq!(
        CardDefinitionDSL::from_json(dup.as_bytes()),
        Err(CardDefinitionError::Canonical(
            CanonicalError::DuplicateKey("layout".into())
        ))
    );
    let nested_dup = text.replacen("\"origin\":", "\"origin\":\"MIXED\",\"origin\":", 1);
    assert!(matches!(
        CardDefinitionDSL::from_json(nested_dup.as_bytes()),
        Err(CardDefinitionError::Canonical(
            CanonicalError::DuplicateKey(_)
        ))
    ));

    // (b) top level must be an object with a string dslVersion.
    for bad in [&b"[]"[..], b"\"1.0\"", b"{}", br#"{"dslVersion":1}"#] {
        assert!(
            matches!(
                CardDefinitionDSL::from_json(bad),
                Err(CardDefinitionError::Schema(_))
            ),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
    assert_eq!(
        CardDefinitionDSL::from_json(br#"{"dslVersion":"one"}"#),
        Err(CardDefinitionError::Version(DslVersionError::Malformed(
            "one".into()
        )))
    );
    // Wrong major, even with an otherwise unknown shape.
    let future = br#"{"dslVersion":"2.0","somethingNew":[1,2,3]}"#;
    let err = CardDefinitionDSL::from_json(future).expect_err("major 2");
    assert_eq!(
        err,
        CardDefinitionError::Version(DslVersionError::MajorMismatch {
            found: 2,
            supported: 1
        })
    );
    assert!(err.to_string().contains("dslVersion"), "{err}");
    let mut major2 = good.clone();
    major2["dslVersion"] = json!("2.1");
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&major2)),
        Err(CardDefinitionError::Version(
            DslVersionError::MajorMismatch { found: 2, .. }
        ))
    ));

    // (c) schema: unknown member, missing required member, wrong type, bad mana cost.
    let mut unknown = good.clone();
    unknown["extra"] = json!(true);
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&unknown)),
        Err(CardDefinitionError::Schema(_))
    ));
    let mut unknown_face = good.clone();
    unknown_face["faces"][0]["extra"] = json!(1);
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&unknown_face)),
        Err(CardDefinitionError::Schema(_))
    ));
    for required in [
        "cardDefId",
        "definitionHash",
        "source",
        "faces",
        "legalities",
        "digitalOnly",
    ] {
        let mut missing = good.clone();
        missing.as_object_mut().expect("obj").remove(required);
        assert!(
            matches!(
                CardDefinitionDSL::from_json(&json_bytes(&missing)),
                Err(CardDefinitionError::Schema(_))
            ),
            "{required}"
        );
    }
    let mut missing_face_member = good.clone();
    missing_face_member["faces"][0]
        .as_object_mut()
        .expect("face")
        .remove("text");
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&missing_face_member)),
        Err(CardDefinitionError::Schema(_))
    ));
    for (path, bad) in [
        ("layout", json!("normal")),
        ("colorIdentity", json!(32)),
        ("digitalOnly", json!("no")),
    ] {
        let mut wrong = good.clone();
        wrong[path] = bad;
        assert!(matches!(
            CardDefinitionDSL::from_json(&json_bytes(&wrong)),
            Err(CardDefinitionError::Schema(_))
        ));
    }
    let mut empty_cost = good.clone();
    empty_cost["faces"][0]["manaCost"] = json!("");
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&empty_cost)),
        Err(CardDefinitionError::Schema(_))
    ));

    // (d) hash: tampered content, tampered hash, uppercase hex.
    let mut tampered = good.clone();
    tampered["cardDefId"] = json!("test.other");
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&tampered)),
        Err(CardDefinitionError::HashMismatch { .. })
    ));
    let mut wrong_hash = good.clone();
    wrong_hash["definitionHash"] = json!("f".repeat(64));
    assert_eq!(
        CardDefinitionDSL::from_json(&json_bytes(&wrong_hash)),
        Err(CardDefinitionError::HashMismatch {
            declared: "f".repeat(64),
            computed: def.definition_hash.clone()
        })
    );
    let mut upper = good.clone();
    upper["definitionHash"] = json!(def.definition_hash.to_uppercase());
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&upper)),
        Err(CardDefinitionError::HashMismatch { .. })
    ));
}

/// REQ-NFR-006, REQ-CORE-006: loyalty 2^53 is rejected as an unsafe integer; 2^53 − 1 is fine.
#[test]
fn req_nfr_006_unsafe_integer_in_definition() {
    let mut ok = definition();
    ok.faces[0].loyalty = Some(MAX_SAFE_INTEGER);
    let ok = ok.seal().expect("seal");
    let bytes = ok.to_canonical_json().expect("json");
    assert!(String::from_utf8_lossy(&bytes).contains("\"loyalty\":9007199254740991"));
    assert_eq!(CardDefinitionDSL::from_json(&bytes), Ok(ok.clone()));

    let mut too_big = ok.clone();
    too_big.faces[0].loyalty = Some(MAX_SAFE_INTEGER + 1);
    assert!(matches!(
        too_big.to_canonical_json(),
        Err(CanonicalError::UnsafeInteger(_))
    ));
    assert!(matches!(
        too_big.compute_hash(),
        Err(CanonicalError::UnsafeInteger(_))
    ));
    assert!(matches!(
        too_big.clone().seal(),
        Err(CanonicalError::UnsafeInteger(_))
    ));
    assert!(matches!(
        too_big.verify_hash(),
        Err(CardDefinitionError::Canonical(
            CanonicalError::UnsafeInteger(_)
        ))
    ));
    let text = String::from_utf8(bytes)
        .expect("utf8")
        .replace("9007199254740991", "9007199254740992");
    assert!(matches!(
        CardDefinitionDSL::from_json(text.as_bytes()),
        Err(CardDefinitionError::Canonical(
            CanonicalError::UnsafeInteger(_)
        ))
    ));
}

/// REQ-DSL-002: error Display strings name the problem.
#[test]
fn req_dsl_002_error_display() {
    let cases = [
        (
            CardDefinitionError::Canonical(CanonicalError::DuplicateKey("k".into())),
            "duplicate",
        ),
        (
            CardDefinitionError::Version(DslVersionError::MajorMismatch {
                found: 3,
                supported: 1,
            }),
            "dslVersion major 3",
        ),
        (
            CardDefinitionError::Schema("missing field".into()),
            "missing field",
        ),
        (
            CardDefinitionError::HashMismatch {
                declared: "aa".into(),
                computed: "bb".into(),
            },
            "definitionHash mismatch",
        ),
    ];
    for (e, needle) in cases {
        assert!(e.to_string().contains(needle), "{e}");
        let as_error: &dyn std::error::Error = &e;
        let _ = as_error.source();
    }
}

/// Builds a random synthetic definition (R5) from the PRNG.
fn random_definition(rng: &mut Pcg64Dxsm) -> CardDefinitionDSL {
    let mut pick = |n: u64| (rng.next_u64() % n) as usize;
    let mut def = definition();
    def.card_def_id = format!("test.random_{}", pick(1_000_000));
    def.layout = LayoutBehavior::ALL[pick(25)];
    def.support_status = SupportStatus::ALL[pick(4)];
    def.source.origin = Origin::ALL[pick(3)];
    def.color_identity = ColorBitmask::new(pick(32) as u8).expect("mask");
    def.digital_only = pick(2) == 1;
    def.known_gaps = (pick(2) == 1).then(|| vec!["gap \u{1F602} \"quoted\"\n".to_owned()]);
    def.meld_partner = (pick(2) == 1).then(|| "test.partner".to_owned());
    def.related_tokens = (pick(2) == 1).then(Vec::new);
    def.commander_eligibility = (pick(2) == 1).then(|| CommanderEligibilityDSL {
        eligible: pick(2) == 1,
        partner_abilities: vec![partner(&format!("p{}", pick(5)))],
    });
    def.legalities
        .insert(format!("fmt_{}", pick(9)), Legality::ALL[pick(4)]);
    def.legalities
        .insert("\u{FB33}".to_owned(), Legality::Banned);
    def.legalities
        .insert("\u{1F602}".to_owned(), Legality::Restricted);
    let faces = 1 + pick(3);
    def.faces = (0..faces)
        .map(|i| {
            let mut f = face(&format!("test.face_{i}"), None);
            f.face_kind = FaceKind::ALL[pick(10)];
            f.mana_cost = match pick(3) {
                0 => None,
                1 => Some(ManaCostDSL::new(vec![ManaSymbol::Generic(0)]).expect("zero")),
                _ => Some(ManaCostDSL::parse("{X}{2/W}{G/U/P}{C}").expect("cost")),
            };
            f.color_indicator = ColorBitmask::new(pick(40) as u8);
            f.loyalty = (pick(2) == 1).then(|| pick(10) as i64 - 5);
            f.defense = (pick(2) == 1).then_some(MAX_SAFE_INTEGER);
            f.hand_modifier = (pick(2) == 1).then_some(-MAX_SAFE_INTEGER);
            f.life_modifier = (pick(2) == 1).then_some(0);
            f.power = match pick(3) {
                0 => None,
                1 => Some(ValueExpressionAST::X),
                _ => Some(ValueExpressionAST::Literal(pick(9) as i64)),
            };
            f.toughness = (pick(2) == 1)
                .then(|| vexpr(json!({"MATH": {"op": "ADD", "lhs": {"LITERAL": 1}, "rhs": "X"}})));
            f
        })
        .collect();
    def
}

/// REQ-DSL-001, REQ-DSL-002, REQ-NFR-006 property: value → JSON → value round trip, canonical
/// stability, hash independence from the current definitionHash, and from_json of the canonical
/// bytes returns the sealed value.
#[test]
fn req_dsl_001_property_round_trip_and_stability() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0dd1_0000_0001);
    for _ in 0..300 {
        let def = random_definition(&mut rng);
        let json = serde_json::to_value(&def).expect("ser");
        assert_eq!(
            serde_json::from_value::<CardDefinitionDSL>(json).expect("de"),
            def
        );

        let sealed = def.clone().seal().expect("seal");
        assert_eq!(sealed.verify_hash(), Ok(()));
        assert_eq!(def.compute_hash(), sealed.compute_hash());
        let canonical = sealed.to_canonical_json().expect("canonical");
        assert_eq!(canonicalize(&canonical).expect("stable"), canonical);
        assert_eq!(
            CardDefinitionDSL::from_json(&canonical).as_ref(),
            Ok(&sealed)
        );
        let pretty = serde_json::to_vec_pretty(&sealed).expect("pretty");
        assert_eq!(CardDefinitionDSL::from_json(&pretty).as_ref(), Ok(&sealed));
    }
}

/// REQ-DSL-001, R12: bounded random bytes and mutated definitions never panic in from_json, and
/// DslVersion::parse never panics on random strings.
#[test]
fn req_dsl_001_random_bytes_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0dd1_0000_0002);
    let base = sealed().to_canonical_json().expect("json");
    for i in 0..2000 {
        // Random bytes.
        let len = (rng.next_u64() % 48) as usize;
        let raw: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
        assert!(CardDefinitionDSL::from_json(&raw).is_err() || raw.is_empty());
        let _ = DslVersion::parse(&String::from_utf8_lossy(&raw));
        // Mutated valid definition: flip, delete or insert one byte.
        let mut mutated = base.clone();
        let at = (rng.next_u64() % mutated.len() as u64) as usize;
        match i % 3 {
            0 => mutated[at] ^= 1 << (rng.next_u64() % 8),
            1 => {
                mutated.remove(at);
            }
            _ => mutated.insert(at, b"{}[]\":,0x"[(rng.next_u64() % 9) as usize]),
        }
        let _ = CardDefinitionDSL::from_json(&mutated);
        // Random version-like strings.
        let digits: String = (0..(rng.next_u64() % 8))
            .map(|_| b"0123456789.-+v "[(rng.next_u64() % 15) as usize] as char)
            .collect();
        if let Ok(v) = DslVersion::parse(&digits) {
            assert!(DslVersion::parse(&v.to_string()).is_ok());
        }
    }
}

// ---- Review round 1: strict enums, no explicit nulls, lossless loader, depth, normalising ----

/// Generic strictness check: only the declared strings decode, from a Value and from bytes.
fn check_enum_strict<T>(all: &[T], as_str: fn(T) -> &'static str)
where
    T: Copy + PartialEq + fmt::Debug + serde::de::DeserializeOwned,
{
    for &v in all {
        let text = as_str(v);
        assert_eq!(serde_json::from_value::<T>(json!(text)).expect("value"), v);
        let quoted = serde_json::to_string(text).expect("quote");
        assert_eq!(serde_json::from_str::<T>(&quoted).expect("str"), v);
        let mut singleton = serde_json::Map::new();
        singleton.insert(text.to_owned(), Value::Null);
        for bad in [
            Value::Object(singleton),
            json!([text]),
            json!(1),
            Value::Null,
            json!(text.to_lowercase()),
            json!(format!("{text} ")),
        ] {
            assert!(serde_json::from_value::<T>(bad.clone()).is_err(), "{bad}");
            assert!(
                serde_json::from_slice::<T>(&json_bytes(&bad)).is_err(),
                "{bad}"
            );
        }
    }
}

/// REQ-DSL-001, REQ-DSL-002: the string enums decode only from their declared strings, never
/// from a singleton object, an array, a number, null or a near-miss string.
#[test]
fn req_dsl_001_string_enums_strict() {
    check_enum_strict(&LayoutBehavior::ALL, LayoutBehavior::as_str);
    check_enum_strict(&FaceKind::ALL, FaceKind::as_str);
    check_enum_strict(&SupportStatus::ALL, SupportStatus::as_str);
    check_enum_strict(&Origin::ALL, Origin::as_str);
    check_enum_strict(&Legality::ALL, Legality::as_str);
    let err = serde_json::from_value::<Origin>(json!("OTHER")).expect_err("unknown");
    assert!(err.to_string().contains("CLIENT_OVERRIDE"), "{err}");
    let err = serde_json::from_value::<Origin>(json!(3)).expect_err("number");
    assert!(err.to_string().contains("Origin"), "{err}");
}

/// A definition with every optional member present (synthetic, R5).
fn full_definition() -> CardDefinitionDSL {
    let mut def = definition();
    def.source.override_source = Some("test.author".into());
    def.source.verified_by = Some(vec!["0".repeat(64)]);
    def.known_gaps = Some(vec!["gap".into()]);
    def.meld_partner = Some("test.partner".into());
    def.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: true,
        partner_abilities: vec![partner("p1")],
    });
    def.related_tokens = Some(vec!["test.token".into()]);
    let f = &mut def.faces[0];
    f.color_indicator = Some(ColorBitmask::GREEN);
    f.loyalty = Some(3);
    f.defense = Some(4);
    f.hand_modifier = Some(-1);
    f.life_modifier = Some(5);
    def
}

/// REQ-OBJ-001, REQ-DSL-002: an explicit `null` is not an omitted optional member: each
/// non-nullable optional member given as `null` is an error, each omitted one is `None`.
#[test]
fn req_obj_001_optional_members_reject_null() {
    let full = serde_json::to_value(full_definition()).expect("ser");
    let minimal = serde_json::to_value({
        let mut d = definition();
        d.source.parser_version = None;
        d.faces[0].mana_cost = None;
        d
    })
    .expect("ser");
    let decoded: CardDefinitionDSL = serde_json::from_value(minimal.clone()).expect("minimal");
    assert_eq!(decoded.source.parser_version, None);
    assert_eq!(decoded.faces[0].mana_cost, None);
    assert_eq!(decoded.known_gaps, None);
    assert_eq!(decoded.faces[0].loyalty, None);
    let top = [
        "knownGaps",
        "meldPartner",
        "commanderEligibility",
        "relatedTokens",
    ];
    let source = ["parserVersion", "overrideSource", "verifiedBy"];
    let face = [
        "manaCost",
        "colorIndicator",
        "loyalty",
        "defense",
        "handModifier",
        "lifeModifier",
    ];
    for (member, slot) in top
        .iter()
        .map(|m| (*m, 0))
        .chain(source.iter().map(|m| (*m, 1)))
        .chain(face.iter().map(|m| (*m, 2)))
    {
        for base in [&full, &minimal] {
            let mut v = base.clone();
            let target = match slot {
                0 => &mut v,
                1 => &mut v["source"],
                _ => &mut v["faces"][0],
            };
            target[member] = Value::Null;
            assert!(
                serde_json::from_value::<CardDefinitionDSL>(v).is_err(),
                "{member}: null"
            );
        }
        let mut omitted = full.clone();
        let target = match slot {
            0 => &mut omitted,
            1 => &mut omitted["source"],
            _ => &mut omitted["faces"][0],
        };
        target.as_object_mut().expect("obj").remove(member);
        assert!(
            serde_json::from_value::<CardDefinitionDSL>(omitted).is_ok(),
            "{member}: omitted"
        );
    }
    // `"{0}"` is a cost; `""` is an error.
    let mut zero = minimal.clone();
    zero["faces"][0]["manaCost"] = json!("{0}");
    let zero: CardDefinitionDSL = serde_json::from_value(zero).expect("zero");
    assert_eq!(
        zero.faces[0].mana_cost.as_ref().map(ManaCostDSL::symbols),
        Some(&[ManaSymbol::Generic(0)][..])
    );
    let mut empty = minimal;
    empty["faces"][0]["manaCost"] = json!("");
    assert!(serde_json::from_value::<CardDefinitionDSL>(empty).is_err());
    // power and toughness: the real value-expression decoder refuses `null` too (#232).
    for member in ["power", "toughness"] {
        let mut null_member = full.clone();
        null_member["faces"][0][member] = Value::Null;
        assert!(
            serde_json::from_value::<CardDefinitionDSL>(null_member).is_err(),
            "{member}: null"
        );
    }
}

/// `object` as an array of its members' values in `order` (absent members become null).
fn positional(object: &Value, order: &[&str]) -> Value {
    Value::Array(
        order
            .iter()
            .map(|k| object.get(*k).cloned().unwrap_or(Value::Null))
            .collect(),
    )
}

/// Sets `definitionHash` of `v` to the hash of its own checked content, so only the schema
/// checks can reject it.
fn rehash(v: &mut Value) {
    let mut without = v.clone();
    without
        .as_object_mut()
        .expect("obj")
        .remove("definitionHash");
    let bytes = canonicalize(&json_bytes(&without)).expect("jcs");
    v["definitionHash"] = json!(fineprint_base::hash::sha256(&bytes).to_hex());
}

/// Asserts that `from_json` rejects `v` with `Schema`, both with its current hash and with a
/// hash recomputed over its own content.
fn assert_schema_both_ways(mut v: Value, what: &str) {
    let result = CardDefinitionDSL::from_json(&json_bytes(&v));
    assert!(
        matches!(result, Err(CardDefinitionError::Schema(_))),
        "{what} (original hash): {result:?}"
    );
    rehash(&mut v);
    let result = CardDefinitionDSL::from_json(&json_bytes(&v));
    assert!(
        matches!(result, Err(CardDefinitionError::Schema(_))),
        "{what} (recomputed hash): {result:?}"
    );
}

/// REQ-DSL-001, REQ-DSL-002, REQ-NFR-006: the loader accepts only lossless decodes: null
/// members, positional arrays and singleton-object enums are Schema errors, whatever the hash.
#[test]
fn req_dsl_001_from_json_lossless() {
    let full = full_definition().seal().expect("seal");
    let good = serde_json::to_value(&full).expect("ser");
    assert_eq!(
        CardDefinitionDSL::from_json(&json_bytes(&good)),
        Ok(full.clone())
    );

    // A null manaCost added to a sealed face without a cost does not reuse its digest.
    let mut no_cost = definition();
    no_cost.faces[0].mana_cost = None;
    let no_cost = no_cost.seal().expect("seal");
    let mut with_null = serde_json::to_value(&no_cost).expect("ser");
    with_null["faces"][0]["manaCost"] = Value::Null;
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&with_null)),
        Err(CardDefinitionError::Schema(_))
    ));
    let mut loyalty_null = good.clone();
    loyalty_null["faces"][0]["loyalty"] = Value::Null;
    assert_schema_both_ways(loyalty_null, "loyalty null");

    // Positional arrays where an object is required.
    let mut source = good.clone();
    source["source"] = positional(
        &good["source"],
        &[
            "oracleTextHash",
            "origin",
            "parserVersion",
            "overrideSource",
            "verifiedBy",
            "rulesBaseline",
        ],
    );
    assert!(serde_json::from_value::<CardDefinitionDSL>(source.clone()).is_ok());
    assert_schema_both_ways(source, "positional source");
    let mut face = good.clone();
    face["faces"][0] = positional(
        &good["faces"][0],
        &[
            "faceKind",
            "name",
            "manaCost",
            "colorIndicator",
            "supertypes",
            "cardTypes",
            "subtypes",
            "power",
            "toughness",
            "loyalty",
            "defense",
            "handModifier",
            "lifeModifier",
            "text",
            "abilities",
        ],
    );
    assert!(serde_json::from_value::<CardDefinitionDSL>(face.clone()).is_ok());
    assert_schema_both_ways(face, "positional face");
    let mut commander = good.clone();
    commander["commanderEligibility"] = positional(
        &good["commanderEligibility"],
        &["eligible", "partnerAbilities"],
    );
    assert!(serde_json::from_value::<CardDefinitionDSL>(commander.clone()).is_ok());
    assert_schema_both_ways(commander, "positional commanderEligibility");

    // A singleton-object enum.
    let mut layout = good.clone();
    layout["layout"] = json!({"NORMAL": null});
    assert_schema_both_ways(layout, "singleton layout");

    // Omitted manaCost and "{0}" still load (and keep distinct hashes).
    let zero = {
        let mut d = definition();
        d.faces[0].mana_cost = Some(ManaCostDSL::parse("{0}").expect("zero"));
        d.seal().expect("seal")
    };
    for def in [&no_cost, &zero] {
        let bytes = def.to_canonical_json().expect("json");
        assert_eq!(CardDefinitionDSL::from_json(&bytes).as_ref(), Ok(def));
    }

    // Whitespace and member order still load with the same value and hash.
    let pretty = serde_json::to_vec_pretty(&full).expect("pretty");
    let loaded = CardDefinitionDSL::from_json(&pretty).expect("pretty");
    assert_eq!(loaded, full);
    assert_eq!(loaded.compute_hash().expect("hash"), full.definition_hash);

    // Tampered and uppercase hashes are still HashMismatch with the computed value.
    let mut upper = good.clone();
    upper["definitionHash"] = json!(full.definition_hash.to_uppercase());
    assert_eq!(
        CardDefinitionDSL::from_json(&json_bytes(&upper)),
        Err(CardDefinitionError::HashMismatch {
            declared: full.definition_hash.to_uppercase(),
            computed: full.definition_hash.clone(),
        })
    );
}

/// A sealed synthetic definition as JSON whose `power` is `{"LITERAL": <spelling>}` and whose
/// ability carries keyword parameters spelled with `spelling`, with a recomputed hash.
fn spelled_numbers(spelling: &str) -> Value {
    let mut v = sealed_json();
    v["faces"][0]["power"] =
        serde_json::from_str(&format!("{{\"LITERAL\": {spelling}}}")).expect("lit");
    v["faces"][0]["abilities"][0] = serde_json::from_str(&format!(
        "{{\"abilityId\": \"a1\", \"kind\": \"KEYWORD\", \"keyword\": \"TEST_KEYWORD\", \
         \"params\": {{\"n\": {spelling}, \"m\": [{spelling}]}}}}"
    ))
    .expect("ab");
    rehash(&mut v);
    v
}

/// REQ-DSL-001, REQ-NFR-006, REQ-DSL-003: numbers in AST positions are normalised on load, so
/// `-0.0`, `1.0` and `1e0` load exactly as `0` and `1` (same value, same hash), and the loaded
/// value seals unchanged and round-trips. A non-integral number is not a valid real AST
/// (REQ-CORE-006): it is refused on load, and a hand-built typed value holding one cannot be
/// sealed or hashed. In memory no number is normalised: a hand-built keyword parameter holding
/// an integral float (`-0.0`, `1.0`, `1e0`) is refused like a fraction, and building the integer
/// instead seals to the wire case's value, hash and bytes (gate 06-turn F97).
#[test]
fn req_dsl_001_seal_normalises_numbers() {
    for (spelling, integer) in [("-0.0", "0"), ("1.0", "1"), ("1e0", "1")] {
        let loaded = CardDefinitionDSL::from_json(&json_bytes(&spelled_numbers(spelling)))
            .unwrap_or_else(|e| panic!("{spelling}: {e}"));
        let plain = CardDefinitionDSL::from_json(&json_bytes(&spelled_numbers(integer)))
            .expect("integer spelling");
        assert_eq!(loaded, plain, "{spelling}");
        let hash_before = loaded.compute_hash().expect("hash");
        let sealed = loaded.clone().seal().expect("seal");
        assert_eq!(sealed, loaded, "{spelling}");
        assert_eq!(sealed.definition_hash, hash_before, "{spelling}");
        let exported = sealed.to_canonical_json().expect("json");
        assert_eq!(
            CardDefinitionDSL::from_json(&exported).as_ref(),
            Ok(&sealed),
            "{spelling}"
        );
        // The in-memory case (gate 06-turn F97; docs/DECISIONS.md "Gate 06-turn F97 —
        // In-memory integral floats in AST params"): the same keyword `params` built in memory
        // with the float spelling is refused by the ability serializer, so it cannot be
        // hashed, exported or sealed (the typed AST wire contract has integers only; `power` is
        // an `i64` and cannot hold a float at all). The migration path is to build the
        // integer (as here) or to load the document text with `from_json`, which normalises it
        // (the wire case above): both give the same sealed value, hash and bytes.
        let float = serde_json::from_str::<Value>(spelling).expect("float");
        assert!(float.is_f64(), "{spelling}: an in-memory float");
        let int = serde_json::from_str::<Value>(integer).expect("integer");
        let with_params = |n: &Value| {
            let mut def = plain.clone();
            let crate::ability::AbilityAST::Keyword(keyword) = &mut def.faces[0].abilities[0]
            else {
                panic!("the spelled ability is a keyword")
            };
            keyword.params = Some(BTreeMap::from([
                ("m".to_owned(), Value::Array(vec![n.clone()])),
                ("n".to_owned(), n.clone()),
            ]));
            def
        };
        let in_memory = with_params(&float);
        assert_ne!(in_memory, plain, "{spelling}: the float is held as given");
        assert!(
            matches!(in_memory.compute_hash(), Err(CanonicalError::Serialize(_))),
            "{spelling}: hash"
        );
        assert!(
            matches!(
                in_memory.to_canonical_json(),
                Err(CanonicalError::Serialize(_))
            ),
            "{spelling}: export"
        );
        assert!(
            matches!(in_memory.clone().seal(), Err(CanonicalError::Serialize(_))),
            "{spelling}: seal"
        );
        let migrated = with_params(&int);
        assert_eq!(migrated, plain, "{spelling}: integer migration");
        let migrated_sealed = migrated.seal().expect("seal migrated");
        assert_eq!(migrated_sealed, sealed, "{spelling}: same sealed value");
        assert_eq!(
            migrated_sealed.to_canonical_json().expect("json"),
            exported,
            "{spelling}: same bytes"
        );
        assert_eq!(
            CardDefinitionDSL::from_json(&exported).as_ref(),
            Ok(&migrated_sealed),
            "{spelling}: reload"
        );
    }
    // A fraction survives canonicalisation but is no value expression: the load refuses it.
    let mut fraction = sealed_json();
    fraction["faces"][0]["power"] =
        serde_json::from_str::<Value>(r#"{"LITERAL": 2.5}"#).expect("lit");
    rehash(&mut fraction);
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&fraction)),
        Err(CardDefinitionError::Schema(_))
    ));
    // A hand-built keyword parameter holding a fraction is refused by the ability serializer.
    let mut hand_built = definition();
    let crate::ability::AbilityAST::Keyword(keyword) = &mut hand_built.faces[0].abilities[0] else {
        panic!("the fixture ability is a keyword")
    };
    keyword.params = Some(BTreeMap::from([(
        "n".to_owned(),
        serde_json::from_str::<Value>("2.5").expect("fraction"),
    )]));
    assert!(matches!(
        hand_built.compute_hash(),
        Err(CanonicalError::Serialize(_))
    ));
    assert!(matches!(
        hand_built.seal(),
        Err(CanonicalError::Serialize(_))
    ));
    let mut one = definition();
    one.faces[0].power = Some(ValueExpressionAST::Literal(1));
    let one = one.seal().expect("seal");
    let text = String::from_utf8(one.to_canonical_json().expect("json")).expect("utf8");
    assert!(text.contains("\"power\":{\"LITERAL\":1},"), "{text}");
    let spelled = text.replace("\"power\":{\"LITERAL\":1},", "\"power\":{\"LITERAL\":1.0},");
    assert_eq!(
        CardDefinitionDSL::from_json(spelled.as_bytes()),
        CardDefinitionDSL::from_json(text.as_bytes())
    );
    assert_eq!(CardDefinitionDSL::from_json(spelled.as_bytes()), Ok(one));
}

/// `depth` nested arrays around `1`, built iteratively.
fn nested(depth: usize) -> Value {
    let mut v = json!(1);
    for _ in 0..depth {
        v = Value::Array(vec![v]);
    }
    v
}

/// REQ-DSL-001, REQ-NFR-006, REQ-DSL-003: one depth budget for sealing, export, hashing and
/// loading, reached with valid real content. The definition object, `faces` and a face take
/// three levels; each `MATH` node of `power` takes two, a `LITERAL` leaf one and an `"X"` leaf
/// none.
#[test]
fn req_dsl_001_definition_depth_budget() {
    use crate::canonical::MAX_DEPTH;
    assert_eq!(MAX_DEPTH, 127);
    // 3 + 2 * 62 = 127: exactly the budget.
    let mut deepest = definition();
    deepest.faces[0].power = Some(typed_math_chain(62, ValueExpressionAST::X));
    let sealed = deepest.seal().expect("deepest allowed seals");
    let exported = sealed.to_canonical_json().expect("export");
    assert_eq!(CardDefinitionDSL::from_json(&exported), Ok(sealed));

    let too_deep = CanonicalError::Serialize(format!("nesting depth exceeds {MAX_DEPTH}"));
    // 3 + 2 * 62 + 1 = 128 and 3 + 2 * 63 + 1 = 130.
    for levels in [62, 63] {
        let mut deeper = definition();
        deeper.faces[0].power = Some(typed_math_chain(levels, ValueExpressionAST::Literal(1)));
        assert_eq!(deeper.clone().seal(), Err(too_deep.clone()));
        assert_eq!(deeper.to_canonical_json(), Err(too_deep.clone()));
        assert_eq!(deeper.compute_hash(), Err(too_deep.clone()));
        // The loader refuses the same depth as a Canonical error.
        let mut v = sealed_json();
        v["faces"][0]["power"] = math_chain(levels, json!({"LITERAL": 1}));
        assert!(matches!(
            CardDefinitionDSL::from_json(&json_bytes(&v)),
            Err(CardDefinitionError::Canonical(CanonicalError::Syntax(_)))
        ));
    }
}

/// The plain JSON of a sealed synthetic definition.
fn sealed_json() -> Value {
    serde_json::to_value(sealed()).expect("value")
}

/// REQ-DSL-001, REQ-NFR-006, REQ-DSL-012: a noncharacter in a definition cannot be sealed or
/// exported, a typed LITERAL text token cannot hold one, and a noncharacter in the input is a
/// Canonical error.
#[test]
fn req_nfr_006_definition_noncharacter() {
    let mut def = definition();
    def.faces[0].name = "test.\u{FFFE}".into();
    assert!(matches!(
        def.clone().seal(),
        Err(CanonicalError::Serialize(_))
    ));
    // A typed token stream cannot hold a noncharacter at all (#232: the text position is the
    // real stream, whose constructor refuses it).
    assert!(
        TextTokenStream::new(vec![crate::text::TextToken::Literal {
            id: "t1".to_owned(),
            text: "abc\u{FFFE}".to_owned(),
        }])
        .is_err()
    );
    let token_text = String::from_utf8(sealed().to_canonical_json().expect("json"))
        .expect("utf8")
        .replace("\"text\":\"Flying\"", "\"text\":\"Fly\\ufdd0\"");
    assert!(token_text.contains("\\ufdd0"), "{token_text}");
    assert!(matches!(
        CardDefinitionDSL::from_json(token_text.as_bytes()),
        Err(CardDefinitionError::Canonical(CanonicalError::Syntax(_)))
    ));
    let text = String::from_utf8(sealed().to_canonical_json().expect("json"))
        .expect("utf8")
        .replace("\"test.flier_2_2\"", "\"test.\\ufdd0\"");
    assert!(matches!(
        CardDefinitionDSL::from_json(text.as_bytes()),
        Err(CardDefinitionError::Canonical(CanonicalError::Syntax(_)))
    ));
}

/// REQ-DSL-001, R12: random structural mutations of valid definitions (null members, positional
/// arrays, wrapped enums, deep nesting, noncharacters) never panic and are never accepted with
/// a different value than their canonical decode.
#[test]
fn req_dsl_001_random_review_mutations_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0dd1_0000_0003);
    for iteration in 0..300 {
        let def = random_definition(&mut rng).seal().expect("seal");
        let mut v = serde_json::to_value(&def).expect("ser");
        let pick = |rng: &mut Pcg64Dxsm, n: usize| (rng.next_u64() % n as u64) as usize;
        let keys: Vec<String> = v.as_object().expect("obj").keys().cloned().collect();
        let key = keys[pick(&mut rng, keys.len())].clone();
        match pick(&mut rng, 6) {
            0 => v[&key] = Value::Null,
            1 => {
                if let Value::Object(m) = &v[&key] {
                    v[&key] = Value::Array(m.values().cloned().collect());
                }
            }
            2 => v[&key] = json!({ "x": v[&key].clone() }),
            3 => v["faces"][0]["abilities"] = json!([nested(pick(&mut rng, 140))]),
            4 => v["faces"][0]["power"] = math_chain(pick(&mut rng, 70), json!("X")),
            _ => v[&key] = json!("\u{FDD0}"),
        }
        if pick(&mut rng, 2) == 1 {
            let mut without = v.clone();
            without
                .as_object_mut()
                .expect("obj")
                .remove("definitionHash");
            if let Ok(bytes) = canonicalize(&json_bytes(&without)) {
                v["definitionHash"] = json!(fineprint_base::hash::sha256(&bytes).to_hex());
            }
        }
        let input = json_bytes(&v);
        if let Ok(loaded) = CardDefinitionDSL::from_json(&input) {
            let again = loaded.to_canonical_json().expect("export");
            assert_eq!(
                canonicalize(&input).expect("canonicalize accepted input"),
                again,
                "accepted input not preserved by load and export, iteration {iteration}"
            );
            assert_eq!(CardDefinitionDSL::from_json(&again), Ok(loaded));
        }
    }
}

/// REQ-DSL-001, REQ-DSL-002: seal rejects a `dslVersion` that `from_json` would reject, and every
/// supported version seals to a value that round-trips.
#[test]
fn req_dsl_001_seal_checks_dsl_version() {
    let mut other = definition();
    other.dsl_version = "2.0".into();
    match other.seal() {
        Err(CanonicalError::Serialize(msg)) => assert!(msg.contains('2'), "{msg}"),
        r => panic!("unexpected {r:?}"),
    }
    let mut bad = definition();
    bad.dsl_version = "abc".into();
    assert!(matches!(bad.seal(), Err(CanonicalError::Serialize(_))));
    for version in ["1.0", "1.7.3", "1.999"] {
        let mut def = definition();
        def.dsl_version = version.into();
        let sealed = def.seal().expect(version);
        let export = sealed.to_canonical_json().expect("export");
        assert_eq!(
            CardDefinitionDSL::from_json(&export),
            Ok(sealed),
            "{version}"
        );
    }
}

/// REQ-DSL-002, REQ-NFR-006: a stale `definitionHash` is excluded from the hash input, even when
/// it holds a noncharacter, but is still validated when the definition is exported.
#[test]
fn req_nfr_006_stale_hash_excluded_from_hash_input() {
    let d = sealed();
    let mut bad = d.clone();
    bad.definition_hash = "\u{FDD0}".to_string();
    assert_eq!(bad.compute_hash(), d.compute_hash());
    assert_eq!(bad.clone().seal(), Ok(d.clone()));
    assert!(matches!(
        bad.to_canonical_json(),
        Err(CanonicalError::Serialize(_))
    ));
    assert!(matches!(
        bad.verify_hash(),
        Err(CardDefinitionError::HashMismatch { .. })
    ));
    for stale in [String::new(), "X".repeat(64), "\u{FFFF}".to_owned()] {
        let mut other = d.clone();
        other.definition_hash = stale;
        assert_eq!(other.compute_hash(), d.compute_hash());
    }
}

/// A keyword ability whose parameter `n` is `depth` nested arrays (valid-shaped, but deeper
/// than the depth budget when `depth` is large).
fn deep_keyword(depth: usize) -> AbilityAST {
    let mut keyword = crate::ability::KeywordInstanceAST::new(
        crate::ability::AbilityBase::new("a1"),
        "TEST_KEYWORD",
    );
    keyword.params = Some(BTreeMap::from([("n".to_owned(), nested(depth))]));
    AbilityAST::Keyword(keyword)
}

/// REQ-NFR-006, REQ-DSL-002, REQ-CORE-006, REQ-DSL-003: `compute_hash` hashes the borrowed
/// definition, so a typed value built in memory far beyond the depth budget is a `Serialize`
/// error rather than a stack overflow, both for a deep value expression and for a deep opaque
/// keyword parameter. The value is forgotten, not dropped, because dropping it would itself
/// recurse.
#[test]
fn req_nfr_006_compute_hash_deep_value_is_an_error() {
    let mut def = definition();
    // 50 000 MATH nodes: 100 000 levels of value expression.
    def.faces[0].power = Some(typed_math_chain(50_000, ValueExpressionAST::X));
    let hash = def.compute_hash();
    let verified = def.verify_hash();
    let exported = def.to_canonical_json();
    // Forget before asserting: a failed assertion must not unwind into a recursive drop.
    std::mem::forget(def);
    assert!(matches!(hash, Err(CanonicalError::Serialize(_))));
    assert!(matches!(
        verified,
        Err(CardDefinitionError::Canonical(CanonicalError::Serialize(_)))
    ));
    assert!(matches!(exported, Err(CanonicalError::Serialize(_))));
    let mut def = definition();
    def.faces[0].abilities = vec![deep_keyword(100_000)];
    let hash = def.compute_hash();
    std::mem::forget(def);
    assert!(matches!(hash, Err(CanonicalError::Serialize(_))));
}

/// REQ-NFR-006, REQ-DSL-002, REQ-CORE-006: `seal` on a definition nested past the budget is a
/// `Serialize` error (1 000 levels, small enough to drop safely), in a value expression and in
/// an opaque keyword parameter.
#[test]
fn req_nfr_006_seal_deep_value_is_an_error() {
    let mut def = definition();
    def.faces[0].power = Some(typed_math_chain(500, ValueExpressionAST::X));
    assert!(matches!(def.seal(), Err(CanonicalError::Serialize(_))));
    let mut def = definition();
    def.faces[0].abilities = vec![deep_keyword(1_000)];
    assert!(matches!(def.seal(), Err(CanonicalError::Serialize(_))));
}

/// REQ-NFR-006, REQ-DSL-002, REQ-CORE-006: only the top-level `definitionHash` is left out of
/// the hash; a nested member of that name is part of the hash input and is checked.
#[test]
fn req_nfr_006_nested_definition_hash_member_is_hashed() {
    let base = sealed();
    let with_param = |value: &str| {
        let mut keyword = crate::ability::KeywordInstanceAST::new(
            crate::ability::AbilityBase::new("a1"),
            "TEST_KEYWORD",
        );
        keyword.params = Some(BTreeMap::from([(
            "definitionHash".to_owned(),
            Value::String(value.to_owned()),
        )]));
        AbilityAST::Keyword(keyword)
    };
    let mut nested_member = definition();
    nested_member.faces[0].abilities = vec![with_param("x")];
    let nested_hash = nested_member.compute_hash().expect("hash");
    assert_ne!(nested_hash, base.compute_hash().expect("hash"));
    let mut other = definition();
    other.faces[0].abilities = vec![with_param("y")];
    assert_ne!(other.compute_hash().expect("hash"), nested_hash);
    let mut bad = definition();
    bad.faces[0].abilities = vec![with_param("\u{FDD0}")];
    assert!(matches!(
        bad.compute_hash(),
        Err(CanonicalError::Serialize(_))
    ));
    // The hash input is the canonical definition with only the top-level member removed.
    let sealed_nested = nested_member.seal().expect("seal");
    let mut value = serde_json::to_value(&sealed_nested).expect("value");
    value
        .as_object_mut()
        .expect("object")
        .remove("definitionHash");
    let bytes = canonicalize(&json_bytes(&value)).expect("canonical");
    assert_eq!(
        sealed_nested.definition_hash,
        fineprint_base::hash::sha256(&bytes).to_hex()
    );
}

// ---- #159 FP-140: AST positions checked against the real DSL types ----

/// Valid value expressions (REQ-DSL-003, SRS §14.4), each in its canonical spelling.
fn valid_values() -> Vec<Value> {
    vec![
        json!({"LITERAL": 0}),
        json!({"LITERAL": -3}),
        json!({"LITERAL": MAX_SAFE_INTEGER}),
        json!("X"),
        json!({"MATH": {"op": "ADD", "lhs": {"LITERAL": 1}, "rhs": "X"}}),
        json!({"MATH": {
            "op": "DIV_FLOOR",
            "lhs": {"MATH": {"op": "MUL", "lhs": "X", "rhs": {"LITERAL": 2}}},
            "rhs": {"MATH": {"op": "MAX", "lhs": {"LITERAL": 1}, "rhs": "X"}}
        }}),
        math_chain(10, json!({"LITERAL": 7})),
    ]
}

/// Valid token streams (REQ-DSL-012, SRS §14.5), synthetic text only (R5).
fn valid_texts() -> Vec<Vec<Value>> {
    vec![
        vec![],
        vec![json!({"id": "t1", "kind": "LITERAL", "text": "abc"})],
        vec![json!({"id": "t1", "kind": "WORD", "category": "COLOR_WORD", "value": "Hue"})],
        vec![json!({"id": "t1", "kind": "NAME_REF", "ref": "SELF"})],
        vec![
            json!({"id": "t1", "kind": "NAME_REF", "ref": "SELF"}),
            json!({"id": "t2", "kind": "LITERAL", "text": " gets +1/+1 for each "}),
            json!({"id": "t3", "kind": "WORD", "category": "CREATURE_TYPE", "value": "TestType"}),
            json!({"id": "t4", "kind": "WORD", "category": "NUMBER", "value": "2"}),
            json!({"id": "t5", "kind": "NAME_REF", "ref": "OTHER", "name": "test.other"}),
        ],
    ]
}

/// Valid abilities of all six kinds (REQ-DSL-010, SRS §14.3). Non-empty effect lists use a real
/// effect instruction (`POPULATE`), never a free-form placeholder op; the replacement uses the
/// replacement op `SKIP`.
fn valid_abilities() -> Vec<Value> {
    vec![
        json!({"abilityId": "a1", "kind": "SPELL", "effect": []}),
        json!({"abilityId": "a1", "kind": "SPELL", "effect": [{"op": "POPULATE"}]}),
        json!({"abilityId": "a1", "kind": "ACTIVATED", "cost": [{"cost": "TAP_SOURCE"}],
               "effect": [{"op": "POPULATE"}]}),
        json!({"abilityId": "a1", "kind": "TRIGGERED",
               "trigger": {"mode": "STATE", "state": {"pred": "IS_CREATURE"}}, "effect": []}),
        json!({"abilityId": "a1", "kind": "STATIC", "effects": [{"type": "RULE", "kind": "SKIP"}]}),
        json!({"abilityId": "a1", "kind": "REPLACEMENT",
               "match": {"event": "DIES", "affected": {"ref": "SELF"}},
               "replaceWith": [{"op": "SKIP"}]}),
        json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD"}),
        json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
               "functionsIn": ["BATTLEFIELD"], "textSpan": [0, 1], "params": {"n": 2}}),
    ]
}

/// Valid partner abilities: KEYWORD abilities only.
fn valid_partners() -> Vec<Value> {
    vec![
        json!({"abilityId": "p1", "kind": "KEYWORD", "keyword": "PARTNER"}),
        json!({"abilityId": "p1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
               "params": {"with": "test.partner"}}),
    ]
}

/// A synthetic two-face definition with a commander eligibility, so every AST position exists
/// (abilities are also checked on the second face).
fn two_face_definition() -> CardDefinitionDSL {
    let mut def = definition();
    let mut back = face("test.back_face", None);
    back.face_kind = FaceKind::Back;
    back.text = text_of(vec![json!({"id": "t1", "kind": "NAME_REF", "ref": "SELF"})]);
    back.abilities = vec![ability_of(
        json!({"abilityId": "b1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD"}),
    )];
    def.layout = LayoutBehavior::DfcNonmodal;
    def.faces.push(back);
    def.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: true,
        partner_abilities: vec![partner("p1")],
    });
    def
}

/// One AST position of [`two_face_definition`]: the typed setter and the JSON member.
#[derive(Clone, Copy, Debug)]
enum Position {
    Power,
    Toughness,
    Text,
    BackAbility,
    Partner,
}

impl Position {
    const ALL: [Position; 5] = [
        Position::Power,
        Position::Toughness,
        Position::Text,
        Position::BackAbility,
        Position::Partner,
    ];

    /// `def` with `content` (valid JSON of this position's real type) decoded into this
    /// position.
    fn set(self, def: &mut CardDefinitionDSL, content: Value) {
        match self {
            Position::Power => def.faces[0].power = Some(vexpr(content)),
            Position::Toughness => def.faces[0].toughness = Some(vexpr(content)),
            Position::Text => {
                def.faces[0].text = serde_json::from_value(content).expect("valid text");
            }
            Position::BackAbility => def.faces[1].abilities = vec![ability_of(content)],
            Position::Partner => {
                if let Some(c) = def.commander_eligibility.as_mut() {
                    c.partner_abilities = vec![partner_of(content)];
                }
            }
        }
    }

    /// Whether plain serde decodes `content` as this position's type.
    fn decodes(self, content: &Value) -> bool {
        match self {
            Position::Power | Position::Toughness => {
                serde_json::from_value::<ValueExpressionAST>(content.clone()).is_ok()
            }
            Position::Text => serde_json::from_value::<TextTokenStream>(content.clone()).is_ok(),
            Position::BackAbility => serde_json::from_value::<AbilityAST>(content.clone()).is_ok(),
            Position::Partner => {
                serde_json::from_value::<PartnerAbilityDSL>(content.clone()).is_ok()
            }
        }
    }

    /// The JSON member of this position in a serialised definition.
    fn get(self, v: &Value) -> &Value {
        match self {
            Position::Power => &v["faces"][0]["power"],
            Position::Toughness => &v["faces"][0]["toughness"],
            Position::Text => &v["faces"][0]["text"],
            Position::BackAbility => &v["faces"][1]["abilities"][0],
            Position::Partner => &v["commanderEligibility"]["partnerAbilities"][0],
        }
    }

    /// Replaces the JSON member of this position in a serialised definition.
    fn replace(self, v: &mut Value, content: Value) {
        let slot = match self {
            Position::Power => &mut v["faces"][0]["power"],
            Position::Toughness => &mut v["faces"][0]["toughness"],
            Position::Text => &mut v["faces"][0]["text"],
            Position::BackAbility => &mut v["faces"][1]["abilities"][0],
            Position::Partner => &mut v["commanderEligibility"]["partnerAbilities"][0],
        };
        *slot = content;
    }

    /// Valid samples for this position (text samples as arrays).
    fn valid(self) -> Vec<Value> {
        match self {
            Position::Power | Position::Toughness => valid_values(),
            Position::Text => valid_texts().into_iter().map(Value::Array).collect(),
            Position::BackAbility => valid_abilities(),
            Position::Partner => valid_partners(),
        }
    }

    /// Content that is not a valid real AST for this position.
    fn invalid(self) -> Vec<Value> {
        match self {
            Position::Power | Position::Toughness => vec![
                Value::Null,
                json!(2),
                json!({"const": 2}),
                json!({"LITERAL": 1, "extra": 1}),
                json!({"LITERAL": "1"}),
                json!({"MATH": {"op": "POW", "lhs": "X", "rhs": "X"}}),
                json!({"MATH": {"op": "ADD", "lhs": "X"}}),
                json!("Y"),
                json!([{"LITERAL": 1}]),
            ],
            Position::Text => vec![
                json!([{"kw": "Flying"}]),
                json!([1]),
                json!([{"id": "t1", "kind": "LITERAL", "text": "a"},
                       {"id": "t1", "kind": "LITERAL", "text": "b"}]),
                json!([{"id": "", "kind": "LITERAL", "text": "a"}]),
                json!([{"id": "t1", "kind": "LITERAL", "text": ""}]),
                json!([{"id": "t1", "kind": "BOLD", "text": "a"}]),
                json!([{"id": "t1", "kind": "WORD", "category": "FLAVOR", "value": "a"}]),
                json!([{"id": "t1", "kind": "NAME_REF", "ref": "OTHER"}]),
                json!([{"id": "t1", "kind": "LITERAL", "text": "a", "extra": true}]),
                json!([null]),
                json!({"id": "t1", "kind": "LITERAL", "text": "a"}),
            ],
            Position::BackAbility => vec![
                json!("test.ability"),
                json!(7),
                Value::Null,
                json!({"keyword": "FLYING"}),
                json!({"abilityId": "a1", "kind": "MAGIC"}),
                json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD", "x": 1}),
                json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "lowercase"}),
                json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
                       "functionsIn": null}),
                json!({"abilityId": "a1", "kind": "SPELL", "effect": [{"note": 1}]}),
                json!({"abilityId": "a1", "kind": "ACTIVATED", "cost": [], "effect": []}),
                json!({"abilityId": "a1", "kind": "SPELL"}),
            ],
            Position::Partner => vec![
                json!({"kind": "test"}),
                json!({"p": 1}),
                json!({"abilityId": "p1", "kind": "STATIC",
                       "effects": [{"type": "RULE", "kind": "SKIP"}]}),
                json!({"abilityId": "p1", "kind": "SPELL", "effect": []}),
                Value::Null,
            ],
        }
    }
}

/// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012, REQ-NFR-006 (#232): the `card::` AST names are the
/// real typed ASTs. A function taking the real `expr`, `text` and `ability` types accepts the
/// `card::` names unchanged (a compile-time identity check), and the face fields hold them.
#[test]
fn req_dsl_003_card_ast_names_are_the_real_types() {
    fn takes_value(v: &crate::expr::ValueExpressionAST) -> &crate::expr::ValueExpressionAST {
        v
    }
    fn takes_text(t: &crate::text::TextTokenStream) -> usize {
        t.tokens().len()
    }
    fn takes_ability(a: &crate::ability::AbilityAST) -> &'static str {
        a.kind()
    }
    let def = definition();
    let face = &def.faces[0];
    let power: &ValueExpressionAST = face.power.as_ref().expect("power");
    assert_eq!(
        takes_value(power),
        &crate::expr::ValueExpressionAST::Literal(2)
    );
    let text: &TextTokenStream = &face.text;
    assert_eq!(takes_text(text), 1);
    let ability: &AbilityAST = &face.abilities[0];
    assert_eq!(takes_ability(ability), "KEYWORD");
    let empty: TextTokenStream = crate::text::TextTokenStream::default();
    assert_eq!(takes_text(&empty), 0);
}

/// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012, REQ-NFR-006 (acceptance 2): valid value expressions,
/// token streams and abilities of all six kinds in every AST position seal; the canonical JSON
/// loads back equal; the hash is stable and verifies; and the position keeps exactly the given
/// JSON (single spelling, the typed value never changes the hash).
#[test]
fn req_dsl_003_valid_asts_seal_and_round_trip() {
    for position in Position::ALL {
        for sample in position.valid() {
            let mut def = two_face_definition();
            position.set(&mut def, sample.clone());
            let hash_before = def.compute_hash().expect("hash");
            let sealed = def
                .seal()
                .unwrap_or_else(|e| panic!("{position:?} {sample}: {e}"));
            assert_eq!(sealed.definition_hash, hash_before, "{position:?} {sample}");
            assert_eq!(sealed.verify_hash(), Ok(()));
            assert_eq!(sealed.compute_hash(), Ok(hash_before.clone()));
            let canonical = sealed.to_canonical_json().expect("canonical");
            assert_eq!(canonicalize(&canonical).expect("stable"), canonical);
            let loaded = CardDefinitionDSL::from_json(&canonical).expect("load");
            assert_eq!(loaded, sealed, "{position:?} {sample}");
            assert_eq!(loaded.compute_hash(), Ok(hash_before));
            let v = serde_json::to_value(&loaded).expect("value");
            assert_eq!(position.get(&v), &sample, "{position:?}");
        }
    }
    // Every ability kind is covered.
    let kinds: std::collections::BTreeSet<&str> = valid_abilities()
        .into_iter()
        .map(|a| ability_of(a).kind())
        .collect();
    assert_eq!(kinds.len(), 6);
}

/// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012 (acceptance 1, #232): content that is not a valid
/// real AST is refused in each of the five positions: plain serde of the position type and of
/// the whole definition refuses it, and `from_json` returns `Schema` (the document carries the
/// correct hash over its checked content, so the failure is the typed decode). Such content can
/// no longer be held by a `CardDefinitionDSL`, so `seal` never sees it (docs/DECISIONS.md,
/// "#232 FP-164 — Golden hashes and replaced tests").
#[test]
fn req_dsl_003_invalid_asts_rejected_on_load_and_seal() {
    let good = serde_json::to_value(two_face_definition().seal().expect("seal")).expect("value");
    for position in Position::ALL {
        for bad in position.invalid() {
            assert!(!position.decodes(&bad), "{position:?} {bad}");
            let mut v = good.clone();
            position.replace(&mut v, bad.clone());
            assert!(
                serde_json::from_value::<CardDefinitionDSL>(v.clone()).is_err(),
                "{position:?} {bad}"
            );
            rehash(&mut v);
            match CardDefinitionDSL::from_json(&json_bytes(&v)) {
                Err(CardDefinitionError::Schema(msg)) => assert!(!msg.is_empty()),
                r => panic!("{position:?} {bad}: unexpected {r:?}"),
            }
        }
    }
}

/// REQ-DSL-003, REQ-DSL-012, REQ-NFR-006: a present `null` power or toughness is refused by
/// plain serde and by `from_json` (`Schema`); and abilities are decoded on every face.
#[test]
fn req_dsl_003_null_power_and_later_faces_rejected() {
    for member in ["power", "toughness"] {
        let mut v = sealed_json();
        v["faces"][0][member] = Value::Null;
        assert!(serde_json::from_value::<CardDefinitionDSL>(v.clone()).is_err());
        rehash(&mut v);
        assert!(matches!(
            CardDefinitionDSL::from_json(&json_bytes(&v)),
            Err(CardDefinitionError::Schema(_))
        ));
    }
    // A third face's second ability is decoded too.
    let mut def = two_face_definition();
    def.faces.push(face("test.third", None));
    let mut v = serde_json::to_value(def.seal().expect("seal")).expect("value");
    v["faces"][2]["abilities"]
        .as_array_mut()
        .expect("abilities")
        .push(json!({"keyword": "FLYING"}));
    rehash(&mut v);
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&v)),
        Err(CardDefinitionError::Schema(_))
    ));
}

/// REQ-DSL-003, REQ-NFR-006: `from_json` decodes ASTs before it checks the hash, so invalid
/// content whose declared hash is also wrong reports `Schema`, not `HashMismatch`.
#[test]
fn req_dsl_003_ast_check_precedes_hash_check() {
    let good = serde_json::to_value(two_face_definition().seal().expect("seal")).expect("value");
    for position in Position::ALL {
        for bad in position.invalid() {
            let mut v = good.clone();
            position.replace(&mut v, bad.clone());
            // The old hash no longer matches the content.
            let result = CardDefinitionDSL::from_json(&json_bytes(&v));
            assert!(
                matches!(&result, Err(CardDefinitionError::Schema(_))),
                "{position:?} {bad}: {result:?}"
            );
            let mut wrong = v.clone();
            wrong["definitionHash"] = json!("f".repeat(64));
            assert!(matches!(
                CardDefinitionDSL::from_json(&json_bytes(&wrong)),
                Err(CardDefinitionError::Schema(_))
            ));
        }
    }
    // A valid AST with a tampered hash is still a HashMismatch.
    let mut v = good.clone();
    v["faces"][0]["power"] = json!({"LITERAL": 9});
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&v)),
        Err(CardDefinitionError::HashMismatch { .. })
    ));
    // A null loyalty together with an invalid power is a Schema error as well.
    let mut both = good;
    both["faces"][0]["loyalty"] = Value::Null;
    both["faces"][0]["power"] = Value::Null;
    rehash(&mut both);
    assert!(matches!(
        CardDefinitionDSL::from_json(&json_bytes(&both)),
        Err(CardDefinitionError::Schema(_))
    ));
}

/// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012 (#232): plain serde of the `card::` AST types decodes
/// valid content into the real types and re-encodes it to exactly the same JSON, and refuses
/// invalid content, for example a `null` or `1` power, a text with a duplicate token id, an
/// ability of an unknown kind and a partner of kind `SPELL`.
#[test]
fn req_dsl_010_typed_positions_decode_strictly() {
    for v in valid_values() {
        let real = vexpr(v.clone());
        assert_eq!(canonical::to_checked_value(&real), Ok(v));
    }
    assert_eq!(
        vexpr(json!({"LITERAL": 4})),
        crate::expr::ValueExpressionAST::Literal(4)
    );
    for t in valid_texts() {
        let real = text_of(t.clone());
        assert_eq!(real.len(), t.len());
        assert_eq!(canonical::to_checked_value(&real), Ok(Value::Array(t)));
    }
    let tokens = text_of(valid_texts()[4].clone());
    assert!(
        tokens
            .find("t3")
            .is_some_and(crate::text::TextToken::is_rewritable)
    );
    for a in valid_abilities() {
        let real = ability_of(a.clone());
        assert_eq!(canonical::to_checked_value(&real), Ok(a.clone()));
        // Only KEYWORD abilities decode as partner abilities.
        let as_partner = serde_json::from_value::<PartnerAbilityDSL>(a);
        if real.kind() == "KEYWORD" {
            assert_eq!(as_partner.expect("keyword partner").as_ability(), &real);
        } else {
            let err = as_partner.expect_err("non-keyword partner").to_string();
            assert!(
                err.contains("KEYWORD") && err.contains(real.kind()),
                "{err}"
            );
        }
    }
    for p in valid_partners() {
        let real = partner_of(p.clone());
        assert_eq!(real.as_ability().kind(), "KEYWORD");
        assert_eq!(canonical::to_checked_value(&real), Ok(p));
    }
    for bad in Position::Power.invalid() {
        assert!(
            serde_json::from_value::<ValueExpressionAST>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    for bad in Position::Text.invalid() {
        assert!(
            serde_json::from_value::<TextTokenStream>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    for bad in Position::BackAbility.invalid() {
        assert!(
            serde_json::from_value::<AbilityAST>(bad.clone()).is_err(),
            "{bad}"
        );
        assert!(
            serde_json::from_value::<PartnerAbilityDSL>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    // The named examples of the brief, explicitly.
    for bad in [Value::Null, json!(1)] {
        assert!(serde_json::from_value::<ValueExpressionAST>(bad).is_err());
    }
    assert!(
        serde_json::from_value::<TextTokenStream>(json!([
            {"id": "t1", "kind": "LITERAL", "text": "a"},
            {"id": "t1", "kind": "LITERAL", "text": "b"}
        ]))
        .is_err()
    );
    assert!(
        serde_json::from_value::<AbilityAST>(json!({"abilityId": "a1", "kind": "MAGIC"})).is_err()
    );
    let spell_partner = serde_json::from_value::<PartnerAbilityDSL>(
        json!({"abilityId": "p1", "kind": "SPELL", "effect": []}),
    )
    .expect_err("a SPELL partner");
    assert!(
        spell_partner
            .to_string()
            .contains("a partner ability must be a KEYWORD ability, found kind SPELL"),
        "{spell_partner}"
    );
    // A non-integral literal is not a value expression (REQ-CORE-006).
    let fraction: Value = serde_json::from_str(r#"{"LITERAL": 1.5}"#).expect("json");
    assert!(serde_json::from_value::<ValueExpressionAST>(fraction).is_err());
}

/// REQ-DSL-010 (#232): `PartnerAbilityDSL::new` accepts only a `KEYWORD` ability, with the exact
/// error message otherwise; `as_ability` and `into_ability` return the wrapped ability; the
/// JSON form is the ability's own (transparent).
#[test]
fn req_dsl_010_partner_ability_new_as_into() {
    for a in valid_abilities() {
        let ability = ability_of(a.clone());
        let kind = ability.kind();
        match PartnerAbilityDSL::new(ability.clone()) {
            Ok(partner) => {
                assert_eq!(kind, "KEYWORD");
                assert_eq!(partner.as_ability(), &ability);
                assert_eq!(serde_json::to_value(&partner).expect("ser"), a);
                assert_eq!(partner.into_ability(), ability);
            }
            Err(msg) => {
                assert_ne!(kind, "KEYWORD");
                assert_eq!(
                    msg,
                    format!("a partner ability must be a KEYWORD ability, found kind {kind}")
                );
            }
        }
    }
    let spell = ability_of(json!({"abilityId": "p1", "kind": "SPELL", "effect": []}));
    assert_eq!(
        PartnerAbilityDSL::new(spell),
        Err("a partner ability must be a KEYWORD ability, found kind SPELL".to_owned())
    );
}

/// REQ-NFR-006, REQ-DSL-003, REQ-DSL-010, REQ-DSL-012 (#232): typing the AST positions changes
/// no byte of a valid definition: the unit fixture `sealed()` still hashes to the value recorded
/// in docs/DECISIONS.md ("#159 — Null power/toughness and regenerated fixtures"), its canonical
/// form is stable, and `from_json(to_canonical_json(seal(d))) == seal(d)`.
#[test]
fn req_nfr_006_typed_positions_keep_the_fixture_hash() {
    let sealed = sealed();
    assert_eq!(
        sealed.definition_hash,
        "ce23dc12dae8b49033785a3832ebece12870f940ce895ab3d01f370b8feee216"
    );
    let canonical = sealed.to_canonical_json().expect("canonical");
    assert_eq!(canonicalize(&canonical).expect("stable"), canonical);
    assert_eq!(CardDefinitionDSL::from_json(&canonical), Ok(sealed.clone()));
    assert_eq!(sealed.clone().seal(), Ok(sealed));
}

/// REQ-NFR-006, REQ-DSL-001 (#232): an AST position has a single spelling. A non-canonical
/// spelling of a valid AST (an explicit `null` or default member the type omits, a positional
/// array) is refused by `from_json` with `Schema`, whatever the hash; and every document that
/// loads re-exports to exactly its own canonical bytes (the lossless check of step (c) covers
/// the AST positions too).
#[test]
fn req_nfr_006_non_canonical_ast_spelling_rejected() {
    let good = serde_json::to_value(two_face_definition().seal().expect("seal")).expect("value");
    let spellings: Vec<(&str, Value)> = vec![
        // The keyword ability with explicit members its type omits when absent.
        (
            "/faces/1/abilities/0",
            json!({"abilityId": "b1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
                   "functionsIn": null}),
        ),
        (
            "/faces/1/abilities/0",
            json!({"abilityId": "b1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
                   "params": null}),
        ),
        (
            "/commanderEligibility/partnerAbilities/0",
            json!({"abilityId": "p1", "kind": "KEYWORD", "keyword": "PARTNER",
                   "linkIds": null}),
        ),
        // A value expression and a token spelled positionally.
        ("/faces/0/power", json!(["LITERAL", 2])),
        ("/faces/0/text/0", json!(["t1", "LITERAL", "Flying"])),
        // A literal spelled as a string.
        ("/faces/0/toughness", json!({"LITERAL": "2"})),
    ];
    for (pointer, spelled) in spellings {
        let mut v = good.clone();
        *v.pointer_mut(pointer).expect("pointer") = spelled.clone();
        assert_schema_both_ways(v, &format!("{pointer} {spelled}"));
    }
    // Every valid AST sample loads with its own spelling unchanged.
    for position in Position::ALL {
        for sample in position.valid() {
            let mut v = good.clone();
            position.replace(&mut v, sample.clone());
            rehash(&mut v);
            let input = json_bytes(&v);
            let loaded = CardDefinitionDSL::from_json(&input).expect("valid sample loads");
            assert_eq!(
                loaded.to_canonical_json().expect("export"),
                canonicalize(&input).expect("canonical"),
                "{position:?} {sample}"
            );
        }
    }
}

/// REQ-DSL-003, REQ-NFR-006 (#232, replaces "wrappers stay permissive"): the placeholder content
/// the old JSON wrappers accepted (a free-form ability map, a free-form text token, a free-form
/// power) is refused by plain serde of the typed positions and by `from_json`.
#[test]
fn req_nfr_006_typed_positions_refuse_placeholders() {
    assert!(serde_json::from_value::<AbilityAST>(json!("test.ability")).is_err());
    assert!(serde_json::from_value::<ValueExpressionAST>(json!({"const": 2})).is_err());
    assert!(serde_json::from_value::<TextTokenStream>(json!([1, "a", null])).is_err());
    assert!(serde_json::from_value::<PartnerAbilityDSL>(json!({"kind": "test"})).is_err());
    let mut placeholder = sealed_json();
    placeholder["faces"][0]["abilities"] = json!([{"keyword": "FLYING"}]);
    placeholder["faces"][0]["text"] = json!([{"kw": "Flying"}]);
    placeholder["faces"][0]["power"] = json!({"const": 2});
    assert!(serde_json::from_value::<CardDefinitionDSL>(placeholder.clone()).is_err());
    assert_schema_both_ways(placeholder, "placeholder content");
}

/// A random valid value expression of at most `depth` MATH levels (synthetic, R5).
fn random_value(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    const OPS: [&str; 7] = ["ADD", "SUB", "MUL", "DIV_FLOOR", "DIV_CEIL", "MIN", "MAX"];
    match rng.next_u64() % if depth == 0 { 2 } else { 3 } {
        0 => json!({"LITERAL": (rng.next_u64() % 41) as i64 - 20}),
        1 => json!("X"),
        _ => {
            let op = OPS[(rng.next_u64() % 7) as usize];
            let lhs = random_value(rng, depth - 1);
            let rhs = random_value(rng, depth - 1);
            json!({"MATH": {"op": op, "lhs": lhs, "rhs": rhs}})
        }
    }
}

/// A random valid token stream (synthetic, R5).
fn random_text(rng: &mut Pcg64Dxsm) -> Vec<Value> {
    (0..rng.next_u64() % 5)
        .map(|i| match rng.next_u64() % 4 {
            0 => json!({"id": format!("t{i}"), "kind": "LITERAL", "text": format!("w{i}")}),
            1 => json!({"id": format!("t{i}"), "kind": "WORD", "category": "COLOR_WORD",
                        "value": "Hue"}),
            2 => json!({"id": format!("t{i}"), "kind": "NAME_REF", "ref": "SELF"}),
            _ => json!({"id": format!("t{i}"), "kind": "NAME_REF", "ref": "OTHER",
                        "name": "test.other"}),
        })
        .collect()
}

/// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012, REQ-NFR-006 property: random valid ASTs decode into
/// the typed positions, have a single spelling (the typed value re-encodes to the given JSON),
/// and survive the seal → export → load round trip with the hash unchanged.
#[test]
fn req_nfr_006_property_valid_asts_single_spelling_round_trip() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0dd1_0159_0001);
    let abilities = valid_abilities();
    for _ in 0..200 {
        let mut def = two_face_definition();
        let power = random_value(&mut rng, 4);
        let toughness = random_value(&mut rng, 4);
        let text = random_text(&mut rng);
        let ability = abilities[(rng.next_u64() % abilities.len() as u64) as usize].clone();
        def.faces[0].power = Some(vexpr(power.clone()));
        def.faces[0].toughness = Some(vexpr(toughness));
        def.faces[0].text = text_of(text.clone());
        def.faces[1].abilities = vec![ability_of(ability.clone())];
        assert_eq!(
            def.faces[0].power.as_ref().map(canonical::to_checked_value),
            Some(Ok(power))
        );
        assert_eq!(
            canonical::to_checked_value(&def.faces[0].text),
            Ok(Value::Array(text))
        );
        assert_eq!(
            canonical::to_checked_value(&def.faces[1].abilities[0]),
            Ok(ability)
        );
        let before = def.compute_hash().expect("hash");
        let sealed = def.seal().expect("seal");
        assert_eq!(sealed.definition_hash, before);
        let loaded =
            CardDefinitionDSL::from_json(&sealed.to_canonical_json().expect("json")).expect("load");
        assert_eq!(loaded, sealed);
        assert_eq!(loaded.compute_hash(), Ok(before));
    }
}

/// REQ-DSL-003, R12: plain serde of the typed positions and `from_json` are total on arbitrary
/// JSON content: random byte strings that parse as JSON, random small JSON trees in an AST
/// position, and mutations of valid samples never panic, and whatever loads re-exports to the
/// same canonical bytes.
#[test]
fn req_dsl_003_typed_views_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0dd1_0159_0002);
    let alphabet = b"{}[]\":,0123456789-.eEtrufalsnXLITERAMH ";
    let good = serde_json::to_value(two_face_definition().seal().expect("seal")).expect("value");
    let decode_all = |v: &Value| {
        let _ = serde_json::from_value::<ValueExpressionAST>(v.clone());
        let _ = serde_json::from_value::<AbilityAST>(v.clone());
        let _ = serde_json::from_value::<PartnerAbilityDSL>(v.clone());
        let _ = serde_json::from_value::<TextTokenStream>(v.clone());
    };
    for i in 0..3000 {
        let len = (rng.next_u64() % 40) as usize;
        let raw: Vec<u8> = (0..len)
            .map(|_| alphabet[(rng.next_u64() % alphabet.len() as u64) as usize])
            .collect();
        let Ok(v) = serde_json::from_slice::<Value>(&raw) else {
            continue;
        };
        decode_all(&v);
        let mut doc = good.clone();
        Position::ALL[i % Position::ALL.len()].replace(&mut doc, v);
        rehash(&mut doc);
        let input = json_bytes(&doc);
        if let Ok(loaded) = CardDefinitionDSL::from_json(&input) {
            assert_eq!(
                loaded.to_canonical_json().expect("export"),
                canonicalize(&input).expect("canonical")
            );
        }
    }
    // Random mutations of valid samples.
    let samples: Vec<Value> = valid_values()
        .into_iter()
        .chain(valid_abilities())
        .chain(valid_partners())
        .chain(valid_texts().into_iter().map(Value::Array))
        .collect();
    for _ in 0..2000 {
        let sample = &samples[(rng.next_u64() % samples.len() as u64) as usize];
        let mut bytes = json_bytes(sample);
        let at = (rng.next_u64() % bytes.len() as u64) as usize;
        bytes[at] = alphabet[(rng.next_u64() % alphabet.len() as u64) as usize];
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            decode_all(&v);
        }
    }
}
