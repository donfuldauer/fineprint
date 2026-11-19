// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::txn` spec tests (REQ-CORE-005, CR 734.1, CR 734.2).
//! Black-box: written from the work-item brief only.
//!
//! Each `spec_txn_*.rs` test file includes this file with `#[path]`; on its own it is an empty
//! test target. All data is synthetic (R5): counter kinds and designation keys are neutral test
//! strings.

#![allow(dead_code)]

use std::fmt::Debug;

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::PlayerId;
use fineprint_core::state::{DesignationValue, GameState};
use fineprint_core::txn::{Transactional, Transactions, TxnError};

/// A player id from its raw value. `GameState::new` issues players `1..=player_count`.
pub fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

/// A synthetic game state with `players` players at 20 life, a fixed seed, turn 3, player 1
/// active and holding priority, and some pre-existing counters and designations so that
/// removals are observable.
pub fn game(players: usize) -> GameState {
    let mut s = GameState::new(0x5EED_0026, players, 20).expect("valid player count");
    s.set_turn_number(3);
    s.set_active_player(Some(pid(1))).unwrap();
    s.set_priority_player(Some(pid(1))).unwrap();
    for raw in 1..=players as u64 {
        let mut p = s.player_mut(pid(raw)).unwrap();
        p.set_counter("test.counter_a", 2);
        p.set_counter("test.counter_b", 5);
        p.set_designation("test.flag", DesignationValue::Flag);
        p.set_designation("test.level", DesignationValue::Int(1));
    }
    s
}

/// Draws `n` values from the state's PRNG.
pub fn draw(s: &mut GameState, n: usize) {
    for _ in 0..n {
        s.prng_mut().next_u64();
    }
}

/// Issues `n` object ids.
pub fn issue_objects(s: &mut GameState, n: usize) {
    for _ in 0..n {
        s.issue_object_id().unwrap();
    }
}

/// Sets player `who`'s life.
pub fn set_life(s: &mut GameState, who: u64, life: i64) {
    s.player_mut(pid(who)).unwrap().set_life(life);
}

/// Mutates every component reachable through the public API: life, counters, designations,
/// PRNG, every id generator, priority, active player, turn number and turn order.
pub fn mutate_everything(s: &mut GameState) {
    let n = s.player_count() as u64;
    for raw in 1..=n {
        let mut p = s.player_mut(pid(raw)).unwrap();
        let life = p.life();
        p.set_life(life - 3 - raw as i64);
        p.set_counter("test.counter_a", 0);
        p.set_counter("test.counter_c", 7);
        p.clear_designation("test.flag");
        p.set_designation("test.label", DesignationValue::Text("x".to_owned()));
    }
    draw(s, 5);
    s.issue_card_id().unwrap();
    s.issue_object_id().unwrap();
    s.issue_ability_instance_id().unwrap();
    s.issue_effect_id().unwrap();
    s.issue_timestamp().unwrap();
    s.issue_link_id().unwrap();
    s.set_priority_player(Some(pid(n))).unwrap();
    s.set_active_player(Some(pid(n))).unwrap();
    s.set_turn_number(s.turn().turn_number() + 4);
    let mut order: Vec<PlayerId> = s.turn_order().to_vec();
    order.reverse();
    if order.len() > 1 {
        s.set_turn_order(order).unwrap();
    }
}

// ---------------------------------------------------------------------------------------------
// Toy Transactional (requirement 8): a minimal state whose durable carry is observable.
// ---------------------------------------------------------------------------------------------

/// A toy state: `a` is a free tag, `log` an append-only byte log, `prio` the priority player,
/// `calls` the `(before.a, after.a)` pairs of every `carry_durable` call that produced it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Toy {
    pub a: i64,
    pub log: Vec<u8>,
    pub prio: Option<PlayerId>,
    pub calls: Vec<(i64, i64)>,
}

impl Toy {
    pub fn new(a: i64) -> Toy {
        Toy {
            a,
            log: Vec::new(),
            prio: Some(pid(1)),
            calls: Vec::new(),
        }
    }

    /// Deterministic canonical bytes of every field.
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(self.a.to_le_bytes());
        out.extend((self.log.len() as u64).to_le_bytes());
        out.extend(&self.log);
        match self.prio {
            None => out.push(0),
            Some(p) => {
                out.push(1);
                out.extend(p.get().to_le_bytes());
            }
        }
        out.extend((self.calls.len() as u64).to_le_bytes());
        for (b, a) in &self.calls {
            out.extend(b.to_le_bytes());
            out.extend(a.to_le_bytes());
        }
        out
    }
}

impl Transactional for Toy {
    fn state_hash(&self) -> Digest {
        sha256(&self.bytes())
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.prio
    }

    /// Appends `after.log` minus `before.log` to `restored.log` and records the call.
    fn carry_durable(mut restored: Self, before: &Self, after: &Self) -> Self {
        let suffix = after.log.get(before.log.len()..).unwrap_or(&[]);
        restored.log.extend_from_slice(suffix);
        restored.calls.push((before.a, after.a));
        restored
    }
}

// ---------------------------------------------------------------------------------------------
// Error-call helpers: a failed call must change nothing (brief semantics b).
// ---------------------------------------------------------------------------------------------

/// Which mutating call to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    Commit,
    CommitDurable,
    Rollback,
}

pub const ALL_CALLS: [Call; 3] = [Call::Commit, Call::CommitDurable, Call::Rollback];

/// Makes `call` on `id`, expects exactly `expected`, and asserts the state, its hash and the
/// depth are unchanged.
pub fn expect_err<S>(
    txns: &mut Transactions<S>,
    call: Call,
    id: fineprint_core::txn::TxnId,
    expected: &TxnError,
) where
    S: Transactional + PartialEq + Debug,
{
    let before = txns.state().clone();
    let hash = txns.state().state_hash();
    let depth = txns.depth();
    let got = match call {
        Call::Commit => txns.commit(id).map(|()| None),
        Call::CommitDurable => txns.commit_durable(id).map(|()| None),
        Call::Rollback => txns.rollback(id).map(Some),
    };
    assert_eq!(got, Err(expected.clone()), "{call:?} on {id:?}");
    assert!(!expected.to_string().is_empty());
    assert_eq!(txns.state(), &before, "{call:?} error changed the state");
    assert_eq!(
        txns.state().state_hash(),
        hash,
        "{call:?} error changed the hash"
    );
    assert_eq!(txns.depth(), depth, "{call:?} error changed the depth");
}
