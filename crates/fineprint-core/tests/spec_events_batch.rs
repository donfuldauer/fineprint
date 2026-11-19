// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for simultaneous batches, AppliedEffects, EventInstance, EventBatch and
//! EventError (REQ-EVENT-001; CR 603.10a, 614.5, 704.3, 704.8). Black-box: written from the
//! work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::{
    AppliedEffects, CounterTarget, DamageTarget, EventBatch, EventError, EventInstance, GameEvent,
    Outcome, ZoneChangeCause,
};
use fineprint_core::memory::MemoryError;
use fineprint_core::permanent::{AttachTarget, PermanentError};
use fineprint_core::state::StateError;
use fineprint_core::zones::{MoveRequest, ZoneError, ZoneId, ZoneKind};
use support::*;

fn zc(object: ObjectId, to: ZoneKind, cause: ZoneChangeCause) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause,
    }
}

// ----- value types ----------------------------------------------------------------------------

// REQ-EVENT-001, CR 614.5: AppliedEffects is a set of effect ids, iterated ascending.
#[test]
fn applied_effects_set() {
    let mut s = AppliedEffects::new();
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert_eq!(s, AppliedEffects::default());
    assert!(s.insert(eid(7)));
    assert!(s.insert(eid(3)));
    assert!(!s.insert(eid(7)), "already present");
    assert_eq!(s.len(), 2);
    assert!(!s.is_empty());
    assert!(s.contains(eid(3)) && s.contains(eid(7)));
    assert!(!s.contains(eid(4)));
    assert_eq!(s.iter().collect::<Vec<_>>(), vec![eid(3), eid(7)]);
    assert_eq!(s, applied(&[7, 3]));
    assert!(applied(&[1]) != applied(&[2]));
}

// REQ-EVENT-001, CR 614.5: an EventInstance carries its event and AppliedEffects.
#[test]
fn event_instance_accessors() {
    let ev = GameEvent::LifeChange {
        player: P1,
        delta: 2,
    };
    let plain = EventInstance::new(ev.clone());
    assert_eq!(plain.event(), &ev);
    assert!(plain.applied_effects().is_empty());
    assert_eq!(EventInstance::from(ev.clone()), plain);
    let with = plain.clone().with_applied(applied(&[5, 9]));
    assert_eq!(with.event(), &ev);
    assert_eq!(with.applied_effects(), &applied(&[5, 9]));
    assert!(with != plain);
}

// REQ-EVENT-001: an EventBatch keeps its events in the given order.
#[test]
fn event_batch_accessors() {
    let a = GameEvent::Draw { player: P1 };
    let b = GameEvent::Draw { player: P2 };
    let empty = EventBatch::new(Vec::new());
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    let batch = EventBatch::from_events(vec![a.clone(), b.clone()]);
    assert_eq!(batch.len(), 2);
    assert!(!batch.is_empty());
    assert_eq!(batch.events()[0].event(), &a);
    assert_eq!(batch.events()[1].event(), &b);
    let same = EventBatch::new(vec![EventInstance::new(a), EventInstance::new(b)]);
    assert_eq!(same, batch);
}

// REQ-EVENT-001: every EventError displays a message and is a std error; wrapped errors convert
// through From.
#[test]
fn event_error_display_and_from() {
    let errors = [
        EventError::UnknownObject(oid(1)),
        EventError::UnknownPlayer(P1),
        EventError::NotOnBattlefield(oid(2)),
        EventError::InvalidAmount(-1),
        EventError::Overflow,
        EventError::Zone(ZoneError::UnknownObject(oid(3))),
        EventError::Permanent(PermanentError::UnknownObject(oid(4))),
        EventError::Memory(MemoryError::UnknownObject(oid(5))),
        EventError::State(StateError::IdExhausted),
    ];
    for e in &errors {
        assert!(!e.to_string().is_empty(), "{e:?}");
        let _: &dyn std::error::Error = e;
    }
    assert_eq!(
        EventError::from(ZoneError::IdExhausted),
        EventError::Zone(ZoneError::IdExhausted)
    );
    assert_eq!(
        EventError::from(PermanentError::SelfAttach(oid(1))),
        EventError::Permanent(PermanentError::SelfAttach(oid(1)))
    );
    assert_eq!(
        EventError::from(MemoryError::UnknownObject(oid(1))),
        EventError::Memory(MemoryError::UnknownObject(oid(1)))
    );
    assert_eq!(
        EventError::from(StateError::IdExhausted),
        EventError::State(StateError::IdExhausted)
    );
}

// ----- all-or-nothing -------------------------------------------------------------------------

