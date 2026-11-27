// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the game-start spec tests (REQ-FMT-001, REQ-NET-003, REQ-CORE-001):
//! a mock `GameStartHost`, a driver over `ChoiceSession`, and an independent reference model
//! of the specified call order (CR 103.1, 103.3, 103.4, 103.5, 103.5c, 103.6). Black-box:
//! written from the work-item brief only. All data is synthetic (R5).

#![allow(dead_code)]

use std::collections::BTreeMap;

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::choice::*;
use fineprint_core::format::{FormatConfig, preset};
use fineprint_core::state::GameState;
use fineprint_core::turn::{GameStartHost, StartOptions, StartReport, run_game_start};
use fineprint_core::txn::Transactional;

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

pub fn oids(ns: &[u64]) -> Vec<ObjectId> {
    ns.iter().copied().map(oid).collect()
}

/// Player `p`'s `i`-th original card (1-based `i`): `p * 1000 + i`.
pub fn card(p: u64, i: u64) -> ObjectId {
    oid(p * 1000 + i)
}

/// A format with the given values (everything else from a preset).
pub fn format(life: i64, hand: u32, free_first: bool) -> FormatConfig {
    let mut f = preset("standard").expect("preset");
    f.starting_life = life;
    f.starting_hand_size = hand;
    f.mulligan.free_first_mulligan_multiplayer = free_first;
    f
}

/// The mock host: real `GameState` plus per-player vectors (no zones).
#[derive(Clone, Debug)]
pub struct Mock {
    pub game: GameState,
    pub format: FormatConfig,
    /// Libraries, top first.
    pub libs: BTreeMap<PlayerId, Vec<ObjectId>>,
    /// Hands, ascending ids.
    pub hands: BTreeMap<PlayerId, Vec<ObjectId>>,
    pub eligible: BTreeMap<PlayerId, Vec<ObjectId>>,
    pub hand_size: BTreeMap<PlayerId, u32>,
    pub starting: Option<PlayerId>,
    pub performed: Vec<(PlayerId, Vec<ObjectId>)>,
    pub log: Vec<String>,
}

impl Mock {
    /// `n` players, each with `lib` cards ([`card`] ids, ascending, top first).
    pub fn new(seed: u64, n: usize, lib: u64, life: i64, hand: u32, free_first: bool) -> Mock {
        let game = GameState::new(seed, n, 20).expect("game");
        let mut libs = BTreeMap::new();
        let mut hands = BTreeMap::new();
        for p in 1..=n as u64 {
            libs.insert(pid(p), (1..=lib).map(|i| card(p, i)).collect());
            hands.insert(pid(p), Vec::new());
        }
        Mock {
            game,
            format: format(life, hand, free_first),
            libs,
            hands,
            eligible: BTreeMap::new(),
            hand_size: BTreeMap::new(),
            starting: None,
            performed: Vec::new(),
            log: Vec::new(),
        }
    }

    pub fn life(&self, p: u64) -> i64 {
        self.game.player(pid(p)).expect("player").life()
    }

    pub fn lib(&self, p: u64) -> Vec<ObjectId> {
        self.libs[&pid(p)].clone()
    }

    pub fn hand_of(&self, p: u64) -> Vec<ObjectId> {
        self.hands[&pid(p)].clone()
    }

    pub fn raw(&self) -> (u128, u128) {
        self.game.prng().raw()
    }

    /// Index of the first log entry starting with `prefix`.
    pub fn first_log(&self, prefix: &str) -> Option<usize> {
        self.log.iter().position(|l| l.starts_with(prefix))
    }
}

fn perr(msg: &str) -> Interrupt {
    ChoiceError::Procedure(msg.to_owned()).into()
}

