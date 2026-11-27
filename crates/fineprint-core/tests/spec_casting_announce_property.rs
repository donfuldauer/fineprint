// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the casting announce procedure (REQ-CAST-001, REQ-CORE-005,
//! REQ-NET-003, REQ-DSL-013, REQ-TEST-001; CR 601.2, 601.2c, 601.2d, 734.1, 734.2, 101.2,
//! 115.3, 115.5, 702.11e; R3, R12).
//!
//! Bounded random inputs from `Pcg64Dxsm::from_u64_seed` over fixed seeds (no wall clock, R3,
//! R7). Black-box: written from the work-item brief only. Synthetic data only (R5).

#[path = "spec_casting_announce_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::ids::PlayerId;
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::casting::*;
use fineprint_core::choice::{
    ChoicePayload, ChoiceResponse, ChoiceTarget, PendingChoiceRequest, Status,
};
use fineprint_core::txn::Transactional;
use fineprint_dsl::ability::RuleKind;
use serde_json::{Map, Value, json};
use support::*;

const CASES: u64 = 300;

fn below(rng: &mut Pcg64Dxsm, bound: u64) -> u64 {
    rng.next_bounded(bound).expect("bound > 0")
}

fn between(rng: &mut Pcg64Dxsm, lo: u64, hi: u64) -> u64 {
    lo + below(rng, hi - lo + 1)
}

fn coin(rng: &mut Pcg64Dxsm) -> bool {
    below(rng, 2) == 1
}

fn pick<'a, T>(rng: &mut Pcg64Dxsm, items: &'a [T]) -> &'a T {
    rng.choose(items).expect("non-empty")
}

/// The candidate universe: both players, objects 10..=15 and the first stack id (CR 115.5).
fn universe() -> Vec<ChoiceTarget> {
    let mut v = vec![tp(1), tp(2)];
    v.extend((10..=15).map(to));
    v.push(to(FIRST_STACK_ID));
    v
}

fn random_candidates(rng: &mut Pcg64Dxsm) -> Vec<ChoiceTarget> {
    let mut v: Vec<ChoiceTarget> = universe().into_iter().filter(|_| coin(rng)).collect();
    v.sort();
    v
}

fn random_slot(rng: &mut Pcg64Dxsm) -> TargetSlotSpec {
    let min = below(rng, 3);
    let max = between(rng, min.max(1), 3);
    slot(min, max, &random_candidates(rng))
}

fn random_slots(rng: &mut Pcg64Dxsm, n: u64) -> Vec<TargetSlotSpec> {
    (0..below(rng, n + 1)).map(|_| random_slot(rng)).collect()
}

/// A random well-formed targeting entry from the brief's vocabulary.
fn random_targeting_params(rng: &mut Pcg64Dxsm) -> Value {
    let mut m = Map::new();
    let keyword = *pick(rng, &["HEXPROOF", "SHROUD", "PROTECTION", "OTHER"]);
    m.insert("keyword".into(), json!(keyword));
    if keyword == "HEXPROOF" && below(rng, 4) == 0 {
        m.insert("effect".into(), json!("IGNORE_HEXPROOF"));
    } else if coin(rng) {
        m.insert("effect".into(), json!("PROHIBIT"));
    }
    match *pick(rng, &universe()) {
        ChoiceTarget::Player(p) => m.insert("target_player".into(), json!(p.get())),
        ChoiceTarget::Object(o) => m.insert("target_object".into(), json!(o.get())),
    };
    if coin(rng) {
        m.insert("sources".into(), json!(*pick(rng, &["ANY", "OPPONENTS"])));
    }
    if coin(rng) {
        m.insert("quality".into(), json!(*pick(rng, &[QUALITY_A, QUALITY_B])));
    }
    if below(rng, 4) == 0 {
        m.insert("source_player".into(), json!(between(rng, 1, 2)));
    }
    Value::Object(m)
}

