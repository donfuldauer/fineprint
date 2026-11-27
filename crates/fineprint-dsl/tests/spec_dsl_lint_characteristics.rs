// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the characteristics lints of `fineprint_dsl::lint` (FP-015): FPL-MANA-VALUE
//! (S-10), FPL-COLORS (S-11) and FPL-COLOR-IDENTITY (S-12). Covers REQ-ING-009, REQ-OBJ-001 and
//! REQ-ING-002. Expected values are computed by integer arithmetic from the brief's examples.
//! Black-box: written from the work-item brief only. Synthetic data only (R5).

#[path = "spec_dsl_lint_common/report.rs"]
mod report;
mod spec_dsl_lint_common;

use fineprint_dsl::card::{
    CardDefinitionDSL, CardFaceDSL, ColorBitmask, FaceKind, LayoutBehavior, SupportStatus,
};
use fineprint_dsl::lint::{
    LintContext, LintId, Severity, SourceCharacteristics, SourceFaceCharacteristics,
};
use fineprint_dsl::mana::{Color, MAX_GENERIC, ManaCostDSL, ManaSymbol, Variable};
use report::*;
use serde_json::json;
use spec_dsl_lint_common::*;

const E: Severity = Severity::Error;
const W: Severity = Severity::Warning;

use Color::{Black as B, Blue as U, Green as G, Red as R, White as Wh};
use ManaSymbol::{
    Colored, ColorlessHybrid, Generic, GenericHybrid, Hybrid, HybridPhyrexian, Phyrexian,
};

fn mv_ctx(values: &[Option<u32>]) -> LintContext {
    LintContext {
        source: Some(SourceCharacteristics {
            faces: values
                .iter()
                .map(|v| SourceFaceCharacteristics {
                    mana_value: *v,
                    colors: None,
                })
                .collect(),
            color_identity: None,
        }),
        known_token_ids: None,
    }
}

fn colors_ctx(values: &[Option<ColorBitmask>]) -> LintContext {
    LintContext {
        source: Some(SourceCharacteristics {
            faces: values
                .iter()
                .map(|c| SourceFaceCharacteristics {
                    mana_value: None,
                    colors: *c,
                })
                .collect(),
            color_identity: None,
        }),
        known_token_ids: None,
    }
}

/// One NORMAL face with `cost`; identity set to the cost's colors so FPL-COLOR-IDENTITY is quiet.
fn costed(cost: Option<ManaCostDSL>, identity: ColorBitmask) -> CardDefinitionDSL {
    let mut d = def_abilities(vec![]);
    d.faces[0].mana_cost = cost;
    d.color_identity = identity;
    d
}

fn c(v: Vec<ManaSymbol>) -> Option<ManaCostDSL> {
    Some(mana(v))
}

fn m(list: &[Color]) -> ColorBitmask {
    list.iter()
        .fold(ColorBitmask::NONE, |acc, c| or(acc, mask_of(*c)))
}

// ================================================================ S-10 FPL-MANA-VALUE

/// The brief's table of costs, their mana value (integer arithmetic) and their colors.
fn mv_table() -> Vec<(Option<ManaCostDSL>, u32, ColorBitmask)> {
    vec![
        // CR 202.3: {3}{U}{U} = 5
        (c(vec![Generic(3), Colored(U), Colored(U)]), 5, m(&[U])),
        // CR 202.3f: {2/B}{2/B}{2/B} = 6
        (
            c(vec![
                GenericHybrid(2, B),
                GenericHybrid(2, B),
                GenericHybrid(2, B),
            ]),
            6,
            m(&[B]),
        ),
        // CR 202.3f: {1}{W/U}{W/U} = 3
        (
            c(vec![Generic(1), Hybrid(Wh, U), Hybrid(Wh, U)]),
            3,
            m(&[Wh, U]),
        ),
        // CR 202.3g: {1}{W/P}{W/P} = 3
        (
            c(vec![Generic(1), Phyrexian(Wh), Phyrexian(Wh)]),
            3,
            m(&[Wh]),
        ),
        // CR 202.3e: {X}{X}{R} = 1
        (
            c(vec![
                ManaSymbol::Variable(Variable::X),
                ManaSymbol::Variable(Variable::X),
                Colored(R),
            ]),
            1,
            m(&[R]),
        ),
        // CR 202.3: {C} = 1, {S} = 1
        (c(vec![ManaSymbol::Colorless]), 1, ColorBitmask::NONE),
        (c(vec![ManaSymbol::Snow]), 1, ColorBitmask::NONE),
        // CR 202.3f: {C/W} = 1
        (c(vec![ColorlessHybrid(Wh)]), 1, m(&[Wh])),
        // CR 202.3f, 202.3g: hybrid Phyrexian = 1
        (c(vec![HybridPhyrexian(G, U)]), 1, m(&[G, U])),
        // CR 202.3: {0} = 0
        (c(vec![Generic(0)]), 0, ColorBitmask::NONE),
        // CR 202.3a: no mana cost = 0
        (None, 0, ColorBitmask::NONE),
    ]
}

