// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the NORMAL, LEVELER, SAGA, CLASS and CASE layouts of
//! `fineprint_core::layouts`, and for gating data (REQ-OBJ-002). Black-box: written from the
//! work-item brief only.
//!
//! These layouts have only printed characteristics in every zone (CR 109.3); a leveler has its
//! uppermost power and toughness off the battlefield and below its first level band (CR 711.5,
//! 711.6). Level bands, chapters, class levels and solved abilities are abilities gated by
//! counters or designations (CR 711.2a-b, 715.2, 717.2a-b, 720.3b), exposed as data only.

#[path = "spec_layouts_support.rs"]
mod support;

use fineprint_core::layouts::{
    CaseLayout, ClassLayout, Gating, Layout, LayoutError, LevelerLayout, NormalLayout, SagaLayout,
    base_characteristics, gating, layout_for, mana_value,
};
use fineprint_core::objects::{Characteristics, ManaValueContext};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{ColorBitmask, LayoutBehavior};
use support::{
    Cast, IN_SCOPE, St, ability, assert_chars, case_main, chars, class_main, contexts, def,
    leveler_main, normal_main, on_stack, printed, query, saga_main, sample, set, some_cost, text,
};

/// The five single-face layouts and their objects.
fn single_face() -> Vec<(LayoutBehavior, &'static dyn Layout)> {
    vec![
        (LayoutBehavior::Normal, &NormalLayout),
        (LayoutBehavior::Leveler, &LevelerLayout),
        (LayoutBehavior::Saga, &SagaLayout),
        (LayoutBehavior::Class, &ClassLayout),
        (LayoutBehavior::Case, &CaseLayout),
    ]
}