/// A random well-formed prohibition (the fake reads `test.*`).
fn random_prohibition_params(rng: &mut Pcg64Dxsm) -> Value {
    let mut m = Map::new();
    m.insert("action".into(), json!(*pick(rng, &["CAST", "ACTIVATE"])));
    match below(rng, 4) {
        0 => {}
        1 => {
            m.insert("test.could_lapse".into(), json!(true));
            m.insert("test.applies".into(), json!(coin(rng)));
        }
        2 => {
            m.insert("test.parity".into(), json!(*pick(rng, &["even", "odd"])));
        }
        _ => {
            m.insert("test.could_lapse".into(), json!(true));
        }
    }
    if below(rng, 4) == 0 {
        m.insert(
            "object".into(),
            json!(*pick(rng, &[SPELL_OBJECT, ABILITY_SOURCE, 99])),
        );
    }
    Value::Object(m)
}

fn random_players(rng: &mut Pcg64Dxsm) -> Vec<PlayerId> {
    match below(rng, 3) {
        0 => vec![P1],
        1 => vec![P2],
        _ => vec![P1, P2],
    }
}

fn random_rules(rng: &mut Pcg64Dxsm, first_id: u64, n: u64, prohibitions: bool) -> Vec<R> {
    let count = below(rng, n + 1);
    (0..count)
        .map(|i| {
            let id = first_id + i;
            let ts = below(rng, 20);
            let players = random_players(rng);
            if prohibitions && below(rng, 4) == 0 {
                prohibition(id, random_prohibition_params(rng), &players, ts)
            } else {
                r(
                    id,
                    RuleKind::TargetingRule,
                    random_targeting_params(rng),
                    &players,
                    ts,
                )
            }
        })
        .collect()
}

fn random_costs(rng: &mut Pcg64Dxsm) -> CostSpec {
    let mut c = no_costs();
    if below(rng, 3) == 0 {
        c.alternative_costs = (0..between(rng, 1, 2))
            .map(|i| format!("test.alt_{i}"))
            .collect();
    }
    if below(rng, 3) == 0 {
        c.additional_costs = (0..between(rng, 1, 2))
            .map(|i| format!("test.add_{i}"))
            .collect();
    }
    if coin(rng) {
        let lo = below(rng, 3);
        c.x_range = Some((lo, lo + below(rng, 4)));
    }
    if below(rng, 4) == 0 {
        c.symbol_slots = (0..between(rng, 1, 2))
            .map(|_| vec!["test.sym_a".to_owned(), "test.sym_b".to_owned()])
            .collect();
    }
    if below(rng, 5) == 0 {
        c.harmonize_candidates = vec![oid(30), oid(31)];
    }
    c
}

/// A random configuration of the fake (its rules built from the returned entries) and a random
/// request.
fn random_case(rng: &mut Pcg64Dxsm) -> (Fake, CastRequest, Vec<R>) {
    let caster = if coin(rng) { P1 } else { P2 };
    let req = if coin(rng) {
        spell(caster)
    } else {
        ability(caster)
    };
    let mut f = Fake::new();
    f.priority = *pick(rng, &[Some(P1), Some(P2), None]);
    f.marker = below(rng, 1000);
    if coin(rng) {
        f.stack = vec![oid(90)];
    }
    for n in 10..=15 {
        if coin(rng) {
            f.controllers.insert(n, P1);
        }
    }
    if below(rng, 3) == 0 {
        let n = between(rng, 1, 3) as usize;
        let min = between(rng, 1, n as u64) as u32;
        let max = between(rng, u64::from(min), n as u64 + 1) as u32;
        f.mode_spec = Some(modes_spec(n, min, max, coin(rng)));
        f.mode_slots = (0..n).map(|_| random_slots(rng, 1)).collect();
    }
    f.cost_spec = random_costs(rng);
    f.base_slots = random_slots(rng, 2);
    if !f.cost_spec.additional_costs.is_empty() && coin(rng) {
        f.additional_slots = random_slots(rng, 1);
    }
    if below(rng, 3) == 0 {
        f.division = Some(below(rng, 6));
    }
    for q in [QUALITY_A, QUALITY_B] {
        if coin(rng) {
            f.qualities.insert(q.to_owned());
        }
    }
    f.combination_legal = below(rng, 6) != 0;
    let items = random_rules(rng, 1, 6, true);
    (f.with_rules(&items), req, items)
}