impl GameStartHost for Mock {
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
        self.hand_size
            .get(&player)
            .copied()
            .unwrap_or(self.format.starting_hand_size)
    }
    fn library(&self, player: PlayerId) -> Vec<ObjectId> {
        self.libs[&player].clone()
    }
    fn set_library(&mut self, player: PlayerId, order: Vec<ObjectId>) -> Result<(), Interrupt> {
        let mut a = self.libs[&player].clone();
        let mut b = order.clone();
        a.sort();
        b.sort();
        if a != b {
            return Err(perr("set_library: not a permutation"));
        }
        self.log.push(format!("setlib:{}", player.get()));
        self.libs.insert(player, order);
        Ok(())
    }
    fn hand(&self, player: PlayerId) -> Vec<ObjectId> {
        self.hands[&player].clone()
    }
    fn draw_cards(
        &mut self,
        player: PlayerId,
        count: u32,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(format!("draw:{}:{count}", player.get()));
        let lib = self.libs.get_mut(&player).expect("lib");
        let k = (count as usize).min(lib.len());
        let drawn: Vec<ObjectId> = lib.drain(..k).collect();
        let hand = self.hands.get_mut(&player).expect("hand");
        hand.extend(drawn);
        hand.sort();
        Ok(())
    }
    fn return_hand_to_library(&mut self, player: PlayerId) -> Result<(), Interrupt> {
        self.log.push(format!("return:{}", player.get()));
        let hand = std::mem::take(self.hands.get_mut(&player).expect("hand"));
        self.libs.get_mut(&player).expect("lib").extend(hand);
        Ok(())
    }
    fn put_on_library_bottom(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
    ) -> Result<(), Interrupt> {
        self.log.push(format!(
            "bottom:{}:{:?}",
            player.get(),
            cards.iter().map(|c| c.get()).collect::<Vec<_>>()
        ));
        for c in cards {
            let hand = self.hands.get_mut(&player).expect("hand");
            let Some(i) = hand.iter().position(|h| h == c) else {
                return Err(perr("put_on_library_bottom: card not in hand"));
            };
            hand.remove(i);
            self.libs.get_mut(&player).expect("lib").push(*c);
        }
        Ok(())
    }
    fn set_starting_player(&mut self, player: PlayerId) -> Result<(), Interrupt> {
        self.log.push(format!("start:{}", player.get()));
        self.starting = Some(player);
        Ok(())
    }
    fn set_player_life(&mut self, player: PlayerId, life: i64) -> Result<(), Interrupt> {
        self.log.push(format!("life:{}:{life}", player.get()));
        self.game
            .player_mut(player)
            .ok_or_else(|| perr("no such player"))?
            .set_life(life);
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
        self.log.push(format!(
            "perform:{}:{:?}",
            player.get(),
            cards.iter().map(|c| c.get()).collect::<Vec<_>>()
        ));
        self.performed.push((player, cards.to_vec()));
        Ok(())
    }
}

impl Transactional for Mock {
    fn state_hash(&self) -> Digest {
        sha256(format!("{self:?}").as_bytes())
    }
    fn priority_player(&self) -> Option<PlayerId> {
        None
    }
    fn carry_durable(restored: Mock, _before: &Mock, _after: &Mock) -> Mock {
        restored
    }
}

/// How answers are chosen, by player and mulligans taken so far (deterministic).
#[derive(Clone, Debug, Default)]
pub struct Policy {
    /// `(player, mulligans_taken) -> take a mulligan`; takes precedence.
    pub scripted: BTreeMap<(u64, u64), bool>,
    /// Pseudo-random decisions from this seed when not scripted (never beyond 5 taken).
    pub hashed: Option<u64>,
}

fn mix(seed: u64, a: u64, b: u64) -> u64 {
    let mut r = Pcg64Dxsm::from_u64_seed(
        seed ^ a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
    );
    r.next_u64()
}

