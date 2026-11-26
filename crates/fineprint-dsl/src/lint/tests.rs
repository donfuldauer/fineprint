// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the lints (REQ-ING-009; REQ-OBJ-001 and REQ-ING-002 for the characteristics
//! checks). Every card, id and word is synthetic (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::prng::Pcg64Dxsm;
use serde_json::{Map, Value, json};

use super::*;
use crate::card::{
    AbilityAST, CardDefinitionDSL, CardFaceDSL, ColorBitmask, CommanderEligibilityDSL, DSL_VERSION,
    FaceKind, LayoutBehavior, Legality, Origin, PartnerAbilityDSL, ProvenanceDSL, SupportStatus,
    TextTokenStream, ValueExpressionAST,
};
use crate::mana::ManaCostDSL;

// ---------------------------------------------------------------------------------------------
// Helpers

/// `n` synthetic LITERAL text tokens.
fn tokens(n: usize) -> TextTokenStream {
    TextTokenStream(
        (0..n)
            .map(|k| json!({"id": format!("t{k}"), "kind": "LITERAL", "text": "word"}))
            .collect(),
    )
}

/// A synthetic face of `kind` with `cost`, `n` text tokens and `abilities`.
fn face_of(kind: FaceKind, cost: Option<&str>, n: usize, abilities: Vec<Value>) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: kind,
        name: "test.face".to_owned(),
        mana_cost: cost.map(|c| ManaCostDSL::parse(c).expect("cost")),
        color_indicator: None,
        supertypes: Vec::new(),
        card_types: vec!["Creature".to_owned()],
        subtypes: Vec::new(),
        power: Some(ValueExpressionAST(json!({"LITERAL": 2}))),
        toughness: Some(ValueExpressionAST(json!({"LITERAL": 2}))),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: tokens(n),
        abilities: abilities.into_iter().map(AbilityAST).collect(),
    }
}

/// A MAIN face with `cost`, `n` tokens and `abilities`.
fn face(cost: Option<&str>, n: usize, abilities: Vec<Value>) -> CardFaceDSL {
    face_of(FaceKind::Main, cost, n, abilities)
}

/// A synthetic definition with `faces` and identity `identity`.
fn def_of(faces: Vec<CardFaceDSL>, identity: ColorBitmask) -> CardDefinitionDSL {
    CardDefinitionDSL {
        card_def_id: "test.lint_card".to_owned(),
        dsl_version: DSL_VERSION.to_owned(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "0".repeat(64),
            origin: Origin::ClientOverride,
            parser_version: None,
            override_source: None,
            verified_by: None,
            rules_baseline: "test-baseline".to_owned(),
        },
        support_status: SupportStatus::AutoParsed,
        known_gaps: None,
        layout: LayoutBehavior::Normal,
        faces,
        meld_partner: None,
        color_identity: identity,
        commander_eligibility: None,
        legalities: BTreeMap::from([("test_format".to_owned(), Legality::Legal)]),
        digital_only: false,
        related_tokens: None,
    }
}

/// A keyword ability `id` covering token span `[s, e]`.
fn keyword(id: &str, s: u32, e: u32) -> Value {
    json!({"abilityId": id, "kind": "KEYWORD", "keyword": "TEST_KEYWORD", "textSpan": [s, e]})
}

/// A one-face definition with cost `{1}{W}`, one token and `abilities` (identity white).
fn one_face(abilities: Vec<Value>) -> CardDefinitionDSL {
    def_of(
        vec![face(Some("{1}{W}"), 1, abilities)],
        ColorBitmask::WHITE,
    )
}

/// The clean synthetic definition `test.lint_card`: no finding under any check.
fn clean() -> CardDefinitionDSL {
    one_face(vec![keyword("a1", 0, 0)])
}

/// Lints with an empty context.
fn run(def: &CardDefinitionDSL) -> LintReport {
    lint_definition(def, &LintContext::default())
}

/// A finding reduced to (severity, path, face, ability).
type Short = (Severity, String, Option<usize>, Option<usize>);

/// The findings of `id`, reduced.
fn of(report: &LintReport, id: LintId) -> Vec<Short> {
    report
        .with_id(id)
        .map(|l| {
            (
                l.severity,
                l.location.path.clone(),
                l.location.face_index,
                l.location.ability_index,
            )
        })
        .collect()
}

/// Asserts the report has no finding of `id`.
fn none_of(report: &LintReport, id: LintId) {
    assert_eq!(of(report, id), Vec::new(), "{id}: {report:#?}");
}

/// Overwrites the members of `base` with those of `extra` (a `null` removes the member).
fn merge(base: &mut Value, extra: Value) {
    let (Value::Object(b), Value::Object(e)) = (base, extra) else {
        panic!("objects expected")
    };
    for (k, v) in e {
        if v.is_null() {
            b.remove(&k);
        } else {
            b.insert(k, v);
        }
    }
}

/// An ACTIVATED ability with `members` merged over a mana-ability-shaped base.
fn activated(members: Value) -> Value {
    let mut base = json!({
        "abilityId": "m1",
        "kind": "ACTIVATED",
        "isManaAbility": true,
        "cost": [{"cost": "TAP_SOURCE"}],
        "effect": [{"op": "POPULATE"}],
        "textSpan": [0, 0],
    });
    merge(&mut base, members);
    base
}

/// A TRIGGERED ability with `members` merged over a mana-ability-shaped base.
fn triggered(members: Value) -> Value {
    let mut base = json!({
        "abilityId": "m2",
        "kind": "TRIGGERED",
        "isManaAbility": true,
        "trigger": {"mode": "EVENT", "event": {"kind": "ACTIVATED", "subject": {"ref": "SELF"}}},
        "effect": [{"op": "POPULATE"}],
        "textSpan": [0, 0],
    });
    merge(&mut base, members);
    base
}

/// A target slot with `min` 0 (still "requires a target", CR 115.6).
fn slot() -> Value {
    json!({"slot": 0, "selector": {"ref": "SELF"}, "min": 0, "max": 1})
}

/// Asserts `ability` decodes (so no FPL-SCHEMA finding).
fn decodes(ability: &Value) {
    if let Err(e) = AbilityAST(ability.clone()).to_real() {
        panic!("fixture does not decode: {e}: {ability}");
    }
}

/// A context with source faces `(mana value, colors)` and source identity.
fn source(
    faces: Vec<(Option<u32>, Option<ColorBitmask>)>,
    identity: Option<ColorBitmask>,
) -> LintContext {
    LintContext {
        source: Some(SourceCharacteristics {
            faces: faces
                .into_iter()
                .map(|(mana_value, colors)| SourceFaceCharacteristics { mana_value, colors })
                .collect(),
            color_identity: identity,
        }),
        known_token_ids: None,
    }
}

/// A color mask from bits.
fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("mask")
}

// ---------------------------------------------------------------------------------------------
// API, Display and the clean definition

/// REQ-ING-009: the twelve stable lint id strings, `ALL` order, `Display` and severity strings.
#[test]
fn req_ing_009_lint_ids_and_severity() {
    let strings: Vec<&str> = LintId::ALL.iter().map(|id| id.as_str()).collect();
    assert_eq!(
        strings,
        [
            "FPL-SCHEMA",
            "FPL-MANA-ABILITY",
            "FPL-TIER-HINT",
            "FPL-LAYER-KIND",
            "FPL-TEXT-COVERAGE",
            "FPL-LOOK-BACK",
            "FPL-MAX-ITER",
            "FPL-TOKEN-REF",
            "FPL-LINK-ID",
            "FPL-MANA-VALUE",
            "FPL-COLORS",
            "FPL-COLOR-IDENTITY",
        ]
    );
    assert_eq!(LintId::SCHEMA.to_string(), "FPL-SCHEMA");
    assert!(Severity::Error < Severity::Warning);
    assert_eq!(Severity::Error.as_str(), "ERROR");
    assert_eq!(Severity::Warning.to_string(), "WARNING");
    assert!(LintId::COLORS < LintId::SCHEMA);
}

