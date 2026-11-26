// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared SYNTHETIC fixtures for the FP-015 DSL lint spec suites (REQ-ING-009). Black-box:
//! written from the work-item brief only. This module deliberately does not import
//! `fineprint_dsl::lint`, so the fixture self-check suite (`spec_dsl_lint_fixtures.rs`) compiles
//! on its own. Every card here is synthetic (R5): neutral ids, marker strings instead of prose.

#![allow(dead_code)]

use std::collections::BTreeMap;

use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, CardFaceDSL, ColorBitmask, CommanderEligibilityDSL, FaceKind,
    LayoutBehavior, Legality, Origin, ProvenanceDSL, SupportStatus,
};
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol};
use serde_json::{Value, json};

#[path = "../typed_ast_support/mod.rs"]
mod typed_ast_support;
#[allow(unused_imports)]
pub use typed_ast_support::{ability_of, pa, tokens_of, tx, vx, wire};

/// Distinctive marker string put in LITERAL token text and ability words; no lint message may
/// contain it (S-0).
pub const MARKER: &str = "zqxmarkerprose";

// ---------------------------------------------------------------- JSON helpers

/// `base` with member `k` set to `v`.
pub fn with(base: &Value, k: &str, v: Value) -> Value {
    let mut out = base.clone();
    out.as_object_mut()
        .expect("fixture is an object")
        .insert(k.to_string(), v);
    out
}

/// `base` without member `k`.
pub fn without(base: &Value, k: &str) -> Value {
    let mut out = base.clone();
    out.as_object_mut().expect("fixture is an object").remove(k);
    out
}

/// A JSON number parsed from text (used for non-integer numbers without float types).
pub fn num(text: &str) -> Value {
    serde_json::from_str(text).expect("valid JSON number text")
}

// ---------------------------------------------------------------- instructions

/// A cheap non-library instruction (stands in for "adds mana"; there is no add-mana op).
pub fn native() -> Value {
    json!({"op":"NATIVE_EFFECT","id":"test.native_effect","version":1,"params":{}})
}

/// `NATIVE_EFFECT` whose opaque params hold `params`.
pub fn native_with(params: Value) -> Value {
    json!({"op":"NATIVE_EFFECT","id":"test.native_effect","version":1,"params":params})
}

