// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the DFC_NONMODAL and DFC_MODAL layouts of `fineprint_core::layouts`
//! (REQ-OBJ-002). Black-box: written from the work-item brief only.
//!
//! Front face outside the stack and battlefield (CR 712.6a); nonmodal: front on the stack unless
//! cast transformed (CR 712.6b, 712.8a), the face up on the battlefield (CR 712.6c, 712.6d),
//! mana value from the front face's cost and 0 for a copy of a back face (CR 202.3a, 202.3b);
//! modal: the chosen face (CR 712.6e, 712.8b), mana value from that face's own cost (CR 202.3a).

#[path = "spec_layouts_support.rs"]
mod support;

use fineprint_core::layouts::{
    DfcModalLayout, DfcNonmodalLayout, Layout, LayoutError, base_characteristics, layout_for,
    mana_value,
};
use fineprint_core::objects::{Characteristics, ManaValueContext};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{CardDefinitionDSL, ColorBitmask, FaceKind, LayoutBehavior};
use support::{
    Cast, St, ability, assert_chars, chars, contexts, cost, def, dfc_back, dfc_front,
    hidden_or_other_zones, mdfc_back, mdfc_front, on_stack, printed, query, sample, set, some_cost,
    text,
};

fn nonmodal() -> CardDefinitionDSL {
    sample(LayoutBehavior::DfcNonmodal)
}

fn modal() -> CardDefinitionDSL {
    sample(LayoutBehavior::DfcModal)
}

