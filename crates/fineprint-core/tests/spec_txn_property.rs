// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property-style tests for `fineprint_core::txn` over random operation sequences (mutate,
//! checkpoint, commit, commit_durable, rollback of the innermost, deliberately wrong ids), for
//! GameState and for a toy `Transactional` (REQ-CORE-005, CR 734.1, CR 734.2; determinism
//! REQ-CORE-001, REQ-CORE-006; R3, R12). Plain loops driven by `SplitMix64` from 16 literal
//! seeds; no wall clock. A reference model (a stack of cloned checkpoint states with their
//! durable records) predicts every state. For the toy state the expected log after every
//! rollback is also computed from independently tracked appended bytes, not from snapshot
//! pairs. Black-box: written from the work-item brief only.

#[path = "spec_txn_support.rs"]
mod support;

use std::fmt::Debug;

use fineprint_base::hash::Digest;
use fineprint_base::prng::SplitMix64;
use fineprint_core::state::{DesignationValue, GameState};
use fineprint_core::txn::{RollbackReport, Transactional, Transactions, TxnError, TxnId};
use support::*;

const SEEDS: [u64; 16] = [
    0,
    1,
    2,
    3,
    7,
    42,
    99,
    1234,
    0xDEAD_BEEF,
    0xFEED_F00D,
    0x0026_0026,
    0x1357_9BDF,
    0x2468_ACE0,
    0x7FFF_FFFF_FFFF_FFFF,
    0x8000_0000_0000_0000,
    u64::MAX,
];
const STEPS: usize = 400;
const MAX_DEPTH: usize = 6;

/// A state the harness can build and mutate deterministically from one random word.
trait Harness: Transactional + PartialEq + Debug {
    fn initial(seed: u64) -> Self;
    fn mutate(&mut self, word: u64);
    /// The append-only effect log, for states that have one (the toy state). Used to track the
    /// expected durable effects independently of `carry_durable`.
    fn effect_log(&self) -> Option<&[u8]> {
        None
    }
}

impl Harness for GameState {
    fn initial(seed: u64) -> Self {
        let players = 2 + (seed % 3) as usize;
        let mut s = game(players);
        draw(&mut s, (seed % 5) as usize);
        s
    }

    fn mutate(&mut self, word: u64) {
        let n = self.player_count() as u64;
        let who = pid(1 + (word >> 8) % n);
        let arg = word >> 16;
        match word % 10 {
            0 => self
                .player_mut(who)
                .unwrap()
                .set_life((arg % 40) as i64 - 5),
            1 => {
                let kind =
                    ["test.counter_a", "test.counter_b", "test.counter_c"][(arg % 3) as usize];
                self.player_mut(who)
                    .unwrap()
                    .set_counter(kind, ((arg >> 4) % 4) as i64);
            }
            2 => {
                let mut p = self.player_mut(who).unwrap();
                match arg % 3 {
                    0 => {
                        p.clear_designation("test.flag");
                    }
                    1 => p.set_designation("test.flag", DesignationValue::Flag),
                    _ => p.set_designation(
                        "test.level",
                        DesignationValue::Int((arg >> 4) as i64 % 9),
                    ),
                }
            }
            3 => draw(self, 1 + (arg % 3) as usize),
            4 => issue_objects(self, 1 + (arg % 2) as usize),
            5 => {
                if arg.is_multiple_of(2) {
                    self.issue_timestamp().unwrap();
                } else {
                    self.issue_card_id().unwrap();
                }
            }
            6 => {
                let p = if arg.is_multiple_of(4) {
                    None
                } else {
                    Some(who)
                };
                self.set_priority_player(p).unwrap();
            }
            7 => self.set_active_player(Some(who)).unwrap(),
            8 => self.set_turn_number(arg % 1000),
            _ => {
                let mut order = self.turn_order().to_vec();
                order.rotate_left(1);
                self.set_turn_order(order).unwrap();
            }
        }
    }
}

impl Harness for Toy {
    fn initial(seed: u64) -> Self {
        Toy::new((seed % 1000) as i64)
    }

    fn mutate(&mut self, word: u64) {
        match word % 3 {
            0 => self.log.push((word >> 8) as u8),
            1 => self.a = ((word >> 8) % 10_000) as i64,
            _ => {
                self.prio = match (word >> 8) % 3 {
                    0 => None,
                    k => Some(pid(k)),
                }
            }
        }
    }

