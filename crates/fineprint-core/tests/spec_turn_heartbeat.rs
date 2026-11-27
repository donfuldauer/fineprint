// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests of the priority and SBA heartbeat (REQ-TURN-001, SRS 5.1; REQ-SBA-001 for the
//! SBA stage). Black-box: written from the work-item brief only. Every card is synthetic (R5).
//!
//! CR 117.5: "Each time a player would get priority, the game first performs all applicable
//! state-based actions as a single event (see rule 704, "State-Based Actions"), then repeats
//! this process until no state-based actions are performed. Then triggered abilities are put on
//! the stack (see rule 603, "Handling Triggered Abilities"). These steps repeat in order until no
//! further state-based actions are performed and no abilities trigger. Then the player who would
//! have received priority does so."
//!
//! CR 117.4: "If all players pass in succession (that is, if all players pass without taking any
//! actions in between passing), the spell or ability on top of the stack resolves or, if the
//! stack is empty, the phase or step ends."
//!
//! The SRS 5.1 transitions: T1 SBA performed -> repeat step 1; T2 none performed -> step 2; T3
//! triggers put on the stack -> back to step 1; T4 none put -> step 3; T5 action -> the actor
//! keeps priority, PassCount 0 -> step 1; T6 pass, PassCount < PlayersInGame -> next player in
//! turn order -> step 3 (no settle); T7 all passed, stack non-empty -> resolve top, active player
//! gets priority, PassCount 0 -> step 1; T8 all passed, stack empty -> the step ends.

#[path = "spec_turn_heartbeat_support.rs"]
mod support;

#[path = "spec_sba_support.rs"]
mod sba_support;

use std::cell::Cell;
use std::rc::Rc;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoicePayload, ChoiceResponse, ChoiceSession, ChoiceType,
    Interrupt, Status,
};
use fineprint_core::events::{EventContext, Outcome, Phase, Step, ZoneChangeCause};
use fineprint_core::layers::{ContinuousEffects, LayerEnvironment};
use fineprint_core::replacement::{
    Action, BusEventKind, BusInterrupt, EventMatcher, ReplacementEffect,
};
use fineprint_core::sba::{SbaEnvironment, SbaOutcome, SbaRegistry, perform_all_post_batch};
use fineprint_core::state::GameState;
use fineprint_core::triggers::TriggerPlacement;
use fineprint_core::turn::{
    HeartbeatHost, HeartbeatOutcome, HeartbeatState, MAX_SETTLE_ITERATIONS, PassRecord, PassResult,
    SbaStage, SettleReport, next_player_in_turn_order, players_in_game, run_heartbeat,
    sba_pass_post_batch, settle_before_priority,
};
use fineprint_core::txn::Transactional;
use fineprint_core::zones::ZoneKind;
use support::*;

/// The passes of a run's final heartbeat state.
fn records<H: Fake, O>(run: &Run<H, O>) -> Vec<PassRecord> {
    run.last.hb.passes().to_vec()
}

/// The state's priority player.
fn prio(w: &World<FakeHost>) -> Option<PlayerId> {
    w.fake().game.turn().priority_player()
}

fn assert_no_anomalies<H: Fake, O>(run: &Run<H, O>) {
    assert!(
        run.last.fake().anomalies.is_empty(),
        "host contract violations: {:?}",
        run.last.fake().anomalies
    );
}

// =============================================================================================
// 16. compile-time pin of the API
// =============================================================================================