/// REQ-ING-009: `Lint` Display is `"{SEVERITY} {id} {path}{span}: {message}"`.
#[test]
fn req_ing_009_display_format() {
    let mut lint = Lint {
        id: LintId::TEXT_COVERAGE,
        severity: Severity::Warning,
        location: LintLocation::face(0, ".text"),
        message: "gap".to_owned(),
    };
    assert_eq!(
        lint.to_string(),
        "WARNING FPL-TEXT-COVERAGE faces[0].text: gap"
    );
    lint.location.text_span = Some((2, 5));
    assert_eq!(
        lint.to_string(),
        "WARNING FPL-TEXT-COVERAGE faces[0].text [2,5]: gap"
    );
}

/// REQ-ING-009, REQ-ING-002: the clean synthetic definition passes every check, with a full
/// context too.
#[test]
fn req_ing_009_clean_definition_passes_every_check() {
    let def = clean();
    assert!(def.validate_asts().is_ok());
    let report = run(&def);
    assert!(report.is_empty(), "{report:#?}");
    assert!(!report.has_errors());
    let mut ctx = source(
        vec![(Some(2), Some(ColorBitmask::WHITE))],
        Some(ColorBitmask::WHITE),
    );
    ctx.known_token_ids = Some(BTreeSet::new());
    assert!(lint_definition(&def, &ctx).is_empty());
}

/// REQ-ING-009: report accessors.
#[test]
fn req_ing_009_report_accessors() {
    let mut def = one_face(vec![
        json!({"abilityId": "x"}),
        json!({"abilityId": "l1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD", "linkIds": ["solo"]}),
    ]);
    def.faces[0].text = tokens(0);
    let report = run(&def);
    assert!(report.has_errors());
    assert_eq!(report.errors().count(), 1);
    assert_eq!(report.warnings().count(), 1);
    assert_eq!(report.with_id(LintId::SCHEMA).count(), 1);
    assert_eq!(report.with_id(LintId::LINK_ID).count(), 1);
    assert_eq!(report.lints.len(), 2);
}

// ---------------------------------------------------------------------------------------------
// FPL-SCHEMA

/// REQ-ING-009 (schema validity), REQ-DSL-003, REQ-DSL-010, REQ-DSL-012: every failing
/// position is reported, not only the first.
#[test]
fn req_ing_009_schema_reports_every_position() {
    let mut def = one_face(vec![
        keyword("a1", 0, 0),
        json!({"kind": "NOPE"}),
        json!(null),
    ]);
    def.faces[0].power = Some(ValueExpressionAST(Value::Null));
    def.faces[0].toughness = Some(ValueExpressionAST(json!("x")));
    def.faces[0].text = TextTokenStream(vec![
        json!({"id": "t0", "kind": "LITERAL", "text": "w"}),
        json!(7),
    ]);
    def.faces[0].abilities[0] = AbilityAST(keyword("a1", 0, 1));
    def.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: true,
        partner_abilities: vec![
            PartnerAbilityDSL(json!({"abilityId": "p0", "kind": "KEYWORD", "keyword": "PARTNER"})),
            PartnerAbilityDSL(json!({"abilityId": "p1", "kind": "SPELL", "effect": []})),
        ],
    });
    let report = run(&def);
    assert_eq!(
        of(&report, LintId::SCHEMA),
        vec![
            (
                Severity::Error,
                "commanderEligibility.partnerAbilities[1]".to_owned(),
                None,
                None
            ),
            (Severity::Error, "faces[0].power".to_owned(), Some(0), None),
            (Severity::Error, "faces[0].text".to_owned(), Some(0), None),
            (
                Severity::Error,
                "faces[0].toughness".to_owned(),
                Some(0),
                None
            ),
            (
                Severity::Error,
                "faces[0].abilities[1]".to_owned(),
                Some(0),
                Some(1)
            ),
            (
                Severity::Error,
                "faces[0].abilities[2]".to_owned(),
                Some(0),
                Some(2)
            ),
        ]
    );
    let partner = report.with_id(LintId::SCHEMA).next().expect("partner");
    assert!(partner.message.contains("KEYWORD"), "{}", partner.message);
}

/// REQ-ING-009: a schema-valid definition has no FPL-SCHEMA finding; decoder messages are
/// clipped.
#[test]
fn req_ing_009_schema_passes_and_clips() {
    none_of(&run(&clean()), LintId::SCHEMA);
    let long = "Z".repeat(5000);
    let def = one_face(vec![json!({"abilityId": "a", "kind": long})]);
    let report = run(&def);
    let lint = report.with_id(LintId::SCHEMA).next().expect("schema");
    assert!(
        lint.message.chars().count() <= CLIP_MESSAGE + 3,
        "{}",
        lint.message.len()
    );
}

// ---------------------------------------------------------------------------------------------
// FPL-MANA-ABILITY

/// REQ-ING-009 (CR 605.1a): a targetless, non-loyalty activated ability with an effect and no
/// library movement may declare `isManaAbility: true`; `false` and `None` are never findings.
#[test]
fn req_ing_009_mana_ability_activated_passes() {
    let ok = activated(json!({}));
    decodes(&ok);
    none_of(&run(&one_face(vec![ok])), LintId::MANA_ABILITY);
    // Not declared: never a finding, even with a target and no effect.
    for flag in [json!(false), Value::Null] {
        let a = activated(json!({"isManaAbility": flag, "targets": [slot()], "effect": []}));
        decodes(&a);
        none_of(&run(&one_face(vec![a])), LintId::MANA_ABILITY);
    }
}

