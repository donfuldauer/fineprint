// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the game start: REQ-FMT-001 (starting life, hand size, mulligan rule),
// REQ-NET-003 (MULLIGAN_DECISION, PREGAME_ACTION), CR 103.1, CR 103.3, CR 103.4, CR 103.5,
// CR 103.5c, CR 103.6, with R3 for deterministic replay. A fake host over a real `GameState`
// with synthetic object ids only (no card data, R5). Expected values are computed from an
// identically seeded PRNG inside the tests, never from the code under test.

use std::collections::BTreeMap;

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;

use super::*;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoicePayload, ChoiceResponse, ChoiceSession, ChoiceType,
    Interrupt, PendingChoiceRequest, Status,
};
use crate::format::{FormatConfig, PRESET_IDS, preset};
use crate::state::GameState;
use crate::txn::Transactional;

const SEED: u64 = 0x5eed_0048;
const LIB_SIZE: u64 = 20;

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

#[derive(Clone, Debug)]
struct Host {
    game: GameState,
    format: FormatConfig,
    libs: BTreeMap<PlayerId, Vec<ObjectId>>,
    hands: BTreeMap<PlayerId, Vec<ObjectId>>,
    sizes: BTreeMap<PlayerId, u32>,
    eligible: BTreeMap<PlayerId, Vec<ObjectId>>,
    performed: Vec<(PlayerId, Vec<ObjectId>)>,
    starting: Option<PlayerId>,
    /// Host calls that matter for ordering: "draw", "pregame".
    log: Vec<(&'static str, PlayerId)>,
}

fn base_format(life: i64, hand: u32, free_first: bool) -> FormatConfig {
    let mut f = preset(PRESET_IDS[0]).unwrap();
    f.starting_life = life;
    f.starting_hand_size = hand;
    f.mulligan.free_first_mulligan_multiplayer = free_first;
    f
}

/// The library of player index `i` before the start: ids `100*(i+1)+1 ..= 100*(i+1)+20`.
fn initial_lib(i: usize) -> Vec<ObjectId> {
    let base = 100 * (u64::try_from(i).unwrap() + 1);
    (1..=LIB_SIZE).map(|n| o(base + n)).collect()
}

impl Host {
    fn new(players: usize, format: FormatConfig) -> Host {
        let game = GameState::new(SEED, players, 0).unwrap();
        let order = game.turn_order().to_vec();
        let libs = order
            .iter()
            .enumerate()
            .map(|(i, p)| (*p, initial_lib(i)))
            .collect();
        Host {
            game,
            format,
            libs,
            hands: BTreeMap::new(),
            sizes: BTreeMap::new(),
            eligible: BTreeMap::new(),
            performed: Vec::new(),
            starting: None,
            log: Vec::new(),
        }
    }

