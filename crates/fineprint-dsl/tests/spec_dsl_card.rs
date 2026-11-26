// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::card`: the top-level card DSL types (SRS §14.2), `dslVersion`
//! checking (SRS §14.1.5), canonical JSON and `definitionHash` (REQ-NFR-006), and the checked
//! loader. Covers REQ-DSL-001, REQ-DSL-002, REQ-OBJ-001 and REQ-NFR-006.
//! Black-box: written from the work-item brief only. Every definition here is SYNTHETIC (R5).
//! The hash oracle is `fineprint_base::hash::sha256` over independently canonicalised bytes, and
//! one minimal definition is pinned to hand-written canonical bytes whose SHA-256 was computed
//! with `printf %s '<canonical>' | sha256sum`.
//! #159 FP-140: AST positions (power, toughness, text, abilities, partner abilities) hold valid
//! real ASTs, because `seal` and `from_json` now reject anything else; the AST validation itself
//! is tested in `spec_dsl_card_ast.rs`.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::sha256;
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::canonical::{CanonicalError, MAX_SAFE_INTEGER, canonicalize};
use fineprint_dsl::card::{
    CardDefinitionDSL, CardDefinitionError, CardFaceDSL, ColorBitmask, CommanderEligibilityDSL,
    DSL_MAJOR, DSL_VERSION, DslVersion, DslVersionError, FaceKind, LayoutBehavior, Legality,
    Origin, ProvenanceDSL, SupportStatus, check_dsl_version,
};
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol};
use serde_json::{Value, json};

mod typed_ast_support;
use typed_ast_support::{ability_of, pa, tokens_of, tx, vx};

// ---------------------------------------------------------------- synthetic definitions

fn mana(s: &str) -> ManaCostDSL {
    ManaCostDSL::parse(s).expect("valid synthetic mana cost")
}

fn bitmask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("bits <= 31")
}

fn plain_face(kind: FaceKind, name: &str) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: kind,
        name: name.to_string(),
        mana_cost: None,
        color_indicator: None,
        supertypes: vec![],
        card_types: vec![],
        subtypes: vec![],
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: tx(vec![]),
        abilities: vec![],
    }
}

/// The pinned minimal definition (see `PINNED_*` below).
fn minimal() -> CardDefinitionDSL {
    let mut face = plain_face(FaceKind::Main, "Test Vanilla");
    face.mana_cost = Some(mana("{1}{G}"));
    face.card_types = vec!["Creature".to_string()];
    // #159 FP-140: real value expressions (`{"LITERAL":1}`), formerly the bare placeholder `1`.
    face.power = Some(vx(json!({"LITERAL": 1})));
    face.toughness = Some(vx(json!({"LITERAL": 1})));
    let mut legalities = BTreeMap::new();
    legalities.insert("testformat".to_string(), Legality::Legal);
    CardDefinitionDSL {
        card_def_id: "test.vanilla_1_1".to_string(),
        dsl_version: "1.0".to_string(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "0000".to_string(),
            origin: Origin::Parser,
            parser_version: None,
            override_source: None,
            verified_by: None,
            rules_baseline: "test-baseline".to_string(),
        },
        support_status: SupportStatus::AutoParsed,
        known_gaps: None,
        layout: LayoutBehavior::Normal,
        faces: vec![face],
        meld_partner: None,
        color_identity: bitmask(16),
        commander_eligibility: None,
        legalities,
        digital_only: false,
        related_tokens: None,
    }
}

/// Canonical JSON of `minimal()` without `definitionHash`, written by hand.
const PINNED_WITHOUT_HASH: &str = concat!(
    r#"{"cardDefId":"test.vanilla_1_1","colorIdentity":16,"digitalOnly":false,"dslVersion":"1.0","#,
    r#""faces":[{"abilities":[],"cardTypes":["Creature"],"faceKind":"MAIN","manaCost":"{1}{G}","#,
    r#""name":"Test Vanilla","power":{"LITERAL":1},"subtypes":[],"supertypes":[],"text":[],"#,
    r#""toughness":{"LITERAL":1}}],"#,
    r#""layout":"NORMAL","legalities":{"testformat":"LEGAL"},"#,
    r#""source":{"oracleTextHash":"0000","origin":"PARSER","rulesBaseline":"test-baseline"},"#,
    r#""supportStatus":"AUTO_PARSED"}"#
);

/// `printf %s "$PINNED_WITHOUT_HASH" | sha256sum`. Regenerated for #159 FP-140 when the power and
/// toughness placeholders `1` became the real value expression `{"LITERAL":1}` (previously
/// `36337ac7f9d175df1da81a397b1b788a4f2c88ee12dcc1b937584e5ca34f3aa3`).
const PINNED_HASH: &str = "e930e89d0aec056f27c5a0bac081553c5435af9dbebf4b13f42d65b294d2b770";

/// Canonical JSON of the sealed `minimal()`, written by hand.
const PINNED_SEALED: &str = concat!(
    r#"{"cardDefId":"test.vanilla_1_1","colorIdentity":16,"#,
    r#""definitionHash":"e930e89d0aec056f27c5a0bac081553c5435af9dbebf4b13f42d65b294d2b770","#,
    r#""digitalOnly":false,"dslVersion":"1.0","#,
    r#""faces":[{"abilities":[],"cardTypes":["Creature"],"faceKind":"MAIN","manaCost":"{1}{G}","#,
    r#""name":"Test Vanilla","power":{"LITERAL":1},"subtypes":[],"supertypes":[],"text":[],"#,
    r#""toughness":{"LITERAL":1}}],"#,
    r#""layout":"NORMAL","legalities":{"testformat":"LEGAL"},"#,
    r#""source":{"oracleTextHash":"0000","origin":"PARSER","rulesBaseline":"test-baseline"},"#,
    r#""supportStatus":"AUTO_PARSED"}"#
);

/// A rich two-face definition with every optional member present on the definition and the
/// first face, and absent on the second face.
fn rich() -> CardDefinitionDSL {
    let mut front = plain_face(FaceKind::Main, "Test Modal Front");
    front.mana_cost = Some(mana("{2}{W}{U/P}"));
    front.color_indicator = Some(bitmask(3));
    front.supertypes = vec!["Legendary".to_string()];
    front.card_types = vec!["Creature".to_string(), "Artifact".to_string()];
    front.subtypes = vec!["Testkin".to_string()];
    // #159 FP-140: valid real content replaces the former free-form placeholders, keeping the
    // intent (a nested expression with a variable; varied token kinds and characters; nested
    // and maximal-integer ability payloads, now as opaque KEYWORD params).
    front.power = Some(vx(rich_power()));
    front.toughness = Some(vx(json!({"LITERAL": 3})));
    front.loyalty = Some(4);
    front.defense = Some(5);
    front.hand_modifier = Some(-1);
    front.life_modifier = Some(7);
    front.text = tx(rich_text());
    front.abilities = vec![
        ability_of(
            json!({"abilityId": "a1", "kind": "STATIC", "effects": [{"type": "RULE", "kind": "SKIP"}]}),
        ),
        ability_of(
            json!({"abilityId": "a2", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
            "params": {"cost": "{T}", "n": MAX_SAFE_INTEGER,
                       "nested": {"a": [1, 2, {"b": false}], "s": "é\u{1F602}"}}}),
        ),
        ability_of(
            json!({"abilityId": "a3", "kind": "ACTIVATED", "cost": [{"cost": "TAP_SOURCE"}], "effect": []}),
        ),
    ];
    let back = plain_face(FaceKind::Back, "Test Modal Back");
    let mut legalities = BTreeMap::new();
    legalities.insert("alpha".to_string(), Legality::Legal);
    legalities.insert("beta".to_string(), Legality::Banned);
    legalities.insert("gamma".to_string(), Legality::Restricted);
    legalities.insert("delta".to_string(), Legality::NotLegal);
    CardDefinitionDSL {
        card_def_id: "test.modal_pair".to_string(),
        dsl_version: "1.0".to_string(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "ab".repeat(32),
            origin: Origin::Mixed,
            parser_version: Some("0.1.0".to_string()),
            override_source: Some("test-override".to_string()),
            verified_by: Some(vec!["test.suite_a".to_string(), "test.suite_b".to_string()]),
            rules_baseline: "test-baseline".to_string(),
        },
        support_status: SupportStatus::Partial,
        known_gaps: Some(vec!["test gap one".to_string(), "test gap two".to_string()]),
        layout: LayoutBehavior::DfcModal,
        faces: vec![front, back],
        meld_partner: Some("test.meld_other".to_string()),
        color_identity: bitmask(0b10011),
        commander_eligibility: Some(CommanderEligibilityDSL {
            eligible: true,
            partner_abilities: vec![pa(rich_partner())],
        }),
        legalities,
        digital_only: true,
        related_tokens: Some(vec!["test.token_1_1".to_string()]),
    }
}

