// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::objects` mana value. Black-box: written from the work-item
//! brief only.
//!
//! One test per mana-value rule (CR 202.3, 202.3a-g), the no-cost versus `{0}` distinction,
//! `symbol_mana_value` for every symbol variant and deterministic property tests (REQ-OBJ-001).

use fineprint_core::objects::{
    ManaValueContext, ManaValueSubject, SplitHalf, SplitState, VariableValues, mana_value,
    symbol_mana_value,
};
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol, Variable};

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn cost(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).expect("synthetic cost parses")
}

fn on_stack(x: u32, y: u32, z: u32) -> ManaValueContext {
    ManaValueContext::OnStack(VariableValues { x, y, z })
}

/// A spread of contexts used where the result must not depend on the context.
fn contexts() -> [ManaValueContext; 5] {
    [
        ManaValueContext::OffStack,
        ManaValueContext::OnStack(VariableValues::default()),
        on_stack(1, 0, 0),
        on_stack(3, 5, 7),
        on_stack(1_000, 2_000, 3_000),
    ]
}

fn plain_mv(text: &str, ctx: ManaValueContext) -> i64 {
    let c = cost(text);
    mana_value(
        &ManaValueSubject::Plain {
            mana_cost: Some(&c),
        },
        ctx,
    )
}

fn split(
    left: Option<&ManaCostDSL>,
    right: Option<&ManaCostDSL>,
    state: SplitState,
    ctx: ManaValueContext,
) -> i64 {
    mana_value(&ManaValueSubject::Split { left, right, state }, ctx)
}

// ---------------------------------------------------------------------------------------------
// Case 1: CR 202.3 basic totals
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.3: mana value is the total amount of mana in the cost, regardless of color.
#[test]
fn mv_202_3_generic_plus_colored_total() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{3}{U}{U}", ctx), 5);
    }
}

/// REQ-OBJ-001, CR 202.3, CR 107.4d: `{0}` represents zero mana.
#[test]
fn mv_202_3_zero_symbol_is_zero() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{0}", ctx), 0);
    }
}

/// REQ-OBJ-001, CR 202.3: a single generic `{1}` is 1.
#[test]
fn mv_202_3_single_generic_one() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{1}", ctx), 1);
    }
}

/// REQ-OBJ-001, CR 202.3: a single colored symbol is 1.
#[test]
fn mv_202_3_single_colored_is_one() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{G}", ctx), 1);
    }
}

/// REQ-OBJ-001, CR 202.3, CR 107.4c: the colorless symbol `{C}` adds 1.
#[test]
fn mv_202_3_colorless_symbol_is_one() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{C}", ctx), 1);
    }
}

/// REQ-OBJ-001, CR 202.3, CR 107.4h: the snow symbol `{S}` adds 1.
#[test]
fn mv_202_3_snow_symbol_is_one() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{S}", ctx), 1);
    }
}

/// REQ-OBJ-001, CR 202.3, CR 107.4c, CR 107.4h: `{2}{C}{S}` = 4.
#[test]
fn mv_202_3_generic_colorless_snow_mix() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{2}{C}{S}", ctx), 4);
    }
}

// ---------------------------------------------------------------------------------------------
// Case 2: CR 202.3a no mana cost versus {0}
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.3a: an object with no mana cost has mana value 0, off and on the stack.
#[test]
fn mv_202_3a_no_cost_is_zero_off_and_on_stack() {
    for ctx in contexts() {
        assert_eq!(
            mana_value(&ManaValueSubject::Plain { mana_cost: None }, ctx),
            0
        );
    }
}

/// REQ-OBJ-001, CR 202.3a, CR 107.4d: a `{0}` cost is a cost of zero mana and has mana value 0.
#[test]
fn mv_202_3a_zero_cost_is_zero() {
    let zero = cost("{0}");
    for ctx in contexts() {
        assert_eq!(
            mana_value(
                &ManaValueSubject::Plain {
                    mana_cost: Some(&zero)
                },
                ctx
            ),
            0
        );
    }
}