/// REQ-ING-009 (CR 605.1a, CR 115.6): each violated criterion of an activated ability is one
/// Error.
#[test]
fn req_ing_009_mana_ability_activated_criteria() {
    let modal_target = json!({"min": 1, "max": 1, "modes": [
        {"id": "m", "targets": [slot()], "effect": [{"op": "POPULATE"}]}
    ]});
    let search = json!({"op": "SEARCH_LIBRARY", "player": {"ref": "CONTROLLER"},
        "selector": {"zones": ["LIBRARY"], "pred": "IS_LAND"}, "count": {"LITERAL": 1},
        "toZone": "HAND"});
    let cases = [
        // CR 115.6: requires a target even when zero may be chosen.
        (activated(json!({"targets": [slot()]})), 1),
        (activated(json!({"modes": modal_target})), 1),
        // Loyalty ability: flag, LOYALTY cost, LOYALTY inside COMPOSITE.
        (activated(json!({"isLoyalty": true})), 1),
        (
            activated(json!({"cost": [{"cost": "LOYALTY", "delta": {"LITERAL": 1}}]})),
            1,
        ),
        (
            activated(json!({"cost": [{"cost": "COMPOSITE", "costs": [
                {"cost": "TAP_SOURCE"}, {"cost": "LOYALTY", "delta": {"LITERAL": -1}}
            ]}]})),
            1,
        ),
        // Library movement in the effect (nested too) or the cost.
        (
            activated(
                json!({"effect": [{"op": "DRAW", "player": {"ref": "CONTROLLER"},
                "count": {"LITERAL": 1}}]}),
            ),
            1,
        ),
        (
            activated(
                json!({"effect": [{"op": "MILL", "player": {"ref": "CONTROLLER"},
                "count": {"LITERAL": 1}}]}),
            ),
            1,
        ),
        (
            activated(
                json!({"effect": [{"op": "PUT_ON_TOP_OR_BOTTOM", "objects": {"ref": "SELF"},
                "player": {"ref": "CONTROLLER"}}]}),
            ),
            1,
        ),
        (
            activated(
                json!({"effect": [{"op": "IF", "cond": {"pred": "IS_CREATURE"},
                "then": [search]}]}),
            ),
            1,
        ),
        (
            activated(json!({"effect": [{"op": "MOVE", "objects": {"ref": "SELF"},
                "toZone": "LIBRARY"}]})),
            1,
        ),
        (
            activated(json!({"effect": [{"op": "MOVE",
                "objects": {"zones": ["LIBRARY"], "pred": "IS_LAND"}, "toZone": "HAND"}]})),
            1,
        ),
        (
            activated(
                json!({"cost": [{"cost": "EXILE_FROM_ZONE", "zone": "LIBRARY",
                "selector": {"zones": ["LIBRARY"]}, "count": {"LITERAL": 1}}]}),
            ),
            1,
        ),
        // No effect at all (cannot add mana).
        (activated(json!({"effect": []})), 1),
        // Two criteria at once: two findings.
        (activated(json!({"isLoyalty": true, "effect": []})), 2),
    ];
    for (a, n) in cases {
        decodes(&a);
        let report = run(&one_face(vec![a.clone()]));
        let found = of(&report, LintId::MANA_ABILITY);
        assert_eq!(found.len(), n, "{a}: {report:#?}");
        for f in found {
            assert_eq!(
                f,
                (
                    Severity::Error,
                    "faces[0].abilities[0]".to_owned(),
                    Some(0),
                    Some(0)
                )
            );
        }
    }
    // A MOVE elsewhere is fine; an effect only in a mode counts as an effect.
    for a in [
        activated(
            json!({"effect": [{"op": "MOVE", "objects": {"ref": "SELF"}, "toZone": "HAND"}]}),
        ),
        activated(
            json!({"effect": [], "modes": {"min": 1, "max": 1, "modes": [
                {"id": "m", "effect": [{"op": "POPULATE"}]}
            ]}}),
        ),
    ] {
        decodes(&a);
        none_of(&run(&one_face(vec![a])), LintId::MANA_ABILITY);
    }
}

/// REQ-ING-009 (CR 605.1b): triggered abilities: target, trigger event and effect criteria;
/// the loyalty and library criteria do not apply.
#[test]
fn req_ing_009_mana_ability_triggered_criteria() {
    let pass = [
        triggered(json!({})),
        triggered(json!({"trigger": {"mode": "EVENT", "event": {"kind": "RESOLVED"}}})),
        // CR 605.1b has no library criterion.
        triggered(
            json!({"effect": [{"op": "SEARCH_LIBRARY", "player": {"ref": "CONTROLLER"},
            "selector": {"zones": ["LIBRARY"]}, "count": {"LITERAL": 1}, "toZone": "HAND"}]}),
        ),
    ];
    for a in pass {
        decodes(&a);
        none_of(&run(&one_face(vec![a])), LintId::MANA_ABILITY);
    }
    let state = json!({"mode": "STATE", "state": {"pred": "IS_CREATURE"}});
    let fail = [
        (triggered(json!({"targets": [slot()]})), 1),
        (
            triggered(
                json!({"trigger": {"mode": "EVENT", "event": {"kind": "ATTACKS",
                "subject": {"ref": "SELF"}}}}),
            ),
            1,
        ),
        (triggered(json!({"trigger": state.clone()})), 1),
        (triggered(json!({"effect": []})), 1),
        (
            triggered(json!({"targets": [slot()], "effect": [], "trigger": state})),
            3,
        ),
    ];
    for (a, n) in fail {
        decodes(&a);
        let report = run(&one_face(vec![a.clone()]));
        assert_eq!(report.with_id(LintId::MANA_ABILITY).count(), n, "{a}");
    }
}

/// REQ-ING-009 (CR 605.1): other kinds with `isManaAbility: true` are Errors; KEYWORD is
/// skipped; an undecodable ability is not checked (FPL-SCHEMA reports it).
#[test]
fn req_ing_009_mana_ability_other_kinds() {
    let kinds = [
        json!({"abilityId": "s", "kind": "SPELL", "isManaAbility": true, "effect": []}),
        json!({"abilityId": "s", "kind": "STATIC", "isManaAbility": true,
            "effects": [{"type": "RULE", "kind": "SKIP"}]}),
        json!({"abilityId": "s", "kind": "REPLACEMENT", "isManaAbility": true,
            "match": {"event": "DIES", "affected": {"ref": "SELF"}}, "replaceWith": [{"op": "SKIP"}]}),
    ];
    for a in kinds {
        decodes(&a);
        assert_eq!(
            run(&one_face(vec![a]))
                .with_id(LintId::MANA_ABILITY)
                .count(),
            1
        );
    }
    let kw = json!({"abilityId": "k", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
        "isManaAbility": true});
    decodes(&kw);
    none_of(&run(&one_face(vec![kw])), LintId::MANA_ABILITY);
    let broken = activated(json!({"targets": [slot()], "bogus": 1}));
    let report = run(&one_face(vec![broken]));
    none_of(&report, LintId::MANA_ABILITY);
    assert_eq!(report.with_id(LintId::SCHEMA).count(), 1);
}

// ---------------------------------------------------------------------------------------------
// FPL-TIER-HINT

/// A replacement with `tierHint`, `event` and `ops`.
fn replacement(hint: Option<&str>, event: &str, ops: Value) -> Value {
    let mut r = json!({
        "abilityId": "r1",
        "kind": "REPLACEMENT",
        "match": {"event": event, "affected": {"ref": "SELF"}},
        "replaceWith": ops,
        "textSpan": [0, 0],
    });
    if let Some(h) = hint {
        r["tierHint"] = json!(h);
    }
    r
}

/// REQ-ING-009, REQ-DSL-009 (CR 616.1b-d): tier hints on ENTERS replacements that modify the
/// entry pass; absent hints never fail; self-replacement is independent.
#[test]
fn req_ing_009_tier_hint_passes() {
    let modify = json!([{"op": "MODIFY_EVENT", "set": {"tapped": true}}]);
    for hint in ["CONTROL_ETB", "COPY_ETB", "BACK_FACE_ETB"] {
        let r = replacement(Some(hint), "ENTERS", modify.clone());
        decodes(&r);
        none_of(&run(&one_face(vec![r])), LintId::TIER_HINT);
    }
    // CR 616.1e: no hint, any event, any ops.
    let r = replacement(None, "DIES", json!([{"op": "SKIP"}]));
    decodes(&r);
    none_of(&run(&one_face(vec![r])), LintId::TIER_HINT);
    // CR 616.1a is a different tier: isSelfReplacement does not interact with the hint.
    let mut r = replacement(Some("COPY_ETB"), "ENTERS", modify);
    r["isSelfReplacement"] = json!(true);
    decodes(&r);
    none_of(&run(&one_face(vec![r])), LintId::TIER_HINT);
}

