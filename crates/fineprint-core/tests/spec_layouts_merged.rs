// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the MUTATE layout and merged permanents of `fineprint_core::layouts`
//! (REQ-OBJ-002). Black-box: written from the work-item brief only.
//!
//! A merged permanent has only its topmost component's characteristics (CR 731.2a); a mutated
//! permanent also has every component's abilities (CR 702.140e); components go over or under
//! (CR 702.140c, 731.2); token and face-up status come from the top (CR 731.2d, 731.2e); leaving,
//! it is one permanent and every component moves (CR 731.3).

#[path = "spec_layouts_composite_support.rs"]
mod composite;

use composite::support::{
    self, Cast, St, ability, assert_chars, blank, dfc_back, dfc_front, printed, query, sample, set,
    text,
};
use composite::{
    bad_type, meld_a, merge_base_def, merge_base_main, mutant_def, mutant_main, prep_def,
    room_sample, station_def, with_ids,
};
use fineprint_core::layouts::{
    CastChoice, FaceUp, Layout, LayoutError, LayoutQuery, LayoutStatus, MergeSide, MergedComponent,
    MergedPermanent, MutateLayout, base_characteristics, gating, layout_for, mana_value,
};
use fineprint_core::objects::{Characteristics, ManaValueContext, ObjectError};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{CardDefinitionDSL, ColorBitmask, FaceKind, LayoutBehavior};

fn v() -> &'static fineprint_dsl::vocab::RulesVocabulary {
    support::vocab()
}

fn comp(def: &CardDefinitionDSL, is_token: bool, face_up: bool) -> MergedComponent<'_> {
    MergedComponent {
        def,
        query: LayoutQuery::new(ZoneKind::Battlefield),
        is_token,
        face_up,
    }
}

fn plain(def: &CardDefinitionDSL) -> MergedComponent<'_> {
    comp(def, false, true)
}

