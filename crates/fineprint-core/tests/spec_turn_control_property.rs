// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the turn scheduler (REQ-TURN-003; CR 500.7, 614.10, 614.10a, 614.10b,
//! 800.4k): random interleavings of `add_extra_turn`, `add_extra_turns_apnap`,
//! `add_turn_skip` and `next_turn` on 2-4 seats, with players leaving, against an
//! independent reference model of the brief. Seeded LCG only (R3); no wall clock (R7).
//! Black-box: written from the work-item brief only.

#[path = "spec_turn_control_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::PlayerId;
use fineprint_core::turn::control::{FollowUpId, PlannedTurn, SkippedTurn, TurnScheduler};
use support::*;

/// One step of a run, as observed.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Obs {
    Turn(PlannedTurn),
    Pending(Vec<PlayerId>, Vec<usize>),
}

/// The result of one run: the observations plus the accounting of extra turns.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RunOut {
    obs: Vec<Obs>,
}

/// Runs one random interleaving from `seed` and checks it against the model step by step.
fn run(seed: u64) -> RunOut {
    let mut rng = Lcg::new(seed);
    let n = 2 + rng.below(3) as usize;
    let seats: Vec<PlayerId> = (1..=n as u64).map(pid).collect();
    let start = seats[rng.below(n as u64) as usize];
    let mut s = TurnScheduler::new(start);
    let mut m = SchedModel::new(start);
    let mut left: Vec<PlayerId> = Vec::new();
    let mut next_follow = 1u64;
    let mut obs = Vec::new();
    // Extra-turn accounting per player: pushed, taken, skipped, dropped.
    let mut pushed: BTreeMap<PlayerId, usize> = BTreeMap::new();
    let mut accounted: BTreeMap<PlayerId, usize> = BTreeMap::new();
    let mut turns_taken = 0usize;
    let mut normal_seen: Vec<PlayerId> = Vec::new();
    for _ in 0..60 {
        let p = seats[rng.below(n as u64) as usize];
        match rng.below(12) {
            0..=2 => {
                s.add_extra_turn(p);
                m.add_extra_turn(p);
                *pushed.entry(p).or_default() += 1;
            }
            3 => {
                // An APNAP batch starting at a random seat.
                let k = rng.below(n as u64) as usize;
                let batch: Vec<PlayerId> = (0..n).map(|i| seats[(k + i) % n]).collect();
                s.add_extra_turns_apnap(&batch);
                for &q in &batch {
                    m.add_extra_turn(q);
                    *pushed.entry(q).or_default() += 1;
                }
            }
            4..=5 => {
                let f = if rng.below(2) == 0 {
                    None
                } else {
                    next_follow += 1;
                    Some(next_follow)
                };
                s.add_turn_skip(p, f.map(FollowUpId));
                m.add_turn_skip(p, f);
            }
            6 if turns_taken > 0 && left.len() + 1 < n && !left.contains(&p) => {
                // CR 800.4k: a player leaves (never the last one; never before the first turn).
                left.push(p);
            }
            _ => {
                let in_game = |q: PlayerId| !left.contains(&q);
                let got = s.next_turn(&seats, &in_game, false);
                let want = m.next_turn(&seats, &in_game);
                let (got, want) = match (got, want) {
                    (Some(g), Some(w)) => (g, w),
                    (g, w) => panic!("seed {seed}: {g:?} vs model {w:?}"),
                };
                assert_eq!(got.player, want.player, "seed {seed}: player");
                assert_eq!(got.extra, want.extra, "seed {seed}: extra");
                let skipped: Vec<(PlayerId, bool)> = got
                    .skipped
                    .iter()
                    .map(|SkippedTurn { player, extra }| (*player, *extra))
                    .collect();
                assert_eq!(skipped, want.skipped, "seed {seed}: skipped");
                let follow: Vec<u64> = got.follow_ups.iter().map(|f| f.0).collect();
                assert_eq!(follow, want.follow_ups, "seed {seed}: follow-ups");
                // The taken turn's player is in the game (CR 800.4k).
                assert!(in_game(got.player), "seed {seed}: departed player's turn");
                // The skipped turns came before the taken one.
                for (q, extra) in &skipped {
                    if *extra {
                        *accounted.entry(*q).or_default() += 1;
                    } else {
                        normal_seen.push(*q);
                    }
                }
                if got.extra {
                    *accounted.entry(got.player).or_default() += 1;
                } else {
                    normal_seen.push(got.player);
                }
                for q in &want.dropped {
                    *accounted.entry(*q).or_default() += 1;
                }
                drain(&mut s);
                turns_taken += 1;
                obs.push(Obs::Turn(got));
            }
        }
        // Pending state agrees with the model after every operation.
        let pending = s.pending_extra_turns();
        assert_eq!(pending, m.pending_extra_turns(), "seed {seed}: extra stack");
        let skips: Vec<usize> = seats.iter().map(|q| s.pending_turn_skips(*q)).collect();
        let want_skips: Vec<usize> = seats.iter().map(|q| m.pending_turn_skips(*q)).collect();
        assert_eq!(skips, want_skips, "seed {seed}: pending skips");
        // Every extra turn pushed is taken, skipped or dropped exactly once, or still pending.
        for q in &seats {
            let still = pending.iter().filter(|x| *x == q).count();
            assert_eq!(
                pushed.get(q).copied().unwrap_or(0),
                accounted.get(q).copied().unwrap_or(0) + still,
                "seed {seed}: extra turns of {q:?} not conserved"
            );
        }
        obs.push(Obs::Pending(pending, skips));
    }
    // The normal turns (taken or skipped) follow the seat order cyclically, passing over only
    // players who had left.
    for w in normal_seen.windows(2) {
        let i = seats.iter().position(|x| *x == w[0]).expect("seated");
        let j = seats.iter().position(|x| *x == w[1]).expect("seated");
        let gap = (j + n - i) % n;
        let gap = if gap == 0 { n } else { gap };
        for k in 1..gap {
            let between = seats[(i + k) % n];
            assert!(
                left.contains(&between),
                "seed {seed}: normal order jumped over {between:?} ({normal_seen:?})"
            );
        }
    }
    // The starting player's turn is the first normal turn (unless they left before it, after
    // some extra turns: CR 800.4k).
    if let Some(first) = normal_seen.first()
        && !left.contains(&start)
    {
        assert_eq!(*first, start, "seed {seed}: the starting player is first");
    }
    RunOut { obs }
}

