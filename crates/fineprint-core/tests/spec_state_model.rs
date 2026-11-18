// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::state`. Black-box: written from the work-item brief only.
//!
//! Model behaviour: construction, identifiers, N-player handling, per-player state,
//! snapshot independence and error values (REQ-CORE-001, REQ-CORE-002, REQ-NFR-010).

use std::collections::BTreeSet;

use fineprint_base::hash::{ComponentPath, Label};
use fineprint_base::ids::PlayerId;
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::state::{
    DesignationValue, EffectRegistry, GameState, HistoryLedger, MAX_PLAYERS, PlayerState,
    STATE_FORMAT_VERSION, StateError, TurnInfo,
};

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn order_raw(s: &GameState) -> Vec<u64> {
    s.turn_order().iter().map(|p| p.get()).collect()
}

fn ids(v: &[u64]) -> Vec<PlayerId> {
    v.iter().map(|&n| pid(n)).collect()
}

/// A three-player state with some content on player 2.
fn base() -> GameState {
    let mut s = GameState::new(5, 3, 20).unwrap();
    let p = s.player_mut(pid(2)).unwrap();
    p.set_counter("test.counter_a", 3);
    p.set_designation("test.designation_a", DesignationValue::Flag);
    s
}

// REQ-CORE-001: constants.
#[test]
fn constants_have_documented_values() {
    assert_eq!(MAX_PLAYERS, 256);
    assert_eq!(STATE_FORMAT_VERSION, 1);
}

// REQ-CORE-001, REQ-CORE-002: a fresh state.
#[test]
fn fresh_state_has_documented_contents() {
    let s = GameState::new(42, 2, 20).unwrap();
    assert_eq!(s.player_count(), 2);
    assert_eq!(order_raw(&s), vec![1, 2]);
    assert_eq!(*s.prng(), Pcg64Dxsm::from_u64_seed(42));
    assert_eq!(s.turn().turn_number(), 0);
    assert_eq!(s.turn().active_player(), None);
    assert_eq!(s.turn().priority_player(), None);
    assert_eq!(*s.turn(), TurnInfo::default());
    assert!(s.effects().is_empty());
    assert!(s.history().is_empty());
    assert_eq!(*s.effects(), EffectRegistry::default());
    assert_eq!(*s.history(), HistoryLedger::default());
    for p in s.players() {
        assert_eq!(p.life(), 20);
        assert!(p.counters().is_empty());
        assert!(p.designations().is_empty());
    }
    assert_eq!(s.ids().card.last_issued(), 0);
    assert_eq!(s.ids().object.last_issued(), 0);
    assert_eq!(s.ids().ability_instance.last_issued(), 0);
    assert_eq!(s.ids().effect.last_issued(), 0);
    assert_eq!(s.ids().timestamp.last_issued(), 0);
    assert_eq!(s.ids().link.last_issued(), 0);
    assert_eq!(s.ids().player.last_issued(), 2);
}

// REQ-CORE-001: the PRNG is the state's own, seeded from the seed.
#[test]
fn prng_is_seeded_and_advances_in_state() {
    let mut s = GameState::new(7, 1, 0).unwrap();
    let mut reference = Pcg64Dxsm::from_u64_seed(7);
    for _ in 0..10 {
        assert_eq!(s.prng_mut().next_u64(), reference.next_u64());
    }
    assert_eq!(*s.prng(), reference);
    assert_ne!(GameState::new(7, 1, 0).unwrap(), s);
    assert_ne!(
        GameState::new(7, 1, 0).unwrap(),
        GameState::new(8, 1, 0).unwrap()
    );
}

// REQ-NFR-010: construction errors.
#[test]
fn construction_limits() {
    assert_eq!(GameState::new(1, 0, 20), Err(StateError::NoPlayers));
    assert_eq!(GameState::new(1, 257, 20), Err(StateError::TooManyPlayers));
    assert_eq!(
        GameState::new(1, usize::MAX, 20),
        Err(StateError::TooManyPlayers)
    );
    assert!(GameState::new(1, 256, 20).is_ok());
    assert!(GameState::new(1, 1, i64::MIN).is_ok());
}