fn expected_front() -> Characteristics {
    Characteristics {
        names: set(&["test.dfc_front"]),
        mana_cost: some_cost("{3}{G}"),
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Human"]),
        rules_text: text(&["test.dfc_front"]),
        abilities: vec![ability("test.dfc_front", 1)],
        power: Some(printed(2)),
        toughness: Some(printed(3)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn expected_back() -> Characteristics {
    Characteristics {
        names: set(&["test.dfc_back"]),
        mana_cost: None,
        color_indicator: Some(ColorBitmask::RED),
        colors: ColorBitmask::RED,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Wizard"]),
        rules_text: text(&["test.dfc_back"]),
        abilities: vec![ability("test.dfc_back", 1)],
        power: Some(printed(5)),
        toughness: Some(printed(5)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn expected_modal_back() -> Characteristics {
    Characteristics {
        names: set(&["test.mdfc_back"]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: set(&[]),
        card_types: set(&["Land"]),
        subtypes: set(&["Forest"]),
        rules_text: text(&["test.mdfc_back"]),
        abilities: vec![ability("test.mdfc_back", 1)],
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn st(back_up: bool, copying_back_face: bool, flipped: bool) -> St {
    St {
        flipped,
        back_up,
        copying_back_face,
    }
}

// ---------------------------------------------------------------------------------------------
// DFC_NONMODAL
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 712.6a, 202.3, 202.3b: outside the stack and battlefield a nonmodal DFC has
/// only its front face, whatever cast choice or status is passed; mana value from the front's
/// cost, `copying_back_face` included (CR 202.3b covers only a spell or permanent).
#[test]
fn dfc_nonmodal_712_6a_front_outside_stack_and_battlefield() {
    let d = nonmodal();
    let v = support::vocab();
    assert_eq!(expected_front(), chars(&dfc_front()));
    for zone in hidden_or_other_zones() {
        for cast in Cast::ALL {
            for s in St::all() {
                let q = query(zone, cast, s);
                let got = base_characteristics(&d, &q, v).expect("front");
                assert_chars(&got, &expected_front(), &format!("{zone:?} {cast:?} {s:?}"));
                for ctx in contexts() {
                    assert_eq!(mana_value(&d, &q, ctx, v), Ok(4), "{zone:?} {s:?}");
                }
            }
        }
    }
}

/// REQ-OBJ-002, CR 712.6b, 712.8a, 202.3b: on the stack, cast None gives the front face; cast
/// transformed gives only the back face, with the mana value of the front face's cost.
#[test]
fn dfc_nonmodal_712_6b_stack_front_or_transformed_back() {
    let d = nonmodal();
    let v = support::vocab();
    for s in [St::DEFAULT, St::back_up(), St::flipped()] {
        let front = query(ZoneKind::Stack, Cast::None, s);
        assert_chars(
            &base_characteristics(&d, &front, v).expect("front"),
            &expected_front(),
            &format!("{s:?}"),
        );
        assert_eq!(mana_value(&d, &front, on_stack(3, 0, 0), v), Ok(4));
        let back = query(ZoneKind::Stack, Cast::Transformed, s);
        assert_chars(
            &base_characteristics(&d, &back, v).expect("back"),
            &expected_back(),
            &format!("{s:?}"),
        );
        // CR 202.3b: the back face (no cost of its own) has the front's mana value.
        assert_eq!(mana_value(&d, &back, on_stack(3, 0, 0), v), Ok(4));
        assert_eq!(mana_value(&d, &back, ManaValueContext::OffStack, v), Ok(4));
    }
}

/// REQ-OBJ-002, CR 712.6b, 712.8a: on the stack every cast choice but None and Transformed is
/// `InvalidCast { DfcNonmodal, Stack }`.
#[test]
fn dfc_nonmodal_712_6b_other_casts_on_stack_invalid() {
    let d = nonmodal();
    let v = support::vocab();
    let bad = Cast::all_except(&[Cast::None, Cast::Transformed]);
    assert_eq!(bad.len(), 8);
    assert!(bad.contains(&Cast::ModalBack));
    let err = LayoutError::InvalidCast {
        layout: LayoutBehavior::DfcNonmodal,
        zone: ZoneKind::Stack,
    };
    for cast in bad {
        let q = query(ZoneKind::Stack, cast, St::DEFAULT);
        assert_eq!(
            base_characteristics(&d, &q, v),
            Err(err.clone()),
            "{cast:?}"
        );
        assert_eq!(
            mana_value(&d, &q, on_stack(1, 0, 0), v),
            Err(err.clone()),
            "{cast:?}"
        );
    }
}

/// REQ-OBJ-002, CR 712.6c, 712.6d, 202.3b: on the battlefield the face up decides (cast
/// ignored); a back-face-up permanent's mana value is the front cost's.
#[test]
fn dfc_nonmodal_712_6d_battlefield_face_up_cast_ignored() {
    let d = nonmodal();
    let v = support::vocab();
    for cast in Cast::ALL {
        for flipped in [false, true] {
            let fq = query(ZoneKind::Battlefield, cast, st(false, false, flipped));
            assert_chars(
                &base_characteristics(&d, &fq, v).expect("front up"),
                &expected_front(),
                &format!("{cast:?}"),
            );
            assert_eq!(mana_value(&d, &fq, ManaValueContext::OffStack, v), Ok(4));
            let bq = query(ZoneKind::Battlefield, cast, st(true, false, flipped));
            assert_chars(
                &base_characteristics(&d, &bq, v).expect("back up"),
                &expected_back(),
                &format!("{cast:?}"),
            );
            assert_eq!(mana_value(&d, &bq, ManaValueContext::OffStack, v), Ok(4));
        }
    }
}

/// REQ-OBJ-002, CR 202.3b, 712.6d, 712.6a: `copying_back_face` makes the mana value 0 for a
/// spell on the stack or a permanent on the battlefield only; in Library, Hand, Graveyard, Exile
/// and Command the flag is ignored and the front face's cost applies. The characteristics still
/// follow the selection table.
#[test]
fn dfc_nonmodal_202_3b_copy_of_back_face_mana_value_zero() {
    let d = nonmodal();
    let v = support::vocab();
    for zone in ZoneKind::ALL {
        for back_up in [false, true] {
            let cast = if zone == ZoneKind::Stack && back_up {
                Cast::Transformed
            } else {
                Cast::None
            };
            let q = query(zone, cast, st(back_up, true, false));
            let in_play = zone == ZoneKind::Stack || zone == ZoneKind::Battlefield;
            let want_mv = if in_play { 0 } else { 4 };
            for ctx in contexts() {
                assert_eq!(
                    mana_value(&d, &q, ctx, v),
                    Ok(want_mv),
                    "{zone:?} {back_up}"
                );
                assert_eq!(
                    DfcNonmodalLayout.mana_value(&d, &q, ctx, v),
                    Ok(want_mv),
                    "{zone:?} {back_up}"
                );
            }
            let shows_back = back_up && (zone == ZoneKind::Stack || zone == ZoneKind::Battlefield);
            let want = if shows_back {
                expected_back()
            } else {
                expected_front()
            };
            assert_chars(
                &base_characteristics(&d, &q, v).expect("chars"),
                &want,
                &format!("{zone:?} {back_up}"),
            );
        }
    }
}

/// REQ-OBJ-002, CR 202.3b, 712.6b, 712.6d: a back face with its own different cost still has
/// the front face's mana value, while its characteristics carry its own cost.
#[test]
fn dfc_nonmodal_202_3b_back_with_different_cost_uses_front() {
    let v = support::vocab();
    let mut back = dfc_back();
    back.mana_cost = some_cost("{9}{R}");
    let d = def(LayoutBehavior::DfcNonmodal, vec![dfc_front(), back]);
    let on_bf = query(ZoneKind::Battlefield, Cast::None, St::back_up());
    let got = base_characteristics(&d, &on_bf, v).expect("back up");
    assert_eq!(got.mana_cost, some_cost("{9}{R}"));
    assert_eq!(got.colors, ColorBitmask::RED);
    assert_eq!(mana_value(&d, &on_bf, ManaValueContext::OffStack, v), Ok(4));
    let transformed = query(ZoneKind::Stack, Cast::Transformed, St::DEFAULT);
    assert_eq!(mana_value(&d, &transformed, on_stack(5, 0, 0), v), Ok(4));
}

/// REQ-OBJ-002, CR 202.3a, 202.3b: a front face with no cost gives mana value 0 for either face
/// (the back's own cost never counts); a front `{0}` also gives 0 but keeps `Some({0})` as its
/// cost, unlike no cost.
#[test]
fn dfc_nonmodal_202_3a_front_no_cost_versus_zero() {
    let v = support::vocab();
    let mut back = dfc_back();
    back.mana_cost = some_cost("{5}");
    for (front_cost, want_cost) in [(None, None), (some_cost("{0}"), Some(cost("{0}")))] {
        let mut front = dfc_front();
        front.mana_cost = front_cost;
        let d = def(LayoutBehavior::DfcNonmodal, vec![front, back.clone()]);
        for q in [
            query(ZoneKind::Hand, Cast::None, St::DEFAULT),
            query(ZoneKind::Stack, Cast::None, St::DEFAULT),
            query(ZoneKind::Stack, Cast::Transformed, St::DEFAULT),
            query(ZoneKind::Battlefield, Cast::None, St::DEFAULT),
            query(ZoneKind::Battlefield, Cast::None, St::back_up()),
        ] {
            assert_eq!(mana_value(&d, &q, on_stack(2, 0, 0), v), Ok(0));
        }
        let front_chars =
            base_characteristics(&d, &query(ZoneKind::Library, Cast::None, St::DEFAULT), v)
                .expect("front");
        assert_eq!(front_chars.mana_cost, want_cost);
    }
}

/// REQ-OBJ-002, CR 202.3b, 712.6b: `{X}` in the front cost counts the announced value for a
/// transformed spell on the stack.
#[test]
fn dfc_nonmodal_202_3b_front_x_counts_on_stack_for_back() {
    let v = support::vocab();
    let mut front = dfc_front();
    front.mana_cost = some_cost("{X}{G}");
    let d = def(LayoutBehavior::DfcNonmodal, vec![front, dfc_back()]);
    let q = query(ZoneKind::Stack, Cast::Transformed, St::DEFAULT);
    assert_eq!(mana_value(&d, &q, on_stack(2, 0, 0), v), Ok(3));
    assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(1));
}

// ---------------------------------------------------------------------------------------------
// DFC_MODAL
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 712.6a: outside the stack and battlefield a modal DFC has only its front
/// face, whatever cast choice or status is passed.
#[test]
fn dfc_modal_712_6a_front_outside_stack_and_battlefield() {
    let d = modal();
    let v = support::vocab();
    let front = chars(&mdfc_front());
    assert_eq!(front.names, set(&["test.mdfc_front"]));
    assert_eq!(front.card_types, set(&["Sorcery"]));
    for zone in hidden_or_other_zones() {
        for cast in Cast::ALL {
            for s in St::all() {
                let q = query(zone, cast, s);
                let got = base_characteristics(&d, &q, v).expect("front");
                assert_chars(&got, &front, &format!("{zone:?} {cast:?} {s:?}"));
                assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(3));
            }
        }
    }
}

/// REQ-OBJ-002, CR 712.6e, 712.8b, 202.3a: on the stack the chosen face; mana value from that
/// face's own cost (the land back face has none: 0, never the front's cost).
#[test]
fn dfc_modal_712_8b_stack_chosen_face() {
    let d = modal();
    let v = support::vocab();
    for s in St::all() {
        let fq = query(ZoneKind::Stack, Cast::ModalFront, s);
        assert_eq!(base_characteristics(&d, &fq, v), Ok(chars(&mdfc_front())));
        assert_eq!(mana_value(&d, &fq, on_stack(1, 0, 0), v), Ok(3));
        let bq = query(ZoneKind::Stack, Cast::ModalBack, s);
        assert_chars(
            &base_characteristics(&d, &bq, v).expect("back"),
            &expected_modal_back(),
            &format!("{s:?}"),
        );
        // copying_back_face is ignored for modal DFCs.
        assert_eq!(mana_value(&d, &bq, on_stack(1, 0, 0), v), Ok(0));
        assert_eq!(mana_value(&d, &fq, ManaValueContext::OffStack, v), Ok(3));
    }
}

/// REQ-OBJ-002, CR 712.8b: on the stack a modal DFC must name a face; None, Transformed and
/// every other cast choice is `InvalidCast { DfcModal, Stack }`.
#[test]
fn dfc_modal_712_8b_other_casts_on_stack_invalid() {
    let d = modal();
    let v = support::vocab();
    let bad = Cast::all_except(&[Cast::ModalFront, Cast::ModalBack]);
    assert_eq!(bad.len(), 8);
    assert!(bad.contains(&Cast::None));
    assert!(bad.contains(&Cast::Transformed));
    let err = LayoutError::InvalidCast {
        layout: LayoutBehavior::DfcModal,
        zone: ZoneKind::Stack,
    };
    for cast in bad {
        let q = query(ZoneKind::Stack, cast, St::DEFAULT);
        assert_eq!(
            base_characteristics(&d, &q, v),
            Err(err.clone()),
            "{cast:?}"
        );
        assert_eq!(
            mana_value(&d, &q, ManaValueContext::OffStack, v),
            Err(err.clone()),
            "{cast:?}"
        );
    }
}

/// REQ-OBJ-002, CR 712.6c, 712.6e, 712.9, 712.10: on the battlefield the face up decides; cast
/// ignored; mana value from the up face's own cost; copying_back_face ignored.
#[test]
fn dfc_modal_712_6e_battlefield_face_up() {
    let d = modal();
    let v = support::vocab();
    for cast in Cast::ALL {
        for copying in [false, true] {
            let fq = query(ZoneKind::Battlefield, cast, st(false, copying, false));
            assert_eq!(base_characteristics(&d, &fq, v), Ok(chars(&mdfc_front())));
            assert_eq!(mana_value(&d, &fq, ManaValueContext::OffStack, v), Ok(3));
            let bq = query(ZoneKind::Battlefield, cast, st(true, copying, false));
            assert_chars(
                &base_characteristics(&d, &bq, v).expect("back up"),
                &expected_modal_back(),
                &format!("{cast:?} {copying}"),
            );
            assert_eq!(mana_value(&d, &bq, ManaValueContext::OffStack, v), Ok(0));
        }
    }
}

/// REQ-OBJ-002, CR 202.3a, 712.6e: a modal back face with its own cost has that cost's mana
/// value (not the front's), and the front face's `{X}` counts on the stack.
#[test]
fn dfc_modal_202_3a_back_face_own_cost() {
    let v = support::vocab();
    let mut front = mdfc_front();
    front.mana_cost = some_cost("{X}{W}");
    let back = support::creature(FaceKind::Back, "test.mdfc_beast", "{1}{G}", 3, 3);
    let d = def(LayoutBehavior::DfcModal, vec![front, back.clone()]);
    let bq = query(ZoneKind::Stack, Cast::ModalBack, St::DEFAULT);
    assert_eq!(base_characteristics(&d, &bq, v), Ok(chars(&back)));
    assert_eq!(mana_value(&d, &bq, on_stack(6, 0, 0), v), Ok(2));
    let fq = query(ZoneKind::Stack, Cast::ModalFront, St::DEFAULT);
    assert_eq!(mana_value(&d, &fq, on_stack(6, 0, 0), v), Ok(7));
    let bf = query(ZoneKind::Battlefield, Cast::None, St::copying());
    assert_eq!(mana_value(&d, &bf, ManaValueContext::OffStack, v), Ok(1));
    let bfb = query(ZoneKind::Battlefield, Cast::None, st(true, true, false));
    assert_eq!(mana_value(&d, &bfb, ManaValueContext::OffStack, v), Ok(2));
}

/// REQ-OBJ-002, CR 712.6a, 712.6e: faces are found by kind (back listed first works), and the
/// layout objects agree with the free functions.
#[test]
fn dfc_712_6_reverse_face_order_and_layout_objects() {
    let v = support::vocab();
    let cases: [(CardDefinitionDSL, CardDefinitionDSL, &dyn Layout); 2] = [
        (
            nonmodal(),
            def(LayoutBehavior::DfcNonmodal, vec![dfc_back(), dfc_front()]),
            &DfcNonmodalLayout,
        ),
        (
            modal(),
            def(LayoutBehavior::DfcModal, vec![mdfc_back(), mdfc_front()]),
            &DfcModalLayout,
        ),
    ];
    for (forward, reverse, obj) in cases {
        assert_eq!(obj.behavior(), forward.layout);
        let via = layout_for(forward.layout).expect("supported");
        assert_eq!(via.behavior(), forward.layout);
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                for s in St::all() {
                    let q = query(zone, cast, s);
                    let a = base_characteristics(&forward, &q, v);
                    assert_eq!(base_characteristics(&reverse, &q, v), a);
                    assert_eq!(obj.base_characteristics(&reverse, &q, v), a);
                    assert_eq!(via.base_characteristics(&forward, &q, v), a);
                    for ctx in contexts() {
                        let m = mana_value(&forward, &q, ctx, v);
                        assert_eq!(mana_value(&reverse, &q, ctx, v), m);
                        assert_eq!(obj.mana_value(&reverse, &q, ctx, v), m);
                    }
                }
            }
        }
    }
}
