// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests: 613.7 timestamps issued through `GameState` from its own timestamp generator
//! (T7) and rolled back with the state (T8). REQ-CORE-002, REQ-CORE-001, REQ-LAYER-001,
//! REQ-NFR-006. Black-box: written from the work-item brief (#183 FP-144) only.

#[path = "spec_layers_state_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{CounterHolder, LayerModification, SimultaneousStamp, StampReason};
use fineprint_core::state::{GameState, GeneratingAbility, StateError};
use fineprint_core::txn::Transactions;
use support::*;

const REASONS: [StampReason; 7] = [
    StampReason::EntersZone,
    StampReason::Attached,
    StampReason::TurnedFaceUpOrDown,
    StampReason::TransformedOrConverted,
    StampReason::SpecialCardTurnedFaceUp,
    StampReason::GameStart,
    StampReason::JoinedMergedPermanent,
];

fn last_ts(s: &GameState) -> u64 {
    s.ids().timestamp.last_issued()
}

// REQ-CORE-002, CR 613.7d, CR 613.7e, CR 613.7f, CR 613.7g, CR 613.7h, CR 613.7i, CR 613.7j
// (T7): `stamp_object` issues strictly increasing timestamps from the state's own generator,
// for every reason, re-stamping allowed; the generator advances by exactly one per stamp and
// the ledger records the latest.
#[test]
fn stamp_object_is_strictly_increasing_from_the_state_generator() {
    let mut s = GameState::new(1, 2, 20).unwrap();
    s.issue_timestamp().unwrap();
    let mut prev = last_ts(&s);
    for i in 0..40u64 {
        let o = oid(1 + i % 5);
        let before = last_ts(&s);
        let t = s.stamp_object(o, REASONS[(i % 7) as usize]).unwrap();
        assert!(t.get() > prev);
        assert_eq!(last_ts(&s), before + 1);
        assert_eq!(t.get(), last_ts(&s));
        assert_eq!(s.timestamps().object_timestamp(o), Some(t));
        prev = t.get();
        // Interleaved plain timestamps share the one counter.
        if i % 4 == 0 {
            let u = s.issue_timestamp().unwrap();
            assert!(u.get() > prev);
            prev = u.get();
        }
    }
    assert_eq!(s.timestamps().object_timestamp(oid(99)), None);
}

// REQ-CORE-002, CR 613.7k (T7): stickers get a timestamp when put on; when their object gets a
// new timestamp each sticker gets a new one immediately after it, keeping their relative order;
// the generator advances by one plus the number of stickers.
#[test]
fn stickers_follow_their_object() {
    let mut s = GameState::new(1, 2, 20).unwrap();
    let host = oid(1);
    let s9 = s.stamp_sticker(host, 9).unwrap();
    let s2 = s.stamp_sticker(host, 2).unwrap();
    let s5 = s.stamp_sticker(host, 5).unwrap();
    s.stamp_sticker(oid(2), 1).unwrap();
    assert!(s9 < s2 && s2 < s5);
    assert_eq!(s.timestamps().sticker_timestamp(host, 2), Some(s2));
    let before = last_ts(&s);
    let t = s.stamp_object(host, StampReason::EntersZone).unwrap();
    assert_eq!(last_ts(&s), before + 4);
    assert_eq!(t.get(), before + 1);
    // Previous relative order 9, 2, 5.
    assert_eq!(
        s.timestamps().sticker_timestamp(host, 9),
        Some(Timestamp::new(t.get() + 1))
    );
    assert_eq!(
        s.timestamps().sticker_timestamp(host, 2),
        Some(Timestamp::new(t.get() + 2))
    );
    assert_eq!(
        s.timestamps().sticker_timestamp(host, 5),
        Some(Timestamp::new(t.get() + 3))
    );
    // Another object's sticker is untouched.
    assert_eq!(
        s.timestamps().sticker_timestamp(oid(2), 1),
        Some(Timestamp::new(4))
    );
}

