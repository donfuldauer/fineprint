// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Black-box spec tests for #159 FP-140: card definitions carry real DSL ASTs. `seal` and
//! `from_json` reject `power`, `toughness`, `text`, `abilities[]` and
//! `commanderEligibility.partnerAbilities[]` content that is not a valid real AST
//! (`expr::ValueExpressionAST`, `text::TextTokenStream`, `ability::AbilityAST`, KEYWORD-kind
//! abilities for partners), while valid real content round-trips canonically with a stable,
//! well-defined `definitionHash`. Covers REQ-DSL-003, REQ-DSL-010, REQ-DSL-012, REQ-NFR-006 and
//! REQ-NFR-008. Written from the work-item brief only; every definition here is SYNTHETIC (R5).
//! No CR rule is implemented here, so none is cited (R6).
//!
//! Fixture rule (brief): no non-empty `effect` list appears in any fixture, because open issue
//! #161 (FP-141) replaces that placeholder and would change hashes.

use std::collections::BTreeMap;

use fineprint_base::hash::sha256;
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::ability::AbilityAST as RealAbility;
use fineprint_dsl::canonical::{CanonicalError, MAX_DEPTH, MAX_SAFE_INTEGER, canonicalize};
use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, CardDefinitionError, CardFaceDSL, ColorBitmask,
    CommanderEligibilityDSL, FaceKind, LayoutBehavior, Legality, Origin, PartnerAbilityDSL,
    ProvenanceDSL, SupportStatus, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::expr::ValueExpressionAST as RealValue;
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::text::TextTokenStream as RealText;
use serde_json::{Value, json};

// ---------------------------------------------------------------- synthetic fixtures

fn kw(id: &str, keyword: &str) -> Value {
    json!({"abilityId": id, "kind": "KEYWORD", "keyword": keyword})
}

fn lit(id: &str, text: &str) -> Value {
    json!({"id": id, "kind": "LITERAL", "text": text})
}

fn word(id: &str, category: &str, value: &str) -> Value {
    json!({"id": id, "kind": "WORD", "category": category, "value": value})
}

fn face(kind: FaceKind, name: &str) -> CardFaceDSL {
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
        text: TextTokenStream(vec![]),
        abilities: vec![],
    }
}

/// A two-face SPLIT definition with valid real content in all five AST positions.
/// Face 1 has two text tokens and two abilities; there are two partner abilities, so a bad
/// element can be placed at index 1 after a valid one.
fn base() -> CardDefinitionDSL {
    let mut left = face(FaceKind::LeftHalf, "Test Split Left");
    left.mana_cost = Some(ManaCostDSL::parse("{1}{G}").unwrap());
    left.card_types = vec!["Creature".to_string()];
    left.power = Some(ValueExpressionAST(json!({"LITERAL": 2})));
    left.toughness = Some(ValueExpressionAST(json!("X")));
    left.text = TextTokenStream(vec![lit("t1", "Test left text")]);
    left.abilities = vec![AbilityAST(kw("l1", "TEST_KEYWORD"))];
    let mut right = face(FaceKind::RightHalf, "Test Split Right");
    right.card_types = vec!["Creature".to_string()];
    right.power = Some(ValueExpressionAST(json!({"LITERAL": 1})));
    right.toughness = Some(ValueExpressionAST(json!({"LITERAL": 1})));
    right.text = TextTokenStream(vec![
        lit("t1", "Test right text"),
        word("t2", "COLOR_WORD", "Hue"),
    ]);
    right.abilities = vec![
        AbilityAST(kw("r1", "TEST_KEYWORD")),
        AbilityAST(
            json!({"abilityId":"r2","kind":"STATIC","effects":[{"type":"RULE","kind":"SKIP"}]}),
        ),
    ];
    let mut legalities = BTreeMap::new();
    legalities.insert("testformat".to_string(), Legality::Legal);
    CardDefinitionDSL {
        card_def_id: "test.split_ast".to_string(),
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
        layout: LayoutBehavior::Split,
        faces: vec![left, right],
        meld_partner: None,
        color_identity: ColorBitmask::new(16).unwrap(),
        commander_eligibility: Some(CommanderEligibilityDSL {
            eligible: true,
            partner_abilities: vec![
                PartnerAbilityDSL(kw("p1", "PARTNER")),
                PartnerAbilityDSL(
                    json!({"abilityId":"p2","kind":"KEYWORD","keyword":"PARTNER",
                           "params":{"with":"test.partner_other"}}),
                ),
            ],
        }),
        legalities,
        digital_only: false,
        related_tokens: None,
    }
}

fn sealed(d: CardDefinitionDSL) -> CardDefinitionDSL {
    d.seal().unwrap_or_else(|e| panic!("seal: {e:?}"))
}

fn to_value(d: &CardDefinitionDSL) -> Value {
    serde_json::to_value(d).expect("serialise")
}

fn load(bytes: &[u8]) -> Result<CardDefinitionDSL, CardDefinitionError> {
    CardDefinitionDSL::from_json(bytes)
}

fn load_value(v: &Value) -> Result<CardDefinitionDSL, CardDefinitionError> {
    load(&serde_json::to_vec(v).unwrap())
}

/// Independent hash oracle (RFC 8785 + SHA-256 over the document without `definitionHash`).
fn oracle_hash_value(v: &Value) -> String {
    let mut v = v.clone();
    v.as_object_mut().expect("object").remove("definitionHash");
    let bytes = canonicalize(&serde_json::to_vec(&v).unwrap()).expect("canonicalize");
    sha256(&bytes).to_hex()
}

/// Sets `definitionHash` to the correct digest of the (patched) document.
fn redigest(v: &mut Value) {
    let h = oracle_hash_value(v);
    v.as_object_mut()
        .unwrap()
        .insert("definitionHash".into(), json!(h));
}

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_u64() % n
}

// ---------------------------------------------------------------- positions

/// One of the five AST positions of a definition.
#[derive(Clone, Copy, Debug)]
enum Pos {
    Power(usize),
    Toughness(usize),
    /// Element `idx` of `faces[face].text`.
    Text(usize, usize),
    /// Element `idx` of `faces[face].abilities`.
    Ability(usize, usize),
    /// Element `idx` of `commanderEligibility.partnerAbilities`.
    Partner(usize),
}

