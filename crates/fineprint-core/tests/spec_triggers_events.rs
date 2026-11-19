// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `events_from_batch`: the mapping from executed game events to trigger events
//! (REQ-EVENT-002; CR 603.2, 603.6a, 603.6c, 603.10a, 701.6a). Real batches are executed through
//! `EventContext::execute_batch` (the `spec_events_support.rs` world); the full from/to/cause
//! matrix uses `ExecutedBatch` literals. Black-box: written from the work-item brief only.

#[path = "spec_triggers_support.rs"]
mod support;

#[path = "spec_events_support.rs"]
mod esupport;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::events::{
    CounterTarget, DamageTarget, EventInstance, ExecutedBatch, ExecutedEvent, GameEvent, Outcome,
    Step, ZoneChangeCause,
};
use fineprint_core::permanent::AttachTarget;
use fineprint_core::triggers::{EventField, TriggerEvent, events_from_batch};
use fineprint_core::zones::{MoveRequest, Moved, ZoneId, ZoneKind};
use fineprint_dsl::ability::{EventKind, FilterValue};
use fineprint_dsl::expr::Zone;

use esupport::World;
use support::{P1, P2, name, vbool, vint, vzone};

use EventKind as K;

fn dsl_zone(z: ZoneKind) -> Zone {
    match z {
        ZoneKind::Library => Zone::Library,
        ZoneKind::Hand => Zone::Hand,
        ZoneKind::Battlefield => Zone::Battlefield,
        ZoneKind::Graveyard => Zone::Graveyard,
        ZoneKind::Stack => Zone::Stack,
        ZoneKind::Exile => Zone::Exile,
        ZoneKind::Command => Zone::Command,
    }
}

fn zc(object: ObjectId, to: ZoneKind, cause: ZoneChangeCause) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause,
    }
}

/// Executes `events` as one batch in `w` and maps it.
fn mapped(w: &mut World, events: Vec<GameEvent>) -> (ExecutedBatch, Vec<TriggerEvent>) {
    let batch = w.batch(events).expect("batch executes");
    let out = events_from_batch(&batch);
    (batch, out)
}

/// Executes one event and maps it, expecting exactly one trigger event.
fn one(w: &mut World, event: GameEvent) -> TriggerEvent {
    let (_, out) = mapped(w, vec![event]);
    assert_eq!(out.len(), 1, "{out:?}");
    out.into_iter().next().expect("one")
}

/// The conditional kinds of a zone change, in the brief's table (CR 603.6a, 603.6c, 603.10a,
/// 701.6a), each with whether it must be present.
fn expected_zone_kinds(from: ZoneKind, to: ZoneKind, cause: ZoneChangeCause) -> Vec<(K, bool)> {
    use ZoneKind as Z;
    vec![
        (K::ZoneChange, true),
        (K::Enters, to == Z::Battlefield && from != Z::Battlefield),
        (
            K::LeavesBattlefield,
            from == Z::Battlefield && to != Z::Battlefield,
        ),
        (K::Dies, from == Z::Battlefield && to == Z::Graveyard),
        (
            K::PutIntoGraveyard,
            to == Z::Graveyard && from != Z::Graveyard,
        ),
        (
            K::LeavesGraveyard,
            from == Z::Graveyard && to != Z::Graveyard,
        ),
        (K::Exiled, to == Z::Exile && from != Z::Exile),
        (
            K::ReturnedToHand,
            to == Z::Hand
                && matches!(
                    from,
                    Z::Battlefield | Z::Graveyard | Z::Exile | Z::Stack | Z::Command
                ),
        ),
        (K::Sacrificed, cause == ZoneChangeCause::Sacrifice),
        (K::Destroyed, cause == ZoneChangeCause::Destroy),
        (K::Discarded, cause == ZoneChangeCause::Discard),
        (K::Milled, cause == ZoneChangeCause::Mill),
        (K::Countered, cause == ZoneChangeCause::CounterSpell),
    ]
}