/// REQ-ING-009, REQ-DSL-009 (CR 616.1b-d): a hint on a non-ENTERS event, or with only SKIP and
/// PREVENT operations, is an Error per criterion.
#[test]
fn req_ing_009_tier_hint_fails() {
    let modify = json!([{"op": "MODIFY_EVENT", "set": {"tapped": true}}]);
    let cases = [
        (
            replacement(Some("CONTROL_ETB"), "ZONE_CHANGE", modify.clone()),
            1,
        ),
        (replacement(Some("COPY_ETB"), "DIES", modify), 1),
        (
            replacement(Some("BACK_FACE_ETB"), "ENTERS", json!([{"op": "SKIP"}])),
            1,
        ),
        (
            replacement(
                Some("COPY_ETB"),
                "ENTERS",
                json!([{"op": "PREVENT"}, {"op": "SKIP"}]),
            ),
            1,
        ),
        (
            replacement(Some("COPY_ETB"), "DIES", json!([{"op": "SKIP"}])),
            2,
        ),
    ];
    for (r, n) in cases {
        decodes(&r);
        let report = run(&one_face(vec![r.clone()]));
        let found = of(&report, LintId::TIER_HINT);
        assert_eq!(found.len(), n, "{r}");
        assert!(
            found
                .iter()
                .all(|f| f.0 == Severity::Error && f.3 == Some(0))
        );
    }
}

// ---------------------------------------------------------------------------------------------
// FPL-LAYER-KIND

/// REQ-ING-009, REQ-DSL-008: static effects declare a layer (CHAR) or kind (RULE); PLAYER needs
/// neither.
#[test]
fn req_ing_009_layer_kind_passes() {
    let s = json!({"abilityId": "s1", "kind": "STATIC", "textSpan": [0, 0], "effects": [
        {"type": "CHAR", "layer": "7c", "affected": {"ref": "SELF"},
            "modification": {"op": "MODIFY_PT", "power": 1, "toughness": 1}},
        {"type": "RULE", "kind": "SKIP"},
        {"type": "PLAYER", "affected": {"ref": "CONTROLLER"},
            "modification": {"op": "TEST_PLAYER_MOD"}},
    ]});
    decodes(&s);
    none_of(&run(&one_face(vec![s])), LintId::LAYER_KIND);
}

/// REQ-ING-009, REQ-DSL-008: raw static effects without a layer or kind, with a missing or
/// unknown type, non-object elements and a non-array `effects` are Errors; non-STATIC
/// abilities are not checked.
#[test]
fn req_ing_009_layer_kind_fails() {
    let s = json!({"abilityId": "s1", "kind": "STATIC", "effects": [
        {"type": "CHAR", "affected": {"ref": "SELF"}, "modification": {"op": "MODIFY_PT"}},
        {"type": "RULE"},
        {"affected": {"ref": "SELF"}},
        {"type": "OTHER"},
        {"type": 3},
        5,
        {"type": "PLAYER"},
    ]});
    let report = run(&one_face(vec![s]));
    let found = of(&report, LintId::LAYER_KIND);
    assert_eq!(found.len(), 6, "{report:#?}");
    assert!(
        found
            .iter()
            .all(|f| f.0 == Severity::Error && f.1 == "faces[0].abilities[0]")
    );
    // The typed decoder refuses it too.
    assert_eq!(report.with_id(LintId::SCHEMA).count(), 1);
    for effects in [json!({"type": "RULE"}), Value::Null] {
        let mut s = json!({"abilityId": "s1", "kind": "STATIC"});
        if !effects.is_null() {
            s["effects"] = effects;
        }
        assert_eq!(
            run(&one_face(vec![s])).with_id(LintId::LAYER_KIND).count(),
            1
        );
    }
    let not_static = json!({"abilityId": "x", "kind": "SPELL", "effects": [{"type": "CHAR"}]});
    none_of(&run(&one_face(vec![not_static])), LintId::LAYER_KIND);
}

// ---------------------------------------------------------------------------------------------
// FPL-TEXT-COVERAGE

/// REQ-ING-009 (SRS §14.3 textSpan): full coverage passes, overlaps are fine, and N = 0 has
/// nothing to cover.
#[test]
fn req_ing_009_text_coverage_passes() {
    let mut def = one_face(vec![
        keyword("a", 0, 2),
        keyword("b", 1, 4),
        keyword("c", 5, 5),
    ]);
    def.faces[0].text = tokens(6);
    none_of(&run(&def), LintId::TEXT_COVERAGE);
    let mut empty = one_face(vec![
        json!({"abilityId": "a", "kind": "KEYWORD", "keyword": "TEST_KEYWORD"}),
    ]);
    empty.faces[0].text = tokens(0);
    none_of(&run(&empty), LintId::TEXT_COVERAGE);
}

/// REQ-ING-009 (SRS §14.3): each maximal uncovered run is one finding with its span; Error for
/// VERIFIED and AUTO_PARSED, Warning for PARTIAL, nothing for UNSUPPORTED.
#[test]
fn req_ing_009_text_coverage_gaps_by_status() {
    let mut def = one_face(vec![keyword("a", 2, 3), keyword("b", 6, 6)]);
    def.faces[0].text = tokens(9);
    for (status, severity) in [
        (SupportStatus::Verified, Some(Severity::Error)),
        (SupportStatus::AutoParsed, Some(Severity::Error)),
        (SupportStatus::Partial, Some(Severity::Warning)),
        (SupportStatus::Unsupported, None),
    ] {
        def.support_status = status;
        let report = run(&def);
        let found: Vec<_> = report
            .with_id(LintId::TEXT_COVERAGE)
            .map(|l| (l.severity, l.location.clone()))
            .collect();
        let Some(severity) = severity else {
            assert!(found.is_empty(), "{found:?}");
            continue;
        };
        let loc = |s: u32, e: u32| LintLocation {
            face_index: Some(0),
            ability_index: None,
            path: "faces[0].text".to_owned(),
            text_span: Some((s, e)),
        };
        assert_eq!(
            found,
            vec![
                (severity, loc(0, 1)),
                (severity, loc(4, 5)),
                (severity, loc(7, 8))
            ]
        );
    }
}

/// REQ-ING-009 (SRS §14.3): a span past the last token or a malformed span is an Error at the
/// ability (except UNSUPPORTED); a well-formed overlong span still covers its in-range part.
#[test]
fn req_ing_009_text_coverage_bad_spans() {
    let kw = |id: &str, span: Value| json!({"abilityId": id, "kind": "KEYWORD", "keyword": "TEST_KEYWORD", "textSpan": span});
    let mut def = one_face(vec![
        keyword("a", 0, 5),
        kw("b", json!([3, 1])),
        kw("c", json!("x")),
        kw("d", json!([-1, 1])),
        kw("e", json!([0])),
        kw("f", json!([9, 99_999_999_999u64])),
    ]);
    def.faces[0].text = tokens(3);
    def.support_status = SupportStatus::Partial;
    let report = run(&def);
    let found = of(&report, LintId::TEXT_COVERAGE);
    let abilities: Vec<_> = found.iter().map(|f| (f.0, f.3)).collect();
    assert_eq!(
        abilities,
        (0..6)
            .map(|j| (Severity::Error, Some(j)))
            .collect::<Vec<_>>()
    );
    // The overlong span keeps its own span in the location; an over-u32 span has none.
    let spans: Vec<_> = report
        .with_id(LintId::TEXT_COVERAGE)
        .map(|l| l.location.text_span)
        .collect();
    assert_eq!(spans[0], Some((0, 5)));
    assert_eq!(spans[5], None);
    def.support_status = SupportStatus::Unsupported;
    none_of(&run(&def), LintId::TEXT_COVERAGE);
}

// ---------------------------------------------------------------------------------------------
// FPL-LOOK-BACK

