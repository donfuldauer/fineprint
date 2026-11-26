// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::lint` (FP-015, REQ-ING-009): the pinned API, the S-0 ordering
//! and determinism contract, and the structural lints S-1..S-9 (FPL-SCHEMA, FPL-MANA-ABILITY,
//! FPL-TIER-HINT, FPL-LAYER-KIND, FPL-TEXT-COVERAGE, FPL-LOOK-BACK, FPL-MAX-ITER, FPL-TOKEN-REF,
//! FPL-LINK-ID), plus the table-driven "one pass and one fail per lint" acceptance test over
//! `LintId::ALL`. Black-box: written from the work-item brief only. Synthetic data only (R5).

#[path = "spec_dsl_lint_common/report.rs"]
mod report;
mod spec_dsl_lint_common;

use std::collections::BTreeSet;

use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, ColorBitmask, CommanderEligibilityDSL, FaceKind, LayoutBehavior,
    PartnerAbilityDSL, SupportStatus, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::lint::{
    Lint, LintContext, LintId, LintLocation, LintReport, Severity, SourceCharacteristics,
    SourceFaceCharacteristics,
};
use fineprint_dsl::mana::{Color, ManaSymbol};
use report::*;
use serde_json::{Value, json};
use spec_dsl_lint_common::*;

const E: Severity = Severity::Error;
const W: Severity = Severity::Warning;

fn a00() -> String {
    ab(0, 0)
}

// ================================================================ pinned API (S-0)

/// REQ-ING-009: `LintId::ALL` lists the twelve ids in the pinned order with the pinned strings,
/// unique, and `Display` prints the string.
#[test]
fn req_ing_009_lint_id_all_strings_order_unique() {
    let want = [
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
    ];
    let got: Vec<&str> = LintId::ALL.iter().map(|i| i.as_str()).collect();
    assert_eq!(got, want);
    let consts = [
        LintId::SCHEMA,
        LintId::MANA_ABILITY,
        LintId::TIER_HINT,
        LintId::LAYER_KIND,
        LintId::TEXT_COVERAGE,
        LintId::LOOK_BACK,
        LintId::MAX_ITER,
        LintId::TOKEN_REF,
        LintId::LINK_ID,
        LintId::MANA_VALUE,
        LintId::COLORS,
        LintId::COLOR_IDENTITY,
    ];
    assert_eq!(consts, LintId::ALL);
    let unique: BTreeSet<&str> = got.iter().copied().collect();
    assert_eq!(unique.len(), 12);
    let unique_ids: BTreeSet<LintId> = LintId::ALL.into_iter().collect();
    assert_eq!(unique_ids.len(), 12);
    for id in LintId::ALL {
        assert_eq!(id.to_string(), id.as_str());
    }
}

/// REQ-ING-009: Severity order (Error < Warning) and strings.
#[test]
fn req_ing_009_severity_order_and_strings() {
    assert!(Severity::Error < Severity::Warning);
    assert_eq!(Severity::Error.as_str(), "ERROR");
    assert_eq!(Severity::Warning.as_str(), "WARNING");
    let mut v = vec![W, E, W];
    v.sort();
    assert_eq!(v, vec![E, W, W]);
}

/// REQ-ING-009: `Lint` Display with and without a span.
#[test]
fn req_ing_009_lint_display_format() {
    let with_span = Lint {
        id: LintId::TEXT_COVERAGE,
        severity: W,
        location: LintLocation {
            face_index: Some(0),
            ability_index: None,
            path: "faces[0].text".to_string(),
            text_span: Some((2, 4)),
        },
        message: "m1".to_string(),
    };
    assert_eq!(
        with_span.to_string(),
        "WARNING FPL-TEXT-COVERAGE faces[0].text [2,4]: m1"
    );
    let no_span = Lint {
        id: LintId::SCHEMA,
        severity: E,
        location: LintLocation {
            face_index: Some(1),
            ability_index: Some(3),
            path: "faces[1].abilities[3]".to_string(),
            text_span: None,
        },
        message: "m2".to_string(),
    };
    assert_eq!(
        no_span.to_string(),
        "ERROR FPL-SCHEMA faces[1].abilities[3]: m2"
    );
    let root = Lint {
        id: LintId::COLOR_IDENTITY,
        severity: E,
        location: LintLocation {
            face_index: None,
            ability_index: None,
            path: "$".to_string(),
            text_span: None,
        },
        message: "m3".to_string(),
    };
    assert_eq!(root.to_string(), "ERROR FPL-COLOR-IDENTITY $: m3");
}

/// REQ-ING-009: LintReport accessors.
#[test]
fn req_ing_009_lint_report_accessors() {
    let empty = LintReport::default();
    assert!(empty.is_empty());
    assert!(!empty.has_errors());
    assert_eq!(empty.errors().count(), 0);
    assert_eq!(empty.warnings().count(), 0);
    let loc = |p: &str| LintLocation {
        face_index: None,
        ability_index: None,
        path: p.to_string(),
        text_span: None,
    };
    let w = Lint {
        id: LintId::LINK_ID,
        severity: W,
        location: loc("$"),
        message: "w".to_string(),
    };
    let warn_only = LintReport {
        lints: vec![w.clone()],
    };
    assert!(!warn_only.is_empty());
    assert!(!warn_only.has_errors());
    assert_eq!(warn_only.warnings().count(), 1);
    let e = Lint {
        id: LintId::SCHEMA,
        severity: E,
        location: loc("$"),
        message: "e".to_string(),
    };
    let both = LintReport {
        lints: vec![e.clone(), w.clone()],
    };
    assert!(both.has_errors());
    assert_eq!(both.errors().cloned().collect::<Vec<_>>(), vec![e.clone()]);
    assert_eq!(
        both.warnings().cloned().collect::<Vec<_>>(),
        vec![w.clone()]
    );
    assert_eq!(
        both.with_id(LintId::LINK_ID).cloned().collect::<Vec<_>>(),
        vec![w]
    );
    assert_eq!(both.with_id(LintId::MAX_ITER).count(), 0);
}

/// REQ-ING-009: the default context is "no source, no token catalogue".
#[test]
fn req_ing_009_default_context() {
    let c = LintContext::default();
    assert_eq!(c.source, None);
    assert_eq!(c.known_token_ids, None);
    assert_eq!(SourceCharacteristics::default().faces, vec![]);
    assert_eq!(SourceCharacteristics::default().color_identity, None);
    assert_eq!(
        SourceFaceCharacteristics::default(),
        SourceFaceCharacteristics {
            mana_value: None,
            colors: None
        }
    );
}

// ================================================================ S-0 ordering, determinism