pub fn draw() -> Value {
    json!({"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}})
}

pub fn mill() -> Value {
    json!({"op":"MILL","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}})
}

pub fn search_library() -> Value {
    json!({"op":"SEARCH_LIBRARY","player":{"ref":"CONTROLLER"},
           "selector":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},
           "count":{"LITERAL":1},"toZone":"GRAVEYARD"})
}

pub fn shuffle() -> Value {
    json!({"op":"SHUFFLE","player":{"ref":"CONTROLLER"}})
}

/// Taps the object a linked ability refers to (a `{"ref":"LINKED","linkId":..}` reference).
pub fn tap_linked(link: &str) -> Value {
    json!({"op":"TAP_OR_UNTAP","action":"TAP","objects":{"ref":"LINKED","linkId":link}})
}

pub fn create_token(token_def: &str) -> Value {
    json!({"op":"CREATE_TOKEN","tokenDef":token_def,"count":{"LITERAL":1}})
}

pub fn create_token_inline() -> Value {
    json!({"op":"CREATE_TOKEN","inline":{"k1":1},"count":{"LITERAL":1}})
}

/// `REPEAT_WHILE` with the given `maxIter` (None = member absent).
pub fn repeat_while(max_iter: Option<Value>, body: Value) -> Value {
    let mut v = json!({"op":"REPEAT_WHILE","cond":{"pred":"TAPPED"},"body":body});
    if let Some(m) = max_iter {
        v = with(&v, "maxIter", m);
    }
    v
}

pub fn if_then(then: Value) -> Value {
    json!({"op":"IF","cond":{"pred":"TAPPED"},"then":then})
}

pub fn for_each(body: Value) -> Value {
    json!({"op":"FOR_EACH","iterator":{"ref":"SELF"},"body":body})
}

pub fn may(body: Value) -> Value {
    json!({"op":"MAY","player":{"ref":"CONTROLLER"},"body":body})
}

// ---------------------------------------------------------------- abilities

pub fn tap_cost() -> Value {
    json!([{"cost":"TAP_SOURCE"}])
}

pub fn activated(id: &str, cost: Value, effect: Value) -> Value {
    json!({"abilityId":id,"kind":"ACTIVATED","cost":cost,"effect":effect})
}

pub fn spell(id: &str, effect: Value) -> Value {
    json!({"abilityId":id,"kind":"SPELL","effect":effect})
}

pub fn triggered(id: &str, trigger: Value, effect: Value) -> Value {
    json!({"abilityId":id,"kind":"TRIGGERED","trigger":trigger,"effect":effect})
}

pub fn event_trigger(kind: &str) -> Value {
    json!({"mode":"EVENT","event":{"kind":kind}})
}

pub fn step_trigger() -> Value {
    json!({"mode":"PHASE_STEP","step":"UPKEEP","whose":{"ref":"CONTROLLER"}})
}

pub fn static_ab(id: &str, effects: Value) -> Value {
    json!({"abilityId":id,"kind":"STATIC","effects":effects})
}

pub fn keyword(id: &str) -> Value {
    json!({"abilityId":id,"kind":"KEYWORD","keyword":"TEST_KEYWORD"})
}

pub fn replacement(id: &str, event: &str, replace_with: Value) -> Value {
    json!({"abilityId":id,"kind":"REPLACEMENT",
           "match":{"event":event,"affected":{"ref":"SELF"}},"replaceWith":replace_with})
}

pub fn modify_event() -> Value {
    json!({"op":"MODIFY_EVENT","set":{"to":"EXILE"}})
}

pub fn instead() -> Value {
    json!({"op":"INSTEAD","instructions":[{"op":"POPULATE"}]})
}

pub fn skip_op() -> Value {
    json!({"op":"SKIP"})
}

pub fn prevent_op() -> Value {
    json!({"op":"PREVENT"})
}

/// One required target (CR 115.6 for min 0 is tested separately).
pub fn one_target() -> Value {
    json!([{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1}])
}

/// A valid activated mana ability (CR 605.1a): tap cost, no targets, not loyalty, no library
/// op, non-empty effect.
pub fn good_activated_mana(id: &str) -> Value {
    with(
        &activated(id, tap_cost(), json!([native()])),
        "isManaAbility",
        json!(true),
    )
}

/// A valid triggered mana ability (CR 605.1b) on the given event kind.
pub fn good_triggered_mana(id: &str, event: &str) -> Value {
    with(
        &triggered(id, event_trigger(event), json!([native()])),
        "isManaAbility",
        json!(true),
    )
}

pub fn char_effect(layer: Option<&str>) -> Value {
    let mut v = json!({"type":"CHAR","affected":{"ref":"SELF"},"modification":{"op":"TEST_OP"}});
    if let Some(l) = layer {
        v = with(&v, "layer", json!(l));
    }
    v
}

pub fn rule_effect(kind: Option<&str>) -> Value {
    let mut v = json!({"type":"RULE"});
    if let Some(k) = kind {
        v = with(&v, "kind", json!(k));
    }
    v
}

pub fn player_effect() -> Value {
    json!({"type":"PLAYER","affected":{"ref":"CONTROLLER"},"modification":{"op":"TEST_OP"}})
}

/// `ab` with `textSpan` `[start, end]`.
pub fn spanned(ab: &Value, start: u64, end: u64) -> Value {
    with(ab, "textSpan", json!([start, end]))
}

/// `ab` with `linkIds` `ids`.
pub fn linked(ab: &Value, ids: &[&str]) -> Value {
    with(ab, "linkIds", json!(ids))
}

// ---------------------------------------------------------------- text

pub fn lit(id: &str, text: &str) -> Value {
    json!({"id":id,"kind":"LITERAL","text":text})
}

/// `n` LITERAL tokens `t0..t{n-1}`, each holding the marker.
pub fn tokens(n: usize) -> Vec<Value> {
    (0..n)
        .map(|i| lit(&format!("t{i}"), &format!("{MARKER} {i}")))
        .collect()
}

// ---------------------------------------------------------------- mana

pub fn mana(symbols: Vec<ManaSymbol>) -> ManaCostDSL {
    ManaCostDSL::new(symbols).expect("valid synthetic mana cost")
}

pub fn bits(b: u8) -> ColorBitmask {
    ColorBitmask::new(b).expect("bits <= 31")
}

pub fn or(a: ColorBitmask, b: ColorBitmask) -> ColorBitmask {
    bits(a.bits() | b.bits())
}

pub fn mask_of(c: Color) -> ColorBitmask {
    match c {
        Color::White => ColorBitmask::WHITE,
        Color::Blue => ColorBitmask::BLUE,
        Color::Black => ColorBitmask::BLACK,
        Color::Red => ColorBitmask::RED,
        Color::Green => ColorBitmask::GREEN,
    }
}

// ---------------------------------------------------------------- faces and definitions

pub fn face(kind: FaceKind, name: &str) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: kind,
        name: name.to_string(),
        mana_cost: None,
        color_indicator: None,
        supertypes: vec![],
        card_types: vec!["Artifact".to_string()],
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

/// A MAIN face with the given abilities and no text.
pub fn face_with(abilities: Vec<Value>) -> CardFaceDSL {
    let mut f = face(FaceKind::Main, "Test Face");
    f.abilities = abilities.into_iter().map(ability_of).collect();
    f
}

/// A definition with the given layout and faces: no costs, colorless identity, AUTO_PARSED.
pub fn def_of(layout: LayoutBehavior, faces: Vec<CardFaceDSL>) -> CardDefinitionDSL {
    let mut legalities = BTreeMap::new();
    legalities.insert("testformat".to_string(), Legality::Legal);
    CardDefinitionDSL {
        card_def_id: "test.lint_subject".to_string(),
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
        layout,
        faces,
        meld_partner: None,
        color_identity: ColorBitmask::NONE,
        commander_eligibility: None,
        legalities,
        digital_only: false,
        related_tokens: None,
    }
}

/// One NORMAL face holding `abilities`.
pub fn def_abilities(abilities: Vec<Value>) -> CardDefinitionDSL {
    def_of(LayoutBehavior::Normal, vec![face_with(abilities)])
}

/// A fully clean definition: two faces (DFC_NONMODAL), text fully covered, power/toughness,
/// a mana cost with a matching identity, a partner keyword, related tokens, linked abilities,
/// a mana ability, a tier-hinted replacement, a static ability and a bounded loop. Every
/// ability word and token holds [`MARKER`].
pub fn clean_def() -> CardDefinitionDSL {
    let mut front = face(FaceKind::Main, "Test Front");
    front.mana_cost = Some(mana(vec![
        ManaSymbol::Generic(1),
        ManaSymbol::Colored(Color::Blue),
    ]));
    front.card_types = vec!["Creature".to_string()];
    front.power = Some(vx(json!({"LITERAL": 1})));
    front.toughness = Some(vx(json!({"LITERAL": 1})));
    front.text = tx(tokens(6));
    front.abilities = vec![
        ability_of(spanned(
            &with(&good_activated_mana("a0"), "abilityWord", json!(MARKER)),
            0,
            1,
        )),
        ability_of(spanned(&linked(&keyword("a1"), &["l1"]), 2, 2)),
        ability_of(spanned(
            &with(
                &replacement("a2", "ENTERS", json!([modify_event()])),
                "tierHint",
                json!("COPY_ETB"),
            ),
            3,
            4,
        )),
        ability_of(spanned(
            &static_ab(
                "a3",
                json!([
                    char_effect(Some("6")),
                    rule_effect(Some("SKIP")),
                    player_effect()
                ]),
            ),
            5,
            5,
        )),
    ];
    let mut back = face(FaceKind::Back, "Test Back");
    back.color_indicator = Some(ColorBitmask::RED);
    back.text = tx(vec![lit("b0", MARKER), lit("b1", MARKER)]);
    back.abilities = vec![
        ability_of(spanned(
            &spell(
                "b0",
                json!([
                    tap_linked("l1"),
                    repeat_while(Some(json!(2)), json!([draw()])),
                    create_token("test.token_a"),
                ]),
            ),
            0,
            1,
        )),
        ability_of(spanned(&good_triggered_mana("b1", "RESOLVED"), 0, 0)),
    ];
    let mut d = def_of(LayoutBehavior::DfcNonmodal, vec![front, back]);
    d.color_identity = or(ColorBitmask::BLUE, ColorBitmask::RED);
    d.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: true,
        partner_abilities: vec![pa(
            json!({"abilityId":"p1","kind":"KEYWORD","keyword":"PARTNER"}),
        )],
    });
    d.related_tokens = Some(vec!["test.token_a".to_string()]);
    d
}

/// Every AST position of `d` whose wire form does not decode back to the same typed value, as
/// `(path, error)`; used by the fixture self-check so that a fixture meant to be clean is proven
/// decodable by this suite itself. #232 FP-164: the positions are typed, so this is the
/// single-spelling round trip `decode(encode(x)) == x` of each one (formerly `to_real`).
pub fn undecodable_positions(d: &CardDefinitionDSL) -> Vec<(String, String)> {
    fn check<T>(x: &T, path: String, out: &mut Vec<(String, String)>)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq,
    {
        let r = serde_json::to_value(x)
            .map_err(|e| e.to_string())
            .and_then(|v| serde_json::from_value::<T>(v).map_err(|e| e.to_string()))
            .and_then(|back| {
                if &back == x {
                    Ok(())
                } else {
                    Err("does not round-trip".to_string())
                }
            });
        if let Err(e) = r {
            out.push((path, e));
        }
    }
    let mut out = Vec::new();
    for (i, f) in d.faces.iter().enumerate() {
        if let Some(p) = &f.power {
            check(p, format!("faces[{i}].power"), &mut out);
        }
        if let Some(t) = &f.toughness {
            check(t, format!("faces[{i}].toughness"), &mut out);
        }
        check(&f.text, format!("faces[{i}].text"), &mut out);
        for (j, a) in f.abilities.iter().enumerate() {
            check(a, format!("faces[{i}].abilities[{j}]"), &mut out);
        }
    }
    if let Some(c) = &d.commander_eligibility {
        for (k, p) in c.partner_abilities.iter().enumerate() {
            check(
                p,
                format!("commanderEligibility.partnerAbilities[{k}]"),
                &mut out,
            );
        }
    }
    out
}

/// Asserts that the single ability `v` decodes (a fixture mistake fails here, not in a lint).
pub fn assert_decodes(v: &Value) {
    if let Err(e) = serde_json::from_value::<AbilityAST>(v.clone()) {
        panic!("fixture ability does not decode: {e}\n{v}");
    }
}

/// Asserts that the single ability `v` does NOT decode.
pub fn assert_undecodable(v: &Value) {
    assert!(
        serde_json::from_value::<AbilityAST>(v.clone()).is_err(),
        "fixture ability unexpectedly decodes: {v}"
    );
}

// ---------------------------------------------------------------- #232 FP-164 typed positions

/// `d` sealed, as a JSON document, with each `(JSON pointer, value)` patch applied (an array
/// index equal to the length appends) and `definitionHash` recomputed independently (RFC 8785
/// + SHA-256 over the document without it).
pub fn patched_document(d: &CardDefinitionDSL, patches: &[(&str, Value)]) -> Value {
    let mut v = serde_json::to_value(d.clone().seal().expect("fixture seals")).unwrap();
    for (ptr, x) in patches {
        let (parent, last) = ptr.rsplit_once('/').expect("pointer");
        match v
            .pointer_mut(parent)
            .unwrap_or_else(|| panic!("{ptr}: no parent"))
        {
            Value::Array(items) => {
                let i: usize = last.parse().expect("index");
                if i == items.len() {
                    items.push(x.clone());
                } else {
                    items[i] = x.clone();
                }
            }
            Value::Object(m) => {
                m.insert(last.to_string(), x.clone());
            }
            other => panic!("{ptr}: parent is {other}"),
        }
    }
    let obj = v.as_object_mut().expect("object");
    obj.remove("definitionHash");
    // A patch the document-level checks refuse (a noncharacter, an unsafe or non-integer number)
    // has no canonical form; it keeps a placeholder digest, `from_json` refuses it first anyway.
    let h = fineprint_dsl::canonical::canonicalize(
        &serde_json::to_vec(&Value::Object(obj.clone())).unwrap(),
    )
    .map(|bytes| fineprint_base::hash::sha256(&bytes).to_hex())
    .unwrap_or_else(|_| "0".repeat(64));
    obj.insert("definitionHash".to_string(), json!(h));
    v
}

/// #232 FP-164 (C5, REQ-DSL-010): AST positions of a `CardDefinitionDSL` are valid by
/// construction, so lint input can no longer hold invalid AST content. Where a lint test fed such
/// content, it now asserts that the same JSON is refused: plain serde of the definition fails and
/// `from_json` reports `Schema` (with a correctly recomputed `definitionHash`).
pub fn assert_document_schema_rejected(doc: &Value, what: &str) {
    assert!(
        serde_json::from_value::<CardDefinitionDSL>(doc.clone()).is_err(),
        "{what}: plain serde accepted invalid AST content"
    );
    let bytes = serde_json::to_vec(doc).unwrap();
    // Document-level checks run before the typed schema (C4): content that fails them is a
    // `Canonical` error, everything else is `Schema`.
    let document_level = fineprint_dsl::canonical::canonicalize(&bytes).is_err();
    match CardDefinitionDSL::from_json(&bytes) {
        Err(fineprint_dsl::card::CardDefinitionError::Schema(_)) if !document_level => {}
        Err(fineprint_dsl::card::CardDefinitionError::Canonical(_)) if document_level => {}
        other => panic!("{what}: expected from_json Schema, got {other:?}"),
    }
}

/// `def_abilities(abilities)` when every ability decodes as the typed `AbilityAST`; otherwise
/// asserts (C5) that the same definition document with these abilities on face 0 is refused by
/// `from_json` with `Schema`, and returns `None`.
pub fn def_abilities_or_schema(abilities: Vec<Value>) -> Option<CardDefinitionDSL> {
    if abilities
        .iter()
        .all(|a| serde_json::from_value::<AbilityAST>(a.clone()).is_ok())
    {
        return Some(def_abilities(abilities));
    }
    let doc = patched_document(
        &def_abilities(vec![]),
        &[("/faces/0/abilities", Value::Array(abilities.clone()))],
    );
    assert_document_schema_rejected(&doc, &format!("abilities {}", Value::Array(abilities)));
    None
}
