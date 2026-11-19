// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the STATION layout of `fineprint_core::layouts` (REQ-OBJ-002). Black-box:
//! written from the work-item brief only.
//!
//! A station card has its printed characteristics, but no power or toughness outside the
//! battlefield, the stack included (CR 722.2c); unstriated abilities are normal (CR 722.4); one
//! or two station symbols (CR 722.1), each active on the battlefield with N or more charge
//! counters (CR 722.2a, 722.2b).

#[path = "spec_layouts_composite_support.rs"]
mod composite;

use composite::support::{
    self, Cast, St, ability, assert_chars, blank, on_stack, printed, query, set, text,
};
use composite::{bad_type, station_def, station_main, with_ids};
use fineprint_core::layouts::{
    Layout, LayoutError, LayoutQuery, StationLayout, StationSymbol, active_station_symbols,
    base_characteristics, gating, layout_for, mana_value,
};
use fineprint_core::objects::{Characteristics, ManaValueContext, ObjectError};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{CardDefinitionDSL, ColorBitmask, FaceKind, LayoutBehavior};

fn v() -> &'static fineprint_dsl::vocab::RulesVocabulary {
    support::vocab()
}

/// The literal printed characteristics of the sample station card.
fn expected_printed() -> Characteristics {
    Characteristics {
        names: set(&["test.station_hull"]),
        mana_cost: support::some_cost("{X}{2}{U}"),
        color_indicator: None,
        colors: ColorBitmask::BLUE,
        supertypes: set(&[]),
        card_types: set(&["Artifact"]),
        subtypes: set(&["Spacecraft"]),
        rules_text: text(&["test.station_hull"]),
        abilities: vec![ability("station", 1), ability("test.station_hull", 1)],
        power: Some(printed(4)),
        toughness: Some(printed(5)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn expected_no_pt() -> Characteristics {
    Characteristics {
        power: None,
        toughness: None,
        ..expected_printed()
    }
}

/// REQ-OBJ-002, CR 722.2c, 722.4: printed P/T on the battlefield only; in hand, library,
/// graveyard, exile, command and on the stack no P/T; everything else (unstriated abilities
/// included) printed in every zone; cast and status ignored.
#[test]
fn station_722_2c_pt_only_on_battlefield() {
    let rows = [
        (ZoneKind::Battlefield, expected_printed()),
        (ZoneKind::Hand, expected_no_pt()),
        (ZoneKind::Library, expected_no_pt()),
        (ZoneKind::Graveyard, expected_no_pt()),
        (ZoneKind::Exile, expected_no_pt()),
        (ZoneKind::Command, expected_no_pt()),
        (ZoneKind::Stack, expected_no_pt()),
    ];
    assert_eq!(rows.len(), ZoneKind::ALL.len());
    let d = station_def();
    for (zone, expected) in &rows {
        for cast in Cast::ALL {
            for st in St::all() {
                let lq = query(*zone, cast, st);
                let got = StationLayout
                    .base_characteristics(&d, &lq, v())
                    .expect("station chars");
                let ctx = format!("{zone:?} {cast:?} {st:?}");
                assert_chars(&got, expected, &ctx);
                assert!(got.abilities.contains(&ability("station", 1)), "{ctx}");
            }
        }
    }
    assert_eq!(StationLayout.behavior(), LayoutBehavior::Station);
}

/// REQ-OBJ-002, CR 722.2c: the stack has no P/T either (the CR overrides the SRS "Printed" cell).
#[test]
fn station_722_2c_no_pt_on_stack() {
    let lq = LayoutQuery::new(ZoneKind::Stack);
    let got = StationLayout
        .base_characteristics(&station_def(), &lq, v())
        .unwrap();
    assert_eq!(got.power, None);
    assert_eq!(got.toughness, None);
}

/// REQ-OBJ-002, CR 722.2c: a station card without printed P/T has none anywhere.
#[test]
fn station_without_printed_pt() {
    let d = with_ids(
        support::def(
            LayoutBehavior::Station,
            vec![fineprint_dsl::card::CardFaceDSL {
                power: None,
                toughness: None,
                ..station_main()
            }],
        ),
        "test.station_bare",
        None,
    );
    for zone in ZoneKind::ALL {
        let got = StationLayout
            .base_characteristics(&d, &LayoutQuery::new(zone), v())
            .unwrap();
        assert_chars(&got, &expected_no_pt(), &format!("{zone:?}"));
    }
}

/// REQ-OBJ-002, CR 722.4: mana value from the printed cost; X is 0 off the stack.
#[test]
fn station_mana_value_x() {
    let d = station_def();
    for zone in ZoneKind::ALL {
        let lq = LayoutQuery::new(zone);
        assert_eq!(
            StationLayout.mana_value(&d, &lq, ManaValueContext::OffStack, v()),
            Ok(3)
        );
        let expected = if zone == ZoneKind::Stack { 5 } else { 3 };
        assert_eq!(
            StationLayout.mana_value(&d, &lq, on_stack(2, 9, 9), v()),
            Ok(expected),
            "{zone:?}"
        );
    }
}

/// REQ-OBJ-002, CR 722.4: no gating data; layout and face errors; type-line error.
#[test]
fn station_errors_and_gating() {
    assert_eq!(StationLayout.gating(&station_def()), Ok(None));
    let lq = LayoutQuery::new(ZoneKind::Battlefield);
    let mut wrong = station_def();
    wrong.layout = LayoutBehavior::Leveler;
    let mismatch = LayoutError::LayoutMismatch {
        expected: LayoutBehavior::Station,
        found: LayoutBehavior::Leveler,
    };
    assert_eq!(
        StationLayout.base_characteristics(&wrong, &lq, v()),
        Err(mismatch.clone())
    );
    assert_eq!(StationLayout.gating(&wrong), Err(mismatch.clone()));
    let wrong_bad = support::def(LayoutBehavior::Leveler, vec![]);
    assert_eq!(
        StationLayout.base_characteristics(&wrong_bad, &lq, v()),
        Err(mismatch)
    );
    let bad = LayoutError::BadFaces {
        layout: LayoutBehavior::Station,
    };
    let shapes: Vec<CardDefinitionDSL> = vec![
        support::def(LayoutBehavior::Station, vec![]),
        support::def(
            LayoutBehavior::Station,
            vec![station_main(), station_main()],
        ),
        support::def(
            LayoutBehavior::Station,
            vec![station_main(), blank(FaceKind::Back, "test.station_back")],
        ),
        support::def(
            LayoutBehavior::Station,
            vec![blank(FaceKind::PrepareSpell, "test.station_other")],
        ),
    ];
    for d in &shapes {
        for zone in ZoneKind::ALL {
            let lq = LayoutQuery::new(zone);
            assert_eq!(
                StationLayout.base_characteristics(d, &lq, v()),
                Err(bad.clone())
            );
            assert_eq!(
                StationLayout.mana_value(d, &lq, ManaValueContext::OffStack, v()),
                Err(bad.clone())
            );
        }
        assert_eq!(StationLayout.gating(d), Err(bad.clone()));
    }
    let d = support::def(LayoutBehavior::Station, vec![bad_type(station_main())]);
    for zone in ZoneKind::ALL {
        assert_eq!(
            StationLayout.base_characteristics(&d, &LayoutQuery::new(zone), v()),
            Err(LayoutError::Object(ObjectError::UnknownCardType(
                "NotACardType".to_owned()
            )))
        );
    }
}

// ---------------------------------------------------------------------------------------------
// station symbols
// ---------------------------------------------------------------------------------------------

fn sym(threshold: i64, key: &str, pt: Option<(i64, i64)>) -> StationSymbol {
    StationSymbol {
        threshold,
        abilities: vec![ability(key, threshold)],
        power_toughness: pt.map(|(p, t)| (printed(p), printed(t))),
    }
}

fn thresholds(active: &[&StationSymbol]) -> Vec<i64> {
    active.iter().map(|s| s.threshold).collect()
}

/// REQ-OBJ-002, CR 722.1: one or two station symbols; zero or three is an error, checked first
/// (in every zone).
#[test]
fn station_722_1_symbol_count() {
    let three = vec![
        sym(1, "test.st_a", None),
        sym(2, "test.st_b", None),
        sym(3, "test.st_c", Some((1, 1))),
    ];
    for zone in ZoneKind::ALL {
        for counters in [i64::MIN, -1, 0, 5, i64::MAX] {
            assert_eq!(
                active_station_symbols(&[], zone, counters),
                Err(LayoutError::InvalidStationSymbols { count: 0 })
            );
            assert_eq!(
                active_station_symbols(&three, zone, counters),
                Err(LayoutError::InvalidStationSymbols { count: 3 })
            );
        }
    }
    assert!(
        !LayoutError::InvalidStationSymbols { count: 3 }
            .to_string()
            .is_empty()
    );
}

/// REQ-OBJ-002, CR 722.2a, 722.2b: one symbol {4+} on the battlefield is active with 4 or more
/// charge counters only.
#[test]
fn station_722_2a_one_symbol_thresholds() {
    let one = vec![sym(4, "test.st_a", Some((6, 6)))];
    let rows: [(i64, &[i64]); 7] = [
        (3, &[]),
        (4, &[4]),
        (5, &[4]),
        (0, &[]),
        (-1, &[]),
        (i64::MIN, &[]),
        (i64::MAX, &[4]),
    ];
    for (counters, expected) in rows {
        let got = active_station_symbols(&one, ZoneKind::Battlefield, counters).unwrap();
        assert_eq!(thresholds(&got), expected.to_vec(), "{counters}");
        for s in &got {
            assert!(std::ptr::eq(*s, &one[0]));
            assert_eq!(**s, one[0]);
        }
    }
}

/// REQ-OBJ-002, CR 722.2a: a {0+} symbol is active with zero counters but negative counters
/// never activate anything.
#[test]
fn station_722_2a_zero_threshold_and_negative_counters() {
    let zero = vec![sym(0, "test.st_zero", None)];
    assert_eq!(
        thresholds(&active_station_symbols(&zero, ZoneKind::Battlefield, 0).unwrap()),
        vec![0]
    );
    assert_eq!(
        thresholds(&active_station_symbols(&zero, ZoneKind::Battlefield, -1).unwrap()),
        Vec::<i64>::new()
    );
}

/// REQ-OBJ-002, CR 722.2a, 722.2b: two symbols; each active independently, in the given order.
#[test]
fn station_722_2b_two_symbols_table() {
    let two = vec![
        sym(2, "test.st_low", None),
        sym(7, "test.st_high", Some((5, 5))),
    ];
    let rows: [(i64, &[i64]); 9] = [
        (0, &[]),
        (1, &[]),
        (2, &[2]),
        (3, &[2]),
        (6, &[2]),
        (7, &[2, 7]),
        (8, &[2, 7]),
        (-5, &[]),
        (i64::MAX, &[2, 7]),
    ];
    for (counters, expected) in rows {
        let got = active_station_symbols(&two, ZoneKind::Battlefield, counters).unwrap();
        assert_eq!(thresholds(&got), expected.to_vec(), "{counters}");
    }
    // Order preserved: listed high first, both returned high first.
    let rev = vec![
        sym(7, "test.st_high", Some((5, 5))),
        sym(2, "test.st_low", None),
    ];
    let got = active_station_symbols(&rev, ZoneKind::Battlefield, 10).unwrap();
    assert_eq!(thresholds(&got), vec![7, 2]);
    assert!(std::ptr::eq(got[0], &rev[0]));
    assert!(std::ptr::eq(got[1], &rev[1]));
    let got = active_station_symbols(&rev, ZoneKind::Battlefield, 3).unwrap();
    assert_eq!(thresholds(&got), vec![2]);
}

/// REQ-OBJ-002, CR 722.2b: equal thresholds: both active together, both with P/T (no choice is
/// made between them), in the given order.
#[test]
fn station_722_2b_equal_thresholds() {
    let two = vec![
        sym(3, "test.st_a", Some((1, 2))),
        sym(3, "test.st_b", Some((3, 4))),
    ];
    assert!(
        active_station_symbols(&two, ZoneKind::Battlefield, 2)
            .unwrap()
            .is_empty()
    );
    let got = active_station_symbols(&two, ZoneKind::Battlefield, 3).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(*got[0], two[0]);
    assert_eq!(*got[1], two[1]);
}

/// REQ-OBJ-002, CR 722.2a, 722.2c: off the battlefield no symbol is active, whatever the counters.
#[test]
fn station_722_2a_off_battlefield_none_active() {
    let two = vec![
        sym(0, "test.st_low", None),
        sym(1, "test.st_high", Some((5, 5))),
    ];
    for zone in ZoneKind::ALL {
        if zone == ZoneKind::Battlefield {
            continue;
        }
        for counters in [0, 1, 100, i64::MAX] {
            assert!(
                active_station_symbols(&two, zone, counters)
                    .unwrap()
                    .is_empty(),
                "{zone:?} {counters}"
            );
            assert!(
                active_station_symbols(&two[..1], zone, counters)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

/// REQ-OBJ-002, CR 722.2a, 722.2b: a symbol with P/T makes the permanent a creature, one
/// without does not.
#[test]
fn station_722_2b_grants_creature() {
    assert!(sym(5, "test.st_pt", Some((4, 4))).grants_creature());
    assert!(!sym(5, "test.st_plain", None).grants_creature());
    let s = sym(5, "test.st_pt", Some((4, 4)));
    assert_eq!(s.clone(), s);
    assert!(!format!("{s:?}").is_empty());
}

/// REQ-OBJ-002, CR 722.2a: a sweep over counters and zones: the result is exactly the symbols
/// with threshold at most the counters, on the battlefield only (with non-negative counters).
#[test]
fn station_symbol_sweep() {
    let pairs = [(0, 0), (0, 3), (3, 0), (2, 2), (5, 9), (9, 5)];
    for (t1, t2) in pairs {
        let two = vec![
            sym(t1, "test.st_a", None),
            sym(t2, "test.st_b", Some((1, 1))),
        ];
        for zone in ZoneKind::ALL {
            for counters in -2..=11 {
                let got = active_station_symbols(&two, zone, counters).unwrap();
                assert_eq!(got, active_station_symbols(&two, zone, counters).unwrap());
                let expected: Vec<i64> = if zone == ZoneKind::Battlefield && counters >= 0 {
                    two.iter()
                        .map(|s| s.threshold)
                        .filter(|t| *t <= counters)
                        .collect()
                } else {
                    Vec::new()
                };
                assert_eq!(thresholds(&got), expected, "{t1} {t2} {zone:?} {counters}");
            }
        }
    }
}

/// REQ-OBJ-002: STATION is not wired into `layout_for` or the free functions.
#[test]
fn station_not_wired_into_layout_for() {
    let err = LayoutError::UnsupportedLayout(LayoutBehavior::Station);
    assert_eq!(layout_for(LayoutBehavior::Station).err(), Some(err.clone()));
    let d = station_def();
    let lq = LayoutQuery::new(ZoneKind::Battlefield);
    assert_eq!(base_characteristics(&d, &lq, v()), Err(err.clone()));
    assert_eq!(
        mana_value(&d, &lq, ManaValueContext::OffStack, v()),
        Err(err.clone())
    );
    assert_eq!(gating(&d), Err(err));
}
