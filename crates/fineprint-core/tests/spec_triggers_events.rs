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
use fineprint_core::triggers::{
    EventField, StateView, TriggerEvent, events_from_batch, events_from_batch_with_lki,
};
use fineprint_core::zones::{MoveRequest, Moved, ZoneId, ZoneKind};
use fineprint_dsl::ability::{EventKind, FilterValue};
use fineprint_dsl::expr::Zone;

use esupport::World;
use support::{
    FakeEnv, P1, P2, ability, bind, filt, m, name, reg, run, subj_obj, subj_players, summary,
    vbool, vint, vzone, zone,
};

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

/// Asserts the kinds of `t` are exactly `expected`, as a set.
fn assert_kinds_exactly(t: &TriggerEvent, expected: &[K]) {
    for k in K::ALL {
        assert_eq!(
            t.kinds.contains(k),
            expected.contains(k),
            "{k:?} (kinds {:?}, expected {expected:?})",
            t.kinds
        );
    }
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

// REQ-EVENT-002, CR 400.8, 406.7: an object already in exile that is exiled again does not change
// zones: it is EXILED but not a ZONE_CHANGE. A move into exile from another zone is both.
#[test]
fn spec_triggers_events_exile_to_exile() {
    for cause in [ZoneChangeCause::Exile, ZoneChangeCause::Other] {
        let mut w = World::new();
        let a = w.create(ZoneKind::Exile, P1);
        let (batch, out) = mapped(&mut w, vec![zc(a, ZoneKind::Exile, cause)]);
        let moved = moved_of(&batch.events[0]);
        assert_ne!(moved.old, moved.new, "re-exiling makes a new object");
        assert_eq!(out.len(), 1, "{cause:?}");
        assert!(out[0].kinds.contains(&K::Exiled), "{cause:?}");
        assert!(!out[0].kinds.contains(&K::ZoneChange), "{cause:?}");
        assert_eq!(out[0].subject_pre, Some(moved.old));
        assert_eq!(out[0].subject_post, Some(moved.new));
        if cause == ZoneChangeCause::Other {
            assert_kinds_exactly(&out[0], &[K::Exiled]);
        }
    }
    for from in [ZoneKind::Graveyard, ZoneKind::Battlefield, ZoneKind::Hand] {
        let mut w = World::new();
        let a = w.create(from, P1);
        let t = one(&mut w, zc(a, ZoneKind::Exile, ZoneChangeCause::Exile));
        assert!(t.kinds.contains(&K::Exiled), "{from:?}");
        assert!(t.kinds.contains(&K::ZoneChange), "{from:?}");
    }
}

// REQ-EVENT-002, CR 400.8, 406.7: detection over a re-exile: an unfiltered EXILED matcher triggers
// and a ZONE_CHANGE matcher does not.
#[test]
fn spec_triggers_events_exile_to_exile_detection() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Exile, P1);
    let (_, out) = mapped(&mut w, vec![zc(a, ZoneKind::Exile, ZoneChangeCause::Exile)]);
    let env = FakeEnv::new()
        .with_post(reg(1, P1, ability("ex", m(K::Exiled))))
        .with_post(reg(2, P1, ability("zc", m(K::ZoneChange))));
    assert_eq!(
        summary(&run(&env, &out)),
        vec![(1, "ex".to_string(), vec![0])]
    );
}