/// REQ-OBJ-001, CR 202.3a: "no mana cost" and `{0}` are different values of the subject even
/// though both have mana value 0.
#[test]
fn mv_202_3a_no_cost_and_zero_cost_are_distinct_subjects() {
    let zero = cost("{0}");
    let none = ManaValueSubject::Plain { mana_cost: None };
    let some = ManaValueSubject::Plain {
        mana_cost: Some(&zero),
    };
    assert_ne!(none, some);
    assert_eq!(ManaCostDSL::parse_optional("").expect("empty"), None);
    assert_eq!(
        ManaCostDSL::parse_optional("{0}").expect("zero"),
        Some(zero.clone())
    );
}

/// REQ-OBJ-001, CR 202.3a, CR 712.3, CR 712.6e: a MODAL double-faced back face with no mana
/// cost has its own characteristics, so its mana value is 0 whatever the front face costs.
#[test]
fn mv_202_3a_modal_dfc_back_face_without_cost_is_zero() {
    let fronts = ["{1}", "{2}{R}{G}", "{X}{X}{B}", "{7}{U}{U}"];
    for front in fronts {
        // The front face has a cost, but the modal back face is modelled with its own (absent)
        // cost; the 202.3b exception applies to NONMODAL back faces only.
        let front_cost = cost(front);
        for ctx in contexts() {
            assert_eq!(
                mana_value(&ManaValueSubject::Plain { mana_cost: None }, ctx),
                0,
                "front {front}"
            );
            assert!(
                mana_value(
                    &ManaValueSubject::NonmodalBackFace {
                        front_mana_cost: Some(&front_cost)
                    },
                    ctx
                ) > 0,
                "contrast: a nonmodal back face would use the front cost {front}"
            );
        }
    }
}

/// REQ-OBJ-001, CR 202.3a, CR 708.2a: a default face-down object has no mana cost, so MV 0.
#[test]
fn mv_202_3a_face_down_object_is_zero() {
    let face_down = ManaValueSubject::Plain { mana_cost: None };
    for ctx in contexts() {
        assert_eq!(mana_value(&face_down, ctx), 0);
    }
}

// ---------------------------------------------------------------------------------------------
// Case 3: CR 202.3b nonmodal double-faced back face
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.3b, CR 712.6b: a nonmodal back face uses its front face's mana cost.
#[test]
fn mv_202_3b_nonmodal_back_face_uses_front_cost() {
    let front = cost("{2}{R}{G}");
    for ctx in contexts() {
        assert_eq!(
            mana_value(
                &ManaValueSubject::NonmodalBackFace {
                    front_mana_cost: Some(&front)
                },
                ctx
            ),
            4
        );
    }
}

/// REQ-OBJ-001, CR 202.3b: the back face has no cost of its own, yet its MV is the front's (4),
/// not the 0 a plain no-cost object would have.
#[test]
fn mv_202_3b_back_face_without_own_cost_still_uses_front() {
    let front = cost("{2}{R}{G}");
    let own = ManaValueSubject::Plain { mana_cost: None };
    let back = ManaValueSubject::NonmodalBackFace {
        front_mana_cost: Some(&front),
    };
    let ctx = ManaValueContext::OffStack;
    assert_eq!(mana_value(&own, ctx), 0);
    assert_eq!(mana_value(&back, ctx), 4);
}

/// REQ-OBJ-001, CR 202.3b: a nonmodal back face whose front face has no mana cost has MV 0.
#[test]
fn mv_202_3b_back_face_of_no_cost_front_is_zero() {
    for ctx in contexts() {
        assert_eq!(
            mana_value(
                &ManaValueSubject::NonmodalBackFace {
                    front_mana_cost: None
                },
                ctx
            ),
            0
        );
    }
}