/// REQ-ING-009, REQ-OBJ-001, REQ-ING-002 (S-10; CR 202.3, 202.3a, 202.3e-g): the declared
/// source mana value equal to the computed one is clean.
#[test]
fn req_obj_001_req_ing_009_mana_value_table_pass() {
    // CR 202.3, CR 202.3a, CR 202.3e, CR 202.3f, CR 202.3g
    for (cost, mv, colors) in mv_table() {
        let d = costed(cost.clone(), colors);
        let r = run(&d, &mv_ctx(&[Some(mv)]));
        assert!(
            of(&r, LintId::MANA_VALUE).is_empty(),
            "cost {} mv {mv}:\n{}",
            ManaCostDSL::print_optional(cost.as_ref()),
            dump(&r)
        );
    }
}

/// REQ-ING-009, REQ-OBJ-001, REQ-ING-002 (S-10; CR 202.3): a mismatch is an Error at
/// `faces[i]` whose message contains both integers.
#[test]
fn req_obj_001_req_ing_009_mana_value_table_fail() {
    // CR 202.3
    for (cost, mv, colors) in mv_table() {
        let d = costed(cost.clone(), colors);
        let declared = 777u32;
        let r = run(&d, &mv_ctx(&[Some(declared)]));
        let l = assert_at(&r, LintId::MANA_VALUE, E, "faces[0]");
        assert_eq!(
            (l.location.face_index, l.location.ability_index),
            (Some(0), None)
        );
        assert!(
            l.message.contains(&declared.to_string()) && l.message.contains(&mv.to_string()),
            "message lacks declared {declared} or computed {mv}: {l}"
        );
        // and one off in the other direction
        let r = run(&d, &mv_ctx(&[Some(mv + 1)]));
        assert_exactly(&r, LintId::MANA_VALUE, &[(E, "faces[0]".to_string())]);
    }
}

/// REQ-ING-009, REQ-ING-002 (S-10): no source, or a None entry, means no FPL-MANA-VALUE.
#[test]
fn req_ing_002_mana_value_skipped_without_source() {
    let d = costed(c(vec![Generic(3)]), ColorBitmask::NONE);
    assert_none(&run0(&d), LintId::MANA_VALUE);
    assert_none(&run(&d, &mv_ctx(&[None])), LintId::MANA_VALUE);
    assert_none(&run(&d, &mv_ctx(&[])), LintId::MANA_VALUE);
    // a source list longer than the face list is compared only where both have an entry
    assert_none(&run(&d, &mv_ctx(&[Some(3), Some(99)])), LintId::MANA_VALUE);
}

fn two_face(
    layout: LayoutBehavior,
    k0: FaceKind,
    c0: Option<ManaCostDSL>,
    k1: FaceKind,
    c1: Option<ManaCostDSL>,
) -> CardDefinitionDSL {
    let mut f0 = face(k0, "Test Face A");
    f0.mana_cost = c0;
    let mut f1 = face(k1, "Test Face B");
    f1.mana_cost = c1;
    let mut d = def_of(layout, vec![f0, f1]);
    d.support_status = SupportStatus::AutoParsed;
    d
}

