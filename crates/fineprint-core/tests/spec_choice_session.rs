// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `ChoiceSession`: suspension at a pending choice, validation of submitted
//! answers, and deterministic resumption by re-running the procedure from the base state
//! (REQ-NET-003, REQ-CORE-001, REQ-CORE-006). Run over `GameState` and over a toy
//! `Transactional` state (the driver is generic). Black-box: written from the work-item brief
//! only.

#[path = "spec_choice_support.rs"]
mod support;

use std::cell::Cell;

use fineprint_core::choice::*;
use fineprint_core::state::GameState;
use fineprint_core::txn::Transactional;
use fineprint_dsl::choice::ChoiceKind;
use support::*;

// ---------------------------------------------------------------------------------------------
// Procedures.
// ---------------------------------------------------------------------------------------------

/// A two-player game at 20 life with a fixed seed.
fn base(seed: u64) -> GameState {
    GameState::new(seed, 2, 20).expect("valid game")
}

/// Player `who` loses `n` life.
fn lose(s: &mut GameState, who: u64, n: i64) {
    let life = s.player(pid(who)).expect("player").life();
    s.player_mut(pid(who)).expect("player").set_life(life - n);
}

/// The life of player `who`.
fn life(s: &GameState, who: u64) -> i64 {
    s.player(pid(who)).expect("player").life()
}

fn yes_no() -> ResolvedChoice {
    resolved(ResolvedCandidates::None, 1, 1)
}

fn number_0_5() -> ResolvedChoice {
    resolved(ResolvedCandidates::None, 0, 5)
}

fn objects_1_2() -> ResolvedChoice {
    resolved(objects(&[1, 2, 3, 4]), 1, 2)
}

/// The number in a NUMBER answer.
fn number_of(r: &ChoiceResponse) -> Result<i64, Interrupt> {
    match r {
        ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Number(v),
        } => Ok(*v),
        _ => Err(ChoiceError::Procedure("not a number".to_owned()).into()),
    }
}

/// The id sum of an OBJECTS answer.
fn id_sum(r: &ChoiceResponse) -> Result<i64, Interrupt> {
    match r {
        ChoiceResponse::SubmitObjectSelection { objects } => {
            Ok(objects.iter().map(|o| o.get() as i64).sum())
        }
        _ => Err(ChoiceError::Procedure("not a selection".to_owned()).into()),
    }
}

/// Three asks over a game: mutates life and draws from the PRNG before, between and after
/// asks; the losses depend on the answers. Output: the PRNG draws.
fn three_asks() -> impl Fn(&mut GameState, &mut ChoiceContext<'_>) -> Result<Vec<u64>, Interrupt> {
    |s, ctx| {
        let mut out = Vec::new();
        lose(s, 1, 1);
        out.push(s.prng_mut().next_u64());
        let a = ask_in(ctx, ChoiceKind::YesNo, yes_no())?;
        if a == ChoiceResponse::Yes {
            lose(s, 2, 3);
        }
        out.push(s.prng_mut().next_u64());
        let b = ask_in(ctx, ChoiceKind::Number, number_0_5())?;
        lose(s, 1, number_of(&b)?);
        out.push(s.prng_mut().next_bounded(100).expect("bound"));
        let c = ask_in(ctx, ChoiceKind::Objects, objects_1_2())?;
        lose(s, 2, id_sum(&c)?);
        out.push(s.prng_mut().next_u64());
        Ok(out)
    }
}

/// The same logic as `three_asks`, written by hand on a clone with the answers known.
fn hand_three(base: &GameState, yes: bool, v: i64, picks: &[u64]) -> (GameState, Vec<u64>) {
    let mut s = base.clone();
    let mut out = Vec::new();
    lose(&mut s, 1, 1);
    out.push(s.prng_mut().next_u64());
    if yes {
        lose(&mut s, 2, 3);
    }
    out.push(s.prng_mut().next_u64());
    lose(&mut s, 1, v);
    out.push(s.prng_mut().next_bounded(100).expect("bound"));
    lose(&mut s, 2, picks.iter().map(|&p| p as i64).sum());
    out.push(s.prng_mut().next_u64());
    (s, out)
}

/// The answers matching `hand_three(.., yes, v, picks)`.
fn three_answers(yes: bool, v: i64, picks: &[u64]) -> Vec<ChoiceResponse> {
    vec![
        if yes {
            ChoiceResponse::Yes
        } else {
            ChoiceResponse::No
        },
        num(v),
        sel(picks),
    ]
}