/// REQ-TURN-001 (SRS 5.1): names every item of the pinned `fineprint_core::turn` API with its
/// field names and signatures, so a mismatch is a compile error pointing at the brief.
#[test]
fn req_turn_001_api_pin() {
    let record = PassRecord {
        sequence: 0,
        turn_number: TURN,
        phase: Some(Phase::Combat),
        step: Some(Step::DeclareBlockers),
        player: P1,
        pass_count: 1,
        players_in_game: 2,
        stack_empty: true,
        result: PassResult::PriorityMoved { to: P2 },
    };
    let copy: PassRecord = record.clone();
    assert_eq!(copy, record);
    assert_ne!(format!("{record:?}"), "");
    for r in [
        PassResult::PriorityMoved { to: P2 },
        PassResult::TopResolved,
        PassResult::StepEnds,
    ] {
        match r {
            PassResult::PriorityMoved { to } => assert_eq!(to, P2),
            PassResult::TopResolved | PassResult::StepEnds => {}
        }
    }
    for o in [HeartbeatOutcome::StepEnds, HeartbeatOutcome::NoPlayerInGame] {
        match o {
            HeartbeatOutcome::StepEnds | HeartbeatOutcome::NoPlayerInGame => {}
        }
    }
    let report = SettleReport {
        sba_passes: 0,
        triggers_stacked: 0,
    };
    let default = SettleReport::default();
    assert_eq!(report.sba_passes, default.sba_passes);
    assert_eq!(report.triggers_stacked, default.triggers_stacked);
    assert_eq!(MAX_SETTLE_ITERATIONS, 10_000);

    let hb = HeartbeatState::new();
    assert_eq!(hb, HeartbeatState::default());
    assert_eq!(hb.clone(), hb);
    assert_eq!(hb.pass_count(), 0);
    assert!(hb.passes().is_empty());
    let _: usize = hb.pass_count();
    let _: &[PassRecord] = hb.passes();

    let _run: fn(
        &mut FakeHost,
        &mut HeartbeatState,
        &mut ChoiceContext<'_>,
    ) -> Result<HeartbeatOutcome, Interrupt> = run_heartbeat::<FakeHost>;
    let _settle: fn(&mut FakeHost, &mut ChoiceContext<'_>) -> Result<SettleReport, Interrupt> =
        settle_before_priority::<FakeHost>;
    let _count: fn(&FakeHost) -> usize = players_in_game::<FakeHost>;
    let _next: fn(&FakeHost, PlayerId) -> Option<PlayerId> = next_player_in_turn_order::<FakeHost>;
    let _stage: fn(SbaStage<'_, '_>, &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> =
        sba_pass_post_batch;

    // `?Sized`: the free functions accept a trait object.
    let host = FakeHost::new(2, Some(P1));
    let dyn_host: &dyn HeartbeatHost = &host;
    assert_eq!(players_in_game(dyn_host), 2);
    assert_eq!(next_player_in_turn_order(dyn_host, P1), Some(P2));
    // The default phase_and_step and has_left_game exist on the trait.
    assert!(!dyn_host.has_left_game(P1));
    assert_eq!(dyn_host.phase_and_step(), host.phase);
}

/// The trait defaults (REQ-TURN-001; CR 800.4a): `phase_and_step` is `(None, None)` and
/// `has_left_game` is true for a player not of the game or with the LOST_THE_GAME designation.
#[test]
fn req_turn_001_trait_defaults() {
    #[derive(Clone)]
    struct Minimal(GameState);
    impl HeartbeatHost for Minimal {
        fn game(&self) -> &GameState {
            &self.0
        }
        fn game_mut(&mut self) -> &mut GameState {
            &mut self.0
        }
        fn sba_pass(&mut self, _: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
            Ok(sba_outcome(false, Vec::new(), Vec::new()))
        }
        fn place_triggers(
            &mut self,
            _: &[PlayerId],
            _: &mut ChoiceContext<'_>,
        ) -> Result<TriggerPlacement, Interrupt> {
            Ok(TriggerPlacement::default())
        }
        fn stack_is_empty(&self) -> bool {
            true
        }
        fn resolve_top(&mut self, _: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
            Ok(())
        }
        fn priority_actions(&self, _: PlayerId) -> Vec<ChoiceResponse> {
            Vec::new()
        }
        fn take_action(
            &mut self,
            _: PlayerId,
            _: &ChoiceResponse,
            _: &mut ChoiceContext<'_>,
        ) -> Result<(), Interrupt> {
            Ok(())
        }
    }
    let mut fake = FakeHost::new(3, Some(P1));
    fake.mark_lost(P2);
    let m = Minimal(fake.game.clone());
    assert_eq!(m.phase_and_step(), (None, None));
    assert!(!m.has_left_game(P1));
    assert!(m.has_left_game(P2), "LOST_THE_GAME means left (CR 800.4a)");
    assert!(!m.has_left_game(P3));
    assert!(
        m.has_left_game(PlayerId::new(9)),
        "not a player of the game"
    );
    assert_eq!(players_in_game(&m), 2);
}

// =============================================================================================
// T1: SBA performed -> repeat step 1
// =============================================================================================

/// REQ-TURN-001, REQ-SBA-001, CR 704.3, CR 117.5 (T1): `sba_pass` reports performed twice, then
/// not: exactly three `sba_pass` calls before the first `place_triggers`, then priority.
#[test]
fn req_turn_001_t1_sba_repeats_while_performed() {
    let host = FakeHost::new(2, Some(P1)).with_sba(vec![SbaStep::Performed, SbaStep::Performed]);
    let run = run_script(host, vec![pass(), pass()]);
    assert_eq!(
        trace(&run),
        vec![
            Ev::Sba,
            Ev::Sba,
            Ev::Sba,
            Ev::Place(vec![P1, P2]),
            Ev::Ask(P1),
            Ev::Ask(P2)
        ]
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert_no_anomalies(&run);
}

/// REQ-TURN-001, REQ-SBA-001, CR 704.3 (T1/T2; DECISIONS "#41 FP-040 — performed means state
/// changed"): findings with `performed == false` do NOT repeat the check.
#[test]
fn req_turn_001_t1_findings_without_performed_do_not_repeat() {
    let host = FakeHost::new(2, Some(P1)).with_sba(vec![SbaStep::FindingsOnly, SbaStep::Performed]);
    let run = run_script(host, vec![]);
    assert_eq!(
        trace(&run),
        vec![Ev::Sba, Ev::Place(vec![P1, P2]), Ev::Ask(P1)]
    );
}

/// REQ-TURN-001, CR 704.3, CR 603.3b (T1-T4): `settle_before_priority` alone runs steps 1 and
/// 2 only (no ask) and reports the SBA passes it ran and the triggers it stacked.
#[test]
fn req_turn_001_settle_before_priority_runs_steps_1_and_2_only() {
    let host = FakeHost::new(3, Some(P2))
        .with_sba(vec![SbaStep::Performed, SbaStep::Performed])
        .with_place(vec![PlaceStep::Place {
            stacked: 2,
            removed: 0,
        }]);
    let run = drive_with(World::new(host), settle_proc::<FakeHost>, script(vec![]));
    assert!(run.requests.is_empty(), "settling never asks for priority");
    assert_eq!(
        trace(&run),
        vec![
            Ev::Sba,
            Ev::Sba,
            Ev::Sba,
            Ev::Place(vec![P2, P3, P1]),
            Ev::Sba,
            Ev::Place(vec![P2, P3, P1]),
        ]
    );
    // sba_passes counts the sba_pass calls (DECISIONS "#44 FP-043 tester", item 1).
    assert_eq!(run.out, Some(Ok((4, 2))));
    assert_eq!(run.last.fake().depth, 2);
    assert_eq!(prio(&run.last), None, "settling grants no priority");
}

// =============================================================================================
// T2: none performed -> step 2 with the APNAP list
// =============================================================================================

/// The turn order starting at `active`.
fn rotate(order: &[PlayerId], active: PlayerId) -> Vec<PlayerId> {
    let i = order.iter().position(|p| *p == active).expect("in order");
    order[i..]
        .iter()
        .chain(order[..i].iter())
        .copied()
        .collect()
}

/// REQ-TURN-001, CR 603.3b, CR 704.3 (T2): with nothing performed, `place_triggers` is called
/// next with APNAP order (active player first, then turn order), for 2, 3 and 4 players and
/// every active seat (seat 1 and the others), with the seat order and a shuffled turn order.
#[test]
fn req_turn_001_t2_place_triggers_gets_apnap_order() {
    for n in 2..=4usize {
        let seats: Vec<PlayerId> = (1..=n as u64).map(PlayerId::new).collect();
        let mut shuffled = seats.clone();
        shuffled.reverse();
        shuffled.swap(0, n - 1);
        for order in [seats.clone(), shuffled] {
            for &active in &seats {
                let mut host = FakeHost::new(n, Some(active));
                host.game
                    .set_turn_order(order.clone())
                    .expect("valid order");
                let run = run_script(host, vec![]);
                let apnap = rotate(&order, active);
                assert_eq!(
                    trace(&run),
                    vec![Ev::Sba, Ev::Place(apnap), Ev::Ask(active)],
                    "n={n} order={order:?} active={active:?}"
                );
            }
        }
    }
}

// =============================================================================================
// T3/T4: triggers put on the stack -> back to step 1; none -> step 3
// =============================================================================================

/// REQ-TURN-001, CR 603.3b, CR 117.5 (T3): a placement that stacked triggers sends the loop
/// back to the SBA check before any priority, until a placement is empty; SBAs performed after
/// a placement repeat as in T1.
#[test]
fn req_turn_001_t3_stacked_triggers_return_to_sba_check() {
    let ap = vec![P1, P2];
    let host = FakeHost::new(2, Some(P1)).with_place(vec![
        PlaceStep::Place {
            stacked: 2,
            removed: 0,
        },
        PlaceStep::Place {
            stacked: 1,
            removed: 1,
        },
    ]);
    let run = run_script(host, vec![]);
    assert_eq!(
        trace(&run),
        vec![
            Ev::Sba,
            Ev::Place(ap.clone()),
            Ev::Sba,
            Ev::Place(ap.clone()),
            Ev::Sba,
            Ev::Place(ap.clone()),
            Ev::Ask(P1)
        ]
    );
    assert_eq!(run.at_ask[0].fake().depth, 3);

    let host = FakeHost::new(2, Some(P1))
        .with_sba(vec![SbaStep::Nothing, SbaStep::Performed])
        .with_place(vec![PlaceStep::Place {
            stacked: 1,
            removed: 0,
        }]);
    let run = run_script(host, vec![]);
    assert_eq!(
        trace(&run),
        vec![
            Ev::Sba,
            Ev::Place(ap.clone()),
            Ev::Sba,
            Ev::Sba,
            Ev::Place(ap.clone()),
            Ev::Ask(P1)
        ]
    );
}

/// REQ-TURN-001, CR 603.3b, CR 603.3c, CR 603.3d (T3; rules-researcher ruling Q4): a trigger
/// removed for having no legal choice was first put on the stack, so a placement that ONLY
/// removed triggers also loops back to the SBA check; only an empty placement ends the settle.
#[test]
fn req_turn_001_t3_removed_only_placement_returns_to_sba_check() {
    let ap = vec![P1, P2];
    let host = FakeHost::new(2, Some(P1)).with_place(vec![PlaceStep::Place {
        stacked: 0,
        removed: 1,
    }]);
    let run = run_script(host, vec![]);
    assert_eq!(
        trace(&run),
        vec![
            Ev::Sba,
            Ev::Place(ap.clone()),
            Ev::Sba,
            Ev::Place(ap),
            Ev::Ask(P1)
        ]
    );
    assert_eq!(run.at_ask[0].fake().depth, 0);
}

// =============================================================================================
// T4: priority request
// =============================================================================================

/// REQ-TURN-001, CR 117.3a, CR 117.5 (T4): after settling, the active player is asked first: a
/// `PRIORITY_ACTION` request to the active player (not seat 1 here) that may pass, may not
/// propose a shortcut, lists exactly `host.priority_actions`, defaults to `PassPriority`, and
/// the game's priority player is the asked player at the ask.
#[test]
fn req_turn_001_t4_active_player_is_asked_first() {
    let actions = vec![land(10), cast(11), mana(12), special(13), activate(14)];
    let host = FakeHost::new(3, Some(P2))
        .with_actions(P2, actions.clone())
        .with_actions(P3, vec![cast(20)]);
    let run = run_script(host, vec![pass()]);
    let req = &run.requests[0];
    assert_eq!(req.chooser, P2);
    assert_eq!(req.choice_type, ChoiceType::PriorityAction);
    assert_eq!(req.default_on_timeout, ChoiceResponse::PassPriority);
    assert_eq!(
        req.payload,
        ChoicePayload::PriorityAction {
            can_pass: true,
            can_propose_shortcut: false,
            actions: actions.clone(),
        }
    );
    assert_eq!(prio(&run.at_ask[0]), Some(P2));
    assert_eq!(run.at_ask[0].priority_player(), Some(P2));
    // The second ask (P3) lists P3's actions.
    let req = &run.requests[1];
    assert_eq!(req.chooser, P3);
    assert_eq!(
        req.payload,
        ChoicePayload::PriorityAction {
            can_pass: true,
            can_propose_shortcut: false,
            actions: vec![cast(20)],
        }
    );
    assert_eq!(prio(&run.at_ask[1]), Some(P3));
}

// =============================================================================================
// T5: action -> actor keeps priority, PassCount 0, settle
// =============================================================================================

/// REQ-TURN-001, CR 117.3c, CR 116.3, CR 605.3b (T5): every action kind goes to `take_action`
/// with the acting player and the exact response; then the loop settles and the SAME player is
/// asked again with PassCount 0. The heartbeat itself never touches the stack: only
/// `take_action` pushes (cast, non-mana activation); a mana ability and a special action push
/// nothing and still end the pass sequence (rules-researcher ruling Q2).
#[test]
fn req_turn_001_t5_actor_keeps_priority_after_settle() {
    let ap = vec![P1, P2];
    for action in [land(10), cast(11), activate(12), mana(13), special(14)] {
        let host = FakeHost::new(2, Some(P1)).with_actions(P1, vec![action.clone()]);
        let run = run_script(host, vec![action.clone()]);
        assert_eq!(
            trace(&run),
            evs(&[
                &quiet_settle(&ap),
                &[Ev::Ask(P1), Ev::Act(P1, action.clone())],
                &quiet_settle(&ap),
                &[Ev::Ask(P1)],
            ]),
            "{action:?}"
        );
        let second = &run.at_ask[1];
        assert_eq!(second.hb.pass_count(), 0);
        assert_eq!(prio(second), Some(P1));
        let pushed = u64::from(pushes_stack(&action));
        assert_eq!(
            second.fake().depth,
            pushed,
            "only take_action touches the stack"
        );
        assert_eq!(second.fake().resolutions, 0);
        assert!(second.hb.passes().is_empty(), "an action is not a pass");
    }
}

/// REQ-TURN-001, CR 117.4, CR 117.3c, CR 116.3, CR 605.3a (T5, T6; ruling Q2): P1 passes, then
/// P2 takes an action that does not use the stack (mana ability or special action): PassCount
/// resets to 0, P2 keeps priority, and BOTH players must pass again before the step ends (P2's
/// next pass is the first of a new succession, so P1 is asked again).
#[test]
fn req_turn_001_t5_any_action_resets_pass_count() {
    let ap = vec![P1, P2];
    for action in [mana(13), special(14), land(15)] {
        let host = FakeHost::new(2, Some(P1)).with_actions(P2, vec![action.clone()]);
        let run = run_script(host, vec![pass(), action.clone(), pass(), pass()]);
        assert_eq!(
            trace(&run),
            evs(&[
                &quiet_settle(&ap),
                &[Ev::Ask(P1), Ev::Ask(P2), Ev::Act(P2, action.clone())],
                &quiet_settle(&ap),
                &[Ev::Ask(P2), Ev::Ask(P1)],
            ]),
            "{action:?}"
        );
        assert_eq!(run.at_ask[1].hb.pass_count(), 1);
        assert_eq!(
            run.at_ask[2].hb.pass_count(),
            0,
            "{action:?} resets PassCount"
        );
        assert_eq!(run.at_ask[3].hb.pass_count(), 1);
        assert_eq!(run.out, Some(Ok(Out::StepEnds)));
        let recs = records(&run);
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[1].player, P2);
        assert_eq!(recs[1].pass_count, 1);
        assert_eq!(recs[2].player, P1);
        assert_eq!(recs[2].pass_count, 2);
        assert_eq!(recs[2].result, PassResult::StepEnds);
    }
}

// =============================================================================================
// T6: pass -> next player in turn order, no settle
// =============================================================================================

/// REQ-TURN-001, CR 117.3d (T6): with 3 players, the active player passes, then the next: the
/// asks go A, B, C with NO `sba_pass`/`place_triggers` between passes; PassCount is 1 then 2;
/// the records say `PriorityMoved` to the next player. Active seat 1 and seat 2.
#[test]
fn req_turn_001_t6_pass_moves_priority_without_settle() {
    for (active, order) in [(P1, [P1, P2, P3]), (P2, [P2, P3, P1])] {
        let host = FakeHost::new(3, Some(active));
        let run = run_script(host, vec![pass(), pass()]);
        assert_eq!(
            trace(&run),
            evs(&[
                &quiet_settle(&order),
                &[Ev::Ask(order[0]), Ev::Ask(order[1]), Ev::Ask(order[2])],
            ])
        );
        assert_eq!(run.at_ask[1].hb.pass_count(), 1);
        assert_eq!(run.at_ask[2].hb.pass_count(), 2);
        assert_eq!(prio(&run.at_ask[1]), Some(order[1]));
        assert_eq!(prio(&run.at_ask[2]), Some(order[2]));
        let recs = run.at_ask[2].hb.passes().to_vec();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].result, PassResult::PriorityMoved { to: order[1] });
        assert_eq!(recs[1].result, PassResult::PriorityMoved { to: order[2] });
        assert_eq!(recs[0].pass_count, 1);
        assert_eq!(recs[1].pass_count, 2);
    }
}

