// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Seeded property tests for `fineprint_core::choice` (REQ-NET-003, REQ-CORE-001,
//! REQ-CORE-006, REQ-DSL-011). The only randomness is `Pcg64Dxsm::from_u64_seed` over fixed
//! seeds (R3, R7). Black-box: written from the work-item brief only.

#[path = "spec_choice_support.rs"]
mod support;

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::choice::*;
use fineprint_core::state::GameState;
use fineprint_core::txn::Transactional;
use fineprint_dsl::choice::ChoiceKind;
use support::*;

/// Seeds per property (the brief asks for at least 64).
const SEEDS: u64 = 96;

/// REQ-NET-003, REQ-DSL-011 property (i): for random small specs, the request is enumerated
/// with exactly the brief's items, and answers built from those items and the bounds are
/// always accepted.
#[test]
fn req_net_003_prop_answers_from_items_accepted() {
    for seed in 0..SEEDS {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        for kind in SMALL_KINDS {
            let r = random_small(&mut rng, kind);
            let req = expect_ask(generate_request(
                RequestId::new(seed),
                &spec(kind),
                r.clone(),
            ));
            assert_eq!(req.choice_type, expected_type(kind));
            assert_eq!(items(&req), oracle_items(kind, &r), "seed {seed} {kind:?}");
            for _ in 0..4 {
                let a = answer_from_items(&mut rng, kind, &req, &r);
                assert!(oracle_valid(kind, &r, &a), "test oracle disagrees: {a:?}");
                accept(&req, &a);
            }
            accept(&req, &req.default_on_timeout);
        }
    }
}

/// One random mutation of a (usually valid) answer.
fn mutate(rng: &mut Pcg64Dxsm, a: &ChoiceResponse) -> ChoiceResponse {
    use ChoiceResponse as R;
    let foreign = 1_000 + below(rng, 5);
    let roll = below(rng, 8);
    if roll == 0 {
        // Wrong variant.
        let s = pinned_samples();
        return s[below(rng, s.len() as u64) as usize].clone();
    }
    let vec_mut = |rng: &mut Pcg64Dxsm, v: &mut Vec<u64>| {
        match below(rng, 4) {
            0 if !v.is_empty() => {
                let i = below(rng, v.len() as u64) as usize;
                v.remove(i);
            }
            1 if !v.is_empty() => {
                let i = below(rng, v.len() as u64) as usize;
                v.push(v[i]);
            }
            2 if !v.is_empty() => {
                let i = below(rng, v.len() as u64) as usize;
                v[i] = foreign;
            }
            _ => v.push(foreign),
        };
    };
    match a {
        R::SubmitObjectSelection { objects } => {
            let mut v: Vec<u64> = objects.iter().map(|o| o.get()).collect();
            vec_mut(rng, &mut v);
            sel(&v)
        }
        R::SelectModes { modes } => {
            let mut v: Vec<u64> = modes.iter().map(|&m| u64::from(m)).collect();
            vec_mut(rng, &mut v);
            R::SelectModes {
                modes: v.into_iter().map(|m| m as u32).collect(),
            }
        }
        R::SubmitChoiceValue {
            value: ChoiceValue::Number(v),
        } => match below(rng, 4) {
            0 => num(v + 1),
            1 => num(v - 1),
            2 => num(-1 - below(rng, 10) as i64),
            _ => text(&v.to_string()),
        },
        R::SubmitChoiceValue {
            value: ChoiceValue::Text(s),
        } => match below(rng, 3) {
            0 => text(&format!("{s}_x")),
            1 => text(&format!("test.value_{:05}", below(rng, 12))),
            _ => num(0),
        },
        R::SubmitPlayer { .. } => R::SubmitPlayer {
            player: pid(below(rng, 70)),
        },
        R::SubmitOrdering { top, .. } => {
            let mut v: Vec<u64> = top.iter().map(|o| o.get()).collect();
            if below(rng, 3) == 0 && !v.is_empty() {
                let moved = v.pop().expect("non-empty");
                if below(rng, 2) == 0 {
                    ordering(&v, &[moved], &[])
                } else {
                    ordering(&v, &[], &[moved])
                }
            } else {
                vec_mut(rng, &mut v);
                ordering(&v, &[], &[])
            }
        }
        R::SubmitDivision { assignments } => {
            let mut v = assignments.clone();
            let i = below(rng, v.len().max(1) as u64) as usize;
            match below(rng, 6) {
                0 if !v.is_empty() => v[i].1 = v[i].1.saturating_add(1),
                1 if !v.is_empty() => v[i].1 = v[i].1.saturating_sub(1),
                2 if !v.is_empty() => {
                    v.remove(i);
                }
                3 if !v.is_empty() => v.push(v[i]),
                4 if !v.is_empty() => v[i].0 = to(foreign),
                _ if !v.is_empty() => v[i].1 = u64::MAX,
                _ => v.push((to(foreign), 1)),
            }
            R::SubmitDivision { assignments: v }
        }
        R::Yes => R::No,
        R::No => R::Yes,
        other => other.clone(),
    }
}