/// `k` YES_NO asks over a game; each Yes costs player 1 `i + 1` life; a PRNG draw precedes
/// every ask.
fn k_asks_game(
    k: usize,
) -> impl Fn(&mut GameState, &mut ChoiceContext<'_>) -> Result<usize, Interrupt> {
    move |s, ctx| {
        let mut yes = 0;
        for i in 0..k {
            s.prng_mut().next_u64();
            lose(s, 2, 1);
            if ask_in(ctx, ChoiceKind::YesNo, yes_no())? == ChoiceResponse::Yes {
                lose(s, 1, i as i64 + 1);
                yes += 1;
            }
        }
        Ok(yes)
    }
}

/// `k` NUMBER asks over the toy state: each answer is added to the counter and logged.
fn k_asks_toy(k: usize) -> impl Fn(&mut Toy, &mut ChoiceContext<'_>) -> Result<i64, Interrupt> {
    move |s, ctx| {
        for i in 0..k {
            s.counter += 100;
            s.log.push(i as u8);
            let v = number_of(&ask_in(ctx, ChoiceKind::Number, number_0_5())?)?;
            s.counter += v * (i as i64 + 1);
            s.log.push(v as u8);
        }
        Ok(s.counter)
    }
}

/// The toy state after `k_asks_toy` with these numbers, by hand.
fn hand_toy(base: &Toy, answers: &[i64]) -> Toy {
    let mut s = base.clone();
    for (i, v) in answers.iter().enumerate() {
        s.counter += 100;
        s.log.push(i as u8);
        s.counter += v * (i as i64 + 1);
        s.log.push(*v as u8);
    }
    s
}

// ---------------------------------------------------------------------------------------------
// (a) Pending count and request ids.
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, REQ-CORE-001: 0 asks is immediately Done; k asks are Pending k times then
/// Done; the successive request ids are 0, 1, 2, ... (GameState).
#[test]
fn req_net_003_pending_k_times_then_done_game() {
    for k in 0..5 {
        let mut s = ChoiceSession::start(base(1), k_asks_game(k)).expect("start");
        let mut seen = Vec::new();
        while let Some(req) = s.pending().cloned() {
            assert!(matches!(s.status(), Status::Pending(p) if *p == req));
            seen.push(req.request_id);
            s.submit(req.request_id, ChoiceResponse::Yes)
                .expect("valid");
        }
        assert!(is_done(&s));
        let expected: Vec<RequestId> = (0..k as u64).map(RequestId::new).collect();
        assert_eq!(seen, expected, "k = {k}");
        assert_eq!(s.answers().len(), k);
        let Status::Done { output, .. } = s.status() else {
            unreachable!()
        };
        assert_eq!(*output, k);
    }
}

/// REQ-NET-003, requirement 8: the same over the toy state.
#[test]
fn req_net_003_pending_k_times_then_done_toy() {
    for k in 0..5 {
        let mut s = ChoiceSession::start(Toy::default(), k_asks_toy(k)).expect("start");
        let mut n = 0;
        while let Some(req) = s.pending().cloned() {
            assert_eq!(req.request_id, RequestId::new(n));
            s.submit(req.request_id, num(2)).expect("valid");
            n += 1;
        }
        assert_eq!(n, k as u64);
        let Status::Done { state, output } = s.status() else {
            panic!("not done")
        };
        let want = hand_toy(&Toy::default(), &vec![2; k]);
        assert_eq!(*state, want);
        assert_eq!(*output, want.counter);
        assert_eq!(s.state_hash(), want.state_hash());
    }
}