/// REQ-ING-009 (CR 603.10): look-back keys at any depth, in any spelling, are Errors naming the
/// key and path; ordinary keys are not.
#[test]
fn req_ing_009_look_back() {
    none_of(&run(&clean()), LintId::LOOK_BACK);
    let t = json!({"abilityId": "t1", "kind": "TRIGGERED", "lookBack": true, "textSpan": [0, 0],
        "trigger": {"mode": "EVENT", "event": {"kind": "DIES", "subject": {"ref": "SELF"},
            "filters": {"look_back": true, "Look-Back-Mode": 1, "lookingBack": 1}}},
        "effect": [{"op": "NATIVE_EFFECT", "id": "test.native", "version": 1,
            "params": {"x": [{"LOOKBACK": 1}]}}]});
    let report = run(&one_face(vec![t]));
    let messages: Vec<&str> = report
        .with_id(LintId::LOOK_BACK)
        .map(|l| l.message.as_str())
        .collect();
    assert_eq!(messages.len(), 4, "{messages:#?}");
    assert!(
        messages
            .iter()
            .any(|m| m.contains("\"lookBack\" at $.lookBack"))
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("$.trigger.event.filters.look_back"))
    );
    assert!(messages.iter().any(|m| m.contains("\"Look-Back-Mode\"")));
    assert!(
        messages
            .iter()
            .any(|m| m.contains("$.effect[0].params.x[0].LOOKBACK"))
    );
    assert!(
        report
            .with_id(LintId::LOOK_BACK)
            .all(|l| l.severity == Severity::Error && l.location.ability_index == Some(0))
    );
    assert!(walk::is_lookback_key("LookBack"));
    assert!(!walk::is_lookback_key("look"));
    assert!(!walk::is_lookback_key("lookingBack"));
}

/// REQ-ING-009 (CR 603.10): long paths are rendered from their last segments only.
#[test]
fn req_ing_009_look_back_deep_path_is_bounded() {
    let mut v = json!({"lookBack": 1});
    for _ in 0..50 {
        v = json!({"k": v});
    }
    let report = run(&one_face(vec![v]));
    let lint = report.with_id(LintId::LOOK_BACK).next().expect("found");
    assert!(lint.message.contains("$...."), "{}", lint.message);
    assert!(lint.message.len() < 300);
}

// ---------------------------------------------------------------------------------------------
// FPL-MAX-ITER

/// A SPELL whose effect holds a REPEAT_WHILE (inside an IF) with `max_iter` (`null`: absent).
fn repeat_while(max_iter: Value) -> Value {
    let mut r = json!({"op": "REPEAT_WHILE", "cond": {"pred": "IS_CREATURE"},
        "body": [{"op": "POPULATE"}]});
    if !max_iter.is_null() {
        r["maxIter"] = max_iter;
    }
    json!({"abilityId": "s", "kind": "SPELL", "textSpan": [0, 0],
        "effect": [{"op": "IF", "cond": {"pred": "IS_CREATURE"}, "then": [r]}]})
}

/// REQ-ING-009, REQ-DSL-010 (SRS §14.4.5): every REPEAT_WHILE declares an integer maxIter >= 1.
#[test]
fn req_ing_009_max_iter() {
    let ok = repeat_while(json!(3));
    decodes(&ok);
    none_of(&run(&one_face(vec![ok])), LintId::MAX_ITER);
    for bad in [Value::Null, json!(0), json!(-2), json!("3"), json!(1.5)] {
        let report = run(&one_face(vec![repeat_while(bad.clone())]));
        assert_eq!(
            of(&report, LintId::MAX_ITER),
            vec![(
                Severity::Error,
                "faces[0].abilities[0]".to_owned(),
                Some(0),
                Some(0)
            )],
            "{bad}"
        );
        let msg = &report
            .with_id(LintId::MAX_ITER)
            .next()
            .expect("one")
            .message;
        assert!(msg.contains("$.effect[0].then[0]"), "{msg}");
    }
    // A huge maxIter is not this check's concern (FPL-SCHEMA bounds it).
    none_of(
        &run(&one_face(vec![repeat_while(json!(u64::MAX))])),
        LintId::MAX_ITER,
    );
}

// ---------------------------------------------------------------------------------------------
// FPL-TOKEN-REF

/// A SPELL creating token `def` and an inline token.
fn create(def: &str) -> Value {
    json!({"abilityId": "s", "kind": "SPELL", "textSpan": [0, 0], "effect": [
        {"op": "CREATE_TOKEN", "tokenDef": def, "count": {"LITERAL": 1}},
        {"op": "CREATE_TOKEN", "inline": {}, "count": {"LITERAL": 1}},
    ]})
}

/// REQ-ING-009 (referenced token definitions exist): with known ids, unknown `tokenDef`s and
/// `relatedTokens` are Errors; inline tokens need no lookup; no known set skips the check.
#[test]
fn req_ing_009_token_ref() {
    let mut def = one_face(vec![create("test.token_known")]);
    def.related_tokens = Some(vec![
        "test.token_known".to_owned(),
        "test.token_other".to_owned(),
    ]);
    let known = LintContext {
        source: None,
        known_token_ids: Some(BTreeSet::from([
            "test.token_known".to_owned(),
            "test.token_other".to_owned(),
        ])),
    };
    decodes(&create("x"));
    none_of(&lint_definition(&def, &known), LintId::TOKEN_REF);
    def.faces[0].abilities[0] = AbilityAST(create("test.token_missing"));
    def.related_tokens = Some(vec![
        "test.token_known".to_owned(),
        "test.token_gone".to_owned(),
    ]);
    def.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: true,
        partner_abilities: vec![PartnerAbilityDSL(
            json!({"abilityId": "p", "kind": "KEYWORD", "keyword": "PARTNER",
                "params": {"x": {"op": "CREATE_TOKEN", "tokenDef": "test.token_p"}}}),
        )],
    });
    let report = lint_definition(&def, &known);
    assert_eq!(
        of(&report, LintId::TOKEN_REF),
        vec![
            (
                Severity::Error,
                "commanderEligibility.partnerAbilities[0]".to_owned(),
                None,
                None
            ),
            (Severity::Error, "relatedTokens[1]".to_owned(), None, None),
            (
                Severity::Error,
                "faces[0].abilities[0]".to_owned(),
                Some(0),
                Some(0)
            ),
        ]
    );
    // Skipped without a known set.
    none_of(&run(&def), LintId::TOKEN_REF);
}

// ---------------------------------------------------------------------------------------------
// FPL-LINK-ID

/// A keyword ability `id` declaring `links`.
fn declares(id: &str, links: &[&str]) -> Value {
    json!({"abilityId": id, "kind": "KEYWORD", "keyword": "TEST_KEYWORD", "linkIds": links})
}

/// A spell ability `id` referring to `link` through a LINKED reference.
fn refers(id: &str, link: &str) -> Value {
    json!({"abilityId": id, "kind": "SPELL", "effect": [
        {"op": "DESTROY", "objects": {"ref": "LINKED", "linkId": link}}
    ]})
}

/// REQ-ING-009 (CR 607.1, 607.1b, 607.1c, 607.4): paired links across faces, self-links,
/// many-declarer ids and many-id abilities pass.
#[test]
fn req_ing_009_link_id_passes() {
    let mut def = def_of(
        vec![
            face(None, 0, vec![declares("a", &["l1", "l2"])]),
            face_of(
                FaceKind::Back,
                None,
                0,
                vec![refers("b", "l1"), refers("c", "l2")],
            ),
        ],
        ColorBitmask::NONE,
    );
    def.layout = LayoutBehavior::DfcNonmodal;
    none_of(&run(&def), LintId::LINK_ID);
    // CR 607.1c: linked to itself.
    let mut own = refers("s", "self");
    own["linkIds"] = json!(["self"]);
    decodes(&own);
    let mut def = one_face(vec![own]);
    def.faces[0].text = tokens(0);
    none_of(&run(&def), LintId::LINK_ID);
    // CR 607.4: an id declared by several abilities and referenced by none is fine.
    let mut def = one_face(vec![declares("a", &["l"]), declares("b", &["l"])]);
    def.faces[0].text = tokens(0);
    none_of(&run(&def), LintId::LINK_ID);
}

