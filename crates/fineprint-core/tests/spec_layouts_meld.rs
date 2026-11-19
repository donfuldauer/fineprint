// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the MELD layout of `fineprint_core::layouts` (REQ-OBJ-002). Black-box:
//! written from the work-item brief only.
//!
//! An unmelded meld card has only its front face in every zone (CR 713.5a, 713.5b, 713.1b); only
//! a designated meld pair melds (CR 701.42b); a melded pair has only the combined back face
//! (CR 713.5c) and the summed front mana values, 0 for a copy (CR 202.3c); leaving the
//! battlefield it is one object and two cards (CR 713.8, 713.8e).

#[path = "spec_layouts_composite_support.rs"]
mod composite;

use composite::support::{
    self, Cast, St, ability, assert_chars, blank, mask, on_stack, printed, query, set, text,
};
use composite::{
    MELD_A, MELD_B, MELD_C, bad_type, meld_a, meld_a_front, meld_b, meld_b_front, meld_result,
    reversed, with_ids,
};
use fineprint_core::layouts::{
    Layout, LayoutError, LayoutQuery, MeldLayout, MeldedPair, base_characteristics, gating,
    layout_for, mana_value,
};
use fineprint_core::objects::{Characteristics, ManaValueContext, ObjectError};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{CardDefinitionDSL, CardFaceDSL, ColorBitmask, FaceKind, LayoutBehavior};

fn v() -> &'static fineprint_dsl::vocab::RulesVocabulary {
    support::vocab()
}

fn bad_faces() -> LayoutError {
    LayoutError::BadFaces {
        layout: LayoutBehavior::Meld,
    }
}