// =============================================================================================
// T7: all passed, stack non-empty -> resolve, active player gets priority
// =============================================================================================

/// REQ-TURN-001, CR 117.4, CR 117.3b (T7): all pass with one object on the stack: exactly one
/// `resolve_top`, then a settle, then the ACTIVE player is asked with PassCount 0; one more full
/// round of passes ends the step.
#[test]
fn req_turn_001_t7_resolution_then_active_player() {
    let ap = vec![P1, P2, P3];
    let round = [Ev::Ask(P1), Ev::Ask(P2), Ev::Ask(P3)];
    let host = FakeHost::new(3, Some(P1)).with_depth(1);
    let run = run_script(host, vec![pass(); 6]);
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&ap),
            &round,
            &[Ev::Resolve],
            &quiet_settle(&ap),
            &round,
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert_eq!(run.at_ask[3].hb.pass_count(), 0);
    assert_eq!(prio(&run.at_ask[3]), Some(P1));
    let recs = records(&run);
    assert_eq!(recs[2].result, PassResult::TopResolved);
    assert_eq!(
        recs[2].player, P3,
        "the last passer was not the active player"
    );
    assert!(!recs[2].stack_empty);
    assert_no_anomalies(&run);
}

/// REQ-TURN-001, CR 117.4, CR 117.3b (T7): a depth-2 stack needs two full rounds, one per
/// resolution, and the active player gets priority after each resolution even though the stack
/// still holds an object after the first.
#[test]
fn req_turn_001_t7_depth_two_needs_two_rounds() {
    let ap = vec![P2, P3, P1];
    let round = [Ev::Ask(P2), Ev::Ask(P3), Ev::Ask(P1)];
    let host = FakeHost::new(3, Some(P2)).with_depth(2);
    let run = run_script(host, vec![pass(); 9]);
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&ap),
            &round,
            &[Ev::Resolve],
            &quiet_settle(&ap),
            &round,
            &[Ev::Resolve],
            &quiet_settle(&ap),
            &round,
        ])
    );
    assert_eq!(run.at_ask[3].fake().depth, 1);
    assert_eq!(prio(&run.at_ask[3]), Some(P2));
    assert_eq!(run.at_ask[3].hb.pass_count(), 0);
    assert_eq!(run.at_ask[6].fake().depth, 0);
    assert_eq!(prio(&run.at_ask[6]), Some(P2));
    let recs = records(&run);
    assert_eq!(recs[2].result, PassResult::TopResolved);
    assert_eq!(recs[5].result, PassResult::TopResolved);
    assert_eq!(recs[8].result, PassResult::StepEnds);
    assert_eq!(run.last.fake().resolutions, 2);
}

/// REQ-TURN-001, CR 117.4, CR 117.3b (T7): the succession that resolves need not start with the
/// active player: P1 and P2 pass, P3 casts (keeps priority), P3, P1, P2 pass: the top resolves
/// and the active player P1 (not the last passer P2) is asked next.
#[test]
fn req_turn_001_t7_active_player_after_resolution_when_last_passer_differs() {
    let ap = vec![P1, P2, P3];
    let host = FakeHost::new(3, Some(P1)).with_actions(P3, vec![cast(30)]);
    let run = run_script(host, vec![pass(), pass(), cast(30), pass(), pass(), pass()]);
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&ap),
            &[Ev::Ask(P1), Ev::Ask(P2), Ev::Ask(P3), Ev::Act(P3, cast(30))],
            &quiet_settle(&ap),
            &[Ev::Ask(P3), Ev::Ask(P1), Ev::Ask(P2), Ev::Resolve],
            &quiet_settle(&ap),
            &[Ev::Ask(P1)],
        ])
    );
    assert_eq!(run.at_ask[6].hb.pass_count(), 0);
    let recs = run.at_ask[6].hb.passes().to_vec();
    assert_eq!(recs.last().expect("a record").player, P2);
    assert_eq!(
        recs.last().expect("a record").result,
        PassResult::TopResolved
    );
}

// =============================================================================================
// T8: all passed, stack empty -> the step ends
// =============================================================================================