/// A definition with findings on several faces, abilities and ids, both severities.
fn noisy_def() -> (CardDefinitionDSL, LintContext) {
    let mut f0 = face_with(vec![
        with(
            &with(&good_activated_mana("x0"), "targets", one_target()),
            "abilityWord",
            json!(MARKER),
        ),
        spell("x1", json!([repeat_while(None, json!([draw()]))])),
        triggered(
            "x2",
            json!({"mode":"EVENT","event":{"kind":"DIES","filters":{"lookBack":true}}}),
            json!([native()]),
        ),
        spanned(&keyword("x3"), 0, 9),
        static_ab("x4", json!([char_effect(None)])),
    ]);
    f0.power = Some(ValueExpressionAST(json!(3)));
    f0.text = TextTokenStream(tokens(5));
    let mut f1 = face(FaceKind::Back, "Test Back");
    f1.abilities = vec![
        AbilityAST(linked(&keyword("y0"), &["solo"])),
        AbilityAST(spell("y1", json!([tap_linked("ghost")]))),
        AbilityAST(with(
            &replacement("y2", "DIES", json!([modify_event()])),
            "tierHint",
            json!("CONTROL_ETB"),
        )),
        AbilityAST(spanned(
            &spell("y3", json!([create_token("test.token_zz")])),
            1,
            1,
        )),
    ];
    f1.text = TextTokenStream(vec![
        lit("u0", MARKER),
        lit("u1", MARKER),
        lit("u2", MARKER),
        lit("u3", MARKER),
    ]);
    f1.mana_cost = Some(mana(vec![ManaSymbol::Colored(Color::Green)]));
    let mut d = def_of(LayoutBehavior::DfcNonmodal, vec![f0, f1]);
    d.color_identity = ColorBitmask::WHITE;
    d.related_tokens = Some(vec![
        "test.token_a".to_string(),
        "test.token_zz".to_string(),
    ]);
    d.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: true,
        partner_abilities: vec![PartnerAbilityDSL(static_ab(
            "p0",
            json!([rule_effect(Some("SKIP"))]),
        ))],
    });
    let ctx = LintContext {
        source: Some(SourceCharacteristics {
            faces: vec![
                SourceFaceCharacteristics {
                    mana_value: Some(7),
                    colors: Some(ColorBitmask::BLUE),
                },
                SourceFaceCharacteristics {
                    mana_value: Some(1),
                    colors: Some(ColorBitmask::GREEN),
                },
            ],
            color_identity: Some(ColorBitmask::BLACK),
        }),
        known_token_ids: Some(["test.token_a".to_string()].into_iter().collect()),
    };
    (d, ctx)
}

/// REQ-ING-009 (S-0): a report with findings in many faces/abilities/ids is equal to its own
/// independently sorted and de-duplicated version; repeated calls give equal reports.
#[test]
fn req_ing_009_ordering_is_total_and_deterministic() {
    let (d, ctx) = noisy_def();
    let r = run(&d, &ctx);
    assert_eq!(r.lints, sorted_dedup(&r.lints));
    let ids: BTreeSet<LintId> = r.lints.iter().map(|l| l.id).collect();
    assert!(ids.len() >= 8, "expected many ids, got:\n{}", dump(&r));
    assert!(r.errors().count() > 0 && r.warnings().count() > 0);
    let faces: BTreeSet<Option<usize>> = r.lints.iter().map(|l| l.location.face_index).collect();
    assert_eq!(faces, [None, Some(0), Some(1)].into_iter().collect());
    // face None comes first
    assert_eq!(r.lints[0].location.face_index, None);
    for _ in 0..3 {
        assert_eq!(crate::report::run(&d, &ctx), r);
    }
    let cloned = d.clone();
    assert_eq!(run(&cloned, &ctx.clone()), r);
}

/// REQ-ING-009 (S-0): reordering the abilities of a face moves each ability's findings with it.
#[test]
fn req_ing_009_ability_reorder_invariance() {
    let abilities = [
        with(&good_activated_mana("q0"), "targets", one_target()),
        spell("q1", json!([repeat_while(Some(json!(0)), json!([draw()]))])),
        with(
            &replacement("q2", "DIES", json!([modify_event()])),
            "tierHint",
            json!("COPY_ETB"),
        ),
        triggered(
            "q3",
            json!({"mode":"EVENT","event":{"kind":"DIES","filters":{"LOOK_BACK":1}}}),
            json!([native()]),
        ),
    ];
    let key = |r: &LintReport, j: usize| -> Vec<(LintId, Severity)> {
        let p = ab(0, j);
        let mut v: Vec<(LintId, Severity)> = r
            .lints
            .iter()
            .filter(|l| l.location.path == p)
            .map(|l| (l.id, l.severity))
            .collect();
        v.sort();
        v
    };
    let base = run0(&def_abilities(abilities.to_vec()));
    let base_keys: Vec<_> = (0..4).map(|j| key(&base, j)).collect();
    for k in &base_keys {
        assert!(!k.is_empty());
    }
    let perms: [[usize; 4]; 5] = [
        [3, 2, 1, 0],
        [1, 0, 3, 2],
        [2, 3, 0, 1],
        [0, 2, 1, 3],
        [3, 0, 2, 1],
    ];
    for perm in perms {
        // new position p holds original ability perm[p]
        let reordered: Vec<Value> = perm.iter().map(|&o| abilities[o].clone()).collect();
        let r = run0(&def_abilities(reordered));
        for (p, &o) in perm.iter().enumerate() {
            assert_eq!(key(&r, p), base_keys[o], "perm {perm:?} position {p}");
        }
        assert_eq!(r.lints.len(), base.lints.len());
    }
}

/// REQ-ING-009 (S-0): a definition with no findings has an empty report.
#[test]
fn req_ing_009_clean_definition_has_empty_report() {
    let d = clean_def();
    assert!(undecodable_positions(&d).is_empty());
    let r = run0(&d);
    assert!(r.is_empty(), "clean definition has findings:\n{}", dump(&r));
    // also with a catalogue that knows the token and a matching source
    let ctx = LintContext {
        source: Some(SourceCharacteristics {
            faces: vec![
                SourceFaceCharacteristics {
                    mana_value: Some(2),
                    colors: Some(ColorBitmask::BLUE),
                },
                SourceFaceCharacteristics {
                    mana_value: Some(2),
                    colors: Some(ColorBitmask::RED),
                },
            ],
            color_identity: Some(or(ColorBitmask::BLUE, ColorBitmask::RED)),
        }),
        known_token_ids: Some(["test.token_a".to_string()].into_iter().collect()),
    };
    let r = run(&d, &ctx);
    assert!(r.is_empty(), "clean definition has findings:\n{}", dump(&r));
}