fn expected_leveler() -> Characteristics {
    Characteristics {
        names: set(&["test.leveler"]),
        mana_cost: some_cost("{1}{W}"),
        color_indicator: None,
        colors: ColorBitmask::WHITE,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Human"]),
        rules_text: text(&["test.leveler"]),
        abilities: vec![
            ability("level_up", 1),
            ability("band_2_4", 2),
            ability("band_5", 5),
        ],
        power: Some(printed(1)),
        toughness: Some(printed(1)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// REQ-OBJ-002, CR 109.3, 202.3, 202.3e: NORMAL has its Main face's characteristics in every
/// zone with every cast choice and status, field by field; mana value from the Main cost with
/// the context passed.
#[test]
fn normal_109_3_main_in_every_zone() {
    let d = sample(LayoutBehavior::Normal);
    let v = support::vocab();
    let expected = Characteristics {
        names: set(&["test.normal_main"]),
        mana_cost: some_cost("{X}{1}{G}"),
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Human"]),
        rules_text: text(&["test.normal_main"]),
        abilities: vec![ability("test.normal_main", 1)],
        power: Some(printed(2)),
        toughness: Some(printed(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    };
    assert_eq!(expected, chars(&normal_main()));
    for zone in ZoneKind::ALL {
        for cast in Cast::ALL {
            for s in St::all() {
                let q = query(zone, cast, s);
                let got = base_characteristics(&d, &q, v).expect("main");
                assert_chars(&got, &expected, &format!("{zone:?} {cast:?} {s:?}"));
                assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(2));
                // CR 202.3e, 107.3g: X counts on the stack only.
                let with_x = if zone == ZoneKind::Stack { 6 } else { 2 };
                assert_eq!(mana_value(&d, &q, on_stack(4, 0, 0), v), Ok(with_x));
            }
        }
    }
}

/// REQ-OBJ-002, CR 711.5, 711.6: a leveler has its printed (uppermost) power and toughness and
/// all its printed abilities in every zone, the battlefield included; no band is applied here.
#[test]
fn leveler_711_6_uppermost_pt_in_every_zone() {
    let d = sample(LayoutBehavior::Leveler);
    let v = support::vocab();
    for zone in ZoneKind::ALL {
        for cast in Cast::ALL {
            for s in St::all() {
                let q = query(zone, cast, s);
                let got = base_characteristics(&d, &q, v).expect("main");
                assert_chars(
                    &got,
                    &expected_leveler(),
                    &format!("{zone:?} {cast:?} {s:?}"),
                );
                for ctx in contexts() {
                    assert_eq!(mana_value(&d, &q, ctx, v), Ok(2));
                }
            }
        }
    }
}

/// REQ-OBJ-002, CR 109.3, 715.2, 717.2a, 720.3b: SAGA, CLASS and CASE have their Main face's
/// characteristics in every zone with every cast choice and status; mana value from the Main
/// cost (the CASE sample has `{X}`, counted on the stack only: CR 202.3e, 107.3g).
#[test]
fn saga_class_case_715_717_720_main_in_every_zone() {
    let v = support::vocab();
    let cases = [
        (LayoutBehavior::Saga, saga_main(), "Saga", 3, 3),
        (LayoutBehavior::Class, class_main(), "Class", 1, 1),
        (LayoutBehavior::Case, case_main(), "Case", 4, 9),
    ];
    for (layout, main, subtype, mv_off, mv_x5) in cases {
        let d = sample(layout);
        let expected = chars(&main);
        assert_eq!(expected.card_types, set(&["Enchantment"]));
        assert_eq!(expected.subtypes, set(&[subtype]));
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                for s in St::all() {
                    let q = query(zone, cast, s);
                    let got = base_characteristics(&d, &q, v).expect("main");
                    assert_chars(&got, &expected, &format!("{layout:?} {zone:?} {cast:?}"));
                    assert_eq!(
                        mana_value(&d, &q, ManaValueContext::OffStack, v),
                        Ok(mv_off)
                    );
                    let with_x = if zone == ZoneKind::Stack {
                        mv_x5
                    } else {
                        mv_off
                    };
                    assert_eq!(mana_value(&d, &q, on_stack(5, 0, 0), v), Ok(with_x));
                }
            }
        }
    }
}

/// REQ-OBJ-002, CR 711.2a, 711.2b, 711.6: LEVELER gating data is the Main face's printed
/// (uppermost) power and toughness and its abilities in definition order.
#[test]
fn gating_711_2_leveler_data() {
    let d = sample(LayoutBehavior::Leveler);
    let want = Gating::Leveler {
        uppermost_power: Some(printed(1)),
        uppermost_toughness: Some(printed(1)),
        abilities: vec![
            ability("level_up", 1),
            ability("band_2_4", 2),
            ability("band_5", 5),
        ],
    };
    assert_eq!(gating(&d), Ok(Some(want.clone())));
    assert_eq!(LevelerLayout.gating(&d), Ok(Some(want.clone())));
    assert_eq!(
        layout_for(LayoutBehavior::Leveler)
            .expect("supported")
            .gating(&d),
        Ok(Some(want))
    );
    // A leveler without printed P/T reports None for both.
    let mut main = leveler_main();
    main.power = None;
    main.toughness = None;
    main.card_types = support::strings(&["Creature"]);
    let d = def(LayoutBehavior::Leveler, vec![main]);
    assert_eq!(
        gating(&d),
        Ok(Some(Gating::Leveler {
            uppermost_power: None,
            uppermost_toughness: None,
            abilities: vec![
                ability("level_up", 1),
                ability("band_2_4", 2),
                ability("band_5", 5),
            ],
        }))
    );
}

/// REQ-OBJ-002, CR 715.2, 717.2a, 717.2b, 720.3b: SAGA, CLASS and CASE gating data is the Main
/// face's abilities in definition order.
#[test]
fn gating_715_717_720_saga_class_case_data() {
    let saga = sample(LayoutBehavior::Saga);
    let want_saga = Gating::Saga {
        abilities: vec![
            ability("chapter", 1),
            ability("chapter", 2),
            ability("chapter", 3),
        ],
    };
    assert_eq!(gating(&saga), Ok(Some(want_saga.clone())));
    assert_eq!(SagaLayout.gating(&saga), Ok(Some(want_saga)));

    let class = sample(LayoutBehavior::Class);
    let want_class = Gating::Class {
        abilities: vec![ability("level_1", 1), ability("level_2", 2)],
    };
    assert_eq!(gating(&class), Ok(Some(want_class.clone())));
    assert_eq!(ClassLayout.gating(&class), Ok(Some(want_class)));

    let case = sample(LayoutBehavior::Case);
    let want_case = Gating::Case {
        abilities: vec![ability("to_solve", 1), ability("solved", 2)],
    };
    assert_eq!(gating(&case), Ok(Some(want_case.clone())));
    assert_eq!(CaseLayout.gating(&case), Ok(Some(want_case)));

    // Order is definition order, not sorted.
    let mut main = saga_main();
    main.abilities = vec![ability("z", 3), ability("a", 1), ability("m", 2)];
    let d = def(LayoutBehavior::Saga, vec![main]);
    assert_eq!(
        gating(&d),
        Ok(Some(Gating::Saga {
            abilities: vec![ability("z", 3), ability("a", 1), ability("m", 2)],
        }))
    );
    // No abilities: empty list, still Some.
    let mut main = class_main();
    main.abilities = Vec::new();
    let d = def(LayoutBehavior::Class, vec![main]);
    assert_eq!(
        gating(&d),
        Ok(Some(Gating::Class {
            abilities: Vec::new()
        }))
    );
}

/// REQ-OBJ-002, CR 709.4, 710.2, 712.6a, 716.4, 719.4, 721.4: the other eight in-scope layouts have no
/// gating data.
#[test]
fn gating_none_for_other_eight_layouts() {
    let gated = [
        LayoutBehavior::Leveler,
        LayoutBehavior::Saga,
        LayoutBehavior::Class,
        LayoutBehavior::Case,
    ];
    let others: Vec<LayoutBehavior> = IN_SCOPE
        .into_iter()
        .filter(|l| !gated.contains(l))
        .collect();
    assert_eq!(others.len(), 8);
    for layout in others {
        let d = sample(layout);
        assert_eq!(gating(&d), Ok(None), "{layout:?}");
        assert_eq!(
            layout_for(layout).expect("supported").gating(&d),
            Ok(None),
            "{layout:?}"
        );
    }
}

/// REQ-OBJ-002, CR 711.2a, 715.2, 717.2a, 720.3b: `layout_for(x).gating(def)` with a definition of
/// another layout is `LayoutMismatch`; a gated layout with wrong faces is `BadFaces`.
#[test]
fn gating_mismatch_and_bad_faces() {
    for (layout, obj) in single_face() {
        assert_eq!(obj.behavior(), layout);
        for other in IN_SCOPE.into_iter().filter(|o| *o != layout) {
            let d = sample(other);
            assert_eq!(
                obj.gating(&d),
                Err(LayoutError::LayoutMismatch {
                    expected: layout,
                    found: other,
                }),
                "{layout:?} given {other:?}"
            );
        }
        // Two Main faces.
        let d = def(layout, vec![leveler_main(), leveler_main()]);
        assert_eq!(gating(&d), Err(LayoutError::BadFaces { layout }));
        assert_eq!(obj.gating(&d), Err(LayoutError::BadFaces { layout }));
        // No faces.
        let d = def(layout, Vec::new());
        assert_eq!(gating(&d), Err(LayoutError::BadFaces { layout }));
    }
}