impl Pos {
    fn pointer(self) -> String {
        match self {
            Pos::Power(f) => format!("/faces/{f}/power"),
            Pos::Toughness(f) => format!("/faces/{f}/toughness"),
            Pos::Text(f, i) => format!("/faces/{f}/text/{i}"),
            Pos::Ability(f, i) => format!("/faces/{f}/abilities/{i}"),
            Pos::Partner(i) => format!("/commanderEligibility/partnerAbilities/{i}"),
        }
    }

    /// Substrings the error message must contain (the brief: index and member names).
    fn markers(self) -> Vec<String> {
        match self {
            Pos::Power(f) => vec![format!("faces[{f}]"), "power".into()],
            Pos::Toughness(f) => vec![format!("faces[{f}]"), "toughness".into()],
            Pos::Text(f, _) => vec![format!("faces[{f}]"), "text".into()],
            Pos::Ability(f, i) => vec![format!("faces[{f}]"), format!("abilities[{i}]")],
            Pos::Partner(i) => vec![format!("partnerAbilities[{i}]")],
        }
    }

    /// Puts `x` into the position of a built definition (the slot must exist, or be the next
    /// index of a vector).
    fn set(self, d: &mut CardDefinitionDSL, x: Value) {
        fn put<T>(v: &mut Vec<T>, i: usize, t: T) {
            if i == v.len() {
                v.push(t);
            } else {
                v[i] = t;
            }
        }
        match self {
            Pos::Power(f) => d.faces[f].power = Some(ValueExpressionAST(x)),
            Pos::Toughness(f) => d.faces[f].toughness = Some(ValueExpressionAST(x)),
            Pos::Text(f, i) => put(&mut d.faces[f].text.0, i, x),
            Pos::Ability(f, i) => put(&mut d.faces[f].abilities, i, AbilityAST(x)),
            Pos::Partner(i) => put(
                &mut d.commander_eligibility.as_mut().unwrap().partner_abilities,
                i,
                PartnerAbilityDSL(x),
            ),
        }
    }

    /// Puts `x` into the position of a definition document.
    fn set_value(self, v: &mut Value, x: Value) {
        let ptr = self.pointer();
        let (parent, last) = ptr.rsplit_once('/').unwrap();
        match v.pointer_mut(parent).unwrap() {
            Value::Array(items) => {
                let i: usize = last.parse().unwrap();
                if i == items.len() {
                    items.push(x);
                } else {
                    items[i] = x;
                }
            }
            Value::Object(m) => {
                m.insert(last.to_string(), x);
            }
            other => panic!("{ptr}: parent is {other}"),
        }
    }
}

fn markers_in(msg: &str, pos: Pos, what: &str) {
    for m in pos.markers() {
        assert!(
            msg.contains(&m),
            "{what}: message {msg:?} does not name {m:?}"
        );
    }
}

/// `x` at `pos` is rejected by `seal` (Serialize, naming the position) and by `from_json`
/// (Schema, naming the position) with a correctly recomputed `definitionHash`.
fn assert_rejected(pos: Pos, x: &Value) {
    let what = format!("{pos:?} = {x}");
    let mut d = base();
    pos.set(&mut d, x.clone());
    match d.seal() {
        Err(CanonicalError::Serialize(msg)) => markers_in(&msg, pos, &format!("seal {what}")),
        other => panic!("seal {what}: expected Serialize, got {other:?}"),
    }
    let mut v = to_value(&sealed(base()));
    pos.set_value(&mut v, x.clone());
    redigest(&mut v);
    match load_value(&v) {
        Err(CardDefinitionError::Schema(msg)) => {
            markers_in(&msg, pos, &format!("from_json {what}"))
        }
        other => panic!("from_json {what}: expected Schema, got {other:?}"),
    }
}

// ---------------------------------------------------------------- 1a invalid content

const MATH_MISSING_RHS: &str = r#"{"MATH":{"op":"ADD","lhs":{"LITERAL":1}}}"#;

fn invalid_values() -> Vec<Value> {
    vec![
        Value::Null,
        json!("Y"),
        json!("x"),
        json!(""),
        json!(3),
        json!({"LITERAL": 1.5}),
        json!({"LITERAL": 1, "extra": 2}),
        serde_json::from_str(MATH_MISSING_RHS).unwrap(),
        json!({"MATH": {"op": "PLUS", "lhs": {"LITERAL": 1}, "rhs": {"LITERAL": 1}}}),
        json!({"const": 2}),
        json!("TARGET(0)"),
        json!({"LITERAL": "2"}),
        json!([{"LITERAL": 2}]),
    ]
}

/// REQ-DSL-003, REQ-DSL-010: power and toughness that are not a real `ValueExpressionAST`
/// (including a present `null`, a bare number and SRS call-style shorthand) are rejected by
/// `seal` and `from_json`, naming the face index and member.
#[test]
fn req_dsl_003_010_invalid_power_toughness_rejected_by_seal_and_from_json() {
    for x in invalid_values() {
        for pos in [Pos::Power(0), Pos::Power(1), Pos::Toughness(1)] {
            assert_rejected(pos, &x);
        }
    }
}

/// REQ-DSL-003, REQ-NFR-006: an unsafe integer inside a LITERAL is rejected by both. `from_json`
/// runs the canonical-level checks first (FP-010 order), so it reports
/// `Canonical(UnsafeInteger)`; `seal` reports the canonical-level error too (`UnsafeInteger`) or
/// `Serialize`. 2^53-1 itself is accepted and round-trips.
#[test]
fn req_dsl_003_nfr_006_unsafe_literal_rejected() {
    for n in [9_007_199_254_740_992_i64, -9_007_199_254_740_992] {
        let mut d = base();
        Pos::Power(1).set(&mut d, json!({ "LITERAL": n }));
        // Canonical-level limit: `seal` may report it as the canonical error itself or as
        // `Serialize` (docs/DECISIONS.md "#159 FP-140 — Spec test migration and golden hash").
        let r = d.seal();
        assert!(
            matches!(
                r,
                Err(CanonicalError::UnsafeInteger(_) | CanonicalError::Serialize(_))
            ),
            "seal {n}: {r:?}"
        );
        let mut v = to_value(&sealed(base()));
        Pos::Power(1).set_value(&mut v, json!({ "LITERAL": n }));
        match load_value(&v) {
            Err(CardDefinitionError::Canonical(CanonicalError::UnsafeInteger(_))) => {}
            other => panic!("from_json {n}: expected Canonical(UnsafeInteger), got {other:?}"),
        }
    }
    for n in [MAX_SAFE_INTEGER, -MAX_SAFE_INTEGER] {
        let mut d = base();
        Pos::Power(1).set(&mut d, json!({ "LITERAL": n }));
        let s = sealed(d);
        assert_eq!(load(&s.to_canonical_json().unwrap()), Ok(s));
    }
}

