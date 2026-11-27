// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for game start: starting player, shuffles, opening hands, London mulligan and
//! pregame actions (FP-046; REQ-FMT-001, REQ-NET-003, REQ-CORE-001; CR 103.1, 103.3, 103.4,
//! 103.5, 103.5c, 103.6). Black-box: written from the work-item brief only. Every golden value
//! is derived from an identically seeded `Pcg64Dxsm` in these tests.

#[path = "spec_turn_start_support.rs"]
mod spec_turn_start_support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::*;
use fineprint_core::turn::{StartOptions, mulligan_to_bottom, run_game_start};
use spec_turn_start_support::*;

const LIB: u64 = 40;

fn opt(p: u64) -> StartOptions {
    StartOptions {
        starting_player: Some(pid(p)),
    }
}

fn sorted(v: &[ObjectId]) -> Vec<ObjectId> {
    let mut v = v.to_vec();
    v.sort();
    v
}

fn base(seed: u64, n: usize) -> Mock {
    Mock::new(seed, n, LIB, 20, 7, false)
}

/// Drives a run with an arbitrary answer function.
fn drive_with(
    mock: Mock,
    opts: StartOptions,
    f: &dyn Fn(&PendingChoiceRequest) -> ChoiceResponse,
) -> Run {
    let mut s = start_session(mock, opts).expect("start");
    let mut requests = Vec::new();
    while let Some(req) = s.pending().cloned() {
        s.submit(req.request_id, f(&req)).expect("valid answer");
        requests.push(req);
    }
    match s.status() {
        Status::Done { state, output } => Run {
            host: state.clone(),
            report: output.clone(),
            answers: s.answers().to_vec(),
            requests,
        },
        Status::Pending(_) => unreachable!(),
    }
}

fn same(a: &Mock, b: &Mock) -> bool {
    a.libs == b.libs
        && a.hands == b.hands
        && a.log == b.log
        && a.raw() == b.raw()
        && a.performed == b.performed
        && a.starting == b.starting
}

// ---------------------------------------------------------------------------------------------
// Starting player.
// ---------------------------------------------------------------------------------------------

/// REQ-FMT-001, CR 103.1: the option names the starting player and the PRNG is not advanced
/// for that step (the final PRNG position equals shuffles only).
#[test]
fn start_option_wins_and_prng_not_advanced() {
    for start in 1..=3u64 {
        let seed = 7;
        let run = drive(base(seed, 3), opt(start), &Policy::keep_all());
        assert_eq!(run.report.starting_player, pid(start));
        assert_eq!(run.host.starting, Some(pid(start)));
        // Independent: three shuffles, then 0 further draws (everyone keeps).
        let mut r = prng(seed);
        for p in 1..=3 {
            let lib: Vec<_> = (1..=LIB).map(|i| card(p, i)).collect();
            let _ = shuf(&mut r, &lib);
        }
        assert_eq!(run.host.raw(), r.raw(), "start {start}");
    }
}

/// REQ-FMT-001, CR 103.1: without the option the starting player is `turn_order[i]` for the
/// one `next_bounded(n)` draw; the first MULLIGAN_DECISION goes to that player (rotated
/// declaration order).
#[test]
fn drawn_start_matches_next_bounded_and_rotates_declaration_order() {
    let mut rotated_seen = 0;
    for n in 2..=4usize {
        for seed in 0..12u64 {
            let m = base(seed, n);
            let order = m.game.turn_order().to_vec();
            let i = prng(seed).next_bounded(n as u64).expect("bound") as usize;
            let run = run_keep(m);
            assert_eq!(run.report.starting_player, order[i], "n {n} seed {seed}");
            assert_eq!(run.requests[0].chooser, order[i]);
            let asked: Vec<PlayerId> = run.requests.iter().map(|r| r.chooser).collect();
            let expect: Vec<PlayerId> = (0..n).map(|k| order[(i + k) % n]).collect();
            assert_eq!(asked, expect);
            if i != 0 {
                rotated_seen += 1;
            }
        }
    }
    assert!(rotated_seen > 0, "no seed rotated the order");
}