// REQ-CORE-002, REQ-NFR-010: player ids 1..=n in seat order.
#[test]
fn new_and_add_player_issue_sequential_ids() {
    let mut s = GameState::new(3, 3, 20).unwrap();
    assert_eq!(order_raw(&s), vec![1, 2, 3]);
    let got: Vec<u64> = s.players().map(|p| p.id().get()).collect();
    assert_eq!(got, vec![1, 2, 3]);
    assert_eq!(s.add_player(11), Ok(pid(4)));
    assert_eq!(s.add_player(12), Ok(pid(5)));
    assert_eq!(order_raw(&s), vec![1, 2, 3, 4, 5]);
    let got: Vec<u64> = s.players().map(|p| p.id().get()).collect();
    assert_eq!(got, vec![1, 2, 3, 4, 5]);
    assert_eq!(s.player(pid(4)).unwrap().life(), 11);
    assert_eq!(s.player(pid(5)).unwrap().life(), 12);
    assert_eq!(s.player(pid(1)).unwrap().life(), 20);
    assert_eq!(s.player_count(), 5);
    assert_eq!(s.ids().player.last_issued(), 5);
    for p in 1..=5u64 {
        assert_eq!(s.player(pid(p)).unwrap().id(), pid(p));
    }
}

// REQ-CORE-002: add_player appends to a permuted turn order.
#[test]
fn add_player_appends_to_turn_order() {
    let mut s = GameState::new(3, 3, 20).unwrap();
    s.set_turn_order(ids(&[3, 1, 2])).unwrap();
    s.add_player(1).unwrap();
    assert_eq!(order_raw(&s), vec![3, 1, 2, 4]);
    let got: Vec<u64> = s.players().map(|p| p.id().get()).collect();
    assert_eq!(got, vec![1, 2, 3, 4]);
}

// REQ-CORE-002: each generator starts at zero and issues 1, 2, 3 independently.
#[test]
fn id_generators_are_monotonic_and_independent() {
    let mut s = GameState::new(9, 2, 20).unwrap();
    let players_before = s.ids().player.last_issued();
    for expect in 1..=3u64 {
        assert_eq!(s.issue_card_id().unwrap().get(), expect);
        assert_eq!(s.ids().card.last_issued(), expect);
        assert_eq!(s.ids().object.last_issued(), 0);
        assert_eq!(s.ids().timestamp.last_issued(), 0);
    }
    for expect in 1..=3u64 {
        assert_eq!(s.issue_object_id().unwrap().get(), expect);
        assert_eq!(s.ids().object.last_issued(), expect);
    }
    for expect in 1..=3u64 {
        assert_eq!(s.issue_ability_instance_id().unwrap().get(), expect);
        assert_eq!(s.ids().ability_instance.last_issued(), expect);
    }
    for expect in 1..=3u64 {
        assert_eq!(s.issue_effect_id().unwrap().get(), expect);
        assert_eq!(s.ids().effect.last_issued(), expect);
    }
    for expect in 1..=3u64 {
        assert_eq!(s.issue_timestamp().unwrap().get(), expect);
        assert_eq!(s.ids().timestamp.last_issued(), expect);
    }
    for expect in 1..=3u64 {
        assert_eq!(s.issue_link_id().unwrap().get(), expect);
        assert_eq!(s.ids().link.last_issued(), expect);
    }
    assert_eq!(s.ids().card.last_issued(), 3);
    assert_eq!(s.ids().player.last_issued(), players_before);
}