/// REQ-DSL-012, REQ-DSL-010: text elements that are not valid tokens (null, a bare string, a
/// duplicate id, an empty id, empty literal text, an unknown kind, an unknown member, SELF with a
/// name, OTHER without one) are rejected by `seal` and `from_json`, naming the face and `text`.
#[test]
fn req_dsl_012_010_invalid_text_rejected_by_seal_and_from_json() {
    let bad = [
        Value::Null,
        json!("test.word"),
        json!(7),
        lit("t1", "duplicate of the first token id"),
        lit("", "empty id"),
        lit("t9", ""),
        json!({"id": "t9", "kind": "GLYPH", "text": "x"}),
        json!({"id": "t9", "kind": "LITERAL", "text": "x", "extra": 1}),
        json!({"id": "t9", "kind": "LITERAL"}),
        json!({"id": "t9", "kind": "WORD", "category": "COLOR_WORD", "value": ""}),
        json!({"id": "t9", "kind": "WORD", "category": "SHADE", "value": "Hue"}),
        json!({"id": "t9", "kind": "NAME_REF", "ref": "SELF", "name": "Test Name"}),
        json!({"id": "t9", "kind": "NAME_REF", "ref": "OTHER"}),
        json!({"t": "word", "v": "Test"}),
    ];
    for x in &bad {
        // Replace the second token of face 1, and append after the single token of face 0.
        assert_rejected(Pos::Text(1, 1), x);
        assert_rejected(Pos::Text(0, 1), x);
    }
}

fn trig_event(event: Value) -> Value {
    json!({"abilityId":"z","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":event},"effect":[]})
}

/// REQ-DSL-010: abilities that are not a real `AbilityAST` (bare string, keyword name as kind,
/// missing abilityId, unknown kind, unknown member, present null optional, trigger member on a
/// STATIC ability, a look-back key under `filters`/`bind` per decision "#13 FP-012 — Rule E5")
/// are rejected by `seal` and `from_json`, naming face and ability index.
#[test]
fn req_dsl_010_invalid_abilities_rejected_by_seal_and_from_json() {
    let mut with_null_span = kw("z", "TEST_KEYWORD");
    with_null_span["textSpan"] = Value::Null;
    let mut with_null_params = kw("z", "TEST_KEYWORD");
    with_null_params["params"] = Value::Null;
    let mut with_unknown = kw("z", "TEST_KEYWORD");
    with_unknown["zzzUnknown"] = json!(1);
    let bad = [
        json!("keyword"),
        json!({"kind": "FLYING"}),
        json!({"kind": "KEYWORD", "keyword": "TEST_KEYWORD"}),
        json!({"abilityId": "z", "kind": "UNKNOWN_KIND"}),
        json!({"abilityId": "z", "kind": "keyword", "keyword": "TEST_KEYWORD"}),
        with_unknown,
        with_null_span,
        with_null_params,
        json!({"abilityId":"z","kind":"STATIC","effects":[{"type":"RULE","kind":"SKIP"}],
               "trigger":{"mode":"STATE","state":{"pred":"IS_CREATURE"}}}),
        trig_event(json!({"kind":"DIES","filters":{"combat":true,"lookBack":true}})),
        trig_event(json!({"kind":"DIES","filters":{"look_back":true}})),
        trig_event(json!({"kind":"DIES","bind":{"recipient":"$P","LOOK-BACK":"$v"}})),
        json!(null),
        json!([]),
        json!({"kind": "STATIC", "nested": {"a": [1, 2]}}),
    ];
    for x in &bad {
        assert_rejected(Pos::Ability(1, 1), x);
        assert_rejected(Pos::Ability(0, 0), x);
    }
}

/// REQ-DSL-010: a partner ability must be a real KEYWORD-kind `AbilityAST`: a valid non-KEYWORD
/// ability, a free-form kind and a number are rejected, naming `partnerAbilities[i]`.
#[test]
fn req_dsl_010_invalid_partner_abilities_rejected_by_seal_and_from_json() {
    let bad = [
        json!({"abilityId":"p9","kind":"STATIC","effects":[{"type":"RULE","kind":"SKIP"}]}),
        json!({"abilityId":"p9","kind":"TRIGGERED",
               "trigger":{"mode":"STATE","state":{"pred":"IS_CREATURE"}},"effect":[]}),
        json!({"abilityId":"p9","kind":"SPELL","effect":[]}),
        json!({"kind": "TEST_PARTNER"}),
        json!({"kind": "TEST_PARTNER", "with": null}),
        json!(42),
        Value::Null,
        json!({"abilityId":"p9","kind":"KEYWORD"}),
        json!({"abilityId":"p9","kind":"KEYWORD","keyword":"partner"}),
    ];
    // The bad shapes really are what they claim: the first three decode as real abilities.
    for x in &bad[..3] {
        let a: RealAbility = serde_json::from_value(x.clone()).expect("valid real ability");
        assert_ne!(a.kind(), "KEYWORD");
    }
    for x in &bad {
        assert_rejected(Pos::Partner(1), x);
        assert_rejected(Pos::Partner(0), x);
    }
}

// ---------------------------------------------------------------- 1b check order