/// `rich()`'s power: a nested real value expression with a variable (#159 FP-140).
fn rich_power() -> Value {
    json!({"MATH": {"op": "ADD", "lhs": {"LITERAL": 1},
        "rhs": {"MATH": {"op": "MAX", "lhs": "X", "rhs": {"LITERAL": -12}}}}})
}

/// `rich()`'s front text: every token kind, escapes and non-ASCII characters (#159 FP-140).
fn rich_text() -> Vec<Value> {
    vec![
        json!({"id": "t1", "kind": "LITERAL", "text": "Test"}),
        json!({"id": "t2", "kind": "WORD", "category": "COLOR_WORD", "value": "Hue"}),
        json!({"id": "t3", "kind": "NAME_REF", "ref": "SELF"}),
        json!({"id": "t4", "kind": "NAME_REF", "ref": "OTHER", "name": "Test Other"}),
        json!({"id": "t5", "kind": "WORD", "category": "NUMBER", "value": "-12"}),
        json!({"id": "t6", "kind": "LITERAL", "text": "plain \"two\"\n\u{1}é\u{1F602}"}),
    ]
}

/// `rich()`'s partner ability: a real KEYWORD ability with params (#159 FP-140; formerly
/// `{"kind":"TEST_PARTNER","with":null}`, but abilities refuse nulls).
fn rich_partner() -> Value {
    json!({"abilityId": "p1", "kind": "KEYWORD", "keyword": "PARTNER",
        "params": {"with": "test.partner_other"}})
}

fn sealed(d: CardDefinitionDSL) -> CardDefinitionDSL {
    d.seal().expect("seal")
}

fn canonical_bytes(d: &CardDefinitionDSL) -> Vec<u8> {
    d.to_canonical_json().expect("to_canonical_json")
}

fn to_value(d: &CardDefinitionDSL) -> Value {
    serde_json::to_value(d).expect("serialise")
}

/// Independent hash oracle: SHA-256 of the RFC 8785 bytes of the serialised definition with the
/// `definitionHash` member removed.
fn oracle_hash(d: &CardDefinitionDSL) -> String {
    let mut v = to_value(d);
    v.as_object_mut()
        .expect("definition serialises to an object")
        .remove("definitionHash")
        .expect("definitionHash member present");
    let bytes = canonicalize(&serde_json::to_vec(&v).unwrap()).expect("canonicalize");
    sha256(&bytes).to_hex()
}

fn keys(v: &Value) -> BTreeSet<String> {
    v.as_object().expect("object").keys().cloned().collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn load(bytes: &[u8]) -> Result<CardDefinitionDSL, CardDefinitionError> {
    CardDefinitionDSL::from_json(bytes)
}

fn load_value(v: &Value) -> Result<CardDefinitionDSL, CardDefinitionError> {
    load(&serde_json::to_vec(v).unwrap())
}

fn assert_schema(r: Result<CardDefinitionDSL, CardDefinitionError>, what: &str) {
    match r {
        Err(CardDefinitionError::Schema(msg)) => {
            let e = CardDefinitionError::Schema(msg);
            assert!(!e.to_string().is_empty(), "{what}");
        }
        other => panic!("{what}: expected Schema error, got {other:?}"),
    }
}

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_u64() % n
}

fn shuffle<T>(rng: &mut Pcg64Dxsm, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = below(rng, i as u64 + 1) as usize;
        items.swap(i, j);
    }
}

/// Serialises `v` with object members in a random order and random whitespace.
fn emit_shuffled(v: &Value, rng: &mut Pcg64Dxsm, out: &mut String) {
    let ws = |rng: &mut Pcg64Dxsm, out: &mut String| {
        for _ in 0..below(rng, 3) {
            out.push([' ', '\n', '\t', '\r'][below(rng, 4) as usize]);
        }
    };
    ws(rng, out);
    match v {
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                emit_shuffled(item, rng, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            shuffle(rng, &mut entries);
            out.push('{');
            for (i, (k, val)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                ws(rng, out);
                out.push_str(&serde_json::to_string(k).unwrap());
                ws(rng, out);
                out.push(':');
                emit_shuffled(val, rng, out);
            }
            out.push('}');
        }
        other => out.push_str(&serde_json::to_string(other).unwrap()),
    }
    ws(rng, out);
}

fn shuffled_bytes(v: &Value, seed: u64) -> Vec<u8> {
    let mut rng = Pcg64Dxsm::from_u64_seed(seed);
    let mut s = String::new();
    emit_shuffled(v, &mut rng, &mut s);
    s.into_bytes()
}

// ---------------------------------------------------------------- enums

macro_rules! check_enum {
    ($ty:ty, $count:expr, [$(($variant:expr, $name:expr)),* $(,)?]) => {{
        let expected: Vec<($ty, &str)> = vec![$(($variant, $name)),*];
        assert_eq!(expected.len(), $count, "{} expected list", stringify!($ty));
        assert_eq!(<$ty>::ALL.len(), $count, "{}::ALL length", stringify!($ty));
        for (i, (variant, name)) in expected.iter().enumerate() {
            assert_eq!(<$ty>::ALL[i], *variant, "{}::ALL[{i}]", stringify!($ty));
            assert_eq!(variant.as_str(), *name);
            assert_eq!(serde_json::to_value(variant).unwrap(), Value::String(name.to_string()));
            let back: $ty = serde_json::from_value(Value::String(name.to_string())).unwrap();
            assert_eq!(back, *variant);
            let lower = name.to_lowercase();
            assert!(serde_json::from_value::<$ty>(Value::String(lower.clone())).is_err(), "{lower}");
            let spaced = format!(" {name}");
            assert!(serde_json::from_value::<$ty>(Value::String(spaced)).is_err());
        }
        let distinct: BTreeSet<&str> = <$ty>::ALL.iter().map(|v| v.as_str()).collect();
        assert_eq!(distinct.len(), $count);
        for bad in [json!("NOPE"), json!(""), json!(0), json!(null), json!(["NORMAL"])] {
            assert!(serde_json::from_value::<$ty>(bad.clone()).is_err(), "{bad}");
        }
    }};
}

/// REQ-DSL-001: every enum's ALL, as_str and serde forms agree with SRS §14.2.
#[test]
fn req_dsl_001_enums_all_as_str_serde_agree() {
    check_enum!(
        LayoutBehavior,
        25,
        [
            (LayoutBehavior::Normal, "NORMAL"),
            (LayoutBehavior::Split, "SPLIT"),
            (LayoutBehavior::Room, "ROOM"),
            (LayoutBehavior::Flip, "FLIP"),
            (LayoutBehavior::Leveler, "LEVELER"),
            (LayoutBehavior::DfcNonmodal, "DFC_NONMODAL"),
            (LayoutBehavior::DfcModal, "DFC_MODAL"),
            (LayoutBehavior::Meld, "MELD"),
            (LayoutBehavior::Saga, "SAGA"),
            (LayoutBehavior::Adventurer, "ADVENTURER"),
            (LayoutBehavior::Class, "CLASS"),
            (LayoutBehavior::Attraction, "ATTRACTION"),
            (LayoutBehavior::Prototype, "PROTOTYPE"),
            (LayoutBehavior::Case, "CASE"),
            (LayoutBehavior::Omen, "OMEN"),
            (LayoutBehavior::Station, "STATION"),
            (LayoutBehavior::Preparation, "PREPARATION"),
            (LayoutBehavior::Mutate, "MUTATE"),
            (LayoutBehavior::Token, "TOKEN"),
            (LayoutBehavior::DfcToken, "DFC_TOKEN"),
            (LayoutBehavior::Planar, "PLANAR"),
            (LayoutBehavior::Scheme, "SCHEME"),
            (LayoutBehavior::Vanguard, "VANGUARD"),
            (LayoutBehavior::Conspiracy, "CONSPIRACY"),
            (LayoutBehavior::Dungeon, "DUNGEON"),
        ]
    );
    check_enum!(
        FaceKind,
        10,
        [
            (FaceKind::Main, "MAIN"),
            (FaceKind::Back, "BACK"),
            (FaceKind::LeftHalf, "LEFT_HALF"),
            (FaceKind::RightHalf, "RIGHT_HALF"),
            (FaceKind::Adventure, "ADVENTURE"),
            (FaceKind::Omen, "OMEN"),
            (FaceKind::PrepareSpell, "PREPARE_SPELL"),
            (FaceKind::Prototype, "PROTOTYPE"),
            (FaceKind::Flipped, "FLIPPED"),
            (FaceKind::MeldResult, "MELD_RESULT"),
        ]
    );
    check_enum!(
        SupportStatus,
        4,
        [
            (SupportStatus::Verified, "VERIFIED"),
            (SupportStatus::AutoParsed, "AUTO_PARSED"),
            (SupportStatus::Partial, "PARTIAL"),
            (SupportStatus::Unsupported, "UNSUPPORTED"),
        ]
    );
    check_enum!(
        Origin,
        3,
        [
            (Origin::Parser, "PARSER"),
            (Origin::ClientOverride, "CLIENT_OVERRIDE"),
            (Origin::Mixed, "MIXED"),
        ]
    );
    check_enum!(
        Legality,
        4,
        [
            (Legality::Legal, "LEGAL"),
            (Legality::Banned, "BANNED"),
            (Legality::Restricted, "RESTRICTED"),
            (Legality::NotLegal, "NOT_LEGAL"),
        ]
    );
    // Cross-enum names are not accepted by the wrong enum.
    assert!(serde_json::from_value::<FaceKind>(json!("NORMAL")).is_err());
    assert!(serde_json::from_value::<Legality>(json!("VERIFIED")).is_err());
}