/// REQ-TURN-001, CR 117.4 (T8): all pass with an empty stack: `Ok(StepEnds)`, nothing is called
/// after the last pass (no resolve, no settle), no priority player, the last record is
/// `StepEnds` with `stack_empty`.
#[test]
fn req_turn_001_t8_step_ends_on_empty_stack() {
    for n in 2..=4usize {
        let host = FakeHost::new(n, Some(P1));
        let run = run_script(host, vec![pass(); n]);
        let order: Vec<PlayerId> = (1..=n as u64).map(PlayerId::new).collect();
        let mut expected = quiet_settle(&order);
        expected.extend(order.iter().map(|p| Ev::Ask(*p)));
        assert_eq!(trace(&run), expected, "nothing after the last pass");
        assert_eq!(run.out, Some(Ok(Out::StepEnds)));
        assert_eq!(prio(&run.last), None);
        let recs = records(&run);
        let last = recs.last().expect("a record");
        assert_eq!(last.result, PassResult::StepEnds);
        assert!(last.stack_empty);
        assert_eq!(last.pass_count, n);
        assert_eq!(run.last.fake().resolutions, 0);
    }
}

// =============================================================================================
// 9. pass records
// =============================================================================================

/// REQ-TURN-001, CR 117.3d, CR 117.4 (SRS 5.1: every pass recorded explicitly): every
/// `PassPriority` answer yields exactly one record, in order, with sequence, turn, phase, step,
/// player, PassCount after the pass, players in game, stack emptiness at the pass and result.
#[test]
fn req_turn_001_pass_records_are_complete() {
    let mut host = FakeHost::new(2, Some(P1))
        .with_depth(1)
        .with_actions(P2, vec![mana(5)]);
    host.phase = (Some(Phase::Combat), Some(Step::DeclareBlockers));
    let run = run_script(host, vec![pass(), mana(5), pass(), pass(), pass(), pass()]);
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    let recs = records(&run);
    assert_eq!(recs.len(), pass_answers(&run.answers));
    assert_eq!(recs.len(), 5);
    let expected = [
        (P1, 1, false, PassResult::PriorityMoved { to: P2 }),
        (P2, 1, false, PassResult::PriorityMoved { to: P1 }),
        (P1, 2, false, PassResult::TopResolved),
        (P1, 1, true, PassResult::PriorityMoved { to: P2 }),
        (P2, 2, true, PassResult::StepEnds),
    ];
    for (i, (rec, (player, count, empty, result))) in recs.iter().zip(expected).enumerate() {
        assert_eq!(
            *rec,
            PassRecord {
                sequence: i as u64,
                turn_number: TURN,
                phase: Some(Phase::Combat),
                step: Some(Step::DeclareBlockers),
                player,
                pass_count: count,
                players_in_game: 2,
                stack_empty: empty,
                result,
            },
            "record {i}"
        );
    }
}

/// REQ-TURN-001, CR 117.4, CR 500.4 (records across steps): a second `run_heartbeat` with the
/// same `HeartbeatState` continues the sequence numbers, starts with PassCount 0, and records
/// the new step from `phase_and_step()`.
#[test]
fn req_turn_001_pass_records_continue_across_runs() {
    let mut w = World::new(FakeHost::new(2, Some(P2)));
    w.rounds = vec![
        (Some(Phase::Beginning), Some(Step::Upkeep)),
        (Some(Phase::Beginning), Some(Step::Draw)),
    ];
    let run = drive(w, script(vec![pass(); 4]));
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert_eq!(run.last.outs, vec![Out::StepEnds, Out::StepEnds]);
    assert_eq!(
        run.at_ask[2].hb.pass_count(),
        0,
        "a new run starts at PassCount 0"
    );
    let recs = records(&run);
    let seqs: Vec<u64> = recs.iter().map(|r| r.sequence).collect();
    assert_eq!(seqs, vec![0, 1, 2, 3]);
    let counts: Vec<usize> = recs.iter().map(|r| r.pass_count).collect();
    assert_eq!(counts, vec![1, 2, 1, 2]);
    let steps: Vec<Option<Step>> = recs.iter().map(|r| r.step).collect();
    assert_eq!(
        steps,
        vec![
            Some(Step::Upkeep),
            Some(Step::Upkeep),
            Some(Step::Draw),
            Some(Step::Draw)
        ]
    );
    let players: Vec<PlayerId> = recs.iter().map(|r| r.player).collect();
    assert_eq!(players, vec![P2, P1, P2, P1]);
    assert_eq!(recs[1].result, PassResult::StepEnds);
    assert_eq!(recs[3].result, PassResult::StepEnds);
}

// =============================================================================================
// 10. yields are not an input; deterministic replay
// =============================================================================================

/// A mixed scenario: 3 players, depth 1, actions, triggers, an SBA repeat.
fn mixed_host() -> FakeHost {
    FakeHost::new(3, Some(P2))
        .with_depth(1)
        .with_sba(vec![SbaStep::Performed])
        .with_place(vec![PlaceStep::Place {
            stacked: 1,
            removed: 0,
        }])
        .with_actions(P3, vec![cast(31), mana(32)])
        .with_actions(P1, vec![special(11)])
}

fn mixed_answers() -> Vec<ChoiceResponse> {
    vec![
        pass(),
        mana(32),
        pass(),
        special(11),
        pass(),
        pass(),
        pass(), // all passed: top resolves
        pass(),
        cast(31),
        pass(),
        pass(),
        pass(), // resolves the cast
        pass(),
        pass(),
        pass(), // resolves the original object
        pass(),
        pass(),
        pass(), // step ends
    ]
}

/// REQ-TURN-001, REQ-CORE-001, CR 117.3d (SRS 5.1: yields are a client convenience): every
/// record comes from an explicit `PassPriority` answer, and replaying `session.answers()` on the
/// same base (one by one, or all at once) gives the same final state hash, the same pass log
/// and the same request sequence (R3).
#[test]
fn req_turn_001_replay_is_deterministic() {
    let base = World::new(mixed_host());
    let run = drive(base.clone(), script(mixed_answers()));
    assert_eq!(run.out, Some(Ok(Out::StepEnds)), "{:?}", trace(&run));
    assert_no_anomalies(&run);
    assert_eq!(run.answers, mixed_answers());
    let recs = records(&run);
    assert_eq!(recs.len(), pass_answers(&run.answers));
    // Each record's player is the chooser of a request answered with PassPriority, in order.
    let passers: Vec<PlayerId> = run
        .requests
        .iter()
        .zip(&run.answers)
        .filter(|(_, a)| matches!(a, ChoiceResponse::PassPriority))
        .map(|(r, _)| r.chooser)
        .collect();
    let recorded: Vec<PlayerId> = recs.iter().map(|r| r.player).collect();
    assert_eq!(recorded, passers);

    // One by one again.
    let again = drive(base.clone(), script(run.answers.clone()));
    assert_eq!(again.hash, run.hash);
    assert_eq!(again.requests, run.requests);
    assert_eq!(again.last, run.last);
    // All at once.
    let (hash, state) = replay_all(base, &run.answers);
    assert_eq!(hash, run.hash);
    assert_eq!(state, run.last);
    assert_eq!(state.hb.passes(), run.last.hb.passes());
}

/// REQ-TURN-001, REQ-NET-003, CR 117.3d: a timeout (`submit_default`) answers with
/// `PassPriority`, which is recorded like any explicit pass.
#[test]
fn req_turn_001_timeout_default_is_an_explicit_pass() {
    let base = World::new(FakeHost::new(2, Some(P1)));
    let mut session =
        ChoiceSession::start(base, heartbeat_proc::<FakeHost>).expect("session starts");
    for _ in 0..2 {
        let id = session.pending().expect("pending").request_id;
        session.submit_default(id).expect("default accepted");
    }
    assert_eq!(session.answers(), &[pass(), pass()]);
    match session.status() {
        Status::Done { state, output } => {
            assert_eq!(*output, Ok(Out::StepEnds));
            assert_eq!(state.hb.passes().len(), 2);
        }
        Status::Pending(r) => panic!("still pending: {r:?}"),
    }
}

// =============================================================================================
// 11. players who have left the game
// =============================================================================================