impl Policy {
    pub fn keep_all() -> Policy {
        Policy::default()
    }
    pub fn script(entries: &[(u64, u64, bool)]) -> Policy {
        Policy {
            scripted: entries.iter().map(|&(p, t, b)| ((p, t), b)).collect(),
            hashed: None,
        }
    }
    pub fn random(seed: u64) -> Policy {
        Policy {
            scripted: BTreeMap::new(),
            hashed: Some(seed),
        }
    }
    pub fn take(&self, player: u64, taken: u64) -> bool {
        if let Some(b) = self.scripted.get(&(player, taken)) {
            return *b;
        }
        match self.hashed {
            Some(s) => taken < 5 && mix(s, player, taken).is_multiple_of(2),
            None => false,
        }
    }
    /// The bottom cards for a kept hand: the last `c` cards of the hand, last first.
    pub fn bottom(hand: &[ObjectId], c: u64) -> Vec<ObjectId> {
        hand.iter().rev().take(c as usize).copied().collect()
    }
    /// The pregame answer: nothing unless hashed; then a pseudo-random reversed prefix.
    pub fn pregame(&self, player: u64, eligible: &[ObjectId]) -> Vec<ObjectId> {
        match self.hashed {
            Some(s) => {
                let k = (mix(s ^ 0xABCD, player, 99) % (eligible.len() as u64 + 1)) as usize;
                eligible.iter().rev().take(k).copied().collect()
            }
            None => Vec::new(),
        }
    }
    /// The response to `req`.
    pub fn answer(&self, req: &PendingChoiceRequest) -> ChoiceResponse {
        match &req.payload {
            ChoicePayload::MulliganDecision {
                hand,
                mulligans_taken,
                to_bottom,
            } => {
                if self.take(req.chooser.get(), *mulligans_taken) {
                    ChoiceResponse::TakeMulligan
                } else {
                    ChoiceResponse::KeepHand {
                        bottom: Policy::bottom(hand, *to_bottom),
                    }
                }
            }
            ChoicePayload::PregameAction { eligible } => ChoiceResponse::SubmitPregame {
                objects: self.pregame(req.chooser.get(), eligible),
            },
            other => panic!("unexpected request {other:?}"),
        }
    }
}

/// What a finished run produced.
#[derive(Clone, Debug)]
pub struct Run {
    pub host: Mock,
    pub report: StartReport,
    pub answers: Vec<ChoiceResponse>,
    pub requests: Vec<PendingChoiceRequest>,
}

/// Starts a session over `mock` with `opts`.
#[allow(clippy::type_complexity)]
pub fn start_session(
    mock: Mock,
    opts: StartOptions,
) -> Result<
    ChoiceSession<
        Mock,
        StartReport,
        impl Fn(&mut Mock, &mut ChoiceContext<'_>) -> Result<StartReport, Interrupt>,
    >,
    ChoiceError,
> {
    ChoiceSession::start(mock, move |h: &mut Mock, ctx: &mut ChoiceContext<'_>| {
        run_game_start(h, &opts, ctx)
    })
}