/// REQ-DSL-001 / REQ-OBJ-001: ColorBitmask holds 0..=31 and is a JSON integer.
#[test]
fn req_obj_001_color_bitmask() {
    assert_eq!(ColorBitmask::NONE.bits(), 0);
    for bits in 0u8..=31 {
        let m = ColorBitmask::new(bits).expect("in range");
        assert_eq!(m.bits(), bits);
        assert_eq!(serde_json::to_value(m).unwrap(), json!(bits));
        let back: ColorBitmask = serde_json::from_value(json!(bits)).unwrap();
        assert_eq!(back, m);
    }
    assert_eq!(ColorBitmask::new(0), Some(ColorBitmask::NONE));
    for bits in [32u8, 33, 63, 64, 128, 255] {
        assert_eq!(ColorBitmask::new(bits), None, "{bits}");
    }
    const C: Option<ColorBitmask> = ColorBitmask::new(17);
    const B: u8 = match C {
        Some(m) => m.bits(),
        None => 0,
    };
    assert_eq!(B, 17);
    for bad in ["32", "-1", "1.5", "\"3\"", "256", "null", "true", "[1]"] {
        assert!(serde_json::from_str::<ColorBitmask>(bad).is_err(), "{bad}");
    }
}

// ---------------------------------------------------------------- dslVersion

/// REQ-DSL-002: DslVersion parsing and check_dsl_version.
#[test]
fn req_dsl_002_dsl_version_table() {
    assert_eq!(DSL_VERSION, "1.0");
    assert_eq!(DSL_MAJOR, 1);
    assert_eq!(
        check_dsl_version(DSL_VERSION),
        Ok(DslVersion {
            major: 1,
            minor: 0,
            patch: 0
        })
    );
    let ok: &[(&str, (u64, u64, u64))] = &[
        ("1.0", (1, 0, 0)),
        ("1.7.3", (1, 7, 3)),
        ("1.0.0", (1, 0, 0)),
        ("1.10.20", (1, 10, 20)),
        ("1.18446744073709551615", (1, u64::MAX, 0)),
    ];
    for (s, (major, minor, patch)) in ok {
        let v = DslVersion {
            major: *major,
            minor: *minor,
            patch: *patch,
        };
        assert_eq!(DslVersion::parse(s), Ok(v), "{s}");
        assert_eq!(check_dsl_version(s), Ok(v), "{s}");
        assert_eq!(v.to_string(), format!("{major}.{minor}.{patch}"));
    }
    assert!(
        DslVersion {
            major: 1,
            minor: 2,
            patch: 0
        } < DslVersion {
            major: 1,
            minor: 10,
            patch: 0
        }
    );

    let mismatch: &[(&str, u64)] = &[
        ("0.9", 0),
        ("2.0", 2),
        ("2.0.1", 2),
        ("10.0", 10),
        ("10.0.0", 10),
        ("0.0", 0),
        ("18446744073709551615.0", u64::MAX),
    ];
    for (s, found) in mismatch {
        let e = check_dsl_version(s).unwrap_err();
        assert_eq!(
            e,
            DslVersionError::MajorMismatch {
                found: *found,
                supported: 1
            },
            "{s}"
        );
        let d = e.to_string();
        assert!(d.contains("dslVersion"), "{d}");
        assert!(d.contains(&found.to_string()), "{d}");
        assert!(d.contains('1'), "{d}");
        // The text itself is a well-formed version.
        assert!(DslVersion::parse(s).is_ok(), "{s}");
    }

    let malformed = [
        "",
        "1",
        "1.",
        ".1",
        "01.0",
        "1.00",
        "1.01",
        "1.0.00",
        "v1.0",
        "1.0-beta",
        "1.0.0.0",
        " 1.0",
        "1.0 ",
        "+1.0",
        "-1.0",
        "1.-0",
        "1..0",
        "1.0.",
        "1,0",
        "1.a",
        "١.٠",
        "１.０",
        "18446744073709551616.0",
        "1.18446744073709551616",
        "1.0.18446744073709551616",
        "99999999999999999999999.0",
    ];
    for s in malformed {
        let r = check_dsl_version(s);
        assert!(
            matches!(r, Err(DslVersionError::Malformed(_))),
            "{s:?}: {r:?}"
        );
        assert!(DslVersion::parse(s).is_err(), "{s:?}");
        let e = r.unwrap_err();
        assert!(!e.to_string().is_empty());
        let _: &dyn std::error::Error = &e;
    }
}

// ---------------------------------------------------------------- JSON shape

const TOP_REQUIRED: &[&str] = &[
    "cardDefId",
    "dslVersion",
    "definitionHash",
    "source",
    "supportStatus",
    "layout",
    "faces",
    "colorIdentity",
    "legalities",
    "digitalOnly",
];
const TOP_OPTIONAL: &[&str] = &[
    "knownGaps",
    "meldPartner",
    "commanderEligibility",
    "relatedTokens",
];
const SOURCE_REQUIRED: &[&str] = &["oracleTextHash", "origin", "rulesBaseline"];
const SOURCE_OPTIONAL: &[&str] = &["parserVersion", "overrideSource", "verifiedBy"];
const FACE_REQUIRED: &[&str] = &[
    "faceKind",
    "name",
    "supertypes",
    "cardTypes",
    "subtypes",
    "text",
    "abilities",
];
const FACE_OPTIONAL: &[&str] = &[
    "manaCost",
    "colorIndicator",
    "power",
    "toughness",
    "loyalty",
    "defense",
    "handModifier",
    "lifeModifier",
];