/// The literal front face of meld card A.
fn expected_a_front() -> Characteristics {
    Characteristics {
        names: set(&["test.meld_a_front"]),
        mana_cost: support::some_cost("{2}{W}"),
        color_indicator: None,
        colors: ColorBitmask::WHITE,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Human"]),
        rules_text: text(&["test.meld_a_front"]),
        abilities: vec![ability("test.meld_a_front", 1)],
        power: Some(printed(2)),
        toughness: Some(printed(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The literal front face of meld card B.
fn expected_b_front() -> Characteristics {
    Characteristics {
        names: set(&["test.meld_b_front"]),
        mana_cost: support::some_cost("{X}{3}{U}"),
        color_indicator: None,
        colors: ColorBitmask::BLUE,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Wizard"]),
        rules_text: text(&["test.meld_b_front"]),
        abilities: vec![ability("test.meld_b_front", 1)],
        power: Some(printed(1)),
        toughness: Some(printed(4)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The literal combined back face.
fn expected_whole() -> Characteristics {
    Characteristics {
        names: set(&["test.meld_whole"]),
        mana_cost: support::some_cost("{9}{B}"),
        color_indicator: Some(ColorBitmask::RED),
        colors: mask(ColorBitmask::BLACK.bits() | ColorBitmask::RED.bits()),
        supertypes: set(&["Legendary"]),
        card_types: set(&["Artifact", "Creature"]),
        subtypes: set(&["Golem"]),
        rules_text: text(&["test.meld_whole"]),
        abilities: vec![ability("test.meld_whole", 1), ability("test.meld_whole", 2)],
        power: Some(printed(9)),
        toughness: Some(printed(9)),
        loyalty: Some(3),
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

// ---------------------------------------------------------------------------------------------
// unmelded: MeldLayout
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 713.5a, 713.5b, 713.1b: an unmelded meld card has only its front face in every
/// zone, for every cast choice and status; the MELD_RESULT face never leaks.
#[test]
fn meld_713_5a_unmelded_front_face_every_zone() {
    let rows = [
        (meld_a(), expected_a_front()),
        (reversed(meld_a()), expected_a_front()),
        (meld_b(), expected_b_front()),
    ];
    for (d, expected) in &rows {
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                for st in St::all() {
                    let lq = query(zone, cast, st);
                    let got = MeldLayout
                        .base_characteristics(d, &lq, v())
                        .expect("front face");
                    let ctx = format!("{} {zone:?} {cast:?} {st:?}", d.card_def_id);
                    assert_chars(&got, expected, &ctx);
                    assert!(!got.names.contains("test.meld_whole"), "{ctx}");
                    assert!(!got.subtypes.contains("Golem"), "{ctx}");
                    assert!(!got.supertypes.contains("Legendary"), "{ctx}");
                }
            }
        }
    }
    assert_eq!(MeldLayout.behavior(), LayoutBehavior::Meld);
}

/// REQ-OBJ-002, CR 713.5a, 202.3: the unmelded mana value is the front face's; X is 0 off the
/// stack and the caller's value on the stack; the back face's cost is never used.
#[test]
fn meld_713_5a_unmelded_mana_value() {
    for zone in ZoneKind::ALL {
        for cast in Cast::ALL {
            let lq = query(zone, cast, St::DEFAULT);
            // A: {2}{W} = 3 everywhere, never the back face's {9}{B} = 10.
            for ctx in [ManaValueContext::OffStack, on_stack(4, 4, 4)] {
                assert_eq!(MeldLayout.mana_value(&meld_a(), &lq, ctx, v()), Ok(3));
            }
            // B: {X}{3}{U}: X = 0 off the stack.
            assert_eq!(
                MeldLayout.mana_value(&meld_b(), &lq, ManaValueContext::OffStack, v()),
                Ok(4)
            );
            let expected = if zone == ZoneKind::Stack { 6 } else { 4 };
            assert_eq!(
                MeldLayout.mana_value(&meld_b(), &lq, on_stack(2, 0, 0), v()),
                Ok(expected),
                "{zone:?} {cast:?}"
            );
        }
    }
}

/// REQ-OBJ-002, CR 713.5a: a meld card has no gating data.
#[test]
fn meld_gating_none() {
    assert_eq!(MeldLayout.gating(&meld_a()), Ok(None));
    assert_eq!(MeldLayout.gating(&meld_b()), Ok(None));
    let mut wrong = meld_a();
    wrong.layout = LayoutBehavior::Normal;
    assert_eq!(
        MeldLayout.gating(&wrong),
        Err(LayoutError::LayoutMismatch {
            expected: LayoutBehavior::Meld,
            found: LayoutBehavior::Normal
        })
    );
    let bad = with_ids(
        support::def(LayoutBehavior::Meld, vec![]),
        MELD_A,
        Some(MELD_B),
    );
    assert_eq!(MeldLayout.gating(&bad), Err(bad_faces()));
}

/// The malformed meld face sets: no MAIN, two MAINs, two MELD_RESULTs, a foreign face kind.
fn malformed_meld_faces() -> Vec<Vec<CardFaceDSL>> {
    vec![
        vec![],
        vec![meld_result()],
        vec![meld_a_front(), meld_a_front()],
        vec![meld_a_front(), meld_result(), meld_result()],
        vec![meld_a_front(), blank(FaceKind::Back, "test.meld_back")],
        vec![
            meld_a_front(),
            meld_result(),
            blank(FaceKind::Back, "test.meld_back"),
        ],
        vec![blank(FaceKind::LeftHalf, "test.meld_left")],
    ]
}

/// REQ-OBJ-002, CR 713.5a: wrong layout is `LayoutMismatch`; a face set other than one MAIN and
/// at most one MELD_RESULT is `BadFaces`; a bad MAIN type line is `Object`.
#[test]
fn meld_layout_errors() {
    for found in [LayoutBehavior::DfcNonmodal, LayoutBehavior::Normal] {
        let mut d = meld_a();
        d.layout = found;
        let err = LayoutError::LayoutMismatch {
            expected: LayoutBehavior::Meld,
            found,
        };
        let lq = LayoutQuery::new(ZoneKind::Battlefield);
        assert_eq!(
            MeldLayout.base_characteristics(&d, &lq, v()),
            Err(err.clone())
        );
        assert_eq!(
            MeldLayout.mana_value(&d, &lq, ManaValueContext::OffStack, v()),
            Err(err.clone())
        );
        // Mismatch beats bad faces.
        let d2 = support::def(found, vec![]);
        assert_eq!(MeldLayout.base_characteristics(&d2, &lq, v()), Err(err));
    }
    for faces in malformed_meld_faces() {
        let d = with_ids(
            support::def(LayoutBehavior::Meld, faces),
            MELD_A,
            Some(MELD_B),
        );
        for zone in ZoneKind::ALL {
            let lq = LayoutQuery::new(zone);
            assert_eq!(
                MeldLayout.base_characteristics(&d, &lq, v()),
                Err(bad_faces())
            );
            assert_eq!(
                MeldLayout.mana_value(&d, &lq, ManaValueContext::OffStack, v()),
                Err(bad_faces())
            );
        }
    }
    let d = with_ids(
        support::def(LayoutBehavior::Meld, vec![bad_type(meld_a_front())]),
        MELD_A,
        None,
    );
    let err = LayoutError::Object(ObjectError::UnknownCardType("NotACardType".to_owned()));
    for zone in ZoneKind::ALL {
        let lq = LayoutQuery::new(zone);
        assert_eq!(
            MeldLayout.base_characteristics(&d, &lq, v()),
            Err(err.clone())
        );
    }
    // A bad type line on the never-used MELD_RESULT face does not matter to the unmelded card.
    let d = with_ids(
        support::def(
            LayoutBehavior::Meld,
            vec![meld_a_front(), bad_type(meld_result())],
        ),
        MELD_A,
        Some(MELD_B),
    );
    let lq = LayoutQuery::new(ZoneKind::Battlefield);
    assert_chars(
        &MeldLayout.base_characteristics(&d, &lq, v()).unwrap(),
        &expected_a_front(),
        "bad result face unused",
    );
}

// ---------------------------------------------------------------------------------------------
// MeldedPair::new
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 701.42b, 713.5c: the designated pair melds in either argument order.
#[test]
fn meld_701_42b_valid_pair_both_orders() {
    let a = meld_a();
    let b = meld_b();
    let ab = MeldedPair::new(&a, &b).expect("pair");
    let ba = MeldedPair::new(&b, &a).expect("pair");
    for pair in [ab, ba] {
        assert_chars(
            &pair.characteristics(v()).unwrap(),
            &expected_whole(),
            "pair",
        );
    }
    // Reversed face order within a card is fine too.
    let ar = reversed(meld_a());
    assert!(MeldedPair::new(&ar, &b).is_ok());
}

/// REQ-OBJ-002, CR 701.42b: only meld cards meld.
#[test]
fn meld_701_42b_wrong_layout_is_mismatch() {
    let a = meld_a();
    let b = meld_b();
    let mut b_normal = meld_b();
    b_normal.layout = LayoutBehavior::Normal;
    let err = Some(LayoutError::LayoutMismatch {
        expected: LayoutBehavior::Meld,
        found: LayoutBehavior::Normal,
    });
    assert_eq!(MeldedPair::new(&a, &b_normal).err(), err);
    assert_eq!(MeldedPair::new(&b_normal, &a).err(), err);
    let mut a_split = meld_a();
    a_split.layout = LayoutBehavior::Split;
    assert_eq!(
        MeldedPair::new(&a_split, &b).err(),
        Some(LayoutError::LayoutMismatch {
            expected: LayoutBehavior::Meld,
            found: LayoutBehavior::Split
        })
    );
}

/// REQ-OBJ-002, CR 701.42b: each card's face set must be valid.
#[test]
fn meld_701_42b_bad_faces() {
    let b = meld_b();
    for faces in malformed_meld_faces() {
        let a = with_ids(
            support::def(LayoutBehavior::Meld, faces),
            MELD_A,
            Some(MELD_B),
        );
        assert_eq!(MeldedPair::new(&a, &b).err(), Some(bad_faces()));
        assert_eq!(MeldedPair::new(&b, &a).err(), Some(bad_faces()));
    }
}

/// REQ-OBJ-002, CR 701.42b: only the designated meld pair melds: both partner links, distinct ids.
#[test]
fn meld_701_42b_not_meld_pair() {
    let a = meld_a();
    let not_pair = Some(LayoutError::NotMeldPair);
    // One-way link: B points at nothing, or at a third card.
    let b_unlinked = with_ids(meld_b(), MELD_B, None);
    assert_eq!(MeldedPair::new(&a, &b_unlinked).err(), not_pair);
    assert_eq!(MeldedPair::new(&b_unlinked, &a).err(), not_pair);
    let b_third = with_ids(meld_b(), MELD_B, Some(MELD_C));
    assert_eq!(MeldedPair::new(&a, &b_third).err(), not_pair);
    assert_eq!(MeldedPair::new(&b_third, &a).err(), not_pair);
    // A points at a third card while B points at A.
    let a_third = with_ids(meld_a(), MELD_A, Some(MELD_C));
    let b = meld_b();
    assert_eq!(MeldedPair::new(&a_third, &b).err(), not_pair);
    // Both links but the same id (a card cannot meld with itself or a same-id twin).
    let self_a = with_ids(meld_a(), MELD_A, Some(MELD_A));
    let self_b = with_ids(meld_b(), MELD_A, Some(MELD_A));
    assert_eq!(MeldedPair::new(&self_a, &self_b).err(), not_pair);
    assert_eq!(MeldedPair::new(&self_a, &self_a).err(), not_pair);
    // Neither linked.
    let a0 = with_ids(meld_a(), MELD_A, None);
    assert_eq!(MeldedPair::new(&a0, &b_unlinked).err(), not_pair);
    // A pair of a third card with A's partner C that points back works.
    let c = with_ids(meld_b(), MELD_C, Some(MELD_A));
    assert!(MeldedPair::new(&a_third, &c).is_ok());
}

/// REQ-OBJ-002, CR 713.5c: exactly one card of the pair carries the combined back face.
#[test]
fn meld_713_5c_exactly_one_result_face() {
    let a = meld_a();
    let b = meld_b();
    // Neither has a MELD_RESULT face.
    let a_plain = with_ids(
        support::def(LayoutBehavior::Meld, vec![meld_a_front()]),
        MELD_A,
        Some(MELD_B),
    );
    assert_eq!(MeldedPair::new(&a_plain, &b).err(), Some(bad_faces()));
    assert_eq!(MeldedPair::new(&b, &a_plain).err(), Some(bad_faces()));
    // Both do.
    let b_full = with_ids(
        support::def(LayoutBehavior::Meld, vec![meld_b_front(), meld_result()]),
        MELD_B,
        Some(MELD_A),
    );
    assert_eq!(MeldedPair::new(&a, &b_full).err(), Some(bad_faces()));
    assert_eq!(MeldedPair::new(&b_full, &a).err(), Some(bad_faces()));
    // B carrying it instead of A is fine.
    let b_carrier = b_full.clone();
    assert!(MeldedPair::new(&a_plain, &b_carrier).is_ok());
}

/// REQ-OBJ-002, CR 701.42b: `MeldedPair::new` checks layout, then faces, then the pair link,
/// then the single result face.
#[test]
fn meld_pair_error_precedence() {
    let b = meld_b();
    // Layout beats bad faces and a missing link.
    let wrong_and_bad = with_ids(support::def(LayoutBehavior::Normal, vec![]), MELD_C, None);
    assert_eq!(
        MeldedPair::new(&wrong_and_bad, &b).err(),
        Some(LayoutError::LayoutMismatch {
            expected: LayoutBehavior::Meld,
            found: LayoutBehavior::Normal
        })
    );
    // Layout of the second card beats bad faces of the first.
    let bad_first = with_ids(
        support::def(LayoutBehavior::Meld, vec![]),
        MELD_A,
        Some(MELD_B),
    );
    let mut b_wrong = meld_b();
    b_wrong.layout = LayoutBehavior::Saga;
    assert_eq!(
        MeldedPair::new(&bad_first, &b_wrong).err(),
        Some(LayoutError::LayoutMismatch {
            expected: LayoutBehavior::Meld,
            found: LayoutBehavior::Saga
        })
    );
    // Bad faces beat a missing link.
    let bad_unlinked = with_ids(support::def(LayoutBehavior::Meld, vec![]), MELD_C, None);
    assert_eq!(MeldedPair::new(&bad_unlinked, &b).err(), Some(bad_faces()));
    assert_eq!(MeldedPair::new(&b, &bad_unlinked).err(), Some(bad_faces()));
    // A missing link beats "neither has a result face".
    let a_plain_unlinked = with_ids(
        support::def(LayoutBehavior::Meld, vec![meld_a_front()]),
        MELD_A,
        None,
    );
    assert_eq!(
        MeldedPair::new(&a_plain_unlinked, &b).err(),
        Some(LayoutError::NotMeldPair)
    );
}

// ---------------------------------------------------------------------------------------------
// the melded permanent
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 713.5c, 713.1b: the melded permanent has only the combined back face; it
/// shares nothing with either front face.
#[test]
fn meld_713_5c_characteristics_combined_face_only() {
    let a = meld_a();
    let b = meld_b();
    let got = MeldedPair::new(&a, &b)
        .unwrap()
        .characteristics(v())
        .unwrap();
    assert_chars(&got, &expected_whole(), "melded");
    for front in [expected_a_front(), expected_b_front()] {
        assert!(got.names.is_disjoint(&front.names));
        assert_ne!(got.mana_cost, front.mana_cost);
        assert!(got.subtypes.is_disjoint(&front.subtypes));
        assert!(got.abilities.iter().all(|x| !front.abilities.contains(x)));
        assert_ne!(got.rules_text, front.rules_text);
        assert_ne!(got.power, front.power);
    }
}

/// REQ-OBJ-002, CR 713.5c: a bad type line on the combined face is `Object`.
#[test]
fn meld_713_5c_bad_result_face_is_object() {
    let a = with_ids(
        support::def(
            LayoutBehavior::Meld,
            vec![meld_a_front(), bad_type(meld_result())],
        ),
        MELD_A,
        Some(MELD_B),
    );
    let b = meld_b();
    let pair = MeldedPair::new(&a, &b).expect("faces and link are valid");
    assert_eq!(
        pair.characteristics(v()),
        Err(LayoutError::Object(ObjectError::UnknownCardType(
            "NotACardType".to_owned()
        )))
    );
}

/// REQ-OBJ-002, CR 202.3c, 713.5c: the melded mana value is the sum of the fronts' mana values
/// (X counts 0), not the back face's own cost; a copy of a melded permanent has mana value 0.
#[test]
fn meld_202_3c_mana_value() {
    let a = meld_a();
    let b = meld_b();
    for (first, second) in [(&a, &b), (&b, &a)] {
        let pair = MeldedPair::new(first, second).unwrap();
        // {2}{W} = 3 plus {X}{3}{U} = 4 (X = 0): 7.
        assert_eq!(pair.mana_value(false, v()), Ok(7));
        assert_eq!(pair.mana_value(true, v()), Ok(0));
    }
    // The back face's own cost {9}{B} would be 10: not used.
    let pair = MeldedPair::new(&a, &b).unwrap();
    assert_ne!(pair.mana_value(false, v()), Ok(10));

    // A costless front counts 0: 3 + 0.
    let b_free = with_ids(
        support::def(
            LayoutBehavior::Meld,
            vec![CardFaceDSL {
                mana_cost: None,
                ..meld_b_front()
            }],
        ),
        MELD_B,
        Some(MELD_A),
    );
    let pair = MeldedPair::new(&a, &b_free).unwrap();
    assert_eq!(pair.mana_value(false, v()), Ok(3));
    assert_eq!(pair.mana_value(true, v()), Ok(0));

    // Both fronts costless: 0, though the back face has a cost.
    let a_free = with_ids(
        support::def(
            LayoutBehavior::Meld,
            vec![
                CardFaceDSL {
                    mana_cost: None,
                    ..meld_a_front()
                },
                meld_result(),
            ],
        ),
        MELD_A,
        Some(MELD_B),
    );
    let pair = MeldedPair::new(&a_free, &b_free).unwrap();
    assert_eq!(pair.mana_value(false, v()), Ok(0));
    assert_chars(
        &pair.characteristics(v()).unwrap(),
        &expected_whole(),
        "free fronts",
    );
}

/// REQ-OBJ-002, CR 713.8, 713.8e: one object, two cards; both cards leave, in argument order.
#[test]
fn meld_713_8_counts_and_cards_on_leaving() {
    let a = meld_a();
    let b = meld_b();
    let ab = MeldedPair::new(&a, &b).unwrap();
    assert_eq!(ab.object_count(), 1);
    assert_eq!(ab.card_count(), 2);
    let [x, y] = ab.cards_on_leaving();
    assert!(std::ptr::eq(x, &a));
    assert!(std::ptr::eq(y, &b));
    let ba = MeldedPair::new(&b, &a).unwrap();
    let [x, y] = ba.cards_on_leaving();
    assert!(std::ptr::eq(x, &b));
    assert!(std::ptr::eq(y, &a));
    assert_eq!(x.card_def_id, MELD_B);
    assert_eq!(y.card_def_id, MELD_A);
    // Copy: a copy answers the same.
    let copy = ab;
    assert_eq!(copy.object_count(), ab.object_count());
}

/// REQ-OBJ-002, CR 713.5c: the pair functions are deterministic.
#[test]
fn meld_pair_deterministic() {
    let a = meld_a();
    let b = meld_b();
    let p1 = MeldedPair::new(&a, &b).unwrap();
    let p2 = MeldedPair::new(&a, &b).unwrap();
    assert_eq!(p1.characteristics(v()), p2.characteristics(v()));
    assert_eq!(p1.mana_value(false, v()), p2.mana_value(false, v()));
    assert!(!format!("{p1:?}").is_empty());
}

/// REQ-OBJ-002: MELD is not wired into `layout_for` or the free functions; the new error has
/// Display text.
#[test]
fn meld_not_wired_into_layout_for() {
    let err = LayoutError::UnsupportedLayout(LayoutBehavior::Meld);
    assert_eq!(layout_for(LayoutBehavior::Meld).err(), Some(err.clone()));
    let d = meld_a();
    let lq = LayoutQuery::new(ZoneKind::Hand);
    assert_eq!(base_characteristics(&d, &lq, v()), Err(err.clone()));
    assert_eq!(
        mana_value(&d, &lq, ManaValueContext::OffStack, v()),
        Err(err.clone())
    );
    assert_eq!(gating(&d), Err(err));
    assert!(!LayoutError::NotMeldPair.to_string().is_empty());
}

/// REQ-OBJ-002, CR 713.5a: no panic on any definition shape, for the layout and the pair.
#[test]
fn meld_sweep_no_panic_agreement() {
    let mut defs: Vec<CardDefinitionDSL> = vec![meld_a(), meld_b(), reversed(meld_a())];
    for faces in malformed_meld_faces() {
        defs.push(with_ids(
            support::def(LayoutBehavior::Meld, faces),
            MELD_A,
            Some(MELD_B),
        ));
    }
    let mut wrong = meld_a();
    wrong.layout = LayoutBehavior::Room;
    defs.push(wrong);
    for d in &defs {
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                let lq = query(zone, cast, St::DEFAULT);
                let c = MeldLayout.base_characteristics(d, &lq, v());
                assert_eq!(c, MeldLayout.base_characteristics(d, &lq, v()));
                for ctx in support::contexts() {
                    let m = MeldLayout.mana_value(d, &lq, ctx, v());
                    match (&c, &m) {
                        (Ok(_), Ok(_)) => {}
                        (Err(x), Err(y)) => assert_eq!(x, y),
                        other => panic!("disagree: {other:?}"),
                    }
                }
            }
        }
        for e in &defs {
            if let Ok(pair) = MeldedPair::new(d, e) {
                let _ = pair.characteristics(v());
                let _ = pair.mana_value(false, v());
            }
        }
    }
}
