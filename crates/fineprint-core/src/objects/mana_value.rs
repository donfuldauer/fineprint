// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Mana value (REQ-OBJ-001, CR 202.3).

use fineprint_dsl::mana::{ManaCostDSL, ManaSymbol, Variable};

/// The values chosen for the variable symbols of a spell on the stack (CR 107.3a, 107.3p).
///
/// `z` exists because the DSL accepts `{Z}`; it is treated exactly like `x` and `y`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VariableValues {
    /// The value of `{X}`.
    pub x: u32,
    /// The value of `{Y}`.
    pub y: u32,
    /// The value of `{Z}`.
    pub z: u32,
}

/// Where the object is, for the purpose of `{X}` (CR 202.3e, 107.3g).
///
/// Off the stack every variable symbol counts 0. On the stack each counts the value chosen for
/// it. A copy of a spell copies the value of X (CR 707.10), so the caller computes a copy's mana
/// value by passing the same `OnStack` values as for the original; copies are not modelled
/// anywhere else here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManaValueContext {
    /// The object is not on the stack: X, Y and Z are 0 (CR 202.3e, 107.3g).
    OffStack,
    /// The object is a spell on the stack with these announced values (CR 202.3e, 107.3a).
    OnStack(VariableValues),
}

/// Which half of a split card is cast (CR 709.3b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitHalf {
    /// The left half.
    Left,
    /// The right half.
    Right,
}

/// The state of a split card for mana value (CR 202.3d).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitState {
    /// Not on the stack: both halves count (CR 202.3d, 709.4b).
    NotOnStack,
    /// On the stack with one half cast: only that half counts (CR 202.3d, 709.3b).
    OnStackHalf(SplitHalf),
    /// A fused split spell on the stack: both halves count (CR 202.3d, 709.4d).
    OnStackFused,
}

/// What a mana value is computed for (CR 202.3a through 202.3d).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManaValueSubject<'a> {
    /// Any object using its own mana cost: also a modal double-faced card's face (CR 712.3,
    /// 712.6e), a token, and a face-down object (CR 708.2a). `None` is "no mana cost", mana
    /// value 0 (CR 202.3a).
    Plain {
        /// The object's own mana cost.
        mana_cost: Option<&'a ManaCostDSL>,
    },
    /// The back face, face up, of a nonmodal double-faced permanent or spell: its mana value
    /// uses the front face's cost, never its own (CR 202.3b, 712.6b).
    NonmodalBackFace {
        /// The front face's mana cost.
        front_mana_cost: Option<&'a ManaCostDSL>,
    },
    /// A copy of the back face of a nonmodal double-faced object: mana value 0 (CR 202.3b).
    CopyOfNonmodalBackFace,
    /// A melded permanent: the combined cost of the two front faces (CR 202.3c, 713.5c).
    Melded {
        /// The front faces' mana costs.
        front_mana_costs: [Option<&'a ManaCostDSL>; 2],
    },
    /// A copy of a melded permanent: mana value 0 (CR 202.3c).
    CopyOfMelded,
    /// A split card (CR 202.3d, 709.4b).
    Split {
        /// The left half's mana cost.
        left: Option<&'a ManaCostDSL>,
        /// The right half's mana cost.
        right: Option<&'a ManaCostDSL>,
        /// Where the card is and which half is cast.
        state: SplitState,
    },
}

/// The mana value one symbol contributes (CR 202.3, 202.3e, 202.3f, 202.3g).
#[must_use]
pub fn symbol_mana_value(symbol: ManaSymbol, ctx: ManaValueContext) -> i64 {
    match symbol {
        // CR 107.4b, 107.4d: {n} is n generic mana, {0} is zero.
        ManaSymbol::Generic(n) => i64::from(n),
        // CR 202.3, 107.4c, 107.4h: one colored, colorless or snow mana each.
        ManaSymbol::Colored(_) | ManaSymbol::Colorless | ManaSymbol::Snow => 1,
        // CR 202.3f: the largest component of a two-color hybrid symbol is 1.
        ManaSymbol::Hybrid(_, _) => 1,
        // CR 202.3f: the largest component of {n/c} is n.
        ManaSymbol::GenericHybrid(n, _) => i64::from(n),
        // CR 202.3g: each Phyrexian symbol contributes 1 (hybrid Phyrexian too, CR 202.3f).
        ManaSymbol::Phyrexian(_) | ManaSymbol::HybridPhyrexian(_, _) => 1,
        // CR 202.3f, 107.4: the largest component of {C/c} is 1 ({C} and {c} are both 1).
        ManaSymbol::ColorlessHybrid(_) => 1,
        // CR 202.3e, 107.3g: X is 0 off the stack; CR 107.3a: the announced value on it;
        // CR 107.3p: Y follows the rules of X (Z likewise, DECISIONS.md #21).
        ManaSymbol::Variable(v) => match ctx {
            ManaValueContext::OffStack => 0,
            ManaValueContext::OnStack(values) => i64::from(match v {
                Variable::X => values.x,
                Variable::Y => values.y,
                Variable::Z => values.z,
            }),
        },
    }
}