/// REQ-FMT-001, CR 103.1: an unknown option player is an error, not a panic.
#[test]
fn unknown_start_player_is_an_error_not_a_panic() {
    let r = start_session(base(1, 2), opt(99));
    assert!(r.is_err());
    let r = start_session(base(1, 2), opt(0));
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------------------------
// Shuffles, life, hand sizes, keep-all.
// ---------------------------------------------------------------------------------------------

/// CR 103.3, CR 103.5: shuffles happen per player in stored order, after the single
/// starting-player draw; keep-all leaves each library as the shuffled order minus the opening
/// hand (the top 7), and the opening hand is the top 7, all mulligans 0, `to_bottom` 0.
#[test]
fn shuffles_in_stored_order_and_keep_all_opening_hands() {
    for seed in [1u64, 2, 3, 99] {
        let m = base(seed, 3);
        let mut r = prng(seed);
        let i = r.next_bounded(3).expect("bound");
        let starting = pid(i + 1);
        let shuffled: Vec<Vec<ObjectId>> = (1..=3u64)
            .map(|p| {
                let lib: Vec<_> = (1..=LIB).map(|k| card(p, k)).collect();
                shuf(&mut r, &lib)
            })
            .collect();
        let run = run_keep(m);
        assert_eq!(run.report.starting_player, starting);
        for p in 1..=3usize {
            assert_eq!(run.host.lib(p as u64), shuffled[p - 1][7..].to_vec());
            assert_eq!(run.host.hand_of(p as u64), sorted(&shuffled[p - 1][..7]));
        }
        assert_eq!(run.host.raw(), r.raw());
        assert!(run.requests.iter().all(|q| matches!(
            q.payload,
            ChoicePayload::MulliganDecision {
                mulligans_taken: 0,
                to_bottom: 0,
                ..
            }
        )));
        // Report in stored turn order, ascending ids.
        let want: Vec<(PlayerId, Vec<ObjectId>)> = (1..=3usize)
            .map(|p| (pid(p as u64), sorted(&shuffled[p - 1][..7])))
            .collect();
        assert_eq!(run.report.opening_hands, want);
        assert_eq!(
            run.report.mulligans,
            vec![(pid(1), 0), (pid(2), 0), (pid(3), 0)]
        );
    }
}

/// REQ-FMT-001, CR 103.4, CR 103.5: starting life 20 and 40, hand size 7 and 5, per-player
/// override.
#[test]
fn format_values_life_and_hand_size() {
    for (life, hand) in [(20, 7), (40, 5), (40, 7), (20, 5)] {
        let run = run_keep(Mock::new(5, 2, LIB, life, hand, false));
        assert_eq!(run.host.life(1), life);
        assert_eq!(run.host.life(2), life);
        assert!(run.host.log.contains(&format!("life:1:{life}")));
        assert!(run.host.log.contains(&format!("life:2:{life}")));
        assert_eq!(run.host.hand_of(1).len(), hand as usize);
        assert_eq!(run.host.hand_of(2).len(), hand as usize);
        assert_eq!(run.host.lib(1).len(), LIB as usize - hand as usize);
    }
    let mut m = Mock::new(5, 2, LIB, 20, 7, false);
    m.hand_size.insert(pid(2), 4);
    let run = run_keep(m);
    assert_eq!(run.host.hand_of(1).len(), 7);
    assert_eq!(run.host.hand_of(2).len(), 4);
    assert!(run.host.log.contains(&"draw:2:4".to_owned()));
    assert!(run.host.log.contains(&"draw:1:7".to_owned()));
}

// ---------------------------------------------------------------------------------------------
// Mulligan sequences for a fixed seed (acceptance criterion).
// ---------------------------------------------------------------------------------------------

const S: u64 = 4242;

fn lib_of(p: u64) -> Vec<ObjectId> {
    (1..=LIB).map(|i| card(p, i)).collect()
}

/// Splits a top-first library into (sorted top-7 hand, rest).
fn split7(v: &[ObjectId]) -> (Vec<ObjectId>, Vec<ObjectId>) {
    (sorted(&v[..7]), v[7..].to_vec())
}

/// Everything after a taken mulligan: return the hand, shuffle, draw 7.
fn redraw(
    r: &mut fineprint_base::prng::Pcg64Dxsm,
    rest: &[ObjectId],
    hand: &[ObjectId],
) -> (Vec<ObjectId>, Vec<ObjectId>) {
    let mut lib = rest.to_vec();
    lib.extend_from_slice(hand);
    let lib = shuf(r, &lib);
    split7(&lib)
}

/// Keeps `hand` bottoming `bottom` (given order, last bottom-most).
fn keep(
    hand: &[ObjectId],
    rest: &[ObjectId],
    bottom: &[ObjectId],
) -> (Vec<ObjectId>, Vec<ObjectId>) {
    let h: Vec<_> = hand
        .iter()
        .copied()
        .filter(|c| !bottom.contains(c))
        .collect();
    let mut l = rest.to_vec();
    l.extend_from_slice(bottom);
    (h, l)
}

/// REQ-FMT-001, CR 103.5: (a) the starting player takes one mulligan, then keeps bottoming one
/// card; exact hands, library order, requests and final PRNG position.
#[test]
fn mulligan_sequence_a_one_mulligan() {
    let mut r = prng(S);
    let l1 = shuf(&mut r, &lib_of(1));
    let l2 = shuf(&mut r, &lib_of(2));
    let (h1, rest1) = split7(&l1);
    let (h2, rest2) = split7(&l2);
    // Round 1: p1 takes; p2 keeps (c 0). Then p1: return, shuffle, draw.
    let (h1b, rest1b) = redraw(&mut r, &rest1, &h1);
    // Round 2: p1 (taken 1, c 1) keeps, bottoming its highest card.
    let bottom = vec![*h1b.last().expect("card")];
    let (fh1, fl1) = keep(&h1b, &rest1b, &bottom);

    let policy = Policy::script(&[(1, 0, true)]);
    let run = drive(base(S, 2), opt(1), &policy);
    assert_eq!(run.host.hand_of(1), fh1);
    assert_eq!(run.host.lib(1), fl1);
    assert_eq!(run.host.hand_of(2), h2);
    assert_eq!(run.host.lib(2), rest2);
    assert_eq!(run.host.raw(), r.raw());
    assert_eq!(run.report.mulligans, vec![(pid(1), 1), (pid(2), 0)]);
    assert_eq!(
        mulligan_asks(&run),
        vec![(pid(1), 0, 0), (pid(2), 0, 0), (pid(1), 1, 1)]
    );
    assert_eq!(fh1.len(), 6);
    assert_eq!(run.report.opening_hands[0], (pid(1), fh1));
}

/// REQ-FMT-001, CR 103.5: (b) two mulligans in a row; two cards go to the bottom in the order
/// given, the last listed bottom-most.
#[test]
fn mulligan_sequence_b_two_in_a_row() {
    let mut r = prng(S);
    let l1 = shuf(&mut r, &lib_of(1));
    let l2 = shuf(&mut r, &lib_of(2));
    let (h1, rest1) = split7(&l1);
    let (h2, rest2) = split7(&l2);
    let (h1b, rest1b) = redraw(&mut r, &rest1, &h1);
    let (h1c, rest1c) = redraw(&mut r, &rest1b, &h1b);
    // Policy bottoms the last two cards of the hand, highest first.
    let n = h1c.len();
    let bottom = vec![h1c[n - 1], h1c[n - 2]];
    let (fh1, fl1) = keep(&h1c, &rest1c, &bottom);

    let policy = Policy::script(&[(1, 0, true), (1, 1, true)]);
    let run = drive(base(S, 2), opt(1), &policy);
    assert_eq!(run.host.hand_of(1), fh1);
    assert_eq!(run.host.lib(1), fl1);
    assert_eq!(*run.host.lib(1).last().expect("card"), h1c[n - 2]);
    assert_eq!(run.host.hand_of(2), h2);
    assert_eq!(run.host.lib(2), rest2);
    assert_eq!(run.host.raw(), r.raw());
    assert_eq!(run.report.mulligans, vec![(pid(1), 2), (pid(2), 0)]);
    assert_eq!(
        mulligan_asks(&run),
        vec![
            (pid(1), 0, 0),
            (pid(2), 0, 0),
            (pid(1), 1, 1),
            (pid(1), 2, 2)
        ]
    );
    assert_eq!(fh1.len(), 5);
}

/// REQ-FMT-001, CR 103.5: (c) both players take a mulligan in the same round (player 2
/// starts): the mulligans resolve in declaration order, shuffles interleaved with draws.
#[test]
fn mulligan_sequence_c_both_in_same_round() {
    let mut r = prng(S);
    let l1 = shuf(&mut r, &lib_of(1));
    let l2 = shuf(&mut r, &lib_of(2));
    let (h1, rest1) = split7(&l1);
    let (h2, rest2) = split7(&l2);
    // Round 1 asks: p2 then p1, both take. Resolution in order: p2 then p1.
    let (h2b, rest2b) = redraw(&mut r, &rest2, &h2);
    let (h1b, rest1b) = redraw(&mut r, &rest1, &h1);
    // Round 2: both keep with c 1.
    let b2 = vec![*h2b.last().expect("card")];
    let b1 = vec![*h1b.last().expect("card")];
    let (fh2, fl2) = keep(&h2b, &rest2b, &b2);
    let (fh1, fl1) = keep(&h1b, &rest1b, &b1);

    let policy = Policy::script(&[(1, 0, true), (2, 0, true)]);
    let run = drive(base(S, 2), opt(2), &policy);
    assert_eq!(run.host.hand_of(1), fh1);
    assert_eq!(run.host.lib(1), fl1);
    assert_eq!(run.host.hand_of(2), fh2);
    assert_eq!(run.host.lib(2), fl2);
    assert_eq!(run.host.raw(), r.raw());
    assert_eq!(run.report.mulligans, vec![(pid(1), 1), (pid(2), 1)]);
    assert_eq!(
        mulligan_asks(&run),
        vec![
            (pid(2), 0, 0),
            (pid(1), 0, 0),
            (pid(2), 1, 1),
            (pid(1), 1, 1)
        ]
    );
    let ret2 = run.host.first_log("return:2").expect("return 2");
    let ret1 = run.host.first_log("return:1").expect("return 1");
    assert!(ret2 < ret1);
}

// ---------------------------------------------------------------------------------------------
// Multiplayer free first mulligan, zero-hand limit.
// ---------------------------------------------------------------------------------------------

/// CR 103.5c: the free first mulligan in a 3-player game with the flag on.
#[test]
fn free_first_mulligan_three_players_flag_on() {
    let policy = Policy::script(&[(1, 0, true), (1, 1, true)]);
    let run = drive(Mock::new(9, 3, LIB, 40, 7, true), opt(1), &policy);
    let p1: Vec<_> = mulligan_asks(&run)
        .into_iter()
        .filter(|a| a.0 == pid(1))
        .collect();
    assert_eq!(p1, vec![(pid(1), 0, 0), (pid(1), 1, 0), (pid(1), 2, 1)]);
    // After the first (free) mulligan a keep would bottom nothing: hand of 7.
    let run1 = drive(
        Mock::new(9, 3, LIB, 40, 7, true),
        opt(1),
        &Policy::script(&[(1, 0, true)]),
    );
    assert_eq!(run1.host.hand_of(1).len(), 7);
    assert_eq!(run1.report.mulligans[0], (pid(1), 1));
    assert_eq!(run.host.hand_of(1).len(), 6);
    assert_eq!(run.report.mulligans[0], (pid(1), 2));
}

/// CR 103.5c: flag off in 3 players, and 2 players with the flag on, count normally.
#[test]
fn free_first_mulligan_off_or_two_players_counts_normally() {
    let policy = Policy::script(&[(1, 0, true), (1, 1, true)]);
    for (n, flag) in [(3usize, false), (2, true), (2, false)] {
        let run = drive(Mock::new(9, n, LIB, 20, 7, flag), opt(1), &policy);
        let p1: Vec<_> = mulligan_asks(&run)
            .into_iter()
            .filter(|a| a.0 == pid(1))
            .collect();
        assert_eq!(
            p1,
            vec![(pid(1), 0, 0), (pid(1), 1, 1), (pid(1), 2, 2)],
            "n {n} flag {flag}"
        );
        assert_eq!(run.host.hand_of(1).len(), 5);
    }
}

/// CR 103.5c: the `mulligan_to_bottom` table.
#[test]
fn mulligan_to_bottom_table() {
    for taken in 0..=4u64 {
        for n in 2..=4usize {
            for free in [false, true] {
                assert_eq!(
                    mulligan_to_bottom(taken, n, free),
                    expect_bottom(taken, n, free),
                    "taken {taken} n {n} free {free}"
                );
            }
        }
    }
    assert_eq!(mulligan_to_bottom(1, 3, true), 0);
    assert_eq!(mulligan_to_bottom(2, 4, true), 1);
    assert_eq!(mulligan_to_bottom(1, 2, true), 1);
    assert_eq!(mulligan_to_bottom(0, 3, true), 0);
    assert_eq!(mulligan_to_bottom(3, 3, false), 3);
}

/// CR 103.5: when the count to bottom reaches the hand size there is no ask, the whole hand
/// goes to the bottom (ascending ids), the opening hand is empty and the run completes.
#[test]
fn zero_hand_limit_stops_asking_and_empties_hand() {
    // Hand size 3 from the format, two players.
    let policy = Policy::script(&[(1, 0, true), (1, 1, true), (1, 2, true), (1, 3, true)]);
    let run = drive(Mock::new(11, 2, LIB, 20, 3, false), opt(1), &policy);
    let p1: Vec<_> = mulligan_asks(&run)
        .into_iter()
        .filter(|a| a.0 == pid(1))
        .collect();
    assert_eq!(p1, vec![(pid(1), 0, 0), (pid(1), 1, 1), (pid(1), 2, 2)]);
    assert_eq!(run.report.mulligans[0], (pid(1), 3));
    assert!(run.host.hand_of(1).is_empty());
    assert_eq!(run.report.opening_hands[0], (pid(1), vec![]));
    let lib = run.host.lib(1);
    assert_eq!(lib.len(), LIB as usize);
    let tail = lib[lib.len() - 3..].to_vec();
    assert_eq!(
        tail,
        sorted(&tail),
        "whole hand bottomed in ascending order"
    );
    assert_eq!(run.host.hand_of(2).len(), 3);

    // Per-player override with the free first mulligan: counted reaches 3 at 4 taken.
    let mut m = Mock::new(11, 3, LIB, 20, 7, true);
    m.hand_size.insert(pid(1), 3);
    let policy = Policy::script(&[(1, 0, true), (1, 1, true), (1, 2, true), (1, 3, true)]);
    let run = drive(m, opt(1), &policy);
    let p1: Vec<_> = mulligan_asks(&run)
        .into_iter()
        .filter(|a| a.0 == pid(1))
        .collect();
    assert_eq!(
        p1,
        vec![
            (pid(1), 0, 0),
            (pid(1), 1, 0),
            (pid(1), 2, 1),
            (pid(1), 3, 2)
        ]
    );
    // taken 4 is not asked as counted 3 >= 3; the 4th answer taken at taken==3 made it 4.
    assert_eq!(run.report.mulligans[0], (pid(1), 4));
    assert!(run.host.hand_of(1).is_empty());
    assert_eq!(run.host.hand_of(2).len(), 7);
}

// ---------------------------------------------------------------------------------------------
// Choice request shape and validation.
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, CR 103.5, R12: shape of MULLIGAN_DECISION requests, the default and bad
/// answers (wrong count, non-hand card, duplicate, wrong type), which change nothing.
#[test]
fn mulligan_request_shape_and_validation() {
    let mut s = start_session(base(S, 2), opt(1)).expect("start");
    let req = s.pending().cloned().expect("pending");
    assert_eq!(req.chooser, pid(1));
    assert_eq!(req.choice_type, ChoiceType::MulliganDecision);
    assert_eq!(req.reason.cr, "103.5");
    assert!(req.hidden);
    assert_eq!(req.reveal_to, vec![pid(1)]);
    let ChoicePayload::MulliganDecision {
        hand,
        mulligans_taken,
        to_bottom,
    } = req.payload.clone()
    else {
        panic!("payload");
    };
    assert_eq!((mulligans_taken, to_bottom), (0, 0));
    assert_eq!(hand.len(), 7);
    assert_eq!(hand, sorted(&hand));
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::KeepHand { bottom: vec![] }
    );
    assert_eq!(req.validate(&req.default_on_timeout), Ok(()));
    assert_eq!(req.validate(&ChoiceResponse::TakeMulligan), Ok(()));
    // c == 0: a non-empty bottom is the wrong count.
    assert!(
        req.validate(&ChoiceResponse::KeepHand {
            bottom: vec![hand[0]]
        })
        .is_err()
    );
    assert!(
        req.validate(&ChoiceResponse::SubmitPregame { objects: vec![] })
            .is_err()
    );
    // Take twice: the third request has to_bottom 2.
    s.submit(req.request_id, ChoiceResponse::TakeMulligan)
        .expect("take");
    let r2 = s.pending().cloned().expect("pending");
    assert_eq!(r2.chooser, pid(2));
    s.submit_default(r2.request_id).expect("default");
    let r3 = s.pending().cloned().expect("pending");
    assert_eq!(r3.chooser, pid(1));
    s.submit(r3.request_id, ChoiceResponse::TakeMulligan)
        .expect("take");
    let r4 = s.pending().cloned().expect("pending");
    let ChoicePayload::MulliganDecision {
        hand,
        mulligans_taken,
        to_bottom,
    } = r4.payload.clone()
    else {
        panic!("payload");
    };
    assert_eq!((mulligans_taken, to_bottom), (2, 2));
    assert_eq!(r4.reason.cr, "103.5");
    assert!(r4.hidden);
    assert_eq!(r4.reveal_to, vec![pid(1)]);
    assert_eq!(
        r4.default_on_timeout,
        ChoiceResponse::KeepHand {
            bottom: hand[..2].to_vec()
        }
    );
    assert_eq!(r4.validate(&r4.default_on_timeout), Ok(()));
    let outsider = oid(777_777);
    let good = vec![hand[3], hand[1]];
    let bad: Vec<ChoiceResponse> = vec![
        ChoiceResponse::KeepHand { bottom: vec![] },
        ChoiceResponse::KeepHand {
            bottom: vec![hand[0]],
        },
        ChoiceResponse::KeepHand {
            bottom: vec![hand[0], hand[1], hand[2]],
        },
        ChoiceResponse::KeepHand {
            bottom: vec![hand[0], outsider],
        },
        ChoiceResponse::KeepHand {
            bottom: vec![hand[0], hand[0]],
        },
        ChoiceResponse::SubmitPregame { objects: vec![] },
    ];
    let answers_before = s.answers().to_vec();
    for b in bad {
        assert!(r4.validate(&b).is_err(), "{b:?}");
        assert!(s.submit(r4.request_id, b).is_err());
        assert_eq!(s.pending(), Some(&r4));
        assert_eq!(s.answers(), answers_before.as_slice());
    }
    s.submit(
        r4.request_id,
        ChoiceResponse::KeepHand {
            bottom: good.clone(),
        },
    )
    .expect("good keep");
    assert!(s.pending().is_none());
    let Status::Done { state, .. } = s.status() else {
        panic!("done")
    };
    let lib = state.lib(1);
    assert_eq!(&lib[lib.len() - 2..], good.as_slice());
}