/// A random answer to `p` built to satisfy the request's pinned semantics.
fn valid_answer(rng: &mut Pcg64Dxsm, p: &PendingChoiceRequest) -> ChoiceResponse {
    match &p.payload {
        ChoicePayload::ChooseModes {
            labels,
            repeats_allowed,
            ..
        } => {
            let n = labels.len() as u64;
            let cap = if *repeats_allowed {
                p.max
            } else {
                p.max.min(n)
            };
            let k = between(rng, p.min, cap.max(p.min));
            let modes: Vec<u32> = if *repeats_allowed {
                (0..k).map(|_| below(rng, n) as u32).collect()
            } else {
                let mut all: Vec<u32> = (0..n as u32).collect();
                rng.shuffle(&mut all);
                all.truncate(k as usize);
                all
            };
            select_modes(&modes)
        }
        ChoicePayload::ChooseCostsAndX {
            alternative_costs,
            additional_costs,
            x_range,
            symbol_slots,
            harmonize_candidates,
        } => {
            let alternative = if !alternative_costs.is_empty() && coin(rng) {
                Some(below(rng, alternative_costs.len() as u64) as u32)
            } else {
                None
            };
            let additional = (0..additional_costs.len() as u32)
                .filter(|_| coin(rng))
                .collect();
            let x = x_range.map(|(lo, hi)| between(rng, lo, hi));
            let symbol_options = symbol_slots
                .iter()
                .map(|s| below(rng, s.len() as u64) as u32)
                .collect();
            let harmonize = if !harmonize_candidates.is_empty() && coin(rng) {
                Some(*pick(rng, harmonize_candidates))
            } else {
                None
            };
            configure(&CostChoice {
                alternative,
                additional,
                x,
                symbol_options,
                harmonize,
            })
        }
        ChoicePayload::ChooseTargets { slots, .. } => {
            let targets = slots
                .iter()
                .map(|s| {
                    let cap = s.max.min(s.legal.len() as u64);
                    let k = between(rng, s.min, cap.max(s.min));
                    let mut l = s.legal.clone();
                    rng.shuffle(&mut l);
                    l.truncate(k as usize);
                    l
                })
                .collect();
            ChoiceResponse::SelectTargets { targets }
        }
        ChoicePayload::DivideValue { total, recipients } => {
            let floor: u64 = recipients.iter().map(|(_, m)| m).sum();
            let mut shares: Vec<u64> = recipients.iter().map(|(_, m)| *m).collect();
            let mut rest = total.saturating_sub(floor);
            while rest > 0 {
                let i = below(rng, shares.len() as u64) as usize;
                shares[i] += 1;
                rest -= 1;
            }
            submit_division(
                &recipients
                    .iter()
                    .zip(shares)
                    .map(|((t, _), s)| (*t, s))
                    .collect::<Vec<_>>(),
            )
        }
        other => panic!("unexpected request payload {other:?}"),
    }
}

/// A random (probably invalid) answer of a random announce-response shape.
fn junk_answer(rng: &mut Pcg64Dxsm) -> ChoiceResponse {
    let u = universe();
    match below(rng, 5) {
        0 => select_modes(
            &(0..below(rng, 4))
                .map(|_| below(rng, 5) as u32)
                .collect::<Vec<_>>(),
        ),
        1 => with_x(below(rng, 20)),
        2 => ChoiceResponse::SelectTargets {
            targets: (0..below(rng, 4))
                .map(|_| (0..below(rng, 4)).map(|_| *pick(rng, &u)).collect())
                .collect(),
        },
        3 => submit_division(
            &(0..below(rng, 4))
                .map(|_| (*pick(rng, &u), below(rng, 5)))
                .collect::<Vec<_>>(),
        ),
        _ => ChoiceResponse::PassPriority,
    }
}