/// REQ-ING-009 (S-0): no message copies the marker held by LITERAL tokens and ability words.
#[test]
fn req_ing_009_messages_never_copy_text_or_ability_words() {
    let (mut d, ctx) = noisy_def();
    // An uncovered marker-bearing text and a broken marker-bearing ability on face 1.
    d.faces[1]
        .abilities
        .push(AbilityAST(json!({"abilityWord": MARKER, "kind": MARKER})));
    let r = run(&d, &ctx);
    assert!(!r.is_empty());
    for l in &r.lints {
        assert!(
            !l.message.contains(MARKER),
            "message copies source text: {l}"
        );
        assert!(!l.to_string().contains(MARKER));
    }
}

// ================================================================ S-1 FPL-SCHEMA

/// REQ-ING-009 (S-1): every undecodable position gets one Error at that position (not only
/// the first); a decodable position gets none.
#[test]
fn req_ing_009_schema_every_position() {
    let mut f0 = face_with(vec![json!("keyword"), keyword("k1")]);
    f0.power = Some(ValueExpressionAST(json!(3)));
    f0.toughness = Some(ValueExpressionAST(json!({"LITERAL": 1})));
    f0.text = TextTokenStream(vec![lit("t0", MARKER), json!({"t": "word"})]);
    let mut f1 = face(FaceKind::Back, "Test Back");
    f1.power = Some(ValueExpressionAST(json!({"LITERAL": 2})));
    f1.toughness = Some(ValueExpressionAST(json!(3)));
    f1.abilities = vec![
        AbilityAST(keyword("k2")),
        AbilityAST(json!({"kind": "FLYING"})),
        AbilityAST(keyword("k3")),
        AbilityAST(Value::Null),
    ];
    let mut d = def_of(LayoutBehavior::DfcNonmodal, vec![f0, f1]);
    d.support_status = SupportStatus::Unsupported;
    d.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: true,
        partner_abilities: vec![
            PartnerAbilityDSL(json!({"abilityId":"p1","kind":"KEYWORD","keyword":"PARTNER"})),
            PartnerAbilityDSL(static_ab("p2", json!([rule_effect(Some("SKIP"))]))),
        ],
    });
    let expected: BTreeSet<String> = undecodable_positions(&d)
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    let want: BTreeSet<String> = [
        "faces[0].abilities[0]",
        "faces[0].power",
        "faces[0].text",
        "faces[1].abilities[1]",
        "faces[1].abilities[3]",
        "faces[1].toughness",
        "commanderEligibility.partnerAbilities[1]",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(expected, want, "fixture self-check");
    let r = run0(&d);
    let schema = of(&r, LintId::SCHEMA);
    assert!(schema.iter().all(|l| l.severity == E));
    let got: Vec<String> = schema.iter().map(|l| l.location.path.clone()).collect();
    let got_set: BTreeSet<String> = got.iter().cloned().collect();
    assert_eq!(got_set, want, "report:\n{}", dump(&r));
    assert_eq!(
        got.len(),
        want.len(),
        "one finding per position:\n{}",
        dump(&r)
    );
    // exact locations
    let p = assert_at(&r, LintId::SCHEMA, E, "faces[1].abilities[3]");
    assert_eq!(p.location.face_index, Some(1));
    assert_eq!(p.location.ability_index, Some(3));
    let p = assert_at(
        &r,
        LintId::SCHEMA,
        E,
        "commanderEligibility.partnerAbilities[1]",
    );
    assert_eq!(
        (p.location.face_index, p.location.ability_index),
        (None, None)
    );
    let p = assert_at(&r, LintId::SCHEMA, E, "faces[0].power");
    assert_eq!(
        (p.location.face_index, p.location.ability_index),
        (Some(0), None)
    );
}

/// REQ-ING-009 (S-1): a clean definition has no FPL-SCHEMA finding.
#[test]
fn req_ing_009_schema_clean() {
    assert_none(&run0(&clean_def()), LintId::SCHEMA);
}

// ================================================================ S-2 FPL-MANA-ABILITY

fn mana_fail(v: Value) {
    let r = run0(&def_abilities(vec![v.clone()]));
    let l = assert_at(&r, LintId::MANA_ABILITY, E, &a00());
    assert_eq!(l.location.face_index, Some(0));
    assert_eq!(l.location.ability_index, Some(0));
    if AbilityAST(v).to_real().is_err() {
        assert_at(&r, LintId::SCHEMA, E, &a00());
    }
}

fn mana_pass(v: Value) {
    let r = run0(&def_abilities(vec![v]));
    assert_none(&r, LintId::MANA_ABILITY);
}

/// REQ-ING-009 (S-2; CR 605.1a): a valid activated mana ability is clean.
#[test]
fn req_ing_009_mana_ability_activated_pass() {
    // CR 605.1a
    mana_pass(good_activated_mana("m"));
    mana_pass(with(
        &good_activated_mana("m"),
        "cost",
        json!([{"cost":"TAP_SOURCE"},{"cost":"PAY_MANA","mana":"{1}"}]),
    ));
}

/// REQ-ING-009 (S-2; CR 605.1a, 115.6): an activated mana ability that requires a target.
#[test]
fn req_ing_009_mana_ability_activated_target() {
    // CR 605.1a, CR 115.6
    mana_fail(with(&good_activated_mana("m"), "targets", one_target()));
    // CR 115.6: a target with min 0 still "requires" a target.
    mana_fail(with(
        &good_activated_mana("m"),
        "targets",
        json!([{"slot":0,"selector":{"ref":"SELF"},"min":0,"max":1}]),
    ));
}

/// REQ-ING-009 (S-2; CR 605.1a): loyalty abilities (flag or LOYALTY cost) are not mana abilities.
#[test]
fn req_ing_009_mana_ability_activated_loyalty() {
    // CR 605.1a
    mana_fail(with(&good_activated_mana("m"), "isLoyalty", json!(true)));
    mana_fail(with(
        &good_activated_mana("m"),
        "cost",
        json!([{"cost":"LOYALTY","delta":{"LITERAL":1}}]),
    ));
}

/// REQ-ING-009 (S-2; CR 605.1a): a library move (SEARCH_LIBRARY, MILL, DRAW) disqualifies.
#[test]
fn req_ing_009_mana_ability_activated_library_move() {
    // CR 605.1a
    for op in [search_library(), mill(), draw()] {
        mana_fail(with(
            &good_activated_mana("m"),
            "effect",
            json!([native(), op.clone()]),
        ));
        // nested inside a control instruction as well
        mana_fail(with(
            &good_activated_mana("m"),
            "effect",
            json!([native(), if_then(json!([op]))]),
        ));
    }
}

