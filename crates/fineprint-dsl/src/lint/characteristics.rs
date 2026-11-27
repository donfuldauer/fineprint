// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `FPL-MANA-VALUE`, `FPL-COLORS` and `FPL-COLOR-IDENTITY` (REQ-ING-009, REQ-ING-002,
//! REQ-OBJ-001): mana value, colors and color identity computed from the definition with
//! integer arithmetic and compared with the declared and source values.
//!
//! Characteristic-defining abilities and continuous effects are runtime and are not computed;
//! color identity is taken from structurally typed mana costs only, never from literal text
//! (docs/DECISIONS.md, "#16 FP-015 — Characteristics checks").

use super::walk::CardFacts;
use super::{Lint, LintContext, LintId, LintLocation, Severity, push};
use crate::card::{CardDefinitionDSL, CardFaceDSL, FaceKind, LayoutBehavior};
use crate::mana::{Color, ManaCostDSL, ManaSymbol};

/// The bitmask bit of `color` (W = 1, U = 2, B = 4, R = 8, G = 16).
const fn bit(color: Color) -> u8 {
    match color {
        Color::White => 1,
        Color::Blue => 2,
        Color::Black => 4,
        Color::Red => 8,
        Color::Green => 16,
    }
}

/// The colors of one symbol (CR 202.2, CR 202.2d: hybrid and Phyrexian symbols are all of
/// their colors).
const fn symbol_colors(symbol: ManaSymbol) -> u8 {
    match symbol {
        ManaSymbol::Colored(c)
        | ManaSymbol::GenericHybrid(_, c)
        | ManaSymbol::ColorlessHybrid(c)
        | ManaSymbol::Phyrexian(c) => bit(c),
        ManaSymbol::Hybrid(a, b) | ManaSymbol::HybridPhyrexian(a, b) => bit(a) | bit(b),
        ManaSymbol::Generic(_)
        | ManaSymbol::Colorless
        | ManaSymbol::Snow
        | ManaSymbol::Variable(_) => 0,
    }
}

/// The colors of a mana cost (CR 202.2).
fn cost_colors(cost: Option<&ManaCostDSL>) -> u8 {
    cost.map_or(0, |c| {
        c.symbols().iter().fold(0, |acc, &s| acc | symbol_colors(s))
    })
}

/// The mana value contribution of one symbol (CR 202.3).
fn symbol_value(symbol: ManaSymbol) -> u64 {
    match symbol {
        ManaSymbol::Generic(n) => u64::from(n),
        // CR 202.3e: X is 0 off the stack.
        ManaSymbol::Variable(_) => 0,
        // CR 202.3f: the largest component of a hybrid symbol.
        ManaSymbol::GenericHybrid(n, _) => u64::from(n.max(1)),
        // CR 202.3g: each Phyrexian symbol contributes 1.
        ManaSymbol::Colored(_)
        | ManaSymbol::Colorless
        | ManaSymbol::Snow
        | ManaSymbol::Hybrid(..)
        | ManaSymbol::ColorlessHybrid(_)
        | ManaSymbol::Phyrexian(_)
        | ManaSymbol::HybridPhyrexian(..) => 1,
    }
}

/// The mana value of a mana cost (CR 202.3, CR 202.3a: no mana cost is 0), saturating.
fn cost_value(cost: Option<&ManaCostDSL>) -> u64 {
    cost.map_or(0, |c| {
        c.symbols()
            .iter()
            .fold(0u64, |acc, &s| acc.saturating_add(symbol_value(s)))
    })
}

/// The face whose mana cost and color indicator give `face` its mana value and colors: the
/// Main face for the Flipped face of a flip card (CR 710.1c: a flip card's color and mana cost
/// don't change if the permanent is flipped), else `face` itself. Modal double-faced cards and
/// every other layout keep their own faces.
fn cost_face<'d>(def: &'d CardDefinitionDSL, face: &'d CardFaceDSL) -> &'d CardFaceDSL {
    match (def.layout, face.face_kind) {
        (LayoutBehavior::Flip, FaceKind::Flipped) => def
            .faces
            .iter()
            .find(|f| f.face_kind == FaceKind::Main)
            .unwrap_or(face),
        _ => face,
    }
}

/// The computed mana value of face `i`, or `None` when it is not computed statically.
fn face_value(def: &CardDefinitionDSL, face: &CardFaceDSL) -> Option<u64> {
    match (def.layout, face.face_kind) {
        // CR 202.3c: a melded permanent's mana value is a runtime property.
        (_, FaceKind::MeldResult) => None,
        // CR 202.3b: the back face of a nonmodal double-faced card uses its front face's cost.
        (LayoutBehavior::DfcNonmodal, FaceKind::Back) => Some(
            def.faces
                .iter()
                .find(|f| f.face_kind == FaceKind::Main)
                .map_or_else(
                    || cost_value(face.mana_cost.as_ref()),
                    |front| cost_value(front.mana_cost.as_ref()),
                ),
        ),
        // CR 202.3d, CR 709.4b: a split card not on the stack has the combined cost of its
        // halves. A Room is a split card with a shared type line (CR 709.5), so it has the
        // combined cost too. The lint has no zone, so it checks the combined value of the
        // zones other than the stack and the battlefield (on the battlefield a locked half
        // has no mana cost, CR 709.5) (docs/DECISIONS.md, "Gate 06-turn F98 — Room mana
        // value in the lint").
        (
            LayoutBehavior::Split | LayoutBehavior::Room,
            FaceKind::LeftHalf | FaceKind::RightHalf,
        ) => Some(
            def.faces
                .iter()
                .filter(|f| matches!(f.face_kind, FaceKind::LeftHalf | FaceKind::RightHalf))
                .fold(0u64, |acc, f| {
                    acc.saturating_add(cost_value(f.mana_cost.as_ref()))
                }),
        ),
        // CR 202.3, CR 202.3a; CR 710.1c: a Flipped face has the Main face's mana cost.
        _ => Some(cost_value(cost_face(def, face).mana_cost.as_ref())),
    }
}

