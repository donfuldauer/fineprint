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
/// and departures in SBAs, resolutions and actions that always leave one player in the game).
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
    host
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
/// (g) replaying the answers reproduces the state hash and the final state.
#[test]
fn req_turn_001_property_heartbeat_terminates_or_asks() {
    let mut step_ends = 0;
    let mut no_player = 0;
    let mut resolutions = 0;
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

        // Walk the trace: (b), (c), (d), (f).
        let records = run.last.hb.passes().to_vec();
        assert_eq!(records.len(), pass_answers(&run.answers), "{ctx}");
        let mut ask_index = 0usize;
        let mut counter = 0usize;
        let mut reset_pending = true;
        let mut rec_index = 0usize;
        let mut pending_moved: Option<PlayerId> = None;
        for ev in trace(&run) {
            match ev {
                Ev::Act(..) | Ev::Resolve => {
                    counter = 0;
                    reset_pending = true;
                }
                Ev::HostAsk(_) => ask_index += 1,
                Ev::Ask(chooser) => {
                    let at = &run.at_ask[ask_index];
                    let answer = &run.answers[ask_index];
                    ask_index += 1;
                    // (b), (c)
                    assert!(!at.host.has_left_game(chooser), "{ctx}");
                    assert_eq!(at.priority_player_now(), Some(chooser), "{ctx}");
                    if let Some(to) = pending_moved.take() {
                        assert_eq!(to, chooser, "{ctx}: PriorityMoved names the next asked");
                    }
                    // (d)
                    let in_game = players_in_game(&at.host);
                    assert!(at.hb.pass_count() <= in_game, "{ctx}");
                    assert_eq!(at.hb.pass_count(), counter, "{ctx}");
                    if reset_pending {
                        assert_eq!(at.hb.pass_count(), 0, "{ctx}");
                        reset_pending = false;
                    }
                    // (f)
                    if matches!(answer, ChoiceResponse::PassPriority) {
                        counter += 1;
                        let rec = &records[rec_index];
                        assert_eq!(rec.sequence, rec_index as u64, "{ctx}");
                        assert_eq!(rec.turn_number, TURN, "{ctx}");
                        assert_eq!((rec.phase, rec.step), at.fake().phase, "{ctx}");
                        assert_eq!(rec.player, chooser, "{ctx}");
                        assert_eq!(rec.pass_count, counter, "{ctx}");
                        assert_eq!(rec.players_in_game, in_game, "{ctx}");
                        assert_eq!(rec.stack_empty, at.host.stack_is_empty(), "{ctx}");
                        let expected_kind = if counter < in_game {
                            0
                        } else if !at.host.stack_is_empty() {
                            1
                        } else {
                            2
                        };
                        match (&rec.result, expected_kind) {
                            (PassResult::PriorityMoved { to }, 0) => pending_moved = Some(*to),
                            (PassResult::TopResolved, 1) | (PassResult::StepEnds, 2) => {}
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

        // (g)
        let (hash, state) = replay_all(base, &run.answers);
        assert_eq!(hash, run.hash, "{ctx}");
        assert_eq!(state, run.last, "{ctx}");
    }
    // The generator exercises both ends and resolutions.
    assert!(step_ends > SEEDS / 2, "step ends: {step_ends}");
    assert!(no_player > 0, "no-player runs: {no_player}");
    assert!(resolutions > SEEDS, "resolutions: {resolutions}");
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