/// REQ-OBJ-001, CR 202.3b, CR 712.6d: a copy of a nonmodal back face has MV 0 in every context.
#[test]
fn mv_202_3b_copy_of_nonmodal_back_face_is_zero() {
    for ctx in contexts() {
        assert_eq!(
            mana_value(&ManaValueSubject::CopyOfNonmodalBackFace, ctx),
            0
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Case 4: CR 202.3c melded permanents
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.3c, CR 713.5c: a melded permanent's MV is the sum of its fronts' costs.
#[test]
fn mv_202_3c_melded_sums_front_faces() {
    let a = cost("{1}{W}{W}");
    let b = cost("{2}{B}{B}");
    for ctx in contexts() {
        assert_eq!(
            mana_value(
                &ManaValueSubject::Melded {
                    front_mana_costs: [Some(&a), Some(&b)]
                },
                ctx
            ),
            7
        );
        assert_eq!(
            mana_value(
                &ManaValueSubject::Melded {
                    front_mana_costs: [Some(&b), Some(&a)]
                },
                ctx
            ),
            7
        );
    }
}

/// REQ-OBJ-001, CR 202.3c: a front with no mana cost contributes 0 to the melded sum.
#[test]
fn mv_202_3c_melded_with_one_no_cost_front() {
    let a = cost("{1}{W}{W}");
    for ctx in contexts() {
        assert_eq!(
            mana_value(
                &ManaValueSubject::Melded {
                    front_mana_costs: [Some(&a), None]
                },
                ctx
            ),
            3
        );
        assert_eq!(
            mana_value(
                &ManaValueSubject::Melded {
                    front_mana_costs: [None, Some(&a)]
                },
                ctx
            ),
            3
        );
    }
}

/// REQ-OBJ-001, CR 202.3c: two no-cost fronts give a melded MV of 0.
#[test]
fn mv_202_3c_melded_with_no_cost_fronts_is_zero() {
    for ctx in contexts() {
        assert_eq!(
            mana_value(
                &ManaValueSubject::Melded {
                    front_mana_costs: [None, None]
                },
                ctx
            ),
            0
        );
    }
}

/// REQ-OBJ-001, CR 202.3c, CR 713.5c: a copy of a melded permanent has MV 0.
#[test]
fn mv_202_3c_copy_of_melded_is_zero() {
    for ctx in contexts() {
        assert_eq!(mana_value(&ManaValueSubject::CopyOfMelded, ctx), 0);
    }
}

// ---------------------------------------------------------------------------------------------
// Case 5: CR 202.3d split cards
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.3d, CR 709.4b: a split card not on the stack uses the combined costs.
#[test]
fn mv_202_3d_split_not_on_stack_combines_halves() {
    let l = cost("{1}{R}");
    let r = cost("{2}{U}");
    for ctx in contexts() {
        assert_eq!(split(Some(&l), Some(&r), SplitState::NotOnStack, ctx), 5);
    }
}

/// REQ-OBJ-001, CR 202.3d, CR 709.3b: on the stack only the chosen left half counts.
#[test]
fn mv_202_3d_split_on_stack_left_half_only() {
    let l = cost("{1}{R}");
    let r = cost("{2}{U}");
    for ctx in contexts() {
        assert_eq!(
            split(
                Some(&l),
                Some(&r),
                SplitState::OnStackHalf(SplitHalf::Left),
                ctx
            ),
            2
        );
    }
}

/// REQ-OBJ-001, CR 202.3d, CR 709.3b: on the stack only the chosen right half counts.
#[test]
fn mv_202_3d_split_on_stack_right_half_only() {
    let l = cost("{1}{R}");
    let r = cost("{2}{U}");
    for ctx in contexts() {
        assert_eq!(
            split(
                Some(&l),
                Some(&r),
                SplitState::OnStackHalf(SplitHalf::Right),
                ctx
            ),
            3
        );
    }
}

/// REQ-OBJ-001, CR 202.3d, CR 709.4d: a fused split spell on the stack uses the combined costs.
#[test]
fn mv_202_3d_split_fused_on_stack_combines_halves() {
    let l = cost("{1}{R}");
    let r = cost("{2}{U}");
    for ctx in contexts() {
        assert_eq!(split(Some(&l), Some(&r), SplitState::OnStackFused, ctx), 5);
    }
}

/// REQ-OBJ-001, CR 202.3d: a half with no mana cost contributes 0.
#[test]
fn mv_202_3d_split_half_without_cost_counts_zero() {
    let l = cost("{1}{R}");
    let r = cost("{2}{U}");
    for ctx in contexts() {
        assert_eq!(split(Some(&l), None, SplitState::NotOnStack, ctx), 2);
        assert_eq!(split(None, Some(&r), SplitState::NotOnStack, ctx), 3);
        assert_eq!(split(Some(&l), None, SplitState::OnStackFused, ctx), 2);
        assert_eq!(
            split(
                Some(&l),
                None,
                SplitState::OnStackHalf(SplitHalf::Right),
                ctx
            ),
            0
        );
        assert_eq!(
            split(
                None,
                Some(&r),
                SplitState::OnStackHalf(SplitHalf::Left),
                ctx
            ),
            0
        );
        assert_eq!(split(None, None, SplitState::NotOnStack, ctx), 0);
    }
}

// ---------------------------------------------------------------------------------------------
// Case 6: CR 202.3e variables
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.3e, CR 107.3g: X is 0 while the object is not on the stack.
#[test]
fn mv_202_3e_x_is_zero_off_stack() {
    assert_eq!(plain_mv("{X}{R}", ManaValueContext::OffStack), 1);
}

/// REQ-OBJ-001, CR 202.3e, CR 107.3a: on the stack X is the announced value.
#[test]
fn mv_202_3e_x_is_announced_value_on_stack() {
    assert_eq!(plain_mv("{X}{R}", on_stack(3, 0, 0)), 4);
}

/// REQ-OBJ-001, CR 202.3e: X announced as 0 on the stack counts 0.
#[test]
fn mv_202_3e_x_announced_zero_on_stack() {
    assert_eq!(plain_mv("{X}{R}", on_stack(0, 0, 0)), 1);
}

/// REQ-OBJ-001, CR 202.3e, CR 107.3p: each X counts, and Y follows the same rules as X.
#[test]
fn mv_202_3e_double_x_and_y_on_and_off_stack() {
    assert_eq!(plain_mv("{X}{X}{Y}", on_stack(2, 5, 0)), 9);
    assert_eq!(plain_mv("{X}{X}{Y}", ManaValueContext::OffStack), 0);
}

/// REQ-OBJ-001, CR 202.3e, CR 707.10: a copy of a spell on the stack copies the value of X, so
/// it is evaluated with the same `OnStack` values and has the same mana value.
#[test]
fn mv_202_3e_copy_on_stack_has_same_x() {
    let c = cost("{X}{U}{U}");
    let ctx = on_stack(4, 0, 0);
    let original = mana_value(
        &ManaValueSubject::Plain {
            mana_cost: Some(&c),
        },
        ctx,
    );
    let copy = mana_value(
        &ManaValueSubject::Plain {
            mana_cost: Some(&c),
        },
        ctx,
    );
    assert_eq!(original, 6);
    assert_eq!(original, copy);
}

/// REQ-OBJ-001, CR 202.3e, CR 202.3d, CR 107.3g: X inside a split half counts per split state.
#[test]
fn mv_202_3e_x_in_split_on_stack() {
    let l = cost("{X}{R}");
    let r = cost("{2}{U}");
    assert_eq!(
        split(
            Some(&l),
            Some(&r),
            SplitState::OnStackHalf(SplitHalf::Left),
            on_stack(3, 0, 0)
        ),
        4
    );
    assert_eq!(
        split(
            Some(&l),
            Some(&r),
            SplitState::OnStackFused,
            on_stack(3, 0, 0)
        ),
        7
    );
    assert_eq!(
        split(
            Some(&l),
            Some(&r),
            SplitState::NotOnStack,
            ManaValueContext::OffStack
        ),
        4
    );
    // CR 202.3e, CR 107.3g: not on the stack, X is 0 even with an on-stack context
    // (CodeRabbit r1 on PR #170).
    assert_eq!(
        split(
            Some(&l),
            Some(&r),
            SplitState::NotOnStack,
            on_stack(3, 0, 0)
        ),
        4
    );
}

/// REQ-OBJ-001, CR 202.3e, CR 202.3b: X in a nonmodal back face's front cost counts per context.
#[test]
fn mv_202_3e_x_in_nonmodal_back_face_front_cost() {
    let front = cost("{X}{G}");
    let back = ManaValueSubject::NonmodalBackFace {
        front_mana_cost: Some(&front),
    };
    assert_eq!(mana_value(&back, ManaValueContext::OffStack), 1);
    assert_eq!(mana_value(&back, on_stack(5, 0, 0)), 6);
}

// ---------------------------------------------------------------------------------------------
// Case 7: CR 202.3f hybrid
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.3f: each two-color hybrid symbol counts 1 (its largest component).
#[test]
fn mv_202_3f_hybrid_example_one_wu_wu_is_three() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{1}{W/U}{W/U}", ctx), 3);
    }
}

/// REQ-OBJ-001, CR 202.3f: each monocolored hybrid `{2/B}` counts 2.
#[test]
fn mv_202_3f_generic_hybrid_example_is_six() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{2/B}{2/B}{2/B}", ctx), 6);
    }
}