/// Checks a mapped zone change against the table: kinds present/absent, subject ids, from/to.
fn check_zone_change(t: &TriggerEvent, m: Moved, cause: ZoneChangeCause, label: &str) {
    let (from, to) = (m.from.kind(), m.to.kind());
    for (kind, present) in expected_zone_kinds(from, to, cause) {
        assert_eq!(
            t.kinds.contains(&kind),
            present,
            "{label}: {from:?}->{to:?} {cause:?}: {kind:?} (kinds {:?})",
            t.kinds
        );
    }
    assert_eq!(t.subject_pre, Some(m.old), "{label}");
    assert_eq!(t.subject_post, Some(m.new), "{label}");
    assert_eq!(
        t.fields.get("from"),
        Some(&vzone(dsl_zone(from))),
        "{label}"
    );
    assert_eq!(t.fields.get("to"), Some(&vzone(dsl_zone(to))), "{label}");
}

fn moved_of(e: &ExecutedEvent) -> Moved {
    match e.outcome {
        Outcome::ZoneChange(m) => m,
        other => panic!("expected a zone change, got {other:?}"),
    }
}

// =============================================================================================
// zone changes, real batches
// =============================================================================================

// REQ-EVENT-002, CR 603.6a, 603.6c, 603.10a, 701.6a: natural zone changes executed for real map
// to the right kinds, subject ids and from/to fields.
#[test]
fn spec_triggers_events_zone_changes_real() {
    use ZoneChangeCause as C;
    use ZoneKind as Z;
    let rows: [(Z, Z, C); 15] = [
        (Z::Battlefield, Z::Graveyard, C::Other),
        (Z::Battlefield, Z::Graveyard, C::Destroy),
        (Z::Battlefield, Z::Graveyard, C::Sacrifice),
        (Z::Battlefield, Z::Graveyard, C::StateBasedAction),
        (Z::Battlefield, Z::Exile, C::Exile),
        (Z::Battlefield, Z::Hand, C::Other),
        (Z::Battlefield, Z::Library, C::Other),
        (Z::Hand, Z::Graveyard, C::Discard),
        (Z::Library, Z::Graveyard, C::Mill),
        (Z::Library, Z::Hand, C::Other),
        (Z::Graveyard, Z::Exile, C::Other),
        (Z::Graveyard, Z::Hand, C::Other),
        (Z::Graveyard, Z::Battlefield, C::Other),
        (Z::Hand, Z::Battlefield, C::Other),
        (Z::Exile, Z::Hand, C::Other),
    ];
    for (from, to, cause) in rows {
        let mut w = World::new();
        let a = w.create(from, P1);
        let (batch, out) = mapped(&mut w, vec![zc(a, to, cause)]);
        assert_eq!(out.len(), 1);
        let m = moved_of(&batch.events[0]);
        assert_eq!(m.old, a);
        check_zone_change(&out[0], m, cause, "real");
    }
}

// REQ-EVENT-002, CR 603.10e, 701.6a: a countered spell (stack -> graveyard, CounterSpell) is
// COUNTERED; a plain stack -> graveyard move is not.
#[test]
fn spec_triggers_events_countered_vs_plain_stack_move() {
    for cause in [ZoneChangeCause::CounterSpell, ZoneChangeCause::Other] {
        let mut w = World::new();
        let s = w.create(ZoneKind::Stack, P1);
        let (batch, out) = mapped(&mut w, vec![zc(s, ZoneKind::Graveyard, cause)]);
        assert_eq!(out.len(), 1);
        check_zone_change(&out[0], moved_of(&batch.events[0]), cause, "stack");
        assert_eq!(
            out[0].kinds.contains(&K::Countered),
            cause == ZoneChangeCause::CounterSpell
        );
        assert!(!out[0].kinds.contains(&K::LeavesBattlefield));
        assert!(!out[0].kinds.contains(&K::Dies));
    }
}