/// REQ-DSL-010, REQ-NFR-006: invalid AST content with a wrong hash is `Schema` (the AST check
/// precedes the hash comparison); valid content with a wrong hash is `HashMismatch`; a wrong
/// `dslVersion` major is `Version` before any AST check.
#[test]
fn req_dsl_010_nfr_006_check_order() {
    let good = to_value(&sealed(base()));
    let wrong = "0".repeat(64);
    let cases: Vec<(Pos, Value)> = vec![
        (Pos::Power(0), json!(3)),
        (Pos::Toughness(1), Value::Null),
        (Pos::Text(1, 1), json!("test.word")),
        (Pos::Ability(1, 0), json!({"kind": "FLYING"})),
        (Pos::Partner(0), json!({"kind": "TEST_PARTNER"})),
    ];
    for (pos, x) in &cases {
        // Invalid content, stale (now wrong) hash, and an explicitly wrong hash.
        let mut v = good.clone();
        pos.set_value(&mut v, x.clone());
        match load_value(&v) {
            Err(CardDefinitionError::Schema(msg)) => markers_in(&msg, *pos, "stale hash"),
            other => panic!("{pos:?}: stale hash: expected Schema, got {other:?}"),
        }
        v["definitionHash"] = json!(wrong);
        assert!(
            matches!(load_value(&v), Err(CardDefinitionError::Schema(_))),
            "{pos:?}: wrong hash"
        );
        // Wrong major version beats the AST check, with a wrong or a correct digest.
        for ver in ["2.0", "0.9"] {
            let mut w = v.clone();
            w["dslVersion"] = json!(ver);
            assert!(
                matches!(load_value(&w), Err(CardDefinitionError::Version(_))),
                "{pos:?} {ver}: wrong hash"
            );
            redigest(&mut w);
            assert!(
                matches!(load_value(&w), Err(CardDefinitionError::Version(_))),
                "{pos:?} {ver}: correct hash"
            );
        }
    }
    // Valid content with a wrong hash is still a HashMismatch.
    let mut v = good.clone();
    v["definitionHash"] = json!(wrong);
    match load_value(&v) {
        Err(CardDefinitionError::HashMismatch { declared, computed }) => {
            assert_eq!(declared, wrong);
            assert_eq!(computed, good["definitionHash"].as_str().unwrap());
        }
        other => panic!("expected HashMismatch, got {other:?}"),
    }
    // Valid replacement content without re-digesting: HashMismatch, not Schema.
    let mut v = good.clone();
    Pos::Power(0).set_value(&mut v, json!({"LITERAL": 5}));
    assert!(matches!(
        load_value(&v),
        Err(CardDefinitionError::HashMismatch { .. })
    ));
    // And a valid seal with an unsupported version is rejected by seal.
    let mut d = base();
    d.dsl_version = "2.0".into();
    Pos::Power(0).set(&mut d, json!({"LITERAL": 3}));
    match d.seal() {
        Err(CanonicalError::Serialize(msg)) => assert!(
            msg.contains("dslVersion major 2"),
            "seal must name the rejected version, got {msg:?}"
        ),
        other => panic!("expected a version Serialize error, got {other:?}"),
    }
    // The same otherwise valid content seals under the supported version.
    let mut d = base();
    Pos::Power(0).set(&mut d, json!({"LITERAL": 3}));
    assert!(d.seal().is_ok());
}

// ---------------------------------------------------------------- 1c valid content

fn math(op: &str, lhs: Value, rhs: Value) -> Value {
    json!({"MATH": {"op": op, "lhs": lhs, "rhs": rhs}})
}

fn valid_values() -> Vec<Value> {
    vec![
        json!({"LITERAL": 2}),
        json!({"LITERAL": 0}),
        json!({"LITERAL": -7}),
        json!({ "LITERAL": MAX_SAFE_INTEGER }),
        json!("X"),
        math("ADD", json!({"LITERAL": 1}), json!({"LITERAL": 2})),
        math(
            "ADD",
            math(
                "ADD",
                math("ADD", json!({"LITERAL": 1}), json!({"LITERAL": 2})),
                json!({"LITERAL": 3}),
            ),
            json!({"LITERAL": 4}),
        ),
        math("DIV_FLOOR", json!("X"), json!({"LITERAL": 2})),
        json!({"COUNT": {"zones": ["BATTLEFIELD"], "pred": "IS_CREATURE"}}),
        json!({"ATTR": {"ref": "SELF", "attr": "POWER"}}),
        json!({"PLAYER_ATTR": {"ref": "CONTROLLER", "attr": "LIFE"}}),
    ]
}

fn valid_texts() -> Vec<Vec<Value>> {
    vec![
        vec![],
        vec![lit("t1", "Test literal")],
        vec![word("t1", "COLOR_WORD", "Hue")],
        vec![json!({"id": "t1", "kind": "NAME_REF", "ref": "SELF"})],
        vec![json!({"id": "t1", "kind": "NAME_REF", "ref": "OTHER", "name": "Test Other"})],
        vec![
            lit("t1", "Test é\u{1F602}"),
            word("t2", "NUMBER", "7"),
            word("t3", "CREATURE_TYPE", "Testkin"),
            word("t4", "BASIC_LAND_TYPE", "Testland"),
            json!({"id": "t5", "kind": "NAME_REF", "ref": "SELF"}),
        ],
    ]
}