// REQ-EVENT-002, CR 701.21a, 603.10a: the player who sacrifices is the permanent's controller at
// the time (from the batch's LKI), not its owner. Without LKI there is no subject_player; other
// causes have none either. A players-subject SACRIFICED matcher matches the controller's group
// only; an object-subject matcher still matches via subject_pre.
#[test]
fn spec_triggers_events_sacrifice_subject_player() {
    let mut w = World::new();
    // Owned by P1, controlled by P2.
    let perm = w.create_controlled(ZoneKind::Battlefield, P1, Some(P2));
    let batch = w
        .batch(vec![zc(
            perm,
            ZoneKind::Graveyard,
            ZoneChangeCause::Sacrifice,
        )])
        .expect("batch executes");
    let moved = moved_of(&batch.events[0]);
    assert_eq!(
        w.lki.get(perm).expect("LKI captured").controller(),
        Some(P2),
        "fixture: the controller differs from the owner"
    );

    let out = events_from_batch_with_lki(&batch, &w.lki);
    assert_eq!(out.len(), 1);
    check_zone_change(&out[0], moved, ZoneChangeCause::Sacrifice, "sacrifice");
    assert_eq!(out[0].subject_player, Some(P2));
    assert_eq!(out[0].subject_pre, Some(moved.old));
    assert_eq!(out[0].subject_post, Some(moved.new));

    let pre = StateView::PreEvent;
    let sac_players = || reg(1, P1, ability("sp", subj_players(m(K::Sacrificed))));
    let for_p2 = FakeEnv::new()
        .accept_player(pre, P2)
        .with_pre(sac_players());
    assert_eq!(
        summary(&run(&for_p2, &out)),
        vec![(1, "sp".to_string(), vec![0])]
    );
    let for_p1 = FakeEnv::new()
        .accept_player(pre, P1)
        .with_pre(sac_players());
    assert!(run(&for_p1, &out).is_empty());

    let by_object = FakeEnv::new().accept(pre, &[moved.old.get()]).with_pre(reg(
        2,
        P1,
        ability("so", subj_obj(m(K::Sacrificed))),
    ));
    assert_eq!(
        summary(&run(&by_object, &out)),
        vec![(2, "so".to_string(), vec![0])]
    );

    // Without LKI the subject player is unknown.
    let plain = events_from_batch(&batch);
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].subject_player, None);
    assert!(run(&for_p2, &plain).is_empty());

    // A non-sacrifice cause has no subject player, with or without LKI.
    for cause in [ZoneChangeCause::Destroy, ZoneChangeCause::Other] {
        let mut w = World::new();
        let perm = w.create_controlled(ZoneKind::Battlefield, P1, Some(P2));
        let batch = w
            .batch(vec![zc(perm, ZoneKind::Graveyard, cause)])
            .expect("batch executes");
        let with = events_from_batch_with_lki(&batch, &w.lki);
        assert_eq!(with.len(), 1);
        assert_eq!(with[0].subject_player, None, "{cause:?}");
        assert_eq!(events_from_batch(&batch)[0].subject_player, None);
    }
}