/// REQ-ING-009, REQ-OBJ-001 (S-10; CR 202.3a, 202.3b): the back face of a nonmodal DFC with
/// no cost has the front face's mana value.
#[test]
fn req_obj_001_mana_value_dfc_back_face() {
    // CR 202.3b
    let mut d = two_face(
        LayoutBehavior::DfcNonmodal,
        FaceKind::Main,
        c(vec![Generic(3), Colored(U), Colored(U)]),
        FaceKind::Back,
        None,
    );
    d.color_identity = ColorBitmask::BLUE;
    assert_none(&run(&d, &mv_ctx(&[Some(5), Some(5)])), LintId::MANA_VALUE);
    let r = run(&d, &mv_ctx(&[Some(5), Some(0)]));
    assert_exactly(&r, LintId::MANA_VALUE, &[(E, "faces[1]".to_string())]);
    let l = assert_at(&r, LintId::MANA_VALUE, E, "faces[1]");
    assert_eq!(l.location.face_index, Some(1));
    // front listed second: the BACK face still takes the MAIN face's cost
    let mut d = two_face(
        LayoutBehavior::DfcNonmodal,
        FaceKind::Back,
        None,
        FaceKind::Main,
        c(vec![Generic(2), Colored(U)]),
    );
    d.color_identity = ColorBitmask::BLUE;
    assert_none(&run(&d, &mv_ctx(&[Some(3), Some(3)])), LintId::MANA_VALUE);
}

/// REQ-ING-009, REQ-OBJ-001 (S-10; CR 202.3d): each half of a SPLIT card has the combined
/// mana value of both halves.
#[test]
fn req_obj_001_mana_value_split_halves() {
    // CR 202.3d
    let mut d = two_face(
        LayoutBehavior::Split,
        FaceKind::LeftHalf,
        c(vec![Generic(1), Colored(R)]),
        FaceKind::RightHalf,
        c(vec![Generic(3), Colored(U)]),
    );
    d.color_identity = m(&[R, U]);
    assert_none(&run(&d, &mv_ctx(&[Some(6), Some(6)])), LintId::MANA_VALUE);
    let r = run(&d, &mv_ctx(&[Some(2), Some(4)]));
    assert_exactly(
        &r,
        LintId::MANA_VALUE,
        &[(E, "faces[0]".to_string()), (E, "faces[1]".to_string())],
    );
}

/// REQ-ING-009 (S-10; CR 202.3c): a MELD_RESULT face is skipped.
#[test]
fn req_ing_009_mana_value_meld_result_skipped() {
    // CR 202.3c (computed at runtime)
    let mut d = two_face(
        LayoutBehavior::Meld,
        FaceKind::Main,
        c(vec![Generic(4)]),
        FaceKind::MeldResult,
        None,
    );
    d.meld_partner = Some("test.meld_partner".to_string());
    for v in [0u32, 4, 99] {
        let r = run(&d, &mv_ctx(&[Some(4), Some(v)]));
        assert_none(&r, LintId::MANA_VALUE);
    }
}

/// REQ-ING-009, REQ-OBJ-001 (S-10): a huge cost (MAX_GENERIC times many symbols) does not
/// overflow or panic, and is still a mismatch against a small declared value.
#[test]
fn req_obj_001_mana_value_huge_no_overflow() {
    // CR 202.3
    let symbols: Vec<ManaSymbol> = (0..5000).map(|_| Generic(MAX_GENERIC)).collect();
    let d = costed(c(symbols), ColorBitmask::NONE);
    for declared in [0u32, 1, u32::MAX] {
        let r = run(&d, &mv_ctx(&[Some(declared)]));
        assert_at(&r, LintId::MANA_VALUE, E, "faces[0]");
    }
    let hyb: Vec<ManaSymbol> = (0..5000).map(|_| GenericHybrid(MAX_GENERIC, B)).collect();
    let d = costed(c(hyb), ColorBitmask::BLACK);
    let r = run(&d, &mv_ctx(&[Some(0)]));
    assert_at(&r, LintId::MANA_VALUE, E, "faces[0]");
}