/// REQ-DSL-001: JSON member names are exactly the camelCase names of SRS §14.2, optional
/// members are omitted when None, and values have the documented JSON forms.
#[test]
fn req_dsl_001_json_member_names_and_omitted_optionals() {
    let all = |a: &[&str], b: &[&str]| -> BTreeSet<String> {
        a.iter().chain(b.iter()).map(|s| s.to_string()).collect()
    };
    let r = to_value(&rich());
    assert_eq!(keys(&r), all(TOP_REQUIRED, TOP_OPTIONAL));
    assert_eq!(keys(&r["source"]), all(SOURCE_REQUIRED, SOURCE_OPTIONAL));
    assert_eq!(keys(&r["faces"][0]), all(FACE_REQUIRED, FACE_OPTIONAL));
    assert_eq!(keys(&r["faces"][1]), set(FACE_REQUIRED));
    assert_eq!(
        keys(&r["commanderEligibility"]),
        set(&["eligible", "partnerAbilities"])
    );

    let m = to_value(&minimal());
    assert_eq!(keys(&m), set(TOP_REQUIRED));
    assert_eq!(keys(&m["source"]), set(SOURCE_REQUIRED));

    // Value forms.
    assert_eq!(r["cardDefId"], json!("test.modal_pair"));
    assert_eq!(r["dslVersion"], json!("1.0"));
    assert_eq!(r["supportStatus"], json!("PARTIAL"));
    assert_eq!(r["layout"], json!("DFC_MODAL"));
    assert_eq!(r["colorIdentity"], json!(19));
    assert_eq!(r["digitalOnly"], json!(true));
    assert_eq!(r["meldPartner"], json!("test.meld_other"));
    assert_eq!(r["knownGaps"], json!(["test gap one", "test gap two"]));
    assert_eq!(r["relatedTokens"], json!(["test.token_1_1"]));
    assert_eq!(
        r["legalities"],
        json!({"alpha": "LEGAL", "beta": "BANNED", "gamma": "RESTRICTED", "delta": "NOT_LEGAL"})
    );
    assert_eq!(r["source"]["origin"], json!("MIXED"));
    assert_eq!(
        r["source"]["verifiedBy"],
        json!(["test.suite_a", "test.suite_b"])
    );
    assert_eq!(r["commanderEligibility"]["eligible"], json!(true));
    assert_eq!(
        r["commanderEligibility"]["partnerAbilities"],
        json!([rich_partner()])
    );
    let f = &r["faces"][0];
    assert_eq!(f["faceKind"], json!("MAIN"));
    assert_eq!(f["manaCost"], json!("{2}{W}{U/P}"));
    assert_eq!(f["colorIndicator"], json!(3));
    assert_eq!(f["power"], rich_power());
    assert_eq!(f["toughness"], json!({"LITERAL": 3}));
    assert_eq!(f["loyalty"], json!(4));
    assert_eq!(f["defense"], json!(5));
    assert_eq!(f["handModifier"], json!(-1));
    assert_eq!(f["lifeModifier"], json!(7));
    assert_eq!(f["text"][0], rich_text()[0]);
    assert_eq!(f["text"], Value::Array(rich_text()));
    assert_eq!(f["text"].as_array().unwrap().len(), 6);
    assert_eq!(f["abilities"][1]["params"]["n"], json!(MAX_SAFE_INTEGER));
    assert_eq!(r["faces"][1]["faceKind"], json!("BACK"));
    assert_eq!(r["faces"][1]["text"], json!([]));
}

/// REQ-DSL-001: every struct rejects unknown members and requires its required members, even
/// through plain serde.
#[test]
fn req_dsl_001_plain_serde_rejects_unknown_and_missing_members() {
    let base = to_value(&rich());
    let paths: &[&[&str]] = &[
        &[],
        &["source"],
        &["faces", "0"],
        &["faces", "1"],
        &["commanderEligibility"],
    ];
    for path in paths {
        let mut v = base.clone();
        let mut node = &mut v;
        for p in *path {
            node = match p.parse::<usize>() {
                Ok(i) => &mut node[i],
                Err(_) => &mut node[*p],
            };
        }
        node.as_object_mut()
            .unwrap()
            .insert("unknownMember".into(), json!(1));
        assert!(
            serde_json::from_value::<CardDefinitionDSL>(v).is_err(),
            "{path:?}"
        );
    }
    for k in TOP_REQUIRED {
        let mut v = base.clone();
        v.as_object_mut().unwrap().remove(*k);
        assert!(
            serde_json::from_value::<CardDefinitionDSL>(v).is_err(),
            "missing {k}"
        );
    }
    // Snake-case spellings are not accepted.
    let mut v = base.clone();
    let obj = v.as_object_mut().unwrap();
    let id = obj.remove("cardDefId").unwrap();
    obj.insert("card_def_id".into(), id);
    assert!(serde_json::from_value::<CardDefinitionDSL>(v).is_err());
}

// ---------------------------------------------------------------- round trips

/// REQ-DSL-001 (acceptance: "round-trip + canonical-form stability"): value -> JSON -> value is
/// the identity for the rich and minimal definitions, sealed or not.
#[test]
fn req_dsl_001_round_trip_and_canonical_form_stability_fixed() {
    for d in [rich(), minimal(), sealed(rich()), sealed(minimal())] {
        let v = to_value(&d);
        let back: CardDefinitionDSL = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(back, d);
        let bytes = serde_json::to_vec(&d).unwrap();
        let back: CardDefinitionDSL = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, d);
        let pretty = serde_json::to_vec_pretty(&d).unwrap();
        let back: CardDefinitionDSL = serde_json::from_slice(&pretty).unwrap();
        assert_eq!(back, d);
        // Canonical-form stability.
        let c = canonical_bytes(&d);
        assert_eq!(canonicalize(&c).unwrap(), c);
        assert_eq!(c, canonicalize(&bytes).unwrap());
        assert_eq!(c, canonicalize(&pretty).unwrap());
        assert_eq!(canonical_bytes(&back), c);
    }
    let s = sealed(rich());
    assert_eq!(load(&canonical_bytes(&s)).unwrap(), s);
}

// ---------------------------------------------------------------- hash

/// REQ-NFR-006 / REQ-DSL-001: pinned canonical bytes and SHA-256 for a minimal definition.
#[test]
fn req_nfr_006_pinned_minimal_canonical_json_and_hash() {
    let d = minimal();
    assert_eq!(d.compute_hash().unwrap(), PINNED_HASH);
    assert_eq!(oracle_hash(&d), PINNED_HASH);
    assert_eq!(sha256(PINNED_WITHOUT_HASH.as_bytes()).to_hex(), PINNED_HASH);
    let mut with_empty_hash = d.clone();
    with_empty_hash.definition_hash = String::new();
    let s = sealed(d);
    assert_eq!(s.definition_hash, PINNED_HASH);
    assert_eq!(
        String::from_utf8(canonical_bytes(&s)).unwrap(),
        PINNED_SEALED
    );
    assert_eq!(s.verify_hash(), Ok(()));
    assert_eq!(load(PINNED_SEALED.as_bytes()).unwrap(), s);
    // Removing the hash member from the sealed canonical bytes leaves exactly the hashed bytes.
    let mut v: Value = serde_json::from_str(PINNED_SEALED).unwrap();
    v.as_object_mut().unwrap().remove("definitionHash");
    assert_eq!(
        canonicalize(&serde_json::to_vec(&v).unwrap()).unwrap(),
        PINNED_WITHOUT_HASH.as_bytes()
    );
}