/// REQ-OBJ-001, CR 202.3f, CR 107.4e: `{2/W}` alone is 2.
#[test]
fn mv_202_3f_single_generic_hybrid_is_two() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{2/W}", ctx), 2);
    }
}

/// REQ-OBJ-001, CR 202.3f: a single `{W/U}` is 1.
#[test]
fn mv_202_3f_single_hybrid_is_one() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{W/U}", ctx), 1);
    }
}

// ---------------------------------------------------------------------------------------------
// Case 8: CR 202.3g Phyrexian
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.3g: each Phyrexian symbol contributes 1.
#[test]
fn mv_202_3g_phyrexian_example_is_three() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{1}{W/P}{W/P}", ctx), 3);
    }
}

/// REQ-OBJ-001, CR 202.3g, CR 107.4f: a hybrid Phyrexian symbol contributes 1.
#[test]
fn mv_202_3g_hybrid_phyrexian_is_one() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{W/U/P}", ctx), 1);
    }
}

/// REQ-OBJ-001, CR 202.3g: a single `{G/P}` is 1.
#[test]
fn mv_202_3g_single_phyrexian_is_one() {
    for ctx in contexts() {
        assert_eq!(plain_mv("{G/P}", ctx), 1);
    }
}

// ---------------------------------------------------------------------------------------------
// Case 9: symbol_mana_value for every variant
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.3, CR 107.4d: `Generic(n)` = n in both contexts.
#[test]
fn symbol_mv_202_3_generic_is_its_amount() {
    for n in [0u32, 1, 2, 7, 16, 1_000_000] {
        for ctx in contexts() {
            assert_eq!(symbol_mana_value(ManaSymbol::Generic(n), ctx), i64::from(n));
        }
    }
}

