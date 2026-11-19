// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for last known information (REQ-CORE-003): `capture_lki_on_move`, `LkiRecord`
//! and `LkiStore`. Black-box: written from the work-item brief only.

#[path = "spec_memory_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::memory::{
    ChoiceValue, LkiStore, Memory, MemoryError, MemoryTable, SpellHistory, capture_lki_on_move,
};
use fineprint_core::permanent::{AttachTarget, Permanents};
use fineprint_core::state::DesignationValue;
use fineprint_core::zones::{MoveRequest, ZoneId, ZoneKind};
use support::*;

// ---------------------------------------------------------------------------------------------
// (a) immutability
// ---------------------------------------------------------------------------------------------

// REQ-CORE-003, CR 608.2h, 704.8: a captured record never changes: not when the provider's
// data changes, not when the object moves, not when permanents or memories change.
#[test]
fn lki_record_is_immutable_after_capture() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P1);
    w.provider.set_chars(a, chars("test.alpha_2_2", 2, 2));
    w.provider.set_counter(a, "+1/+1", 2);
    w.provider.set_attached(a, vec![b]);
    w.permanents.set_tapped(a, true).unwrap();
    w.memory_mut(a)
        .set_choice("color", ChoiceValue::Color("green".into()));

    let req = MoveRequest::new(ZoneKind::Graveyard);
    assert_eq!(w.capture(a, &req), Ok(true));
    let snapshot = w.lki.get(a).expect("captured").clone();

    // Change everything the record was derived from.
    w.provider.set_chars(a, chars("test.changed_9_9", 9, 9));
    w.provider.set_counter(a, "+1/+1", 7);
    w.provider.set_counter(a, "charge", 1);
    w.provider.set_attached(a, vec![]);
    w.provider.set_fallback(None);
    w.permanents.set_tapped(a, false).unwrap();
    w.permanents
        .set_designation(a, "test.mark", DesignationValue::Flag)
        .unwrap();
    w.memory_mut(a)
        .set_choice("color", ChoiceValue::Color("blue".into()));
    w.memory_mut(a)
        .set_persistent_choice("number", ChoiceValue::Number(4));
    let moved = w
        .zones
        .move_object(&mut w.ids, a, req)
        .expect("move succeeds");
    w.permanents.leave(a).unwrap();
    w.memories.apply_zone_change(&moved);

    let rec = w.lki.get(a).expect("still captured");
    assert_eq!(rec, &snapshot);
    assert_eq!(rec.object(), a);
    assert_eq!(rec.zone(), ZoneId::Battlefield);
    assert_eq!(rec.controller(), Some(P1));
    assert_eq!(rec.characteristics(), &chars("test.alpha_2_2", 2, 2));
    assert_eq!(rec.counters().get("+1/+1"), Some(&2));
    assert_eq!(rec.counters().len(), 1);
    assert_eq!(rec.attached_here(), &[b]);
    assert!(rec.permanent().expect("was a permanent").status().tapped);
    assert!(rec.permanent().unwrap().designations().is_empty());
    assert_eq!(
        rec.memory().choice("color"),
        Some(&ChoiceValue::Color("green".into()))
    );
    assert_eq!(rec.memory().choice("number"), None);
}

// ---------------------------------------------------------------------------------------------
// (b) fidelity
// ---------------------------------------------------------------------------------------------