/// REQ-NET-003, CR 103.6: shape of PREGAME_ACTION requests and validation.
#[test]
fn pregame_request_shape_and_validation() {
    let mut m = base(3, 2);
    m.eligible.insert(pid(1), oids(&[5001, 5002, 5003]));
    let mut s = start_session(m, opt(1)).expect("start");
    // Two mulligan keeps first.
    for _ in 0..2 {
        let r = s.pending().cloned().expect("pending");
        assert_eq!(r.choice_type, ChoiceType::MulliganDecision);
        s.submit_default(r.request_id).expect("keep");
    }
    let req = s.pending().cloned().expect("pending");
    assert_eq!(req.choice_type, ChoiceType::PregameAction);
    assert_eq!(req.chooser, pid(1));
    assert_eq!(req.reason.cr, "103.6");
    assert_eq!((req.min, req.max), (0, 3));
    assert!(!req.hidden);
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitPregame { objects: vec![] }
    );
    assert_eq!(
        req.payload,
        ChoicePayload::PregameAction {
            eligible: oids(&[5001, 5002, 5003])
        }
    );
    assert_eq!(req.validate(&req.default_on_timeout), Ok(()));
    for ok in [
        vec![],
        oids(&[5003]),
        oids(&[5003, 5001]),
        oids(&[5001, 5002, 5003]),
    ] {
        assert_eq!(
            req.validate(&ChoiceResponse::SubmitPregame { objects: ok }),
            Ok(())
        );
    }
    let bad = [
        ChoiceResponse::SubmitPregame {
            objects: oids(&[9999]),
        },
        ChoiceResponse::SubmitPregame {
            objects: oids(&[5001, 5001]),
        },
        ChoiceResponse::KeepHand { bottom: vec![] },
        ChoiceResponse::TakeMulligan,
    ];
    for b in bad {
        assert!(req.validate(&b).is_err(), "{b:?}");
        assert!(s.submit(req.request_id, b).is_err());
        assert_eq!(s.pending(), Some(&req));
    }
}