fn valid_abilities() -> Vec<Value> {
    vec![
        kw("v1", "TEST_KEYWORD"),
        json!({"abilityId":"v1","kind":"KEYWORD","keyword":"TEST_KEYWORD","params":{"n":1,"s":"Hue"}}),
        json!({"abilityId":"v1","kind":"STATIC","effects":[{"type":"RULE","kind":"SKIP"}]}),
        json!({"abilityId":"v1","kind":"TRIGGERED",
               "trigger":{"mode":"STATE","state":{"pred":"IS_CREATURE"}},"effect":[]}),
        trig_event(json!({"kind":"DIES","filters":{"combat":true}})),
        json!({"abilityId":"v1","kind":"REPLACEMENT",
               "match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}),
        json!({"abilityId":"v1","kind":"SPELL","effect":[]}),
        json!({"abilityId":"v1","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"effect":[]}),
    ]
}

fn valid_partners() -> Vec<Value> {
    vec![
        kw("p1", "PARTNER"),
        json!({"abilityId":"p1","kind":"KEYWORD","keyword":"PARTNER",
               "params":{"with":"test.partner_other"}}),
        kw("p1", "TEST_KEYWORD"),
    ]
}

/// Asserts that `sample` decodes as the real type and re-encodes to exactly `sample`.
fn single_spelling<T: serde::de::DeserializeOwned + serde::Serialize>(sample: &Value) -> T {
    let t: T = serde_json::from_value(sample.clone())
        .unwrap_or_else(|e| panic!("{sample} must decode as the real type: {e}"));
    assert_eq!(
        serde_json::to_value(&t).unwrap(),
        *sample,
        "single spelling: re-encoding must reproduce {sample}"
    );
    t
}

/// The valid definition `d` seals, round-trips canonically, verifies, and its hash equals the
/// oracle hash of the same definition built as a JSON document `doc`, and of the definition
/// decoded from `doc` and sealed.
fn assert_accepted(d: CardDefinitionDSL, doc: &Value, what: &str) {
    let s = d
        .clone()
        .seal()
        .unwrap_or_else(|e| panic!("{what}: seal {e:?}"));
    assert_eq!(s.verify_hash(), Ok(()), "{what}");
    let bytes = s.to_canonical_json().unwrap();
    assert_eq!(load(&bytes), Ok(s.clone()), "{what}: canonical round trip");
    assert_eq!(s.compute_hash().unwrap(), s.definition_hash, "{what}");
    assert_eq!(oracle_hash_value(doc), s.definition_hash, "{what}: oracle");
    let from_doc: CardDefinitionDSL = serde_json::from_value(doc.clone()).unwrap();
    assert_eq!(sealed(from_doc), s, "{what}: built from Value");
    // The document itself, digested, loads to the same value.
    let mut v = doc.clone();
    redigest(&mut v);
    assert_eq!(load_value(&v), Ok(s), "{what}: document load");
}

/// REQ-DSL-003, REQ-DSL-012, REQ-DSL-010, REQ-NFR-006: valid real content in every position is
/// accepted by `seal` and `from_json`, round-trips exactly, is single-spelling under the real
/// types, and hashes like the same definition built from `serde_json::Value` documents.
#[test]
fn req_dsl_003_012_010_nfr_006_valid_content_accepted_and_round_trips() {
    let doc0 = to_value(&base());
    assert_accepted(base(), &doc0, "base");
    for x in valid_values() {
        let _: RealValue = single_spelling(&x);
        for pos in [Pos::Power(0), Pos::Toughness(1)] {
            let mut d = base();
            pos.set(&mut d, x.clone());
            let mut doc = doc0.clone();
            pos.set_value(&mut doc, x.clone());
            assert_accepted(d, &doc, &format!("{pos:?} = {x}"));
        }
    }
    for tokens in valid_texts() {
        let stream = Value::Array(tokens.clone());
        let real: RealText = single_spelling(&stream);
        assert_eq!(real.len(), tokens.len());
        for f in 0..2 {
            let mut d = base();
            d.faces[f].text = TextTokenStream(tokens.clone());
            let mut doc = doc0.clone();
            doc["faces"][f]["text"] = stream.clone();
            assert_accepted(d, &doc, &format!("faces[{f}].text = {stream}"));
        }
    }
    let mut kinds = std::collections::BTreeSet::new();
    for x in valid_abilities() {
        let a: RealAbility = single_spelling(&x);
        kinds.insert(a.kind());
        for pos in [Pos::Ability(0, 1), Pos::Ability(1, 1)] {
            let mut d = base();
            pos.set(&mut d, x.clone());
            let mut doc = doc0.clone();
            pos.set_value(&mut doc, x.clone());
            assert_accepted(d, &doc, &format!("{pos:?} = {x}"));
        }
    }
    assert_eq!(kinds.len(), 6, "all six ability kinds: {kinds:?}");
    for x in valid_partners() {
        let a: RealAbility = single_spelling(&x);
        assert_eq!(a.kind(), "KEYWORD");
        let mut d = base();
        Pos::Partner(1).set(&mut d, x.clone());
        // Keep the two partner ids distinct.
        d.commander_eligibility.as_mut().unwrap().partner_abilities[0] =
            PartnerAbilityDSL(kw("p0", "PARTNER"));
        let mut doc = doc0.clone();
        Pos::Partner(1).set_value(&mut doc, x.clone());
        Pos::Partner(0).set_value(&mut doc, kw("p0", "PARTNER"));
        assert_accepted(d, &doc, &format!("partner {x}"));
    }
    // No partners at all, and a DFC_NONMODAL two-face layout.
    let mut d = base();
    d.commander_eligibility.as_mut().unwrap().partner_abilities = vec![];
    d.layout = LayoutBehavior::DfcNonmodal;
    d.faces[0].face_kind = FaceKind::Main;
    d.faces[1].face_kind = FaceKind::Back;
    let doc = to_value(&d);
    assert_accepted(d, &doc, "DFC_NONMODAL, no partners");
}

// ---------------------------------------------------------------- 1d hash well-definedness

/// REQ-NFR-006: seal is idempotent and ignores the previous `definitionHash`; `compute_hash` is
/// SHA-256 of the RFC 8785 bytes of `to_canonical_json` without `definitionHash`; changing one
/// valid AST member changes the hash.
#[test]
fn req_nfr_006_hash_well_defined_over_real_asts() {
    let s = sealed(base());
    assert_eq!(sealed(s.clone()), s, "seal(seal(d)) == seal(d)");
    for stale in ["", "x", &"f".repeat(64), &s.definition_hash.to_uppercase()] {
        let mut d = base();
        d.definition_hash = stale.to_string();
        assert_eq!(sealed(d), s, "stale {stale:?}");
    }
    // Recompute independently from the canonical bytes.
    let bytes = s.to_canonical_json().unwrap();
    let mut v: Value = serde_json::from_slice(&bytes).unwrap();
    v.as_object_mut().unwrap().remove("definitionHash");
    let without = canonicalize(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(sha256(&without).to_hex(), s.definition_hash);
    assert_eq!(s.compute_hash().unwrap(), s.definition_hash);
    // One AST member changed in each position changes the hash.
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(s.definition_hash.clone());
    let changes: Vec<(Pos, Value)> = vec![
        (Pos::Power(0), json!({"LITERAL": 3})),
        (Pos::Toughness(0), json!({"LITERAL": 0})),
        (Pos::Text(1, 1), word("t2", "COLOR_WORD", "Hve")),
        (Pos::Ability(1, 0), kw("r1", "TEST_KEYWORD_B")),
        (Pos::Partner(0), kw("p0", "PARTNER")),
    ];
    for (pos, x) in changes {
        let mut d = base();
        pos.set(&mut d, x);
        let h = sealed(d).definition_hash;
        assert!(seen.insert(h), "{pos:?}: hash did not change");
    }
}

/// REQ-NFR-006, REQ-DSL-003: two spellings of the same valid AST cannot both survive: the
/// loader's canonical step turns `{"LITERAL":2.0}` into `{"LITERAL":2}` (same value, same hash),
/// and member-order variants of the same object load and re-seal to the same hash.
#[test]
fn req_nfr_006_dsl_003_single_spelling_on_load() {
    let mut d = base();
    Pos::Power(0).set(&mut d, json!({"LITERAL": 2}));
    let s = sealed(d);
    let text = String::from_utf8(s.to_canonical_json().unwrap()).unwrap();
    let int_member = r#""power":{"LITERAL":2}"#;
    assert!(text.contains(int_member), "{text}");
    let float_doc = text.replacen(int_member, r#""power":{"LITERAL":2.0}"#, 1);
    assert_ne!(float_doc, text);
    let loaded = load(float_doc.as_bytes()).expect("2.0 normalises to 2 and loads");
    assert_eq!(loaded, s);
    assert_eq!(
        loaded.faces[0].power,
        Some(ValueExpressionAST(json!({"LITERAL": 2})))
    );

    // Member-order variants of a MATH node and of an ability object.
    let m = math("SUB", json!({"LITERAL": 5}), json!("X"));
    let mut d = base();
    Pos::Power(1).set(&mut d, m.clone());
    let s = sealed(d);
    let text = String::from_utf8(s.to_canonical_json().unwrap()).unwrap();
    let canon_math = r#"{"MATH":{"lhs":{"LITERAL":5},"op":"SUB","rhs":"X"}}"#;
    assert!(text.contains(canon_math), "{text}");
    for variant in [
        r#"{"MATH":{"rhs":"X","op":"SUB","lhs":{"LITERAL":5}}}"#,
        r#"{ "MATH" : { "op":"SUB", "rhs":"X", "lhs":{ "LITERAL" : 5 } } }"#,
    ] {
        let doc = text.replacen(canon_math, variant, 1);
        assert_ne!(doc, text);
        let loaded = load(doc.as_bytes()).unwrap_or_else(|e| panic!("{variant}: {e:?}"));
        assert_eq!(loaded, s, "{variant}");
        assert_eq!(sealed(loaded).definition_hash, s.definition_hash);
    }
    let canon_kw = r#"{"abilityId":"r1","keyword":"TEST_KEYWORD","kind":"KEYWORD"}"#;
    assert!(text.contains(canon_kw), "{text}");
    let doc = text.replacen(
        canon_kw,
        r#"{"kind":"KEYWORD","keyword":"TEST_KEYWORD","abilityId":"r1"}"#,
        1,
    );
    assert_eq!(load(doc.as_bytes()), Ok(s));
}

// ---------------------------------------------------------------- 1e plain serde

/// REQ-NFR-006: the `card::` wrappers keep permissive transparent serde: any JSON constructs and
/// round-trips through `serde_json`, and `compute_hash`/`to_canonical_json` work on free-form
/// placeholder content (only `seal` and `from_json` validate).
#[test]
fn req_nfr_006_wrappers_keep_plain_serde() {
    let a: AbilityAST = serde_json::from_value(json!("test.ability")).unwrap();
    assert_eq!(a, AbilityAST(json!("test.ability")));
    assert_eq!(serde_json::to_value(&a).unwrap(), json!("test.ability"));
    let t = TextTokenStream(vec![json!("test.word")]);
    assert_eq!(serde_json::to_value(&t).unwrap(), json!(["test.word"]));
    let back: TextTokenStream = serde_json::from_value(json!(["test.word"])).unwrap();
    assert_eq!(back, t);
    let p: PartnerAbilityDSL = serde_json::from_value(json!({"kind": "TEST_PARTNER"})).unwrap();
    assert_eq!(
        serde_json::to_value(&p).unwrap(),
        json!({"kind": "TEST_PARTNER"})
    );
    let v: ValueExpressionAST = serde_json::from_value(json!(1)).unwrap();
    assert_eq!(v, ValueExpressionAST(json!(1)));

    let mut d = base();
    d.faces[0].power = Some(v);
    d.faces[0].text = t;
    d.faces[0].abilities = vec![a];
    d.commander_eligibility.as_mut().unwrap().partner_abilities = vec![p];
    let h = d
        .compute_hash()
        .expect("compute_hash does not validate ASTs");
    assert_eq!(h, oracle_hash_value(&to_value(&d)));
    assert!(d.to_canonical_json().is_ok());
    let round: CardDefinitionDSL =
        serde_json::from_slice(&serde_json::to_vec(&d).unwrap()).unwrap();
    assert_eq!(round, d);
    assert!(matches!(d.seal(), Err(CanonicalError::Serialize(_))));
}

// ---------------------------------------------------------------- 1f properties

const SAFE_CHARS: &[char] = &[
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
    'é',
    'あ',
    '\u{1F602}',
];
const MATH_OPS: [&str; 7] = ["ADD", "SUB", "MUL", "DIV_FLOOR", "DIV_CEIL", "MIN", "MAX"];
const CATEGORIES: [&str; 5] = [
    "COLOR_WORD",
    "BASIC_LAND_TYPE",
    "CREATURE_TYPE",
    "NUMBER",
    "CARD_NAME",
];

fn gen_str(rng: &mut Pcg64Dxsm) -> String {
    let n = 1 + below(rng, 6);
    (0..n)
        .map(|_| SAFE_CHARS[below(rng, SAFE_CHARS.len() as u64) as usize])
        .collect()
}

fn gen_int(rng: &mut Pcg64Dxsm) -> i64 {
    match below(rng, 4) {
        0 => MAX_SAFE_INTEGER,
        1 => -MAX_SAFE_INTEGER,
        2 => below(rng, 21) as i64 - 10,
        _ => (below(rng, 2 * MAX_SAFE_INTEGER as u64 + 1) as i64) - MAX_SAFE_INTEGER,
    }
}

fn gen_value(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    if depth == 0 || below(rng, 3) == 0 {
        if below(rng, 4) == 0 {
            json!("X")
        } else {
            json!({ "LITERAL": gen_int(rng) })
        }
    } else {
        let op = MATH_OPS[below(rng, 7) as usize];
        math(op, gen_value(rng, depth - 1), gen_value(rng, depth - 1))
    }
}

fn gen_text(rng: &mut Pcg64Dxsm) -> Vec<Value> {
    (0..below(rng, 5))
        .map(|i| {
            let id = format!("t{i}");
            match below(rng, 4) {
                0 => lit(&id, &gen_str(rng)),
                1 => word(&id, CATEGORIES[below(rng, 5) as usize], &gen_str(rng)),
                2 => json!({"id": id, "kind": "NAME_REF", "ref": "SELF"}),
                _ => json!({"id": id, "kind": "NAME_REF", "ref": "OTHER", "name": gen_str(rng)}),
            }
        })
        .collect()
}

fn gen_keyword(rng: &mut Pcg64Dxsm, id: &str, keyword: &str) -> Value {
    let mut a = kw(id, keyword);
    if below(rng, 3) == 0 {
        a["params"] = json!({"n": gen_int(rng), "s": gen_str(rng)});
    }
    a
}

fn gen_definition(rng: &mut Pcg64Dxsm) -> (CardDefinitionDSL, Vec<Value>) {
    let mut d = base();
    let mut pieces = Vec::new();
    d.faces.truncate(1 + below(rng, 2) as usize);
    for f in 0..d.faces.len() {
        let p = if below(rng, 4) == 0 {
            None
        } else {
            Some(gen_value(rng, 6))
        };
        let t = if below(rng, 4) == 0 {
            None
        } else {
            Some(gen_value(rng, 6))
        };
        pieces.extend(p.iter().cloned());
        pieces.extend(t.iter().cloned());
        d.faces[f].power = p.map(ValueExpressionAST);
        d.faces[f].toughness = t.map(ValueExpressionAST);
        let text = gen_text(rng);
        d.faces[f].text = TextTokenStream(text);
        let keywords = ["TEST_KEYWORD", "TEST_KW_A", "TEST_KW_2"];
        d.faces[f].abilities = (0..below(rng, 4))
            .map(|i| {
                let k = keywords[below(rng, 3) as usize];
                AbilityAST(gen_keyword(rng, &format!("a{f}_{i}"), k))
            })
            .collect();
    }
    d.commander_eligibility = if below(rng, 3) == 0 {
        None
    } else {
        Some(CommanderEligibilityDSL {
            eligible: below(rng, 2) == 0,
            partner_abilities: (0..below(rng, 3))
                .map(|i| PartnerAbilityDSL(gen_keyword(rng, &format!("p{i}"), "PARTNER")))
                .collect(),
        })
    };
    (d, pieces)
}

/// REQ-DSL-003, REQ-DSL-012, REQ-DSL-010, REQ-NFR-006 (property, seeded): random valid real ASTs
/// (LITERAL/X/MATH to depth 6, token streams with unique ids, KEYWORD abilities with unique ids)
/// are single-spelling, always seal, satisfy `from_json(to_canonical_json(sealed)) == sealed`,
/// and keep the same hash on re-seal.
#[test]
fn req_dsl_003_012_nfr_006_random_valid_asts_seal_and_round_trip() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xF1E0_0140_A570_0001);
    for i in 0..300 {
        let (d, pieces) = gen_definition(&mut rng);
        for p in &pieces {
            let _: RealValue = single_spelling(p);
        }
        for f in &d.faces {
            let _: RealText = single_spelling(&Value::Array(f.text.0.clone()));
            for a in &f.abilities {
                let _: RealAbility = single_spelling(&a.0);
            }
        }
        let s = d
            .clone()
            .seal()
            .unwrap_or_else(|e| panic!("def {i}: {e:?}"));
        assert_eq!(s.verify_hash(), Ok(()), "def {i}");
        assert_eq!(
            load(&s.to_canonical_json().unwrap()),
            Ok(s.clone()),
            "def {i}"
        );
        assert_eq!(sealed(s.clone()), s, "def {i}: re-seal");
        assert_eq!(oracle_hash_value(&to_value(&d)), s.definition_hash);
    }
}

fn random_string(rng: &mut Pcg64Dxsm, max_len: u64) -> String {
    let n = below(rng, max_len + 1);
    (0..n)
        .map(|_| SAFE_CHARS[below(rng, SAFE_CHARS.len() as u64) as usize])
        .collect()
}

/// Random JSON in the style of `spec_dsl_card.rs::random_json`, biased towards AST-looking keys.
fn random_json(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    const KEYS: [&str; 12] = [
        "LITERAL",
        "MATH",
        "op",
        "lhs",
        "rhs",
        "id",
        "kind",
        "text",
        "abilityId",
        "keyword",
        "KEYWORD",
        "X",
    ];
    let kinds = if depth == 0 { 4 } else { 6 };
    match below(rng, kinds) {
        0 => Value::Null,
        1 => Value::Bool(below(rng, 2) == 0),
        2 => json!(gen_int(rng)),
        3 => {
            if below(rng, 3) == 0 {
                json!(KEYS[below(rng, 12) as usize])
            } else {
                Value::String(random_string(rng, 6))
            }
        }
        4 => Value::Array(
            (0..below(rng, 4))
                .map(|_| random_json(rng, depth - 1))
                .collect(),
        ),
        _ => {
            let mut m = serde_json::Map::new();
            for _ in 0..below(rng, 4) {
                let k = if below(rng, 2) == 0 {
                    KEYS[below(rng, 12) as usize].to_string()
                } else {
                    random_string(rng, 4)
                };
                m.insert(k, random_json(rng, depth - 1));
            }
            Value::Object(m)
        }
    }
}

/// REQ-DSL-010, REQ-NFR-006 (property, R12): random JSON in each of the five positions never
/// panics, and `seal` and `from_json` agree: both reject, or both accept with an exact round
/// trip to the same value.
#[test]
fn req_dsl_010_nfr_006_random_json_in_positions_seal_and_load_agree() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xF1E0_0140_A570_0002);
    let good = to_value(&sealed(base()));
    let positions = [
        Pos::Power(0),
        Pos::Toughness(1),
        Pos::Text(1, 1),
        Pos::Ability(1, 1),
        Pos::Partner(1),
    ];
    let mut accepted = 0usize;
    for i in 0..1500 {
        let pos = positions[i % positions.len()];
        let x = random_json(&mut rng, 3);
        let mut d = base();
        pos.set(&mut d, x.clone());
        let sealed_r = d.seal();
        let mut v = good.clone();
        pos.set_value(&mut v, x.clone());
        redigest(&mut v);
        let loaded_r = load_value(&v);
        match (sealed_r, loaded_r) {
            (Ok(s), Ok(l)) => {
                accepted += 1;
                assert_eq!(s, l, "{pos:?} = {x}");
                assert_eq!(load(&s.to_canonical_json().unwrap()), Ok(s.clone()));
            }
            (Err(CanonicalError::Serialize(_)), Err(CardDefinitionError::Schema(_))) => {}
            (s, l) => panic!("{pos:?} = {x}: seal {s:?} vs from_json {l:?}"),
        }
    }
    let _ = accepted;
}

/// REQ-DSL-010, REQ-NFR-006 (R12): bounded random bytes and mutations of a sealed real-AST
/// definition never panic `from_json`; anything accepted is hash-valid and round-trips.
#[test]
fn req_dsl_010_from_json_never_panics_on_random_bytes() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xF1E0_0140_BAD0_0003);
    const ALPHABET: &[u8] = b"{}[]\":,0123456789-.eE trufalsn\\LITERALMATHXkind";
    let check = |bytes: &[u8]| {
        if let Ok(d) = load(bytes) {
            assert_eq!(d.verify_hash(), Ok(()));
            assert_eq!(load(&d.to_canonical_json().unwrap()), Ok(d.clone()));
        }
    };
    for _ in 0..2000 {
        let len = below(&mut rng, 128) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                if below(&mut rng, 4) == 0 {
                    rng.next_u64() as u8
                } else {
                    ALPHABET[below(&mut rng, ALPHABET.len() as u64) as usize]
                }
            })
            .collect();
        check(&bytes);
    }
    let seed = sealed(base()).to_canonical_json().unwrap();
    for _ in 0..2000 {
        let mut bytes = seed.clone();
        for _ in 0..1 + below(&mut rng, 3) {
            match below(&mut rng, 3) {
                0 => {
                    let j = below(&mut rng, bytes.len() as u64) as usize;
                    bytes[j] ^= 1 << below(&mut rng, 8);
                }
                1 => {
                    let j = below(&mut rng, bytes.len() as u64 + 1) as usize;
                    bytes.insert(j, b"{}[]\",: 9"[below(&mut rng, 9) as usize]);
                }
                _ => {
                    let j = below(&mut rng, bytes.len() as u64) as usize;
                    bytes.remove(j);
                }
            }
            if bytes.is_empty() {
                bytes.push(b'{');
            }
        }
        check(&bytes);
    }
}