// REQ-CORE-002, CR 613.7c (T7): counters of a kind on a holder share the latest timestamp;
// `clear_counter_stamp` forgets it; holders and kinds are independent.
#[test]
fn counter_stamps() {
    let mut s = GameState::new(1, 2, 20).unwrap();
    let h = CounterHolder::Object(oid(1));
    let p = CounterHolder::Player(P2);
    let a = s.stamp_counters(h, "+1/+1").unwrap();
    let b = s.stamp_counters(p, "+1/+1").unwrap();
    let c = s.stamp_counters(h, "test.kind").unwrap();
    assert!(a < b && b < c);
    assert_eq!(s.timestamps().counter_timestamp(h, "+1/+1"), Some(a));
    let a2 = s.stamp_counters(h, "+1/+1").unwrap();
    assert!(a2 > c);
    assert_eq!(s.timestamps().counter_timestamp(h, "+1/+1"), Some(a2));
    assert_eq!(s.timestamps().counter_timestamp(p, "+1/+1"), Some(b));
    s.clear_counter_stamp(h, "+1/+1");
    assert_eq!(s.timestamps().counter_timestamp(h, "+1/+1"), None);
    assert_eq!(s.timestamps().counter_timestamp(h, "test.kind"), Some(c));
    assert_eq!(s.timestamps().counter_timestamp(p, "+1/+1"), Some(b));
    // Clearing an absent stamp changes nothing.
    let before = s.clone();
    s.clear_counter_stamp(CounterHolder::Player(P1), "+1/+1");
    assert_eq!(s, before);
}

// REQ-CORE-002, CR 613.7b (T7): a resolved effect's timestamp is later than every earlier
// stamp.
#[test]
fn resolved_effect_stamp_is_latest() {
    let mut s = GameState::new(1, 2, 20).unwrap();
    let mut all: Vec<Timestamp> = vec![
        s.stamp_object(oid(1), StampReason::EntersZone).unwrap(),
        s.stamp_sticker(oid(1), 1).unwrap(),
        s.stamp_counters(CounterHolder::Object(oid(1)), "+1/+1")
            .unwrap(),
        s.stamp_object(oid(1), StampReason::Attached).unwrap(),
    ];
    all.push(s.timestamps().sticker_timestamp(oid(1), 1).unwrap());
    let r = s.stamp_resolved_effect().unwrap();
    assert!(all.iter().all(|t| *t < r));
    assert_eq!(r.get(), last_ts(&s));
    let r2 = s.stamp_resolved_effect().unwrap();
    assert!(r2 > r);
}

// REQ-CORE-002, CR 613.7 (T7): `forget_object_stamps` forgets the object's timestamp, its
// stickers and the counters on it; nothing else.
#[test]
fn forget_object_stamps_forgets_only_that_object() {
    let mut s = GameState::new(1, 2, 20).unwrap();
    s.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    s.stamp_sticker(oid(1), 1).unwrap();
    s.stamp_counters(CounterHolder::Object(oid(1)), "+1/+1")
        .unwrap();
    let o2 = s.stamp_object(oid(2), StampReason::EntersZone).unwrap();
    let pc = s
        .stamp_counters(CounterHolder::Player(P1), "+1/+1")
        .unwrap();
    s.forget_object_stamps(oid(1));
    let l = s.timestamps();
    assert_eq!(l.object_timestamp(oid(1)), None);
    assert_eq!(l.sticker_timestamp(oid(1), 1), None);
    assert_eq!(
        l.counter_timestamp(CounterHolder::Object(oid(1)), "+1/+1"),
        None
    );
    assert_eq!(l.object_timestamp(oid(2)), Some(o2));
    assert_eq!(
        l.counter_timestamp(CounterHolder::Player(P1), "+1/+1"),
        Some(pc)
    );
    // The generator never rewinds.
    let next = s.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    assert!(next > pc);
}

fn stamp(object: u64, player: PlayerId, choice: u32) -> SimultaneousStamp {
    SimultaneousStamp {
        object: oid(object),
        player,
        choice,
    }
}

fn order_of(out: &[(ObjectId, Timestamp)]) -> Vec<u64> {
    out.iter().map(|(o, _)| o.get()).collect()
}