/// REQ-OBJ-001, CR 202.3: every colored symbol is 1 in both contexts.
#[test]
fn symbol_mv_202_3_colored_is_one() {
    for c in Color::ALL {
        for ctx in contexts() {
            assert_eq!(symbol_mana_value(ManaSymbol::Colored(c), ctx), 1);
        }
    }
}

/// REQ-OBJ-001, CR 202.3, CR 107.4c, CR 107.4h: `{C}` and `{S}` are 1 in both contexts.
#[test]
fn symbol_mv_202_3_colorless_and_snow_are_one() {
    for ctx in contexts() {
        assert_eq!(symbol_mana_value(ManaSymbol::Colorless, ctx), 1);
        assert_eq!(symbol_mana_value(ManaSymbol::Snow, ctx), 1);
    }
}

/// REQ-OBJ-001, CR 202.3f: every two-color hybrid is 1 in both contexts.
#[test]
fn symbol_mv_202_3f_hybrid_is_one() {
    for a in Color::ALL {
        for b in Color::ALL {
            if a == b {
                continue;
            }
            for ctx in contexts() {
                assert_eq!(symbol_mana_value(ManaSymbol::Hybrid(a, b), ctx), 1);
            }
        }
    }
}

/// REQ-OBJ-001, CR 202.3f: `GenericHybrid(n, c)` is n (the largest component) in both contexts.
#[test]
fn symbol_mv_202_3f_generic_hybrid_is_its_amount() {
    for c in Color::ALL {
        for n in [1u32, 2, 3, 10] {
            for ctx in contexts() {
                assert_eq!(
                    symbol_mana_value(ManaSymbol::GenericHybrid(n, c), ctx),
                    i64::from(n)
                );
            }
        }
    }
}