// REQ-EVENT-002, CR 701.9a, 701.17a: a discard (hand -> graveyard) carries the owner of the hand
// as subject_player, a mill (library -> graveyard) the owner of the library; object ids and
// kinds are unchanged. A players-subject matcher matches only the group containing that player;
// an object-subject matcher still matches via the ids. Destroy has no subject player.
#[test]
fn spec_triggers_events_discard_mill_subject_player() {
    // Not a look-back event: the view is the post-event one; accept in both to stay agnostic.
    let views = [StateView::PreEvent, StateView::PostEvent];
    for (from, cause, kind) in [
        (ZoneKind::Hand, ZoneChangeCause::Discard, K::Discarded),
        (ZoneKind::Library, ZoneChangeCause::Mill, K::Milled),
    ] {
        let mut w = World::new();
        let card = w.create(from, P2);
        let (batch, out) = mapped(&mut w, vec![zc(card, ZoneKind::Graveyard, cause)]);
        assert_eq!(out.len(), 1);
        let moved = moved_of(&batch.events[0]);
        check_zone_change(&out[0], moved, cause, "discard/mill");
        assert!(out[0].kinds.contains(&kind));
        assert_eq!(out[0].subject_player, Some(P2), "{cause:?}");
        assert_eq!(out[0].subject_pre, Some(moved.old));
        assert_eq!(out[0].subject_post, Some(moved.new));
        // With an LKI store the owner is still the zone's owner.
        let with = events_from_batch_with_lki(&batch, &w.lki);
        assert_eq!(with[0].subject_player, Some(P2), "{cause:?} with LKI");

        let players = || reg(1, P1, ability("sp", subj_players(m(kind))));
        let mut for_p2 = FakeEnv::new().with_both(players());
        let mut for_p1 = FakeEnv::new().with_both(players());
        for v in views {
            for_p2 = for_p2.accept_player(v, P2);
            for_p1 = for_p1.accept_player(v, P1);
        }
        assert_eq!(
            summary(&run(&for_p2, &out)),
            vec![(1, "sp".to_string(), vec![0])],
            "{cause:?}"
        );
        assert!(run(&for_p1, &out).is_empty(), "{cause:?}");

        let mut by_object = FakeEnv::new().with_both(reg(2, P1, ability("so", subj_obj(m(kind)))));
        for v in views {
            by_object = by_object.accept(v, &[moved.old.get(), moved.new.get()]);
        }
        assert_eq!(
            summary(&run(&by_object, &out)),
            vec![(2, "so".to_string(), vec![0])],
            "{cause:?}"
        );
    }

    // A destroy (battlefield -> graveyard) has no subject player.
    let mut w = World::new();
    let perm = w.create(ZoneKind::Battlefield, P2);
    let t = one(
        &mut w,
        zc(perm, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
    );
    assert_eq!(t.subject_player, None);
}

/// A literal same-zone move of kind `z` (owner P1) from object `old` to object `new`.
fn literal_same_zone(z: ZoneKind, cause: ZoneChangeCause, old: u64, new: u64) -> ExecutedEvent {
    ExecutedEvent {
        instance: EventInstance::new(zc(ObjectId::new(old), z, cause)),
        outcome: Outcome::ZoneChange(Moved {
            old: ObjectId::new(old),
            new: ObjectId::new(new),
            from: ZoneId::of(z, P1),
            to: ZoneId::of(z, P1),
        }),
        lki_captured: false,
    }
}

// REQ-EVENT-002, CR 400.7, 400.8, 406.7, 400.10: a same-zone move of the same object (old == new:
// library, hand, graveyard, battlefield, stack) is not a zone change, so it yields no occurrence
// for any cause. Exile -> exile and command -> command (a new id) are occurrences: exile gives
// {Exiled}, command {ZoneChange}.
#[test]
fn spec_triggers_events_same_zone_matrix() {
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
    for z in ZoneKind::ALL {
        for cause in causes {
            // Same object: never an occurrence.
            let batch = ExecutedBatch {
                index: 0,
                turn: 1,
                events: vec![literal_same_zone(z, cause, 7, 7)],
            };
            assert!(
                events_from_batch(&batch).is_empty(),
                "{z:?} {cause:?} same id"
            );
            // A new id (only exile and the command zone give one): one occurrence.
            if !matches!(z, ZoneKind::Exile | ZoneKind::Command) {
                continue;
            }
            let batch = ExecutedBatch {
                index: 0,
                turn: 1,
                events: vec![literal_same_zone(z, cause, 7, 8)],
            };
            let out = events_from_batch(&batch);
            match z {
                ZoneKind::Exile => {
                    assert_eq!(out.len(), 1, "{cause:?}");
                    assert!(out[0].kinds.contains(&K::Exiled));
                    assert!(!out[0].kinds.contains(&K::ZoneChange));
                    if cause == ZoneChangeCause::Other {
                        assert_kinds_exactly(&out[0], &[K::Exiled]);
                    }
                }
                ZoneKind::Command => {
                    assert_eq!(out.len(), 1, "{cause:?}");
                    assert!(out[0].kinds.contains(&K::ZoneChange));
                    assert!(!out[0].kinds.contains(&K::Exiled));
                    if cause == ZoneChangeCause::Other {
                        assert_kinds_exactly(&out[0], &[K::ZoneChange]);
                    }
                }
                _ => unreachable!("filtered above"),
            }
        }
    }
}

// REQ-EVENT-002, CR 400.7: real executed same-zone moves (library Top and Bottom, hand, graveyard,
// battlefield, stack) log a zone change with old == new and yield no occurrence; a move to a
// distinct zone yields one.
#[test]
fn spec_triggers_events_same_zone_real() {
    for (zone_kind, position) in [
        (ZoneKind::Library, fineprint_core::zones::Position::Top),
        (ZoneKind::Library, fineprint_core::zones::Position::Bottom),
        (ZoneKind::Hand, fineprint_core::zones::Position::Top),
        (ZoneKind::Graveyard, fineprint_core::zones::Position::Top),
        (ZoneKind::Battlefield, fineprint_core::zones::Position::Top),
        (ZoneKind::Stack, fineprint_core::zones::Position::Top),
    ] {
        let mut w = World::new();
        let a = w.create(zone_kind, P1);
        w.create(zone_kind, P1);
        let mut request = MoveRequest::new(zone_kind);
        request.position = position;
        let (batch, out) = mapped(
            &mut w,
            vec![GameEvent::ZoneChange {
                object: a,
                request,
                cause: ZoneChangeCause::Other,
            }],
        );
        assert_eq!(batch.events.len(), 1, "{zone_kind:?} {position:?}");
        let moved = moved_of(&batch.events[0]);
        assert_eq!(moved.old, moved.new, "{zone_kind:?} {position:?}");
        assert!(out.is_empty(), "{zone_kind:?} {position:?}: {out:?}");
    }
    let mut w = World::new();
    let a = w.create(ZoneKind::Library, P1);
    let t = one(&mut w, zc(a, ZoneKind::Hand, ZoneChangeCause::Other));
    assert!(t.kinds.contains(&K::ZoneChange));
}

// =============================================================================================
// other events
// =============================================================================================

// REQ-EVENT-002, CR 121.1, 400.2: a draw is one occurrence that is both DRAWN and a zone change
// (library -> hand): kinds exactly {Drawn, ZoneChange} (never RETURNED_TO_HAND), the drawing
// player as subject_player, the old and new card ids, and from/to fields.
#[test]
fn spec_triggers_events_draw() {
    let mut w = World::new();
    let top = w.create(ZoneKind::Library, P1);
    let (batch, out) = mapped(&mut w, vec![GameEvent::Draw { player: P1 }]);
    assert_eq!(out.len(), 1, "{out:?}");
    let moved = moved_of(&batch.events[0]);
    assert_eq!(moved.old, top);
    assert_ne!(moved.old, moved.new, "a draw is a zone change");
    let t = &out[0];
    assert_kinds_exactly(t, &[K::Drawn, K::ZoneChange]);
    assert_eq!(t.subject_player, Some(P1));
    assert_eq!(t.subject_pre, Some(moved.old));
    assert_eq!(t.subject_post, Some(moved.new));
    assert_eq!(t.fields.get("from"), Some(&vzone(Zone::Library)));
    assert_eq!(t.fields.get("to"), Some(&vzone(Zone::Hand)));
}

// REQ-EVENT-002, CR 121.1: the draw count stays one occurrence per draw.
#[test]
fn spec_triggers_events_draw_count_one_per_draw() {
    let mut w = World::new();
    w.create(ZoneKind::Library, P1);
    w.create(ZoneKind::Library, P1);
    w.create(ZoneKind::Library, P2);
    let (_, out) = mapped(
        &mut w,
        vec![
            GameEvent::Draw { player: P1 },
            GameEvent::Draw { player: P1 },
            GameEvent::Draw { player: P2 },
        ],
    );
    assert_eq!(out.len(), 3, "{out:?}");
    for (t, p) in out.iter().zip([P1, P1, P2]) {
        assert_kinds_exactly(t, &[K::Drawn, K::ZoneChange]);
        assert_eq!(t.subject_player, Some(p));
    }
}

// REQ-EVENT-002, CR 121.1, 400.2, 603.10a: detection over a real draw. A ZONE_CHANGE{to: HAND}
// ability looks back (pre view) when the env says the drawn card is visible to all, and uses the
// post view otherwise; a DRAWN matcher with a players subject triggers once either way; a
// RETURNED_TO_HAND matcher never triggers on a draw.
#[test]
fn spec_triggers_events_draw_detection() {
    let mut w = World::new();
    let top = w.create(ZoneKind::Library, P1);
    let (batch, out) = mapped(&mut w, vec![GameEvent::Draw { player: P1 }]);
    let moved = moved_of(&batch.events[0]);
    assert_eq!(moved.old, top);

    let to_hand = || {
        reg(
            1,
            P1,
            ability(
                "zc",
                subj_obj(filt(m(K::ZoneChange), "to", zone(Zone::Hand))),
            ),
        )
    };
    let drawn = || reg(2, P1, ability("dr", subj_players(m(K::Drawn))));
    let returned = || reg(3, P1, ability("rt", m(K::ReturnedToHand)));
    let build = |visible: bool| {
        let mut env = FakeEnv::new()
            .with_both(to_hand())
            .with_post(drawn())
            .with_both(returned())
            .accept(StateView::PreEvent, &[moved.old.get()])
            .accept(StateView::PostEvent, &[moved.new.get()])
            .accept_player(StateView::PostEvent, P1);
        if visible {
            env = env.visible(&[moved.old.get()]);
        }
        env
    };

    // Visible to all: the pre view, with look-back.
    let out_v = run(&build(true), &out);
    assert_eq!(
        summary(&out_v),
        vec![
            (1, "zc".to_string(), vec![0]),
            (2, "dr".to_string(), vec![0])
        ],
        "{out_v:?}"
    );
    assert_eq!(
        out_v[0].look_back,
        Some(fineprint_core::triggers::LookBackKind::ToHandOrLibrary)
    );
    assert_eq!(out_v[1].look_back, None);

    // Not visible to all: the post view, no look-back.
    let out_h = run(&build(false), &out);
    assert_eq!(
        summary(&out_h),
        vec![
            (1, "zc".to_string(), vec![0]),
            (2, "dr".to_string(), vec![0])
        ],
        "{out_h:?}"
    );
    assert_eq!(out_h[0].look_back, None);
    assert_eq!(out_h[1].look_back, None);
}

// REQ-EVENT-002, CR 603.2, 120.3a, 120.4b: damage to a player is two occurrences in sequence: the
// DAMAGE_DEALT occurrence (source ids, combat, amount, recipient) then a LIFE_LOST occurrence for
// the player; damage to an object is one occurrence (CR 120.3c/d/e/h: not life loss).
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
    assert_eq!(out.len(), 3, "{out:?}");
    // Occurrence 0: the damage dealt to the player.
    assert!(out[0].kinds.contains(&K::DamageDealt));
    assert!(!out[0].kinds.contains(&K::LifeLost));
    assert_eq!(out[0].subject_pre, Some(src));
    assert_eq!(out[0].subject_post, Some(src));
    assert_eq!(out[0].fields.get("combat"), Some(&vbool(true)));
    assert_eq!(out[0].fields.get("amount"), Some(&vint(3)));
    assert_eq!(
        out[0].fields.get("recipient"),
        Some(&EventField::Player(P2))
    );
    // Occurrence 1: the life loss that results (CR 120.3a).
    assert!(out[1].kinds.contains(&K::LifeLost));
    assert_eq!(out[1].subject_player, Some(P2));
    assert_eq!(out[1].fields.get("amount"), Some(&vint(3)));
    assert_eq!(out[1].subject_pre, Some(src));
    assert_eq!(out[1].subject_post, Some(src));
    assert_eq!(
        out[1].fields.get("recipient"),
        Some(&EventField::Player(P2))
    );
    // Occurrence 2: the damage dealt to the object, with no life loss after it.
    assert!(out[2].kinds.contains(&K::DamageDealt));
    assert!(!out[2].kinds.contains(&K::LifeLost));
    assert_eq!(out[2].subject_pre, Some(src));
    assert_eq!(out[2].subject_post, Some(src));
    assert_eq!(out[2].fields.get("combat"), Some(&vbool(false)));
    assert_eq!(out[2].fields.get("amount"), Some(&vint(1)));
    assert_eq!(
        out[2].fields.get("recipient"),
        Some(&EventField::Object(victim))
    );
}