// REQ-EVENT-002, CR 603.10a: a library -> hand move is neither RETURNED_TO_HAND nor
// LEAVES_BATTLEFIELD.
#[test]
fn spec_triggers_events_library_to_hand() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Library, P1);
    let t = one(&mut w, zc(a, ZoneKind::Hand, ZoneChangeCause::Other));
    assert!(t.kinds.contains(&K::ZoneChange));
    assert!(!t.kinds.contains(&K::ReturnedToHand));
    assert!(!t.kinds.contains(&K::LeavesBattlefield));
    assert!(!t.kinds.contains(&K::Enters));
    assert_eq!(t.fields.get("from"), Some(&vzone(Zone::Library)));
    assert_eq!(t.fields.get("to"), Some(&vzone(Zone::Hand)));
}

// =============================================================================================
// zone changes, full matrix (ExecutedBatch literals)
// =============================================================================================

fn literal_zone_change(
    from: ZoneKind,
    to: ZoneKind,
    cause: ZoneChangeCause,
    old: u64,
    new: u64,
) -> ExecutedEvent {
    ExecutedEvent {
        instance: EventInstance::new(zc(ObjectId::new(old), to, cause)),
        outcome: Outcome::ZoneChange(Moved {
            old: ObjectId::new(old),
            new: ObjectId::new(new),
            from: ZoneId::of(from, P1),
            to: ZoneId::of(to, P2),
        }),
        lki_captured: from == ZoneKind::Battlefield,
    }
}

// REQ-EVENT-002, CR 603.6a, 603.6c, 603.10a, 701.6a: every (from, to, cause) combination maps by
// the table: required kinds present, all others in the table absent.
#[test]
fn spec_triggers_events_zone_change_matrix() {
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
    let mut n = 0;
    for from in ZoneKind::ALL {
        for to in ZoneKind::ALL {
            if from == to {
                continue;
            }
            for cause in causes {
                n += 1;
                let batch = ExecutedBatch {
                    index: 0,
                    turn: 1,
                    events: vec![literal_zone_change(from, to, cause, n, n + 1000)],
                };
                let out = events_from_batch(&batch);
                assert_eq!(out.len(), 1);
                check_zone_change(&out[0], moved_of(&batch.events[0]), cause, "literal");
            }
        }
    }
}

// =============================================================================================
// other events
// =============================================================================================

// REQ-EVENT-002, CR 603.2: a draw maps to DRAWN with the drawing player.
#[test]
fn spec_triggers_events_draw() {
    let mut w = World::new();
    w.create(ZoneKind::Library, P1);
    let t = one(&mut w, GameEvent::Draw { player: P1 });
    assert!(t.kinds.contains(&K::Drawn));
    assert!(!t.kinds.contains(&K::ReturnedToHand));
    assert!(!t.kinds.contains(&K::LeavesBattlefield));
    assert_eq!(t.subject_player, Some(P1));
}

// REQ-EVENT-002, CR 603.2: damage maps to DAMAGE_DEALT with source ids, combat, amount and
// recipient.
#[test]
fn spec_triggers_events_damage() {
    let mut w = World::new();
    let src = w.create(ZoneKind::Battlefield, P1);
    let victim = w.create(ZoneKind::Battlefield, P2);
    let (_, out) = mapped(
        &mut w,
        vec![
            GameEvent::Damage {
                source: Some(src),
                source_controller: Some(P1),
                target: DamageTarget::Player(P2),
                amount: 3,
                combat: true,
            },
            GameEvent::Damage {
                source: Some(src),
                source_controller: Some(P1),
                target: DamageTarget::Object(victim),
                amount: 1,
                combat: false,
            },
        ],
    );
    assert_eq!(out.len(), 2);
    for t in &out {
        assert!(t.kinds.contains(&K::DamageDealt));
        assert_eq!(t.subject_pre, Some(src));
        assert_eq!(t.subject_post, Some(src));
    }
    assert_eq!(out[0].fields.get("combat"), Some(&vbool(true)));
    assert_eq!(out[0].fields.get("amount"), Some(&vint(3)));
    assert_eq!(
        out[0].fields.get("recipient"),
        Some(&EventField::Player(P2))
    );
    assert_eq!(out[1].fields.get("combat"), Some(&vbool(false)));
    assert_eq!(out[1].fields.get("amount"), Some(&vint(1)));
    assert_eq!(
        out[1].fields.get("recipient"),
        Some(&EventField::Object(victim))
    );
}