/// The literal characteristics of the mutating creature.
fn expected_mutant() -> Characteristics {
    Characteristics {
        names: set(&["test.mutant"]),
        mana_cost: support::some_cost("{2}{B}{B}"),
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Wizard"]),
        rules_text: text(&["test.mutant"]),
        abilities: vec![ability("test.shared", 1), ability("test.mutant", 1)],
        power: Some(printed(4)),
        toughness: Some(printed(4)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The literal characteristics of the NORMAL merge base.
fn expected_base() -> Characteristics {
    Characteristics {
        names: set(&["test.merge_base"]),
        mana_cost: support::some_cost("{1}{U}"),
        color_indicator: None,
        colors: ColorBitmask::BLUE,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Human"]),
        rules_text: text(&["test.merge_base"]),
        abilities: vec![ability("test.shared", 1), ability("test.merge_base", 1)],
        power: Some(printed(1)),
        toughness: Some(printed(1)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The literal characteristics of the shared NORMAL sample (`{X}{1}{G}` 2/2 Human).
fn expected_normal() -> Characteristics {
    Characteristics {
        names: set(&["test.normal_main"]),
        mana_cost: support::some_cost("{X}{1}{G}"),
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
    }
}

fn defs_ptr(list: &[MergedComponent<'_>]) -> Vec<*const CardDefinitionDSL> {
    list.iter()
        .map(|c| c.def as *const CardDefinitionDSL)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// MutateLayout
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 702.140e: a mutate card off the battlefield (and as a component) has its
/// printed characteristics in every zone; mana value from its cost.
#[test]
fn mutate_layout_printed_every_zone() {
    let d = mutant_def();
    for zone in ZoneKind::ALL {
        for cast in Cast::ALL {
            for st in St::all() {
                let lq = query(zone, cast, st);
                let got = MutateLayout.base_characteristics(&d, &lq, v()).unwrap();
                assert_chars(
                    &got,
                    &expected_mutant(),
                    &format!("{zone:?} {cast:?} {st:?}"),
                );
                assert_eq!(
                    MutateLayout.mana_value(&d, &lq, ManaValueContext::OffStack, v()),
                    Ok(4)
                );
            }
        }
    }
    assert_eq!(MutateLayout.behavior(), LayoutBehavior::Mutate);
}

/// REQ-OBJ-002, CR 702.140e: MUTATE faces are exactly MAIN; layout mismatch first.
#[test]
fn mutate_layout_errors() {
    let lq = LayoutQuery::new(ZoneKind::Hand);
    let mut wrong = mutant_def();
    wrong.layout = LayoutBehavior::Normal;
    assert_eq!(
        MutateLayout.base_characteristics(&wrong, &lq, v()),
        Err(LayoutError::LayoutMismatch {
            expected: LayoutBehavior::Mutate,
            found: LayoutBehavior::Normal
        })
    );
    let bad = LayoutError::BadFaces {
        layout: LayoutBehavior::Mutate,
    };
    for faces in [
        vec![],
        vec![mutant_main(), mutant_main()],
        vec![mutant_main(), blank(FaceKind::Back, "test.mutant_back")],
    ] {
        let d = support::def(LayoutBehavior::Mutate, faces);
        assert_eq!(
            MutateLayout.base_characteristics(&d, &lq, v()),
            Err(bad.clone())
        );
        assert_eq!(
            MutateLayout.mana_value(&d, &lq, ManaValueContext::OffStack, v()),
            Err(bad.clone())
        );
    }
    let d = support::def(LayoutBehavior::Mutate, vec![bad_type(mutant_main())]);
    assert_eq!(
        MutateLayout.base_characteristics(&d, &lq, v()),
        Err(LayoutError::Object(ObjectError::UnknownCardType(
            "NotACardType".to_owned()
        )))
    );
}

// ---------------------------------------------------------------------------------------------
// MergedPermanent: characteristics
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 731.2a: two components, either on top: only the top's characteristics.
#[test]
fn merged_731_2a_top_supplies_characteristics_two() {
    let m = mutant_def();
    let b = merge_base_def();
    let mp = MergedPermanent::new(vec![plain(&m), plain(&b)], false).unwrap();
    assert_chars(
        &mp.base_characteristics(v()).unwrap(),
        &expected_mutant(),
        "mutant top",
    );
    assert_eq!(mp.mana_value(v()), Ok(4));
    let mp = MergedPermanent::new(vec![plain(&b), plain(&m)], false).unwrap();
    assert_chars(
        &mp.base_characteristics(v()).unwrap(),
        &expected_base(),
        "base top",
    );
    assert_eq!(mp.mana_value(v()), Ok(2));
}

/// REQ-OBJ-002, CR 731.2a: three components: only the top's characteristics; mana value from
/// the top with X counted 0.
#[test]
fn merged_731_2a_top_supplies_characteristics_three() {
    let n = sample(LayoutBehavior::Normal);
    let m = mutant_def();
    let b = merge_base_def();
    let mp = MergedPermanent::new(vec![plain(&n), plain(&m), plain(&b)], false).unwrap();
    assert_chars(
        &mp.base_characteristics(v()).unwrap(),
        &expected_normal(),
        "normal top",
    );
    // {X}{1}{G}: X = 0.
    assert_eq!(mp.mana_value(v()), Ok(2));
    let mp = MergedPermanent::new(vec![plain(&b), plain(&n), plain(&m)], false).unwrap();
    assert_chars(
        &mp.base_characteristics(v()).unwrap(),
        &expected_base(),
        "base top",
    );
}

/// REQ-OBJ-002, CR 702.140e: a mutated permanent has every component's abilities, top to
/// bottom, duplicates kept; every other characteristic, rules text included, is the top's.
#[test]
fn merged_702_140e_mutated_abilities() {
    let n = sample(LayoutBehavior::Normal);
    let m = mutant_def();
    let b = merge_base_def();
    let mp = MergedPermanent::new(vec![plain(&m), plain(&b), plain(&n)], true).unwrap();
    let expected = Characteristics {
        abilities: vec![
            ability("test.shared", 1),
            ability("test.mutant", 1),
            ability("test.shared", 1),
            ability("test.merge_base", 1),
            ability("test.normal_main", 1),
        ],
        ..expected_mutant()
    };
    assert_chars(&mp.base_characteristics(v()).unwrap(), &expected, "mutated");
    assert_eq!(mp.mana_value(v()), Ok(4));

    // Unmutated: the top's abilities only.
    let mp = MergedPermanent::new(vec![plain(&m), plain(&b), plain(&n)], false).unwrap();
    assert_chars(
        &mp.base_characteristics(v()).unwrap(),
        &expected_mutant(),
        "merged",
    );

    // Other order.
    let mp = MergedPermanent::new(vec![plain(&n), plain(&b), plain(&m)], true).unwrap();
    let expected = Characteristics {
        abilities: vec![
            ability("test.normal_main", 1),
            ability("test.shared", 1),
            ability("test.merge_base", 1),
            ability("test.shared", 1),
            ability("test.mutant", 1),
        ],
        ..expected_normal()
    };
    assert_chars(
        &mp.base_characteristics(v()).unwrap(),
        &expected,
        "normal top mutated",
    );

    // The same card twice: both copies of its abilities.
    let m2 = mutant_def();
    let mp = MergedPermanent::new(vec![plain(&m), plain(&m2)], true).unwrap();
    assert_eq!(
        mp.base_characteristics(v()).unwrap().abilities,
        vec![
            ability("test.shared", 1),
            ability("test.mutant", 1),
            ability("test.shared", 1),
            ability("test.mutant", 1),
        ]
    );
}

/// REQ-OBJ-002, CR 731.2a: component layouts: the top's own layout is selected at the
/// battlefield whatever zone its query names (Station P/T present; Preparation normal face;
/// Meld front face; a DFC with its back face up).
#[test]
fn merged_731_2a_component_layouts() {
    let st = station_def();
    let pr = prep_def();
    let me = meld_a();
    let b = merge_base_def();
    let hand = |def| MergedComponent {
        def,
        query: LayoutQuery::new(ZoneKind::Hand),
        is_token: false,
        face_up: true,
    };
    // Station on top, query naming the hand: the battlefield's P/T 4/5 is present.
    let mp = MergedPermanent::new(vec![hand(&st), plain(&b)], false).unwrap();
    let got = mp.base_characteristics(v()).unwrap();
    assert_eq!(got.names, set(&["test.station_hull"]));
    assert_eq!(got.power, Some(printed(4)));
    assert_eq!(got.toughness, Some(printed(5)));
    assert_eq!(got.card_types, set(&["Artifact"]));
    // {X}{2}{U}: X = 0.
    assert_eq!(mp.mana_value(v()), Ok(3));
    // Stack query: still the battlefield.
    let stack = MergedComponent {
        query: LayoutQuery::new(ZoneKind::Stack),
        ..plain(&st)
    };
    let mp = MergedPermanent::new(vec![stack, plain(&b)], false).unwrap();
    assert_eq!(
        mp.base_characteristics(v()).unwrap().power,
        Some(printed(4))
    );

    // Preparation on top: its normal characteristics.
    let mp = MergedPermanent::new(vec![hand(&pr), plain(&b)], false).unwrap();
    let got = mp.base_characteristics(v()).unwrap();
    assert_eq!(got.names, set(&["test.prep_body"]));
    assert_eq!(got.mana_cost, support::some_cost("{X}{2}{G}"));
    assert_eq!(got.power, Some(printed(3)));
    assert_eq!(mp.mana_value(v()), Ok(3));

    // Meld card on top: its front face.
    let mp = MergedPermanent::new(vec![plain(&me), plain(&b)], false).unwrap();
    let got = mp.base_characteristics(v()).unwrap();
    assert_eq!(got.names, set(&["test.meld_a_front"]));
    assert_eq!(mp.mana_value(v()), Ok(3));

    // Mutate card on top.
    let m = mutant_def();
    let mp = MergedPermanent::new(vec![hand(&m), plain(&b)], false).unwrap();
    assert_chars(
        &mp.base_characteristics(v()).unwrap(),
        &expected_mutant(),
        "mutate",
    );

    // A nonmodal DFC on top with its back face up: the query's status is used.
    let dfc = support::def(LayoutBehavior::DfcNonmodal, vec![dfc_front(), dfc_back()]);
    let back_up = MergedComponent {
        query: LayoutQuery::new(ZoneKind::Hand).with_status(LayoutStatus {
            face_up: FaceUp::Back,
            ..LayoutStatus::default()
        }),
        ..plain(&dfc)
    };
    let mp = MergedPermanent::new(vec![back_up, plain(&b)], false).unwrap();
    assert_eq!(
        mp.base_characteristics(v()).unwrap().names,
        set(&["test.dfc_back"])
    );
    // And a prototype top cast prototyped uses the prototype cost and P/T.
    let proto = sample(LayoutBehavior::Prototype);
    let proto_c = MergedComponent {
        query: LayoutQuery::new(ZoneKind::Battlefield).with_cast(CastChoice::Prototype),
        ..plain(&proto)
    };
    let mp = MergedPermanent::new(vec![proto_c, plain(&b)], false).unwrap();
    let got = mp.base_characteristics(v()).unwrap();
    assert_eq!(got.mana_cost, support::some_cost("{X}{2}{R}{R}"));
    assert_eq!(got.power, Some(printed(3)));
    assert_eq!(mp.mana_value(v()), Ok(4));
}

/// REQ-OBJ-002, CR 731.2a: an unsupported top (Room, Token) is `UnsupportedLayout`; under a
/// supported top an unsupported component matters only when mutated.
#[test]
fn merged_731_2a_unsupported_components() {
    let room = room_sample();
    let b = merge_base_def();
    let room_err = LayoutError::UnsupportedLayout(LayoutBehavior::Room);
    for mutated in [false, true] {
        let mp = MergedPermanent::new(vec![plain(&room), plain(&b)], mutated).unwrap();
        assert_eq!(
            mp.base_characteristics(v()),
            Err(room_err.clone()),
            "{mutated}"
        );
        assert_eq!(mp.mana_value(v()), Err(room_err.clone()), "{mutated}");
    }
    let mp = MergedPermanent::new(vec![plain(&b), plain(&room)], false).unwrap();
    assert_chars(
        &mp.base_characteristics(v()).unwrap(),
        &expected_base(),
        "room under",
    );
    assert_eq!(mp.mana_value(v()), Ok(2));
    let mp = MergedPermanent::new(vec![plain(&b), plain(&room)], true).unwrap();
    assert_eq!(mp.base_characteristics(v()), Err(room_err.clone()));

    let token = with_ids(
        support::def(LayoutBehavior::Token, vec![merge_base_main()]),
        "test.token",
        None,
    );
    let mp = MergedPermanent::new(vec![plain(&token), plain(&b)], false).unwrap();
    assert_eq!(
        mp.base_characteristics(v()),
        Err(LayoutError::UnsupportedLayout(LayoutBehavior::Token))
    );
}

/// REQ-OBJ-002, CR 702.140e, 731.2a: errors of non-top components surface only when mutated,
/// the first in top-to-bottom order; a bad top errors either way.
#[test]
fn merged_component_error_order() {
    let b = merge_base_def();
    let room = room_sample();
    let bad = support::def(LayoutBehavior::Normal, vec![bad_type(merge_base_main())]);
    let obj = LayoutError::Object(ObjectError::UnknownCardType("NotACardType".to_owned()));
    let room_err = LayoutError::UnsupportedLayout(LayoutBehavior::Room);

    let mp = MergedPermanent::new(vec![plain(&b), plain(&room), plain(&bad)], true).unwrap();
    assert_eq!(mp.base_characteristics(v()), Err(room_err));
    let mp = MergedPermanent::new(vec![plain(&b), plain(&bad), plain(&room)], true).unwrap();
    assert_eq!(mp.base_characteristics(v()), Err(obj.clone()));
    let mp = MergedPermanent::new(vec![plain(&b), plain(&bad), plain(&room)], false).unwrap();
    assert_chars(
        &mp.base_characteristics(v()).unwrap(),
        &expected_base(),
        "unmutated",
    );

    for mutated in [false, true] {
        let mp = MergedPermanent::new(vec![plain(&bad), plain(&b)], mutated).unwrap();
        assert_eq!(mp.base_characteristics(v()), Err(obj.clone()), "{mutated}");
    }
}

// ---------------------------------------------------------------------------------------------
// construction, merge, status, leaving
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 731.2: a merged permanent needs at least two components.
#[test]
fn merged_731_2_too_few_components() {
    let b = merge_base_def();
    for mutated in [false, true] {
        assert_eq!(
            MergedPermanent::new(vec![], mutated).err(),
            Some(LayoutError::TooFewComponents { count: 0 })
        );
        assert_eq!(
            MergedPermanent::new(vec![plain(&b)], mutated).err(),
            Some(LayoutError::TooFewComponents { count: 1 })
        );
    }
    assert!(
        !LayoutError::TooFewComponents { count: 1 }
            .to_string()
            .is_empty()
    );
}

/// REQ-OBJ-002, CR 702.140c, 731.2: `merge` over puts the new component on top, under at the
/// bottom; the original is unchanged; the two merges commute.
#[test]
fn merged_702_140c_merge_over_and_under() {
    let n = sample(LayoutBehavior::Normal);
    let m = mutant_def();
    let b = merge_base_def();
    let st = station_def();
    let base = MergedPermanent::new(vec![plain(&m), plain(&b)], false).unwrap();

    let over = base.merge(plain(&n), MergeSide::Over);
    assert_eq!(
        defs_ptr(over.components()),
        vec![&n as *const _, &m as *const _, &b as *const _]
    );
    assert_chars(
        &over.base_characteristics(v()).unwrap(),
        &expected_normal(),
        "over",
    );

    let under = base.merge(plain(&n), MergeSide::Under);
    assert_eq!(
        defs_ptr(under.components()),
        vec![&m as *const _, &b as *const _, &n as *const _]
    );
    assert_chars(
        &under.base_characteristics(v()).unwrap(),
        &expected_mutant(),
        "under",
    );

    // Original unchanged.
    assert_eq!(
        defs_ptr(base.components()),
        vec![&m as *const _, &b as *const _]
    );
    assert_chars(
        &base.base_characteristics(v()).unwrap(),
        &expected_mutant(),
        "base",
    );

    // Over then under, and under then over, give the same stack.
    let a1 = base
        .merge(plain(&n), MergeSide::Over)
        .merge(plain(&st), MergeSide::Under);
    let a2 = base
        .merge(plain(&st), MergeSide::Under)
        .merge(plain(&n), MergeSide::Over);
    let expected = vec![
        &n as *const _,
        &m as *const _,
        &b as *const _,
        &st as *const _,
    ];
    assert_eq!(defs_ptr(a1.components()), expected);
    assert_eq!(defs_ptr(a2.components()), expected);

    // Two overs: the last is on top.
    let twice = base
        .merge(plain(&n), MergeSide::Over)
        .merge(plain(&st), MergeSide::Over);
    assert_eq!(
        defs_ptr(twice.components()),
        vec![
            &st as *const _,
            &n as *const _,
            &m as *const _,
            &b as *const _
        ]
    );
    assert_eq!(MergeSide::Over, MergeSide::Over);
    assert_ne!(MergeSide::Over, MergeSide::Under);
}

/// REQ-OBJ-002, CR 731.2d, 731.2e: token and face-up status come from the topmost component.
#[test]
fn merged_731_2d_token_and_face_up_from_top() {
    let m = mutant_def();
    let b = merge_base_def();
    let rows = [
        // (top token, top face up, bottom token, bottom face up)
        (false, true, true, true),
        (true, true, false, true),
        (true, false, true, true),
        (false, false, false, true),
        (false, true, false, false),
        (true, true, true, false),
    ];
    for (tt, tf, bt, bf) in rows {
        let mp = MergedPermanent::new(vec![comp(&m, tt, tf), comp(&b, bt, bf)], false).unwrap();
        assert_eq!(mp.is_token(), tt, "{tt} {tf} {bt} {bf}");
        assert_eq!(mp.face_up(), tf, "{tt} {tf} {bt} {bf}");
    }
    // After a merge over, the new top decides.
    let mp =
        MergedPermanent::new(vec![comp(&m, false, true), comp(&b, false, true)], false).unwrap();
    let over = mp.merge(comp(&b, true, false), MergeSide::Over);
    assert!(over.is_token());
    assert!(!over.face_up());
    let under = mp.merge(comp(&b, true, false), MergeSide::Under);
    assert!(!under.is_token());
    assert!(under.face_up());
}

/// REQ-OBJ-002, CR 731.3: one permanent leaves; every component, top to bottom, moves.
#[test]
fn merged_731_3_leaving() {
    let n = sample(LayoutBehavior::Normal);
    let m = mutant_def();
    let b = merge_base_def();
    let mp = MergedPermanent::new(
        vec![
            comp(&n, true, true),
            comp(&m, false, false),
            comp(&b, false, true),
        ],
        true,
    )
    .unwrap();
    assert_eq!(mp.object_count(), 1);
    let leaving = mp.components_leaving();
    assert_eq!(
        defs_ptr(&leaving),
        vec![&n as *const _, &m as *const _, &b as *const _]
    );
    let flags: Vec<(bool, bool)> = leaving.iter().map(|c| (c.is_token, c.face_up)).collect();
    assert_eq!(flags, vec![(true, true), (false, false), (false, true)]);
    assert_eq!(defs_ptr(mp.components()), defs_ptr(&leaving));
    let two = MergedPermanent::new(vec![plain(&m), plain(&b)], false).unwrap();
    assert_eq!(two.object_count(), 1);
    assert_eq!(two.components_leaving().len(), 2);
    // Clone and Debug.
    let c = two.clone();
    assert_eq!(defs_ptr(c.components()), defs_ptr(two.components()));
    assert!(!format!("{c:?}").is_empty());
}

/// REQ-OBJ-002, CR 731.2a: results are deterministic.
#[test]
fn merged_deterministic() {
    let n = sample(LayoutBehavior::Normal);
    let m = mutant_def();
    let b = merge_base_def();
    for mutated in [false, true] {
        let mp = MergedPermanent::new(vec![plain(&n), plain(&m), plain(&b)], mutated).unwrap();
        assert_eq!(mp.base_characteristics(v()), mp.base_characteristics(v()));
        assert_eq!(mp.mana_value(v()), mp.mana_value(v()));
    }
}

// ---------------------------------------------------------------------------------------------
// non-wiring (T6)
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002: none of the five new layouts is wired into `layout_for` or the free functions;
/// the four new errors have Display text.
#[test]
fn composite_layouts_not_wired_into_layout_for() {
    for b in [
        LayoutBehavior::Room,
        LayoutBehavior::Meld,
        LayoutBehavior::Station,
        LayoutBehavior::Preparation,
        LayoutBehavior::Mutate,
    ] {
        assert_eq!(
            layout_for(b).err(),
            Some(LayoutError::UnsupportedLayout(b)),
            "{b:?}"
        );
    }
    let d = mutant_def();
    let lq = LayoutQuery::new(ZoneKind::Battlefield);
    let err = LayoutError::UnsupportedLayout(LayoutBehavior::Mutate);
    assert_eq!(base_characteristics(&d, &lq, v()), Err(err.clone()));
    assert_eq!(
        mana_value(&d, &lq, ManaValueContext::OffStack, v()),
        Err(err.clone())
    );
    assert_eq!(gating(&d), Err(err));
    for e in [
        LayoutError::NotMeldPair,
        LayoutError::AlreadyPrepared,
        LayoutError::TooFewComponents { count: 0 },
        LayoutError::InvalidStationSymbols { count: 3 },
    ] {
        assert!(!e.to_string().is_empty(), "{e:?}");
    }
}