/// REQ-NET-003 property (ii): for random mutations of valid answers (drop, duplicate,
/// non-candidate, out-of-range, wrong variant), `validate` agrees with the independent oracle,
/// and every rejection carries a non-empty explanation. Includes validator-sized requests.
#[test]
fn req_net_003_prop_validate_matches_oracle() {
    let mut rejected = 0u64;
    let mut accepted = 0u64;
    for seed in 0..SEEDS {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed ^ 0xC401_CE00);
        for kind in SMALL_KINDS {
            let r = random_small(&mut rng, kind);
            let req = ask_kind(kind, r.clone());
            let valid = answer_from_items(&mut rng, kind, &req, &r);
            let mut current = valid;
            for _ in 0..6 {
                let m = mutate(&mut rng, &current);
                let want = oracle_valid(kind, &r, &m);
                match req.validate(&m) {
                    Ok(()) => {
                        assert!(
                            want,
                            "seed {seed} {kind:?}: accepted invalid {m:?} for {r:?}"
                        );
                        accepted += 1;
                    }
                    Err(e) => {
                        assert!(!want, "seed {seed} {kind:?}: rejected valid {m:?}: {e}");
                        assert!(!e.trim().is_empty());
                        rejected += 1;
                    }
                }
                current = m;
            }
        }
        // A validator-sized OBJECTS request (n = 14..=20, min 0, max n).
        let n = between(&mut rng, 14, 20);
        let r = resolved(objects(&range_ids(n)), 0, n);
        let req = ask_kind(ChoiceKind::Objects, r.clone());
        assert!(is_validator(&req));
        let mut v: Vec<u64> = range_ids(n);
        rng.shuffle(&mut v);
        v.truncate(below(&mut rng, n + 1) as usize);
        let m = mutate(&mut rng, &sel(&v));
        assert_eq!(
            req.validate(&m).is_ok(),
            oracle_valid(ChoiceKind::Objects, &r, &m),
            "{m:?}"
        );
    }
    assert!(
        rejected > 0 && accepted > 0,
        "mutations exercise both verdicts"
    );
}