// REQ-EVENT-002, CR 603.2: life changes map to LIFE_GAINED / LIFE_LOST with |delta|.
#[test]
fn spec_triggers_events_life() {
    let mut w = World::new();
    let (_, out) = mapped(
        &mut w,
        vec![
            GameEvent::LifeChange {
                player: P1,
                delta: 2,
            },
            GameEvent::LifeChange {
                player: P2,
                delta: -3,
            },
        ],
    );
    assert_eq!(out.len(), 2);
    assert!(out[0].kinds.contains(&K::LifeGained));
    assert!(!out[0].kinds.contains(&K::LifeLost));
    assert_eq!(out[0].subject_player, Some(P1));
    assert_eq!(out[0].fields.get("amount"), Some(&vint(2)));
    assert!(out[1].kinds.contains(&K::LifeLost));
    assert!(!out[1].kinds.contains(&K::LifeGained));
    assert_eq!(out[1].subject_player, Some(P2));
    assert_eq!(out[1].fields.get("amount"), Some(&vint(3)));
}

// REQ-EVENT-002, CR 603.2: counters map to COUNTERS_PUT / COUNTERS_REMOVED (amount actually
// removed), with object or player subject and a Name kind field.
#[test]
fn spec_triggers_events_counters() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let (_, out) = mapped(
        &mut w,
        vec![
            GameEvent::AddCounters {
                target: CounterTarget::Object(a),
                kind: "plus_one".into(),
                amount: 2,
            },
            GameEvent::AddCounters {
                target: CounterTarget::Player(P2),
                kind: "poison".into(),
                amount: 1,
            },
        ],
    );
    assert_eq!(out.len(), 2);
    assert!(out[0].kinds.contains(&K::CountersPut));
    assert_eq!(out[0].subject_pre, Some(a));
    assert_eq!(out[0].subject_post, Some(a));
    assert_eq!(out[0].fields.get("amount"), Some(&vint(2)));
    assert_eq!(
        out[0].fields.get("kind"),
        Some(&EventField::Value(name("plus_one")))
    );
    assert!(out[1].kinds.contains(&K::CountersPut));
    assert_eq!(out[1].subject_player, Some(P2));
    assert_eq!(
        out[1].fields.get("kind"),
        Some(&EventField::Value(name("poison")))
    );

    let t = one(
        &mut w,
        GameEvent::RemoveCounters {
            target: CounterTarget::Object(a),
            kind: "plus_one".into(),
            amount: 5,
        },
    );
    assert!(t.kinds.contains(&K::CountersRemoved));
    assert!(!t.kinds.contains(&K::CountersPut));
    assert_eq!(t.subject_pre, Some(a));
    assert_eq!(t.fields.get("amount"), Some(&vint(2)), "actually removed");
    assert_eq!(
        t.fields.get("kind"),
        Some(&EventField::Value(name("plus_one")))
    );

    let t = one(
        &mut w,
        GameEvent::RemoveCounters {
            target: CounterTarget::Player(P2),
            kind: "poison".into(),
            amount: 1,
        },
    );
    assert!(t.kinds.contains(&K::CountersRemoved));
    assert_eq!(t.subject_player, Some(P2));
    assert_eq!(t.fields.get("amount"), Some(&vint(1)));
}

