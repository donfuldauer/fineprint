// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests of the turn sequencer's cleanup step (REQ-TURN-002; CR 402.2, 514.1,
//! 514.3a). Black-box, from the work-item brief only. Deterministic enumeration of small inputs
//! with a fixed-seed generator (R3); synthetic ids only (R5).

#[path = "spec_turn_steps_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_core::choice::ChoiceType;
use fineprint_core::events::Step;
use fineprint_core::turn::{ExpiryPoint, Segment, TurnBasedRegistry, TurnHost};

use support::*;

/// REQ-TURN-002; CR 402.2, 514.1: for every hand size 0..=12, maximum 0..=9 and a valid
/// answer, the discarded set is a subset of the hand of size max(0, len - max), discarded
/// ascending, and the remaining hand is at most the maximum.
#[test]
fn discard_to_hand_size_for_all_small_hands() {
    let reg = TurnBasedRegistry::standard();
    let mut rng = Lcg::new(514);
    for len in 0u64..=12 {
        for max in 0u64..=9 {
            let hand: Vec<_> = (1..=len).map(|i| oid(10 * i)).collect();
            let k = len.saturating_sub(max);
            // A random valid answer: k distinct cards of the hand, in a random order.
            let mut pool = hand.clone();
            let mut answer = Vec::new();
            for _ in 0..k {
                let i = rng.below(pool.len() as u64) as usize;
                answer.push(pool.remove(i));
            }
            let fake = TurnFake::new(2)
                .at(2, Some(P1))
                .with_hand(P1, hand.clone())
                .with_max(P1, Some(max));
            let run = drive(
                World::new(fake),
                Proc::Segment(Segment::Step(Step::Cleanup)),
                &reg,
                Answers::none().discard(answer.clone()),
            );
            let ctx = format!("len {len} max {max}");
            let discards: Vec<Vec<_>> = run
                .calls()
                .into_iter()
                .filter_map(|c| match c {
                    TC::Discard(p, cards) => {
                        assert_eq!(p, P1, "{ctx}");
                        Some(cards)
                    }
                    _ => None,
                })
                .collect();
            if k == 0 {
                assert!(run.requests.is_empty(), "{ctx}");
                assert!(discards.is_empty(), "{ctx}");
                assert_eq!(
                    run.calls(),
                    step(Step::Cleanup, quiet_cleanup_body(&[P1, P2]))
                );
            } else {
                assert_eq!(
                    run.types(),
                    vec![ChoiceType::SelectObjectsFromZone],
                    "{ctx}"
                );
                assert_eq!(run.requests[0].min, k, "{ctx}");
                assert_eq!(run.requests[0].max, k, "{ctx}");
                assert_eq!(discards.len(), 1, "{ctx}");
                let cards = &discards[0];
                assert_eq!(cards.len() as u64, k, "{ctx}");
                let set: BTreeSet<_> = cards.iter().copied().collect();
                let in_hand: BTreeSet<_> = hand.iter().copied().collect();
                assert!(set.is_subset(&in_hand), "{ctx}");
                let mut sorted = answer.clone();
                sorted.sort();
                assert_eq!(cards, &sorted, "{ctx}: the chosen cards, ascending");
            }
            let remaining = run.last.host.hand(P1);
            assert!(remaining.len() as u64 <= max, "{ctx}");
            assert_eq!(remaining.len() as u64, len.min(max), "{ctx}");
            assert_eq!(run.segment().cleanup_steps, 1, "{ctx}");
            assert!(run.last.host.anomalies.is_empty(), "{ctx}");
        }
    }
}

/// REQ-TURN-002; CR 514.3a, 500.4, 500.5: for 0..=5 rounds of work (random kinds), the log has
/// rounds + 1 cleanup steps with the same boundary pattern each time, and the last one is
/// quiet.
#[test]
fn cleanup_rounds_repeat_with_the_same_boundaries() {
    let reg = TurnBasedRegistry::standard();
    let mut rng = Lcg::new(5143);
    for trial in 0..3 {
        for rounds in 0usize..=5 {
            let work: Vec<Option<Work>> = (0..rounds)
                .map(|_| {
                    Some(match rng.below(3) {
                        0 => Work::Trigger,
                        1 => Work::Sba,
                        _ => Work::Stack,
                    })
                })
                .collect();
            let fake = TurnFake::new(2)
                .at(2, Some(P1))
                .with_cleanup_work(work.clone());
            let run = drive(
                World::new(fake),
                Proc::Segment(Segment::Step(Step::Cleanup)),
                &reg,
                Answers::none(),
            );
            let ctx = format!("trial {trial} {work:?}");
            let report = run.segment();
            assert_eq!(report.cleanup_steps as usize, rounds + 1, "{ctx}");
            assert_eq!(report.heartbeat_ran, rounds > 0, "{ctx}");
            let log = run.calls();
            let begin = TC::BeginStep(Step::Cleanup);
            assert_eq!(count(&log, &begin), rounds + 1, "{ctx}");
            assert_eq!(log[0], begin, "{ctx}");
            for n in 0..=rounds {
                let chunk = between(&log, &begin, n, &begin);
                let len = chunk.len();
                assert_eq!(
                    chunk[..3],
                    [
                        begin.clone(),
                        TC::Expire(ExpiryPoint::BeginStep(Step::Cleanup)),
                        TC::EotCleanup
                    ],
                    "{ctx} step {n}"
                );
                assert_eq!(
                    chunk[len - 2..],
                    [
                        TC::Expire(ExpiryPoint::EndStep(Step::Cleanup)),
                        TC::EmptyMana
                    ],
                    "{ctx} step {n}"
                );
                // No other boundary call inside a cleanup step.
                assert_eq!(
                    chunk
                        .iter()
                        .filter(|c| matches!(c, TC::Expire(_) | TC::EmptyMana | TC::BeginStep(_)))
                        .count(),
                    4,
                    "{ctx} step {n}"
                );
                let prio = chunk.iter().any(|c| matches!(c, TC::Prio(_)));
                assert_eq!(prio, n < rounds, "{ctx} step {n}");
            }
            assert!(
                log.ends_with(&step(Step::Cleanup, quiet_cleanup_body(&[P1, P2]))),
                "{ctx}"
            );
            assert!(run.last.host.anomalies.is_empty(), "{ctx}");
            assert_eq!(run.last.host.depth, 0, "{ctx}");
        }
    }
}