// ---------------------------------------------------------------------------------------------
// Pregame ordering.
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, CR 103.6: nobody eligible means no PREGAME_ACTION request at all.
#[test]
fn pregame_nobody_eligible_no_request() {
    let run = run_keep(base(3, 3));
    assert!(
        run.requests
            .iter()
            .all(|r| r.choice_type == ChoiceType::MulliganDecision)
    );
    assert!(run.host.performed.is_empty());
}

/// REQ-NET-003, CR 103.6: pregame asks come in declaration order and only after every
/// mulligan finished; an empty answer performs nothing; a non-empty answer performs the
/// cards in the order given.
#[test]
fn pregame_order_after_all_mulligans_and_performed_in_given_order() {
    let mut m = base(21, 2);
    m.eligible.insert(pid(1), oids(&[5001, 5002]));
    m.eligible.insert(pid(2), oids(&[6001, 6002]));
    // Player 1 starts; player 2 takes one mulligan.
    let policy = Policy::script(&[(2, 0, true)]);
    let answer = |req: &PendingChoiceRequest| match &req.payload {
        ChoicePayload::PregameAction { .. } if req.chooser == pid(1) => {
            ChoiceResponse::SubmitPregame {
                objects: oids(&[5002, 5001]),
            }
        }
        ChoicePayload::PregameAction { .. } => ChoiceResponse::SubmitPregame { objects: vec![] },
        _ => policy.answer(req),
    };
    let run = drive_with(m, opt(1), &answer);
    let kinds: Vec<(ChoiceType, PlayerId)> = run
        .requests
        .iter()
        .map(|r| (r.choice_type, r.chooser))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (ChoiceType::MulliganDecision, pid(1)),
            (ChoiceType::MulliganDecision, pid(2)),
            (ChoiceType::MulliganDecision, pid(2)),
            (ChoiceType::PregameAction, pid(1)),
            (ChoiceType::PregameAction, pid(2)),
        ]
    );
    assert_eq!(run.host.performed, vec![(pid(1), oids(&[5002, 5001]))]);
    assert!(run.host.first_log("perform:2").is_none());
    let last_draw = run
        .host
        .log
        .iter()
        .rposition(|l| l.starts_with("draw:"))
        .expect("draw");
    let perform = run.host.first_log("perform:1").expect("perform");
    assert!(last_draw < perform);
}