// ================================================================ S-11 FPL-COLORS

/// REQ-ING-009, REQ-OBJ-001 (S-11; CR 202.2d, 202.2b): computed colors equal to the source
/// colors are clean, for every symbol kind of the brief's table.
#[test]
fn req_obj_001_colors_table_pass() {
    // CR 202.2b, CR 202.2d
    for (cost, _, colors) in mv_table() {
        let d = costed(cost.clone(), colors);
        let r = run(&d, &colors_ctx(&[Some(colors)]));
        assert_none(&r, LintId::COLORS);
    }
    // GenericHybrid -> that color; ColorlessHybrid -> that color
    let d = costed(c(vec![GenericHybrid(2, G), ColorlessHybrid(R)]), m(&[G, R]));
    assert_none(&run(&d, &colors_ctx(&[Some(m(&[G, R]))])), LintId::COLORS);
}

/// REQ-ING-009, REQ-OBJ-001 (S-11; CR 202.2d): a computed color missing from the source is an
/// Error; a source color not derivable is a Warning; both can occur together.
#[test]
fn req_obj_001_colors_mismatch() {
    // CR 202.2d
    let d = costed(c(vec![Generic(1), Hybrid(Wh, U)]), m(&[Wh, U]));
    let r = run(&d, &colors_ctx(&[Some(m(&[Wh]))]));
    assert_exactly(&r, LintId::COLORS, &[(E, "faces[0]".to_string())]);
    let l = assert_at(&r, LintId::COLORS, E, "faces[0]");
    assert_eq!(
        (l.location.face_index, l.location.ability_index),
        (Some(0), None)
    );
    let r = run(&d, &colors_ctx(&[Some(m(&[Wh, U, R]))]));
    assert_exactly(&r, LintId::COLORS, &[(W, "faces[0]".to_string())]);
    let r = run(&d, &colors_ctx(&[Some(m(&[R]))]));
    assert_exactly(
        &r,
        LintId::COLORS,
        &[(E, "faces[0]".to_string()), (W, "faces[0]".to_string())],
    );
    // Phyrexian and hybrid Phyrexian contribute their colors
    let d = costed(c(vec![Phyrexian(B), HybridPhyrexian(G, U)]), m(&[B, G, U]));
    let r = run(&d, &colors_ctx(&[Some(m(&[B, G]))]));
    assert_exactly(&r, LintId::COLORS, &[(E, "faces[0]".to_string())]);
}

/// REQ-ING-009, REQ-OBJ-001 (S-11; CR 202.2b, 202.2e, 105.2, 204.2): colorless costs with
/// NONE are clean; a color indicator alone adds its color.
#[test]
fn req_obj_001_colors_colorless_and_indicator() {
    // CR 202.2b
    let colorless = costed(
        c(vec![
            Generic(2),
            ManaSymbol::Colorless,
            ManaSymbol::Snow,
            ManaSymbol::Variable(Variable::X),
        ]),
        ColorBitmask::NONE,
    );
    assert_none(
        &run(&colorless, &colors_ctx(&[Some(ColorBitmask::NONE)])),
        LintId::COLORS,
    );
    let r = run(&colorless, &colors_ctx(&[Some(ColorBitmask::GREEN)]));
    assert_exactly(&r, LintId::COLORS, &[(W, "faces[0]".to_string())]);
    // CR 202.2e, CR 105.2, CR 204.2: indicator alone
    let mut ind = costed(None, ColorBitmask::RED);
    ind.faces[0].color_indicator = Some(ColorBitmask::RED);
    assert_none(
        &run(&ind, &colors_ctx(&[Some(ColorBitmask::RED)])),
        LintId::COLORS,
    );
    let r = run(&ind, &colors_ctx(&[Some(ColorBitmask::NONE)]));
    assert_exactly(&r, LintId::COLORS, &[(E, "faces[0]".to_string())]);
    // indicator plus cost
    let mut both = costed(c(vec![Colored(U)]), m(&[U, R]));
    both.faces[0].color_indicator = Some(ColorBitmask::RED);
    assert_none(
        &run(&both, &colors_ctx(&[Some(m(&[U, R]))])),
        LintId::COLORS,
    );
}