/// REQ-ING-009 (S-2; CR 605.1a): an empty effect is an Error.
#[test]
fn req_ing_009_mana_ability_activated_empty_effect() {
    // CR 605.1a
    mana_fail(with(&good_activated_mana("m"), "effect", json!([])));
}

/// REQ-ING-009 (S-2; CR 605.1a): one Error per violated criterion.
#[test]
fn req_ing_009_mana_ability_one_error_per_criterion() {
    // CR 605.1a
    let v = with(
        &with(
            &with(&good_activated_mana("m"), "targets", one_target()),
            "isLoyalty",
            json!(true),
        ),
        "effect",
        json!([draw()]),
    );
    let r = run0(&def_abilities(vec![v]));
    let n = paths(&r, LintId::MANA_ABILITY, E)
        .iter()
        .filter(|p| **p == a00())
        .count();
    assert!(n >= 3, "expected one Error per criterion:\n{}", dump(&r));
}

/// REQ-ING-009 (S-2): only `isManaAbility: true` can be wrong; absent or false never is.
#[test]
fn req_ing_009_mana_ability_flag_absent_or_false() {
    let bad_body = with(
        &with(
            &with(&good_activated_mana("m"), "targets", one_target()),
            "isLoyalty",
            json!(true),
        ),
        "effect",
        json!([]),
    );
    mana_pass(with(&bad_body, "isManaAbility", json!(false)));
    mana_pass(without(&bad_body, "isManaAbility"));
    mana_pass(triggered("t", step_trigger(), json!([])));
    mana_pass(spell("s", json!([draw()])));
    mana_pass(with(
        &spell("s", json!([draw()])),
        "isManaAbility",
        json!(false),
    ));
}

/// REQ-ING-009 (S-2; CR 605.1b): a triggered mana ability on ACTIVATED or RESOLVED is clean,
/// even with a library op (the library clause is in 605.1a only).
#[test]
fn req_ing_009_mana_ability_triggered_pass() {
    // CR 605.1b
    for ev in ["ACTIVATED", "RESOLVED"] {
        mana_pass(good_triggered_mana("t", ev));
        // CR 605.1b has no library clause (unlike CR 605.1a): regression for the SRS paraphrase.
        let with_search = with(
            &good_triggered_mana("t", ev),
            "effect",
            json!([native(), search_library()]),
        );
        assert_decodes(&with_search);
        mana_pass(with_search);
    }
}

/// REQ-ING-009 (S-2; CR 605.1b, 115.6): a triggered mana ability with a target, a wrong
/// trigger, or an empty effect is an Error.
#[test]
fn req_ing_009_mana_ability_triggered_fail() {
    // CR 605.1b, CR 115.6
    mana_fail(with(
        &good_triggered_mana("t", "ACTIVATED"),
        "targets",
        one_target(),
    ));
    // CR 605.1b: not triggered from activation/resolution of a mana ability
    mana_fail(with(
        &good_triggered_mana("t", "RESOLVED"),
        "trigger",
        step_trigger(),
    ));
    mana_fail(with(
        &good_triggered_mana("t", "RESOLVED"),
        "trigger",
        event_trigger("ENTERS"),
    ));
    mana_fail(with(
        &good_triggered_mana("t", "RESOLVED"),
        "trigger",
        event_trigger("DIES"),
    ));
    mana_fail(with(
        &good_triggered_mana("t", "ACTIVATED"),
        "effect",
        json!([]),
    ));
}

/// REQ-ING-009 (S-2; CR 605.1a, 605.1b): SPELL, STATIC and REPLACEMENT abilities cannot be
/// mana abilities; KEYWORD with the flag is not this lint's concern.
#[test]
fn req_ing_009_mana_ability_other_kinds() {
    // CR 605.1a, CR 605.1b
    mana_fail(with(
        &spell("s", json!([native()])),
        "isManaAbility",
        json!(true),
    ));
    mana_fail(with(
        &static_ab("s", json!([rule_effect(Some("SKIP"))])),
        "isManaAbility",
        json!(true),
    ));
    mana_fail(with(
        &replacement("s", "DIES", json!([skip_op()])),
        "isManaAbility",
        json!(true),
    ));
    mana_pass(with(&keyword("k"), "isManaAbility", json!(true)));
}

/// REQ-ING-009 (S-2): location of a finding on a later face and ability.
#[test]
fn req_ing_009_mana_ability_location() {
    let mut f1 = face(FaceKind::Back, "Test Back");
    f1.abilities = vec![
        AbilityAST(keyword("k")),
        AbilityAST(good_activated_mana("ok")),
        AbilityAST(with(&good_activated_mana("bad"), "effect", json!([]))),
    ];
    let d = def_of(LayoutBehavior::DfcNonmodal, vec![face_with(vec![]), f1]);
    let r = run0(&d);
    let l = assert_at(&r, LintId::MANA_ABILITY, E, &ab(1, 2));
    assert_eq!(l.location.face_index, Some(1));
    assert_eq!(l.location.ability_index, Some(2));
    assert!(
        r.with_id(LintId::MANA_ABILITY)
            .all(|l| l.location.path == ab(1, 2))
    );
}

// ================================================================ S-3 FPL-TIER-HINT

const TIERS: [&str; 3] = ["CONTROL_ETB", "COPY_ETB", "BACK_FACE_ETB"];

fn tiered(tier: &str, event: &str, ops: Value) -> Value {
    with(&replacement("r", event, ops), "tierHint", json!(tier))
}

/// REQ-ING-009 (S-3; CR 616.1b-d): a tier-hinted ENTERS replacement with a real op is clean.
#[test]
fn req_ing_009_tier_hint_pass() {
    // CR 616.1b, CR 616.1c, CR 616.1d
    for t in TIERS {
        for ops in [
            json!([modify_event()]),
            json!([instead()]),
            json!([skip_op(), modify_event()]),
        ] {
            let v = tiered(t, "ENTERS", ops);
            assert_decodes(&v);
            for self_rep in [None, Some(true), Some(false)] {
                let v = match self_rep {
                    Some(b) => with(&v, "isSelfReplacement", json!(b)),
                    None => v.clone(),
                };
                assert_none(&run0(&def_abilities(vec![v])), LintId::TIER_HINT);
            }
        }
    }
}