/// One random run: drives the session with valid answers, interleaving junk answers that must
/// either be rejected without changing anything or be valid.
struct Driven {
    requests: Vec<PendingChoiceRequest>,
    state: Fake,
    final_hash: fineprint_base::hash::Digest,
    observed: Observed,
}

fn drive_random(rng: &mut Pcg64Dxsm, f: &Fake, req: &CastRequest) -> Driven {
    let mut s = start(f, req);
    let mut requests = Vec::new();
    loop {
        let p = match s.status() {
            Status::Pending(p) => p.clone(),
            Status::Done { state, output } => {
                return Driven {
                    requests,
                    state: state.clone(),
                    final_hash: s.state_hash(),
                    observed: output.clone(),
                };
            }
        };
        assert_eq!(s.state_hash(), f.state_hash(), "base hash while pending");
        assert!(requests.len() < 8, "too many requests: {requests:?}");
        if coin(rng) {
            let junk = junk_answer(rng);
            if p.validate(&junk).is_err() {
                let before = s.answers().len();
                assert!(s.submit(p.request_id, junk).is_err());
                assert_eq!(s.answers().len(), before, "a rejected answer was recorded");
                assert_eq!(
                    s.pending(),
                    Some(&p),
                    "a rejected answer changed the session"
                );
            }
        }
        let a = valid_answer(rng, &p);
        if let Err(e) = p.validate(&a) {
            panic!("constructed answer {a:?} rejected by {:?}: {e}", p.payload);
        }
        requests.push(p.clone());
        s.submit(p.request_id, a).expect("valid answer accepted");
    }
}

/// Checks (a) and (b) on one run.
fn check_invariants(f: &Fake, d: &Driven) {
    match &d.observed.outcome {
        // (a) CR 734.1, 734.2, REQ-CORE-005.
        AnnounceOutcome::RolledBack(rb) => {
            assert_eq!(rb.state_hash, f.state_hash());
            assert_eq!(rb.priority_player, f.priority);
            assert_eq!(d.final_hash, f.state_hash());
            assert_eq!(&d.state, f);
        }
        AnnounceOutcome::NotStarted { reason } => {
            assert!(
                matches!(reason, StopReason::Prohibited { .. }),
                "{reason:?}"
            );
            assert!(d.requests.is_empty());
            assert_eq!(&d.state, f);
        }
        // (b) CR 601.2c, 601.2d, 115.3, 115.5.
        AnnounceOutcome::Announced {
            stack_object,
            proposal,
        } => {
            assert_eq!(*stack_object, oid(f.next_id));
            assert_eq!(d.state.stack.len(), f.stack.len() + 1);
            assert_eq!(d.state.priority, f.priority);
            let targets_req = d
                .requests
                .iter()
                .find(|p| matches!(p.payload, ChoicePayload::ChooseTargets { .. }));
            match targets_req {
                Some(p) => {
                    let slots = target_slots_of(p);
                    assert_eq!(proposal.targets.len(), slots.len());
                    for (chosen, s) in proposal.targets.iter().zip(&slots) {
                        let set: BTreeSet<_> = chosen.iter().collect();
                        assert_eq!(set.len(), chosen.len(), "repeat in a slot");
                        assert!(chosen.iter().all(|t| s.legal.contains(t)));
                        assert!(!s.legal.contains(&ChoiceTarget::Object(*stack_object)));
                        assert!(chosen.len() as u64 >= s.min && chosen.len() as u64 <= s.max);
                    }
                }
                None => assert!(proposal.targets.iter().all(Vec::is_empty)),
            }
            let chosen: Vec<ChoiceTarget> = proposal.targets.iter().flatten().copied().collect();
            if let Some(div) = &proposal.division {
                let p = d
                    .requests
                    .iter()
                    .find(|p| matches!(p.payload, ChoicePayload::DivideValue { .. }))
                    .expect("a division without a DIVIDE_VALUE request");
                let ChoicePayload::DivideValue { total, .. } = &p.payload else {
                    unreachable!()
                };
                assert_eq!(Some(*total), f.division);
                assert_eq!(div.iter().map(|(_, a)| a).sum::<u64>(), *total);
                assert!(div.iter().all(|(_, a)| *a >= 1));
                // Each chosen target is a recipient once (the DIVIDE_VALUE protocol needs
                // distinct recipients; docs/DECISIONS.md "#52 FP-050 tester — Division
                // recipients chosen in two slots").
                let got: Vec<ChoiceTarget> = div.iter().map(|(t, _)| *t).collect();
                let distinct: BTreeSet<ChoiceTarget> = got.iter().copied().collect();
                assert_eq!(distinct.len(), got.len(), "a recipient appears twice");
                let want: BTreeSet<ChoiceTarget> = chosen.iter().copied().collect();
                assert_eq!(distinct, want, "division recipients are the chosen targets");
            } else if f.division.is_some() {
                assert!(chosen.is_empty(), "division total without a division");
            }
        }
    }
}