/// REQ-ING-009, REQ-ING-002 (S-11): no source or a None entry means no FPL-COLORS; faces
/// compared by index; face 1 reported at faces[1].
#[test]
fn req_ing_002_colors_skipped_and_per_face() {
    let d = costed(c(vec![Colored(G)]), ColorBitmask::GREEN);
    assert_none(&run0(&d), LintId::COLORS);
    assert_none(&run(&d, &colors_ctx(&[None])), LintId::COLORS);
    let mut d = two_face(
        LayoutBehavior::DfcModal,
        FaceKind::Main,
        c(vec![Colored(G)]),
        FaceKind::Back,
        c(vec![Colored(B)]),
    );
    d.color_identity = m(&[G, B]);
    assert_none(
        &run(&d, &colors_ctx(&[Some(m(&[G])), Some(m(&[B]))])),
        LintId::COLORS,
    );
    let r = run(&d, &colors_ctx(&[None, Some(m(&[G]))]));
    assert_exactly(
        &r,
        LintId::COLORS,
        &[(E, "faces[1]".to_string()), (W, "faces[1]".to_string())],
    );
    for l in r.with_id(LintId::COLORS) {
        assert_eq!(l.location.face_index, Some(1));
    }
}

// ================================================================ S-12 FPL-COLOR-IDENTITY

/// REQ-ING-009, REQ-OBJ-001 (S-12; CR 903.4, 903.4d): front {1}{U}, back face red indicator:
/// identity must be blue and red.
#[test]
fn req_obj_001_color_identity_back_face_indicator() {
    // CR 903.4d
    let mut d = two_face(
        LayoutBehavior::DfcNonmodal,
        FaceKind::Main,
        c(vec![Generic(1), Colored(U)]),
        FaceKind::Back,
        None,
    );
    d.faces[1].color_indicator = Some(ColorBitmask::RED);
    d.color_identity = m(&[U, R]);
    assert_none(&run0(&d), LintId::COLOR_IDENTITY);
    d.color_identity = m(&[U]);
    let r = run0(&d);
    assert_exactly(&r, LintId::COLOR_IDENTITY, &[(E, "$".to_string())]);
    let l = assert_at(&r, LintId::COLOR_IDENTITY, E, "$");
    assert_eq!(
        (l.location.face_index, l.location.ability_index),
        (None, None)
    );
    d.color_identity = m(&[U, R, G]);
    assert_exactly(&run0(&d), LintId::COLOR_IDENTITY, &[(W, "$".to_string())]);
    d.color_identity = m(&[G]);
    assert_exactly(
        &run0(&d),
        LintId::COLOR_IDENTITY,
        &[(E, "$".to_string()), (W, "$".to_string())],
    );
}

/// REQ-ING-009, REQ-OBJ-001 (S-12; CR 903.4): PAY_MANA costs of decodable ACTIVATED abilities
/// (also inside COMPOSITE) count toward identity.
#[test]
fn req_obj_001_color_identity_activated_pay_mana() {
    // CR 903.4
    for cost in [
        json!([{"cost":"PAY_MANA","mana":"{3}{R}"},{"cost":"TAP_SOURCE"}]),
        json!([{"cost":"COMPOSITE","costs":[{"cost":"TAP_SOURCE"},{"cost":"PAY_MANA","mana":"{3}{R}"}]}]),
    ] {
        let a = activated("a", cost, json!([native()]));
        assert_decodes(&a);
        let mut d = costed(c(vec![Generic(2)]), ColorBitmask::NONE);
        d.faces[0].abilities = vec![ability_of(a)];
        let r = run0(&d);
        assert_exactly(&r, LintId::COLOR_IDENTITY, &[(E, "$".to_string())]);
        d.color_identity = ColorBitmask::RED;
        assert_none(&run0(&d), LintId::COLOR_IDENTITY);
    }
    // an undecodable activated ability does not contribute. #232 FP-164 (C5): it can no longer
    // be held by a `CardDefinitionDSL`; the same JSON is refused by `from_json` with `Schema`.
    let bad = with(
        &activated(
            "a",
            json!([{"cost":"PAY_MANA","mana":"{R}"}]),
            json!([native()]),
        ),
        "zzzUnknown",
        json!(1),
    );
    assert_undecodable(&bad);
    let d = costed(None, ColorBitmask::NONE);
    assert_document_schema_rejected(
        &patched_document(&d, &[("/faces/0/abilities/0", bad)]),
        "undecodable PAY_MANA ability",
    );
    let r = run0(&d);
    assert_none(&r, LintId::SCHEMA);
    assert_none(&r, LintId::COLOR_IDENTITY);
}

