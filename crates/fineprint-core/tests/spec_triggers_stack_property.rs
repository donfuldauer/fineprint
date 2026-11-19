// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for trigger placement (REQ-EVENT-002, REQ-ARCH-002; CR 603.3b, 603.3d, 101.4):
//! random trigger sets, players, parts and ordering answers, hand-rolled with fixed seeds (R3,
//! R7). Black-box: written from the work-item brief only.

#[path = "spec_triggers_stack_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::ids::PlayerId;
use fineprint_core::choice::{
    ChoiceContext, ChoicePayload, ChoiceSession, ChoiceType, PendingChoiceRequest, Status,
};
use fineprint_core::triggers::{PendingTrigger, RemovedReason, place_triggers};
use support::*;

/// One random case: APNAP order, waiting triggers (creation order), the ids that cannot be put
/// on the stack, and the environment.
struct Case {
    apnap: Vec<PlayerId>,
    waiting: Vec<PendingTrigger>,
    dead: BTreeSet<String>,
    env: ScriptEnv,
}

fn gen_case(lcg: &mut Lcg) -> Case {
    let all = [P1, P2, P3, P4];
    let n_players = 2 + lcg.idx(3);
    let mut apnap: Vec<PlayerId> = all[..n_players].to_vec();
    lcg.shuffle(&mut apnap);
    let n = lcg.idx(10);
    let mut waiting = Vec::new();
    let mut dead = BTreeSet::new();
    let mut env = ScriptEnv::new();
    for i in 0..n {
        let id = format!("test.trigger_{i}");
        let controller = apnap[lcg.idx(n_players)];
        let source = 100 + i as u64;
        let mut t = if lcg.coin() {
            pt(source, controller, &id)
        } else {
            pt2(source, controller, &id)
        };
        match lcg.idx(4) {
            0 => {
                t = targeted(t, vec![slot(0, 1, 1)]);
                env = env.slot(&id, 0, vec![obj(500 + i as u64), obj(600 + i as u64)]);
            }
            1 => {
                t = targeted(t, vec![slot(0, 1, 1)]);
                dead.insert(id.clone());
            }
            2 => t = optional(t),
            _ => {}
        }
        waiting.push(t);
    }
    Case {
        apnap,
        waiting,
        dead,
        env,
    }
}

/// Answers ORDER with a random permutation and CHOOSE_TARGETS with a random legal target.
fn answer(r: &PendingChoiceRequest, lcg: &mut Lcg) -> fineprint_core::choice::ChoiceResponse {
    match &r.payload {
        ChoicePayload::OrderSimultaneousTriggers { triggers } => order(&lcg.perm(triggers.len())),
        ChoicePayload::ChooseTargets { slots, .. } => targets(
            slots
                .iter()
                .map(|sl| {
                    if sl.legal.is_empty() {
                        vec![]
                    } else {
                        vec![sl.legal[lcg.idx(sl.legal.len())]]
                    }
                })
                .collect(),
        ),
        other => panic!("unexpected request {other:?}"),
    }
}

