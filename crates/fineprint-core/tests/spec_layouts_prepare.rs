// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the PREPARATION layout of `fineprint_core::layouts` (REQ-OBJ-002). Black-box:
//! written from the work-item brief only.
//!
//! A preparation card has only its normal characteristics in every zone (CR 723.4, 723.3); the
//! prepare spell (CR 723.2a) supplies the characteristics of the copy made when a permanent
//! becomes prepared (CR 723.3c), which it cannot do twice (CR 723.3a).

#[path = "spec_layouts_composite_support.rs"]
mod composite;

use composite::support::{
    self, Cast, St, ability, assert_chars, blank, on_stack, printed, query, set, text,
};
use composite::{bad_type, prep_def, prep_main, prep_spell, reversed};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::layouts::{
    Layout, LayoutError, LayoutQuery, PREPARED_DESIGNATION, PreparationLayout,
    base_characteristics, become_prepared, gating, layout_for, mana_value,
    prepare_spell_characteristics,
};
use fineprint_core::objects::{Characteristics, ManaValueContext, ObjectError};
use fineprint_core::permanent::Permanents;
use fineprint_core::state::DesignationValue;
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{CardDefinitionDSL, ColorBitmask, FaceKind, LayoutBehavior};

fn v() -> &'static fineprint_dsl::vocab::RulesVocabulary {
    support::vocab()
}