/// REQ-ING-009 (S-12): LITERAL text tokens are not scanned for mana symbols.
#[test]
fn req_ing_009_color_identity_ignores_literal_text() {
    // CR 903.4 (rules-text symbols are not modelled at this baseline; see the brief's limits)
    let mut d = costed(None, ColorBitmask::NONE);
    d.faces[0].text = tx(vec![lit("t0", "{R}"), lit("t1", "{G/U}")]);
    d.faces[0].abilities = vec![ability_of(spanned(&keyword("k"), 0, 1))];
    assert!(undecodable_positions(&d).is_empty());
    let r = run0(&d);
    assert!(r.is_empty(), "unexpected findings:\n{}", dump(&r));
}

/// REQ-ING-009, REQ-ING-002 (S-12): identity is also checked against `source.color_identity`.
#[test]
fn req_ing_002_color_identity_against_source() {
    // CR 903.4
    let mut d = costed(c(vec![Colored(R)]), ColorBitmask::RED);
    let src = |ci: ColorBitmask| LintContext {
        source: Some(SourceCharacteristics {
            faces: vec![],
            color_identity: Some(ci),
        }),
        known_token_ids: None,
    };
    assert_none(&run(&d, &src(ColorBitmask::RED)), LintId::COLOR_IDENTITY);
    // declared identity right, source wrong: reported against the source value
    let r = run(&d, &src(ColorBitmask::NONE));
    assert_exactly(&r, LintId::COLOR_IDENTITY, &[(E, "$".to_string())]);
    let r = run(&d, &src(m(&[R, G])));
    assert_exactly(&r, LintId::COLOR_IDENTITY, &[(W, "$".to_string())]);
    // no source identity: only the declared one is checked
    let none_ci = LintContext {
        source: Some(SourceCharacteristics {
            faces: vec![],
            color_identity: None,
        }),
        known_token_ids: None,
    };
    assert_none(&run(&d, &none_ci), LintId::COLOR_IDENTITY);
    d.color_identity = ColorBitmask::NONE;
    assert_exactly(
        &run(&d, &none_ci),
        LintId::COLOR_IDENTITY,
        &[(E, "$".to_string())],
    );
}

/// REQ-ING-009, REQ-OBJ-001 (S-12; CR 903.4d): identity is the union over ALL faces.
#[test]
fn req_obj_001_color_identity_union_over_faces() {
    // CR 903.4d
    let mut faces: Vec<CardFaceDSL> = Vec::new();
    for (k, sym) in [
        (FaceKind::LeftHalf, Hybrid(Wh, B)),
        (FaceKind::RightHalf, GenericHybrid(2, G)),
    ] {
        let mut f = face(k, "Test Half");
        f.mana_cost = c(vec![sym]);
        faces.push(f);
    }
    let mut d = def_of(LayoutBehavior::Split, faces);
    d.color_identity = m(&[Wh, B, G]);
    assert_none(&run0(&d), LintId::COLOR_IDENTITY);
    d.color_identity = m(&[Wh, B]);
    assert_exactly(&run0(&d), LintId::COLOR_IDENTITY, &[(E, "$".to_string())]);
}