// REQ-CORE-003, CR 608.2h: every field of the record matches the object immediately before the
// event, on the battlefield (rules-level controller Some, permanent state held).
#[test]
fn lki_fidelity_battlefield_object() {
    let mut w = World::new();
    let x = w.create(ZoneKind::Battlefield, P2);
    let y = w.create(ZoneKind::Battlefield, P1);
    let z = w.create(ZoneKind::Battlefield, P1);
    let a = w
        .zones
        .create_object(
            &mut w.ids,
            cid(77),
            P1,
            MoveRequest {
                controller: Some(P2),
                face_down: true,
                ..MoveRequest::new(ZoneKind::Battlefield)
            },
        )
        .unwrap();
    w.permanents.enter(a, P2, 3).unwrap();
    w.permanents.set_tapped(a, true).unwrap();
    w.permanents
        .set_designation(a, "test.flag", DesignationValue::Int(5))
        .unwrap();
    w.permanents
        .attach(y, AttachTarget::Permanent(a))
        .expect("attach");
    w.provider.set_chars(a, chars("test.target_3_4", 3, 4));
    w.provider.set_counter(a, "+1/+1", 3);
    w.provider.set_counter(a, "charge", 1);
    // Unsorted from the provider; the record lists them ascending.
    w.provider.set_attached(a, vec![z, x, y]);
    let history = SpellHistory {
        costs_paid: vec!["test.cost".into()],
        x_value: Some(2),
        ..SpellHistory::default()
    };
    w.memory_mut(a).set_spell_history(history.clone());
    w.memory_mut(a)
        .set_choice("type", ChoiceValue::CreatureType("test_kind".into()));
    w.memory_mut(a)
        .set_persistent_choice("player", ChoiceValue::Player(P1));
    let expected_perm = w.permanents.get(a).unwrap().clone();
    let expected_mem = w.memories.get(a).unwrap().clone();

    assert_eq!(w.capture(a, &MoveRequest::new(ZoneKind::Exile)), Ok(true));
    let rec = w.lki.get(a).unwrap();
    assert_eq!(rec.object(), a);
    assert_eq!(rec.card(), cid(77));
    assert_eq!(rec.owner(), P1);
    assert_eq!(rec.controller(), Some(P2));
    assert_eq!(rec.zone(), ZoneId::Battlefield);
    assert!(rec.face_down());
    assert_eq!(rec.characteristics(), &chars("test.target_3_4", 3, 4));
    let mut counters = BTreeMap::new();
    counters.insert("+1/+1".to_string(), 3);
    counters.insert("charge".to_string(), 1);
    assert_eq!(rec.counters(), &counters);
    let mut sorted = vec![x, y, z];
    sorted.sort();
    assert_eq!(rec.attached_here(), sorted.as_slice());
    let perm = rec.permanent().expect("battlefield permanent");
    assert_eq!(perm, &expected_perm);
    assert!(perm.status().tapped);
    assert_eq!(perm.controller(), P2);
    assert_eq!(
        perm.designation("test.flag"),
        Some(&DesignationValue::Int(5))
    );
    assert_eq!(rec.memory(), &expected_mem);
    assert_eq!(rec.memory().spell_history(), Some(&history));
    assert_eq!(
        rec.memory().choice("type"),
        Some(&ChoiceValue::CreatureType("test_kind".into()))
    );
    assert_eq!(
        rec.memory().choice("player"),
        Some(&ChoiceValue::Player(P1))
    );
}

// REQ-CORE-003, CR 109.4, 608.2h: outside the battlefield and the stack the rules-level
// controller is None, there is no permanent, and an object without Memory has an empty one.
#[test]
fn lki_fidelity_graveyard_object_without_memory() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P2);
    assert!(w.memories.get(g).is_none());
    assert_eq!(w.capture(g, &MoveRequest::new(ZoneKind::Exile)), Ok(true));
    let rec = w.lki.get(g).unwrap();
    assert_eq!(rec.object(), g);
    assert_eq!(rec.owner(), P2);
    assert_eq!(rec.controller(), None);
    assert_eq!(rec.zone(), ZoneId::Graveyard(P2));
    assert!(!rec.face_down());
    assert!(rec.permanent().is_none());
    assert_eq!(rec.memory(), &Memory::new());
    assert!(rec.counters().is_empty());
    assert!(rec.attached_here().is_empty());
    assert_eq!(rec.characteristics(), &chars("test.vanilla_1_1", 1, 1));
}

// REQ-CORE-003, CR 109.4, 405.4: a spell on the stack has a rules-level controller; it is not
// a permanent.
#[test]
fn lki_fidelity_stack_object() {
    let mut w = World::new();
    let s = w
        .zones
        .create_object(
            &mut w.ids,
            cid(5),
            P1,
            MoveRequest {
                controller: Some(P2),
                ..MoveRequest::new(ZoneKind::Stack)
            },
        )
        .unwrap();
    assert_eq!(
        w.capture(s, &MoveRequest::new(ZoneKind::Battlefield)),
        Ok(true)
    );
    let rec = w.lki.get(s).unwrap();
    assert_eq!(rec.controller(), Some(P2));
    assert_eq!(rec.owner(), P1);
    assert_eq!(rec.zone(), ZoneId::Stack);
    assert!(rec.permanent().is_none());
}