// ---------------------------------------------------------------------------------------------
// (b), (c) Hashes while pending and after Done.
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, REQ-CORE-001: while Pending the session hash is the base hash, though the
/// procedure changed life and drew from the PRNG before each ask; after Done it is the Done
/// state's hash, and equals a hand-computed run on a clone.
#[test]
fn req_net_003_hash_pending_is_base_done_is_final_game() {
    let b = base(7);
    let h0 = b.state_hash();
    for (yes, v, picks) in [
        (true, 3, vec![2u64]),
        (false, 0, vec![4, 1]),
        (true, 5, vec![3, 4]),
    ] {
        let mut s = ChoiceSession::start(b.clone(), three_asks()).expect("start");
        for a in three_answers(yes, v, &picks) {
            assert_eq!(s.state_hash(), h0, "pending hash must be the base hash");
            let id = s.pending().expect("pending").request_id;
            s.submit(id, a).expect("valid");
        }
        assert!(is_done(&s));
        let (want, out) = hand_three(&b, yes, v, &picks);
        assert_ne!(want.state_hash(), h0);
        assert_eq!(s.state_hash(), done_hash(&s));
        assert_eq!(s.state_hash(), want.state_hash());
        let Status::Done { state, output } = s.status() else {
            unreachable!()
        };
        assert_eq!(*state, want);
        assert_eq!(*output, out);
        assert_eq!(life(state, 1), 20 - 1 - v);
    }
}

/// REQ-NET-003, requirement 8: pending hash is the base hash over the toy state too.
#[test]
fn req_net_003_hash_pending_is_base_toy() {
    let b = Toy {
        counter: 5,
        log: vec![9],
    };
    let h0 = b.state_hash();
    let mut s = ChoiceSession::start(b.clone(), k_asks_toy(3)).expect("start");
    for v in [1, 4, 0] {
        assert_eq!(s.state_hash(), h0);
        let id = s.pending().expect("pending").request_id;
        s.submit(id, num(v)).expect("valid");
    }
    assert_eq!(s.state_hash(), hand_toy(&b, &[1, 4, 0]).state_hash());
}

// ---------------------------------------------------------------------------------------------
// (d) Determinism.
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-001, REQ-CORE-006, REQ-NET-003: the same base and answers give the identical
/// final hash, output and answers over 10 runs, by incremental submit and by `replay` of the
/// recorded answers; different answers can give different results.
#[test]
fn req_core_001_same_answers_same_hash_game() {
    let answers = three_answers(true, 4, &[3, 1]);
    let mut first = ChoiceSession::start(base(11), three_asks()).expect("start");
    submit_all(&mut first, &answers);
    let h = first.state_hash();
    let recorded = first.answers().to_vec();
    assert_eq!(recorded, answers);
    for _ in 0..10 {
        let mut s = ChoiceSession::start(base(11), three_asks()).expect("start");
        submit_all(&mut s, &answers);
        assert_eq!(s.state_hash(), h);
        assert_eq!(s.answers(), &recorded[..]);
        let r = ChoiceSession::replay(base(11), three_asks(), &recorded).expect("replay");
        assert!(is_done(&r));
        assert_eq!(r.state_hash(), h);
        assert_eq!(r.answers(), &recorded[..]);
        let (Status::Done { output: o1, .. }, Status::Done { output: o2, .. }) =
            (s.status(), r.status())
        else {
            panic!("not done")
        };
        assert_eq!(o1, o2);
    }
    let mut other = ChoiceSession::start(base(11), three_asks()).expect("start");
    submit_all(&mut other, &three_answers(false, 4, &[3, 1]));
    assert_ne!(other.state_hash(), h);
}

/// REQ-CORE-001, requirement 8: determinism over the toy state, including replay of a prefix
/// (a pending session) and of the empty answer list.
#[test]
fn req_core_001_same_answers_same_hash_toy() {
    let answers = [num(1), num(5), num(2)];
    let mut a = ChoiceSession::start(Toy::default(), k_asks_toy(3)).expect("start");
    submit_all(&mut a, &answers);
    for _ in 0..10 {
        let r = ChoiceSession::replay(Toy::default(), k_asks_toy(3), &answers).expect("replay");
        assert_eq!(r.state_hash(), a.state_hash());
        assert_eq!(r.answers(), a.answers());
    }
    let p = ChoiceSession::replay(Toy::default(), k_asks_toy(3), &answers[..2]).expect("replay");
    assert_eq!(p.pending().expect("pending").request_id, RequestId::new(2));
    assert_eq!(p.answers(), &answers[..2]);
    assert_eq!(p.state_hash(), Toy::default().state_hash());
    let e = ChoiceSession::replay(Toy::default(), k_asks_toy(3), &[]).expect("replay");
    let s = ChoiceSession::start(Toy::default(), k_asks_toy(3)).expect("start");
    assert_eq!(observe(&e), observe(&s));
    let mut b = ChoiceSession::start(Toy::default(), k_asks_toy(3)).expect("start");
    submit_all(&mut b, &[num(1), num(5), num(3)]);
    assert_ne!(b.state_hash(), a.state_hash());
}