// REQ-EVENT-002: a counter kind FilterValue::name rejects (e.g. "ANY") is omitted from fields.
#[test]
fn spec_triggers_events_counter_kind_not_a_name() {
    assert!(FilterValue::name("ANY").is_err());
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let t = one(
        &mut w,
        GameEvent::AddCounters {
            target: CounterTarget::Object(a),
            kind: "ANY".into(),
            amount: 1,
        },
    );
    assert!(t.kinds.contains(&K::CountersPut));
    assert_eq!(t.fields.get("kind"), None);
    assert_eq!(t.fields.get("amount"), Some(&vint(1)));
}

// REQ-EVENT-002: tap, untap and turn-face-up map with the object as subject.
#[test]
fn spec_triggers_events_permanent_status() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let t = one(&mut w, GameEvent::Tap { object: a });
    assert!(t.kinds.contains(&K::Tapped));
    assert_eq!((t.subject_pre, t.subject_post), (Some(a), Some(a)));
    let t = one(&mut w, GameEvent::Untap { object: a });
    assert!(t.kinds.contains(&K::Untapped));
    assert!(!t.kinds.contains(&K::Tapped));
    assert_eq!((t.subject_pre, t.subject_post), (Some(a), Some(a)));

    // Turning face down maps to nothing; turning face up maps to TURNED_FACE_UP.
    let (batch, out) = mapped(&mut w, vec![GameEvent::TurnFaceDown { object: a }]);
    assert_eq!(batch.events.len(), 1);
    assert!(out.is_empty(), "{out:?}");
    let t = one(&mut w, GameEvent::TurnFaceUp { object: a });
    assert!(t.kinds.contains(&K::TurnedFaceUp));
    assert_eq!((t.subject_pre, t.subject_post), (Some(a), Some(a)));
}

// REQ-EVENT-002, CR 603.6a: a created token maps to TOKEN_CREATED with only a post-event id and
// the controller (else owner) as player.
#[test]
fn spec_triggers_events_token_created() {
    let mut w = World::new();
    let c1 = w.fresh_card();
    let c2 = w.fresh_card();
    let (batch, out) = mapped(
        &mut w,
        vec![
            GameEvent::CreateToken {
                owner: P1,
                card: c1,
                controller: None,
            },
            GameEvent::CreateToken {
                owner: P1,
                card: c2,
                controller: Some(P2),
            },
        ],
    );
    assert_eq!(out.len(), 2);
    let token = |i: usize| match batch.events[i].outcome {
        Outcome::TokenCreated { object } => object,
        other => panic!("{other:?}"),
    };
    let expect: [(usize, PlayerId); 2] = [(0, P1), (1, P2)];
    for (i, p) in expect {
        assert!(out[i].kinds.contains(&K::TokenCreated));
        assert_eq!(out[i].subject_pre, None);
        assert_eq!(out[i].subject_post, Some(token(i)));
        assert_eq!(out[i].subject_player, Some(p));
    }
}

// REQ-EVENT-002, CR 603.10c: attach / unattach map with the attachment as subject.
#[test]
fn spec_triggers_events_attach_unattach() {
    let mut w = World::new();
    let gear = w.create(ZoneKind::Battlefield, P1);
    let host = w.create(ZoneKind::Battlefield, P1);
    let t = one(
        &mut w,
        GameEvent::Attach {
            attachment: gear,
            target: AttachTarget::Permanent(host),
        },
    );
    assert!(t.kinds.contains(&K::Attached));
    assert_eq!((t.subject_pre, t.subject_post), (Some(gear), Some(gear)));
    let t = one(&mut w, GameEvent::Unattach { attachment: gear });
    assert!(t.kinds.contains(&K::Unattached));
    assert!(!t.kinds.contains(&K::Attached));
    assert_eq!((t.subject_pre, t.subject_post), (Some(gear), Some(gear)));
}