// REQ-CORE-002: one shared Timestamp counter; interleaving other kinds changes nothing.
#[test]
fn timestamp_is_a_single_strictly_increasing_counter() {
    let mut s = GameState::new(9, 1, 20).unwrap();
    let mut seen = BTreeSet::new();
    let mut prev = 0;
    for i in 0..200u64 {
        if i % 3 == 0 {
            s.issue_effect_id().unwrap();
        }
        if i % 5 == 0 {
            s.issue_object_id().unwrap();
        }
        let t = s.issue_timestamp().unwrap().get();
        assert!(t > prev);
        assert_eq!(t, i + 1);
        assert!(seen.insert(t));
        prev = t;
    }
}

// REQ-CORE-002: a failing add_player does not burn a player id.
#[test]
fn player_ids_are_not_reused_or_skipped() {
    let mut s = GameState::new(1, 255, 0).unwrap();
    assert_eq!(s.add_player(0), Ok(pid(256)));
    assert_eq!(s.ids().player.last_issued(), 256);
    let before = s.clone();
    assert_eq!(s.add_player(0), Err(StateError::TooManyPlayers));
    assert_eq!(s, before);
    assert_eq!(s.ids().player.last_issued(), 256);
}

// REQ-NFR-010: sizes 1, 2, 3, 8, 256 - hash, round trip, turn order, per-player mutation.
#[test]
fn n_player_sizes() {
    for n in [1usize, 2, 3, 8, 256] {
        let mut s = GameState::new(n as u64, n, 20).unwrap();
        assert_eq!(s.player_count(), n);
        assert_eq!(s.players().count(), n);
        let want: Vec<u64> = (1..=n as u64).collect();
        assert_eq!(order_raw(&s), want);
        let fresh_hash = s.state_hash();
        assert_eq!(
            fresh_hash,
            GameState::new(n as u64, n, 20).unwrap().state_hash()
        );

        // Round trip.
        let b = s.serialize().unwrap();
        let d = GameState::deserialize(&b).unwrap();
        assert_eq!(d, s);
        assert_eq!(d.state_hash(), fresh_hash);

        // Rotations and reversal.
        let mut hashes = BTreeSet::new();
        for r in 0..n {
            let mut rot = want.clone();
            rot.rotate_left(r);
            s.set_turn_order(ids(&rot)).unwrap();
            assert_eq!(order_raw(&s), rot);
            hashes.insert(s.state_hash().to_hex());
            let d = GameState::deserialize(&s.serialize().unwrap()).unwrap();
            assert_eq!(d, s);
        }
        assert_eq!(hashes.len(), n, "each rotation hashes differently");
        let mut rev = want.clone();
        rev.reverse();
        s.set_turn_order(ids(&rev)).unwrap();
        assert_eq!(order_raw(&s), rev);
        s.set_turn_order(ids(&want)).unwrap();
        assert_eq!(s.state_hash(), fresh_hash);

        // Per-player mutation touches exactly one player.
        for id in 1..=n as u64 {
            let before = s.clone();
            s.player_mut(pid(id)).unwrap().set_life(1000 + id as i64);
            assert_ne!(s.state_hash(), before.state_hash());
            for other in 1..=n as u64 {
                let want_life = if other == id { 1000 + id as i64 } else { 20 };
                assert_eq!(s.player(pid(other)).unwrap().life(), want_life);
            }
            s.player_mut(pid(id)).unwrap().set_life(20);
            assert_eq!(s.state_hash(), fresh_hash);
            assert_eq!(s, before);
        }
    }
}

// REQ-NFR-010: the 257th player is refused with state unchanged.
#[test]
fn max_players_enforced_on_add() {
    let mut s = GameState::new(1, 256, 20).unwrap();
    let before = s.clone();
    let hash = s.state_hash();
    assert_eq!(s.add_player(5), Err(StateError::TooManyPlayers));
    assert_eq!(s, before);
    assert_eq!(s.state_hash(), hash);
    assert_eq!(s.player_count(), 256);
    assert_eq!(s.player(pid(257)), None);
}