/// REQ-ING-009 (S-3; CR 616.1b-d): wrong event, or only SKIP/PREVENT ops, is an Error.
#[test]
fn req_ing_009_tier_hint_fail() {
    // CR 616.1b, CR 616.1c, CR 616.1d
    for t in TIERS {
        let mut bad = vec![
            tiered(t, "ZONE_CHANGE", json!([modify_event()])),
            tiered(t, "DIES", json!([instead()])),
            tiered(t, "ENTERS", json!([skip_op()])),
            tiered(t, "ENTERS", json!([prevent_op()])),
            tiered(t, "ENTERS", json!([skip_op(), prevent_op()])),
        ];
        let more: Vec<Value> = bad
            .iter()
            .flat_map(|v| {
                [
                    with(v, "isSelfReplacement", json!(true)),
                    with(v, "isSelfReplacement", json!(false)),
                ]
            })
            .collect();
        bad.extend(more);
        for v in bad {
            let r = run0(&def_abilities(vec![keyword("k"), v.clone()]));
            let l = assert_at(&r, LintId::TIER_HINT, E, &ab(0, 1));
            assert_eq!(l.location.ability_index, Some(1));
        }
    }
}

/// REQ-ING-009 (S-3): no tierHint is never an error, whatever the event.
#[test]
fn req_ing_009_tier_hint_absent_never_error() {
    for ev in ["ENTERS", "ZONE_CHANGE", "DIES"] {
        for ops in [
            json!([skip_op()]),
            json!([prevent_op()]),
            json!([modify_event()]),
        ] {
            let v = replacement("r", ev, ops);
            assert_none(&run0(&def_abilities(vec![v])), LintId::TIER_HINT);
        }
    }
}

// ================================================================ S-4 FPL-LAYER-KIND

/// REQ-ING-009 (S-4): valid CHAR/RULE/PLAYER effects are clean.
#[test]
fn req_ing_009_layer_kind_pass() {
    for effs in [
        json!([char_effect(Some("6"))]),
        json!([char_effect(Some("7b")), rule_effect(Some("EVASION"))]),
        json!([player_effect()]),
        json!([
            rule_effect(Some("SKIP")),
            player_effect(),
            char_effect(Some("1a"))
        ]),
    ] {
        let v = static_ab("s", effs);
        assert_decodes(&v);
        assert_none(&run0(&def_abilities(vec![v])), LintId::LAYER_KIND);
    }
}

/// REQ-ING-009 (S-4): CHAR without layer, RULE without kind, missing or unknown type.
#[test]
fn req_ing_009_layer_kind_fail() {
    for bad in [
        char_effect(None),
        rule_effect(None),
        json!({"kind":"SKIP"}),
        json!({"type":"BOGUS_TYPE","kind":"SKIP"}),
        json!({"layer":"6","affected":{"ref":"SELF"},"modification":{"op":"TEST_OP"}}),
    ] {
        let v = static_ab("s", json!([char_effect(Some("6")), bad]));
        let r = run0(&def_abilities(vec![keyword("k"), v]));
        let l = assert_at(&r, LintId::LAYER_KIND, E, &ab(0, 1));
        assert_eq!(
            (l.location.face_index, l.location.ability_index),
            (Some(0), Some(1))
        );
    }
}

// ================================================================ S-5 FPL-TEXT-COVERAGE

/// A coverage finding as (severity, path, span).
type Cov = (Severity, String, Option<(u32, u32)>);
/// A coverage finding with its face index.
type CovAt = (Severity, String, Option<(u32, u32)>, Option<usize>);
/// (token count, ability spans, expected uncovered runs).
type GapCase = (usize, Vec<(u64, u64)>, Vec<(u32, u32)>);

fn coverage(r: &LintReport) -> Vec<Cov> {
    r.with_id(LintId::TEXT_COVERAGE)
        .map(|l| (l.severity, l.location.path.clone(), l.location.text_span))
        .collect()
}

fn text_def(n: usize, spans: &[(u64, u64)], status: SupportStatus) -> CardDefinitionDSL {
    let abilities = spans
        .iter()
        .enumerate()
        .map(|(j, (s, e))| spanned(&keyword(&format!("k{j}")), *s, *e))
        .collect();
    let mut f = face_with(abilities);
    f.text = TextTokenStream(tokens(n));
    let mut d = def_abilities(vec![]);
    d.faces = vec![f];
    d.support_status = status;
    d
}

fn text_path() -> String {
    "faces[0].text".to_string()
}

/// REQ-ING-009 (S-5): full coverage (one ability, two abutting, overlaps, gaps filled) is clean.
#[test]
fn req_ing_009_text_coverage_pass() {
    for status in [
        SupportStatus::Verified,
        SupportStatus::AutoParsed,
        SupportStatus::Partial,
        SupportStatus::Unsupported,
    ] {
        for spans in [
            vec![(0, 9)],
            vec![(0, 4), (5, 9)],
            vec![(5, 9), (0, 4)],
            vec![(0, 6), (3, 9)],
            vec![(0, 2), (6, 9), (3, 5), (2, 7)],
        ] {
            let d = text_def(10, &spans, status);
            assert!(undecodable_positions(&d).is_empty());
            let r = run0(&d);
            assert_eq!(coverage(&r), vec![], "{spans:?} {status:?}\n{}", dump(&r));
        }
        // N = 0: no coverage finding
        let r = run0(&text_def(0, &[], status));
        assert_eq!(coverage(&r), vec![]);
    }
}

/// REQ-ING-009 (S-5): uncovered maximal runs, one finding each, ascending span order; Error for
/// VERIFIED and AUTO_PARSED, Warning for PARTIAL, nothing for UNSUPPORTED.
#[test]
fn req_ing_009_text_coverage_gaps() {
    for (status, sev) in [
        (SupportStatus::Verified, Some(E)),
        (SupportStatus::AutoParsed, Some(E)),
        (SupportStatus::Partial, Some(W)),
        (SupportStatus::Unsupported, None),
    ] {
        let cases: Vec<GapCase> = vec![
            // token 10 uncovered
            (11, vec![(0, 4), (5, 9)], vec![(10, 10)]),
            // two separate gaps
            (10, vec![(2, 3), (6, 7)], vec![(0, 1), (4, 5), (8, 9)]),
            (10, vec![(0, 3), (6, 9)], vec![(4, 5)]),
            (10, vec![(1, 1), (3, 3)], vec![(0, 0), (2, 2), (4, 9)]),
            // no abilities at all: the whole text
            (3, vec![], vec![(0, 2)]),
        ];
        for (n, spans, gaps) in cases {
            let r = run0(&text_def(n, &spans, status));
            let want: Vec<Cov> = match sev {
                Some(s) => gaps.iter().map(|g| (s, text_path(), Some(*g))).collect(),
                None => vec![],
            };
            assert_eq!(
                coverage(&r),
                want,
                "{status:?} n={n} {spans:?}\n{}",
                dump(&r)
            );
            for l in r.with_id(LintId::TEXT_COVERAGE) {
                assert_eq!(l.location.face_index, Some(0));
                assert_eq!(l.location.ability_index, None);
            }
        }
    }
}