/// REQ-CAST-001, REQ-CORE-005, REQ-NET-003; CR 601.2, 734.1, 734.2 (property a), CR 601.2c,
/// 601.2d, 115.3, 115.5 (property b): for random fakes, rule sets and answer sequences, every
/// RolledBack restores the identical hash and priority, and every Announced proposal uses only
/// offered legal targets without repeats in a slot and divides the exact total with shares of
/// at least 1.
#[test]
fn prop_rollback_identity_and_announced_proposals() {
    let mut kinds = [0u32; 3];
    for seed in 0..CASES {
        let mut rng = Pcg64Dxsm::from_u64_seed(0x0052_0050_0000 + seed);
        let (f, req, _) = random_case(&mut rng);
        let d = drive_random(&mut rng, &f, &req);
        check_invariants(&f, &d);
        kinds[match d.observed.outcome {
            AnnounceOutcome::NotStarted { .. } => 0,
            AnnounceOutcome::Announced { .. } => 1,
            AnnounceOutcome::RolledBack(_) => 2,
        }] += 1;
        // Determinism (R3): the same seed gives the same run.
        let mut again = Pcg64Dxsm::from_u64_seed(0x0052_0050_0000 + seed);
        let (f2, req2, _) = random_case(&mut again);
        let d2 = drive_random(&mut again, &f2, &req2);
        assert_eq!(d2.requests, d.requests);
        assert_eq!(d2.observed, d.observed);
        assert_eq!(d2.final_hash, d.final_hash);
    }
    // The generator reaches every outcome.
    assert!(kinds.iter().all(|&k| k > 0), "outcome mix {kinds:?}");
}

/// Random JSON of any shape (depth <= 3), with keys and strings drawn from a pool that includes
/// the convention's names.
fn random_json(rng: &mut Pcg64Dxsm, depth: u32, keys: &[&str]) -> Value {
    let strings = [
        "HEXPROOF",
        "SHROUD",
        "PROTECTION",
        "OTHER",
        "WARD",
        "PROHIBIT",
        "IGNORE_HEXPROOF",
        "OPPONENTS",
        "ANY",
        "CAST",
        "ACTIVATE",
        QUALITY_A,
        "",
        "\u{0}",
        "even",
    ];
    let top = if depth == 0 { 6 } else { 8 };
    match below(rng, top) {
        0 => Value::Null,
        1 => json!(coin(rng)),
        2 => json!(*pick(rng, &[0u64, 1, 2, 5, 6, 10, 11, 12, 100, u64::MAX])),
        3 => json!(*pick(rng, &[-1i64, -10, i64::MIN])),
        4 => json!(*pick(rng, &[0.5f64, 10.0, -1.5, 1e300])),
        5 => json!(*pick(rng, &strings)),
        6 => Value::Array(
            (0..below(rng, 4))
                .map(|_| random_json(rng, depth - 1, keys))
                .collect(),
        ),
        _ => {
            let mut m = Map::new();
            for _ in 0..below(rng, 6) {
                let k = (*pick(rng, keys)).to_owned();
                let v = random_json(rng, depth - 1, keys);
                m.insert(k, v);
            }
            Value::Object(m)
        }
    }
}