/// A world with something of everything, and a batch touching every component.
fn busy_world() -> (World, Vec<GameEvent>) {
    let mut w = World::new();
    for _ in 0..2 {
        w.create(ZoneKind::Library, P1);
    }
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P2);
    let host = w.create(ZoneKind::Battlefield, P2);
    w.memories.get_mut_or_default(b).set_choice(
        "test.choice",
        fineprint_core::memory::ChoiceValue::Color("red".into()),
    );
    let card = w.fresh_card();
    let events = vec![
        GameEvent::Draw { player: P1 },
        GameEvent::LifeChange {
            player: P2,
            delta: -3,
        },
        GameEvent::Damage {
            source: Some(a),
            source_controller: Some(P1),
            target: DamageTarget::Player(P2),
            amount: 2,
            combat: false,
        },
        GameEvent::AddCounters {
            target: CounterTarget::Object(a),
            kind: "test.charge".into(),
            amount: 2,
        },
        GameEvent::AddCounters {
            target: CounterTarget::Player(P1),
            kind: "test.poisonish".into(),
            amount: 1,
        },
        GameEvent::Tap { object: a },
        GameEvent::Attach {
            attachment: a,
            target: AttachTarget::Permanent(host),
        },
        GameEvent::BecomeCopy {
            object: host,
            copied: a,
        },
        zc(b, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
        GameEvent::CreateToken {
            owner: P2,
            card,
            controller: None,
        },
        GameEvent::StepBegins {
            step: fineprint_core::events::Step::Upkeep,
        },
        GameEvent::TurnBegins { player: P2 },
    ];
    (w, events)
}

// REQ-EVENT-001, CR 704.3: a batch whose last event fails restores every component (game,
// zones, id generator, permanents, memories, LKI, EventState incl. history, log and flags);
// a retry without the failing event then succeeds (LKI is captured again).
#[test]
fn failing_last_event_rolls_back_everything() {
    let (mut w, mut events) = busy_world();
    // An earlier logged batch, and a pending empty-library flag, so the log and flags are
    // non-trivial.
    w.ok(GameEvent::LifeChange {
        player: P1,
        delta: 1,
    });
    assert_eq!(w.exec(GameEvent::Draw { player: P2 }), Ok(None));
    let before = w.snap();
    let flags = w.events.empty_library_draws();
    let log_len = w.events.log().len();

    events.push(zc(GHOST, ZoneKind::Exile, ZoneChangeCause::Other));
    assert_eq!(
        w.batch(events.clone()),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(w.snap(), before);
    assert_eq!(w.events.empty_library_draws(), flags);
    assert_eq!(w.events.log().len(), log_len);

    events.pop();
    let n = events.len();
    let done = w.batch(events).expect("retry succeeds");
    assert_eq!(done.events.len(), n);
    assert_eq!(done.index, log_len as u64);
    assert_eq!(w.events.log().len(), log_len + 1);
}

// REQ-EVENT-001: the rollback also covers a failure in the middle of the batch.
#[test]
fn failing_middle_event_rolls_back_everything() {
    let (mut w, mut events) = busy_world();
    let before = w.snap();
    events.insert(
        3,
        GameEvent::Damage {
            source: None,
            source_controller: None,
            target: DamageTarget::Player(P1),
            amount: -4,
            combat: false,
        },
    );
    assert_eq!(w.batch(events), Err(EventError::InvalidAmount(-4)));
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 603.10a, 704.8: simultaneous destruction of two creatures captures both LKI
// records before either moves: each record shows the state from before the batch, including
// the attachment that the other creature's departure would end. Both submission orders.
#[test]
fn simultaneous_destruction_captures_all_lki_first() {
    for host_first in [true, false] {
        let mut w = World::new();
        let host = w.create(ZoneKind::Battlefield, P1);
        let rider = w.create(ZoneKind::Battlefield, P2);
        w.permanents
            .attach(rider, AttachTarget::Permanent(host))
            .unwrap();
        w.permanents.set_tapped(rider, true).unwrap();
        w.provider.set_attached(host, vec![rider]);
        w.provider.set_counter(host, "+1/+1", 1);

        let ev_host = zc(host, ZoneKind::Graveyard, ZoneChangeCause::Destroy);
        let ev_rider = zc(rider, ZoneKind::Graveyard, ZoneChangeCause::Destroy);
        let events = if host_first {
            vec![ev_host, ev_rider]
        } else {
            vec![ev_rider, ev_host]
        };
        let done = w.batch(events).expect("batch succeeds");
        assert_eq!(done.events.len(), 2);
        assert!(done.events.iter().all(|e| e.lki_captured));

        let h = w.lki.get(host).expect("host LKI");
        assert_eq!(h.attached_here(), &[rider]);
        assert_eq!(h.counters().get("+1/+1"), Some(&1));
        assert_eq!(h.zone(), ZoneId::Battlefield);
        assert!(h.permanent().is_some());

        let r = w.lki.get(rider).expect("rider LKI");
        let rp = r.permanent().expect("rider permanent state");
        assert_eq!(
            rp.attached_to(),
            Some(AttachTarget::Permanent(host)),
            "host_first = {host_first}"
        );
        assert!(rp.status().tapped);
        assert_eq!(r.controller(), Some(P2));

        assert_eq!(w.zones.len(ZoneId::Graveyard(P1)), 1);
        assert_eq!(w.zones.len(ZoneId::Graveyard(P2)), 1);
        assert!(w.permanents.is_empty());
    }
}

// REQ-EVENT-001, CR 400.7: an event naming an id that an earlier event of the same batch
// replaced fails with UnknownObject and the batch rolls back.
#[test]
fn stale_id_in_same_batch_fails_and_rolls_back() {
    let stale_followers = |a: ObjectId| {
        vec![
            GameEvent::Tap { object: a },
            zc(a, ZoneKind::Exile, ZoneChangeCause::Other),
        ]
    };
    for i in 0..2 {
        let mut w = World::new();
        let a = w.create(ZoneKind::Battlefield, P1);
        let before = w.snap();
        let follower = stale_followers(a)[i].clone();
        let r = w.batch(vec![
            zc(a, ZoneKind::Graveyard, ZoneChangeCause::Other),
            follower.clone(),
        ]);
        assert_eq!(
            r,
            Err(EventError::UnknownObject(a)),
            "follower {follower:?}"
        );
        assert_eq!(w.snap(), before);
    }
}

// REQ-EVENT-001: events naming the same object are applied in order.
#[test]
fn same_object_events_apply_in_order() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let done = w
        .batch(vec![
            GameEvent::Tap { object: a },
            GameEvent::Untap { object: a },
            GameEvent::Tap { object: a },
        ])
        .unwrap();
    assert_eq!(done.events.len(), 3);
    assert!(w.permanents.get(a).unwrap().status().tapped);
    // The second Tap of [Tap, Tap] finds the permanent already tapped: no event.
    let b = w.create(ZoneKind::Battlefield, P1);
    let done = w
        .batch(vec![
            GameEvent::Tap { object: b },
            GameEvent::Tap { object: b },
        ])
        .unwrap();
    assert_eq!(done.events.len(), 1);
}

// REQ-EVENT-001, CR 120.8, 119.9: a batch where nothing occurs returns an empty list and is not
// logged; it does not advance the index.
#[test]
fn batch_where_nothing_occurs_is_not_logged() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let before = w.snap();
    let done = w
        .batch(vec![
            GameEvent::LifeChange {
                player: P1,
                delta: 0,
            },
            GameEvent::Damage {
                source: None,
                source_controller: None,
                target: DamageTarget::Object(a),
                amount: 0,
                combat: false,
            },
            GameEvent::Untap { object: a },
        ])
        .unwrap();
    assert!(done.events.is_empty());
    assert_eq!(w.events.log().len(), 0);
    assert_eq!(w.snap(), before);
    let empty = w.batch(Vec::new()).unwrap();
    assert!(empty.events.is_empty());
    assert_eq!(w.events.log().len(), 0);

    let logged = w.batch(vec![GameEvent::Tap { object: a }]).unwrap();
    assert_eq!(logged.index, 0);
    assert_eq!(w.events.log().len(), 1);
}