/// REQ-ING-009 (S-5): an ability with no textSpan covers nothing.
#[test]
fn req_ing_009_text_coverage_ability_without_span() {
    let mut f = face_with(vec![keyword("k0"), spanned(&keyword("k1"), 0, 1)]);
    f.text = TextTokenStream(tokens(4));
    let d = def_of(LayoutBehavior::Normal, vec![f]);
    let r = run0(&d);
    assert_eq!(coverage(&r), vec![(E, text_path(), Some((2, 3)))]);
}

/// REQ-ING-009 (S-5): coverage is per face (face 1 finding at faces[1].text).
#[test]
fn req_ing_009_text_coverage_second_face() {
    let mut f0 = face_with(vec![spanned(&keyword("k0"), 0, 1)]);
    f0.text = TextTokenStream(tokens(2));
    let mut f1 = face(FaceKind::Back, "Test Back");
    f1.abilities = vec![AbilityAST(spanned(&keyword("k1"), 1, 1))];
    f1.text = TextTokenStream(vec![
        lit("v0", MARKER),
        lit("v1", MARKER),
        lit("v2", MARKER),
    ]);
    let d = def_of(LayoutBehavior::DfcNonmodal, vec![f0, f1]);
    let r = run0(&d);
    let got: Vec<Lint> = of(&r, LintId::TEXT_COVERAGE);
    let want: Vec<CovAt> = vec![
        (E, "faces[1].text".to_string(), Some((0, 0)), Some(1)),
        (E, "faces[1].text".to_string(), Some((2, 2)), Some(1)),
    ];
    let got: Vec<_> = got
        .iter()
        .map(|l| {
            (
                l.severity,
                l.location.path.clone(),
                l.location.text_span,
                l.location.face_index,
            )
        })
        .collect();
    assert_eq!(got, want);
}

/// REQ-ING-009 (S-5): a span ending at or past N is an Error at that ability (VERIFIED,
/// AUTO_PARSED and PARTIAL).
#[test]
fn req_ing_009_text_coverage_span_out_of_range() {
    for status in [
        SupportStatus::Verified,
        SupportStatus::AutoParsed,
        SupportStatus::Partial,
    ] {
        for (s, e) in [(0u64, 5u64), (3, 4), (4, 4), (2, 4_294_967_295)] {
            // N = 4 tokens; abilities: [0] covers everything validly, [1] is out of range.
            let d = text_def(4, &[(0, 3), (s, e)], status);
            let r = run0(&d);
            let l = assert_at(&r, LintId::TEXT_COVERAGE, E, &ab(0, 1));
            assert_eq!(l.location.ability_index, Some(1));
            if e < 4 {
                panic!("fixture: span must be out of range");
            }
            // ability 0 is in range and not reported
            assert!(
                r.with_id(LintId::TEXT_COVERAGE)
                    .all(|l| l.location.path != ab(0, 0))
            );
        }
        // An out-of-range span still covers the in-range tokens it spans.
        let r = run0(&text_def(4, &[(0, 7)], status));
        assert_exactly(&r, LintId::TEXT_COVERAGE, &[(E, a00())]);
    }
}

// ================================================================ S-6 FPL-LOOK-BACK

/// REQ-ING-009 (S-6; CR 603.10): a look-back key at any depth, any spelling, any value.
#[test]
fn req_ing_009_look_back_fail() {
    // CR 603.10
    let keys = [
        "lookBack",
        "LOOK_BACK",
        "look-back",
        "lookback_flag",
        "LookBackTime",
    ];
    let values = [json!(true), json!(false), json!({"k1": 1})];
    for k in keys {
        for val in &values {
            let mut filters = serde_json::Map::new();
            filters.insert(k.to_string(), val.clone());
            let mut in_trigger = json!({"mode":"EVENT","event":{"kind":"DIES"}});
            in_trigger[k] = val.clone();
            let mut params = serde_json::Map::new();
            params.insert(k.to_string(), val.clone());
            let mut top = keyword("k");
            top[k] = val.clone();
            let cases = [
                triggered(
                    "t",
                    json!({"mode":"EVENT","event":{"kind":"DIES","filters":Value::Object(filters)}}),
                    json!([native()]),
                ),
                triggered("t", in_trigger, json!([native()])),
                spell("s", json!([native_with(Value::Object(params.clone()))])),
                spell(
                    "s",
                    json!([if_then(json!([native_with(
                        json!({"deep":[{"x": Value::Object(params)}]})
                    )]))]),
                ),
                top,
            ];
            for v in cases {
                let r = run0(&def_abilities(vec![keyword("k0"), v.clone()]));
                assert_at(&r, LintId::LOOK_BACK, E, &ab(0, 1));
                assert!(
                    r.with_id(LintId::LOOK_BACK)
                        .all(|l| l.location.path == ab(0, 1))
                );
                if AbilityAST(v).to_real().is_err() {
                    assert_at(&r, LintId::SCHEMA, E, &ab(0, 1));
                }
            }
        }
    }
}

/// REQ-ING-009 (S-6; CR 603.10): keys such as `lookup` or `back` are not look-back keys.
#[test]
fn req_ing_009_look_back_pass() {
    // CR 603.10
    let v = spell(
        "s",
        json!([native_with(
            json!({"lookup": true, "back": {"x": 1}, "look": 1, "backlook": 2})
        )]),
    );
    assert_decodes(&v);
    let r = run0(&def_abilities(vec![v]));
    assert_none(&r, LintId::LOOK_BACK);
    assert_none(&run0(&clean_def()), LintId::LOOK_BACK);
}

// ================================================================ S-7 FPL-MAX-ITER

fn max_iter_wrappers(rw: Value) -> Vec<Value> {
    vec![
        spell("s", json!([rw.clone()])),
        spell("s", json!([if_then(json!([rw.clone()]))])),
        spell("s", json!([for_each(json!([rw.clone()]))])),
        spell("s", json!([may(json!([rw.clone()]))])),
        spell(
            "s",
            json!([repeat_while(Some(json!(2)), json!([rw.clone()]))]),
        ),
        triggered("t", event_trigger("DIES"), json!([draw(), rw.clone()])),
        activated("a", tap_cost(), json!([may(json!([if_then(json!([rw]))]))])),
    ]
}