// REQ-CORE-002, CR 613.7m (T7): a simultaneous batch is stamped in APNAP order (the active
// player first, then the others in turn order), each player's objects in that player's chosen
// order; timestamps are consecutive and recorded; with no active player the turn order alone
// decides.
#[test]
fn simultaneous_stamps_follow_apnap_and_choice() {
    let batch = [
        stamp(10, P1, 1),
        stamp(11, P1, 0),
        stamp(20, P2, 5),
        stamp(21, P2, 2),
        stamp(30, PlayerId::new(3), 0),
    ];
    let p3 = PlayerId::new(3);

    // Active player 3, turn order 1, 2, 3: APNAP is 3, 1, 2. (The active player is the last
    // in the turn order in each case here, so the order of the other players does not depend
    // on whether the turn order is read cyclically from the active player; see the report.)
    let mut s = GameState::new(1, 3, 20).unwrap();
    s.set_active_player(Some(p3)).unwrap();
    s.issue_timestamp().unwrap();
    let out = s
        .stamp_objects_simultaneously(&batch, StampReason::EntersZone)
        .unwrap();
    assert_eq!(order_of(&out), vec![30, 11, 10, 21, 20]);
    for (i, (o, t)) in out.iter().enumerate() {
        assert_eq!(t.get(), 2 + i as u64);
        assert_eq!(s.timestamps().object_timestamp(*o), Some(*t));
    }
    assert_eq!(last_ts(&s), 6);

    // A custom turn order 3, 1, 2 with player 2 active: APNAP 2, 3, 1.
    let mut s = GameState::new(1, 3, 20).unwrap();
    s.set_turn_order(vec![p3, P1, P2]).unwrap();
    s.set_active_player(Some(P2)).unwrap();
    let out = s
        .stamp_objects_simultaneously(&batch, StampReason::Attached)
        .unwrap();
    assert_eq!(order_of(&out), vec![21, 20, 30, 11, 10]);

    // No active player: the turn order 3, 1, 2.
    let mut s = GameState::new(1, 3, 20).unwrap();
    s.set_turn_order(vec![p3, P1, P2]).unwrap();
    let out = s
        .stamp_objects_simultaneously(&batch, StampReason::EntersZone)
        .unwrap();
    assert_eq!(order_of(&out), vec![30, 11, 10, 21, 20]);

    // Every permutation of the batch input gives the same result.
    let mut reference: Option<Vec<(ObjectId, Timestamp)>> = None;
    let mut idx: Vec<usize> = (0..batch.len()).collect();
    let mut rng = Rng::new(0x6137);
    for _ in 0..30 {
        for i in (1..idx.len()).rev() {
            let j = rng.below(i as u64 + 1) as usize;
            idx.swap(i, j);
        }
        let shuffled: Vec<SimultaneousStamp> = idx.iter().map(|&i| batch[i]).collect();
        let mut s = GameState::new(1, 3, 20).unwrap();
        s.set_active_player(Some(p3)).unwrap();
        let out = s
            .stamp_objects_simultaneously(&shuffled, StampReason::EntersZone)
            .unwrap();
        assert_eq!(order_of(&out), vec![30, 11, 10, 21, 20]);
        if let Some(r) = &reference {
            assert_eq!(&out, r);
        }
        reference = Some(out);
    }

    // Stickers follow each object (CR 613.7k).
    let mut s = GameState::new(1, 2, 20).unwrap();
    s.set_active_player(Some(P1)).unwrap();
    s.stamp_sticker(oid(1), 1).unwrap();
    let out = s
        .stamp_objects_simultaneously(&[stamp(2, P2, 0), stamp(1, P1, 0)], StampReason::EntersZone)
        .unwrap();
    assert_eq!(order_of(&out), vec![1, 2]);
    assert_eq!(out[0].1.get(), 2);
    assert_eq!(
        s.timestamps().sticker_timestamp(oid(1), 1),
        Some(Timestamp::new(3))
    );
    assert_eq!(out[1].1.get(), 4);

    // An empty batch issues nothing.
    let mut s = GameState::new(1, 2, 20).unwrap();
    let before = s.clone();
    assert!(
        s.stamp_objects_simultaneously(&[], StampReason::EntersZone)
            .unwrap()
            .is_empty()
    );
    assert_eq!(s, before);
}