/// Drives a run to completion, answering from `policy`; records every request.
pub fn drive(mock: Mock, opts: StartOptions, policy: &Policy) -> Run {
    let mut s = start_session(mock, opts).expect("start");
    let mut requests = Vec::new();
    while let Some(req) = s.pending().cloned() {
        let resp = policy.answer(&req);
        s.submit(req.request_id, resp).expect("valid answer");
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

/// Convenience: everyone keeps, no starting-player option.
pub fn run_keep(mock: Mock) -> Run {
    drive(mock, StartOptions::default(), &Policy::keep_all())
}

pub fn prng(seed: u64) -> Pcg64Dxsm {
    Pcg64Dxsm::from_u64_seed(seed)
}

/// Independent shuffle of a top-first vector.
pub fn shuf(r: &mut Pcg64Dxsm, v: &[ObjectId]) -> Vec<ObjectId> {
    let mut v = v.to_vec();
    r.shuffle(&mut v);
    v
}

/// The expected `mulligan_to_bottom` per CR 103.5c.
pub fn expect_bottom(taken: u64, n: usize, free: bool) -> u64 {
    if free && n > 2 && taken >= 1 {
        taken - 1
    } else {
        taken
    }
}

/// The reference model's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub starting: PlayerId,
    pub libs: BTreeMap<PlayerId, Vec<ObjectId>>,
    pub hands: BTreeMap<PlayerId, Vec<ObjectId>>,
    pub mulls: BTreeMap<PlayerId, u64>,
    pub raw: (u128, u128),
    pub performed: Vec<(PlayerId, Vec<ObjectId>)>,
    /// `(chooser, taken, to_bottom)` of every MULLIGAN_DECISION ask, in order.
    pub asks: Vec<(PlayerId, u64, u64)>,
}

/// Independent reference implementation of the specified procedure, run over a fresh copy of
/// the mock's initial data (`m` must be unrun).
pub fn model(m: &Mock, seed: u64, opts: &StartOptions, policy: &Policy) -> Model {
    let mut r = prng(seed);
    let order_stored: Vec<PlayerId> = m.game.turn_order().to_vec();
    let n = order_stored.len();
    let starting = match opts.starting_player {
        Some(p) => p,
        None => order_stored[r.next_bounded(n as u64).expect("n>0") as usize],
    };
    let mut libs = m.libs.clone();
    let mut hands: BTreeMap<PlayerId, Vec<ObjectId>> =
        order_stored.iter().map(|p| (*p, Vec::new())).collect();
    for p in &order_stored {
        let v = shuf(&mut r, &libs[p]);
        libs.insert(*p, v);
    }
    let i0 = order_stored
        .iter()
        .position(|p| *p == starting)
        .expect("start");
    let order: Vec<PlayerId> = (0..n).map(|k| order_stored[(i0 + k) % n]).collect();
    let size = |p: &PlayerId| {
        m.hand_size
            .get(p)
            .copied()
            .unwrap_or(m.format.starting_hand_size) as usize
    };
    let draw = |libs: &mut BTreeMap<PlayerId, Vec<ObjectId>>,
                hands: &mut BTreeMap<PlayerId, Vec<ObjectId>>,
                p: &PlayerId| {
        let lib = libs.get_mut(p).expect("lib");
        let k = size(p).min(lib.len());
        let d: Vec<ObjectId> = lib.drain(..k).collect();
        let h = hands.get_mut(p).expect("hand");
        h.extend(d);
        h.sort();
    };
    for p in &order {
        draw(&mut libs, &mut hands, p);
    }
    let mut taken: BTreeMap<PlayerId, u64> = order.iter().map(|p| (*p, 0)).collect();
    let mut deciding: Vec<PlayerId> = order.clone();
    let mut asks = Vec::new();
    while !deciding.is_empty() {
        let mut took = Vec::new();
        let mut still = Vec::new();
        for p in &deciding {
            let c = expect_bottom(
                taken[p],
                n,
                m.format.mulligan.free_first_mulligan_multiplayer,
            );
            if c as usize >= size(p) {
                let h = std::mem::take(hands.get_mut(p).expect("hand"));
                libs.get_mut(p).expect("lib").extend(h);
                continue;
            }
            asks.push((*p, taken[p], c));
            if policy.take(p.get(), taken[p]) {
                took.push(*p);
                still.push(*p);
            } else {
                let hand = hands[p].clone();
                for b in Policy::bottom(&hand, c) {
                    let h = hands.get_mut(p).expect("hand");
                    let i = h.iter().position(|x| *x == b).expect("in hand");
                    h.remove(i);
                    libs.get_mut(p).expect("lib").push(b);
                }
            }
        }
        for p in &order {
            if took.contains(p) {
                *taken.get_mut(p).expect("taken") += 1;
                let h = std::mem::take(hands.get_mut(p).expect("hand"));
                libs.get_mut(p).expect("lib").extend(h);
                let v = shuf(&mut r, &libs[p]);
                libs.insert(*p, v);
                draw(&mut libs, &mut hands, p);
            }
        }
        deciding = still;
    }
    let mut performed = Vec::new();
    for p in &order {
        let el = m.eligible.get(p).cloned().unwrap_or_default();
        if el.is_empty() {
            continue;
        }
        let objs = policy.pregame(p.get(), &el);
        if !objs.is_empty() {
            performed.push((*p, objs));
        }
    }
    Model {
        starting,
        libs,
        hands,
        mulls: taken.into_iter().collect::<BTreeMap<_, _>>(),
        raw: r.raw(),
        performed,
        asks,
    }
}

/// The `(chooser, taken, to_bottom)` of the MULLIGAN_DECISION requests in `run`.
pub fn mulligan_asks(run: &Run) -> Vec<(PlayerId, u64, u64)> {
    run.requests
        .iter()
        .filter_map(|r| match &r.payload {
            ChoicePayload::MulliganDecision {
                mulligans_taken,
                to_bottom,
                ..
            } => Some((r.chooser, *mulligans_taken, *to_bottom)),
            _ => None,
        })
        .collect()
}