// REQ-CORE-003: a provider that only implements `characteristics` gives no counters and no
// attachments (trait defaults).
#[test]
fn lki_provider_defaults_give_no_counters_or_attachments() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let p = MinimalProvider(chars("test.plain_1_2", 1, 2));
    let mut lki = LkiStore::new();
    let r = capture_lki_on_move(
        &w.zones,
        &w.permanents,
        &w.memories,
        &p,
        &mut lki,
        a,
        &MoveRequest::new(ZoneKind::Hand),
    );
    assert_eq!(r, Ok(true));
    let rec = lki.get(a).unwrap();
    assert!(rec.counters().is_empty());
    assert!(rec.attached_here().is_empty());
    assert_eq!(rec.characteristics(), &chars("test.plain_1_2", 1, 2));
}

// ---------------------------------------------------------------------------------------------
// (c) capture matrix
// ---------------------------------------------------------------------------------------------

// REQ-CORE-003, CR 400.7, 400.8, 406.7, 400.10: every ordered pair of zone kinds: a move to a
// different zone captures (keyed by the old id); exile->exile and command->command capture; a
// same-zone reposition in library, hand, graveyard, battlefield or stack stores nothing.
#[test]
fn lki_capture_matrix_all_zone_pairs() {
    let mut checked = 0;
    for from in ZoneKind::ALL {
        for to in ZoneKind::ALL {
            let mut w = World::new();
            let id = w.create(from, P1);
            let req = MoveRequest::new(to);
            // Skip pairs the Zones API rejects (checked on a clone, before capturing).
            let mut probe = w.zones.clone();
            let mut probe_ids = w.ids.clone();
            if probe.move_object(&mut probe_ids, id, req).is_err() {
                continue;
            }
            checked += 1;
            let expected = expect_capture(from, to);
            assert_eq!(w.capture(id, &req), Ok(expected), "{from:?} -> {to:?}");
            if expected {
                assert_eq!(w.lki.len(), 1, "{from:?} -> {to:?}");
                assert_eq!(w.lki.ids(), vec![id]);
                let rec = w.lki.get(id).expect("keyed by old id");
                assert_eq!(rec.object(), id);
                assert_eq!(rec.zone().kind(), from);
                assert_eq!(rec.permanent().is_some(), from == ZoneKind::Battlefield);
                assert_eq!(
                    rec.controller().is_some(),
                    matches!(from, ZoneKind::Battlefield | ZoneKind::Stack)
                );
                let moved = w.zones.move_object(&mut w.ids, id, req).unwrap();
                assert_ne!(moved.old, moved.new);
                assert_eq!(moved.old, id);
                assert!(w.lki.get(moved.new).is_none());
            } else {
                assert!(w.lki.is_empty(), "{from:?} -> {to:?}");
                assert_eq!(w.lki.len(), 0);
                assert!(w.lki.ids().is_empty());
                assert!(w.lki.get(id).is_none());
                let moved = w.zones.move_object(&mut w.ids, id, req).unwrap();
                assert_eq!(moved.old, moved.new);
            }
        }
    }
    assert_eq!(checked, 49, "the Zones API accepts every plain request");
}

// REQ-CORE-003, CR 400.7, 109.4: a request that names a controller for the battlefield is a
// zone exit like any other; the record holds the pre-event (hand: no) controller.
#[test]
fn lki_capture_with_controller_request_to_battlefield() {
    let mut w = World::new();
    let h = w.create(ZoneKind::Hand, P1);
    let req = MoveRequest {
        controller: Some(P2),
        ..MoveRequest::new(ZoneKind::Battlefield)
    };
    assert_eq!(w.capture(h, &req), Ok(true));
    assert_eq!(w.lki.get(h).unwrap().controller(), None);
}

// REQ-CORE-003, CR 608.2h: public -> hidden moves are captured.
#[test]
fn lki_public_to_hidden_moves_are_captured() {
    let pairs = [
        (ZoneKind::Battlefield, ZoneKind::Hand),
        (ZoneKind::Battlefield, ZoneKind::Library),
        (ZoneKind::Graveyard, ZoneKind::Library),
        (ZoneKind::Exile, ZoneKind::Hand),
    ];
    for (from, to) in pairs {
        let mut w = World::new();
        let id = w.create(from, P2);
        let moved = w.move_to(id, to);
        assert_eq!(moved.old, id);
        let rec = w.lki.get(id).expect("captured");
        assert_eq!(rec.zone().kind(), from, "{from:?} -> {to:?}");
        assert_eq!(rec.owner(), P2);
    }
}