/// REQ-TURN-001, CR 800.4a, CR 117.3d (SRS 5.1: players who left are skipped): in a 4-player
/// game with player 3 lost, player 3 is never asked or counted; passes go 1 -> 2 -> 4 -> all
/// passed; the APNAP list excludes player 3.
#[test]
fn req_turn_001_left_player_is_skipped() {
    let mut host = FakeHost::new(4, Some(P1));
    host.mark_lost(P3);
    assert_eq!(players_in_game(&host), 3);
    let run = run_script(host, vec![pass(); 3]);
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&[P1, P2, P4]),
            &[Ev::Ask(P1), Ev::Ask(P2), Ev::Ask(P4)],
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    let recs = records(&run);
    assert!(recs.iter().all(|r| r.players_in_game == 3));
    assert_eq!(recs[1].result, PassResult::PriorityMoved { to: P4 });
    assert_eq!(recs[2].result, PassResult::StepEnds);
    assert_eq!(recs[2].pass_count, 3);
}

/// REQ-TURN-001, CR 800.4a, CR 117.3d: `next_player_in_turn_order` skips players who left,
/// wraps from the last seat, works for a departed `after`, may return `after` itself when it is
/// the only one left, and is `None` when nobody is in the game.
#[test]
fn req_turn_001_next_player_in_turn_order() {
    let mut host = FakeHost::new(4, Some(P1));
    host.mark_lost(P3);
    assert_eq!(next_player_in_turn_order(&host, P1), Some(P2));
    assert_eq!(next_player_in_turn_order(&host, P2), Some(P4));
    assert_eq!(next_player_in_turn_order(&host, P4), Some(P1), "wraps");
    assert_eq!(
        next_player_in_turn_order(&host, P3),
        Some(P4),
        "departed `after`"
    );

    // A non-seat turn order.
    let mut host = FakeHost::new(4, Some(P1));
    host.game
        .set_turn_order(vec![P1, P4, P2, P3])
        .expect("valid order");
    host.mark_lost(P2);
    assert_eq!(next_player_in_turn_order(&host, P4), Some(P3));
    assert_eq!(next_player_in_turn_order(&host, P2), Some(P3));
    assert_eq!(next_player_in_turn_order(&host, P3), Some(P1));

    let mut host = FakeHost::new(3, Some(P1));
    host.mark_lost(P2);
    host.mark_lost(P3);
    assert_eq!(players_in_game(&host), 1);
    assert_eq!(
        next_player_in_turn_order(&host, P1),
        Some(P1),
        "only one left"
    );
    assert_eq!(next_player_in_turn_order(&host, P2), Some(P1));
    host.mark_lost(P1);
    assert_eq!(players_in_game(&host), 0);
    assert_eq!(next_player_in_turn_order(&host, P1), None);
    assert_eq!(next_player_in_turn_order(&host, P3), None);
}

/// REQ-TURN-001, CR 800.4a, CR 117.4: a single remaining player is asked, and their one pass is
/// "all players passed".
#[test]
fn req_turn_001_single_remaining_player() {
    let mut host = FakeHost::new(3, Some(P2));
    host.mark_lost(P1);
    host.mark_lost(P3);
    let run = run_script(host, vec![pass()]);
    assert_eq!(trace(&run), evs(&[&quiet_settle(&[P2]), &[Ev::Ask(P2)]]));
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    let rec = &records(&run)[0];
    assert_eq!(
        (rec.pass_count, rec.players_in_game, &rec.result),
        (1, 1, &PassResult::StepEnds)
    );
}

/// REQ-TURN-001, CR 800.4a: nobody in the game: `run_heartbeat` returns `NoPlayerInGame` on
/// entry without asking (and without settling).
#[test]
fn req_turn_001_no_player_in_game() {
    let mut host = FakeHost::new(2, Some(P1));
    host.mark_lost(P1);
    host.mark_lost(P2);
    let run = run_script(host, vec![]);
    assert!(run.requests.is_empty());
    assert_eq!(run.out, Some(Ok(Out::NoPlayerInGame)));
    assert!(run.last.fake().log.is_empty(), "returns on entry");
    assert!(run.last.hb.passes().is_empty());
}

/// REQ-TURN-001, CR 800.4j, CR 117.3a, CR 117.3b (ruling Q5a/Q5b): the active player has left:
/// priority starts at the next in-game player after the active player's seat, the APNAP list
/// starts there too, and after a resolution that player gets priority again.
#[test]
fn req_turn_001_active_player_left() {
    let mut host = FakeHost::new(4, Some(P2)).with_depth(1);
    host.mark_lost(P2);
    let ap = [P3, P4, P1];
    let round = [Ev::Ask(P3), Ev::Ask(P4), Ev::Ask(P1)];
    let run = run_script(host, vec![pass(); 6]);
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&ap),
            &round,
            &[Ev::Resolve],
            &quiet_settle(&ap),
            &round,
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert!(records(&run).iter().all(|r| r.players_in_game == 3));
}

/// REQ-TURN-001, CR 117.3a (brief: no active player -> the first in-game player in turn order;
/// DECISIONS "#44 FP-043 tester", item 3): with no active player at all, priority and APNAP
/// start at the first in-game player in turn order.
#[test]
fn req_turn_001_no_active_player_starts_at_first_in_game() {
    let run = run_script(FakeHost::new(3, None), vec![pass()]);
    assert_eq!(
        trace(&run),
        evs(&[&quiet_settle(&[P1, P2, P3]), &[Ev::Ask(P1), Ev::Ask(P2)]])
    );
    let mut host = FakeHost::new(3, None);
    host.mark_lost(P1);
    let run = run_script(host, vec![]);
    assert_eq!(
        trace(&run),
        evs(&[&quiet_settle(&[P2, P3]), &[Ev::Ask(P2)]])
    );
}

/// REQ-TURN-001, CR 704.3, CR 800.4a: the player who would receive priority loses to an SBA
/// during the settle: the next in-game player in turn order is asked instead; later the
/// departed active player is skipped after a resolution too, and the count is 2.
#[test]
fn req_turn_001_priority_holder_lost_during_settle_is_replaced() {
    // At the start of the step.
    let host = FakeHost::new(3, Some(P1)).with_sba(vec![SbaStep::Lose(vec![P1])]);
    let run = run_script(host, vec![pass(), pass()]);
    assert_eq!(
        trace(&run),
        vec![
            Ev::Sba,
            Ev::Sba,
            Ev::Place(vec![P2, P3]),
            Ev::Ask(P2),
            Ev::Ask(P3)
        ]
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert!(records(&run).iter().all(|r| r.players_in_game == 2));

    // After an action: P1 casts and keeps priority, then loses to an SBA in the settle.
    let host = FakeHost::new(3, Some(P1))
        .with_actions(P1, vec![cast(11)])
        .with_sba(vec![SbaStep::Nothing, SbaStep::Lose(vec![P1])]);
    let run = run_script(host, vec![cast(11), pass(), pass(), pass(), pass()]);
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&[P1, P2, P3]),
            &[Ev::Ask(P1), Ev::Act(P1, cast(11)), Ev::Sba, Ev::Sba],
            &[
                Ev::Place(vec![P2, P3]),
                Ev::Ask(P2),
                Ev::Ask(P3),
                Ev::Resolve
            ],
            &quiet_settle(&[P2, P3]),
            &[Ev::Ask(P2), Ev::Ask(P3)],
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert_eq!(prio(&run.at_ask[1]), Some(P2));
}

/// REQ-TURN-001, CR 800.4a, CR 117.4: a player who left during a resolution changes the count
/// of the next comparison: 3 players, P3 loses while the top resolves; then P1 and P2 passing
/// is "all passed" (2 in game).
#[test]
fn req_turn_001_departure_changes_the_count() {
    let mut host = FakeHost::new(3, Some(P1)).with_depth(1);
    host.resolve_steps = vec![HookStep::Lose(vec![P3])].into();
    let run = run_script(host, vec![pass(); 5]);
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&[P1, P2, P3]),
            &[Ev::Ask(P1), Ev::Ask(P2), Ev::Ask(P3), Ev::Resolve],
            &quiet_settle(&[P1, P2]),
            &[Ev::Ask(P1), Ev::Ask(P2)],
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    let recs = records(&run);
    assert_eq!(recs[2].players_in_game, 3);
    assert_eq!(recs[3].players_in_game, 2);
    assert_eq!(
        (recs[4].pass_count, recs[4].players_in_game, &recs[4].result),
        (2, 2, &PassResult::StepEnds)
    );
}

/// The conceding host: `leaver` leaves just before `when_asking` is asked.
fn conceding(host: FakeHost, when_asking: PlayerId, leaver: PlayerId) -> World<Conceding> {
    World::new(Conceding {
        inner: host,
        when_asking,
        leaver,
        conceded: Cell::new(false),
    })
}