/// REQ-DSL-010, REQ-NFR-006 (R12): a document nested far beyond MAX_DEPTH inside an AST position
/// (built as text, so the test cannot overflow) is a `Canonical` error, not a stack overflow.
#[test]
fn req_dsl_010_over_deep_ast_position_is_canonical_error() {
    let text = String::from_utf8(sealed(base()).to_canonical_json().unwrap()).unwrap();
    let deep_math = |n: usize| {
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(r#"{"MATH":{"op":"ADD","rhs":"X","lhs":"#);
        }
        s.push_str(r#"{"LITERAL":1}"#);
        for _ in 0..n {
            s.push_str("}}");
        }
        s
    };
    let deep_arrays = |n: usize| "[".repeat(n) + &"]".repeat(n);
    for (needle, replacement) in [
        (
            r#""power":{"LITERAL":2}"#,
            format!("\"power\":{}", deep_math(MAX_DEPTH)),
        ),
        (
            r#""power":{"LITERAL":2}"#,
            format!("\"power\":{}", deep_math(50_000)),
        ),
        (
            r#""power":{"LITERAL":2}"#,
            format!("\"power\":{}", deep_arrays(100_000)),
        ),
        (
            r#""text":[{"#,
            format!("\"text\":[{},{{", deep_arrays(MAX_DEPTH + 5)),
        ),
        (
            r#""partnerAbilities":["#,
            format!("\"partnerAbilities\":[{},", deep_arrays(10_000)),
        ),
        (
            r#""abilities":["#,
            format!("\"abilities\":[{},", deep_arrays(MAX_DEPTH * 3)),
        ),
    ] {
        assert!(text.contains(needle), "{needle}");
        let doc = text.replacen(needle, &replacement, 1);
        match load(doc.as_bytes()) {
            Err(CardDefinitionError::Canonical(_)) => {}
            other => panic!("{needle}: expected Canonical, got {other:?}"),
        }
    }
}

/// REQ-DSL-003, REQ-NFR-006: a deep but in-limit MATH chain seals and round-trips; past the
/// document limit `seal` is a `Serialize` error.
#[test]
fn req_dsl_003_nfr_006_deep_valid_math_seals_up_to_the_limit() {
    let chain = |levels: usize| {
        let mut v = json!({"LITERAL": 1});
        for _ in 0..levels {
            v = math("ADD", v, json!("X"));
        }
        v
    };
    let mut d = base();
    Pos::Power(0).set(&mut d, chain(40));
    let s = sealed(d);
    assert_eq!(load(&s.to_canonical_json().unwrap()), Ok(s));
    let mut d = base();
    Pos::Power(0).set(&mut d, chain(200));
    assert!(matches!(d.seal(), Err(CanonicalError::Serialize(_))));
}