// REQ-CORE-002, REQ-CORE-001 (T7): a batch naming an unknown player or an object twice is
// rejected with the state unchanged.
#[test]
fn simultaneous_stamp_errors_leave_state_unchanged() {
    let mut s = GameState::new(1, 2, 20).unwrap();
    let before = s.clone();
    assert!(
        s.stamp_objects_simultaneously(
            &[stamp(1, P1, 0), stamp(2, PlayerId::new(7), 0)],
            StampReason::EntersZone
        )
        .is_err()
    );
    assert!(
        s.stamp_objects_simultaneously(
            &[stamp(1, P1, 0), stamp(1, P2, 1)],
            StampReason::EntersZone
        )
        .is_err()
    );
    assert_eq!(s, before);
    assert_eq!(s.state_hash(), before.state_hash());
}

/// A state whose timestamp generator has issued `last` (built by deserializing hand-edited
/// bytes, the brief's technique).
fn with_timestamp_generator_at(last: u64) -> GameState {
    let s = GameState::new(1, 2, 20).unwrap();
    let mut v = wire(&s.serialize().unwrap());
    v["ids"]["timestamp"] = serde_json::json!(last);
    let s = GameState::deserialize(&unwire(&v)).unwrap();
    assert_eq!(last_ts(&s), last);
    s
}

/// Asserts `f` returns `Err(StateError::IdExhausted)` and leaves `s` unchanged.
fn assert_exhausted<T: std::fmt::Debug>(
    s: &GameState,
    what: &str,
    f: impl FnOnce(&mut GameState) -> Result<T, StateError>,
) {
    let mut c = s.clone();
    let r = f(&mut c);
    assert!(matches!(r, Err(StateError::IdExhausted)), "{what}: {r:?}");
    assert_eq!(&c, s, "{what}: unchanged");
    assert_eq!(c.state_hash(), s.state_hash(), "{what}: hash");
    assert_eq!(c.timestamps(), s.timestamps(), "{what}: no partial stamps");
}

// REQ-CORE-002, REQ-CORE-001 (T7): when the timestamp generator is exhausted every stamping
// wrapper returns `Err(StateError::IdExhausted)` with the state, its hash and its ledger
// unchanged and no partial stamps (an object whose stickers cannot all be re-stamped, a
// batch that does not fit).
#[test]
fn exhaustion_is_all_or_nothing() {
    // One timestamp left.
    let mut s = with_timestamp_generator_at(u64::MAX - 1);
    let t = s.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    assert_eq!(t.get(), u64::MAX);
    assert_exhausted(&s, "object", |c| {
        c.stamp_object(oid(2), StampReason::EntersZone)
    });
    assert_exhausted(&s, "re-stamp", |c| {
        c.stamp_object(oid(1), StampReason::Attached)
    });
    assert_exhausted(&s, "sticker", |c| c.stamp_sticker(oid(1), 1));
    assert_exhausted(&s, "counters", |c| {
        c.stamp_counters(CounterHolder::Object(oid(1)), "+1/+1")
    });
    assert_exhausted(&s, "resolved", |c| c.stamp_resolved_effect());
    assert_exhausted(&s, "batch", |c| {
        c.stamp_objects_simultaneously(&[stamp(3, P1, 0)], StampReason::EntersZone)
    });

    // Two left: an object with a sticker needs two after the sticker took one.
    let mut s = with_timestamp_generator_at(u64::MAX - 2);
    s.stamp_sticker(oid(1), 1).unwrap();
    assert_eq!(last_ts(&s), u64::MAX - 1);
    assert_exhausted(&s, "object with sticker", |c| {
        c.stamp_object(oid(1), StampReason::EntersZone)
    });
    // A batch of two with one left.
    assert_exhausted(&s, "batch of two", |c| {
        c.stamp_objects_simultaneously(&[stamp(2, P1, 0), stamp(3, P2, 0)], StampReason::EntersZone)
    });
    // The last one is still usable.
    let t = s.stamp_object(oid(5), StampReason::EntersZone).unwrap();
    assert_eq!(t.get(), u64::MAX);
}