const ALL_KEYS: [&str; 14] = [
    "keyword",
    "effect",
    "target_object",
    "target_player",
    "sources",
    "quality",
    "source_player",
    "action",
    "zone",
    "object",
    "test.applies",
    "test.could_lapse",
    "test.parity",
    "test.unrelated",
];

/// Keys that cannot make an entry well formed: targeting entries without "keyword",
/// prohibitions without "action".
const TARGETING_JUNK_KEYS: [&str; 9] = [
    "effect",
    "target_object",
    "target_player",
    "sources",
    "quality",
    "source_player",
    "zone",
    "object",
    "test.unrelated",
];
const PROHIBITION_JUNK_KEYS: [&str; 8] = [
    "zone",
    "object",
    "keyword",
    "test.applies",
    "test.could_lapse",
    "test.parity",
    "test.unrelated",
    "effect",
];

/// Requests and outcome with the hash fields removed (the rule set is part of the fake's hash).
fn shape(d: &Driven) -> (Vec<PendingChoiceRequest>, String) {
    let o = match &d.observed.outcome {
        AnnounceOutcome::RolledBack(rb) => {
            format!(
                "RolledBack {:?} {:?} {:?}",
                rb.step, rb.reason, rb.priority_player
            )
        }
        other => format!("{other:?}"),
    };
    (d.requests.clone(), o)
}

/// REQ-CAST-001, REQ-DSL-013, R12; CR 601.2, 734.1 (property c): `announce` never panics for
/// TARGETING_RULE and PROHIBITION entries with params of any shape, still upholds (a)/(b), and
/// entries that cannot be well formed (no string "keyword" / no string "action", or an unknown
/// keyword or action) change nothing.
#[test]
fn prop_arbitrary_params_never_panic_and_malformed_do_not_apply() {
    for seed in 0..CASES {
        let mut rng = Pcg64Dxsm::from_u64_seed(0x0052_00C0_0000 + seed);
        let (f, req, items0) = random_case(&mut rng);
        let answer_seed = rng.next_u64();

        // Fully arbitrary params: no panic, invariants hold.
        let mut items = Vec::new();
        for i in 0..between(&mut rng, 1, 4) {
            let kind = if coin(&mut rng) {
                RuleKind::TargetingRule
            } else {
                RuleKind::Prohibition
            };
            let params = random_json(&mut rng, 3, &ALL_KEYS);
            let players = random_players(&mut rng);
            items.push(r(200 + i, kind, params, &players, below(&mut rng, 20)));
        }
        let mut all = items0.clone();
        all.extend(items.iter().cloned());
        let wild = f.clone().with_rules(&all);
        let d = drive_random(&mut Pcg64Dxsm::from_u64_seed(answer_seed), &wild, &req);
        check_invariants(&wild, &d);

        // Malformed entries only: identical requests and outcome to the case without them.
        let mut junk = Vec::new();
        for i in 0..between(&mut rng, 1, 4) {
            let players = random_players(&mut rng);
            let ts = below(&mut rng, 20);
            let (kind, mut params) = if coin(&mut rng) {
                (
                    RuleKind::TargetingRule,
                    random_json(&mut rng, 2, &TARGETING_JUNK_KEYS),
                )
            } else {
                (
                    RuleKind::Prohibition,
                    random_json(&mut rng, 2, &PROHIBITION_JUNK_KEYS),
                )
            };
            // Optionally add the required key with a value that cannot be valid.
            if let Value::Object(m) = &mut params
                && coin(&mut rng)
            {
                let key = if kind == RuleKind::TargetingRule {
                    "keyword"
                } else {
                    "action"
                };
                let bad = match below(&mut rng, 4) {
                    0 => json!(7),
                    1 => json!("WARD"),
                    2 => json!(["CAST"]),
                    _ => json!(null),
                };
                m.insert(key.to_owned(), bad);
            }
            junk.push(r(300 + i, kind, params, &players, ts));
        }
        let mut with_junk_items = items0.clone();
        with_junk_items.extend(junk.iter().cloned());
        let with_junk = f.clone().with_rules(&with_junk_items);
        let a = drive_random(&mut Pcg64Dxsm::from_u64_seed(answer_seed), &f, &req);
        let b = drive_random(&mut Pcg64Dxsm::from_u64_seed(answer_seed), &with_junk, &req);
        check_invariants(&with_junk, &b);
        assert_eq!(
            shape(&a),
            shape(&b),
            "seed {seed}: malformed entries {junk:?}"
        );
    }
}