/// REQ-NET-003, R12 property: random strings submitted as card names agree with the brief's
/// structural check and never panic.
#[test]
fn req_net_003_prop_card_names_match_oracle() {
    let mut r = resolved(ResolvedCandidates::None, 1, 1);
    r.default = Some(ChoiceResponse::SubmitCardName {
        name: "test.name_a".to_owned(),
    });
    let req = ask_kind(ChoiceKind::CardName, r);
    for seed in 0..SEEDS {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed ^ 0x0A4E);
        for _ in 0..32 {
            let len = match below(&mut rng, 4) {
                0 => below(&mut rng, 4),
                1 => between(&mut rng, 250, 262),
                _ => below(&mut rng, 40),
            };
            let mut name = String::new();
            for _ in 0..len {
                let c = match below(&mut rng, 6) {
                    0 => char::from_u32(below(&mut rng, 0x20) as u32),
                    1 => Some(' '),
                    2 => char::from_u32(0x80 + below(&mut rng, 0x800) as u32),
                    _ => char::from_u32(0x61 + below(&mut rng, 26) as u32),
                };
                name.push(c.unwrap_or('x'));
            }
            let resp = ChoiceResponse::SubmitCardName { name: name.clone() };
            match req.validate(&resp) {
                Ok(()) => assert!(oracle_card_name(&name), "accepted {name:?}"),
                Err(e) => {
                    assert!(!oracle_card_name(&name), "rejected {name:?}: {e}");
                    assert!(!e.trim().is_empty());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Property (iii): random multi-ask procedures.
// ---------------------------------------------------------------------------------------------

type Steps = Vec<(ChoiceKind, ResolvedChoice)>;

/// 1..=4 random asks.
fn random_steps(rng: &mut Pcg64Dxsm) -> Steps {
    (0..between(rng, 1, 4))
        .map(|_| {
            let kind = SMALL_KINDS[below(rng, SMALL_KINDS.len() as u64) as usize];
            (kind, random_small(rng, kind))
        })
        .collect()
}

/// The toy procedure: before each ask the counter moves and the log grows; the answer is
/// folded into the log.
fn toy_proc(steps: Steps) -> impl Fn(&mut Toy, &mut ChoiceContext<'_>) -> Result<usize, Interrupt> {
    move |s, ctx| {
        for (i, (kind, r)) in steps.iter().enumerate() {
            s.counter += i as i64 + 1;
            s.log.push(i as u8);
            let a = ask_in(ctx, *kind, r.clone())?;
            s.log.extend(format!("{a:?}").as_bytes());
        }
        Ok(steps.len())
    }
}

/// `toy_proc` by hand.
fn toy_hand(base: &Toy, answers: &[ChoiceResponse]) -> Toy {
    let mut s = base.clone();
    for (i, a) in answers.iter().enumerate() {
        s.counter += i as i64 + 1;
        s.log.push(i as u8);
        s.log.extend(format!("{a:?}").as_bytes());
    }
    s
}

/// The game procedure: PRNG draws and life changes before and after each ask, the change
/// depending on the answer.
fn game_proc(
    steps: Steps,
) -> impl Fn(&mut GameState, &mut ChoiceContext<'_>) -> Result<Vec<u64>, Interrupt> {
    move |s, ctx| {
        let mut out = Vec::new();
        for (kind, r) in &steps {
            out.push(s.prng_mut().next_u64());
            let a = ask_in(ctx, *kind, r.clone())?;
            let cost = format!("{a:?}").len() as i64 % 7;
            let life = s.player(pid(1)).expect("player").life();
            s.player_mut(pid(1)).expect("player").set_life(life - cost);
            out.push(s.prng_mut().next_bounded(1_000).expect("bound"));
        }
        Ok(out)
    }
}

/// Valid answers for `steps` (the i-th request is generated with id i).
fn answers_for(rng: &mut Pcg64Dxsm, steps: &Steps) -> Vec<ChoiceResponse> {
    steps
        .iter()
        .enumerate()
        .map(|(i, (kind, r))| {
            let req = expect_ask(generate_request(
                RequestId::new(i as u64),
                &spec(*kind),
                r.clone(),
            ));
            answer_from_items(rng, *kind, &req, r)
        })
        .collect()
}

/// REQ-CORE-001, REQ-CORE-006, REQ-NET-003 property (iii), toy state: incremental submission
/// and replay of the recorded answers give identical hashes; re-running the whole sequence
/// gives the same hash; the hash equals a hand-computed run.
#[test]
fn req_core_001_prop_incremental_equals_replay_toy() {
    for seed in 0..SEEDS {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed ^ 0x7011);
        let steps = random_steps(&mut rng);
        let answers = answers_for(&mut rng, &steps);
        let b = Toy {
            counter: seed as i64,
            log: vec![seed as u8],
        };
        let mut s = ChoiceSession::start(b.clone(), toy_proc(steps.clone())).expect("start");
        for (i, a) in answers.iter().enumerate() {
            assert_eq!(s.state_hash(), b.state_hash());
            assert_eq!(
                s.pending().expect("pending").request_id,
                RequestId::new(i as u64)
            );
            s.submit(RequestId::new(i as u64), a.clone())
                .expect("valid");
        }
        assert!(is_done(&s));
        let h = s.state_hash();
        assert_eq!(h, toy_hand(&b, &answers).state_hash(), "seed {seed}");
        let r =
            ChoiceSession::replay(b.clone(), toy_proc(steps.clone()), s.answers()).expect("replay");
        assert_eq!(r.state_hash(), h);
        let mut again = ChoiceSession::start(b.clone(), toy_proc(steps.clone())).expect("start");
        submit_all(&mut again, &answers);
        assert_eq!(again.state_hash(), h);
        assert_eq!(again.answers(), s.answers());
    }
}

/// REQ-CORE-001, REQ-CORE-006, REQ-NET-003 property (iii), GameState: the same, with PRNG
/// draws between asks; defaults and explicit answers mix.
#[test]
fn req_core_001_prop_incremental_equals_replay_game() {
    for seed in 0..SEEDS {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed ^ 0x6A3E);
        let steps = random_steps(&mut rng);
        let answers = answers_for(&mut rng, &steps);
        let b = GameState::new(seed, 2, 20).expect("game");
        let mut s = ChoiceSession::start(b.clone(), game_proc(steps.clone())).expect("start");
        for a in &answers {
            assert_eq!(s.state_hash(), b.state_hash());
            let id = s.pending().expect("pending").request_id;
            if below(&mut rng, 3) == 0 {
                s.submit_default(id).expect("default");
            } else {
                s.submit(id, a.clone()).expect("valid");
            }
        }
        assert!(is_done(&s));
        let h = s.state_hash();
        let recorded = s.answers().to_vec();
        assert_eq!(recorded.len(), steps.len());
        for _ in 0..2 {
            let r = ChoiceSession::replay(b.clone(), game_proc(steps.clone()), &recorded)
                .expect("replay");
            assert_eq!(r.state_hash(), h, "seed {seed}");
            let mut again =
                ChoiceSession::start(b.clone(), game_proc(steps.clone())).expect("start");
            submit_all(&mut again, &recorded);
            assert_eq!(again.state_hash(), h, "seed {seed}");
        }
    }
}

/// REQ-DSL-011, REQ-NET-003 property (iv): the order in which Objects/Players candidates are
/// supplied does not change the generated request (sorted ascending); the order of Strings
/// does (kept as given).
#[test]
fn req_dsl_011_prop_candidate_order() {
    for seed in 0..SEEDS {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed ^ 0x0DE5);
        let n = between(&mut rng, 2, 9);
        let mut ids: Vec<u64> = (1..=40).collect();
        rng.shuffle(&mut ids);
        ids.truncate(n as usize);
        let mut shuffled = ids.clone();
        rng.shuffle(&mut shuffled);
        let min = below(&mut rng, n + 1);
        for kind in [ChoiceKind::Objects, ChoiceKind::Order] {
            let (lo, hi) = if kind == ChoiceKind::Order {
                (n, n)
            } else {
                (min, n)
            };
            let a = ask_kind(kind, resolved(objects(&ids), lo, hi));
            let b = ask_kind(kind, resolved(objects(&shuffled), lo, hi));
            assert_eq!(a, b, "seed {seed} {kind:?}");
            assert_eq!(a.digest(), b.digest());
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            assert_eq!(
                items(&a),
                sorted
                    .iter()
                    .map(|&i| ChoiceOption::Object(oid(i)))
                    .collect::<Vec<_>>()
            );
        }
        let a = ask_kind(ChoiceKind::Player, resolved(players(&ids), 1, 1));
        let b = ask_kind(ChoiceKind::Player, resolved(players(&shuffled), 1, 1));
        assert_eq!(a, b, "seed {seed} PLAYER");
        assert_eq!(a.digest(), b.digest());

        let l = labels("value", n as usize);
        let mut l2 = l.clone();
        rng.shuffle(&mut l2);
        if l2 == l {
            l2.reverse();
        }
        for kind in [
            ChoiceKind::Color,
            ChoiceKind::CreatureType,
            ChoiceKind::Pile,
        ] {
            let a = ask_kind(kind, resolved(strings(&l), 1, 1));
            let b = ask_kind(kind, resolved(strings(&l2), 1, 1));
            assert_ne!(a, b, "seed {seed} {kind:?}");
            assert_eq!(
                items(&b),
                l2.iter()
                    .cloned()
                    .map(ChoiceOption::Text)
                    .collect::<Vec<_>>()
            );
        }
    }
}

/// REQ-CORE-006: generation is a pure function of its inputs: repeated generation from the
/// same random inputs gives equal requests and digests.
#[test]
fn req_core_006_prop_generation_pure() {
    for seed in 0..SEEDS {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed ^ 0x9E7E);
        for kind in SMALL_KINDS {
            let r = random_small(&mut rng, kind);
            let a = ask_kind(kind, r.clone());
            let b = ask_kind(kind, r);
            assert_eq!(a, b);
            assert_eq!(a.digest(), b.digest());
        }
    }
}