// REQ-CORE-003, CR 400.7, 400.5: same-zone repositions, including to another position or face
// down, are not zone exits.
#[test]
fn lki_same_zone_reposition_stores_nothing() {
    for kind in [
        ZoneKind::Library,
        ZoneKind::Hand,
        ZoneKind::Graveyard,
        ZoneKind::Battlefield,
        ZoneKind::Stack,
    ] {
        let mut w = World::new();
        let _other = w.create(kind, P1);
        let id = w.create(kind, P1);
        let req = MoveRequest {
            position: fineprint_core::zones::Position::Bottom,
            face_down: true,
            ..MoveRequest::new(kind)
        };
        assert_eq!(w.capture(id, &req), Ok(false), "{kind:?}");
        assert!(w.lki.is_empty());
    }
}

// REQ-CORE-003, CR 400.7: ids() lists records ascending whatever the capture order.
#[test]
fn lki_ids_are_ascending() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Hand, P1);
    let b = w.create(ZoneKind::Graveyard, P1);
    let c = w.create(ZoneKind::Battlefield, P2);
    w.capture(c, &MoveRequest::new(ZoneKind::Exile)).unwrap();
    w.capture(a, &MoveRequest::new(ZoneKind::Stack)).unwrap();
    w.capture(b, &MoveRequest::new(ZoneKind::Exile)).unwrap();
    let mut expect = vec![a, b, c];
    expect.sort();
    assert_eq!(w.lki.ids(), expect);
    assert_eq!(w.lki.len(), 3);
    assert!(!w.lki.is_empty());
}

// ---------------------------------------------------------------------------------------------
// (d) errors
// ---------------------------------------------------------------------------------------------

// REQ-CORE-003, REQ-CORE-001: an id in no zone is UnknownObject; the store is unchanged.
#[test]
fn lki_error_unknown_object() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Hand, P1);
    w.capture(a, &MoveRequest::new(ZoneKind::Graveyard))
        .unwrap();
    let before = w.lki.clone();
    let ghost = oid(9_999);
    assert_eq!(
        w.capture(ghost, &MoveRequest::new(ZoneKind::Graveyard)),
        Err(MemoryError::UnknownObject(ghost))
    );
    assert_eq!(w.lki, before);
    // An id that has left (old id after a move) is also unknown.
    let moved = w
        .zones
        .move_object(&mut w.ids, a, MoveRequest::new(ZoneKind::Graveyard))
        .unwrap();
    let before = w.lki.clone();
    // `a` is no longer in any zone, so UnknownObject takes precedence over AlreadyCaptured
    // (the zone lookup runs before the already-captured check); the store must not change.
    assert_eq!(moved.old, a);
    assert_eq!(
        w.capture(moved.old, &MoveRequest::new(ZoneKind::Exile)),
        Err(MemoryError::UnknownObject(a))
    );
    assert_eq!(w.lki, before);
}

// REQ-CORE-003: a second capture of the same id is AlreadyCaptured; records are never
// overwritten.
#[test]
fn lki_error_already_captured() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.provider.set_chars(a, chars("test.first_1_1", 1, 1));
    assert_eq!(
        w.capture(a, &MoveRequest::new(ZoneKind::Graveyard)),
        Ok(true)
    );
    let before = w.lki.clone();
    w.provider.set_chars(a, chars("test.second_5_5", 5, 5));
    assert_eq!(
        w.capture(a, &MoveRequest::new(ZoneKind::Exile)),
        Err(MemoryError::AlreadyCaptured(a))
    );
    assert_eq!(w.lki, before);
    assert_eq!(
        w.lki.get(a).unwrap().characteristics(),
        &chars("test.first_1_1", 1, 1)
    );
}

// REQ-CORE-003: a provider without characteristics gives NoCharacteristics; atomic.
#[test]
fn lki_error_no_characteristics() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P1);
    w.capture(b, &MoveRequest::new(ZoneKind::Graveyard))
        .unwrap();
    let empty = TestProvider::empty();
    let before = w.lki.clone();
    let r = capture_lki_on_move(
        &w.zones,
        &w.permanents,
        &w.memories,
        &empty,
        &mut w.lki,
        a,
        &MoveRequest::new(ZoneKind::Graveyard),
    );
    assert_eq!(r, Err(MemoryError::NoCharacteristics(a)));
    assert_eq!(w.lki, before);
    assert!(w.lki.get(a).is_none());
}