    fn p(&self, i: usize) -> PlayerId {
        self.game.turn_order()[i]
    }
}

impl Transactional for Host {
    fn state_hash(&self) -> Digest {
        sha256(format!("{self:?}").as_bytes())
    }
    fn priority_player(&self) -> Option<PlayerId> {
        None
    }
    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

impl GameStartHost for Host {
    fn game(&self) -> &GameState {
        &self.game
    }
    fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
    }
    fn format(&self) -> &FormatConfig {
        &self.format
    }
    fn starting_hand_size(&self, player: PlayerId) -> u32 {
        self.sizes
            .get(&player)
            .copied()
            .unwrap_or(self.format.starting_hand_size)
    }
    fn library(&self, player: PlayerId) -> Vec<ObjectId> {
        self.libs.get(&player).cloned().unwrap_or_default()
    }
    fn set_library(&mut self, player: PlayerId, order: Vec<ObjectId>) -> Result<(), Interrupt> {
        self.libs.insert(player, order);
        Ok(())
    }
    fn hand(&self, player: PlayerId) -> Vec<ObjectId> {
        self.hands.get(&player).cloned().unwrap_or_default()
    }
    fn draw_cards(
        &mut self,
        player: PlayerId,
        count: u32,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(("draw", player));
        let lib = self.libs.entry(player).or_default();
        let n = usize::try_from(count).unwrap().min(lib.len());
        let drawn: Vec<ObjectId> = lib.drain(..n).collect();
        let hand = self.hands.entry(player).or_default();
        hand.extend(drawn);
        hand.sort_unstable();
        Ok(())
    }
    fn return_hand_to_library(&mut self, player: PlayerId) -> Result<(), Interrupt> {
        let hand = self.hands.remove(&player).unwrap_or_default();
        self.libs.entry(player).or_default().extend(hand);
        Ok(())
    }
    fn put_on_library_bottom(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
    ) -> Result<(), Interrupt> {
        let hand = self.hands.entry(player).or_default();
        hand.retain(|c| !cards.contains(c));
        self.libs
            .entry(player)
            .or_default()
            .extend_from_slice(cards);
        Ok(())
    }
    fn set_starting_player(&mut self, player: PlayerId) -> Result<(), Interrupt> {
        self.starting = Some(player);
        Ok(())
    }
    fn set_player_life(&mut self, player: PlayerId, life: i64) -> Result<(), Interrupt> {
        self.game.player_mut(player).unwrap().set_life(life);
        Ok(())
    }
    fn pregame_eligible(&self, player: PlayerId) -> Vec<ObjectId> {
        self.eligible.get(&player).cloned().unwrap_or_default()
    }
    fn perform_pregame_actions(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(("pregame", player));
        self.performed.push((player, cards.to_vec()));
        // A pregame action puts the card onto the battlefield: it leaves the hand.
        self.hands
            .entry(player)
            .or_default()
            .retain(|c| !cards.contains(c));
        Ok(())
    }
}

type Script<'a> = &'a dyn Fn(&PendingChoiceRequest) -> ChoiceResponse;

struct Run {
    asked: Vec<PendingChoiceRequest>,
    answers: Vec<ChoiceResponse>,
    host: Host,
    report: StartReport,
    hash: Digest,
}

fn procedure(
    opts: StartOptions,
) -> impl Fn(&mut Host, &mut ChoiceContext<'_>) -> Result<StartReport, Interrupt> {
    move |h, c| run_game_start(h, &opts, c)
}

/// Drives the start to the end, answering each request with `script`.
fn drive(host: Host, opts: &StartOptions, script: Script<'_>) -> Run {
    let mut session = ChoiceSession::start(host, procedure(opts.clone())).unwrap();
    let mut asked = Vec::new();
    loop {
        let req = match session.status() {
            Status::Pending(r) => r.clone(),
            Status::Done { state, output } => {
                return Run {
                    asked,
                    answers: session.answers().to_vec(),
                    host: state.clone(),
                    report: output.clone(),
                    hash: session.state_hash(),
                };
            }
        };
        let answer = script(&req);
        session.submit(req.request_id, answer).unwrap();
        asked.push(req);
    }
}

fn keep(req: &PendingChoiceRequest) -> ChoiceResponse {
    req.default_on_timeout.clone()
}

/// A PRNG seeded as the state's.
fn rng() -> Pcg64Dxsm {
    Pcg64Dxsm::from_u64_seed(SEED)
}

fn shuffled(r: &mut Pcg64Dxsm, mut v: Vec<ObjectId>) -> Vec<ObjectId> {
    r.shuffle(&mut v);
    v
}

fn sorted(mut v: Vec<ObjectId>) -> Vec<ObjectId> {
    v.sort_unstable();
    v
}

#[test]
fn req_fmt_001_cr_103_1_starting_player_from_option_uses_no_random_draw() {
    // REQ-FMT-001, CR 103.1, CR 103.3: with the option set the first PRNG call is a shuffle.
    let host = Host::new(3, base_format(20, 7, false));
    let chosen = host.p(2);
    let opts = StartOptions {
        starting_player: Some(chosen),
    };
    let run = drive(host.clone(), &opts, &keep);
    assert_eq!(run.report.starting_player, chosen);
    assert_eq!(run.host.starting, Some(chosen));
    let mut r = rng();
    let lib0 = shuffled(&mut r, initial_lib(0));
    let lib1 = shuffled(&mut r, initial_lib(1));
    let lib2 = shuffled(&mut r, initial_lib(2));
    let (h0, h1, h2) = (
        sorted(lib0[..7].to_vec()),
        sorted(lib1[..7].to_vec()),
        sorted(lib2[..7].to_vec()),
    );
    assert_eq!(run.report.opening_hands[0].1, h0);
    assert_eq!(run.report.opening_hands[1].1, h1);
    assert_eq!(run.report.opening_hands[2].1, h2);
}

#[test]
fn req_fmt_001_cr_103_1_starting_player_drawn_with_one_bounded_call() {
    // REQ-FMT-001, CR 103.1: the drawn player is turn_order[next_bounded(n)] of the seeded PRNG,
    // and exactly one call is made before the shuffles.
    let host = Host::new(4, base_format(20, 7, false));
    let mut r = rng();
    let i = usize::try_from(r.next_bounded(4).unwrap()).unwrap();
    let expect = host.p(i);
    let lib0 = shuffled(&mut r, initial_lib(0));
    let run = drive(host, &StartOptions::default(), &keep);
    assert_eq!(run.report.starting_player, expect);
    assert_eq!(run.host.starting, Some(expect));
    assert_eq!(run.report.opening_hands[0].1, sorted(lib0[..7].to_vec()));
}

#[test]
fn req_fmt_001_cr_103_1_unknown_starting_player_is_an_error() {
    // REQ-FMT-001, CR 103.1: a player outside the game is a procedure error.
    let host = Host::new(2, base_format(20, 7, false));
    let opts = StartOptions {
        starting_player: Some(PlayerId::new(99)),
    };
    let err = ChoiceSession::start(host, procedure(opts)).err().unwrap();
    assert!(matches!(err, ChoiceError::Procedure(_)));
}

#[test]
fn cr_103_3_libraries_are_shuffled_in_stored_turn_order() {
    // REQ-FMT-001, CR 103.3: one shuffle per player in stored order, after the starting-player draw; the
    // starting player is not stored-first here.
    let host = Host::new(3, base_format(20, 2, false));
    let opts = StartOptions {
        starting_player: Some(host.p(1)),
    };
    let run = drive(host, &opts, &keep);
    let mut r = rng();
    for i in 0..3 {
        let lib = shuffled(&mut r, initial_lib(i));
        let p = run.host.p(i);
        assert_eq!(run.host.libs[&p], lib[2..].to_vec(), "library {i}");
        assert_eq!(run.host.hands[&p], sorted(lib[..2].to_vec()));
    }
}

#[test]
fn req_fmt_001_cr_103_4_cr_103_5_life_and_hand_size_come_from_the_format() {
    // REQ-FMT-001, CR 103.4, CR 103.5: life 40 and hand 5, with a per-player override of 3.
    let mut host = Host::new(2, base_format(40, 5, false));
    let b = host.p(1);
    host.sizes.insert(b, 3);
    let run = drive(host, &StartOptions::default(), &keep);
    for p in run.host.game.turn_order().to_vec() {
        assert_eq!(run.host.game.player(p).unwrap().life(), 40);
    }
    assert_eq!(run.host.hands[&run.host.p(0)].len(), 5);
    assert_eq!(run.host.hands[&b].len(), 3);
    let host = Host::new(2, base_format(20, 7, false));
    let run = drive(host, &StartOptions::default(), &keep);
    assert_eq!(run.host.game.player(run.host.p(0)).unwrap().life(), 20);
    assert_eq!(run.host.hands[&run.host.p(0)].len(), 7);
}

#[test]
fn cr_103_5_no_mulligan_keeps_the_top_cards() {
    // REQ-FMT-001, REQ-NET-003, CR 103.5: all keep; to_bottom 0, KEEP_HAND{[]}; hand = top 7 of the shuffled library.
    let host = Host::new(2, base_format(20, 7, false));
    let run = drive(host, &StartOptions::default(), &keep);
    assert_eq!(run.asked.len(), 2);
    for req in &run.asked {
        assert!(req.hidden);
        assert_eq!(req.reveal_to, vec![req.chooser]);
        assert_eq!(req.reason.cr, "103.5");
        let ChoicePayload::MulliganDecision {
            mulligans_taken,
            to_bottom,
            hand,
        } = &req.payload
        else {
            panic!("payload");
        };
        assert_eq!((*mulligans_taken, *to_bottom, hand.len()), (0, 0, 7));
        assert_eq!(
            req.default_on_timeout,
            ChoiceResponse::KeepHand { bottom: vec![] }
        );
    }
    assert!(run.report.mulligans.iter().all(|(_, n)| *n == 0));
}

/// Expected result of the two-player mulligan script: the player at stored index 0 takes
/// `takes` mulligans then keeps, bottoming the first `takes` cards of the hand in descending
/// order; the other keeps at once. Returns (hand, library) of player 0.
fn model_two_players(takes: usize) -> (Vec<ObjectId>, Vec<ObjectId>, Vec<ObjectId>, Vec<ObjectId>) {
    let mut r = rng();
    let _ = r.next_bounded(2).unwrap();
    let mut lib = shuffled(&mut r, initial_lib(0));
    let lib_b = shuffled(&mut r, initial_lib(1));
    let hand_b = sorted(lib_b[..7].to_vec());
    let mut hand = sorted(lib[..7].to_vec());
    lib.drain(..7);
    for _ in 0..takes {
        lib.extend(hand.iter().copied());
        lib = shuffled(&mut r, lib);
        hand = sorted(lib[..7].to_vec());
        lib.drain(..7);
    }
    let mut bottom: Vec<ObjectId> = hand[..takes].to_vec();
    bottom.reverse();
    hand.drain(..takes);
    lib.extend(bottom);
    (hand, lib, hand_b, lib_b[7..].to_vec())
}

fn mulligan_script(
    taker: PlayerId,
    takes: u64,
) -> impl Fn(&PendingChoiceRequest) -> ChoiceResponse {
    move |req| {
        let ChoicePayload::MulliganDecision {
            hand,
            mulligans_taken,
            to_bottom,
        } = &req.payload
        else {
            return keep(req);
        };
        if req.chooser == taker && *mulligans_taken < takes {
            return ChoiceResponse::TakeMulligan;
        }
        if req.chooser == taker {
            let n = usize::try_from(*to_bottom).unwrap();
            let mut bottom = hand[..n].to_vec();
            bottom.reverse();
            return ChoiceResponse::KeepHand { bottom };
        }
        keep(req)
    }
}

#[test]
fn cr_103_5_one_and_two_mulligans_give_exact_hands_and_library_order() {
    // REQ-FMT-001, REQ-NET-003, CR 103.5: mulligan shuffles the hand back, draws a new hand, bottoms one card per
    // mulligan taken, in the order given (the last listed is bottom-most).
    for takes in 1..=2usize {
        let host = Host::new(2, base_format(20, 7, false));
        // Fix the starting player so the order of RNG calls is independent of the draw: the
        // model draws 0..2 first, so use the drawn player (whoever it is) as is.
        let a = host.p(0);
        let b = host.p(1);
        let script = mulligan_script(a, takes as u64);
        let run = drive(host, &StartOptions::default(), &script);
        let (hand_a, lib_a, hand_b, lib_b) = model_two_players(takes);
        assert_eq!(run.host.hands[&a], hand_a, "takes {takes}");
        assert_eq!(run.host.libs[&a], lib_a, "takes {takes}");
        assert_eq!(run.host.hands[&b], hand_b);
        assert_eq!(run.host.libs[&b], lib_b);
        assert_eq!(run.report.mulligans, vec![(a, takes as u64), (b, 0)]);
        assert_eq!(run.report.opening_hands[0].1, hand_a);
        assert_eq!(hand_a.len(), 7 - takes);
        let asks: Vec<_> = run
            .asked
            .iter()
            .filter(|q| q.chooser == a)
            .map(|q| match &q.payload {
                ChoicePayload::MulliganDecision {
                    mulligans_taken,
                    to_bottom,
                    ..
                } => (*mulligans_taken, *to_bottom),
                _ => panic!("payload"),
            })
            .collect();
        let expect: Vec<(u64, u64)> = (0..=takes as u64).map(|t| (t, t)).collect();
        assert_eq!(asks, expect);
    }
}

#[test]
fn cr_103_5_declarations_start_with_the_starting_player_in_turn_order() {
    // REQ-NET-003, CR 103.5: the starting player declares first, then the others in turn order; the starting
    // player is not stored-first. All declarations of a round come before any mulligan.
    let host = Host::new(3, base_format(20, 7, false));
    let (p0, p1, p2) = (host.p(0), host.p(1), host.p(2));
    let opts = StartOptions {
        starting_player: Some(p1),
    };
    // Round 1: p1 takes; round 2: everyone left keeps.
    let script = mulligan_script(p1, 1);
    let run = drive(host, &opts, &script);
    let choosers: Vec<PlayerId> = run.asked.iter().map(|q| q.chooser).collect();
    assert_eq!(choosers, vec![p1, p2, p0, p1]);
    // Opening draws happen in declaration order too.
    assert_eq!(
        run.host.log.iter().map(|(_, p)| *p).collect::<Vec<_>>(),
        vec![p1, p2, p0, p1]
    );
    // Report is in stored order.
    assert_eq!(run.report.mulligans, vec![(p0, 0), (p1, 1), (p2, 0)]);
}

#[test]
fn cr_103_5c_mulligan_to_bottom_table() {
    // REQ-FMT-001, CR 103.5c: the free first mulligan only in multiplayer and only with the flag.
    assert_eq!(mulligan_to_bottom(0, 3, true), 0);
    assert_eq!(mulligan_to_bottom(1, 3, true), 0);
    assert_eq!(mulligan_to_bottom(2, 3, true), 1);
    assert_eq!(mulligan_to_bottom(5, 4, true), 4);
    assert_eq!(mulligan_to_bottom(1, 3, false), 1);
    assert_eq!(mulligan_to_bottom(2, 2, true), 2);
    assert_eq!(mulligan_to_bottom(1, 2, true), 1);
    assert_eq!(mulligan_to_bottom(0, 2, false), 0);
}

fn to_bottoms_for(run: &Run, who: PlayerId) -> Vec<u64> {
    run.asked
        .iter()
        .filter(|q| q.chooser == who)
        .map(|q| match &q.payload {
            ChoicePayload::MulliganDecision { to_bottom, .. } => *to_bottom,
            _ => panic!("payload"),
        })
        .collect()
}

#[test]
fn cr_103_5c_free_first_mulligan_in_multiplayer() {
    // REQ-FMT-001, CR 103.5c: 3 players with the flag: after one mulligan 0 to bottom, after two 1.
    let host = Host::new(3, base_format(20, 7, true));
    let p0 = host.p(0);
    let script = mulligan_script(p0, 2);
    let run = drive(host, &StartOptions::default(), &script);
    assert_eq!(to_bottoms_for(&run, p0), vec![0, 0, 1]);
    assert_eq!(run.host.hands[&p0].len(), 6);
    // Flag unset: the normal count.
    let host = Host::new(3, base_format(20, 7, false));
    let p0 = host.p(0);
    let script = mulligan_script(p0, 2);
    let run = drive(host, &StartOptions::default(), &script);
    assert_eq!(to_bottoms_for(&run, p0), vec![0, 1, 2]);
    // Two players with the flag set: the normal count.
    let host = Host::new(2, base_format(20, 7, true));
    let p0 = host.p(0);
    let script = mulligan_script(p0, 2);
    let run = drive(host, &StartOptions::default(), &script);
    assert_eq!(to_bottoms_for(&run, p0), vec![0, 1, 2]);
}

#[test]
fn req_fmt_001_req_net_003_cr_103_5_last_mulligan_asks_for_the_bottom_order() {
    // REQ-FMT-001, REQ-NET-003, CR 103.5 (F86): when the counted mulligans reach the hand size
    // no further mulligan is offered; the player is asked once for the order in which the whole
    // hand goes to the bottom ("in any order"); the timeout default is ascending ids.
    let mut host = Host::new(2, base_format(20, 2, false));
    let a = host.p(0);
    host.sizes.insert(a, 2);
    let script = mulligan_script(a, 2);
    let run = drive(host, &StartOptions::default(), &script);
    // Asks for a: taken 0 (c 0), 1 (c 1), then the forced bottom of the whole hand (2).
    assert_eq!(to_bottoms_for(&run, a), vec![0, 1, 2]);
    let last = run.asked.iter().rfind(|q| q.chooser == a).unwrap();
    assert_eq!((last.min, last.max), (2, 2));
    let ChoicePayload::MulliganDecision { hand, .. } = &last.payload else {
        panic!("payload");
    };
    // The reversed answer of the script reaches the bottom in that order.
    let mut reversed = hand.clone();
    reversed.reverse();
    assert_ne!(reversed, *hand);
    assert_eq!(
        last.default_on_timeout,
        ChoiceResponse::KeepHand {
            bottom: hand.clone()
        }
    );
    assert_eq!(run.report.mulligans[0], (a, 2));
    assert!(run.report.opening_hands[0].1.is_empty());
    assert!(run.host.hands[&a].is_empty());
    let lib = &run.host.libs[&a];
    assert_eq!(lib.len(), 20);
    assert_eq!(lib[18..].to_vec(), reversed);
    // The timeout default keeps the ascending order.
    let mut host = Host::new(2, base_format(20, 2, false));
    let a = host.p(0);
    host.sizes.insert(a, 2);
    let take_twice = |req: &PendingChoiceRequest| match &req.payload {
        ChoicePayload::MulliganDecision {
            mulligans_taken, ..
        } if req.chooser == a && *mulligans_taken < 2 => ChoiceResponse::TakeMulligan,
        _ => keep(req),
    };
    let run = drive(host, &StartOptions::default(), &take_twice);
    let tail = run.host.libs[&a][18..].to_vec();
    assert_eq!(tail, sorted(tail.clone()));
}

#[test]
fn req_net_003_r12_take_mulligan_is_rejected_on_the_last_mulligan() {
    // REQ-NET-003, REQ-FMT-001, CR 103.5, R12: at the last mulligan TAKE_MULLIGAN is a procedure
    // error (the payload has no can-mulligan flag).
    let mut host = Host::new(2, base_format(20, 2, false));
    let a = host.p(0);
    host.sizes.insert(a, 2);
    let mut session = ChoiceSession::start(host, procedure(StartOptions::default())).unwrap();
    for _ in 0..8 {
        let req = session.pending().unwrap().clone();
        let ChoicePayload::MulliganDecision {
            mulligans_taken,
            to_bottom,
            ..
        } = &req.payload
        else {
            panic!("payload");
        };
        if req.chooser == a && *to_bottom == 2 {
            assert_eq!(*mulligans_taken, 2);
            let result = session.submit(req.request_id, ChoiceResponse::TakeMulligan);
            assert!(result.is_err());
            return;
        }
        let answer = if req.chooser == a {
            ChoiceResponse::TakeMulligan
        } else {
            keep(&req)
        };
        session.submit(req.request_id, answer).unwrap();
    }
    panic!("the last-mulligan request was never issued");
}

#[test]
fn req_net_003_cr_103_5_cr_103_6_opening_hands_are_read_before_pregame_actions() {
    // REQ-NET-003, REQ-FMT-001, CR 103.5, CR 103.6 (F85): the report lists the hand kept after
    // the last mulligan even when a pregame action then moves a card out of the hand.
    let mut host = Host::new(2, base_format(20, 2, false));
    let a = host.p(0);
    let opts = StartOptions {
        starting_player: Some(a),
    };
    let probe = drive(host.clone(), &opts, &keep);
    let kept = probe.host.hands[&a].clone();
    assert_eq!(kept.len(), 2);
    host.eligible.insert(a, vec![kept[0]]);
    let script = |req: &PendingChoiceRequest| match &req.payload {
        ChoicePayload::PregameAction { eligible } => ChoiceResponse::SubmitPregame {
            objects: eligible.clone(),
        },
        _ => keep(req),
    };
    let run = drive(host, &opts, &script);
    assert_eq!(run.host.performed, vec![(a, vec![kept[0]])]);
    assert_eq!(run.report.opening_hands[0], (a, kept.clone()));
    assert!(
        !run.host.hands[&a].contains(&kept[0]),
        "the host state reflects the move"
    );
}

#[test]
fn req_net_003_defaults_validate_and_bad_answers_are_rejected() {
    // REQ-NET-003, R12: the defaults validate (ask checks), a wrong-type answer and a bad bottom
    // are rejected by the session without a panic, and the request stays pending.
    let host = Host::new(2, base_format(20, 7, false));
    let a = host.p(0);
    let mut session = ChoiceSession::start(host, procedure(StartOptions::default())).unwrap();
    let first = session.pending().unwrap().clone();
    assert_eq!(first.choice_type, ChoiceType::MulliganDecision);
    let wrong = ChoiceResponse::SubmitPregame { objects: vec![] };
    assert!(session.submit(first.request_id, wrong).is_err());
    let bad_count = ChoiceResponse::KeepHand {
        bottom: vec![o(101)],
    };
    assert!(session.submit(first.request_id, bad_count).is_err());
    assert_eq!(session.pending().unwrap().request_id, first.request_id);
    // Take a mulligan: now one card must go to the bottom.
    session
        .submit(first.request_id, ChoiceResponse::TakeMulligan)
        .unwrap();
    let mut second = None;
    // The same player (or the other) is asked again; find a's request for taken 1.
    for _ in 0..6 {
        let req = session.pending().unwrap().clone();
        let ChoicePayload::MulliganDecision {
            hand,
            mulligans_taken,
            ..
        } = &req.payload
        else {
            panic!("payload")
        };
        if req.chooser == a && *mulligans_taken == 1 {
            // Take a second mulligan: then two cards must go to the bottom.
            session
                .submit(req.request_id, ChoiceResponse::TakeMulligan)
                .unwrap();
            continue;
        }
        if req.chooser == a && *mulligans_taken == 2 {
            second = Some((req.clone(), hand.clone()));
            break;
        }
        session.submit_default(req.request_id).unwrap();
    }
    let (req, hand) = second.expect("a is asked again");
    let outside = ChoiceResponse::KeepHand {
        bottom: vec![o(9999)],
    };
    assert!(session.submit(req.request_id, outside).is_err());
    // The request needs two cards: the duplicate pair has the right count, so the duplicate is
    // the only reason for the rejection.
    assert_eq!((req.min, req.max), (2, 2));
    let dup = ChoiceResponse::KeepHand {
        bottom: vec![hand[0], hand[0]],
    };
    assert!(session.submit(req.request_id, dup).is_err());
    let good = ChoiceResponse::KeepHand {
        bottom: vec![hand[0], hand[1]],
    };
    session.submit(req.request_id, good).unwrap();
}

#[test]
fn req_net_003_cr_103_6_pregame_actions_follow_all_mulligans_one_ask_per_player() {
    // REQ-NET-003, CR 103.6: players without eligible cards are not asked; the others are asked
    // once, starting player first, after every mulligan; the answer order is the order taken.
    let mut host = Host::new(3, base_format(20, 3, false));
    let (p0, p1, p2) = (host.p(0), host.p(1), host.p(2));
    host.eligible.insert(p0, vec![o(1)]);
    host.eligible.insert(p2, vec![o(2), o(3)]);
    let opts = StartOptions {
        starting_player: Some(p2),
    };
    let script = |req: &PendingChoiceRequest| match &req.payload {
        ChoicePayload::PregameAction { eligible } if req.chooser == p2 => {
            ChoiceResponse::SubmitPregame {
                objects: eligible.iter().rev().copied().collect(),
            }
        }
        ChoicePayload::MulliganDecision { .. } if req.chooser == p1 => {
            // p1 mulligans once, so the mulligan of a later round precedes every pregame ask.
            match &req.payload {
                ChoicePayload::MulliganDecision {
                    mulligans_taken, ..
                } if *mulligans_taken == 0 => ChoiceResponse::TakeMulligan,
                _ => keep(req),
            }
        }
        _ => keep(req),
    };
    let run = drive(host, &opts, &script);
    let kinds: Vec<(ChoiceType, PlayerId)> = run
        .asked
        .iter()
        .map(|q| (q.choice_type, q.chooser))
        .collect();
    let pregame: Vec<(ChoiceType, PlayerId)> = kinds
        .iter()
        .copied()
        .filter(|(t, _)| *t == ChoiceType::PregameAction)
        .collect();
    assert_eq!(
        pregame,
        vec![
            (ChoiceType::PregameAction, p2),
            (ChoiceType::PregameAction, p0)
        ]
    );
    let first_pregame = kinds
        .iter()
        .position(|(t, _)| *t == ChoiceType::PregameAction)
        .unwrap();
    assert!(
        kinds[..first_pregame]
            .iter()
            .all(|(t, _)| *t == ChoiceType::MulliganDecision)
    );
    assert!(
        kinds[first_pregame..]
            .iter()
            .all(|(t, _)| *t == ChoiceType::PregameAction)
    );
    // p0 answered with the default (empty): nothing performed; p2's order is as given.
    assert_eq!(run.host.performed, vec![(p2, vec![o(3), o(2)])]);
    let req = run
        .asked
        .iter()
        .find(|q| q.choice_type == ChoiceType::PregameAction)
        .unwrap();
    assert_eq!((req.min, req.max, req.hidden), (0, 2, false));
    assert_eq!(req.reason.cr, "103.6");
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitPregame { objects: vec![] }
    );
}

#[test]
fn req_net_003_r3_replay_gives_the_same_state_and_report() {
    // REQ-NET-003, R3: replaying the recorded answers on the base gives the same hash.
    let mut host = Host::new(3, base_format(20, 7, true));
    let p0 = host.p(0);
    host.eligible.insert(p0, vec![o(101)]);
    let script = mulligan_script(p0, 2);
    let run = drive(host.clone(), &StartOptions::default(), &script);
    let replayed =
        ChoiceSession::replay(host, procedure(StartOptions::default()), &run.answers).unwrap();
    assert_eq!(replayed.state_hash(), run.hash);
    match replayed.status() {
        Status::Done { output, state } => {
            assert_eq!(*output, run.report);
            assert_eq!(state.libs, run.host.libs);
        }
        Status::Pending(r) => panic!("replay should finish, pending {r:?}"),
    }
}