// ---------------------------------------------------------------------------------------------
// (e), (f) Rejected submits change nothing.
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003: an invalid answer, a stale id, an id from the future, and a submit after Done
/// are rejected with the pinned errors and leave the session unchanged.
#[test]
fn req_net_003_rejected_submits_change_nothing() {
    let mut s = ChoiceSession::start(base(3), three_asks()).expect("start");
    s.submit(RequestId::new(0), ChoiceResponse::Yes)
        .expect("valid");
    let before = observe(&s);
    let pending = RequestId::new(1);
    assert_eq!(before.0.as_ref().expect("pending").request_id, pending);

    // Invalid answer (out of range) to the pending request.
    match s.submit(pending, num(6)) {
        Err(ChoiceError::Invalid {
            request_id,
            explanation,
        }) => {
            assert_eq!(request_id, pending);
            assert!(!explanation.trim().is_empty());
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(observe(&s), before);
    // Wrong variant.
    assert!(matches!(
        s.submit(pending, ChoiceResponse::Yes),
        Err(ChoiceError::Invalid { .. })
    ));
    assert_eq!(observe(&s), before);
    // Stale id (already answered), with a valid-looking answer.
    for (submitted, answer) in [
        (RequestId::new(0), ChoiceResponse::Yes),
        (RequestId::new(0), num(2)),
        (RequestId::new(2), num(2)),
        (RequestId::new(99), num(2)),
        (RequestId::new(u64::MAX), num(2)),
    ] {
        let got = s.submit(submitted, answer);
        assert!(
            matches!(
                got,
                Err(ChoiceError::StaleRequest { submitted: sub, pending: Some(p) })
                    if sub == submitted && p == pending
            ),
            "submitted {submitted:?}: {got:?}"
        );
        assert_eq!(observe(&s), before);
    }
    // submit_default with a wrong id is rejected too.
    assert!(s.submit_default(RequestId::new(0)).is_err());
    assert_eq!(observe(&s), before);

    // Finish, then submit after Done.
    submit_all(&mut s, &[num(2), sel(&[1])]);
    assert!(is_done(&s));
    let done = observe(&s);
    assert!(matches!(
        s.submit(RequestId::new(3), num(1)),
        Err(ChoiceError::NotPending)
    ));
    assert!(matches!(
        s.submit(RequestId::new(2), sel(&[1])),
        Err(ChoiceError::NotPending)
    ));
    assert!(matches!(
        s.submit_default(RequestId::new(3)),
        Err(ChoiceError::NotPending)
    ));
    assert_eq!(observe(&s), done);
}

/// REQ-NET-003: an invalid answer to the 2nd of 3 asks leaves exactly 1 recorded answer and the
/// 2nd request pending; a later valid answer proceeds.
#[test]
fn req_net_003_invalid_second_answer_then_valid() {
    let mut s = ChoiceSession::start(base(5), three_asks()).expect("start");
    s.submit(RequestId::new(0), ChoiceResponse::No)
        .expect("valid");
    let id = RequestId::new(1);
    for bad in [num(-1), num(6), text("2"), sel(&[1]), ChoiceResponse::No] {
        assert!(s.submit(id, bad).is_err());
        assert_eq!(s.answers(), &[ChoiceResponse::No]);
        assert_eq!(s.pending().expect("pending").request_id, id);
        assert_eq!(
            s.pending().expect("pending").choice_type,
            ChoiceType::ChooseOptionOrValue
        );
    }
    s.submit(id, num(5)).expect("valid");
    assert_eq!(s.answers(), &[ChoiceResponse::No, num(5)]);
    assert_eq!(s.pending().expect("pending").request_id, RequestId::new(2));
    s.submit(RequestId::new(2), sel(&[4, 2])).expect("valid");
    let (want, _) = hand_three(&base(5), false, 5, &[4, 2]);
    assert_eq!(s.state_hash(), want.state_hash());
}

// ---------------------------------------------------------------------------------------------
// (g) Defaults; (h) replay errors.
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003: `submit_default(id)` gives the same hash and answers as
/// `submit(id, default_on_timeout)`.
#[test]
fn req_net_003_submit_default_equals_explicit_default() {
    let mut a = ChoiceSession::start(base(9), three_asks()).expect("start");
    let mut b = ChoiceSession::start(base(9), three_asks()).expect("start");
    while let Some(req) = a.pending().cloned() {
        a.submit_default(req.request_id).expect("default accepted");
        let breq = b.pending().cloned().expect("same pending");
        assert_eq!(breq, req);
        b.submit(breq.request_id, breq.default_on_timeout.clone())
            .expect("default accepted");
        assert_eq!(observe(&a), observe(&b));
    }
    assert!(is_done(&a));
    // Defaults: No, Number(0), the 1 smallest id.
    assert_eq!(a.answers(), &three_answers(false, 0, &[1])[..]);
    let (want, _) = hand_three(&base(9), false, 0, &[1]);
    assert_eq!(a.state_hash(), want.state_hash());
}

/// REQ-NET-003: `replay` with an invalid answer, or with more answers than asks, is an error.
#[test]
fn req_net_003_replay_errors() {
    let r = ChoiceSession::replay(
        base(1),
        three_asks(),
        &[ChoiceResponse::Yes, num(9), sel(&[1])],
    );
    assert!(r.is_err());
    let r = ChoiceSession::replay(base(1), three_asks(), &[num(1)]);
    assert!(r.is_err());
    let mut too_long = three_answers(true, 1, &[1]);
    too_long.push(ChoiceResponse::Yes);
    let r = ChoiceSession::replay(base(1), three_asks(), &too_long);
    assert!(r.is_err());
    let r = ChoiceSession::replay(base(1), k_asks_game(0), &[ChoiceResponse::Yes]);
    assert!(r.is_err());
    let r = ChoiceSession::replay(Toy::default(), k_asks_toy(1), &[num(1), num(1)]);
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------------------------
// (i) Divergence; (j) independence; (k) random; (l) procedure errors.
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, REQ-CORE-001: a procedure whose request content changes between runs (a test
/// counter in a `Cell`) makes `submit` fail with `Divergence`, leaving the session unchanged.
#[test]
fn req_net_003_divergence_detected() {
    let runs = Cell::new(0u64);
    let mut s = ChoiceSession::start(base(2), |_s: &mut GameState, ctx| {
        runs.set(runs.get() + 1);
        let mut r = yes_no();
        r.reason.cr = format!("test.reason_run_{}", runs.get());
        ask_in(ctx, ChoiceKind::YesNo, r)
    })
    .expect("start");
    let before = observe(&s);
    let id = s.pending().expect("pending").request_id;
    let got = s.submit(id, ChoiceResponse::Yes);
    assert!(
        matches!(got, Err(ChoiceError::Divergence { .. })),
        "{got:?}"
    );
    assert_eq!(observe(&s), before);
}

/// REQ-NET-003, REQ-CORE-001: divergence at a later ask (candidates change between runs once
/// the first answer is known) is also detected.
#[test]
fn req_net_003_divergence_at_second_ask() {
    let runs = Cell::new(0u64);
    let mut s = ChoiceSession::start(Toy::default(), |_s: &mut Toy, ctx| {
        runs.set(runs.get() + 1);
        ask_in(ctx, ChoiceKind::YesNo, yes_no())?;
        let n = 2 + runs.get();
        ask_in(
            ctx,
            ChoiceKind::Number,
            resolved(ResolvedCandidates::None, 0, n),
        )
    })
    .expect("start");
    s.submit(RequestId::new(0), ChoiceResponse::Yes)
        .expect("first ask is stable");
    let before = observe(&s);
    let got = s.submit(RequestId::new(1), num(0));
    assert!(
        matches!(got, Err(ChoiceError::Divergence { .. })),
        "{got:?}"
    );
    assert_eq!(observe(&s), before);
}

/// REQ-NET-003: two sessions on the same base advance independently.
#[test]
fn req_net_003_sessions_independent() {
    let b = base(4);
    let mut x = ChoiceSession::start(b.clone(), three_asks()).expect("start");
    let mut y = ChoiceSession::start(b.clone(), three_asks()).expect("start");
    x.submit(RequestId::new(0), ChoiceResponse::Yes)
        .expect("valid");
    assert_eq!(y.answers().len(), 0);
    assert_eq!(y.pending().expect("pending").request_id, RequestId::new(0));
    y.submit(RequestId::new(0), ChoiceResponse::No)
        .expect("valid");
    submit_all(&mut x, &[num(1), sel(&[2])]);
    assert_eq!(y.answers(), &[ChoiceResponse::No]);
    submit_all(&mut y, &[num(1), sel(&[2])]);
    assert_eq!(x.state_hash(), hand_three(&b, true, 1, &[2]).0.state_hash());
    assert_eq!(
        y.state_hash(),
        hand_three(&b, false, 1, &[2]).0.state_hash()
    );
}

/// A procedure whose choice is random: it takes the PRNG's pick without any ask.
fn random_pick() -> impl Fn(&mut GameState, &mut ChoiceContext<'_>) -> Result<u64, Interrupt> {
    |s, ctx| {
        let mut sp = spec(ChoiceKind::Player);
        sp.random = Some(true);
        let id = ctx.next_request_id();
        match generate_request(id, &sp, resolved(players(&[1, 2]), 1, 1))? {
            Generated::Random => {
                let i = s.prng_mut().next_bounded(2).expect("bound");
                lose(s, i + 1, 5);
                Ok(i + 1)
            }
            Generated::Ask(_) => Err(ChoiceError::Procedure("expected Random".to_owned()).into()),
        }
    }
}

/// REQ-NET-003, REQ-CORE-001: a `random` spec is chosen by the state's PRNG with no ask;
/// the same seed gives the same result.
#[test]
fn req_core_001_random_choice_deterministic() {
    for seed in 0..16u64 {
        let a = ChoiceSession::start(base(seed), random_pick()).expect("start");
        let b = ChoiceSession::start(base(seed), random_pick()).expect("start");
        assert!(is_done(&a));
        assert!(a.answers().is_empty());
        assert_eq!(a.state_hash(), b.state_hash());
        let mut hand = base(seed);
        let i = hand.prng_mut().next_bounded(2).expect("bound");
        lose(&mut hand, i + 1, 5);
        assert_eq!(a.state_hash(), hand.state_hash());
        let Status::Done { output, .. } = a.status() else {
            unreachable!()
        };
        assert_eq!(*output, i + 1);
    }
}

/// REQ-NET-003: a procedure error fails `start`; an error that depends on the answer fails
/// `submit` and leaves the session unchanged, and another answer proceeds.
#[test]
fn req_net_003_procedure_error() {
    let r = ChoiceSession::start(
        base(1),
        |_s: &mut GameState, _ctx| -> Result<(), Interrupt> {
            Err(ChoiceError::Procedure("test.failure".to_owned()).into())
        },
    );
    assert!(r.is_err());

    let mut s = ChoiceSession::start(base(1), |s: &mut GameState, ctx| {
        lose(s, 1, 2);
        let a = ask_in(ctx, ChoiceKind::YesNo, yes_no())?;
        if a == ChoiceResponse::Yes {
            return Err(ChoiceError::Procedure("test.failure".to_owned()).into());
        }
        Ok(())
    })
    .expect("start");
    let before = observe(&s);
    assert!(s.submit(RequestId::new(0), ChoiceResponse::Yes).is_err());
    assert_eq!(observe(&s), before);
    s.submit(RequestId::new(0), ChoiceResponse::No)
        .expect("valid");
    assert!(is_done(&s));
    let mut hand = base(1);
    lose(&mut hand, 1, 2);
    assert_eq!(s.state_hash(), hand.state_hash());
}

/// REQ-NET-003: `ctx.ask` rejects a request whose id is not `ctx.next_request_id()`, so the
/// procedure (and `start`) fails.
#[test]
fn req_net_003_ask_with_wrong_id_fails() {
    let r = ChoiceSession::start(Toy::default(), |_s: &mut Toy, ctx| {
        let wrong = RequestId::new(ctx.next_request_id().get() + 5);
        match generate_request(wrong, &spec(ChoiceKind::YesNo), yes_no())? {
            Generated::Ask(req) => ctx.ask(req),
            Generated::Random => Err(ChoiceError::Procedure("random".to_owned()).into()),
        }
    });
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------------------------
// Ask-time request well-formedness (REQ-NET-003).
// ---------------------------------------------------------------------------------------------

/// A procedure that asks `req` as its only ask.
fn ask_only(
    req: PendingChoiceRequest,
) -> impl Fn(&mut GameState, &mut ChoiceContext<'_>) -> Result<ChoiceResponse, Interrupt> {
    move |_s, ctx| ctx.ask(req.clone())
}

/// A NUMBER request with `19_999` values (a validator contract), id 0.
fn big_number_request() -> PendingChoiceRequest {
    let req = ask_kind(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 0, 19_998),
    );
    assert!(is_validator(&req));
    req
}

/// REQ-NET-003: `ask` rejects a request whose default does not validate: `start` returns
/// `InvalidDefault` with an explanation.
#[test]
fn req_net_003_ask_rejects_invalid_default() {
    let mut req = ask_kind(ChoiceKind::Player, resolved(players(&[1, 2]), 1, 1));
    req.default_on_timeout = ChoiceResponse::Yes;
    match ChoiceSession::start(base(1), ask_only(req)) {
        Err(ChoiceError::InvalidDefault(e)) => assert!(!e.trim().is_empty()),
        Err(other) => panic!("expected InvalidDefault, got {other:?}"),
        Ok(_) => panic!("expected InvalidDefault, got a session"),
    }
}

/// REQ-NET-003: a validator request whose `max` (or `min`) was changed without its contract is
/// rejected with `Procedure`; changing both together is accepted and pending.
#[test]
fn req_net_003_ask_rejects_stale_contract_bounds() {
    let stale = |edit: &dyn Fn(&mut PendingChoiceRequest)| {
        let mut req = big_number_request();
        edit(&mut req);
        ChoiceSession::start(base(1), ask_only(req))
    };
    match stale(&|r| r.max = 9_999) {
        Err(ChoiceError::Procedure(m)) => assert!(!m.trim().is_empty()),
        Err(other) => panic!("expected Procedure, got {other:?}"),
        Ok(_) => panic!("stale max accepted"),
    }
    match stale(&|r| {
        r.min = 5;
        r.default_on_timeout = num(5);
    }) {
        Err(ChoiceError::Procedure(m)) => assert!(!m.trim().is_empty()),
        Err(other) => panic!("expected Procedure, got {other:?}"),
        Ok(_) => panic!("stale min accepted"),
    }
    let s = stale(&|r| {
        r.max = 9_999;
        if let ChoiceOptions::Validator(c) = &mut r.options {
            c.max = 9_999;
        }
    })
    .expect("consistent bounds start");
    assert!(s.pending().is_some());
    let s = stale(&|r| {
        r.min = 5;
        r.default_on_timeout = num(5);
        if let ChoiceOptions::Validator(c) = &mut r.options {
            c.min = 5;
        }
    })
    .expect("consistent bounds start");
    assert!(s.pending().is_some());
}

/// REQ-NET-003: a second ask with an invalid default makes `submit` of the first answer return
/// the error and leave status, answers and state hash unchanged.
#[test]
fn req_net_003_second_ask_invalid_default_leaves_session_unchanged() {
    let mut s = ChoiceSession::start(base(1), |s: &mut GameState, ctx| {
        lose(s, 1, 1);
        ask_in(ctx, ChoiceKind::YesNo, yes_no())?;
        let mut req = ask_kind(ChoiceKind::Player, resolved(players(&[1, 2]), 1, 1));
        req.request_id = ctx.next_request_id();
        req.default_on_timeout = ChoiceResponse::Yes;
        ctx.ask(req)?;
        Ok(())
    })
    .expect("first ask is well-formed");
    let before = observe(&s);
    match s.submit(RequestId::new(0), ChoiceResponse::Yes) {
        Err(ChoiceError::InvalidDefault(e)) => assert!(!e.trim().is_empty()),
        other => panic!("expected InvalidDefault, got {other:?}"),
    }
    assert_eq!(observe(&s), before);
    assert!(!is_done(&s));
    assert!(s.answers().is_empty());
}

/// REQ-NET-003: same for a stale validator contract on the second ask (`Procedure`).
#[test]
fn req_net_003_second_ask_stale_contract_leaves_session_unchanged() {
    let mut s = ChoiceSession::start(base(1), |_s: &mut GameState, ctx| {
        ask_in(ctx, ChoiceKind::YesNo, yes_no())?;
        let mut req = big_number_request();
        req.request_id = ctx.next_request_id();
        req.max = 9_999;
        ctx.ask(req)?;
        Ok(())
    })
    .expect("first ask is well-formed");
    let before = observe(&s);
    match s.submit(RequestId::new(0), ChoiceResponse::No) {
        Err(ChoiceError::Procedure(m)) => assert!(!m.trim().is_empty()),
        other => panic!("expected Procedure, got {other:?}"),
    }
    assert_eq!(observe(&s), before);
}