// REQ-CORE-003: MemoryError is a std::error::Error with a non-empty Display.
#[test]
fn lki_memory_error_display_and_error_trait() {
    let errs = [
        MemoryError::UnknownObject(oid(1)),
        MemoryError::AlreadyCaptured(oid(1)),
        MemoryError::NoCharacteristics(oid(1)),
    ];
    for e in &errs {
        let as_err: &dyn std::error::Error = e;
        assert!(!as_err.to_string().is_empty());
        assert_eq!(e.clone(), *e);
    }
    assert_ne!(errs[0], errs[1]);
    assert_ne!(errs[1], errs[2]);
    assert_ne!(errs[0], errs[2]);
}

// ---------------------------------------------------------------------------------------------
// (e) CR 704.8: simultaneous exits
// ---------------------------------------------------------------------------------------------

// REQ-CORE-003, CR 704.8: two permanents leave at once: both are captured before either
// moves, so both records show the pre-event state, including attachments.
#[test]
fn lki_704_8_simultaneous_exits_see_pre_event_state() {
    let mut w = World::new();
    let host = w.create(ZoneKind::Battlefield, P1);
    let aura = w.create(ZoneKind::Battlefield, P1);
    w.permanents
        .attach(aura, AttachTarget::Permanent(host))
        .unwrap();
    w.provider.set_attached(host, vec![aura]);
    w.provider.set_counter(host, "-1/-1", 2);
    w.provider.set_chars(host, chars("test.host_2_2", 2, 2));
    w.provider.set_chars(aura, chars("test.aura_0_0", 0, 0));

    let req = MoveRequest::new(ZoneKind::Graveyard);
    assert_eq!(w.capture(host, &req), Ok(true));
    assert_eq!(w.capture(aura, &req), Ok(true));

    // Perform both moves; the provider now reflects the post-event world.
    let m1 = w.zones.move_object(&mut w.ids, host, req).unwrap();
    w.permanents.leave(host).unwrap();
    w.provider.set_attached(host, vec![]);
    w.provider.clear_counters(host);
    let m2 = w.zones.move_object(&mut w.ids, aura, req).unwrap();
    w.permanents.leave(aura).unwrap();
    w.memories.apply_zone_change(&m1);
    w.memories.apply_zone_change(&m2);

    let rh = w.lki.get(host).unwrap();
    let ra = w.lki.get(aura).unwrap();
    assert_eq!(rh.attached_here(), &[aura]);
    assert_eq!(rh.counters().get("-1/-1"), Some(&2));
    assert_eq!(rh.zone(), ZoneId::Battlefield);
    assert_eq!(ra.zone(), ZoneId::Battlefield);
    assert_eq!(
        ra.permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(host))
    );
    assert_eq!(rh.characteristics(), &chars("test.host_2_2", 2, 2));
    assert_eq!(ra.characteristics(), &chars("test.aura_0_0", 0, 0));
    assert_eq!(w.lki.len(), 2);
}

// ---------------------------------------------------------------------------------------------
// (f) borrowed state is not mutated
// ---------------------------------------------------------------------------------------------

// REQ-CORE-003, REQ-CORE-001: capture only reads Zones, Permanents and MemoryTable.
#[test]
fn lki_capture_does_not_mutate_borrowed_state() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.memory_mut(a).set_choice("mode", ChoiceValue::Mode(1));
    let g = w.create(ZoneKind::Graveyard, P2);
    w.provider.set_attached(a, vec![g]);
    let zones = w.zones.clone();
    let perms: Permanents = w.permanents.clone();
    let mems: MemoryTable = w.memories.clone();
    assert_eq!(w.capture(a, &MoveRequest::new(ZoneKind::Exile)), Ok(true));
    assert_eq!(
        w.capture(g, &MoveRequest::new(ZoneKind::Graveyard)),
        Ok(false)
    );
    assert_eq!(w.capture(g, &MoveRequest::new(ZoneKind::Hand)), Ok(true));
    assert_eq!(w.zones, zones);
    assert_eq!(w.permanents, perms);
    assert_eq!(w.memories, mems);
}