/// REQ-EVENT-002, REQ-ARCH-002; CR 603.3b, 603.3d, 101.4: for random cases, `stacked` is part 1
/// then part 2; inside each part the players' groups follow APNAP order; inside each (player,
/// part) group the chosen order; ORDER is asked exactly for groups of two or more, in that
/// sequence; stacked and removed are disjoint and together are the input; no optional ask.
#[test]
fn req_event_002_property_apnap_parts_and_order() {
    for seed in 0..300u64 {
        let mut lcg = Lcg::new(seed);
        let case = gen_case(&mut lcg);
        let d = drive_with(&case.env, &case.apnap, &case.waiting, |r| {
            answer(r, &mut lcg)
        });
        let p = d.placement();

        // ORDER requests, with their answers.
        let orders: Vec<(&PendingChoiceRequest, Vec<u32>)> = d
            .requests
            .iter()
            .zip(&d.answers)
            .filter(|(r, _)| r.choice_type == ChoiceType::OrderSimultaneousTriggers)
            .map(|(r, a)| match a {
                fineprint_core::choice::ChoiceResponse::OrderTriggers { order } => {
                    (r, order.clone())
                }
                other => panic!("seed {seed}: ORDER answered with {other:?}"),
            })
            .collect();
        assert!(
            d.requests
                .iter()
                .all(|r| r.choice_type != ChoiceType::ChooseOptionalTrigger),
            "seed {seed}"
        );

        let mut expected: Vec<String> = Vec::new();
        let mut used = 0;
        for part2 in [false, true] {
            for &player in &case.apnap {
                let group: Vec<&PendingTrigger> = case
                    .waiting
                    .iter()
                    .filter(|t| t.controller == player && t.responds_to_trigger == part2)
                    .collect();
                if group.len() >= 2 {
                    let (r, o) = orders.get(used).unwrap_or_else(|| {
                        panic!("seed {seed}: missing ORDER for {player:?} part2={part2}")
                    });
                    used += 1;
                    assert_eq!(r.chooser, player, "seed {seed}");
                    let descs: Vec<String> = group.iter().map(|t| t.description()).collect();
                    assert_eq!(order_payload(r), descs, "seed {seed}");
                    expected.extend(o.iter().map(|&i| group[i as usize].ability_id.clone()));
                } else {
                    expected.extend(group.iter().map(|t| t.ability_id.clone()));
                }
            }
        }
        assert_eq!(used, orders.len(), "seed {seed}: extra ORDER requests");

        let want_stacked: Vec<String> = expected
            .iter()
            .filter(|id| !case.dead.contains(*id))
            .cloned()
            .collect();
        let want_removed: Vec<String> = expected
            .iter()
            .filter(|id| case.dead.contains(*id))
            .cloned()
            .collect();
        assert_eq!(stacked_ids(p), want_stacked, "seed {seed}");
        assert_eq!(removed_ids(p), want_removed, "seed {seed}");
        assert!(
            p.removed
                .iter()
                .all(|r| r.reason == RemovedReason::NoLegalTargets)
        );

        // Disjoint and complete, with the triggers carried unchanged.
        let mut out: Vec<PendingTrigger> = p.stacked.iter().map(|t| t.trigger.clone()).collect();
        out.extend(p.removed.iter().map(|r| r.trigger.clone()));
        let mut want = case.waiting.clone();
        out.sort_by(|a, b| a.ability_id.cmp(&b.ability_id));
        want.sort_by(|a, b| a.ability_id.cmp(&b.ability_id));
        assert_eq!(out, want, "seed {seed}");

        // Every stacked targeted trigger got one legal target in its single slot.
        for st in &p.stacked {
            if st.trigger.targets.is_empty() {
                assert!(st.targets.is_empty(), "seed {seed}");
            } else {
                assert_eq!(st.targets.len(), 1, "seed {seed}");
                assert_eq!(st.targets[0].len(), 1, "seed {seed}");
            }
        }
    }
}

/// REQ-ARCH-002, REQ-EVENT-002; CR 603.3b: placement is deterministic: the same answers give the
/// same requests and an equal placement, also via `ChoiceSession::replay`.
#[test]
fn req_arch_002_property_placement_deterministic() {
    for seed in 1000..1150u64 {
        let mut lcg = Lcg::new(seed);
        let case = gen_case(&mut lcg);
        let d = drive_with(&case.env, &case.apnap, &case.waiting, |r| {
            answer(r, &mut lcg)
        });
        let again = drive(&case.env, &case.apnap, &case.waiting, &d.answers);
        assert_eq!(again.requests, d.requests, "seed {seed}");
        assert_eq!(again.placement, d.placement, "seed {seed}");

        let procedure = |_s: &mut Blank, ctx: &mut ChoiceContext<'_>| {
            place_triggers(&case.env, ctx, &case.apnap, case.waiting.clone())
        };
        let replayed = ChoiceSession::replay(Blank, procedure, &d.answers).expect("replays");
        match replayed.status() {
            Status::Done { output, .. } => {
                assert_eq!(Some(output), d.placement.as_ref(), "seed {seed}");
            }
            Status::Pending(r) => panic!("seed {seed}: still pending {r:?}"),
        }
        let defaults_a = drive_default(&case.env, &case.apnap, &case.waiting);
        let defaults_b = drive_default(&case.env, &case.apnap, &case.waiting);
        assert_eq!(defaults_a.placement, defaults_b.placement, "seed {seed}");
        assert_eq!(defaults_a.requests, defaults_b.requests, "seed {seed}");
    }
}