/// REQ-ING-009 (S-7): REPEAT_WHILE without an integer maxIter >= 1, at any depth.
#[test]
fn req_ing_009_max_iter_fail() {
    let bad_values: Vec<Option<Value>> = vec![
        None,
        Some(json!(0)),
        Some(json!(-1)),
        Some(json!("5")),
        Some(Value::Null),
        Some(num("1.5")),
    ];
    for m in bad_values {
        for v in max_iter_wrappers(repeat_while(m.clone(), json!([draw()]))) {
            let r = run0(&def_abilities(vec![keyword("k"), v.clone()]));
            let l = assert_at(&r, LintId::MAX_ITER, E, &ab(0, 1));
            assert_eq!(l.location.ability_index, Some(1));
            if AbilityAST(v).to_real().is_err() {
                assert_at(&r, LintId::SCHEMA, E, &ab(0, 1));
            }
        }
    }
}

/// REQ-ING-009 (S-7): maxIter of 1 and a large safe integer are clean; another op never is a
/// finding.
#[test]
fn req_ing_009_max_iter_pass() {
    for m in [json!(1), json!(4_294_967_295u64)] {
        for v in max_iter_wrappers(repeat_while(Some(m.clone()), json!([draw()]))) {
            assert_decodes(&v);
            assert_none(&run0(&def_abilities(vec![v])), LintId::MAX_ITER);
        }
    }
    // A large safe integer above u32 (decodability not asserted).
    let v = spell(
        "s",
        json!([repeat_while(
            Some(json!(9_007_199_254_740_991u64)),
            json!([draw()])
        )]),
    );
    assert_none(&run0(&def_abilities(vec![v])), LintId::MAX_ITER);
    // Other ops, even with a bad maxIter member, are not this lint's concern.
    for v in [
        spell("s", json!([draw(), if_then(json!([draw()]))])),
        spell("s", json!([{"op":"NOT_REPEAT_WHILE","maxIter":0}])),
        spell("s", json!([{"op":"REPEAT","maxIter":-1}])),
    ] {
        assert_none(&run0(&def_abilities(vec![v])), LintId::MAX_ITER);
    }
}

// ================================================================ S-8 FPL-TOKEN-REF

fn token_ctx() -> LintContext {
    LintContext {
        source: None,
        known_token_ids: Some(
            ["test.token_a".to_string(), "test.token_b".to_string()]
                .into_iter()
                .collect(),
        ),
    }
}

/// REQ-ING-009 (S-8): an unknown tokenDef at any depth and an unknown relatedTokens entry.
#[test]
fn req_ing_009_token_ref_fail() {
    let mut d = def_abilities(vec![
        spell("s0", json!([create_token("test.token_a")])),
        spell("s1", json!([may(json!([create_token("test.token_zz")]))])),
        triggered(
            "t2",
            event_trigger("DIES"),
            json!([if_then(json!([create_token("test.token_qq")]))]),
        ),
        activated("a3", tap_cost(), json!([create_token_inline()])),
    ]);
    d.related_tokens = Some(vec![
        "test.token_a".to_string(),
        "test.token_unknown".to_string(),
        "test.token_b".to_string(),
    ]);
    let r = run(&d, &token_ctx());
    assert_exactly(
        &r,
        LintId::TOKEN_REF,
        &[
            (E, "relatedTokens[1]".to_string()),
            (E, ab(0, 1)),
            (E, ab(0, 2)),
        ],
    );
    let l = assert_at(&r, LintId::TOKEN_REF, E, "relatedTokens[1]");
    assert_eq!(
        (l.location.face_index, l.location.ability_index),
        (None, None)
    );
}

/// REQ-ING-009 (S-8): known ids and inline sources are clean; no catalogue means no check.
#[test]
fn req_ing_009_token_ref_pass() {
    let mut d = def_abilities(vec![
        spell(
            "s0",
            json!([create_token("test.token_a"), create_token_inline()]),
        ),
        activated(
            "a1",
            tap_cost(),
            json!([may(json!([create_token("test.token_b")]))]),
        ),
    ]);
    d.related_tokens = Some(vec!["test.token_b".to_string()]);
    assert_none(&run(&d, &token_ctx()), LintId::TOKEN_REF);
    // default context: even unknown ids are not reported
    let mut d2 = def_abilities(vec![spell("s0", json!([create_token("test.token_zz")]))]);
    d2.related_tokens = Some(vec!["test.token_zz".to_string()]);
    assert_none(&run0(&d2), LintId::TOKEN_REF);
    // empty catalogue reports every reference
    let empty = LintContext {
        source: None,
        known_token_ids: Some(BTreeSet::new()),
    };
    assert_exactly(
        &run(&d2, &empty),
        LintId::TOKEN_REF,
        &[(E, "relatedTokens[0]".to_string()), (E, a00())],
    );
}

// ================================================================ S-9 FPL-LINK-ID

/// REQ-ING-009 (S-9; CR 607.1): a reference to an undeclared id is an Error at the referencing
/// ability; a declared id referenced nowhere is a Warning at the declaring ability.
#[test]
fn req_ing_009_link_id_fail() {
    // CR 607.1
    let d = def_abilities(vec![
        linked(&keyword("k0"), &["l_ok"]),
        spell("s1", json!([tap_linked("l_ok")])),
        spell("s2", json!([may(json!([tap_linked("l_missing")]))])),
        linked(&keyword("k3"), &["l_unused"]),
    ]);
    let r = run0(&d);
    assert_exactly(&r, LintId::LINK_ID, &[(E, ab(0, 2)), (W, ab(0, 3))]);
}

/// REQ-ING-009 (S-9; CR 607.1c, 607.4, 607.1b): self-linked, many-to-many, two ids on one
/// ability, and cross-face links on a nonmodal DFC are clean.
#[test]
fn req_ing_009_link_id_pass() {
    // CR 607.1c: declares and references the same id
    let self_linked = linked(&spell("s", json!([tap_linked("l1")])), &["l1"]);
    assert_decodes(&self_linked);
    assert_none(&run0(&def_abilities(vec![self_linked])), LintId::LINK_ID);
    // CR 607.4: two or three declarers of one id (referenced or not)
    for n in [2usize, 3] {
        let mut abilities: Vec<Value> = (0..n)
            .map(|i| linked(&keyword(&format!("k{i}")), &["shared"]))
            .collect();
        assert_none(&run0(&def_abilities(abilities.clone())), LintId::LINK_ID);
        abilities.push(spell("s", json!([tap_linked("shared")])));
        assert_none(&run0(&def_abilities(abilities)), LintId::LINK_ID);
    }
    // one ability declaring two ids, both referenced
    let d = def_abilities(vec![
        linked(&keyword("k"), &["l1", "l2"]),
        spell("s", json!([tap_linked("l1"), tap_linked("l2")])),
    ]);
    assert_none(&run0(&d), LintId::LINK_ID);
    // CR 607.1b: declared on face 0, referenced from face 1 of a nonmodal DFC
    let mut back = face(FaceKind::Back, "Test Back");
    back.abilities = vec![AbilityAST(spell("s", json!([tap_linked("cross")])))];
    let d = def_of(
        LayoutBehavior::DfcNonmodal,
        vec![face_with(vec![linked(&keyword("k"), &["cross"])]), back],
    );
    assert_none(&run0(&d), LintId::LINK_ID);
}