/// REQ-TURN-001, CR 117.4, CR 800.4a (ruling Q5c): a departed player's earlier pass no longer
/// counts. 3 players: A passes, A leaves before B is asked (no host call in between), B passes:
/// not all passed (B alone of the 2 in-game players); C must pass too.
#[test]
fn req_turn_001_departed_players_pass_no_longer_counts() {
    let run = drive(
        conceding(FakeHost::new(3, Some(P1)), P2, P1),
        script(vec![pass(); 3]),
    );
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&[P1, P2, P3]),
            &[Ev::Ask(P1), Ev::Ask(P2), Ev::Ask(P3)],
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    let recs = run.last.hb.passes().to_vec();
    assert_eq!(recs.len(), 3);
    assert_eq!(
        (recs[0].player, recs[0].pass_count, recs[0].players_in_game),
        (P1, 1, 3)
    );
    assert_eq!(recs[0].result, PassResult::PriorityMoved { to: P2 });
    assert_eq!(
        (recs[1].player, recs[1].pass_count, recs[1].players_in_game),
        (P2, 1, 2),
        "A's pass no longer counts after A left"
    );
    assert_eq!(recs[1].result, PassResult::PriorityMoved { to: P3 });
    assert_eq!(
        (recs[2].player, recs[2].pass_count, recs[2].players_in_game),
        (P3, 2, 2)
    );
    assert_eq!(recs[2].result, PassResult::StepEnds);
    assert_eq!(run.at_ask[2].hb.pass_count(), 1);
}

/// REQ-TURN-001, CR 117.4, CR 800.4a (ruling Q5c): a departure does not reset the passes of the
/// players still in the game. 3 players: A and B pass, B leaves before C is asked, C passes:
/// A and C (every in-game player) have passed in succession, so the top resolves.
#[test]
fn req_turn_001_departure_keeps_remaining_passes() {
    let host = FakeHost::new(3, Some(P1)).with_depth(1);
    let run = drive(conceding(host, P3, P2), script(vec![pass(); 5]));
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&[P1, P2, P3]),
            &[Ev::Ask(P1), Ev::Ask(P2), Ev::Ask(P3), Ev::Resolve],
            &quiet_settle(&[P1, P3]),
            &[Ev::Ask(P1), Ev::Ask(P3)],
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    let recs = run.last.hb.passes().to_vec();
    assert_eq!(
        (recs[2].player, recs[2].pass_count, recs[2].players_in_game),
        (P3, 2, 2)
    );
    assert_eq!(recs[2].result, PassResult::TopResolved);
    // `HeartbeatState::pass_count()` takes no host, so at the ask of C it cannot yet see B's
    // concession (which this fake makes inside `priority_actions(C)`); the binding count is the
    // record's, checked above (DECISIONS "#44 FP-043 tester", item 5).
    assert_eq!(
        recs[4].result,
        PassResult::StepEnds,
        "after the resolution P1 then P3 pass"
    );
}

/// REQ-TURN-001, CR 800.4a (F43): the player about to be asked leaves while their actions are
/// collected, before the request is published: nobody asks them; the next player in turn
/// order still in the game is the first chooser.
#[test]
fn req_turn_001_player_leaving_before_publication_is_not_asked() {
    let run = drive(
        conceding(FakeHost::new(3, Some(P1)), P1, P1),
        script(vec![pass(); 2]),
    );
    let choosers: Vec<PlayerId> = run.requests.iter().map(|r| r.chooser).collect();
    assert_eq!(choosers, vec![P2, P3]);
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert!(records(&run).iter().all(|r| r.player != P1));
}

/// REQ-TURN-001, CR 117.4, CR 800.4a, CR 800.4j (F45): P1 and P2 pass, P3 leaves while deciding.
/// Every player still in the game has passed in succession: with an empty stack the step ends,
/// with an object on the stack it resolves and the active player receives priority; no request
/// goes to anyone in between and no pass is recorded for the departure.
#[test]
fn req_turn_001_departure_after_the_others_passed_completes_the_round() {
    let run = drive(
        conceding(FakeHost::new(3, Some(P1)), P3, P3),
        script(vec![pass(); 4]),
    );
    assert_eq!(
        trace(&run),
        evs(&[&quiet_settle(&[P1, P2, P3]), &[Ev::Ask(P1), Ev::Ask(P2)]])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert_eq!(records(&run).len(), 2);

    let run = drive(
        conceding(FakeHost::new(3, Some(P1)).with_depth(1), P3, P3),
        script(vec![pass(); 4]),
    );
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&[P1, P2, P3]),
            &[Ev::Ask(P1), Ev::Ask(P2), Ev::Resolve],
            &quiet_settle(&[P1, P2]),
            &[Ev::Ask(P1), Ev::Ask(P2)],
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    let recs = records(&run);
    assert_eq!(recs.len(), 4);
    assert_eq!(recs[1].result, PassResult::PriorityMoved { to: P3 });
    assert_eq!(recs[3].result, PassResult::StepEnds);
}

// =============================================================================================
// 12. settle guard
// =============================================================================================

fn assert_guard_error(out: &Option<Result<impl std::fmt::Debug, ChoiceError>>) {
    match out {
        Some(Err(ChoiceError::Procedure(_))) => {}
        other => panic!("expected Err(Procedure), got {other:?}"),
    }
}

/// REQ-TURN-001, CR 704.3, CR 117.5 (settle guard): an `sba_pass` that always performs makes
/// `run_heartbeat` and `settle_before_priority` fail with `ChoiceError::Procedure` after about
/// `MAX_SETTLE_ITERATIONS` iterations, without a panic and without asking.
#[test]
fn req_turn_001_settle_guard_sba_forever() {
    let mut host = FakeHost::new(2, Some(P1));
    host.sba_forever = true;
    let run = run_script(host.clone(), vec![]);
    assert!(run.requests.is_empty());
    assert_guard_error(&run.out);
    let calls = run.last.fake().sba_calls;
    assert!(
        (MAX_SETTLE_ITERATIONS - 2..=MAX_SETTLE_ITERATIONS + 2).contains(&calls),
        "sba_pass calls: {calls}"
    );
    assert_eq!(run.last.fake().place_calls, 0);

    let run = drive_with(World::new(host), settle_proc::<FakeHost>, script(vec![]));
    assert_guard_error(&run.out);
    let calls = run.last.fake().sba_calls;
    assert!(
        (MAX_SETTLE_ITERATIONS - 2..=MAX_SETTLE_ITERATIONS + 2).contains(&calls),
        "sba_pass calls: {calls}"
    );
}

/// REQ-TURN-001, CR 603.3b, CR 117.5 (settle guard): a `place_triggers` that always stacks a
/// trigger also ends in `ChoiceError::Procedure`, bounded by `MAX_SETTLE_ITERATIONS`.
#[test]
fn req_turn_001_settle_guard_triggers_forever() {
    let mut host = FakeHost::new(2, Some(P1));
    host.place_forever = true;
    for settle_only in [false, true] {
        let run = if settle_only {
            let r = drive_with(
                World::new(host.clone()),
                settle_proc::<FakeHost>,
                script(vec![]),
            );
            assert_guard_error(&r.out);
            (
                r.requests.len(),
                r.last.fake().place_calls,
                r.last.fake().sba_calls,
            )
        } else {
            let r = run_script(host.clone(), vec![]);
            assert_guard_error(&r.out);
            (
                r.requests.len(),
                r.last.fake().place_calls,
                r.last.fake().sba_calls,
            )
        };
        let (asks, places, sbas) = run;
        assert_eq!(asks, 0);
        assert!(
            (MAX_SETTLE_ITERATIONS / 2 - 2..=MAX_SETTLE_ITERATIONS + 2).contains(&places),
            "place_triggers calls: {places}"
        );
        assert!(sbas <= MAX_SETTLE_ITERATIONS + 2, "sba_pass calls: {sbas}");
    }
}

// =============================================================================================
// 13. interrupts
// =============================================================================================