/// REQ-NFR-006: compute_hash is the SHA-256 of the RFC 8785 bytes without definitionHash,
/// independent of the current definition_hash; seal sets it; verify_hash checks it exactly.
#[test]
fn req_nfr_006_hash_independent_of_current_value_and_seal_verify() {
    for base in [rich(), minimal()] {
        let expected = oracle_hash(&base);
        assert_eq!(expected.len(), 64);
        assert!(
            expected
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
        for h in [
            "",
            "x",
            "garbage \u{1F602}",
            &"f".repeat(64),
            &expected.to_uppercase(),
        ] {
            let mut d = base.clone();
            d.definition_hash = h.to_string();
            assert_eq!(d.compute_hash().unwrap(), expected, "definition_hash {h:?}");
            if h != expected {
                assert!(matches!(
                    d.verify_hash(),
                    Err(CardDefinitionError::HashMismatch { .. })
                ));
            }
        }
        let s = sealed(base.clone());
        assert_eq!(s.definition_hash, expected);
        assert_eq!(s.verify_hash(), Ok(()));
        assert_eq!(sealed(s.clone()), s, "sealing twice is stable");
        // Uppercase hex is a mismatch.
        let mut up = s.clone();
        up.definition_hash = s.definition_hash.to_uppercase();
        assert_ne!(up.definition_hash, s.definition_hash);
        match up.verify_hash() {
            Err(CardDefinitionError::HashMismatch { declared, computed }) => {
                assert_eq!(declared, up.definition_hash);
                assert_eq!(computed, expected);
            }
            other => panic!("expected HashMismatch, got {other:?}"),
        }
    }
}

/// REQ-NFR-006 / REQ-DSL-001: changing any one field family changes the hash.
#[test]
fn req_nfr_006_every_field_family_affects_hash() {
    type Tamper = fn(&mut CardDefinitionDSL);
    let tampers: &[(&str, Tamper)] = &[
        ("cardDefId", |d| d.card_def_id.push('x')),
        ("dslVersion", |d| d.dsl_version = "1.1".into()),
        ("source.oracleTextHash", |d| {
            d.source.oracle_text_hash = "cd".repeat(32)
        }),
        ("source.origin", |d| {
            d.source.origin = Origin::ClientOverride
        }),
        ("source.parserVersion", |d| d.source.parser_version = None),
        ("source.overrideSource", |d| {
            d.source.override_source = Some("other".into())
        }),
        ("source.verifiedBy", |d| d.source.verified_by = Some(vec![])),
        ("source.rulesBaseline", |d| {
            d.source.rules_baseline.push('2')
        }),
        ("supportStatus", |d| {
            d.support_status = SupportStatus::Verified
        }),
        ("knownGaps", |d| d.known_gaps = None),
        ("layout", |d| d.layout = LayoutBehavior::DfcNonmodal),
        ("faces order", |d| d.faces.reverse()),
        ("faces removed", |d| {
            d.faces.pop();
        }),
        ("faceKind", |d| d.faces[1].face_kind = FaceKind::Flipped),
        ("name", |d| d.faces[0].name.push(' ')),
        ("manaCost", |d| {
            d.faces[0].mana_cost = Some(mana("{2}{W}{U/P}{S}"))
        }),
        ("manaCost removed", |d| d.faces[0].mana_cost = None),
        ("colorIndicator", |d| {
            d.faces[0].color_indicator = Some(bitmask(4))
        }),
        ("supertypes", |d| d.faces[0].supertypes.clear()),
        ("cardTypes", |d| d.faces[0].card_types.swap(0, 1)),
        ("subtypes", |d| d.faces[1].subtypes.push("Testkin".into())),
        ("power", |d| {
            d.faces[0].power = Some(vx(json!({"LITERAL": 2})))
        }),
        ("toughness", |d| d.faces[0].toughness = None),
        ("loyalty", |d| d.faces[0].loyalty = Some(5)),
        ("defense", |d| d.faces[0].defense = Some(-5)),
        ("handModifier", |d| d.faces[0].hand_modifier = Some(0)),
        ("lifeModifier", |d| d.faces[0].life_modifier = None),
        ("text", |d| {
            let mut t = tokens_of(&d.faces[0].text);
            t.push(json!({"id": "t7", "kind": "LITERAL", "text": "more"}));
            d.faces[0].text = tx(t);
        }),
        ("text nested", |d| {
            let mut t = tokens_of(&d.faces[0].text);
            t[0] = json!({"id": "t1", "kind": "LITERAL", "text": "Tesu"});
            d.faces[0].text = tx(t);
        }),
        ("abilities", |d| {
            d.faces[0].abilities.pop().map(|_| ()).unwrap_or(())
        }),
        ("ability payload", |d| {
            d.faces[0].abilities[0] =
                ability_of(json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "TEST_OTHER"}))
        }),
        ("meldPartner", |d| d.meld_partner = None),
        ("colorIdentity", |d| d.color_identity = ColorBitmask::NONE),
        ("commanderEligibility.eligible", |d| {
            d.commander_eligibility.as_mut().unwrap().eligible = false
        }),
        ("partnerAbilities", |d| {
            d.commander_eligibility
                .as_mut()
                .unwrap()
                .partner_abilities
                .clear()
        }),
        ("commanderEligibility removed", |d| {
            d.commander_eligibility = None
        }),
        ("legalities value", |d| {
            d.legalities.insert("alpha".into(), Legality::Banned);
        }),
        ("legalities key", |d| {
            d.legalities.insert("epsilon".into(), Legality::Legal);
        }),
        ("digitalOnly", |d| d.digital_only = false),
        ("relatedTokens", |d| d.related_tokens = Some(vec![])),
    ];
    let s = sealed(rich());
    let mut seen = BTreeSet::new();
    seen.insert(s.definition_hash.clone());
    for (what, f) in tampers {
        let mut d = s.clone();
        f(&mut d);
        assert_ne!(d, s, "{what}: tamper had no effect");
        match d.verify_hash() {
            Err(CardDefinitionError::HashMismatch { declared, computed }) => {
                assert_eq!(declared, s.definition_hash, "{what}");
                assert_eq!(computed, oracle_hash(&d), "{what}");
                assert!(
                    seen.insert(computed),
                    "{what}: hash collides with another variant"
                );
            }
            other => panic!("{what}: expected HashMismatch, got {other:?}"),
        }
    }
}

/// REQ-OBJ-001 / REQ-NFR-006: "no mana cost" and {0} hash differently, and so do {0} and {W}.
#[test]
fn req_obj_001_no_mana_cost_vs_zero_changes_hash() {
    let mut none = minimal();
    none.faces[0].mana_cost = None;
    let mut zero = minimal();
    zero.faces[0].mana_cost = Some(mana("{0}"));
    let mut white = minimal();
    white.faces[0].mana_cost = Some(mana("{W}"));
    let h_none = none.compute_hash().unwrap();
    let h_zero = zero.compute_hash().unwrap();
    let h_white = white.compute_hash().unwrap();
    assert_ne!(h_none, h_zero);
    assert_ne!(h_zero, h_white);
    assert_ne!(h_none, h_white);
    assert!(to_value(&none)["faces"][0].get("manaCost").is_none());
    assert_eq!(to_value(&zero)["faces"][0]["manaCost"], json!("{0}"));
    assert_eq!(
        zero.faces[0].mana_cost.as_ref().unwrap().symbols(),
        &[ManaSymbol::Generic(0)][..]
    );
    for d in [none, zero, white] {
        let s = sealed(d);
        assert_eq!(load(&canonical_bytes(&s)).unwrap(), s);
    }
}

// ---------------------------------------------------------------- loader: accepted inputs

/// REQ-DSL-001 / REQ-NFR-006: from_json accepts canonical, pretty-printed and shuffled forms of
/// the same sealed definition, yielding equal values and equal hashes.
#[test]
fn req_dsl_001_from_json_ignores_member_order_and_whitespace() {
    for d in [sealed(rich()), sealed(minimal())] {
        let canonical = canonical_bytes(&d);
        let pretty = serde_json::to_vec_pretty(&d).unwrap();
        let v = to_value(&d);
        let mut forms = vec![canonical.clone(), pretty];
        for seed in 0..8 {
            forms.push(shuffled_bytes(&v, seed));
        }
        for (i, f) in forms.iter().enumerate() {
            let loaded = load(f).unwrap_or_else(|e| panic!("form {i}: {e:?}"));
            assert_eq!(loaded, d, "form {i}");
            assert_eq!(
                loaded.compute_hash().unwrap(),
                d.definition_hash,
                "form {i}"
            );
            assert_eq!(canonical_bytes(&loaded), canonical, "form {i}");
            assert_eq!(canonicalize(f).unwrap(), canonical, "form {i}");
        }
    }
}

/// REQ-DSL-002: a minor version bump with the same shape loads (hash over the document as given).
#[test]
fn req_dsl_002_minor_version_bump_loads() {
    for ver in ["1.5", "1.0.0", "1.7.3", "1.99"] {
        let mut d = rich();
        d.dsl_version = ver.to_string();
        let s = sealed(d);
        assert_eq!(load(&canonical_bytes(&s)).unwrap(), s, "{ver}");
    }
}

/// REQ-DSL-001 / REQ-NFR-006: integers up to 2^53-1 are accepted in typed fields, larger ones
/// are rejected as unsafe integers.
#[test]
fn req_nfr_006_typed_integer_limits() {
    let mut d = rich();
    d.faces[0].loyalty = Some(MAX_SAFE_INTEGER);
    d.faces[0].defense = Some(-MAX_SAFE_INTEGER);
    let s = sealed(d);
    assert_eq!(load(&canonical_bytes(&s)).unwrap(), s);

    let mut v = to_value(&sealed(rich()));
    v["faces"][0]["loyalty"] = json!(9_007_199_254_740_992_i64);
    match load_value(&v) {
        Err(CardDefinitionError::Canonical(CanonicalError::UnsafeInteger(_))) => {}
        other => panic!("expected Canonical(UnsafeInteger), got {other:?}"),
    }
    v["faces"][0]["loyalty"] = json!(-9_007_199_254_740_992_i64);
    assert!(matches!(
        load_value(&v),
        Err(CardDefinitionError::Canonical(
            CanonicalError::UnsafeInteger(_)
        ))
    ));
    // Also inside an AST position (formerly a free-form ability payload; #159 FP-140 keeps the
    // case on a real KEYWORD ability's opaque params and on a LITERAL power). The document-level
    // check runs before the AST check, so the error stays Canonical(UnsafeInteger).
    let mut v = to_value(&sealed(rich()));
    v["faces"][0]["abilities"][1]["params"]["big"] = json!(u64::MAX);
    assert!(matches!(
        load_value(&v),
        Err(CardDefinitionError::Canonical(
            CanonicalError::UnsafeInteger(_)
        ))
    ));
    for n in [9_007_199_254_740_992_i64, -9_007_199_254_740_992] {
        let mut v = to_value(&sealed(rich()));
        v["faces"][0]["power"] = json!({ "LITERAL": n });
        assert!(
            matches!(
                load_value(&v),
                Err(CardDefinitionError::Canonical(
                    CanonicalError::UnsafeInteger(_)
                ))
            ),
            "LITERAL {n}"
        );
    }
    // The largest safe LITERAL is accepted in power and toughness.
    let mut d = rich();
    d.faces[0].power = Some(vx(json!({ "LITERAL": MAX_SAFE_INTEGER })));
    d.faces[0].toughness = Some(vx(json!({ "LITERAL": -MAX_SAFE_INTEGER })));
    let s = sealed(d);
    assert_eq!(load(&canonical_bytes(&s)).unwrap(), s);
    let mut v = to_value(&sealed(rich()));
    v["faces"][0]["abilities"][1]["params"]["big"] = json!(u64::MAX);
    assert!(matches!(
        load_value(&v),
        Err(CardDefinitionError::Canonical(
            CanonicalError::UnsafeInteger(_)
        ))
    ));
    // Typed values beyond the safe range cannot be canonicalised.
    let mut d = rich();
    d.faces[0].loyalty = Some(MAX_SAFE_INTEGER + 1);
    assert!(matches!(
        d.to_canonical_json(),
        Err(CanonicalError::UnsafeInteger(_))
    ));
    assert!(d.compute_hash().is_err());
}

