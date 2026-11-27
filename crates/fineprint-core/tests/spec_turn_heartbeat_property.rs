// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests of the priority and SBA heartbeat over seeded generated hosts (REQ-TURN-001,
//! REQ-CORE-001; CR 117.3a-d, 117.4, 117.5, 704.3, 603.3b, 800.4a, 800.4j). The generator is a
//! fixed-seed LCG (R3: no system randomness). Black-box: written from the work-item brief only.
//!
//! CR 117.4: "If all players pass in succession (that is, if all players pass without taking any
//! actions in between passing), the spell or ability on top of the stack resolves or, if the
//! stack is empty, the phase or step ends."

#[path = "spec_turn_heartbeat_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::ids::PlayerId;
use fineprint_core::choice::{ChoicePayload, ChoiceResponse, ChoiceType, PendingChoiceRequest};
use fineprint_core::turn::{HeartbeatHost, PassResult, players_in_game};
use support::*;

const SEEDS: u64 = 256;

/// Most actions the answer policy takes in one run (keeps every run finite).
const MAX_ACTIONS: usize = 6;

/// A generated host for `seed` (2 to 4 players; random active seat or none; a random set of
/// players who left, possibly empty or everyone; scripted SBA and trigger sequences that settle
/// within a bound; stack depth up to 4; random legal actions per player; random host requests
/// and departures in SBAs, resolutions and actions that always leave one player in the game;
/// and departures while a player decides priority: as the heartbeat collects the actions of the
/// player about to be asked, another player or that player leaves (F23)).
fn generate(seed: u64) -> FakeHost {
    let mut g = Lcg::new(seed);
    let n = 2 + g.below(3) as usize;
    let seats: Vec<PlayerId> = (1..=n as u64).map(PlayerId::new).collect();
    let active = if g.chance(1, 8) {
        None
    } else {
        Some(seats[g.below(n as u64) as usize])
    };
    let mut host = FakeHost::new(n, active);
    if g.chance(1, 4) {
        let mut order = seats.clone();
        for i in (1..order.len()).rev() {
            let j = g.below(i as u64 + 1) as usize;
            order.swap(i, j);
        }
        host.game.set_turn_order(order).expect("a permutation");
    }
    if !g.chance(1, 3) {
        for &p in &seats {
            if g.chance(1, 3) {
                host.mark_lost(p);
            }
        }
    }
    let in_game: Vec<PlayerId> = seats.iter().copied().filter(|p| !host.lost(*p)).collect();
    // One in-game player never leaves during the run; the others may.
    let survivor = if in_game.is_empty() {
        None
    } else {
        Some(in_game[g.below(in_game.len() as u64) as usize])
    };
    let leavers: Vec<PlayerId> = in_game
        .iter()
        .copied()
        .filter(|p| Some(*p) != survivor)
        .collect();
    let pick_leaver = |g: &mut Lcg| -> Option<Vec<PlayerId>> {
        if leavers.is_empty() {
            None
        } else {
            Some(vec![leavers[g.below(leavers.len() as u64) as usize]])
        }
    };

    for _ in 0..g.below(6) {
        let step = match g.below(6) {
            0 => SbaStep::Nothing,
            1 => SbaStep::FindingsOnly,
            2 => SbaStep::Performed,
            3 => match pick_leaver(&mut g) {
                Some(l) => SbaStep::Lose(l),
                None => SbaStep::Performed,
            },
            4 => SbaStep::Ask,
            _ => SbaStep::Nothing,
        };
        host.sba.push_back(step);
    }
    for _ in 0..g.below(5) {
        let step = if g.chance(1, 5) {
            PlaceStep::Ask
        } else {
            PlaceStep::Place {
                stacked: g.below(3) as usize,
                removed: g.below(2) as usize,
            }
        };
        host.place.push_back(step);
    }
    for _ in 0..g.below(3) {
        let step = match g.below(4) {
            0 => HookStep::Ask,
            1 => match pick_leaver(&mut g) {
                Some(l) => HookStep::Lose(l),
                None => HookStep::Plain,
            },
            _ => HookStep::Plain,
        };
        host.resolve_steps.push_back(step);
    }
    for _ in 0..g.below(3) {
        let step = match g.below(4) {
            0 => HookStep::Ask,
            1 => match pick_leaver(&mut g) {
                Some(l) => HookStep::Lose(l),
                None => HookStep::Plain,
            },
            _ => HookStep::Plain,
        };
        host.action_steps.push_back(step);
    }
    host.depth = g.below(5);
    let makers: [fn(u64) -> ChoiceResponse; 5] = [land, cast, activate, mana, special];
    for (i, &p) in seats.iter().enumerate() {
        let mut list = Vec::new();
        for (k, make) in makers.iter().enumerate() {
            if g.chance(1, 3) {
                list.push(make((i as u64 + 1) * 100 + k as u64));
            }
        }
        if !list.is_empty() {
            host.actions.insert(p, list);
        }
    }
    let phases = [
        (None, None),
        (
            Some(fineprint_core::events::Phase::Beginning),
            Some(fineprint_core::events::Step::Upkeep),
        ),
        (Some(fineprint_core::events::Phase::PostcombatMain), None),
    ];
    host.phase = phases[g.below(3) as usize];
    // F23: departures while a seat decides priority (never the survivor).
    if g.chance(1, 2) {
        let mut queue = std::collections::VecDeque::new();
        for _ in 0..g.below(8) {
            let entry = if g.chance(1, 3) {
                pick_leaver(&mut g).map(|l| l[0])
            } else {
                None
            };
            queue.push_back(entry);
        }
        *host.concede_on_ask.borrow_mut() = queue;
    }
    host
}

