// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the ZoneChange event (REQ-EVENT-001; CR 400.7, 122.2, 603.10a, 608.2h).
//! Black-box: written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_core::events::{
    CounterTarget, DamageTarget, EventError, GameEvent, Outcome, ZoneChangeCause,
};
use fineprint_core::memory::ChoiceValue;
use fineprint_core::permanent::AttachTarget;
use fineprint_core::zones::{MoveRequest, Position, ZoneId, ZoneKind};
use support::*;

fn zc(object: fineprint_base::ids::ObjectId, to: ZoneKind) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause: ZoneChangeCause::Other,
    }
}

fn moved_of(outcome: &Outcome) -> fineprint_core::zones::Moved {
    match outcome {
        Outcome::ZoneChange(m) => *m,
        other => panic!("expected a ZoneChange outcome, got {other:?}"),
    }
}

// REQ-EVENT-001, CR 400.7: a zone change gives the object a new id; the old id is gone from
// the zones; the outcome carries old/new ids and both zones.
#[test]
fn zone_change_gives_a_new_object_id() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let card = w.zones.object(a).unwrap().card;
    let done = w.ok(zc(a, ZoneKind::Graveyard));
    let m = moved_of(&done.outcome);
    assert_eq!(m.old, a);
    assert_ne!(m.new, a);
    assert_eq!(m.from, ZoneId::Battlefield);
    assert_eq!(m.to, ZoneId::Graveyard(P1));
    assert!(w.zones.object(a).is_none());
    let rec = w.zones.object(m.new).expect("new object present");
    assert_eq!(rec.card, card);
    assert_eq!(rec.zone, ZoneId::Graveyard(P1));
    assert_eq!(w.zones.contents(ZoneId::Graveyard(P1)), vec![m.new]);
}

// REQ-EVENT-001, CR 603.10a, 608.2h: leaving a zone captures LKI with the characteristics,
// counters, controller, zone and permanent state the object had immediately before.
#[test]
fn zone_change_captures_lki_from_before_the_move() {
    let mut w = World::new();
    let a = w.create_controlled(ZoneKind::Battlefield, P1, Some(P2));
    let host = w.create(ZoneKind::Battlefield, P2);
    w.provider.set_chars(a, chars("test.alpha_3_4", 3, 4));
    w.provider.set_counter(a, "+1/+1", 2);
    w.permanents.set_tapped(a, true).unwrap();
    w.permanents
        .attach(a, AttachTarget::Permanent(host))
        .unwrap();
    assert!(w.lki.get(a).is_none());

    let done = w.ok(zc(a, ZoneKind::Exile));
    assert!(done.lki_captured);
    let rec = w.lki.get(a).expect("LKI captured");
    assert_eq!(rec.object(), a);
    assert_eq!(rec.owner(), P1);
    assert_eq!(rec.controller(), Some(P2));
    assert_eq!(rec.zone(), ZoneId::Battlefield);
    assert_eq!(rec.characteristics(), &chars("test.alpha_3_4", 3, 4));
    assert_eq!(rec.counters().get("+1/+1"), Some(&2));
    let perm = rec
        .permanent()
        .expect("battlefield permanent state captured");
    assert!(perm.status().tapped);
    assert_eq!(perm.controller(), P2);
    assert_eq!(perm.attached_to(), Some(AttachTarget::Permanent(host)));
}

// REQ-EVENT-001, CR 603.10a: a zone exit from a non-battlefield zone also captures LKI, with no
// permanent state.
#[test]
fn zone_change_from_graveyard_captures_lki_without_permanent_state() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P2);
    let done = w.ok(zc(g, ZoneKind::Exile));
    assert!(done.lki_captured);
    let rec = w.lki.get(g).expect("captured");
    assert_eq!(rec.zone(), ZoneId::Graveyard(P2));
    assert!(rec.permanent().is_none());
    assert_eq!(rec.controller(), None);
}