// REQ-EVENT-002, CR 120.3a, 120.4b/c: damage from no source: the life-loss occurrence has no
// source ids.
#[test]
fn spec_triggers_events_damage_without_source() {
    let mut w = World::new();
    let (_, out) = mapped(
        &mut w,
        vec![GameEvent::Damage {
            source: None,
            source_controller: None,
            target: DamageTarget::Player(P2),
            amount: 2,
            combat: false,
        }],
    );
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out[0].kinds.contains(&K::DamageDealt));
    assert!(out[1].kinds.contains(&K::LifeLost));
    assert_eq!(out[1].subject_player, Some(P2));
    assert_eq!(out[1].subject_pre, None);
    assert_eq!(out[1].subject_post, None);
    assert_eq!(out[1].fields.get("amount"), Some(&vint(2)));
    assert_eq!(
        out[1].fields.get("recipient"),
        Some(&EventField::Player(P2))
    );
}

// REQ-EVENT-002, CR 120.3a, 120.4b/c, 120.8: detection over damage. LIFE_LOST (players subject P2)
// triggers once with amount 3 and DAMAGE_DEALT triggers once; damage to a permanent does not
// trigger LIFE_LOST; 0 damage is not logged at all.
#[test]
fn spec_triggers_events_damage_detection() {
    let mut w = World::new();
    let src = w.create(ZoneKind::Battlefield, P1);
    let victim = w.create(ZoneKind::Battlefield, P2);
    let dmg = |target| GameEvent::Damage {
        source: Some(src),
        source_controller: Some(P1),
        target,
        amount: 3,
        combat: false,
    };
    let env = || {
        FakeEnv::new()
            .with_post(reg(
                1,
                P1,
                ability("ll", bind(subj_players(m(K::LifeLost)), "amount", "$N")),
            ))
            .with_post(reg(2, P1, ability("dd", m(K::DamageDealt))))
            .accept_player(StateView::PostEvent, P2)
    };

    let (_, out) = mapped(&mut w, vec![dmg(DamageTarget::Player(P2))]);
    assert_eq!(out.len(), 2);
    let ts = run(&env(), &out);
    assert_eq!(
        summary(&ts),
        vec![
            (2, "dd".to_string(), vec![0]),
            (1, "ll".to_string(), vec![1])
        ]
    );
    let ll = ts.iter().find(|t| t.ability_id == "ll").expect("ll");
    assert_eq!(ll.bindings.get("$N"), Some(&vint(3)));

    let (_, out) = mapped(&mut w, vec![dmg(DamageTarget::Object(victim))]);
    assert_eq!(out.len(), 1, "{out:?}");
    let ts = run(&env(), &out);
    assert_eq!(summary(&ts), vec![(2, "dd".to_string(), vec![0])]);

    // Damage of 0 is not dealt (CR 120.8): no occurrence.
    let mut zero = dmg(DamageTarget::Player(P2));
    if let GameEvent::Damage { amount, .. } = &mut zero {
        *amount = 0;
    }
    let (batch, out) = mapped(&mut w, vec![zero]);
    assert!(batch.events.is_empty());
    assert!(out.is_empty(), "{out:?}");
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

// REQ-EVENT-002, CR 603.2: a counter kind that is a zone wire name ("HAND") is a Zone value, an
// ordinary name is a Name value, and "ANY" gives no kind field; for both AddCounters and
// RemoveCounters.
#[test]
fn spec_triggers_events_counter_kind_values() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let kinds = ["HAND", "poison", "ANY"];
    let expect = |t: &TriggerEvent, kind: &str| match kind {
        "HAND" => assert_eq!(t.fields.get("kind"), Some(&vzone(Zone::Hand))),
        "poison" => assert_eq!(
            t.fields.get("kind"),
            Some(&EventField::Value(name("poison")))
        ),
        _ => assert_eq!(t.fields.get("kind"), None),
    };
    for kind in kinds {
        let t = one(
            &mut w,
            GameEvent::AddCounters {
                target: CounterTarget::Object(a),
                kind: kind.into(),
                amount: 2,
            },
        );
        assert!(t.kinds.contains(&K::CountersPut));
        assert_eq!(t.fields.get("amount"), Some(&vint(2)));
        expect(&t, kind);
    }
    for kind in kinds {
        let t = one(
            &mut w,
            GameEvent::RemoveCounters {
                target: CounterTarget::Object(a),
                kind: kind.into(),
                amount: 1,
            },
        );
        assert!(t.kinds.contains(&K::CountersRemoved));
        assert_eq!(t.fields.get("amount"), Some(&vint(1)));
        expect(&t, kind);
    }
}