/// The mana value of a cost; "no mana cost" is 0 (CR 202.3, 202.3a).
fn cost_mana_value(cost: Option<&ManaCostDSL>, ctx: ManaValueContext) -> i64 {
    cost.map_or(0, |c| {
        c.symbols().iter().fold(0i64, |acc, s| {
            acc.saturating_add(symbol_mana_value(*s, ctx))
        })
    })
}

/// The mana value of `subject` in `ctx` (CR 202.3 through 202.3g). Arithmetic saturates.
#[must_use]
pub fn mana_value(subject: &ManaValueSubject<'_>, ctx: ManaValueContext) -> i64 {
    match *subject {
        // CR 202.3, 202.3a: the object's own cost; no mana cost is 0.
        ManaValueSubject::Plain { mana_cost } => cost_mana_value(mana_cost, ctx),
        // CR 202.3b, 712.6b: the front face's cost.
        ManaValueSubject::NonmodalBackFace { front_mana_cost } => {
            cost_mana_value(front_mana_cost, ctx)
        }
        // CR 202.3b, 202.3c: a copy of a nonmodal back face or of a melded permanent is 0.
        ManaValueSubject::CopyOfNonmodalBackFace | ManaValueSubject::CopyOfMelded => 0,
        // CR 202.3c, 713.5c: the combined cost of both front faces.
        ManaValueSubject::Melded { front_mana_costs } => cost_mana_value(front_mana_costs[0], ctx)
            .saturating_add(cost_mana_value(front_mana_costs[1], ctx)),
        // CR 202.3d: both halves off the stack or fused; one half when that half is cast.
        // The split state decides the context: off the stack, X is 0 whatever `ctx` says
        // (CR 202.3e, 107.3g); on the stack the supplied `ctx` applies.
        ManaValueSubject::Split { left, right, state } => match state {
            SplitState::NotOnStack => cost_mana_value(left, ManaValueContext::OffStack)
                .saturating_add(cost_mana_value(right, ManaValueContext::OffStack)),
            SplitState::OnStackFused => {
                cost_mana_value(left, ctx).saturating_add(cost_mana_value(right, ctx))
            }
            SplitState::OnStackHalf(SplitHalf::Left) => cost_mana_value(left, ctx),
            SplitState::OnStackHalf(SplitHalf::Right) => cost_mana_value(right, ctx),
        },
    }
}

#[cfg(test)]
mod colorless_hybrid_tests {
    //! REQ-OBJ-001, REQ-DSL-002: mana value of colorless-hybrid symbols (CR 202.3f, 107.4).

    use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol};

    use super::{ManaValueContext, ManaValueSubject, mana_value, symbol_mana_value};

    fn mv(text: &str) -> i64 {
        let cost = ManaCostDSL::parse(text).expect("cost");
        mana_value(
            &ManaValueSubject::Plain {
                mana_cost: Some(&cost),
            },
            ManaValueContext::OffStack,
        )
    }

    /// REQ-OBJ-001 (CR 202.3f, 107.4): each `{C/c}` has mana value 1.
    #[test]
    fn req_obj_001_cr_202_3f_colorless_hybrid_symbol_is_one() {
        for c in Color::ALL {
            assert_eq!(
                symbol_mana_value(ManaSymbol::ColorlessHybrid(c), ManaValueContext::OffStack),
                1
            );
        }
        assert_eq!(mv("{C/W}"), 1);
    }

    /// REQ-OBJ-001, REQ-DSL-002 (CR 202.3, 202.3f): costs with `{C/c}` sum each symbol as 1.
    #[test]
    fn req_obj_001_cr_202_3f_colorless_hybrid_costs() {
        assert_eq!(mv("{C/W}{1}"), 2);
        assert_eq!(mv("{C/W}{C/U}"), 2);
        assert_eq!(mv("{C/W}{C/W}{1}{U}{C/G}"), 5);
        assert_eq!(mv("{2/B}{C/B}"), 3);
    }
}