    fn effect_log(&self) -> Option<&[u8]> {
        Some(&self.log)
    }
}

/// One open frame of the reference model.
struct Frame<S> {
    id: TxnId,
    checkpoint: S,
    checkpoint_hash: Digest,
    records: Vec<(S, S)>,
    /// Independent effect tracking (toy state): the bytes appended to the log during this
    /// frame's window (including what closed children left behind), in order.
    window: Vec<u8>,
    /// Independent effect tracking (toy state): the bytes that survive this frame's rollback,
    /// each durable effect once, in commit order.
    durable_bytes: Vec<u8>,
}

/// Model of a plain `commit` of `frame`: its records and tracked bytes move to the parent.
fn model_commit<S>(frames: &mut [Frame<S>], frame: Frame<S>) {
    if let Some(parent) = frames.last_mut() {
        parent.records.extend(frame.records);
        parent.window.extend(frame.window);
        parent.durable_bytes.extend(frame.durable_bytes);
    }
}

/// Model of `commit_durable` of `frame` (semantics 2): with a parent, the parent receives one
/// record covering the frame's whole window; the frame's inherited records are dropped because
/// that window already contains them. Its whole window becomes durable on the parent.
fn model_commit_durable<S: Clone>(frames: &mut [Frame<S>], frame: Frame<S>, live: &S) {
    if let Some(parent) = frames.last_mut() {
        parent.records.push((frame.checkpoint, live.clone()));
        parent.durable_bytes.extend_from_slice(&frame.window);
        parent.window.extend(frame.window);
    }
}

/// Model of `rollback` of `frame`: the live state is the checkpoint with the records carried
/// in commit order; records and surviving bytes move to the parent. Returns the new live state
/// and the expected log (checkpoint log plus the independently tracked durable bytes), if the
/// state has a log.
fn model_rollback<S: Harness>(frames: &mut [Frame<S>], frame: Frame<S>) -> (S, Option<Vec<u8>>) {
    let expected_log = frame.checkpoint.effect_log().map(|log| {
        let mut out = log.to_vec();
        out.extend_from_slice(&frame.durable_bytes);
        out
    });
    let mut live = frame.checkpoint;
    for (before, after) in &frame.records {
        live = S::carry_durable(live, before, after);
    }
    if let Some(parent) = frames.last_mut() {
        parent.records.extend(frame.records);
        parent.window.extend_from_slice(&frame.durable_bytes);
        parent.durable_bytes.extend(frame.durable_bytes);
    }
    (live, expected_log)
}

/// Makes `call` on `id` and checks the result is exactly `expected`, with nothing changed.
fn wrong<S: Harness>(
    txns: &mut Transactions<S>,
    rng: &mut SplitMix64,
    id: TxnId,
    expected: TxnError,
) {
    let call = ALL_CALLS[(rng.next_u64() % 3) as usize];
    expect_err(txns, call, id, &expected);
}