// ---------------------------------------------------------------------------------------------
// Replay and properties.
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, R3: replaying the recorded answers gives identical hosts and report.
#[test]
fn replay_gives_identical_result() {
    let mut m = Mock::new(31, 3, LIB, 40, 7, true);
    m.eligible.insert(pid(2), oids(&[5001, 5002]));
    let policy = Policy::random(77);
    let run = drive(m.clone(), StartOptions::default(), &policy);
    assert!(!run.answers.is_empty());
    let s2 = ChoiceSession::replay(
        m.clone(),
        |h: &mut Mock, ctx: &mut ChoiceContext<'_>| {
            run_game_start(h, &StartOptions::default(), ctx)
        },
        &run.answers,
    )
    .expect("replay");
    let Status::Done { state, output } = s2.status() else {
        panic!("done")
    };
    assert!(same(state, &run.host));
    assert_eq!(*output, run.report);
    // And a fresh drive with the same policy is identical too.
    let again = drive(m, StartOptions::default(), &policy);
    assert!(same(&again.host, &run.host));
    assert_eq!(again.report, run.report);
    assert_eq!(again.answers, run.answers);
}

/// REQ-CORE-001, REQ-FMT-001, REQ-NET-003, CR 103.5: over many seeds and answer scripts each
/// run matches the reference model, conserves every player's cards, ends with
/// hand size = starting size minus counted mulligans, and is reproducible.
#[test]
fn property_random_runs_conserve_cards_and_match_model() {
    let mut meta = prng(2024);
    for case in 0..300u64 {
        let n = 2 + meta.next_bounded(3).expect("b") as usize;
        let hand = 1 + meta.next_bounded(7).expect("b") as u32;
        let free = meta.next_bounded(2).expect("b") == 1;
        let seed = meta.next_u64();
        let mut m = Mock::new(seed, n, 20, 20, hand, free);
        if meta.next_bounded(3).expect("b") == 0 {
            m.hand_size
                .insert(pid(1), 1 + meta.next_bounded(7).expect("b") as u32);
        }
        for p in 1..=n as u64 {
            if meta.next_bounded(2).expect("b") == 1 {
                m.eligible.insert(
                    pid(p),
                    oids(&[p * 10_000 + 1, p * 10_000 + 2, p * 10_000 + 3]),
                );
            }
        }
        let opts = if meta.next_bounded(2).expect("b") == 1 {
            opt(1 + meta.next_bounded(n as u64).expect("b"))
        } else {
            StartOptions::default()
        };
        let policy = Policy::random(meta.next_u64());
        let want = model(&m, seed, &opts, &policy);
        let run = drive(m.clone(), opts.clone(), &policy);
        let ctx = format!("case {case}");
        assert_eq!(run.report.starting_player, want.starting, "{ctx}");
        assert_eq!(run.host.libs, want.libs, "{ctx}");
        assert_eq!(run.host.hands, want.hands, "{ctx}");
        assert_eq!(run.host.raw(), want.raw, "{ctx}");
        assert_eq!(run.host.performed, want.performed, "{ctx}");
        assert_eq!(mulligan_asks(&run), want.asks, "{ctx}");
        for p in 1..=n as u64 {
            let id = pid(p);
            let mut all = run.host.lib(p);
            all.extend(run.host.hand_of(p));
            all.sort();
            let orig: Vec<_> = (1..=20).map(|i| card(p, i)).collect();
            assert_eq!(all, orig, "{ctx} player {p}");
            let size = m.hand_size.get(&id).copied().unwrap_or(hand) as u64;
            let taken = run
                .report
                .mulligans
                .iter()
                .find(|x| x.0 == id)
                .expect("p")
                .1;
            let counted = expect_bottom(taken, n, free);
            assert_eq!(
                run.host.hand_of(p).len() as u64,
                size.saturating_sub(counted),
                "{ctx} player {p}"
            );
        }
        let again = drive(m, opts, &policy);
        assert!(same(&again.host, &run.host), "{ctx}");
        assert_eq!(again.report, run.report, "{ctx}");
    }
}