// REQ-EVENT-001: `execute` equals a batch of one.
#[test]
fn execute_equals_a_batch_of_one() {
    let mut w1 = World::new();
    for _ in 0..2 {
        w1.create(ZoneKind::Library, P1);
    }
    let a = w1.create(ZoneKind::Battlefield, P1);
    let mut w2 = w1.clone();
    let script = vec![
        GameEvent::Draw { player: P1 },
        GameEvent::Tap { object: a },
        GameEvent::Tap { object: a },
        zc(a, ZoneKind::Exile, ZoneChangeCause::Exile),
        GameEvent::LifeChange {
            player: P2,
            delta: 5,
        },
    ];
    for ev in script {
        let single = w1.exec(ev.clone()).expect("ok");
        let batch = w2.batch(vec![ev]).expect("ok");
        assert_eq!(single.as_ref(), batch.events.first());
        assert!(batch.events.len() <= 1);
        assert_eq!(w1.snap(), w2.snap());
    }
    assert_eq!(bytes(w1.events.log()), bytes(w2.events.log()));
    assert_eq!(w1.events.log().batches(), w2.events.log().batches());
}

// REQ-EVENT-001, CR 614.5: AppliedEffects are carried unchanged into the executed event and the
// log.
#[test]
fn applied_effects_are_carried_unchanged() {
    let mut w = World::new();
    let inst = EventInstance::new(GameEvent::LifeChange {
        player: P1,
        delta: 3,
    })
    .with_applied(applied(&[2, 11, 40]));
    let done = w.exec(inst.clone()).unwrap().expect("occurred");
    assert_eq!(done.instance, inst);
    assert_eq!(done.instance.applied_effects(), &applied(&[2, 11, 40]));
    assert_eq!(done.outcome, Outcome::Applied);
    let logged = &w.events.log().batches()[0].events[0];
    assert_eq!(logged.instance.applied_effects(), &applied(&[2, 11, 40]));

    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let batch = EventBatch::new(vec![
        EventInstance::new(GameEvent::Tap { object: a }).with_applied(applied(&[1])),
        EventInstance::new(GameEvent::LifeChange {
            player: P2,
            delta: -1,
        }),
    ]);
    let done = w.ctx().execute_batch(&batch).unwrap();
    assert_eq!(done.events[0].instance, batch.events()[0]);
    assert_eq!(done.events[1].instance, batch.events()[1]);
}
