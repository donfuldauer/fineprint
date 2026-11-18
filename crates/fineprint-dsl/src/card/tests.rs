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
        power: Some(ValueExpressionAST(json!({"const": 2}))),
        toughness: Some(ValueExpressionAST(json!({"const": 2}))),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: TextTokenStream(vec![json!({"kw": "Flying"})]),
        abilities: vec![AbilityAST(json!({"keyword": "FLYING"}))],
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
        partner_abilities: vec![PartnerAbilityDSL(json!({"kind": "test"}))],
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
        partner_abilities: vec![PartnerAbilityDSL(json!({"n": pick(5)}))],
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
                1 => Some(ValueExpressionAST(Value::Null)),
                _ => Some(ValueExpressionAST(json!([pick(9), "*"]))),
            };
            f.toughness = (pick(2) == 1).then(|| ValueExpressionAST(json!({"b": 1, "a": [true]})));
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
        partner_abilities: vec![PartnerAbilityDSL(json!({"p": 1}))],
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
    // power and toughness keep their documented `null` behaviour.
    let mut null_power = full;
    null_power["faces"][0]["power"] = Value::Null;
    let decoded: CardDefinitionDSL = serde_json::from_value(null_power).expect("power null");
    assert_eq!(
        decoded.faces[0].power,
        Some(ValueExpressionAST(Value::Null))
    );
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

/// REQ-DSL-001, REQ-NFR-006: sealing normalises placeholder numbers, so a sealed definition
/// holding `-0.0` or `1.0` loads back equal, and `1.0` and `1` load the same.
#[test]
fn req_dsl_001_seal_normalises_numbers() {
    for spelling in ["-0.0", "1.0", "1e0", "2.5"] {
        let mut def = definition();
        def.faces[0].power = Some(ValueExpressionAST(
            serde_json::from_str::<Value>(spelling).expect("number"),
        ));
        def.faces[0].abilities = vec![AbilityAST(
            serde_json::from_str(&format!("[{spelling}, {{\"n\": {spelling}}}]")).expect("ab"),
        )];
        let hash_before = def.compute_hash().expect("hash");
        let bytes_before = def.to_canonical_json().expect("json");
        let sealed = def.seal().expect("seal");
        assert_eq!(sealed.definition_hash, hash_before, "{spelling}");
        let mut unsealed_bytes = sealed.clone();
        unsealed_bytes.definition_hash = String::new();
        assert_eq!(
            unsealed_bytes.to_canonical_json().expect("json"),
            bytes_before
        );
        let exported = sealed.to_canonical_json().expect("json");
        assert_eq!(
            CardDefinitionDSL::from_json(&exported).as_ref(),
            Ok(&sealed),
            "{spelling}"
        );
    }
    let mut one = definition();
    one.faces[0].power = Some(ValueExpressionAST(json!(1)));
    let one = one.seal().expect("seal");
    let text = String::from_utf8(one.to_canonical_json().expect("json")).expect("utf8");
    assert!(text.contains("\"power\":1,"), "{text}");
    let spelled = text.replace("\"power\":1,", "\"power\":1.0,");
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

/// REQ-DSL-001, REQ-NFR-006: one depth budget for sealing, export, hashing and loading. The
/// definition object, `faces`, a face and `abilities` take four levels.
#[test]
fn req_dsl_001_definition_depth_budget() {
    use crate::canonical::MAX_DEPTH;
    let mut deepest = definition();
    deepest.faces[0].abilities = vec![AbilityAST(nested(MAX_DEPTH - 4))];
    let sealed = deepest.seal().expect("deepest allowed seals");
    let exported = sealed.to_canonical_json().expect("export");
    assert_eq!(CardDefinitionDSL::from_json(&exported), Ok(sealed));

    let too_deep = CanonicalError::Serialize(format!("nesting depth exceeds {MAX_DEPTH}"));
    for depth in [MAX_DEPTH - 3, 130] {
        let mut deeper = definition();
        deeper.faces[0].abilities = vec![AbilityAST(nested(depth))];
        assert_eq!(deeper.clone().seal(), Err(too_deep.clone()));
        assert_eq!(deeper.to_canonical_json(), Err(too_deep.clone()));
        assert_eq!(deeper.compute_hash(), Err(too_deep.clone()));
    }
}

/// REQ-DSL-001, REQ-NFR-006: a noncharacter in a definition cannot be sealed or
/// exported, and a noncharacter in the input is a Canonical error.
#[test]
fn req_nfr_006_definition_noncharacter() {
    let mut def = definition();
    def.faces[0].name = "test.\u{FFFE}".into();
    assert!(matches!(
        def.clone().seal(),
        Err(CanonicalError::Serialize(_))
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
        match pick(&mut rng, 5) {
            0 => v[&key] = Value::Null,
            1 => {
                if let Value::Object(m) = &v[&key] {
                    v[&key] = Value::Array(m.values().cloned().collect());
                }
            }
            2 => v[&key] = json!({ "x": v[&key].clone() }),
            3 => v["faces"][0]["abilities"] = json!([nested(pick(&mut rng, 140))]),
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

/// REQ-NFR-006, REQ-DSL-002, REQ-CORE-006: `compute_hash` hashes the borrowed definition, so a
/// value built in memory far beyond the depth budget is a `Serialize` error rather than a stack
/// overflow. The value is forgotten, not dropped, because dropping it would itself recurse.
#[test]
fn req_nfr_006_compute_hash_deep_value_is_an_error() {
    let mut def = definition();
    def.faces[0].abilities = vec![AbilityAST(nested(100_000))];
    assert!(matches!(
        def.compute_hash(),
        Err(CanonicalError::Serialize(_))
    ));
    assert!(matches!(
        def.verify_hash(),
        Err(CardDefinitionError::Canonical(CanonicalError::Serialize(_)))
    ));
    std::mem::forget(def);
}

/// REQ-NFR-006, REQ-DSL-002, REQ-CORE-006: `seal` on a definition nested past the budget is a
/// `Serialize` error (1 000 levels, small enough to drop safely).
#[test]
fn req_nfr_006_seal_deep_value_is_an_error() {
    let mut def = definition();
    def.faces[0].abilities = vec![AbilityAST(nested(1_000))];
    assert!(matches!(def.seal(), Err(CanonicalError::Serialize(_))));
}

/// REQ-NFR-006, REQ-DSL-002, REQ-CORE-006: only the top-level `definitionHash` is left out of
/// the hash; a nested member of that name is part of the hash input and is checked.
#[test]
fn req_nfr_006_nested_definition_hash_member_is_hashed() {
    let base = sealed();
    let mut nested_member = definition();
    nested_member.faces[0].abilities = vec![AbilityAST(json!({"definitionHash": "x"}))];
    let nested_hash = nested_member.compute_hash().expect("hash");
    assert_ne!(nested_hash, base.compute_hash().expect("hash"));
    let mut other = definition();
    other.faces[0].abilities = vec![AbilityAST(json!({"definitionHash": "y"}))];
    assert_ne!(other.compute_hash().expect("hash"), nested_hash);
    let mut bad = definition();
    bad.faces[0].abilities = vec![AbilityAST(json!({"definitionHash": "\u{FDD0}"}))];
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