// REQ-EVENT-002: turn structure and copy events produce no trigger event.
#[test]
fn spec_triggers_events_unmapped() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P1);
    let (batch, out) = mapped(
        &mut w,
        vec![
            GameEvent::StepBegins { step: Step::Upkeep },
            GameEvent::BecomeCopy {
                object: b,
                copied: a,
            },
        ],
    );
    assert_eq!(batch.events.len(), 2);
    assert!(out.is_empty(), "{out:?}");
    let (batch, out) = mapped(&mut w, vec![GameEvent::TurnBegins { player: P2 }]);
    assert_eq!(batch.events.len(), 1);
    assert!(out.is_empty(), "{out:?}");
    let (batch, out) = mapped(
        &mut w,
        vec![GameEvent::PhaseBegins {
            phase: fineprint_core::events::Phase::Ending,
        }],
    );
    assert_eq!(batch.events.len(), 1);
    assert!(out.is_empty(), "{out:?}");
}

// REQ-EVENT-002, CR 603.2g: no-op events (not logged by the executor) produce nothing.
#[test]
fn spec_triggers_events_no_ops_produce_nothing() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.ok(GameEvent::Tap { object: a });
    let (batch, out) = mapped(
        &mut w,
        vec![
            GameEvent::Damage {
                source: Some(a),
                source_controller: Some(P1),
                target: DamageTarget::Player(P2),
                amount: 0,
                combat: false,
            },
            GameEvent::Tap { object: a },
            GameEvent::LifeChange {
                player: P1,
                delta: 1,
            },
        ],
    );
    assert_eq!(batch.events.len(), 1, "only the life change occurred");
    assert_eq!(out.len(), 1);
    assert!(out[0].kinds.contains(&K::LifeGained));
}

// REQ-EVENT-002, CR 603.2c: output follows the batch order; an empty batch gives nothing.
#[test]
fn spec_triggers_events_order_and_empty() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P2);
    w.create(ZoneKind::Library, P2);
    let (_, out) = mapped(
        &mut w,
        vec![
            GameEvent::Tap { object: a },
            GameEvent::StepBegins { step: Step::Upkeep },
            zc(b, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
            GameEvent::LifeChange {
                player: P1,
                delta: -1,
            },
            GameEvent::Draw { player: P2 },
        ],
    );
    assert_eq!(out.len(), 4);
    assert!(out[0].kinds.contains(&K::Tapped));
    assert!(out[1].kinds.contains(&K::Dies));
    assert!(out[2].kinds.contains(&K::LifeLost));
    assert!(out[3].kinds.contains(&K::Drawn));

    let empty = ExecutedBatch {
        index: 0,
        turn: 1,
        events: Vec::new(),
    };
    assert!(events_from_batch(&empty).is_empty());
}

// REQ-EVENT-002, R12: odd literal batches (applied outcomes on zone events, mismatched outcomes)
// never panic.
#[test]
fn spec_triggers_events_odd_literals_do_not_panic() {
    let odd = ExecutedBatch {
        index: 7,
        turn: 3,
        events: vec![
            ExecutedEvent {
                instance: EventInstance::new(zc(
                    ObjectId::new(1),
                    ZoneKind::Exile,
                    ZoneChangeCause::Other,
                )),
                outcome: Outcome::Applied,
                lki_captured: false,
            },
            ExecutedEvent {
                instance: EventInstance::new(GameEvent::Tap {
                    object: ObjectId::new(2),
                }),
                outcome: Outcome::TurnBegan { turn: 9 },
                lki_captured: false,
            },
            ExecutedEvent {
                instance: EventInstance::new(GameEvent::RemoveCounters {
                    target: CounterTarget::Object(ObjectId::new(3)),
                    kind: String::new(),
                    amount: i64::MAX,
                }),
                outcome: Outcome::Applied,
                lki_captured: false,
            },
            ExecutedEvent {
                instance: EventInstance::new(GameEvent::LifeChange {
                    player: P1,
                    delta: i64::MIN,
                }),
                outcome: Outcome::Applied,
                lki_captured: false,
            },
        ],
    };
    let _ = events_from_batch(&odd);
}