// REQ-EVENT-002, CR 603.2: detection over counter kinds. A {kind: Zone(HAND)} filter triggers for
// the "HAND" counter and not for "poison"; a {kind: ANY} filter triggers for all three.
#[test]
fn spec_triggers_events_counter_kind_detection() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let add = |kind: &str| GameEvent::AddCounters {
        target: CounterTarget::Object(a),
        kind: kind.into(),
        amount: 1,
    };
    let rem = |kind: &str| GameEvent::RemoveCounters {
        target: CounterTarget::Object(a),
        kind: kind.into(),
        amount: 1,
    };
    for (put, kinds) in [(true, K::CountersPut), (false, K::CountersRemoved)] {
        if !put {
            // Counters to remove must exist.
            for kind in ["HAND", "poison", "ANY"] {
                w.ok(add(kind));
            }
        }
        let (_, out) = mapped(
            &mut w,
            ["HAND", "poison", "ANY"]
                .into_iter()
                .map(|k| if put { add(k) } else { rem(k) })
                .collect(),
        );
        assert_eq!(out.len(), 3, "{out:?}");
        let env = FakeEnv::new()
            .with_post(reg(
                1,
                P1,
                ability("hand", filt(m(kinds), "kind", zone(Zone::Hand))),
            ))
            .with_post(reg(
                2,
                P1,
                ability(
                    "any",
                    filt(m(kinds), "kind", fineprint_dsl::ability::FilterValue::Any),
                ),
            ));
        let ts = run(&env, &out);
        assert_eq!(
            summary(&ts),
            vec![
                (1, "hand".to_string(), vec![0]),
                (2, "any".to_string(), vec![0]),
                (2, "any".to_string(), vec![1]),
                (2, "any".to_string(), vec![2]),
            ],
            "{kinds:?}"
        );
    }
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