/// The players of `w` still in the game.
fn live_of(w: &World<FakeHost>) -> BTreeSet<PlayerId> {
    w.fake().live().into_iter().collect()
}

/// The seeded answer policy: a priority request is answered with a legal action (at most
/// [`MAX_ACTIONS`] per run) or a pass; a host request with its default.
fn policy(
    seed: u64,
) -> impl FnMut(&PendingChoiceRequest, &World<FakeHost>) -> Option<ChoiceResponse> {
    let mut g = Lcg::new(seed ^ 0x00A5_5A00);
    let mut acted = 0usize;
    move |req, _| {
        if let ChoicePayload::PriorityAction { actions, .. } = &req.payload {
            if acted < MAX_ACTIONS && !actions.is_empty() && g.chance(1, 3) {
                acted += 1;
                return Some(actions[g.below(actions.len() as u64) as usize].clone());
            }
            Some(ChoiceResponse::PassPriority)
        } else {
            Some(req.default_on_timeout.clone())
        }
    }
}

/// REQ-TURN-001, REQ-CORE-001, CR 117.4, CR 117.5, CR 704.3, CR 800.4a (issue acceptance: the
/// heartbeat terminates or asks for a choice on generated states). For every seed:
/// (a) driven to completion, `run_heartbeat` returns `StepEnds` (or `NoPlayerInGame` exactly
/// when nobody is in the game) and never errors, and every request is a PRIORITY_ACTION or a
/// host request; (b) every priority ask goes to an in-game player; (c) the asked player is the
/// game's priority player; (d) PassCount never exceeds the players in game and is 0 at the
/// first ask after every action and resolution; (e) resolutions = initial depth + pushes by
/// actions + stacked triggers, ending with an empty stack; (f) the records are consistent;
/// (g) replaying the answers reproduces the state hash and the final state; (h) with seats
/// leaving while another seat (or the seat itself) decides priority, the expected PassCount and
/// the all-passed threshold come from the players still in the game, the top resolves and the
/// step ends only after every one of them has passed in succession, and every host request
/// names a player still in the game (F21, F23).
#[test]
fn req_turn_001_property_heartbeat_terminates_or_asks() {
    let mut step_ends = 0;
    let mut no_player = 0;
    let mut resolutions = 0;
    let mut mid_decision_departures = 0;
    for seed in 0..SEEDS {
        let host = generate(seed);
        let initial_depth = host.depth;
        let initially_in_game = players_in_game(&host);
        let base = World::new(host);
        let run = drive(base.clone(), policy(seed));
        let ctx = format!("seed {seed}: {:?}", trace(&run));

        // (a)
        match &run.out {
            Some(Ok(Out::StepEnds)) => {
                step_ends += 1;
                assert!(initially_in_game > 0, "{ctx}");
            }
            Some(Ok(Out::NoPlayerInGame)) => {
                no_player += 1;
                assert_eq!(initially_in_game, 0, "{ctx}");
                assert!(run.requests.is_empty(), "{ctx}");
            }
            other => panic!("{ctx}: unexpected end {other:?}"),
        }
        for r in &run.requests {
            assert!(
                matches!(
                    r.choice_type,
                    ChoiceType::PriorityAction | ChoiceType::ChoosePlayer
                ),
                "{ctx}"
            );
        }
        let last = run.last.fake();
        assert!(last.anomalies.is_empty(), "{ctx}: {:?}", last.anomalies);

        // (e)
        if run.out == Some(Ok(Out::StepEnds)) {
            assert_eq!(
                last.resolutions,
                initial_depth + last.action_pushes + last.triggers_stacked,
                "{ctx}"
            );
            assert_eq!(last.depth, 0, "{ctx}");
            assert_eq!(run.last.priority_player_now(), None, "{ctx}");
        }
        resolutions += last.resolutions;

        // Walk the trace: (b), (c), (d), (f), (h). The oracle keeps the SET of players who
        // passed since the last action or resolution, and derives the expected PassCount and the
        // all-passed threshold from the players still in the game at each pass (F23; CR 117.4,
        // CR 800.4a, CR 800.4j).
        let records = run.last.hb.passes().to_vec();
        assert_eq!(records.len(), pass_answers(&run.answers), "{ctx}");
        let mut ask_index = 0usize;
        let mut resolve_index = 0usize;
        let mut passed: BTreeSet<PlayerId> = BTreeSet::new();
        let mut reset_pending = true;
        let mut rec_index = 0usize;
        let mut pending_moved: Option<PlayerId> = None;
        // R11 (CR 117.4): set by a TopResolved pass, cleared at the resolution.
        let mut resolution_pending = false;
        for ev in trace(&run) {
            match ev {
                Ev::Act(..) => {
                    passed.clear();
                    reset_pending = true;
                }
                Ev::Resolve => {
                    // (h) The top resolves only after every player still in the game (as the
                    // resolution begins) has passed in succession.
                    let live = &last.live_at_resolve[resolve_index];
                    resolve_index += 1;
                    assert!(
                        live.is_subset(&passed),
                        "{ctx}: resolved with {live:?} live but only {passed:?} passed"
                    );
                    if let Some(to) = pending_moved.take() {
                        assert!(
                            !live.contains(&to),
                            "{ctx}: {to:?} was skipped while in game"
                        );
                    }
                    passed.clear();
                    reset_pending = true;
                    resolution_pending = false;
                }
                Ev::HostAsk(chooser) => {
                    // F21: a generated host request names a player still in the game.
                    let at = &run.at_ask[ask_index];
                    ask_index += 1;
                    assert!(
                        !at.host.has_left_game(chooser),
                        "{ctx}: host request to departed {chooser:?}"
                    );
                }
                Ev::Ask(chooser) => {
                    let at = &run.at_ask[ask_index];
                    let answer = &run.answers[ask_index];
                    ask_index += 1;
                    assert!(
                        !resolution_pending,
                        "{ctx}: priority asked before the top object resolved"
                    );
                    let live = live_of(at);
                    // (b), (c)
                    assert!(!at.host.has_left_game(chooser), "{ctx}");
                    assert_eq!(at.priority_player_now(), Some(chooser), "{ctx}");
                    if let Some(to) = pending_moved.take() {
                        // PriorityMoved names the next asked player, unless that player left
                        // before being asked (CR 800.4a).
                        assert!(
                            to == chooser || !live.contains(&to),
                            "{ctx}: PriorityMoved to {to:?} but {chooser:?} was asked"
                        );
                    }
                    // (d) The state's count lies between the live passers and all passers
                    // (a departure during this ask is forgotten at the next pass).
                    let in_game = players_in_game(&at.host);
                    assert_eq!(in_game, live.len(), "{ctx}");
                    let live_passed = passed.intersection(&live).count();
                    assert!(at.hb.pass_count() <= in_game, "{ctx}");
                    assert!(
                        (live_passed..=passed.len()).contains(&at.hb.pass_count()),
                        "{ctx}: pass count {} vs live {live_passed} / all {}",
                        at.hb.pass_count(),
                        passed.len()
                    );
                    if reset_pending {
                        assert_eq!(at.hb.pass_count(), 0, "{ctx}");
                        reset_pending = false;
                    }
                    // (f)
                    if matches!(answer, ChoiceResponse::PassPriority) {
                        // R10 (CR 117.4): one pass per player per pass cycle; an action or a
                        // resolution clears the set.
                        assert!(
                            passed.insert(chooser),
                            "{ctx}: {chooser:?} passed twice in one pass cycle"
                        );
                        passed.retain(|p| live.contains(p));
                        let count = passed.len();
                        let rec = &records[rec_index];
                        assert_eq!(rec.sequence, rec_index as u64, "{ctx}");
                        assert_eq!(rec.turn_number, TURN, "{ctx}");
                        assert_eq!((rec.phase, rec.step), at.fake().phase, "{ctx}");
                        assert_eq!(rec.player, chooser, "{ctx}");
                        assert_eq!(rec.pass_count, count, "{ctx}");
                        assert_eq!(rec.players_in_game, in_game, "{ctx}");
                        assert_eq!(rec.stack_empty, at.host.stack_is_empty(), "{ctx}");
                        let expected_kind = if count < in_game {
                            0
                        } else if !at.host.stack_is_empty() {
                            1
                        } else {
                            2
                        };
                        match (&rec.result, expected_kind) {
                            (PassResult::PriorityMoved { to }, 0) => pending_moved = Some(*to),
                            (PassResult::TopResolved, 1) => resolution_pending = true,
                            (PassResult::StepEnds, 2) => {}
                            (r, k) => panic!("{ctx}: record {rec_index} result {r:?}, kind {k}"),
                        }
                        rec_index += 1;
                    }
                }
                Ev::Sba | Ev::Place(_) => {}
            }
        }
        assert_eq!(rec_index, records.len(), "{ctx}");
        assert_eq!(ask_index, run.requests.len(), "{ctx}");
        assert_eq!(resolve_index, last.live_at_resolve.len(), "{ctx}");
        if !run.last.fake().conceded.borrow().is_empty() {
            mid_decision_departures += 1;
        }
        // (h) The step ends only after every player still in the game has passed in
        // succession: by a StepEnds pass, or (F45) by a departure after the others passed.
        if run.out == Some(Ok(Out::StepEnds)) {
            let live = live_of(&run.last);
            assert!(
                live.is_subset(&passed),
                "{ctx}: step ended with {live:?} live but only {passed:?} passed"
            );
            if let Some(to) = pending_moved {
                assert!(
                    !live.contains(&to),
                    "{ctx}: {to:?} was skipped while in game"
                );
            }
            assert_eq!(run.last.hb.pass_count(), live.len(), "{ctx}");
        }

        // (g)
        let (hash, state) = replay_all(base, &run.answers);
        assert_eq!(hash, run.hash, "{ctx}");
        assert_eq!(state, run.last, "{ctx}");
    }
    // The generator exercises both ends and resolutions.
    assert!(step_ends > SEEDS / 2, "step ends: {step_ends}");
    assert!(no_player > 0, "no-player runs: {no_player}");
    assert!(resolutions > SEEDS, "resolutions: {resolutions}");
    assert!(
        mid_decision_departures > SEEDS / 10,
        "departures while deciding: {mid_decision_departures}"
    );
}

/// The game's current priority player.
trait PriorityNow {
    fn priority_player_now(&self) -> Option<PlayerId>;
}

impl PriorityNow for World<FakeHost> {
    fn priority_player_now(&self) -> Option<PlayerId> {
        self.fake().game.turn().priority_player()
    }
}