// ---------------------------------------------------------------- loader: rejections

fn insert_after(haystack: &[u8], needle: &str, insert: &str) -> Vec<u8> {
    let s = std::str::from_utf8(haystack).unwrap();
    let pos = s
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} not in {s}"))
        + needle.len();
    format!("{}{}{}", &s[..pos], insert, &s[pos..]).into_bytes()
}

/// REQ-DSL-001: duplicate keys anywhere are Canonical(DuplicateKey).
#[test]
fn req_dsl_001_from_json_duplicate_keys() {
    let c = canonical_bytes(&sealed(rich()));
    let cases = [
        insert_after(&c, "{", r#""cardDefId":"test.other","#),
        insert_after(&c, r#""source":{"#, r#""origin":"PARSER","#),
        insert_after(&c, r#""faces":[{"#, r#""name":"Test Other","#),
        insert_after(&c, r#""legalities":{"#, r#""alpha":"LEGAL","#),
        insert_after(&c, r#""partnerAbilities":[{"#, r#""kind":"X","#),
        insert_after(&c, "{", r#""cardDefId":"test.modal_pair","#),
    ];
    for (i, doc) in cases.iter().enumerate() {
        match load(doc) {
            Err(CardDefinitionError::Canonical(CanonicalError::DuplicateKey(_))) => {}
            other => panic!("case {i}: expected Canonical(DuplicateKey), got {other:?}"),
        }
    }
}

/// REQ-DSL-001: unknown members are Schema errors at every level.
#[test]
fn req_dsl_001_from_json_unknown_members() {
    let c = canonical_bytes(&sealed(rich()));
    let cases = [
        insert_after(&c, "{", r#""zzzUnknown":1,"#),
        insert_after(&c, r#""source":{"#, r#""extra":true,"#),
        insert_after(&c, r#""faces":[{"#, r#""extra":"x","#),
        insert_after(&c, r#""commanderEligibility":{"#, r#""extra":null,"#),
    ];
    for (i, doc) in cases.iter().enumerate() {
        assert_schema(load(doc), &format!("unknown member case {i}"));
    }
}

/// REQ-DSL-001: each missing required member is a Schema error.
#[test]
fn req_dsl_001_from_json_missing_required_members() {
    let base = to_value(&sealed(rich()));
    for k in TOP_REQUIRED {
        let mut v = base.clone();
        v.as_object_mut().unwrap().remove(*k);
        assert_schema(load_value(&v), &format!("missing top-level {k}"));
    }
    for k in SOURCE_REQUIRED {
        let mut v = base.clone();
        v["source"].as_object_mut().unwrap().remove(*k);
        assert_schema(load_value(&v), &format!("missing source.{k}"));
    }
    for k in FACE_REQUIRED {
        for face in 0..2 {
            let mut v = base.clone();
            v["faces"][face].as_object_mut().unwrap().remove(*k);
            assert_schema(load_value(&v), &format!("missing faces[{face}].{k}"));
        }
    }
    for k in ["eligible", "partnerAbilities"] {
        let mut v = base.clone();
        v["commanderEligibility"].as_object_mut().unwrap().remove(k);
        assert_schema(load_value(&v), &format!("missing commanderEligibility.{k}"));
    }
}

/// REQ-DSL-001: wrong member types are Schema errors.
#[test]
fn req_dsl_001_from_json_wrong_types() {
    let base = to_value(&sealed(rich()));
    let cases: &[(&[&str], Value)] = &[
        (&["cardDefId"], json!(5)),
        (&["definitionHash"], json!(null)),
        (&["digitalOnly"], json!("no")),
        (&["colorIdentity"], json!(32)),
        (&["colorIdentity"], json!(-1)),
        (&["colorIdentity"], json!("3")),
        (&["faces"], json!({})),
        (&["layout"], json!("normal")),
        (&["layout"], json!("UNKNOWN_LAYOUT")),
        (&["supportStatus"], json!(1)),
        (&["legalities"], json!({"alpha": "OK"})),
        (&["legalities"], json!([])),
        (&["knownGaps"], json!("gap")),
        (&["source"], json!("PARSER")),
        (&["source", "origin"], json!("parser")),
        (&["source", "verifiedBy"], json!([1])),
        (&["faces", "0", "loyalty"], json!("3")),
        (&["faces", "0", "manaCost"], json!("{Q}")),
        (&["faces", "0", "manaCost"], json!("")),
        (&["faces", "0", "manaCost"], json!(0)),
        (&["faces", "0", "colorIndicator"], json!(40)),
        (&["faces", "0", "faceKind"], json!("FRONT")),
        (&["faces", "0", "supertypes"], json!("Legendary")),
        (&["faces", "0", "text"], json!({"t": 1})),
        (&["faces", "0", "abilities"], json!({})),
        (&["commanderEligibility", "eligible"], json!(1)),
        (&["commanderEligibility", "partnerAbilities"], json!(null)),
    ];
    for (path, bad) in cases {
        let mut v = base.clone();
        let mut node = &mut v;
        for p in *path {
            node = match p.parse::<usize>() {
                Ok(i) => &mut node[i],
                Err(_) => &mut node[*p],
            };
        }
        *node = bad.clone();
        assert_schema(load_value(&v), &format!("{path:?} = {bad}"));
    }
}

/// REQ-DSL-002 (acceptance: "different major dslVersion rejected with a clear error"):
/// SRS §14.1.5, the engine rejects definitions whose major version doesn't match its own.
#[test]
fn req_dsl_002_different_major_dsl_version_rejected_with_clear_error() {
    for (ver, found) in [("2.0", 2u64), ("0.9", 0), ("10.0.0", 10), ("2.0.1", 2)] {
        // Round-2 contract (REQ-DSL-001): `seal` rejects a version `from_json` would reject.
        let mut d = rich();
        d.dsl_version = ver.to_string();
        assert!(
            matches!(d.seal(), Err(CanonicalError::Serialize(_))),
            "{ver}: seal must reject"
        );
        // Build the document at the value level: patch a sealed 1.x document and recompute the
        // digest with the independent oracle, so the version check alone must reject it.
        let mut v = to_value(&sealed(rich()));
        v["dslVersion"] = json!(ver);
        v.as_object_mut().unwrap().remove("definitionHash");
        let digest = sha256(&canonicalize(&serde_json::to_vec(&v).unwrap()).unwrap()).to_hex();
        v["definitionHash"] = json!(digest);
        let docs = [
            canonicalize(&serde_json::to_vec(&v).unwrap()).unwrap(),
            // Hash also wrong: the version error wins over HashMismatch.
            {
                let mut v = v.clone();
                v["definitionHash"] = json!("0".repeat(64));
                serde_json::to_vec(&v).unwrap()
            },
            // Unknown shape: the version is checked before the rest is decoded.
            format!(r#"{{"dslVersion":"{ver}","somethingNew":true}}"#).into_bytes(),
            format!(r#"{{"somethingNew":[1,2],"dslVersion":"{ver}","faces":"x"}}"#).into_bytes(),
        ];
        for (i, doc) in docs.iter().enumerate() {
            match load(doc) {
                Err(CardDefinitionError::Version(e)) => {
                    assert_eq!(
                        e,
                        DslVersionError::MajorMismatch {
                            found,
                            supported: 1
                        },
                        "{ver} doc {i}"
                    );
                    let inner = e.to_string();
                    let outer = CardDefinitionError::Version(e).to_string();
                    for text in [inner.as_str(), outer.as_str()] {
                        assert!(text.contains("dslVersion"), "{text}");
                        assert!(text.contains(&found.to_string()), "{text}");
                        assert!(text.contains('1'), "{text}");
                    }
                }
                other => panic!("{ver} doc {i}: expected Version(MajorMismatch), got {other:?}"),
            }
        }
    }
}

/// REQ-DSL-002: missing or non-string dslVersion is Schema; a malformed string is Version(Malformed).
#[test]
fn req_dsl_002_dsl_version_missing_or_malformed() {
    let base = to_value(&sealed(rich()));
    let mut v = base.clone();
    v.as_object_mut().unwrap().remove("dslVersion");
    assert_schema(load_value(&v), "missing dslVersion");
    for bad in [
        json!(1),
        json!(null),
        json!(["1.0"]),
        json!({"major": 1}),
        json!(true),
    ] {
        let mut v = base.clone();
        v["dslVersion"] = bad.clone();
        assert_schema(load_value(&v), &format!("dslVersion = {bad}"));
    }
    assert_schema(
        load(br#"{"somethingNew":true}"#),
        "no dslVersion, unknown shape",
    );
    assert_schema(
        load(br#"{"dslVersion":2}"#),
        "integer dslVersion, unknown shape",
    );
    for bad in ["1.x", "", "v1.0", "1", "01.0", "1.0-beta"] {
        let mut v = base.clone();
        v["dslVersion"] = json!(bad);
        match load_value(&v) {
            Err(CardDefinitionError::Version(DslVersionError::Malformed(_))) => {}
            other => panic!("dslVersion {bad:?}: expected Version(Malformed), got {other:?}"),
        }
    }
}

/// REQ-NFR-006: a tampered hash is HashMismatch with the declared and computed values.
#[test]
fn req_nfr_006_from_json_tampered_hash() {
    let s = sealed(rich());
    for declared in [
        "0".repeat(64),
        s.definition_hash.to_uppercase(),
        String::new(),
        "x".into(),
    ] {
        let mut v = to_value(&s);
        v["definitionHash"] = json!(declared);
        match load_value(&v) {
            Err(CardDefinitionError::HashMismatch {
                declared: got_declared,
                computed,
            }) => {
                assert_eq!(got_declared, declared);
                assert_eq!(computed, s.definition_hash);
                let e = CardDefinitionError::HashMismatch {
                    declared: got_declared,
                    computed,
                };
                assert!(!e.to_string().is_empty());
            }
            other => panic!("declared {declared:?}: expected HashMismatch, got {other:?}"),
        }
    }
    // A content change without resealing is a mismatch too.
    let mut v = to_value(&s);
    v["faces"][0]["name"] = json!("Test Modal Changed");
    assert!(matches!(
        load_value(&v),
        Err(CardDefinitionError::HashMismatch { .. })
    ));
}

/// REQ-DSL-001: syntactically invalid and non-object documents are rejected.
#[test]
fn req_dsl_001_from_json_invalid_documents() {
    assert!(matches!(load(b""), Err(CardDefinitionError::Canonical(_))));
    assert!(matches!(load(b"{"), Err(CardDefinitionError::Canonical(_))));
    let c = canonical_bytes(&sealed(minimal()));
    let mut trailing = c.clone();
    trailing.extend_from_slice(b" x");
    assert!(matches!(
        load(&trailing),
        Err(CardDefinitionError::Canonical(_))
    ));
    assert!(matches!(
        load(&c[..c.len() - 1]),
        Err(CardDefinitionError::Canonical(_))
    ));
    for doc in ["[]", "[{}]", "\"1.0\"", "null", "1", "true", "{}"] {
        let r = load(doc.as_bytes());
        assert!(r.is_err(), "{doc}");
        let e = r.unwrap_err();
        assert!(!e.to_string().is_empty());
        let _: &dyn std::error::Error = &e;
    }
}

// ---------------------------------------------------------------- property tests

const STRING_POOL: &[char] = &[
    'a',
    'z',
    'T',
    '_',
    '.',
    ' ',
    '0',
    '7',
    '"',
    '\\',
    '/',
    '\n',
    '\u{1}',
    '\u{7f}',
    'é',
    'あ',
    '\u{FB33}',
    '\u{1F602}',
];

fn random_string(rng: &mut Pcg64Dxsm, max_len: u64) -> String {
    let n = below(rng, max_len + 1);
    (0..n)
        .map(|_| STRING_POOL[below(rng, STRING_POOL.len() as u64) as usize])
        .collect()
}

fn random_int(rng: &mut Pcg64Dxsm) -> i64 {
    match below(rng, 5) {
        0 => MAX_SAFE_INTEGER,
        1 => -MAX_SAFE_INTEGER,
        2 => below(rng, 21) as i64 - 10,
        _ => (below(rng, 2 * MAX_SAFE_INTEGER as u64 + 1) as i64) - MAX_SAFE_INTEGER,
    }
}

fn random_json(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    let kinds = if depth == 0 { 4 } else { 6 };
    match below(rng, kinds) {
        0 => Value::Null,
        1 => Value::Bool(below(rng, 2) == 0),
        2 => json!(random_int(rng)),
        3 => Value::String(random_string(rng, 6)),
        4 => Value::Array(
            (0..below(rng, 4))
                .map(|_| random_json(rng, depth - 1))
                .collect(),
        ),
        _ => {
            let mut m = serde_json::Map::new();
            for _ in 0..below(rng, 4) {
                m.insert(random_string(rng, 4), random_json(rng, depth - 1));
            }
            Value::Object(m)
        }
    }
}

/// `random_json` with every `null` replaced by `false` (ability params refuse nulls).
fn without_nulls(v: Value) -> Value {
    match v {
        Value::Null => Value::Bool(false),
        Value::Array(items) => Value::Array(items.into_iter().map(without_nulls).collect()),
        Value::Object(m) => {
            Value::Object(m.into_iter().map(|(k, x)| (k, without_nulls(x))).collect())
        }
        other => other,
    }
}

/// A random real value expression: LITERAL, X or MATH (#159 FP-140; formerly random JSON).
fn random_value_expr(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    const OPS: [&str; 7] = ["ADD", "SUB", "MUL", "DIV_FLOOR", "DIV_CEIL", "MIN", "MAX"];
    match below(rng, if depth == 0 { 2 } else { 3 }) {
        0 => json!({ "LITERAL": random_int(rng) }),
        1 => json!("X"),
        _ => {
            let op = OPS[below(rng, 7) as usize];
            json!({"MATH": {"op": op, "lhs": random_value_expr(rng, depth - 1),
                "rhs": random_value_expr(rng, depth - 1)}})
        }
    }
}

/// A random real token stream with unique ids (#159 FP-140; formerly random JSON elements).
fn random_text(rng: &mut Pcg64Dxsm) -> Vec<Value> {
    const CATEGORIES: [&str; 5] = [
        "COLOR_WORD",
        "BASIC_LAND_TYPE",
        "CREATURE_TYPE",
        "NUMBER",
        "CARD_NAME",
    ];
    (0..below(rng, 4))
        .map(|i| {
            let id = format!("t{i}");
            let s = format!("T{}", random_string(rng, 6));
            match below(rng, 4) {
                0 => json!({"id": id, "kind": "LITERAL", "text": s}),
                1 => json!({"id": id, "kind": "WORD",
                    "category": CATEGORIES[below(rng, 5) as usize], "value": s}),
                2 => json!({"id": id, "kind": "NAME_REF", "ref": "SELF"}),
                _ => json!({"id": id, "kind": "NAME_REF", "ref": "OTHER", "name": s}),
            }
        })
        .collect()
}

/// A random real KEYWORD ability; its opaque params carry random (null-free) JSON, keeping the
/// former "exotic placeholder JSON" coverage (#159 FP-140).
fn random_keyword(rng: &mut Pcg64Dxsm, id: &str, keyword: &str) -> Value {
    let mut a = json!({"abilityId": id, "kind": "KEYWORD", "keyword": keyword});
    if below(rng, 2) == 0 {
        let mut params = serde_json::Map::new();
        for i in 0..below(rng, 3) {
            params.insert(format!("p{i}"), without_nulls(random_json(rng, 2)));
        }
        a["params"] = Value::Object(params);
    }
    a
}

fn random_strings(rng: &mut Pcg64Dxsm) -> Vec<String> {
    (0..below(rng, 4)).map(|_| random_string(rng, 8)).collect()
}

fn maybe<T>(rng: &mut Pcg64Dxsm, f: impl FnOnce(&mut Pcg64Dxsm) -> T) -> Option<T> {
    if below(rng, 2) == 0 {
        Some(f(rng))
    } else {
        None
    }
}

fn random_cost(rng: &mut Pcg64Dxsm) -> ManaCostDSL {
    let colors = [
        Color::White,
        Color::Blue,
        Color::Black,
        Color::Red,
        Color::Green,
    ];
    let n = 1 + below(rng, 5);
    let mut v = Vec::new();
    for _ in 0..n {
        let a = colors[below(rng, 5) as usize];
        let b =
            colors[(below(rng, 4) as usize + 1 + colors.iter().position(|c| *c == a).unwrap()) % 5];
        v.push(match below(rng, 6) {
            0 => ManaSymbol::Generic(below(rng, 20) as u32),
            1 => ManaSymbol::Colored(a),
            2 => ManaSymbol::Hybrid(a, b),
            3 => ManaSymbol::Phyrexian(a),
            4 => ManaSymbol::GenericHybrid(1 + below(rng, 5) as u32, a),
            _ => ManaSymbol::Colorless,
        });
    }
    ManaCostDSL::new(v).unwrap()
}

fn random_face(rng: &mut Pcg64Dxsm) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: FaceKind::ALL[below(rng, FaceKind::ALL.len() as u64) as usize],
        name: format!("Test {}", random_string(rng, 6)),
        mana_cost: maybe(rng, random_cost),
        color_indicator: maybe(rng, |r| bitmask(below(r, 32) as u8)),
        supertypes: random_strings(rng),
        card_types: random_strings(rng),
        subtypes: random_strings(rng),
        power: maybe(rng, |r| vx(random_value_expr(r, 3))),
        toughness: maybe(rng, |r| vx(random_value_expr(r, 3))),
        loyalty: maybe(rng, random_int),
        defense: maybe(rng, random_int),
        hand_modifier: maybe(rng, random_int),
        life_modifier: maybe(rng, random_int),
        text: tx(random_text(rng)),
        abilities: (0..below(rng, 3))
            .map(|i| ability_of(random_keyword(rng, &format!("a{i}"), "TEST_KEYWORD")))
            .collect(),
    }
}

fn random_definition(rng: &mut Pcg64Dxsm) -> CardDefinitionDSL {
    let versions = ["1.0", "1.3", "1.0.7", "1.12.0"];
    let mut legalities = BTreeMap::new();
    for _ in 0..below(rng, 5) {
        legalities.insert(
            random_string(rng, 6),
            Legality::ALL[below(rng, Legality::ALL.len() as u64) as usize],
        );
    }
    CardDefinitionDSL {
        card_def_id: format!("test.{}", random_string(rng, 8)),
        dsl_version: versions[below(rng, versions.len() as u64) as usize].to_string(),
        definition_hash: random_string(rng, 4),
        source: ProvenanceDSL {
            oracle_text_hash: random_string(rng, 8),
            origin: Origin::ALL[below(rng, 3) as usize],
            parser_version: maybe(rng, |r| random_string(r, 5)),
            override_source: maybe(rng, |r| random_string(r, 5)),
            verified_by: maybe(rng, random_strings),
            rules_baseline: random_string(rng, 6),
        },
        support_status: SupportStatus::ALL[below(rng, 4) as usize],
        known_gaps: maybe(rng, random_strings),
        layout: LayoutBehavior::ALL[below(rng, LayoutBehavior::ALL.len() as u64) as usize],
        faces: (0..below(rng, 4)).map(|_| random_face(rng)).collect(),
        meld_partner: maybe(rng, |r| format!("test.{}", random_string(r, 5))),
        color_identity: bitmask(below(rng, 32) as u8),
        commander_eligibility: maybe(rng, |r| CommanderEligibilityDSL {
            eligible: below(r, 2) == 0,
            partner_abilities: (0..below(r, 3))
                .map(|i| pa(random_keyword(r, &format!("p{i}"), "PARTNER")))
                .collect(),
        }),
        legalities,
        digital_only: below(rng, 2) == 0,
        related_tokens: maybe(rng, random_strings),
    }
}

/// REQ-DSL-001 / REQ-DSL-002 / REQ-NFR-006 (acceptance: "round-trip + canonical-form
/// stability"): 400 seeded random definitions round-trip through serde_json values and bytes,
/// hash to the independent oracle, seal, and load back through from_json from canonical,
/// pretty and shuffled encodings.
#[test]
fn req_dsl_001_round_trip_and_canonical_form_stability_property() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xF1E0_0010_CA4D_0001);
    for i in 0..400 {
        let d = random_definition(&mut rng);
        let v = to_value(&d);
        assert_eq!(
            serde_json::from_value::<CardDefinitionDSL>(v.clone()).unwrap(),
            d,
            "def {i}"
        );
        let bytes = serde_json::to_vec(&d).unwrap();
        assert_eq!(
            serde_json::from_slice::<CardDefinitionDSL>(&bytes).unwrap(),
            d,
            "def {i}"
        );

        let h = d.compute_hash().unwrap();
        assert_eq!(h, oracle_hash(&d), "def {i}");
        let c = canonical_bytes(&d);
        assert_eq!(canonicalize(&c).unwrap(), c, "def {i}");
        assert_eq!(canonicalize(&bytes).unwrap(), c, "def {i}");

        let s = d.clone().seal().unwrap();
        assert_eq!(s.definition_hash, h);
        assert_eq!(s.verify_hash(), Ok(()));
        let sc = canonical_bytes(&s);
        assert_eq!(load(&sc).unwrap(), s, "def {i}");
        if i % 4 == 0 {
            let pretty = serde_json::to_vec_pretty(&s).unwrap();
            assert_eq!(load(&pretty).unwrap(), s, "def {i} pretty");
            let shuffled = shuffled_bytes(&to_value(&s), i);
            let loaded = load(&shuffled).unwrap_or_else(|e| panic!("def {i} shuffled: {e:?}"));
            assert_eq!(loaded, s, "def {i} shuffled");
            assert_eq!(loaded.compute_hash().unwrap(), h);
        }
    }
}

/// REQ-DSL-001 / REQ-NFR-006 (R12): random bytes and mutated valid documents never panic the
/// loader; any accepted document passes verify_hash.
#[test]
fn req_dsl_001_from_json_never_panics_on_random_or_mutated_input() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xF1E0_0010_BAD0_0002);
    const ALPHABET: &[u8] = b"{}[]\":,0123456789-.eE trufalsn\\dslVersion1.0";
    for _ in 0..2000 {
        let len = below(&mut rng, 96) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                if below(&mut rng, 4) == 0 {
                    rng.next_u64() as u8
                } else {
                    ALPHABET[below(&mut rng, ALPHABET.len() as u64) as usize]
                }
            })
            .collect();
        if let Ok(d) = load(&bytes) {
            assert_eq!(d.verify_hash(), Ok(()));
        }
    }
    let seeds: Vec<Vec<u8>> = vec![
        canonical_bytes(&sealed(rich())),
        canonical_bytes(&sealed(minimal())),
        serde_json::to_vec_pretty(&sealed(rich())).unwrap(),
    ];
    let mut accepted = 0usize;
    for i in 0..3000 {
        let mut bytes = seeds[i % seeds.len()].clone();
        for _ in 0..1 + below(&mut rng, 3) {
            match below(&mut rng, 4) {
                0 if !bytes.is_empty() => {
                    let j = below(&mut rng, bytes.len() as u64) as usize;
                    bytes[j] ^= 1 << below(&mut rng, 8);
                }
                1 if !bytes.is_empty() => {
                    let j = below(&mut rng, bytes.len() as u64 + 1) as usize;
                    bytes.truncate(j);
                }
                2 => {
                    let j = below(&mut rng, bytes.len() as u64 + 1) as usize;
                    bytes.insert(j, b"{}[]\",: 9"[below(&mut rng, 9) as usize]);
                }
                _ if !bytes.is_empty() => {
                    let j = below(&mut rng, bytes.len() as u64) as usize;
                    bytes.remove(j);
                }
                _ => {}
            }
        }
        if let Ok(d) = load(&bytes) {
            accepted += 1;
            assert_eq!(d.verify_hash(), Ok(()));
        }
    }
    // Whitespace-only mutations of the pretty form are accepted, so the Ok path is exercised.
    let _ = accepted;
}