// REQ-NFR-010: set_turn_order validation.
#[test]
fn set_turn_order_rejects_non_permutations() {
    let mut s = GameState::new(1, 3, 20).unwrap();
    s.set_turn_order(ids(&[2, 3, 1])).unwrap();
    let before = s.clone();
    let bad: [Vec<u64>; 9] = [
        vec![],
        vec![1, 1, 3],
        vec![2, 2, 2],
        vec![1, 2],
        vec![3],
        vec![1, 2, 3, 4],
        vec![1, 2, 3, 3],
        vec![1, 2, 99],
        vec![0, 1, 2],
    ];
    for b in bad {
        assert_eq!(
            s.set_turn_order(ids(&b)),
            Err(StateError::InvalidTurnOrder),
            "{b:?}"
        );
        assert_eq!(s, before, "{b:?}");
        assert_eq!(s.state_hash(), before.state_hash());
    }
    let mut one = GameState::new(1, 1, 20).unwrap();
    let before = one.clone();
    for b in [vec![], vec![1, 1], vec![2], vec![1, 2]] {
        assert_eq!(
            one.set_turn_order(ids(&b)),
            Err(StateError::InvalidTurnOrder)
        );
        assert_eq!(one, before);
    }
    assert_eq!(one.set_turn_order(ids(&[1])), Ok(()));
}

// REQ-NFR-010: active and priority player validation.
#[test]
fn active_and_priority_players_are_validated() {
    let mut s = GameState::new(1, 3, 20).unwrap();
    let before = s.clone();
    assert_eq!(
        s.set_active_player(Some(pid(99))),
        Err(StateError::UnknownPlayer(pid(99)))
    );
    assert_eq!(
        s.set_priority_player(Some(pid(99))),
        Err(StateError::UnknownPlayer(pid(99)))
    );
    assert_eq!(
        s.set_active_player(Some(pid(0))),
        Err(StateError::UnknownPlayer(pid(0)))
    );
    assert_eq!(s, before);
    assert_eq!(s.set_active_player(Some(pid(2))), Ok(()));
    assert_eq!(s.set_priority_player(Some(pid(3))), Ok(()));
    assert_eq!(s.turn().active_player(), Some(pid(2)));
    assert_eq!(s.turn().priority_player(), Some(pid(3)));
    let mid = s.clone();
    assert!(s.set_active_player(Some(pid(4))).is_err());
    assert_eq!(s, mid);
    assert_eq!(s.set_active_player(None), Ok(()));
    assert_eq!(s.set_priority_player(None), Ok(()));
    assert_eq!(s.turn().active_player(), None);
    assert_eq!(s.turn().priority_player(), None);
    s.set_turn_number(u64::MAX);
    assert_eq!(s.turn().turn_number(), u64::MAX);
}

// REQ-NFR-010: unknown players are None.
#[test]
fn unknown_players_are_absent() {
    let mut s = GameState::new(1, 2, 20).unwrap();
    assert!(s.player(pid(0)).is_none());
    assert!(s.player(pid(3)).is_none());
    assert!(s.player(pid(u64::MAX)).is_none());
    assert!(s.player_mut(pid(0)).is_none());
    assert!(s.player_mut(pid(3)).is_none());
    assert!(s.player(pid(1)).is_some());
    assert!(s.player_mut(pid(2)).is_some());
}

// REQ-CORE-001: PlayerState behaviour.
#[test]
fn player_life_takes_extremes() {
    let mut s = GameState::new(1, 2, 20).unwrap();
    for v in [i64::MIN, -1, 0, 1, i64::MAX, 20] {
        s.player_mut(pid(1)).unwrap().set_life(v);
        assert_eq!(s.player(pid(1)).unwrap().life(), v);
        assert_eq!(s.player(pid(2)).unwrap().life(), 20);
    }
}