/// Runs one random sequence and returns the trace of state hashes (one per step plus the final
/// one). Every step is checked against the reference model.
fn run<S: Harness>(seed: u64, foreign: TxnId) -> (Vec<Digest>, usize) {
    let mut rng = SplitMix64::new(seed);
    let mut live = S::initial(seed);
    let mut txns = Transactions::new(live.clone());
    let mut frames: Vec<Frame<S>> = Vec::new();
    let mut closed: Vec<TxnId> = Vec::new();
    let mut last_id: Option<TxnId> = None;
    let mut trace = Vec::new();
    let mut carried_rollbacks = 0usize;

    for step in 0..STEPS {
        let roll = rng.next_u64() % 100;
        let depth = frames.len();
        if roll < 35 || (roll < 55 && depth >= MAX_DEPTH) {
            let word = rng.next_u64();
            let old_len = live.effect_log().map_or(0, <[u8]>::len);
            live.mutate(word);
            txns.state_mut().mutate(word);
            if let (Some(log), Some(frame)) = (live.effect_log(), frames.last_mut()) {
                frame.window.extend_from_slice(&log[old_len..]);
            }
        } else if roll < 55 {
            let hash = txns.state().state_hash();
            let id = txns.checkpoint();
            if let Some(prev) = last_id {
                assert!(id > prev, "seed {seed} step {step}: ids increase");
            }
            assert!(!closed.contains(&id), "seed {seed} step {step}: id reused");
            last_id = Some(id);
            frames.push(Frame {
                id,
                checkpoint: live.clone(),
                checkpoint_hash: hash,
                records: Vec::new(),
                window: Vec::new(),
                durable_bytes: Vec::new(),
            });
        } else if roll < 83 && depth > 0 {
            let frame = frames.pop().unwrap();
            closed.push(frame.id);
            if roll < 65 {
                txns.commit(frame.id).unwrap();
                model_commit(&mut frames, frame);
            } else if roll < 73 {
                txns.commit_durable(frame.id).unwrap();
                model_commit_durable(&mut frames, frame, &live);
            } else {
                let report = txns.rollback(frame.id).unwrap();
                let no_records = frame.records.is_empty();
                if !no_records {
                    carried_rollbacks += 1;
                }
                let checkpoint_prio = frame.checkpoint.priority_player();
                let checkpoint_hash = frame.checkpoint_hash;
                let (next, expected_log) = model_rollback(&mut frames, frame);
                live = next;
                // Independent oracle: each durable effect exactly once, in commit order. A
                // duplicated durable effect (for example `[X, X]` where only `[X]` was
                // appended durably) fails here.
                if let Some(expected_log) = expected_log {
                    assert_eq!(
                        txns.state().effect_log(),
                        Some(expected_log.as_slice()),
                        "seed {seed} step {step}: durable effects once, in commit order"
                    );
                }
                if no_records {
                    assert_eq!(
                        report.state_hash, checkpoint_hash,
                        "seed {seed} step {step}: exact hash restore"
                    );
                }
                // CR 734.2: the checkpoint's priority player keeps priority.
                assert_eq!(report.priority_player, checkpoint_prio);
                assert_eq!(
                    report,
                    RollbackReport {
                        priority_player: txns.state().priority_player(),
                        state_hash: txns.state().state_hash(),
                    }
                );
            }
        } else {
            // A deliberately wrong id.
            let kind = rng.next_u64() % 3;
            if kind == 0 && !closed.is_empty() {
                let stale = closed[(rng.next_u64() % closed.len() as u64) as usize];
                wrong(&mut txns, &mut rng, stale, TxnError::UnknownTxn(stale));
            } else if kind == 1 && depth >= 2 {
                let requested = frames[(rng.next_u64() % (depth as u64 - 1)) as usize].id;
                let innermost = frames[depth - 1].id;
                wrong(
                    &mut txns,
                    &mut rng,
                    requested,
                    TxnError::NotInnermost {
                        requested,
                        innermost,
                    },
                );
            } else {
                wrong(&mut txns, &mut rng, foreign, TxnError::UnknownTxn(foreign));
            }
        }
        assert_eq!(
            txns.state(),
            &live,
            "seed {seed} step {step}: model mismatch"
        );
        assert_eq!(txns.depth(), frames.len(), "seed {seed} step {step}: depth");
        trace.push(txns.state().state_hash());
    }

    // Finish: half the seeds close every frame, the others try into_state while open.
    if seed.is_multiple_of(2) {
        while let Some(frame) = frames.pop() {
            if rng.next_u64().is_multiple_of(2) {
                txns.commit(frame.id).unwrap();
                model_commit(&mut frames, frame);
            } else {
                txns.rollback(frame.id).unwrap();
                let (next, expected_log) = model_rollback(&mut frames, frame);
                live = next;
                if let Some(expected_log) = expected_log {
                    assert_eq!(
                        txns.state().effect_log(),
                        Some(expected_log.as_slice()),
                        "seed {seed}: unwind durable effects once"
                    );
                }
            }
            assert_eq!(txns.state(), &live, "seed {seed}: unwind");
        }
    }
    let open = frames.len();
    match txns.into_state() {
        Ok(s) => {
            assert_eq!(open, 0, "seed {seed}");
            assert_eq!(s, live);
            trace.push(s.state_hash());
        }
        Err(e) => {
            assert_eq!(e, TxnError::OpenTransactions { open }, "seed {seed}");
            assert!(open > 0);
            trace.push(live.state_hash());
        }
    }
    (trace, carried_rollbacks)
}