/// The literal normal characteristics.
fn expected_main() -> Characteristics {
    Characteristics {
        names: set(&["test.prep_body"]),
        mana_cost: support::some_cost("{X}{2}{G}"),
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Human"]),
        rules_text: text(&["test.prep_body"]),
        abilities: vec![ability("test.prep_body", 1)],
        power: Some(printed(3)),
        toughness: Some(printed(3)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The literal prepare-spell characteristics.
fn expected_spell() -> Characteristics {
    Characteristics {
        names: set(&["test.prep_inset"]),
        mana_cost: support::some_cost("{1}{R}{R}"),
        color_indicator: Some(ColorBitmask::BLUE),
        colors: support::mask(ColorBitmask::RED.bits() | ColorBitmask::BLUE.bits()),
        supertypes: set(&["Legendary"]),
        card_types: set(&["Artifact", "Creature"]),
        subtypes: set(&["Golem"]),
        rules_text: text(&["test.prep_inset"]),
        abilities: vec![ability("test.prep_inset", 1)],
        power: Some(printed(7)),
        toughness: Some(printed(1)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn mismatch(found: LayoutBehavior) -> LayoutError {
    LayoutError::LayoutMismatch {
        expected: LayoutBehavior::Preparation,
        found,
    }
}

fn bad_faces() -> LayoutError {
    LayoutError::BadFaces {
        layout: LayoutBehavior::Preparation,
    }
}

fn invalid(zone: ZoneKind) -> LayoutError {
    LayoutError::InvalidCast {
        layout: LayoutBehavior::Preparation,
        zone,
    }
}

fn obj_err() -> LayoutError {
    LayoutError::Object(ObjectError::UnknownCardType("NotACardType".to_owned()))
}

/// REQ-OBJ-002, CR 723.4, 723.3: in every zone, stack and battlefield included, for every cast
/// choice and status, a preparation card has only its normal characteristics; nothing of the
/// prepare spell leaks.
#[test]
fn prepare_723_4_main_face_every_zone() {
    for d in [prep_def(), reversed(prep_def())] {
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                for st in St::all() {
                    let lq = query(zone, cast, st);
                    let got = PreparationLayout
                        .base_characteristics(&d, &lq, v())
                        .expect("normal chars");
                    let ctx = format!("{zone:?} {cast:?} {st:?}");
                    assert_chars(&got, &expected_main(), &ctx);
                    assert!(!got.names.contains("test.prep_inset"), "{ctx}");
                    assert!(!got.card_types.contains("Artifact"), "{ctx}");
                }
            }
        }
    }
    assert_eq!(PreparationLayout.behavior(), LayoutBehavior::Preparation);
}

/// REQ-OBJ-002, CR 723.4: the mana value is the normal cost's; X is 0 off the stack.
#[test]
fn prepare_723_4_mana_value() {
    let d = prep_def();
    for zone in ZoneKind::ALL {
        for cast in Cast::ALL {
            let lq = query(zone, cast, St::DEFAULT);
            assert_eq!(
                PreparationLayout.mana_value(&d, &lq, ManaValueContext::OffStack, v()),
                Ok(3)
            );
            let expected = if zone == ZoneKind::Stack { 7 } else { 3 };
            assert_eq!(
                PreparationLayout.mana_value(&d, &lq, on_stack(4, 0, 0), v()),
                Ok(expected),
                "{zone:?} {cast:?}"
            );
        }
    }
    assert_eq!(PreparationLayout.gating(&d), Ok(None));
}

/// REQ-OBJ-002, CR 723.2a, 723.3c: the prepare spell's characteristics are its face only.
#[test]
fn prepare_723_3c_prepare_spell_characteristics() {
    let got = prepare_spell_characteristics(&prep_def(), v()).unwrap();
    assert_chars(&got, &expected_spell(), "prepare spell");
    assert_eq!(got, support::chars(&prep_spell()));
    let got = prepare_spell_characteristics(&reversed(prep_def()), v()).unwrap();
    assert_chars(&got, &expected_spell(), "reversed");
}

/// REQ-OBJ-002, CR 723.3a, 723.3c: only a permanent becomes prepared, and not twice; success is
/// the prepare spell's characteristics.
#[test]
fn prepare_723_3a_become_prepared_every_zone() {
    let d = prep_def();
    for zone in ZoneKind::ALL {
        for already in [false, true] {
            let got = become_prepared(&d, zone, already, v());
            let expected = match (zone, already) {
                (ZoneKind::Battlefield, false) => Ok(expected_spell()),
                (ZoneKind::Battlefield, true) => Err(LayoutError::AlreadyPrepared),
                _ => Err(invalid(zone)),
            };
            assert_eq!(got, expected, "{zone:?} {already}");
        }
    }
}

fn malformed() -> Vec<CardDefinitionDSL> {
    let mk = |faces| support::def(LayoutBehavior::Preparation, faces);
    vec![
        mk(vec![]),
        mk(vec![prep_main()]),
        mk(vec![prep_spell()]),
        mk(vec![prep_main(), prep_spell(), prep_spell()]),
        mk(vec![prep_main(), prep_main(), prep_spell()]),
        mk(vec![
            prep_main(),
            prep_spell(),
            blank(FaceKind::Back, "test.prep_back"),
        ]),
        mk(vec![
            prep_main(),
            blank(FaceKind::Adventure, "test.prep_adv"),
        ]),
    ]
}

/// REQ-OBJ-002, CR 723.4: wrong layout, bad faces and bad type lines for the layout and the
/// prepare-spell function.
#[test]
fn prepare_layout_errors() {
    let mut wrong = prep_def();
    wrong.layout = LayoutBehavior::Adventurer;
    let lq = LayoutQuery::new(ZoneKind::Battlefield);
    let m = mismatch(LayoutBehavior::Adventurer);
    assert_eq!(
        PreparationLayout.base_characteristics(&wrong, &lq, v()),
        Err(m.clone())
    );
    assert_eq!(
        PreparationLayout.mana_value(&wrong, &lq, ManaValueContext::OffStack, v()),
        Err(m.clone())
    );
    assert_eq!(PreparationLayout.gating(&wrong), Err(m.clone()));
    assert_eq!(prepare_spell_characteristics(&wrong, v()), Err(m));
    for d in malformed() {
        for zone in ZoneKind::ALL {
            let lq = LayoutQuery::new(zone);
            assert_eq!(
                PreparationLayout.base_characteristics(&d, &lq, v()),
                Err(bad_faces())
            );
            assert_eq!(
                PreparationLayout.mana_value(&d, &lq, ManaValueContext::OffStack, v()),
                Err(bad_faces())
            );
        }
        assert_eq!(PreparationLayout.gating(&d), Err(bad_faces()));
        assert_eq!(prepare_spell_characteristics(&d, v()), Err(bad_faces()));
    }
    // A bad normal type line is Object for the layout; a bad prepare-spell type line is Object
    // for the prepare-spell function.
    let d = support::def(
        LayoutBehavior::Preparation,
        vec![bad_type(prep_main()), prep_spell()],
    );
    assert_eq!(
        PreparationLayout.base_characteristics(&d, &lq, v()),
        Err(obj_err())
    );
    let d = support::def(
        LayoutBehavior::Preparation,
        vec![prep_main(), bad_type(prep_spell())],
    );
    assert_eq!(prepare_spell_characteristics(&d, v()), Err(obj_err()));
    assert_eq!(
        become_prepared(&d, ZoneKind::Battlefield, false, v()),
        Err(obj_err())
    );
}

/// REQ-OBJ-002, CR 723.3a: `become_prepared` checks layout, faces, zone, already prepared, then
/// the type line.
#[test]
fn prepare_723_3a_error_precedence() {
    // Wrong layout beats everything: bad faces, wrong zone, already prepared.
    let wrong_bad = support::def(LayoutBehavior::Normal, vec![]);
    for zone in ZoneKind::ALL {
        for already in [false, true] {
            assert_eq!(
                become_prepared(&wrong_bad, zone, already, v()),
                Err(mismatch(LayoutBehavior::Normal)),
                "{zone:?} {already}"
            );
        }
    }
    // Bad faces beat the zone and already prepared.
    for d in malformed() {
        for zone in ZoneKind::ALL {
            for already in [false, true] {
                assert_eq!(
                    become_prepared(&d, zone, already, v()),
                    Err(bad_faces()),
                    "{zone:?} {already}"
                );
            }
        }
    }
    // The zone beats already prepared and a bad type line.
    let bad_type_def = support::def(
        LayoutBehavior::Preparation,
        vec![prep_main(), bad_type(prep_spell())],
    );
    for zone in ZoneKind::ALL {
        if zone == ZoneKind::Battlefield {
            continue;
        }
        assert_eq!(
            become_prepared(&prep_def(), zone, true, v()),
            Err(invalid(zone))
        );
        assert_eq!(
            become_prepared(&bad_type_def, zone, true, v()),
            Err(invalid(zone))
        );
        assert_eq!(
            become_prepared(&bad_type_def, zone, false, v()),
            Err(invalid(zone))
        );
    }
    // Already prepared beats a bad type line.
    assert_eq!(
        become_prepared(&bad_type_def, ZoneKind::Battlefield, true, v()),
        Err(LayoutError::AlreadyPrepared)
    );
    assert!(!LayoutError::AlreadyPrepared.to_string().is_empty());
}

/// REQ-OBJ-002, CR 723.3a: the designation name.
#[test]
fn prepare_723_3a_designation_name() {
    assert_eq!(PREPARED_DESIGNATION, "prepared");
}

/// REQ-OBJ-002, CR 723.3a: the prepared designation can be recorded on a permanent with the
/// existing designation API.
#[test]
fn prepare_723_3a_designation_on_permanent() {
    let mut p = Permanents::new();
    let id = ObjectId::new(1);
    p.enter(id, PlayerId::new(1), 1).unwrap();
    assert_eq!(p.get(id).unwrap().designation(PREPARED_DESIGNATION), None);
    p.set_designation(id, PREPARED_DESIGNATION, DesignationValue::Flag)
        .unwrap();
    assert_eq!(
        p.get(id).unwrap().designation("prepared"),
        Some(&DesignationValue::Flag)
    );
}

/// REQ-OBJ-002: PREPARATION is not wired into `layout_for` or the free functions.
#[test]
fn prepare_not_wired_into_layout_for() {
    let err = LayoutError::UnsupportedLayout(LayoutBehavior::Preparation);
    assert_eq!(
        layout_for(LayoutBehavior::Preparation).err(),
        Some(err.clone())
    );
    let d = prep_def();
    let lq = LayoutQuery::new(ZoneKind::Hand);
    assert_eq!(base_characteristics(&d, &lq, v()), Err(err.clone()));
    assert_eq!(
        mana_value(&d, &lq, ManaValueContext::OffStack, v()),
        Err(err.clone())
    );
    assert_eq!(gating(&d), Err(err));
}