// REQ-CORE-001: counters.
#[test]
fn player_counters() {
    let mut s = GameState::new(1, 2, 20).unwrap();
    let p = s.player_mut(pid(2)).unwrap();
    assert_eq!(p.counter("test.counter_a"), 0);
    assert_eq!(p.counter(""), 0);
    p.set_counter("test.counter_a", 5);
    p.set_counter("", u64::MAX);
    p.set_counter("\u{4e2d}", 1);
    assert_eq!(p.counter("test.counter_a"), 5);
    assert_eq!(p.counter(""), u64::MAX);
    assert_eq!(p.counters().len(), 3);
    p.set_counter("test.counter_a", 7);
    assert_eq!(p.counter("test.counter_a"), 7);
    assert_eq!(p.counters().len(), 3);
    p.set_counter("test.counter_a", 0);
    assert_eq!(p.counter("test.counter_a"), 0);
    assert!(!p.counters().contains_key("test.counter_a"));
    p.set_counter("never_set", 0);
    assert!(!p.counters().contains_key("never_set"));
    assert!(p.counters().values().all(|&v| v != 0));
    let keys: Vec<&String> = p.counters().keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "counters iterate in key order");
    // The other player is unaffected.
    assert!(s.player(pid(1)).unwrap().counters().is_empty());
}

// REQ-CORE-001: designations.
#[test]
fn player_designations() {
    let mut s = GameState::new(1, 1, 20).unwrap();
    let p = s.player_mut(pid(1)).unwrap();
    assert_eq!(p.designation("test.designation_a"), None);
    assert_eq!(p.clear_designation("test.designation_a"), None);
    p.set_designation("test.designation_a", DesignationValue::Flag);
    p.set_designation("test.designation_b", DesignationValue::Int(i64::MIN));
    p.set_designation("test.designation_c", DesignationValue::Text("x".into()));
    assert_eq!(
        p.designation("test.designation_a"),
        Some(&DesignationValue::Flag)
    );
    assert_eq!(
        p.designation("test.designation_b"),
        Some(&DesignationValue::Int(i64::MIN))
    );
    assert_eq!(
        p.designation("test.designation_c"),
        Some(&DesignationValue::Text("x".into()))
    );
    assert_eq!(p.designations().len(), 3);
    // Replace, including a change of variant.
    p.set_designation("test.designation_a", DesignationValue::Int(4));
    assert_eq!(
        p.designation("test.designation_a"),
        Some(&DesignationValue::Int(4))
    );
    assert_eq!(p.designations().len(), 3);
    // Clear returns the old value, once.
    assert_eq!(
        p.clear_designation("test.designation_a"),
        Some(DesignationValue::Int(4))
    );
    assert_eq!(p.clear_designation("test.designation_a"), None);
    assert_eq!(p.designation("test.designation_a"), None);
    assert_eq!(p.designations().len(), 2);
    assert_eq!(
        p.clear_designation("test.designation_c"),
        Some(DesignationValue::Text("x".into()))
    );
    assert_ne!(DesignationValue::Flag, DesignationValue::Int(0));
    assert_ne!(DesignationValue::Int(1), DesignationValue::Text("1".into()));
}

// REQ-CORE-001: a standalone PlayerState.
#[test]
fn player_state_standalone() {
    let mut p = PlayerState::new(pid(7), -3);
    assert_eq!(p.id(), pid(7));
    assert_eq!(p.life(), -3);
    assert!(p.counters().is_empty());
    assert!(p.designations().is_empty());
    let q = p.clone();
    p.set_life(1);
    assert_ne!(p, q);
    assert_eq!(q.life(), -3);
}

// REQ-CORE-001: fresh TurnInfo, EffectRegistry, HistoryLedger.
#[test]
fn turn_effects_history_defaults() {
    let t = TurnInfo::default();
    assert_eq!(t.turn_number(), 0);
    assert_eq!(t.active_player(), None);
    assert_eq!(t.priority_player(), None);
    assert!(EffectRegistry::default().is_empty());
    assert!(HistoryLedger::default().is_empty());
}

