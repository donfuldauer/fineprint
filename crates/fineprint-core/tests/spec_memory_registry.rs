// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the CR 400.7 exception registry bodies of 400.7d and 400.7j and their link
//! to the memory behaviour (REQ-CORE-002, REQ-CORE-007). Black-box: written from the work-item
//! brief only.

#[path = "spec_memory_support.rs"]
mod support;

use fineprint_core::memory::{DurableRef, MoveFinder, SpellHistory};
use fineprint_core::zones::{EXCEPTIONS, Exception400_7, ExceptionOutcome, ZoneKind};
use support::*;

const EXPECTED: [(Exception400_7, &str); 12] = [
    (Exception400_7::A, "400.7a"),
    (Exception400_7::B, "400.7b"),
    (Exception400_7::C, "400.7c"),
    (Exception400_7::D, "400.7d"),
    (Exception400_7::E, "400.7e"),
    (Exception400_7::F, "400.7f"),
    (Exception400_7::G, "400.7g"),
    (Exception400_7::H, "400.7h"),
    (Exception400_7::I, "400.7i"),
    (Exception400_7::J, "400.7j"),
    (Exception400_7::K, "400.7k"),
    (Exception400_7::M, "400.7m"),
];

const EXPECTED_NAMES: [&str; 12] = [
    "permanent_spell_effects",
    "static_granted_abilities",
    "prevention_effects",
    "spell_information",
    "leaves_zone_triggers",
    "aura_leaves_triggers",
    "granted_cast_ability",
    "cast_effect_finds_spell",
    "play_land_effect_finds_permanent",
    "effect_finds_moved_object",
    "madness_finds_card",
    "stickers_retained",
];

fn outcome_of(
    e: Exception400_7,
) -> fn(fineprint_base::ids::ObjectId, fineprint_base::ids::ObjectId) -> ExceptionOutcome {
    EXCEPTIONS
        .iter()
        .find(|entry| entry.exception == e)
        .expect("registered")
        .apply
}

// REQ-CORE-002, CR 400.7a-400.7k, 400.7m: still twelve entries in order, names and rule
// strings unchanged.
#[test]
fn registry_twelve_entries_in_order_names_unchanged() {
    assert_eq!(EXCEPTIONS.len(), 12);
    for (i, entry) in EXCEPTIONS.iter().enumerate() {
        assert_eq!(entry.exception, EXPECTED[i].0);
        assert_eq!(entry.exception.rule(), EXPECTED[i].1);
        assert_eq!(entry.exception.name(), EXPECTED_NAMES[i]);
    }
}

// REQ-CORE-002, REQ-CORE-007, CR 400.7d, 400.7j: D and J apply; the other ten are not
// implemented and name themselves.
#[test]
fn registry_d_and_j_apply_others_not_implemented() {
    for (old, new) in [(1u64, 2u64), (7, 7), (100, 3)] {
        for entry in EXCEPTIONS.iter() {
            let got = (entry.apply)(oid(old), oid(new));
            let want = match entry.exception {
                Exception400_7::D | Exception400_7::J => ExceptionOutcome::Applies(entry.exception),
                other => ExceptionOutcome::NotImplemented(other),
            };
            assert_eq!(got, want, "{:?}", entry.exception);
        }
    }
    assert_ne!(
        ExceptionOutcome::Applies(Exception400_7::D),
        ExceptionOutcome::NotImplemented(Exception400_7::D)
    );
}

// REQ-CORE-002, REQ-CORE-007, CR 400.7d: scenario: after a real stack -> battlefield move the
// registry entry D applies and the permanent's Memory holds the spell's history.
#[test]
fn registry_d_scenario_spell_history_carried() {
    let mut w = World::new();
    let h = w.create(ZoneKind::Hand, P1);
    let spell = w.move_to(h, ZoneKind::Stack).new;
    let history = SpellHistory {
        x_value: Some(5),
        cast_face: Some("test.back_face".into()),
        ..SpellHistory::default()
    };
    w.memory_mut(spell).set_spell_history(history.clone());
    let moved = w.move_to(spell, ZoneKind::Battlefield);
    assert_eq!(
        (outcome_of(Exception400_7::D))(moved.old, moved.new),
        ExceptionOutcome::Applies(Exception400_7::D)
    );
    assert_eq!(
        w.memories.get(moved.new).unwrap().spell_history(),
        Some(&history)
    );
}

// REQ-CORE-002, REQ-CORE-007, CR 400.7j: scenario: after an effect exiles an object, entry J
// applies and that effect's MoveFinder finds the object.
#[test]
fn registry_j_scenario_effect_finds_moved_object() {
    let mut w = World::new();
    let b = w.create(ZoneKind::Battlefield, P1);
    let r = DurableRef::new(&w.zones, b).unwrap();
    let mut finder = MoveFinder::new();
    let moved = w.move_to(b, ZoneKind::Exile);
    assert!(finder.record(&moved));
    assert_eq!(
        (outcome_of(Exception400_7::J))(moved.old, moved.new),
        ExceptionOutcome::Applies(Exception400_7::J)
    );
    assert_eq!(finder.find(b), Some(moved.new));
    assert_eq!(
        finder.resolve(&r, &w.zones, Some(ZoneKind::Exile)),
        Some(moved.new)
    );
    assert_eq!(r.resolve(&w.zones, None), None);
}