// REQ-EVENT-001, CR 400.7, 110.1: entering the battlefield creates permanent state (controller
// from the new record, current turn); leaving removes it.
#[test]
fn zone_change_creates_and_removes_permanent_state() {
    let mut w = World::new();
    w.game.set_turn_number(4);
    let g = w.create(ZoneKind::Graveyard, P1);
    let entered = moved_of(&w.ok(zc(g, ZoneKind::Battlefield)).outcome);
    let perm = w.permanents.get(entered.new).expect("permanent created");
    assert_eq!(perm.controller(), P1);
    assert_eq!(perm.controlled_since_turn(), 4);
    assert!(!perm.status().tapped);

    let left = moved_of(&w.ok(zc(entered.new, ZoneKind::Hand)).outcome);
    assert!(w.permanents.get(entered.new).is_none());
    assert!(w.permanents.get(left.new).is_none());
    assert_eq!(w.permanents.len(), 0);
}

// REQ-EVENT-001, CR 110.2: a request with a controller puts the permanent under that player's
// control.
#[test]
fn zone_change_to_battlefield_honours_request_controller() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P1);
    let mut req = MoveRequest::new(ZoneKind::Battlefield);
    req.controller = Some(P2);
    let done = w.ok(GameEvent::ZoneChange {
        object: g,
        request: req,
        cause: ZoneChangeCause::Other,
    });
    let m = moved_of(&done.outcome);
    assert_eq!(w.zones.object(m.new).unwrap().controller, P2);
    assert_eq!(w.zones.object(m.new).unwrap().owner, P1);
    assert_eq!(w.permanents.get(m.new).unwrap().controller(), P2);
}

// REQ-EVENT-001, CR 122.2, 400.7, 120.3e: counters, marked damage and the copy link of the old
// object cease to exist; the new object has none.
#[test]
fn zone_change_forgets_counters_damage_and_copy_link() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let other = w.create(ZoneKind::Graveyard, P2);
    w.ok(GameEvent::AddCounters {
        target: CounterTarget::Object(a),
        kind: "+1/+1".into(),
        amount: 3,
    });
    w.ok(GameEvent::Damage {
        source: None,
        source_controller: None,
        target: DamageTarget::Object(a),
        amount: 2,
        combat: false,
    });
    w.ok(GameEvent::BecomeCopy {
        object: a,
        copied: other,
    });
    assert_eq!(w.events.markers().counter(a, "+1/+1"), 3);
    assert_eq!(w.events.markers().damage(a), 2);
    assert_eq!(w.events.markers().copy_of(a), Some(other));

    let m = moved_of(&w.ok(zc(a, ZoneKind::Graveyard)).outcome);
    for id in [a, m.new] {
        assert!(w.events.markers().counters(id).is_empty());
        assert_eq!(w.events.markers().counter(id, "+1/+1"), 0);
        assert_eq!(w.events.markers().damage(id), 0);
        assert_eq!(w.events.markers().copy_of(id), None);
    }
}

// REQ-EVENT-001, CR 400.8: exile -> exile gives a new id and captures LKI.
#[test]
fn exile_to_exile_gives_a_new_id_and_captures_lki() {
    let mut w = World::new();
    let x = w.create(ZoneKind::Exile, P1);
    let done = w.ok(zc(x, ZoneKind::Exile));
    let m = moved_of(&done.outcome);
    assert_eq!(m.old, x);
    assert_ne!(m.new, x);
    assert_eq!(m.from, ZoneId::Exile);
    assert_eq!(m.to, ZoneId::Exile);
    assert!(done.lki_captured);
    assert!(w.lki.get(x).is_some());
}

// REQ-EVENT-001, CR 400.7: a same-zone reposition keeps the id, captures no LKI, keeps the
// counters, marked damage and permanent state, and still occurs (it is logged).
#[test]
fn same_zone_reposition_keeps_identity_and_markers() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.ok(GameEvent::AddCounters {
        target: CounterTarget::Object(a),
        kind: "test.charge".into(),
        amount: 2,
    });
    w.ok(GameEvent::Damage {
        source: None,
        source_controller: None,
        target: DamageTarget::Object(a),
        amount: 1,
        combat: false,
    });
    w.ok(GameEvent::Tap { object: a });
    let perms_before = bytes(&w.permanents);
    let lki_len = w.lki.len();
    let log_len = w.events.log().len();

    let done = w.ok(zc(a, ZoneKind::Battlefield));
    let m = moved_of(&done.outcome);
    assert_eq!(m.old, a);
    assert_eq!(m.new, a);
    assert!(!done.lki_captured);
    assert_eq!(w.lki.len(), lki_len);
    assert!(w.lki.get(a).is_none());
    assert_eq!(w.events.markers().counter(a, "test.charge"), 2);
    assert_eq!(w.events.markers().damage(a), 1);
    assert_eq!(bytes(&w.permanents), perms_before);
    assert_eq!(w.events.log().len(), log_len + 1);
}