// REQ-EVENT-002, CR 111.2, 701.7a, 603.6a: a created token maps to {TOKEN_CREATED, ENTERS} with
// only a post-event id (it has no origin zone, so no ZONE_CHANGE), its owner as subject_player
// (even when another player controls it) and the controller in a "controller" field.
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
    let expect: [(usize, PlayerId, PlayerId); 2] = [(0, P1, P1), (1, P1, P2)];
    for (i, owner, controller) in expect {
        assert_kinds_exactly(&out[i], &[K::TokenCreated, K::Enters]);
        assert_eq!(out[i].subject_pre, None);
        assert_eq!(out[i].subject_post, Some(token(i)));
        assert_eq!(out[i].subject_player, Some(owner));
        assert_eq!(
            out[i].fields.get("controller"),
            Some(&EventField::Player(controller))
        );
        // CR 701.7a: a created token enters the battlefield; it has no origin zone.
        assert_eq!(out[i].fields.get("to"), Some(&vzone(Zone::Battlefield)));
        assert!(!out[i].fields.contains_key("from"));
        assert!(!out[i].kinds.contains(&K::ZoneChange));
    }
}

// REQ-EVENT-002, CR 111.2, 701.7a, 603.6a: an ENTERS matcher (post view, object subject) triggers
// for both created tokens.
#[test]
fn spec_triggers_events_token_enters_detection() {
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
    let ids: Vec<u64> = batch
        .events
        .iter()
        .map(|e| match e.outcome {
            Outcome::TokenCreated { object } => object.get(),
            other => panic!("{other:?}"),
        })
        .collect();
    let env = FakeEnv::new()
        .accept(StateView::PostEvent, &ids)
        .with_post(reg(1, P1, ability("en", subj_obj(m(K::Enters)))));
    let ts = run(&env, &out);
    assert_eq!(
        summary(&ts),
        vec![
            (1, "en".to_string(), vec![0]),
            (1, "en".to_string(), vec![1])
        ]
    );

    // CR 701.7a, 603.6a: an ENTERS matcher filtered on `to: BATTLEFIELD` triggers for both tokens;
    // one filtered on `from: GRAVEYARD` does not.
    let to_bf = FakeEnv::new().with_post(reg(
        2,
        P1,
        ability("tb", filt(m(K::Enters), "to", zone(Zone::Battlefield))),
    ));
    assert_eq!(
        summary(&run(&to_bf, &out)),
        vec![
            (2, "tb".to_string(), vec![0]),
            (2, "tb".to_string(), vec![1])
        ]
    );
    let from_gy = FakeEnv::new().with_post(reg(
        3,
        P1,
        ability("fg", filt(m(K::Enters), "from", zone(Zone::Graveyard))),
    ));
    assert!(run(&from_gy, &out).is_empty());
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