// REQ-CORE-001: effects/history leaves encode as four zero bytes.
#[test]
fn effects_and_history_leaves_are_four_zero_bytes() {
    let s = base();
    let t = s.merkle_tree();
    let eff = ComponentPath::new(vec![Label::name("effects")]);
    let his = ComponentPath::new(vec![Label::name("history")]);
    assert_eq!(t.leaf_payload(&eff), Some(&[0u8, 0, 0, 0][..]));
    assert_eq!(t.leaf_payload(&his), Some(&[0u8, 0, 0, 0][..]));
}

/// Number of mutators in [`apply_mutator`].
const MUTATORS: usize = 17;

/// Applies mutator `k` to `s`; each one changes `base()` by value.
fn apply_mutator(k: usize, s: &mut GameState) {
    match k {
        0 => s.player_mut(pid(2)).unwrap().set_life(99),
        1 => s
            .player_mut(pid(2))
            .unwrap()
            .set_counter("test.counter_b", 4),
        2 => s
            .player_mut(pid(2))
            .unwrap()
            .set_counter("test.counter_a", 0),
        3 => s
            .player_mut(pid(2))
            .unwrap()
            .set_designation("test.designation_b", DesignationValue::Int(5)),
        4 => {
            s.player_mut(pid(2))
                .unwrap()
                .clear_designation("test.designation_a");
        }
        5 => {
            s.prng_mut().next_u64();
        }
        6 => {
            s.issue_card_id().unwrap();
        }
        7 => {
            s.issue_object_id().unwrap();
        }
        8 => {
            s.issue_ability_instance_id().unwrap();
        }
        9 => {
            s.issue_effect_id().unwrap();
        }
        10 => {
            s.issue_timestamp().unwrap();
        }
        11 => {
            s.issue_link_id().unwrap();
        }
        12 => {
            s.add_player(7).unwrap();
        }
        13 => s.set_turn_order(ids(&[3, 2, 1])).unwrap(),
        14 => s.set_turn_number(9),
        15 => s.set_active_player(Some(pid(2))).unwrap(),
        16 => s.set_priority_player(Some(pid(3))).unwrap(),
        _ => unreachable!(),
    }
}

// REQ-CORE-001: mutating the original never affects a snapshot.
#[test]
fn snapshot_unaffected_by_mutating_original() {
    for k in 0..MUTATORS {
        let mut s = base();
        let snap = s.clone();
        let snap_hash = snap.state_hash();
        let snap_bytes = snap.serialize().unwrap();
        apply_mutator(k, &mut s);
        assert_ne!(s, snap, "mutator {k} changes the state");
        assert_ne!(s.state_hash(), snap_hash, "mutator {k}");
        assert_eq!(snap, base(), "mutator {k}");
        assert_eq!(snap.state_hash(), snap_hash, "mutator {k}");
        assert_eq!(snap.serialize().unwrap(), snap_bytes, "mutator {k}");
    }
}

// REQ-CORE-001: mutating the clone never affects the original.
#[test]
fn original_unaffected_by_mutating_snapshot() {
    for k in 0..MUTATORS {
        let s = base();
        let s_hash = s.state_hash();
        let s_bytes = s.serialize().unwrap();
        let mut snap = s.clone();
        apply_mutator(k, &mut snap);
        assert_ne!(snap, s, "mutator {k}");
        assert_eq!(s, base(), "mutator {k}");
        assert_eq!(s.state_hash(), s_hash, "mutator {k}");
        assert_eq!(s.serialize().unwrap(), s_bytes, "mutator {k}");
    }
}