/// REQ-CAST-001 (step 3), REQ-DSL-013; CR 101.2, 702.11e (property d): a candidate removed by
/// an applicable PROHIBIT entry stays removed whatever other entries are added, unless the
/// entry is a HEXPROOF one and an IGNORE_HEXPROOF for the caster on that target is added.
#[test]
fn prop_prohibit_is_monotone() {
    let target = to(10);
    let prohibits = [
        shroud_obj(10),
        protection_all_obj(10),
        protection_from_player_obj(10, 1),
        other_obj(10),
        hexproof_obj(10),
        hexproof_from_obj(10, QUALITY_A),
        protection_obj(10, QUALITY_A),
    ];
    for seed in 0..CASES {
        let mut rng = Pcg64Dxsm::from_u64_seed(0x0052_00D0_0000 + seed);
        let req = if coin(&mut rng) {
            spell(P1)
        } else {
            ability(P1)
        };
        let p = pick(&mut rng, &prohibits).clone();
        let is_hexproof = p["keyword"] == "HEXPROOF";
        let base = Fake::one_slot(0, 3, &[tp(1), tp(2), to(10), to(11), to(12)])
            .with_qualities(&[QUALITY_A]);
        let alone = base
            .clone()
            .with_rules(&[tr(1, p.clone(), below(&mut rng, 20))]);
        assert!(!legal(&alone, &req).contains(&target), "precondition {p}");

        let mut extra = random_rules(&mut rng, 10, 8, false);
        // A random grant of a "can" for the caster in a random position.
        if coin(&mut rng) {
            extra.push(r(
                40,
                RuleKind::CastPermission,
                json!({"action": action_of(&req)}),
                &[P1],
                below(&mut rng, 20),
            ));
        }
        let lifts = |e: &R| {
            e.kind == RuleKind::TargetingRule
                && e.params.get("effect") == Some(&json!("IGNORE_HEXPROOF"))
                && e.params.get("target_object") == Some(&json!(10))
                && e.players.contains(&P1)
        };
        if is_hexproof {
            extra.retain(|e| !lifts(e));
        }
        let mut all = vec![tr(1, p.clone(), below(&mut rng, 20))];
        all.extend(extra.iter().cloned());
        let f = base.clone().with_rules(&all);
        assert!(
            !legal(&f, &req).contains(&target),
            "seed {seed}: {p} lifted by {extra:?}"
        );

        // The exception: IGNORE_HEXPROOF lifts a lone HEXPROOF entry.
        if is_hexproof {
            let f = base.clone().with_rules(&[
                tr(1, p.clone(), below(&mut rng, 20)),
                tr_for(2, ignore_hexproof_obj(10), &[P1], below(&mut rng, 20)),
            ]);
            assert!(
                legal(&f, &req).contains(&target),
                "seed {seed}: {p} not lifted"
            );
        }
    }
}