/// The colors of face `face` from its cost and color indicator (CR 202.2, CR 105.2, CR 204.2);
/// for the Flipped face of a flip card, the Main face's (CR 710.1c).
fn face_colors(def: &CardDefinitionDSL, face: &CardFaceDSL) -> u8 {
    own_colors(cost_face(def, face))
}

/// The colors of `face`'s own mana cost and color indicator (CR 202.2, CR 105.2, CR 204.2).
fn own_colors(face: &CardFaceDSL) -> u8 {
    cost_colors(face.mana_cost.as_ref()) | face.color_indicator.map_or(0, |c| c.bits())
}

/// The letters of `bits` in W, U, B, R, G order, or `"none"`.
fn letters(bits: u8) -> String {
    let s: String = Color::ALL
        .into_iter()
        .filter(|&c| bits & bit(c) != 0)
        .map(Color::letter)
        .collect();
    if s.is_empty() { "none".to_owned() } else { s }
}

/// The computed color identity (CR 903.4): over every face (CR 903.4d), the colors of its mana
/// cost and color indicator, plus the symbol colors of every `PAY_MANA` and `WATERBEND` cost
/// node of every ability. Literal text is not scanned (CR 903.4c reminder text cannot
/// be told apart reliably).
fn identity(def: &CardDefinitionDSL, facts: &CardFacts<'_>) -> u8 {
    let faces = def.faces.iter().fold(0, |acc, f| acc | own_colors(f));
    facts
        .abilities
        .iter()
        .flat_map(|a| a.scan.mana_strings.iter())
        .filter_map(|m| ManaCostDSL::parse(m).ok())
        .fold(faces, |acc, c| acc | cost_colors(Some(&c)))
}

/// Pushes the asymmetric color comparison: a computed color missing from `declared` is an
/// Error, a declared color not derivable is a Warning.
fn compare_colors(
    out: &mut Vec<Lint>,
    id: LintId,
    location: &LintLocation,
    what: &str,
    computed: u8,
    declared: u8,
    rule: &str,
) {
    let missing = computed & !declared;
    if missing != 0 {
        push(
            out,
            id,
            Severity::Error,
            location.clone(),
            format!(
                "{what} {} lacks computed colors {} ({rule})",
                letters(declared),
                letters(missing)
            ),
        );
    }
    let extra = declared & !computed;
    if extra != 0 {
        push(
            out,
            id,
            Severity::Warning,
            location.clone(),
            format!(
                "{what} {} has colors {} not derivable from mana costs or color indicators \
                 ({rule})",
                letters(declared),
                letters(extra)
            ),
        );
    }
}

/// Runs the three characteristics checks.
pub(super) fn check(
    def: &CardDefinitionDSL,
    facts: &CardFacts<'_>,
    ctx: &LintContext,
    out: &mut Vec<Lint>,
) {
    if let Some(source) = &ctx.source {
        for (i, (face, src)) in def.faces.iter().zip(&source.faces).enumerate() {
            let location = LintLocation::face(i, "");
            // CR 202.3 (SRS §4.1, REQ-ING-002: recompute and flag mismatches).
            if let (Some(declared), Some(computed)) = (src.mana_value, face_value(def, face))
                && u64::from(declared) != computed
            {
                push(
                    out,
                    LintId::MANA_VALUE,
                    Severity::Error,
                    location.clone(),
                    format!(
                        "source mana value {declared} differs from the computed mana value \
                         {computed} (CR 202.3)"
                    ),
                );
            }
            // CR 202.2, CR 105.2, CR 204.2.
            if let Some(colors) = src.colors {
                compare_colors(
                    out,
                    LintId::COLORS,
                    &location,
                    "source colors",
                    face_colors(def, face),
                    colors.bits(),
                    "CR 202.2",
                );
            }
        }
    }
    // CR 903.4: always against the declared colorIdentity, and against the source when given.
    let computed = identity(def, facts);
    let location = LintLocation::card("$");
    compare_colors(
        out,
        LintId::COLOR_IDENTITY,
        &location,
        "declared colorIdentity",
        computed,
        def.color_identity.bits(),
        "CR 903.4",
    );
    if let Some(src) = ctx.source.as_ref().and_then(|s| s.color_identity) {
        compare_colors(
            out,
            LintId::COLOR_IDENTITY,
            &location,
            "source color identity",
            computed,
            src.bits(),
            "CR 903.4",
        );
    }
}