/// REQ-OBJ-001, CR 202.3g: every Phyrexian and hybrid Phyrexian symbol is 1 in both contexts.
#[test]
fn symbol_mv_202_3g_phyrexian_is_one() {
    for a in Color::ALL {
        for ctx in contexts() {
            assert_eq!(symbol_mana_value(ManaSymbol::Phyrexian(a), ctx), 1);
        }
        for b in Color::ALL {
            if a == b {
                continue;
            }
            for ctx in contexts() {
                assert_eq!(symbol_mana_value(ManaSymbol::HybridPhyrexian(a, b), ctx), 1);
            }
        }
    }
}

/// REQ-OBJ-001, CR 202.3e, CR 107.3g: every variable symbol is 0 off the stack.
#[test]
fn symbol_mv_202_3e_variables_are_zero_off_stack() {
    for v in Variable::ALL {
        assert_eq!(
            symbol_mana_value(ManaSymbol::Variable(v), ManaValueContext::OffStack),
            0
        );
    }
}

/// REQ-OBJ-001, CR 202.3e, CR 107.3a, CR 107.3p: on the stack X and Y are their announced
/// values. (Nothing is asserted about Z's value: no CR rule defines it.)
#[test]
fn symbol_mv_202_3e_x_and_y_on_stack_are_announced() {
    for (x, y) in [(0u32, 0u32), (1, 0), (0, 1), (3, 5), (9, 2)] {
        let ctx = on_stack(x, y, 11);
        assert_eq!(
            symbol_mana_value(ManaSymbol::Variable(Variable::X), ctx),
            i64::from(x)
        );
        assert_eq!(
            symbol_mana_value(ManaSymbol::Variable(Variable::Y), ctx),
            i64::from(y)
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Case 10: deterministic property tests
// ---------------------------------------------------------------------------------------------

/// A small 64-bit linear congruential generator (integer math only, seeded, R3/R7).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn color(&mut self) -> Color {
        let i = usize::try_from(self.below(5)).expect("index fits");
        Color::ALL[i]
    }

    fn two_colors(&mut self) -> (Color, Color) {
        let a = self.color();
        let mut b = self.color();
        while b == a {
            b = self.color();
        }
        (a, b)
    }

    fn symbol(&mut self) -> ManaSymbol {
        match self.below(9) {
            0 => ManaSymbol::Generic(u32::try_from(self.below(20)).expect("fits")),
            1 => ManaSymbol::Colored(self.color()),
            2 => ManaSymbol::Colorless,
            3 => ManaSymbol::Snow,
            4 => ManaSymbol::Variable(Variable::ALL[usize::try_from(self.below(3)).expect("fits")]),
            5 => {
                let (a, b) = self.two_colors();
                ManaSymbol::Hybrid(a, b)
            }
            6 => ManaSymbol::GenericHybrid(
                u32::try_from(1 + self.below(9)).expect("fits"),
                self.color(),
            ),
            7 => ManaSymbol::Phyrexian(self.color()),
            _ => {
                let (a, b) = self.two_colors();
                ManaSymbol::HybridPhyrexian(a, b)
            }
        }
    }

    fn cost(&mut self) -> ManaCostDSL {
        let len = 1 + self.below(8);
        let symbols: Vec<ManaSymbol> = (0..len).map(|_| self.symbol()).collect();
        ManaCostDSL::new(symbols).expect("generated symbols are valid")
    }
}

/// The fixed list of 200 generated costs.
fn generated_costs() -> Vec<ManaCostDSL> {
    let mut g = Lcg(0x0F1E_2D3C_4B5A_6978);
    (0..200).map(|_| g.cost()).collect()
}

fn symbol_sum(c: &ManaCostDSL, ctx: ManaValueContext) -> i64 {
    c.symbols().iter().map(|s| symbol_mana_value(*s, ctx)).sum()
}

/// REQ-OBJ-001, CR 202.3: the MV of a plain cost is the sum of its symbols' MVs, and never < 0.
#[test]
fn prop_202_3_plain_mv_is_sum_of_symbols_and_non_negative() {
    let costs = generated_costs();
    assert_eq!(costs.len(), 200);
    for c in &costs {
        for ctx in contexts() {
            let mv = mana_value(&ManaValueSubject::Plain { mana_cost: Some(c) }, ctx);
            assert_eq!(mv, symbol_sum(c, ctx), "cost {c:?} ctx {ctx:?}");
            assert!(mv >= 0, "cost {c:?} ctx {ctx:?}");
        }
    }
}

/// REQ-OBJ-001, CR 202.3: every subject kind built from generated costs has MV >= 0.
#[test]
fn prop_202_3_every_subject_is_non_negative() {
    let costs = generated_costs();
    for pair in costs.chunks(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let subjects = [
            ManaValueSubject::Plain { mana_cost: Some(a) },
            ManaValueSubject::NonmodalBackFace {
                front_mana_cost: Some(a),
            },
            ManaValueSubject::Melded {
                front_mana_costs: [Some(a), Some(b)],
            },
            ManaValueSubject::Split {
                left: Some(a),
                right: Some(b),
                state: SplitState::OnStackHalf(SplitHalf::Right),
            },
        ];
        for s in &subjects {
            for ctx in contexts() {
                assert!(mana_value(s, ctx) >= 0, "{s:?} {ctx:?}");
            }
        }
    }
}

/// REQ-OBJ-001, CR 202.3b, CR 202.3c: copies of nonmodal back faces and melded permanents are
/// always 0, whatever the context.
#[test]
fn prop_202_3b_202_3c_copies_always_zero() {
    let mut g = Lcg(0x1234_5678_9ABC_DEF0);
    for _ in 0..200 {
        let x = u32::try_from(g.below(1_000)).expect("fits");
        let y = u32::try_from(g.below(1_000)).expect("fits");
        let z = u32::try_from(g.below(1_000)).expect("fits");
        for ctx in [ManaValueContext::OffStack, on_stack(x, y, z)] {
            assert_eq!(
                mana_value(&ManaValueSubject::CopyOfNonmodalBackFace, ctx),
                0
            );
            assert_eq!(mana_value(&ManaValueSubject::CopyOfMelded, ctx), 0);
        }
    }
}

/// REQ-OBJ-001, CR 202.3d, CR 202.3e, CR 107.3g: swapping the halves of a NotOnStack or fused
/// split leaves the MV unchanged. A NotOnStack split is the sum of both halves evaluated off the
/// stack whatever context is supplied (the state decides); a fused split is the sum of both
/// halves in the supplied context.
#[test]
fn prop_202_3d_split_half_order_irrelevant_when_combined() {
    let costs = generated_costs();
    for pair in costs.chunks(2) {
        let (a, b) = (&pair[0], &pair[1]);
        for state in [SplitState::NotOnStack, SplitState::OnStackFused] {
            for ctx in contexts() {
                let ab = split(Some(a), Some(b), state, ctx);
                let ba = split(Some(b), Some(a), state, ctx);
                assert_eq!(ab, ba, "{a:?} {b:?} {state:?} {ctx:?}");
                let expected = if state == SplitState::NotOnStack {
                    symbol_sum(a, ManaValueContext::OffStack)
                        + symbol_sum(b, ManaValueContext::OffStack)
                } else {
                    symbol_sum(a, ctx) + symbol_sum(b, ctx)
                };
                assert_eq!(ab, expected, "{a:?} {b:?} {state:?} {ctx:?}");
            }
        }
    }
}

/// REQ-OBJ-001, CR 202.3e, CR 107.3g: with `OffStack`, X, Y and Z never contribute.
#[test]
fn prop_202_3e_variables_never_contribute_off_stack() {
    let costs = generated_costs();
    for c in &costs {
        let without_variables: i64 = c
            .symbols()
            .iter()
            .filter(|s| !matches!(s, ManaSymbol::Variable(_)))
            .map(|s| symbol_mana_value(*s, ManaValueContext::OffStack))
            .sum();
        assert_eq!(
            mana_value(
                &ManaValueSubject::Plain { mana_cost: Some(c) },
                ManaValueContext::OffStack
            ),
            without_variables,
            "{c:?}"
        );
    }
}