/// REQ-TURN-003, CR 500.7, CR 614.10, CR 614.10a, CR 614.10b, CR 800.4k: over random
/// interleavings on 2-4 seats the scheduler matches the reference model; every pushed extra
/// turn is taken, skipped or dropped exactly once (never duplicated, never lost); pending
/// skips never underflow; the normal order never repeats a player out of order.
#[test]
fn scheduler_matches_model_over_random_interleavings() {
    for seed in 0..400u64 {
        run(seed);
    }
}

/// REQ-TURN-003, REQ-CORE-001: each run is deterministic: twice from the same seed gives
/// identical sequences.
#[test]
fn scheduler_runs_are_deterministic() {
    for seed in [0u64, 1, 7, 42, 99, 1234, 65_535] {
        assert_eq!(run(seed), run(seed), "seed {seed}");
    }
}

/// REQ-TURN-003: a cloned scheduler behaves identically to the original.
#[test]
fn cloned_scheduler_is_identical() {
    let seats = [P1, P2, P3];
    let mut a = TurnScheduler::new(P2);
    take(&mut a, &seats);
    a.add_extra_turn(P1);
    a.add_turn_skip(P3, Some(FollowUpId(5)));
    let mut b = a.clone();
    for _ in 0..6 {
        assert_eq!(take(&mut a, &seats), take(&mut b, &seats));
    }
}