// REQ-CORE-001: structural sharing is observable and never affects value semantics.
#[test]
fn shares_player_with_observes_sharing() {
    let s = GameState::new(1, 4, 20).unwrap();
    let c = s.clone();
    for p in 1..=4 {
        assert!(s.shares_player_with(&c, pid(p)));
        assert!(c.shares_player_with(&s, pid(p)));
    }
    assert!(!s.shares_player_with(&c, pid(99)));
    assert!(!s.shares_player_with(&c, pid(0)));
    for target in 1..=4u64 {
        let mut w = s.clone();
        w.player_mut(pid(target)).unwrap().set_life(1);
        for p in 1..=4u64 {
            assert_eq!(
                s.shares_player_with(&w, pid(p)),
                p != target,
                "write to {target}, check {p}"
            );
        }
        assert_ne!(w, s);
        assert_eq!(s, c);
    }
    // Independently built states are equal but never share.
    let other = GameState::new(1, 4, 20).unwrap();
    assert_eq!(other, s);
    assert_eq!(other.state_hash(), s.state_hash());
    for p in 1..=4 {
        assert!(!s.shares_player_with(&other, pid(p)));
    }
    // Writing and restoring a value gives an equal, equal-hash, unshared state.
    let mut w = s.clone();
    w.player_mut(pid(2)).unwrap().set_life(5);
    w.player_mut(pid(2)).unwrap().set_life(20);
    assert_eq!(w, s);
    assert_eq!(w.state_hash(), s.state_hash());
    assert!(!w.shares_player_with(&s, pid(2)));
    assert!(w.shares_player_with(&s, pid(1)));
    // A clone that adds a player still shares the existing ones.
    let mut grown = s.clone();
    grown.add_player(20).unwrap();
    for p in 1..=4 {
        assert!(grown.shares_player_with(&s, pid(p)));
    }
    assert!(!grown.shares_player_with(&s, pid(5)));
}

// REQ-CORE-001, REQ-NFR-010: a failing mutator leaves the state equal to its prior clone.
#[test]
fn failing_mutators_leave_state_unchanged() {
    let mut s = base();
    let before = s.clone();
    let bytes = s.serialize().unwrap();
    assert!(s.set_turn_order(ids(&[1, 2])).is_err());
    assert!(s.set_turn_order(ids(&[1, 1, 2])).is_err());
    assert!(s.set_active_player(Some(pid(99))).is_err());
    assert!(s.set_priority_player(Some(pid(99))).is_err());
    assert_eq!(s, before);
    assert_eq!(s.serialize().unwrap(), bytes);
    assert_eq!(s.state_hash(), before.state_hash());
    let mut full = GameState::new(1, 256, 20).unwrap();
    let fb = full.clone();
    assert!(full.add_player(1).is_err());
    assert_eq!(full, fb);
    assert!(full.shares_player_with(&fb, pid(256)));
}

// REQ-CORE-001: Display and Error for every StateError variant.
#[test]
fn state_error_display_and_error_trait() {
    let all = vec![
        StateError::NoPlayers,
        StateError::TooManyPlayers,
        StateError::IdExhausted,
        StateError::UnknownPlayer(pid(3)),
        StateError::InvalidTurnOrder,
        StateError::Encode("encode detail".to_string()),
        StateError::Decode("decode detail".to_string()),
    ];
    let mut messages = BTreeSet::new();
    for e in &all {
        let m = e.to_string();
        assert!(!m.is_empty(), "{e:?}");
        assert!(!format!("{e:?}").is_empty());
        messages.insert(m);
        let boxed: Box<dyn std::error::Error> = Box::new(e.clone());
        assert_eq!(boxed.to_string(), e.to_string());
        let sendable: Box<dyn std::error::Error + Send + Sync> = Box::new(e.clone());
        assert!(!sendable.to_string().is_empty());
    }
    assert_eq!(messages.len(), all.len(), "variant messages are distinct");
    assert_eq!(StateError::NoPlayers, StateError::NoPlayers);
    assert_ne!(StateError::NoPlayers, StateError::TooManyPlayers);
    assert_ne!(
        StateError::UnknownPlayer(pid(1)),
        StateError::UnknownPlayer(pid(2))
    );
}