// ---------------------------------------------------------------------------------------------
// T8: rollback
// ---------------------------------------------------------------------------------------------

fn game() -> GameState {
    let mut s = GameState::new(0x5EED_0183, 2, 20).unwrap();
    s.set_active_player(Some(P1)).unwrap();
    s.set_priority_player(Some(P1)).unwrap();
    for _ in 0..4 {
        s.issue_object_id().unwrap();
    }
    s.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    s
}

fn add_layer_content(s: &mut GameState) {
    let t = s.stamp_object(oid(2), StampReason::EntersZone).unwrap();
    s.stamp_sticker(oid(2), 3).unwrap();
    s.stamp_counters(CounterHolder::Object(oid(2)), "+1/+1")
        .unwrap();
    s.register_effect(
        static_sel(
            oid(2),
            t,
            Layer::L6,
            SEL_ALL,
            LayerModification::AddAbility(marker()),
        ),
        Some(GeneratingAbility::new(oid(2), ability("test.o2"))),
    )
    .unwrap();
    let r = s.stamp_resolved_effect().unwrap();
    s.register_effect(
        resolved(
            r,
            Layer::L7c,
            &[1],
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
        None,
    )
    .unwrap();
    let etb = s.issue_effect_id().unwrap();
    s.link_effect_generator(etb, GeneratingAbility::new(oid(3), ability("test.o3")))
        .unwrap();
}

// REQ-CORE-001, REQ-CORE-002, REQ-LAYER-001 (T8, REQ-CORE-005 style): effects, timestamps
// and links added after a checkpoint are removed by rolling back to it.
#[test]
fn rollback_reverts_effects_timestamps_and_links() {
    let mut txns = Transactions::new(game());
    let checkpoint = txns.state().clone();
    let outer = txns.checkpoint();
    add_layer_content(txns.state_mut());
    assert_ne!(txns.state().state_hash(), checkpoint.state_hash());
    txns.rollback(outer).unwrap();
    assert_eq!(txns.state(), &checkpoint);
    assert_eq!(txns.state().state_hash(), checkpoint.state_hash());
    assert!(txns.state().effects().is_empty());
    assert_eq!(txns.state().timestamps(), checkpoint.timestamps());
    // A plain commit keeps everything.
    let mut txns = Transactions::new(game());
    let outer = txns.checkpoint();
    add_layer_content(txns.state_mut());
    let live = txns.state().clone();
    txns.commit(outer).unwrap();
    assert_eq!(txns.into_state().unwrap(), live);
}

// REQ-CORE-002, REQ-CORE-001 (T8, REQ-CORE-005 style): effect ids and timestamps issued inside
// a durable sub-transaction are never issued again after the parent is rolled back.
#[test]
fn durable_sub_ids_and_timestamps_are_never_reused() {
    let mut txns = Transactions::new(game());
    let outer = txns.checkpoint();
    let sub = txns.checkpoint();
    add_layer_content(txns.state_mut());
    let effect_hw = txns.state().ids().effect.last_issued();
    let ts_hw = last_ts(txns.state());
    let used_effects: Vec<u64> = txns
        .state()
        .effects()
        .iter()
        .map(|(id, _)| id.get())
        .collect();
    txns.commit_durable(sub).unwrap();
    txns.rollback(outer).unwrap();
    let s = txns.state_mut();
    assert!(s.ids().effect.last_issued() >= effect_hw);
    assert!(last_ts(s) >= ts_hw);
    let t = s.stamp_object(oid(4), StampReason::EntersZone).unwrap();
    assert!(t.get() > ts_hw, "timestamp {t:?} reused");
    let r = s.stamp_resolved_effect().unwrap();
    let id = s
        .register_effect(
            resolved(
                r,
                Layer::L5,
                &[4],
                LayerModification::AddColors(ColorBitmask::RED),
            ),
            None,
        )
        .unwrap();
    assert!(id.get() > effect_hw, "effect id {id:?} reused");
    assert!(!used_effects.contains(&id.get()));
    let etb = s.issue_effect_id().unwrap();
    assert!(etb.get() > effect_hw);
}