// ================================================================ acceptance table (all ids)

struct Case {
    pass: (CardDefinitionDSL, LintContext),
    fail: (CardDefinitionDSL, LintContext),
    severity: Severity,
    path: String,
}

fn src_faces(faces: Vec<SourceFaceCharacteristics>, ci: Option<ColorBitmask>) -> LintContext {
    LintContext {
        source: Some(SourceCharacteristics {
            faces,
            color_identity: ci,
        }),
        known_token_ids: None,
    }
}

fn case(id: LintId) -> Case {
    let one = |v: Value| def_abilities(vec![v]);
    let c0 = ctx0;
    if id == LintId::SCHEMA {
        Case {
            pass: (one(keyword("k")), c0()),
            fail: (one(json!({"kind":"FLYING"})), c0()),
            severity: E,
            path: a00(),
        }
    } else if id == LintId::MANA_ABILITY {
        // CR 605.1a
        Case {
            pass: (one(good_activated_mana("m")), c0()),
            fail: (
                one(with(&good_activated_mana("m"), "targets", one_target())),
                c0(),
            ),
            severity: E,
            path: a00(),
        }
    } else if id == LintId::TIER_HINT {
        // CR 616.1b
        Case {
            pass: (
                one(tiered("CONTROL_ETB", "ENTERS", json!([modify_event()]))),
                c0(),
            ),
            fail: (
                one(tiered("CONTROL_ETB", "DIES", json!([modify_event()]))),
                c0(),
            ),
            severity: E,
            path: a00(),
        }
    } else if id == LintId::LAYER_KIND {
        Case {
            pass: (one(static_ab("s", json!([char_effect(Some("6"))]))), c0()),
            fail: (one(static_ab("s", json!([char_effect(None)]))), c0()),
            severity: E,
            path: a00(),
        }
    } else if id == LintId::TEXT_COVERAGE {
        Case {
            pass: (text_def(3, &[(0, 2)], SupportStatus::AutoParsed), c0()),
            fail: (text_def(3, &[(0, 1)], SupportStatus::AutoParsed), c0()),
            severity: E,
            path: text_path(),
        }
    } else if id == LintId::LOOK_BACK {
        // CR 603.10
        Case {
            pass: (
                one(spell("s", json!([native_with(json!({"lookup":1}))]))),
                c0(),
            ),
            fail: (
                one(spell("s", json!([native_with(json!({"lookBack":1}))]))),
                c0(),
            ),
            severity: E,
            path: a00(),
        }
    } else if id == LintId::MAX_ITER {
        Case {
            pass: (
                one(spell(
                    "s",
                    json!([repeat_while(Some(json!(1)), json!([draw()]))]),
                )),
                c0(),
            ),
            fail: (
                one(spell(
                    "s",
                    json!([repeat_while(Some(json!(0)), json!([draw()]))]),
                )),
                c0(),
            ),
            severity: E,
            path: a00(),
        }
    } else if id == LintId::TOKEN_REF {
        Case {
            pass: (
                one(spell("s", json!([create_token("test.token_a")]))),
                token_ctx(),
            ),
            fail: (
                one(spell("s", json!([create_token("test.token_zz")]))),
                token_ctx(),
            ),
            severity: E,
            path: a00(),
        }
    } else if id == LintId::LINK_ID {
        // CR 607.1
        Case {
            pass: (
                one(linked(&spell("s", json!([tap_linked("l")])), &["l"])),
                c0(),
            ),
            fail: (one(spell("s", json!([tap_linked("l")]))), c0()),
            severity: E,
            path: a00(),
        }
    } else if id == LintId::MANA_VALUE {
        // CR 202.3; REQ-OBJ-001, REQ-ING-002
        let mut d = def_abilities(vec![]);
        d.faces[0].mana_cost = Some(mana(vec![ManaSymbol::Generic(2)]));
        let mv = |n: u32| {
            src_faces(
                vec![SourceFaceCharacteristics {
                    mana_value: Some(n),
                    colors: None,
                }],
                None,
            )
        };
        Case {
            pass: (d.clone(), mv(2)),
            fail: (d, mv(3)),
            severity: E,
            path: "faces[0]".to_string(),
        }
    } else if id == LintId::COLORS {
        // CR 202.2d, 105.2; REQ-OBJ-001, REQ-ING-002
        let mut d = def_abilities(vec![]);
        d.faces[0].mana_cost = Some(mana(vec![ManaSymbol::Colored(Color::Green)]));
        d.color_identity = ColorBitmask::GREEN;
        let col = |c: ColorBitmask| {
            src_faces(
                vec![SourceFaceCharacteristics {
                    mana_value: None,
                    colors: Some(c),
                }],
                None,
            )
        };
        Case {
            pass: (d.clone(), col(ColorBitmask::GREEN)),
            fail: (d, col(ColorBitmask::NONE)),
            severity: E,
            path: "faces[0]".to_string(),
        }
    } else if id == LintId::COLOR_IDENTITY {
        // CR 903.4; REQ-OBJ-001, REQ-ING-002
        let mut d = def_abilities(vec![]);
        d.faces[0].mana_cost = Some(mana(vec![ManaSymbol::Colored(Color::Green)]));
        let mut good = d.clone();
        good.color_identity = ColorBitmask::GREEN;
        Case {
            pass: (good, c0()),
            fail: (d, c0()),
            severity: E,
            path: "$".to_string(),
        }
    } else {
        panic!("no acceptance case for {id}")
    }
}

/// REQ-ING-009, REQ-OBJ-001, REQ-ING-002: every lint id has a passing definition (no finding
/// of that id) and a failing one (the expected finding, severity and exact location).
#[test]
fn req_ing_009_every_lint_has_pass_and_fail_case() {
    for id in LintId::ALL {
        let c = case(id);
        let r = run(&c.pass.0, &c.pass.1);
        assert_none(&r, id);
        let r = run(&c.fail.0, &c.fail.1);
        let l = assert_at(&r, id, c.severity, &c.path);
        assert_eq!(
            parse_path(&c.fail.0, &c.path),
            Some((l.location.face_index, l.location.ability_index))
        );
    }
}