/// REQ-ING-009 (CR 607.1): a reference to an undeclared id is an Error at the referencing
/// ability; an id declared once and never referenced is a Warning at the declarer.
#[test]
fn req_ing_009_link_id_fails() {
    let mut def = one_face(vec![
        refers("r", "ghost"),
        declares("d", &["lonely", "lonely"]),
    ]);
    def.faces[0].text = tokens(0);
    let report = run(&def);
    assert_eq!(
        of(&report, LintId::LINK_ID),
        vec![
            (
                Severity::Error,
                "faces[0].abilities[0]".to_owned(),
                Some(0),
                Some(0)
            ),
            (
                Severity::Warning,
                "faces[0].abilities[1]".to_owned(),
                Some(0),
                Some(1)
            ),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// FPL-MANA-VALUE

/// REQ-ING-009, REQ-ING-002, REQ-OBJ-001 (CR 202.3, 202.3e-g): every symbol's contribution.
#[test]
fn req_ing_009_mana_value_symbols() {
    // {2}=2, {W}=1, {C}=1, {S}=1, {X}=0, {W/U}=1, {2/W}=2, {C/W}=1, {W/P}=1, {W/U/P}=1 -> 11
    let cost = "{X}{2}{W}{W/U}{2/W}{C/W}{W/P}{W/U/P}{C}{S}";
    let def = def_of(vec![face(Some(cost), 0, vec![])], mask(3));
    none_of(
        &lint_definition(&def, &source(vec![(Some(11), None)], None)),
        LintId::MANA_VALUE,
    );
    let report = lint_definition(&def, &source(vec![(Some(10), None)], None));
    assert_eq!(
        of(&report, LintId::MANA_VALUE),
        vec![(Severity::Error, "faces[0]".to_owned(), Some(0), None)]
    );
    let msg = &report
        .with_id(LintId::MANA_VALUE)
        .next()
        .expect("one")
        .message;
    assert!(msg.contains("10") && msg.contains("11"), "{msg}");
    // CR 202.3a: no mana cost is 0.
    let none = def_of(vec![face(None, 0, vec![])], ColorBitmask::NONE);
    none_of(
        &lint_definition(&none, &source(vec![(Some(0), None)], None)),
        LintId::MANA_VALUE,
    );
    assert_eq!(
        lint_definition(&none, &source(vec![(Some(1), None)], None))
            .with_id(LintId::MANA_VALUE)
            .count(),
        1
    );
}

/// REQ-ING-009, REQ-ING-002 (CR 202.3b, 202.3c, 202.3d): DFC back faces, split halves, meld
/// results; extra or missing source entries and absent values are ignored.
#[test]
fn req_ing_009_mana_value_layouts() {
    let mv = |def: &CardDefinitionDSL, faces: Vec<Option<u32>>| {
        let ctx = source(faces.into_iter().map(|m| (m, None)).collect(), None);
        of(&lint_definition(def, &ctx), LintId::MANA_VALUE)
    };
    let mut dfc = def_of(
        vec![
            face(Some("{3}{G}"), 0, vec![]),
            face_of(FaceKind::Back, None, 0, vec![]),
        ],
        ColorBitmask::GREEN,
    );
    dfc.layout = LayoutBehavior::DfcNonmodal;
    // CR 202.3b: the back face uses the front face's cost.
    assert!(mv(&dfc, vec![Some(4), Some(4)]).is_empty());
    assert_eq!(
        mv(&dfc, vec![Some(4), Some(0)]),
        vec![(Severity::Error, "faces[1]".to_owned(), Some(1), None)]
    );
    // A modal DFC back face uses its own (absent) cost.
    dfc.layout = LayoutBehavior::DfcModal;
    assert!(mv(&dfc, vec![Some(4), Some(0)]).is_empty());
    // CR 202.3d: each split half has the combined cost off the stack.
    let mut split = def_of(
        vec![
            face_of(FaceKind::LeftHalf, Some("{1}{R}"), 0, vec![]),
            face_of(FaceKind::RightHalf, Some("{2}{U}"), 0, vec![]),
        ],
        mask(10),
    );
    split.layout = LayoutBehavior::Split;
    assert!(mv(&split, vec![Some(5), Some(5)]).is_empty());
    assert_eq!(mv(&split, vec![Some(2), Some(3)]).len(), 2);
    // CR 202.3c: a meld result is skipped.
    let meld = def_of(
        vec![face_of(FaceKind::MeldResult, None, 0, vec![])],
        ColorBitmask::NONE,
    );
    assert!(mv(&meld, vec![Some(9)]).is_empty());
    // Extra source entries, no source value, no source at all: skipped.
    let one = def_of(vec![face(Some("{1}"), 0, vec![])], ColorBitmask::NONE);
    assert!(mv(&one, vec![Some(1), Some(7)]).is_empty());
    assert!(mv(&one, vec![None]).is_empty());
    assert!(mv(&one, vec![]).is_empty());
    none_of(&run(&one), LintId::MANA_VALUE);
}

/// REQ-ING-009, REQ-OBJ-001: generic amounts at the limit sum without overflow (u64).
#[test]
fn req_ing_009_mana_value_large() {
    let cost = "{1000000}{1000000}{1000000/W}";
    let def = def_of(vec![face(Some(cost), 0, vec![])], ColorBitmask::WHITE);
    none_of(
        &lint_definition(&def, &source(vec![(Some(3_000_000), None)], None)),
        LintId::MANA_VALUE,
    );
}

// ---------------------------------------------------------------------------------------------
// FPL-COLORS

/// REQ-ING-009, REQ-ING-002 (CR 202.2, 105.2, 204.2): colors from cost symbols and the color
/// indicator; a computed color missing from the source is an Error, an extra source color a
/// Warning.
#[test]
fn req_ing_009_colors() {
    let colors = |def: &CardDefinitionDSL, src: Option<ColorBitmask>| {
        of(
            &lint_definition(def, &source(vec![(None, src)], None)),
            LintId::COLORS,
        )
    };
    let mut f = face(Some("{W/U}{B/P}{2/R}{C/G}{1}{C}{S}{X}"), 0, vec![]);
    let def = def_of(vec![f.clone()], mask(31));
    assert!(colors(&def, Some(mask(31))).is_empty());
    // Missing red: Error.
    assert_eq!(
        colors(&def, Some(mask(31 - 8))),
        vec![(Severity::Error, "faces[0]".to_owned(), Some(0), None)]
    );
    let report = lint_definition(&def, &source(vec![(None, Some(mask(31 - 8)))], None));
    let msg = &report.with_id(LintId::COLORS).next().expect("one").message;
    assert!(msg.contains("colors R "), "{msg}");
    // Color indicator (CR 204.2) on a costless face; an extra source color is a Warning.
    f.mana_cost = None;
    f.color_indicator = Some(ColorBitmask::GREEN);
    let def = def_of(vec![f], ColorBitmask::GREEN);
    assert!(colors(&def, Some(ColorBitmask::GREEN)).is_empty());
    assert_eq!(
        colors(&def, Some(mask(16 | 1))),
        vec![(Severity::Warning, "faces[0]".to_owned(), Some(0), None)]
    );
    // Both directions at once.
    let sev: Vec<_> = colors(&def, Some(ColorBitmask::WHITE))
        .into_iter()
        .map(|f| f.0)
        .collect();
    assert_eq!(sev, vec![Severity::Error, Severity::Warning]);
    // No source colors, or no source: skipped.
    assert!(colors(&def, None).is_empty());
    none_of(&run(&def), LintId::COLORS);
}

// ---------------------------------------------------------------------------------------------
// FPL-COLOR-IDENTITY

/// REQ-ING-009, REQ-ING-002 (CR 903.4, 903.4d): identity unions every face's cost and indicator
/// and every typed PAY_MANA / WATERBEND cost node of decodable abilities.
#[test]
fn req_ing_009_color_identity_passes() {
    let act = json!({"abilityId": "a", "kind": "ACTIVATED",
        "cost": [{"cost": "COMPOSITE", "costs": [
            {"cost": "PAY_MANA", "mana": "{B}"}, {"cost": "TAP_SOURCE"}
        ]}],
        "effect": [{"op": "WATERBEND", "cost": [{"cost": "WATERBEND", "mana": "{G/U}"}]}]});
    decodes(&act);
    let mut back = face_of(FaceKind::Back, None, 0, vec![]);
    back.color_indicator = Some(ColorBitmask::RED);
    let mut def = def_of(vec![face(Some("{W}"), 0, vec![act]), back], mask(31));
    def.layout = LayoutBehavior::DfcNonmodal;
    none_of(&run(&def), LintId::COLOR_IDENTITY);
    none_of(
        &lint_definition(&def, &source(vec![], Some(mask(31)))),
        LintId::COLOR_IDENTITY,
    );
}

/// REQ-ING-009, REQ-ING-002 (CR 903.4): a computed color missing from the declared or source
/// identity is an Error; a declared color not derivable is a Warning; literal text and
/// undecodable abilities contribute nothing.
#[test]
fn req_ing_009_color_identity_fails() {
    // Declared white, computed white + black (from an activated cost): Error.
    let act = json!({"abilityId": "a", "kind": "ACTIVATED",
        "cost": [{"cost": "PAY_MANA", "mana": "{B}"}], "effect": []});
    decodes(&act);
    let mut def = def_of(vec![face(Some("{W}"), 0, vec![act])], ColorBitmask::WHITE);
    assert_eq!(
        of(&run(&def), LintId::COLOR_IDENTITY),
        vec![(Severity::Error, "$".to_owned(), None, None)]
    );
    // Declared white + black + green: green is a Warning.
    def.color_identity = mask(1 | 4 | 16);
    assert_eq!(
        of(&run(&def), LintId::COLOR_IDENTITY),
        vec![(Severity::Warning, "$".to_owned(), None, None)]
    );
    // Against the source too: declared fine, source lacks black.
    def.color_identity = mask(1 | 4);
    let report = lint_definition(&def, &source(vec![], Some(ColorBitmask::WHITE)));
    let found: Vec<_> = report.with_id(LintId::COLOR_IDENTITY).collect();
    assert_eq!(found.len(), 1);
    assert!(found[0].message.contains("source"), "{}", found[0].message);
    // LITERAL text with symbols and an undecodable ability contribute nothing.
    let mut f = face(
        Some("{W}"),
        0,
        vec![json!({"abilityId": "x", "kind": "ACTIVATED",
            "cost": [{"cost": "PAY_MANA", "mana": "{G}"}], "effect": [], "bogus": 1})],
    );
    f.text = TextTokenStream(vec![json!({"id": "t0", "kind": "LITERAL", "text": "{U}"})]);
    let def = def_of(vec![f], ColorBitmask::WHITE);
    none_of(&run(&def), LintId::COLOR_IDENTITY);
}

// ---------------------------------------------------------------------------------------------
// Ordering, de-duplication, determinism

/// REQ-ING-009 (deterministic and ordered output): the report follows the total key
/// (face, ability, id, severity, span, path, message), None first.
#[test]
fn req_ing_009_report_is_sorted_by_total_key() {
    let mut def = def_of(
        vec![
            face(
                Some("{W}"),
                4,
                vec![
                    refers("r", "ghost"),
                    json!({"abilityId": "z", "lookBack": 1}),
                ],
            ),
            face(Some("{U}"), 2, vec![declares("d", &["lonely"])]),
        ],
        ColorBitmask::NONE,
    );
    def.related_tokens = Some(vec!["test.token_gone".to_owned()]);
    let mut ctx = source(vec![(Some(9), Some(ColorBitmask::GREEN))], None);
    ctx.known_token_ids = Some(BTreeSet::new());
    let report = lint_definition(&def, &ctx);
    assert!(report.lints.len() > 8, "{report:#?}");
    let keys: Vec<_> = report.lints.iter().map(Lint::key).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    sorted.dedup();
    assert_eq!(keys.len(), sorted.len(), "duplicates");
    // Card-level findings come first, the last face's last.
    assert_eq!(report.lints[0].location.face_index, None);
    assert_eq!(
        report.lints.last().expect("last").location.face_index,
        Some(1)
    );
}

/// REQ-ING-009: equal findings appear once.
#[test]
fn req_ing_009_duplicates_removed() {
    // Two unknown references to the same token in one ability give equal findings.
    let s = json!({"abilityId": "s", "kind": "SPELL", "effect": [
        {"op": "CREATE_TOKEN", "tokenDef": "test.t", "count": {"LITERAL": 1}},
        {"op": "CREATE_TOKEN", "tokenDef": "test.t", "count": {"LITERAL": 1}},
    ]});
    let mut def = one_face(vec![s]);
    def.faces[0].text = tokens(0);
    let ctx = LintContext {
        source: None,
        known_token_ids: Some(BTreeSet::new()),
    };
    assert_eq!(
        lint_definition(&def, &ctx)
            .with_id(LintId::TOKEN_REF)
            .count(),
        1
    );
}

/// Per original ability index (`order[j]` is the original index of position `j`): its
/// findings as (id, severity, message).
type PerAbility = BTreeMap<usize, Vec<(LintId, Severity, String)>>;

/// Groups the ability-level findings of `report` by original ability index.
fn per_ability(report: &LintReport, order: &[usize]) -> PerAbility {
    let mut m: PerAbility = BTreeMap::new();
    for l in &report.lints {
        if let Some(j) = l.location.ability_index {
            m.entry(order[j])
                .or_default()
                .push((l.id, l.severity, l.message.clone()));
        }
    }
    m
}

/// REQ-ING-009, REQ-CORE-001 (R3): two runs give equal reports; reordering abilities gives the
/// same findings per ability, with indices following the new positions.
#[test]
fn req_ing_009_determinism_and_reordering() {
    let abilities = vec![
        activated(json!({"targets": [slot()]})),
        refers("r", "ghost"),
        json!({"abilityId": "z", "lookBack": 1, "textSpan": [9, 9]}),
        declares("d", &["lonely"]),
    ];
    let def = def_of(
        vec![face(Some("{R}"), 3, abilities.clone())],
        ColorBitmask::NONE,
    );
    let ctx = source(
        vec![(Some(5), Some(ColorBitmask::BLUE))],
        Some(ColorBitmask::GREEN),
    );
    let first = lint_definition(&def, &ctx);
    assert_eq!(first, lint_definition(&def, &ctx));
    let order = [3usize, 1, 0, 2];
    let mut reordered = def.clone();
    reordered.faces[0].abilities = order
        .iter()
        .map(|&k| AbilityAST(abilities[k].clone()))
        .collect();
    let second = lint_definition(&reordered, &ctx);
    assert_eq!(
        per_ability(&first, &[0, 1, 2, 3]),
        per_ability(&second, &order)
    );
    assert_eq!(per_ability(&first, &[0, 1, 2, 3]).len(), 4);
    let card_level = |r: &LintReport| -> Vec<Lint> {
        r.lints
            .iter()
            .filter(|l| l.location.ability_index.is_none())
            .cloned()
            .collect()
    };
    assert_eq!(card_level(&first), card_level(&second));
}

// ---------------------------------------------------------------------------------------------
// Robustness (R12)

/// `depth` alternately nested objects and arrays around `leaf`, built iteratively.
fn nested(depth: usize, leaf: Value) -> Value {
    let mut v = leaf;
    for k in 0..depth {
        let mut m = Map::new();
        let key = if k % 3 == 0 { "op" } else { "k" };
        m.insert(key.to_owned(), v);
        v = if k % 2 == 0 {
            Value::Object(m)
        } else {
            Value::Array(vec![Value::Object(m)])
        };
    }
    v
}

/// Drops a deep value without recursion.
fn drop_deep(v: Value) {
    let mut stack = vec![v];
    while let Some(v) = stack.pop() {
        match v {
            Value::Array(items) => stack.extend(items),
            Value::Object(map) => stack.extend(map.into_iter().map(|(_, v)| v)),
            _ => {}
        }
    }
}

/// Drops every raw wrapper of `def` without recursion.
fn drop_def(mut def: CardDefinitionDSL) {
    for face in std::mem::take(&mut def.faces) {
        face.abilities.into_iter().for_each(|a| drop_deep(a.0));
        face.power.into_iter().for_each(|p| drop_deep(p.0));
        face.toughness.into_iter().for_each(|p| drop_deep(p.0));
        face.text.0.into_iter().for_each(drop_deep);
    }
    if let Some(e) = def.commander_eligibility.take() {
        e.partner_abilities.into_iter().for_each(|p| drop_deep(p.0));
    }
}

/// REQ-ING-009, R12: deep nesting far beyond MAX_DEPTH, wrong types, null, huge integers and
/// an empty face list never panic.
#[test]
fn req_ing_009_robust_on_hostile_values() {
    let depth = crate::canonical::MAX_DEPTH * 40;
    let deep = nested(depth, json!({"lookBack": 1, "op": "REPEAT_WHILE"}));
    let mut def = one_face(vec![
        deep,
        json!(null),
        json!(18_446_744_073_709_551_615u64),
        json!("x"),
        json!([1, [2]]),
    ]);
    def.faces[0].power = Some(ValueExpressionAST(nested(depth, json!(1))));
    def.faces[0].text = TextTokenStream(vec![nested(depth, Value::Null), json!(-1)]);
    def.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: false,
        partner_abilities: vec![PartnerAbilityDSL(nested(depth, json!({"look_back": 0})))],
    });
    let report = run(&def);
    assert!(report.with_id(LintId::SCHEMA).count() >= 7, "{report:#?}");
    assert_eq!(report.with_id(LintId::LOOK_BACK).count(), 2);
    assert_eq!(report.with_id(LintId::MAX_ITER).count(), 1);
    for l in &report.lints {
        assert!(l.message.len() < 400, "{}", l.message);
    }
    drop_def(def);
    let mut empty = one_face(vec![]);
    empty.faces.clear();
    empty.color_identity = ColorBitmask::NONE;
    let ctx = source(
        vec![(Some(1), Some(ColorBitmask::RED))],
        Some(ColorBitmask::RED),
    );
    let report = lint_definition(&empty, &ctx);
    assert_eq!(
        of(&report, LintId::COLOR_IDENTITY),
        vec![(Severity::Warning, "$".to_owned(), None, None)]
    );
    assert_eq!(report.lints.len(), 1);
}

/// Member names biased towards the DSL.
const KEYS: [&str; 16] = [
    "op",
    "kind",
    "abilityId",
    "effect",
    "effects",
    "type",
    "layer",
    "maxIter",
    "tokenDef",
    "ref",
    "linkId",
    "linkIds",
    "textSpan",
    "lookBack",
    "cost",
    "mana",
];

/// String values biased towards the DSL.
const STRS: [&str; 14] = [
    "REPEAT_WHILE",
    "CREATE_TOKEN",
    "LINKED",
    "STATIC",
    "CHAR",
    "RULE",
    "PLAYER",
    "PAY_MANA",
    "{W}{U}",
    "{2/G}",
    "ACTIVATED",
    "KEYWORD",
    "l1",
    "",
];

/// A uniform index below `n` from `rng`.
fn pick(rng: &mut Pcg64Dxsm, n: usize) -> usize {
    let bound = u64::try_from(n).unwrap_or(u64::MAX);
    usize::try_from(rng.next_bounded(bound).unwrap_or(0)).unwrap_or(0)
}

/// A pseudo-random JSON value from `rng` with at most about `budget` nodes.
fn random_value(rng: &mut Pcg64Dxsm, budget: &mut u32, depth: u32) -> Value {
    *budget = budget.saturating_sub(1);
    let choice = if *budget == 0 || depth > 12 {
        pick(rng, 5)
    } else {
        pick(rng, 8)
    };
    match choice {
        0 => Value::Null,
        1 => Value::Bool(rng.next_u64() & 1 == 1),
        2 => match pick(rng, 4) {
            0 => json!(rng.next_u64()),
            1 => json!(-i64::try_from(rng.next_u64() >> 2).unwrap_or(0)),
            2 => serde_json::from_str("0.5").expect("number"),
            _ => json!(pick(rng, 4)),
        },
        3 | 4 => json!(STRS[pick(rng, STRS.len())]),
        5 => {
            let n = pick(rng, 4);
            Value::Array(
                (0..n)
                    .map(|_| random_value(rng, budget, depth + 1))
                    .collect(),
            )
        }
        _ => {
            let n = pick(rng, 5);
            let mut m = Map::new();
            for _ in 0..n {
                let k = KEYS[pick(rng, KEYS.len())].to_owned();
                m.insert(k, random_value(rng, budget, depth + 1));
            }
            Value::Object(m)
        }
    }
}

/// REQ-ING-009, R12 (bounded pseudo-random run): random raw abilities, text tokens and value
/// expressions never panic, and the report is sorted, de-duplicated and stable.
#[test]
fn req_ing_009_random_values_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0016_1157_0000_0001);
    for _ in 0..400 {
        let mut budget = 60;
        let abilities: Vec<Value> = (0..3)
            .map(|_| random_value(&mut rng, &mut budget, 0))
            .collect();
        let mut def = one_face(abilities);
        let mut budget = 20;
        def.faces[0].text = TextTokenStream(
            (0..2)
                .map(|_| random_value(&mut rng, &mut budget, 0))
                .collect(),
        );
        def.faces[0].toughness = Some(ValueExpressionAST(random_value(&mut rng, &mut budget, 0)));
        def.related_tokens = Some(vec!["test.t".to_owned()]);
        let mut ctx = source(
            vec![(Some(1), Some(ColorBitmask::BLUE))],
            Some(ColorBitmask::GREEN),
        );
        ctx.known_token_ids = Some(BTreeSet::from(["l1".to_owned()]));
        let report = lint_definition(&def, &ctx);
        let keys: Vec<_> = report.lints.iter().map(Lint::key).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(keys, sorted);
        assert_eq!(report, lint_definition(&def, &ctx));
    }
}

/// REQ-ING-009: `clip` cuts on a character boundary.
#[test]
fn req_ing_009_clip_on_char_boundary() {
    assert_eq!(clip("abc", 5), "abc");
    assert_eq!(clip("ééé", 2), "éé...");
}
