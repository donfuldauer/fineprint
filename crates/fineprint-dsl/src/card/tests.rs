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