// REQ-EVENT-001, CR 400.7: a library reposition changes the order and keeps the id.
#[test]
fn library_reposition_changes_order() {
    let mut w = World::new();
    let bottom = w.create(ZoneKind::Library, P1);
    let top = w.create(ZoneKind::Library, P1);
    let lib = ZoneId::Library(P1);
    let before = w.zones.contents(lib);
    assert_eq!(before.len(), 2);
    let first = before[0];
    let mut req = MoveRequest::new(ZoneKind::Library);
    req.position = Position::Bottom;
    let done = w.ok(GameEvent::ZoneChange {
        object: first,
        request: req,
        cause: ZoneChangeCause::Other,
    });
    assert!(!done.lki_captured);
    let after = w.zones.contents(lib);
    assert_eq!(after.len(), 2);
    assert_eq!(after[1], first);
    assert!(after.contains(&top) && after.contains(&bottom));
}

// REQ-EVENT-001, CR 400.7, 607.2p: the memory carry rules are exactly MemoryTable's own
// (`apply_zone_change`); the event layer does not change them.
#[test]
fn zone_change_applies_memory_table_rules_unchanged() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    {
        let m = w.memories.get_mut_or_default(a);
        m.set_choice("test.choice", ChoiceValue::Color("green".into()));
        m.set_persistent_choice("test.persistent", ChoiceValue::Color("blue".into()));
    }
    let mut reference = w.memories.clone();
    let done = w.ok(zc(a, ZoneKind::Graveyard));
    let m = moved_of(&done.outcome);
    reference.apply_zone_change(&m);
    assert_eq!(bytes(&w.memories), bytes(&reference));
    assert!(w.memories.get(a).is_none());
    let carried = w.memories.get(m.new).expect("persistent choice carried");
    assert!(carried.choice("test.persistent").is_some());
    assert!(carried.choice("test.choice").is_none());
}

// REQ-EVENT-001: the cause is recorded in the executed event and changes nothing else.
#[test]
fn zone_change_cause_is_recorded_and_changes_nothing() {
    let causes = [
        ZoneChangeCause::Other,
        ZoneChangeCause::Destroy,
        ZoneChangeCause::Sacrifice,
        ZoneChangeCause::Discard,
        ZoneChangeCause::Mill,
        ZoneChangeCause::Exile,
        ZoneChangeCause::CounterSpell,
        ZoneChangeCause::StateBasedAction,
    ];
    let mut reference: Option<Snap> = None;
    for cause in causes {
        let mut w = World::new();
        let a = w.create(ZoneKind::Battlefield, P1);
        let ev = GameEvent::ZoneChange {
            object: a,
            request: MoveRequest::new(ZoneKind::Graveyard),
            cause,
        };
        let done = w.ok(ev.clone());
        assert_eq!(done.instance.event(), &ev);
        assert_eq!(w.events.log().batches()[0].events[0].instance.event(), &ev);
        // Everything but the log (which records the cause) is identical across causes.
        let mut s = w.snap();
        s.events = Vec::new();
        match &reference {
            None => reference = Some(s),
            Some(r) => assert_eq!(&s, r, "cause {cause:?} changed state"),
        }
    }
}

// REQ-EVENT-001: an unknown object is UnknownObject and nothing changes.
#[test]
fn zone_change_of_unknown_object_fails_cleanly() {
    let mut w = World::new();
    w.create(ZoneKind::Battlefield, P1);
    let before = w.snap();
    assert_eq!(
        w.exec(zc(GHOST, ZoneKind::Graveyard)),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(w.snap(), before);
}