/// REQ-TURN-001, REQ-NET-003, CR 704.3: an `sba_pass` that asks (`BusInterrupt::Pending`)
/// suspends the session with that request; after the answer the heartbeat resumes (the pass
/// performed, so the check repeats) and reaches priority.
#[test]
fn req_turn_001_sba_pending_suspends_and_resumes() {
    let host = FakeHost::new(2, Some(P2)).with_sba(vec![SbaStep::Ask]);
    let run = run_script(host, vec![host_answer(P2), pass(), pass()]);
    assert_eq!(run.requests[0].choice_type, ChoiceType::ChoosePlayer);
    assert_eq!(run.requests[0].chooser, P2);
    assert_eq!(
        trace(&run),
        vec![
            Ev::Sba,
            Ev::HostAsk(P2),
            Ev::Sba,
            Ev::Place(vec![P2, P1]),
            Ev::Ask(P2),
            Ev::Ask(P1)
        ]
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert_eq!(records(&run).len(), 2);
}

/// REQ-TURN-001, REQ-NET-003, CR 603.3b: a `place_triggers` that asks suspends and resumes.
#[test]
fn req_turn_001_place_pending_suspends_and_resumes() {
    let host = FakeHost::new(2, Some(P1)).with_place(vec![PlaceStep::Ask]);
    let run = run_script(host, vec![host_answer(P1), pass(), pass()]);
    assert_eq!(
        trace(&run),
        vec![
            Ev::Sba,
            Ev::Place(vec![P1, P2]),
            Ev::HostAsk(P1),
            Ev::Ask(P1),
            Ev::Ask(P2)
        ]
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
}

/// REQ-TURN-001, REQ-NET-003, CR 117.3c: a `take_action` that asks suspends; after the answer
/// the action completes and the actor keeps priority.
#[test]
fn req_turn_001_take_action_pending_suspends_and_resumes() {
    let mut host = FakeHost::new(2, Some(P1)).with_actions(P1, vec![cast(11)]);
    host.action_steps = vec![HookStep::Ask].into();
    let run = run_script(
        host,
        vec![cast(11), host_answer(P1), pass(), pass(), pass(), pass()],
    );
    let ap = [P1, P2];
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&ap),
            &[Ev::Ask(P1), Ev::Act(P1, cast(11)), Ev::HostAsk(P1)],
            &quiet_settle(&ap),
            &[Ev::Ask(P1), Ev::Ask(P2), Ev::Resolve],
            &quiet_settle(&ap),
            &[Ev::Ask(P1), Ev::Ask(P2)],
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
    assert_eq!(run.last.fake().resolutions, 1);
}

/// REQ-TURN-001, REQ-NET-003, CR 117.4: a `resolve_top` that asks suspends; after the answer the
/// resolution completes and the active player gets priority.
#[test]
fn req_turn_001_resolve_pending_suspends_and_resumes() {
    let mut host = FakeHost::new(2, Some(P1)).with_depth(1);
    host.resolve_steps = vec![HookStep::Ask].into();
    let run = run_script(host, vec![pass(), pass(), host_answer(P1), pass(), pass()]);
    let ap = [P1, P2];
    assert_eq!(
        trace(&run),
        evs(&[
            &quiet_settle(&ap),
            &[Ev::Ask(P1), Ev::Ask(P2), Ev::Resolve, Ev::HostAsk(P1)],
            &quiet_settle(&ap),
            &[Ev::Ask(P1), Ev::Ask(P2)],
        ])
    );
    assert_eq!(run.out, Some(Ok(Out::StepEnds)));
}

/// REQ-TURN-001: host errors surface as `Err`, never a panic: `BusInterrupt::Error` from
/// `sba_pass` and `Interrupt::Error` from `place_triggers`, `take_action` and `resolve_top`.
#[test]
fn req_turn_001_host_errors_surface_as_err() {
    let host = FakeHost::new(2, Some(P1)).with_sba(vec![SbaStep::Error]);
    let run = run_script(host, vec![]);
    assert!(matches!(run.out, Some(Err(_))), "{:?}", run.out);
    assert!(run.requests.is_empty());

    let host = FakeHost::new(2, Some(P1)).with_place(vec![PlaceStep::Error]);
    let run = run_script(host, vec![]);
    assert_eq!(run.out, Some(Err(fake_error())));

    let mut host = FakeHost::new(2, Some(P1)).with_actions(P1, vec![land(3)]);
    host.action_steps = vec![HookStep::Error].into();
    let run = run_script(host, vec![land(3)]);
    assert_eq!(run.out, Some(Err(fake_error())));

    let mut host = FakeHost::new(2, Some(P1)).with_depth(1);
    host.resolve_steps = vec![HookStep::Error].into();
    let run = run_script(host, vec![pass(), pass()]);
    assert_eq!(run.out, Some(Err(fake_error())));
}

// =============================================================================================
// 14. the standard SBA stage against a real world
// =============================================================================================

use sba_support::{Sim, eid, submit_player};

/// The state-based graveyard move of `object`.
fn sba_move_of(object: ObjectId) -> EventMatcher {
    EventMatcher {
        affected_object: Some(object),
        cause: Some(ZoneChangeCause::StateBasedAction),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    }
}

/// How a pass is run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Via {
    Stage,
    Direct,
}

/// A post-batch eligibility gate: while `blocker` is on the battlefield, `player` may not
/// protect `battle`; the provider's post-batch view lifts the restriction once `blocker` has
/// left (in the same batch).
#[derive(Clone, Copy, Debug)]
struct Gate {
    blocker: ObjectId,
    battle: ObjectId,
    player: PlayerId,
}

/// One SBA pass over `s` with the standard registry, through `sba_pass_post_batch` or
/// `sba::perform_all_post_batch` with the same arguments; the provider hands back the pass's own
/// layer inputs and environment (adjusted by `gate`, if any, to the state after the batch) and
/// counts its calls in `calls`.
fn one_pass(
    s: &mut Sim,
    c: &mut ChoiceContext<'_>,
    via: Via,
    calls: &Cell<usize>,
) -> Result<SbaOutcome, BusInterrupt> {
    one_pass_gated(s, c, via, calls, None)
}

fn one_pass_gated(
    s: &mut Sim,
    c: &mut ChoiceContext<'_>,
    via: Via,
    calls: &Cell<usize>,
    gate: Option<Gate>,
) -> Result<SbaOutcome, BusInterrupt> {
    let registry = SbaRegistry::standard();
    let view_env = s.env.clone();
    let Sim {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        provider,
        layer,
        effects,
        env,
        replacements,
        ..
    } = s;
    let mut ctx = EventContext {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        provider,
    };
    let (effects, layer): (&ContinuousEffects, &dyn LayerEnvironment) = (effects, layer);
    let mut post_batch = |after: &EventContext<'_>,
                          view: &mut dyn FnMut(
        &ContinuousEffects,
        &dyn LayerEnvironment,
        &dyn SbaEnvironment,
    )| {
        calls.set(calls.get() + 1);
        let mut post = view_env.clone();
        if let Some(g) = gate {
            let blocker_left = !after
                .zones
                .object(g.blocker)
                .is_some_and(|r| r.zone.kind() == ZoneKind::Battlefield);
            if blocker_left {
                post.illegal_protectors.remove(&(g.battle, g.player));
            }
        }
        view(effects, layer, &post);
    };
    match via {
        Via::Stage => sba_pass_post_batch(
            SbaStage {
                registry: &registry,
                ctx: &mut ctx,
                replacements,
                effects,
                layer_env: layer,
                env,
                post_batch: &mut post_batch,
            },
            c,
        ),
        Via::Direct => perform_all_post_batch(
            &registry,
            &mut ctx,
            replacements,
            effects,
            layer,
            env,
            c,
            &mut post_batch,
        ),
    }
}

/// Up to `passes` passes in one session (with `sync_after` between them), answering the
/// protector requests with `answers`; returns the outcomes, the final state hash, the final Sim
/// and the provider calls of the final run.
fn stage_session(
    base: &Sim,
    via: Via,
    passes: usize,
    answers: &[ChoiceResponse],
) -> (Vec<SbaOutcome>, fineprint_base::hash::Digest, Sim, usize) {
    stage_session_gated(base, via, passes, answers, None)
}

fn stage_session_gated(
    base: &Sim,
    via: Via,
    passes: usize,
    answers: &[ChoiceResponse],
    gate: Option<Gate>,
) -> (Vec<SbaOutcome>, fineprint_base::hash::Digest, Sim, usize) {
    let calls = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&calls);
    let procedure = move |s: &mut Sim, c: &mut ChoiceContext<'_>| {
        counter.set(0);
        let mut outs = Vec::new();
        for _ in 0..passes {
            match one_pass_gated(s, c, via, &counter, gate) {
                Ok(o) => {
                    s.sync_after(&o);
                    outs.push(o);
                }
                Err(BusInterrupt::Pending(r)) => return Err(Interrupt::Pending(r)),
                Err(BusInterrupt::Error(e)) => panic!("bus error {e:?}"),
            }
        }
        Ok(outs)
    };
    let session = ChoiceSession::replay(base.clone(), procedure, answers).expect("session runs");
    let hash = session.state_hash();
    match session.status() {
        Status::Done { state, output } => (output.clone(), hash, state.clone(), calls.get()),
        Status::Pending(r) => panic!("still pending: {r:?}"),
    }
}