/// An id never issued by any run (each run issues at most `STEPS` ids).
fn far_foreign_id() -> TxnId {
    let mut other = Transactions::new(Toy::new(0));
    let mut last = None;
    for _ in 0..(STEPS * 4) {
        let id = other.checkpoint();
        other.rollback(id).unwrap();
        last = Some(id);
    }
    last.unwrap()
}

/// REQ-CORE-005, REQ-CORE-001, REQ-CORE-006, CR 734.1, CR 734.2: random operation sequences
/// over GameState for 16 seeds: no panic, failed calls change nothing, rollbacks with no durable
/// record beneath restore the exact checkpoint hash, every state matches the reference model,
/// and a replay with the same seed gives the identical hash trace.
#[test]
fn property_gamestate_random_sequences() {
    let foreign = far_foreign_id();
    for seed in SEEDS {
        let a = run::<GameState>(seed, foreign);
        let b = run::<GameState>(seed, foreign);
        assert_eq!(a, b, "seed {seed}: replay must be identical");
    }
}

/// REQ-CORE-005, REQ-CORE-001, REQ-CORE-006, CR 734.1, CR 734.2: the same over the toy state,
/// whose durable carry is order-sensitive, so the model checks commit order and propagation.
#[test]
fn property_toy_random_sequences() {
    let foreign = far_foreign_id();
    for seed in SEEDS {
        let a = run::<Toy>(seed, foreign);
        let b = run::<Toy>(seed, foreign);
        assert_eq!(a, b, "seed {seed}: replay must be identical");
    }
}

/// REQ-CORE-005: the generator actually rolls back frames with durable records beneath them
/// (guards against a vacuous property run), for both state types.
#[test]
fn property_generator_reaches_durable_rollbacks() {
    let foreign = far_foreign_id();
    let mut toy = 0usize;
    let mut gs = 0usize;
    for seed in SEEDS {
        toy += run::<Toy>(seed, foreign).1;
        gs += run::<GameState>(seed, foreign).1;
    }
    assert!(toy >= SEEDS.len(), "toy carried rollbacks: {toy}");
    assert!(gs >= SEEDS.len(), "game-state carried rollbacks: {gs}");
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 5): root R, middle M, leaf L; only L appends X and
/// commits durably, then M commits durably, then R is rolled back. M's single record covers L,
/// so the log is exactly `[X]` from exactly one `carry_durable` call. An oracle that forwarded
/// L's record and also added M's would expect `[X, X]`; the model above does not.
#[test]
fn nested_durable_commits_root_rollback_leaves_one_effect() {
    const X: u8 = 0x58;
    let mut txns = Transactions::new(Toy::new(0));
    let r = txns.checkpoint();
    let m = txns.checkpoint();
    let l = txns.checkpoint();
    txns.state_mut().log.push(X);
    txns.commit_durable(l).unwrap();
    txns.commit_durable(m).unwrap();
    txns.rollback(r).unwrap();
    assert_eq!(txns.state().log, vec![X]);
    assert_eq!(txns.state().calls.len(), 1);
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 5): as above, but M appends Y before L and Z after
/// L. M's record covers its whole window, so after R's rollback each of Y, X, Z appears exactly
/// once, in order, over the checkpoint log, from exactly one `carry_durable` call.
#[test]
fn nested_durable_commits_cover_whole_middle_window_once() {
    const X: u8 = 0x58;
    const Y: u8 = 0x59;
    const Z: u8 = 0x5A;
    let mut txns = Transactions::new(Toy::new(0));
    txns.state_mut().log.push(1);
    let r = txns.checkpoint();
    txns.state_mut().log.push(2); // R's own change: reverted
    let m = txns.checkpoint();
    txns.state_mut().log.push(Y);
    let l = txns.checkpoint();
    txns.state_mut().log.push(X);
    txns.commit_durable(l).unwrap();
    txns.state_mut().log.push(Z);
    txns.commit_durable(m).unwrap();
    txns.state_mut().log.push(3); // R's own change: reverted
    txns.rollback(r).unwrap();
    assert_eq!(txns.state().log, vec![1, Y, X, Z]);
    assert_eq!(txns.state().calls.len(), 1);
}