/// REQ-TURN-001, REQ-SBA-001, CR 704.3, CR 704.5g (gate 02-state-objects R4): the standard SBA
/// stage of the heartbeat performs one pass through `perform_all_post_batch`: a creature with
/// lethal damage goes to the graveyard (performed), the next pass finds nothing (not
/// performed), the post-batch provider is never called for this board, and the outcomes and
/// state hash equal those of `sba::perform_all_post_batch` called directly.
#[test]
fn req_turn_001_sba_stage_lethal_damage() {
    let mut s = Sim::with_players(2);
    let c = s.creature(sba_support::P1, "test.vanilla_2_2", 2, 2);
    s.damage(c, 2);
    let (outs, hash, after, calls) = stage_session(&s, Via::Stage, 2, &[]);
    assert_eq!(outs.len(), 2);
    assert!(outs[0].performed);
    let moved = outs[0]
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .find_map(|e| match &e.outcome {
            Outcome::ZoneChange(m) if m.old == c => Some(m.new),
            _ => None,
        })
        .expect("the creature moved");
    assert_eq!(after.zone_of(moved), Some(ZoneKind::Graveyard));
    assert!(!after.on_battlefield(c));
    assert!(!outs[1].performed);
    assert!(outs[1].findings.is_empty());
    assert_eq!(calls, 0, "no retained battle: the provider is not called");

    let (d_outs, d_hash, d_after, d_calls) = stage_session(&s, Via::Direct, 2, &[]);
    assert_eq!(outs, d_outs);
    assert_eq!(hash, d_hash);
    assert_eq!(after.state_hash(), d_after.state_hash());
    assert_eq!(calls, d_calls);
}

/// REQ-TURN-001, REQ-SBA-001, CR 704.5v, CR 704.5x, CR 704.3, CR 614.6 (gate 02-state-objects
/// R4, DECISIONS "Gate 02-state-objects R3c"): a zero-defense Siege whose graveyard move a
/// replacement skips is retained; the stage calls the `PostBatchState` provider exactly once and
/// asks its controller for a protector; the outcome and hash equal the direct call's.
#[test]
fn req_turn_001_sba_stage_retained_battle_uses_post_batch_provider() {
    let mut s = Sim::with_players(3);
    let b = s.siege(sba_support::P1, sba_support::P1, 0, None);
    s.add_replacement(ReplacementEffect::new(eid(1), sba_move_of(b), Action::Skip));
    let answers = [submit_player(sba_support::P2)];
    let (outs, hash, after, calls) = stage_session(&s, Via::Stage, 1, &answers);
    assert_eq!(
        calls, 1,
        "the provider is called once for the retained battle"
    );
    assert_eq!(outs[0].protectors_chosen, vec![(b, sba_support::P2)]);
    assert!(outs[0].performed);
    assert!(after.on_battlefield(b));

    let (d_outs, d_hash, _, d_calls) = stage_session(&s, Via::Direct, 1, &answers);
    assert_eq!(outs, d_outs);
    assert_eq!(hash, d_hash);
    assert_eq!(d_calls, 1);
}

/// REQ-TURN-001, REQ-SBA-001, CR 704.5x, CR 704.3 (F20): the protector request of a retained
/// battle lists the candidates of the state AFTER the batch. P3 may not protect the Siege while
/// a creature is on the battlefield; that creature has lethal damage and leaves in the same
/// batch, and the provider's post-batch view lifts the restriction, so the request lists P2 and
/// P3 (a stage that used the pre-batch view would list only P2). Both paths agree.
#[test]
fn req_turn_001_sba_stage_protector_candidates_are_post_batch() {
    let mut s = Sim::with_players(3);
    let b = s.siege(sba_support::P1, sba_support::P1, 0, None);
    s.add_replacement(ReplacementEffect::new(eid(1), sba_move_of(b), Action::Skip));
    let blocker = s.creature(sba_support::P2, "test.vanilla_1_1", 1, 1);
    s.damage(blocker, 1);
    let p3 = PlayerId::new(3);
    s.env.illegal_protectors.insert((b, p3));
    let gate = Some(Gate {
        blocker,
        battle: b,
        player: p3,
    });
    for via in [Via::Stage, Via::Direct] {
        // The first run suspends on the protector request: read its candidates.
        let calls = Rc::new(Cell::new(0usize));
        let counter = Rc::clone(&calls);
        let procedure = move |sim: &mut Sim, c: &mut ChoiceContext<'_>| match one_pass_gated(
            sim, c, via, &counter, gate,
        ) {
            Ok(o) => Ok(o),
            Err(BusInterrupt::Pending(r)) => Err(Interrupt::Pending(r)),
            Err(BusInterrupt::Error(e)) => panic!("bus error {e:?}"),
        };
        let session = ChoiceSession::start(s.clone(), procedure).expect("session runs");
        let request = session.pending().cloned().expect("the protector request");
        assert_eq!(request.choice_type, ChoiceType::ChoosePlayer, "{via:?}");
        assert_eq!(request.chooser, sba_support::P1, "{via:?}");
        match &request.payload {
            ChoicePayload::ChoosePlayer { players } => {
                assert_eq!(
                    players,
                    &vec![sba_support::P2, p3],
                    "{via:?}: post-batch candidates"
                );
            }
            other => panic!("{via:?}: unexpected payload {other:?}"),
        }
        assert_eq!(calls.get(), 1, "{via:?}");
    }
    // Answering with the candidate only the post-batch view allows is accepted.
    let (outs, _, after, _) = stage_session_gated(&s, Via::Stage, 1, &[submit_player(p3)], gate);
    assert_eq!(outs[0].protectors_chosen, vec![(b, p3)]);
    assert!(!after.on_battlefield(blocker));
}

/// A heartbeat host over a real SBA world: `sba_pass` is the standard stage.
#[derive(Clone)]
struct SimHost {
    sim: Sim,
    passes: Vec<bool>,
}

impl HeartbeatHost for SimHost {
    fn game(&self) -> &GameState {
        &self.sim.game
    }
    fn game_mut(&mut self) -> &mut GameState {
        &mut self.sim.game
    }
    fn sba_pass(&mut self, c: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        let calls = Cell::new(0);
        let o = one_pass(&mut self.sim, c, Via::Stage, &calls)?;
        self.sim.sync_after(&o);
        self.passes.push(o.performed);
        Ok(o)
    }
    fn place_triggers(
        &mut self,
        _: &[PlayerId],
        _: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        Ok(TriggerPlacement::default())
    }
    fn stack_is_empty(&self) -> bool {
        true
    }
    fn resolve_top(&mut self, _: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        Err(Interrupt::Error(ChoiceError::Procedure(
            "the stack is empty".to_string(),
        )))
    }
    fn priority_actions(&self, _: PlayerId) -> Vec<ChoiceResponse> {
        Vec::new()
    }
    fn take_action(
        &mut self,
        _: PlayerId,
        _: &ChoiceResponse,
        _: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Ok(())
    }
}

#[derive(Clone)]
struct SimWorld {
    host: SimHost,
    hb: HeartbeatState,
}

impl Transactional for SimWorld {
    fn state_hash(&self) -> fineprint_base::hash::Digest {
        self.host.sim.state_hash()
    }
    fn priority_player(&self) -> Option<PlayerId> {
        self.host.sim.game.turn().priority_player()
    }
    fn carry_durable(restored: Self, _: &Self, _: &Self) -> Self {
        restored
    }
}

/// REQ-TURN-001, REQ-SBA-001, CR 704.3, CR 117.5, CR 117.4: the heartbeat over a real world: the
/// creature with lethal damage dies in the first settle (one performed pass, one empty pass),
/// then the players are asked; two passes end the step.
#[test]
fn req_turn_001_heartbeat_over_real_sba_world() {
    let mut sim = Sim::with_players(2);
    let c = sim.creature(sba_support::P1, "test.vanilla_2_2", 2, 2);
    sim.damage(c, 3);
    let base = SimWorld {
        host: SimHost {
            sim,
            passes: Vec::new(),
        },
        hb: HeartbeatState::new(),
    };
    let procedure = |w: &mut SimWorld, ctx: &mut ChoiceContext<'_>| {
        run_heartbeat(&mut w.host, &mut w.hb, ctx).map(out_of)
    };
    let session = ChoiceSession::replay(base, procedure, &[pass(), pass()]).expect("session runs");
    match session.status() {
        Status::Done { state, output } => {
            assert_eq!(*output, Out::StepEnds);
            assert_eq!(state.host.passes, vec![true, false]);
            assert!(!state.host.sim.on_battlefield(c));
            assert_eq!(state.hb.passes().len(), 2);
            assert_eq!(state.host.sim.game.turn().priority_player(), None);
        }
        Status::Pending(r) => panic!("still pending: {r:?}"),
    }
}
