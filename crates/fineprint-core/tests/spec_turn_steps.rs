// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests of the turn sequencer (REQ-TURN-002, SRS 5). Black-box: written from the
//! work-item brief only, against the public API of `fineprint_core::turn`. Every object is a
//! synthetic id (R5).
//!
//! CR 500.1: "A turn consists of five phases, in this order: beginning, precombat main, combat,
//! postcombat main, and ending. ... The beginning, combat, and ending phases are further broken
//! down into steps, which proceed in order."
//!
//! CR 500.4 / 500.5: effects that last until a step or phase expire as it begins; effects that
//! last until its end expire as it ends, then unspent mana empties (CR 703.4q, CR 106.4).

#[path = "spec_turn_steps_support.rs"]
mod support;

use fineprint_base::ids::PlayerId;
use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoicePayload, ChoiceResponse, ChoiceType, Interrupt,
};
use fineprint_core::events::{Phase, Step};
use fineprint_core::format::FirstTurnDrawRule;
use fineprint_core::turn::{
    ExpiryPoint, MAX_CLEANUP_STEPS, Segment, SegmentReport, TurnBasedAction, TurnBasedRegistry,
    TurnHost, first_turn_draw_skipped, segments_of,
};

use support::*;

fn standard() -> TurnBasedRegistry {
    TurnBasedRegistry::standard()
}

fn seg_report(r: &Rep) -> SegmentReport {
    match r {
        Rep::Segment(s) => *s,
        other => panic!("not a segment report: {other:?}"),
    }
}

fn phase_reports(r: &Rep) -> Vec<SegmentReport> {
    match r {
        Rep::Phase(s) => s.clone(),
        other => panic!("not a phase report: {other:?}"),
    }
}

fn assert_procedure_error(out: &Option<Out>) {
    match out {
        Some(Err(ChoiceError::Procedure(_))) => {}
        other => panic!("expected a ChoiceError::Procedure, got {other:?}"),
    }
}

fn prio_requests_match_log(run: &Run) {
    // Every priority ask is logged once (the heartbeat computes the actions just before it
    // asks); the choosers agree in order.
    let asked: Vec<PlayerId> = run
        .requests
        .iter()
        .filter(|r| r.choice_type == ChoiceType::PriorityAction)
        .map(|r| r.chooser)
        .collect();
    let logged: Vec<PlayerId> = run
        .calls()
        .iter()
        .filter_map(|c| match c {
            TC::Prio(p) => Some(*p),
            _ => None,
        })
        .collect();
    assert_eq!(asked, logged);
    for (i, r) in run.requests.iter().enumerate() {
        assert_eq!(r.request_id.get(), i as u64, "request ids are 0, 1, 2, ...");
    }
}

// =============================================================================================
// 1. Scripted two-player turn
// =============================================================================================

fn two_turns(first_strike: bool) -> (Run, Vec<TC>) {
    let mut fake = TurnFake::new(2).with_untap(P1, oids(&[3]), Vec::new());
    fake.first_strike = first_strike;
    let run = drive(
        World::new(fake),
        Proc::Turns(vec![P1, P2]),
        &standard(),
        Answers::none(),
    );
    // CR 103.8a: P1 plays first in a two-player game and skips the draw of turn 1; P2 draws on
    // turn 2. CR 502.2: the day/night check gets the previous turn's active player.
    let expected = cat(vec![
        quiet_turn(
            P1,
            P2,
            None,
            vec![TC::Untap(P1, oids(&[3]))],
            false,
            first_strike,
        ),
        quiet_turn(P2, P1, Some(P1), Vec::new(), true, first_strike),
    ]);
    (run, expected)
}

fn check_two_turns(first_strike: bool) {
    let (run, expected) = two_turns(first_strike);
    assert_eq!(run.calls(), expected);
    assert!(
        run.last.host.anomalies.is_empty(),
        "{:?}",
        run.last.host.anomalies
    );
    assert!(
        run.requests
            .iter()
            .all(|r| r.choice_type == ChoiceType::PriorityAction)
    );
    prio_requests_match_log(&run);
    let Rep::Turns(reports) = run.rep() else {
        panic!("not a turn report")
    };
    assert_eq!(reports.len(), 2);
    for (report, active) in reports.iter().zip([P1, P2]) {
        assert_eq!(report.active_player, active);
        let segs: Vec<Segment> = report.segments.iter().map(|s| s.segment).collect();
        assert_eq!(segs, turn_segments(first_strike));
        for s in &report.segments {
            let quiet_no_priority = matches!(
                s.segment,
                Segment::Step(Step::Untap) | Segment::Step(Step::Cleanup)
            );
            assert_eq!(s.heartbeat_ran, !quiet_no_priority, "{s:?}");
            let cleanups = u32::from(s.segment == Segment::Step(Step::Cleanup));
            assert_eq!(s.cleanup_steps, cleanups, "{s:?}");
        }
    }
    assert_eq!(run.last.ts.previous_active_player(), Some(P2));
    assert_eq!(run.last.host.game.turn().turn_number(), 2);
}

/// REQ-TURN-002; CR 500.1, 500.4, 500.5, 502.1-502.4, 504.1, 504.2, 103.8a, 514.3: two
/// players' turns (turns 1 and 2) in sequence pass through every step in order.
#[test]
fn scripted_two_player_turns_follow_the_full_call_order() {
    check_two_turns(false);
}

/// REQ-TURN-002; CR 500.1: with a first-strike damage step the combat phase has six steps.
#[test]
fn scripted_two_player_turns_with_first_strike_damage_step() {
    check_two_turns(true);
}

/// REQ-TURN-002; CR 502.2: `previous_active` is None on the first turn, then the previous
/// turn's active player.
#[test]
fn day_night_check_gets_the_previous_turns_active_player() {
    let run = drive(
        World::new(TurnFake::new(2)),
        Proc::Turns(vec![P1, P2, P1]),
        &standard(),
        Answers::none(),
    );
    let checks: Vec<TC> = run
        .calls()
        .into_iter()
        .filter(|c| matches!(c, TC::DayNight(_)))
        .collect();
    assert_eq!(
        checks,
        vec![
            TC::DayNight(None),
            TC::DayNight(Some(P1)),
            TC::DayNight(Some(P2))
        ]
    );
    assert_eq!(run.last.ts.previous_active_player(), Some(P1));
}

// =============================================================================================
// 2. "Expire, then mana" at every boundary
// =============================================================================================

/// REQ-TURN-002; CR 500.4, 500.5, 500.5a, 703.4q, 106.4: begin expiries immediately follow the
/// begin call, every end expiry is immediately followed by emptying mana pools, and "until end
/// of combat" expires at the end of the combat phase, after the end of combat step ends.
#[test]
fn expire_then_mana_at_every_boundary() {
    for fs in [false, true] {
        let (run, _) = two_turns(fs);
        let log = run.calls();
        let mut ends = 0usize;
        for (i, c) in log.iter().enumerate() {
            match c {
                TC::Expire(ExpiryPoint::EndStep(_)) | TC::Expire(ExpiryPoint::EndPhase(_)) => {
                    ends += 1;
                    assert_eq!(log.get(i + 1), Some(&TC::EmptyMana), "after {c:?} at {i}");
                }
                TC::Expire(ExpiryPoint::BeginStep(s)) => {
                    assert_eq!(log[i - 1], TC::BeginStep(*s), "before {c:?} at {i}");
                }
                TC::Expire(ExpiryPoint::BeginPhase(p)) => {
                    assert_eq!(log[i - 1], TC::BeginPhase(*p), "before {c:?} at {i}");
                }
                TC::EmptyMana => {
                    assert!(
                        matches!(
                            log[i - 1],
                            TC::Expire(ExpiryPoint::EndStep(_))
                                | TC::Expire(ExpiryPoint::EndPhase(_))
                        ),
                        "mana empties only right after an end expiry (at {i})"
                    );
                }
                TC::BeginStep(s) => {
                    assert_eq!(
                        log.get(i + 1),
                        Some(&TC::Expire(ExpiryPoint::BeginStep(*s)))
                    );
                }
                TC::BeginPhase(p) => {
                    assert_eq!(
                        log.get(i + 1),
                        Some(&TC::Expire(ExpiryPoint::BeginPhase(*p)))
                    );
                }
                _ => {}
            }
        }
        // Per turn: 5 phases plus the steps (10 or 11) end, each once.
        let steps_per_turn = if fs { 11 } else { 10 };
        assert_eq!(ends, 2 * (5 + steps_per_turn));
        assert_eq!(count(&log, &TC::EmptyMana), ends);
        // CR 500.5a: EndPhase(Combat) follows the end of the end of combat step.
        let eoc = TC::Expire(ExpiryPoint::EndStep(Step::EndOfCombat));
        let combat_end = TC::Expire(ExpiryPoint::EndPhase(Phase::Combat));
        assert_eq!(count(&log, &eoc), 2);
        assert_eq!(count(&log, &combat_end), 2);
        for (i, c) in log.iter().enumerate() {
            if *c == eoc {
                assert_eq!(log[i + 1], TC::EmptyMana);
                assert_eq!(log[i + 2], combat_end);
                assert_eq!(log[i + 3], TC::EmptyMana);
            }
        }
    }
}

// =============================================================================================
// 3. Untap
// =============================================================================================

/// REQ-TURN-002; CR 502.1, 502.2, 502.3, 502.4, 117.3a: phasing, then day/night, then untap;
/// no settle and no priority in the untap step, a trigger waiting then is put on the stack in
/// the upkeep.
#[test]
fn untap_step_order_and_no_priority_with_a_waiting_trigger() {
    let fake = TurnFake::new(2)
        .at(3, Some(P1))
        .with_untap(P1, oids(&[4]), Vec::new())
        .with_place(vec![1]);
    let run = drive(
        World::new(fake),
        Proc::Phase(Phase::Beginning),
        &standard(),
        Answers::none(),
    );
    let order = [P1, P2];
    let expected = phase(
        Phase::Beginning,
        cat(vec![
            step(
                Step::Untap,
                vec![
                    TC::Phasing(P1),
                    TC::DayNight(None),
                    TC::UntapOptions(P1),
                    TC::Untap(P1, oids(&[4])),
                ],
            ),
            step(Step::Upkeep, hb_with_trigger(&order)),
            step(Step::Draw, cat(vec![vec![TC::Draw(P1)], hb(&order)])),
        ]),
    );
    assert_eq!(run.calls(), expected);
    assert!(run.last.host.anomalies.is_empty());
    let reports = phase_reports(&run.rep());
    assert_eq!(
        reports,
        vec![
            SegmentReport {
                segment: Segment::Step(Step::Untap),
                heartbeat_ran: false,
                cleanup_steps: 0
            },
            SegmentReport {
                segment: Segment::Step(Step::Upkeep),
                heartbeat_ran: true,
                cleanup_steps: 0
            },
            SegmentReport {
                segment: Segment::Step(Step::Draw),
                heartbeat_ran: true,
                cleanup_steps: 0
            },
        ]
    );
    // The first priority request goes to the active player in the upkeep.
    assert_eq!(run.requests[0].choice_type, ChoiceType::PriorityAction);
    assert_eq!(run.requests[0].chooser, P1);
    prio_requests_match_log(&run);
}

fn untap_world(forced: &[u64], choosable: &[u64]) -> World {
    World::new(
        TurnFake::new(2)
            .at(2, Some(P2))
            .with_untap(P2, oids(forced), oids(choosable)),
    )
}

fn untap_run(forced: &[u64], choosable: &[u64], answers: Answers) -> Run {
    drive(
        untap_world(forced, choosable),
        Proc::Segment(Segment::Step(Step::Untap)),
        &standard(),
        answers,
    )
}

fn untap_calls(run: &Run) -> Vec<TC> {
    run.calls()
        .into_iter()
        .filter(|c| matches!(c, TC::Untap(..)))
        .collect()
}

/// REQ-TURN-002; CR 502.3, 502.4: with nothing choosable there is no request and one untap
/// call with the forced permanents; nothing at all to untap means no untap call.
#[test]
fn untap_without_choosable_permanents_asks_nothing() {
    let run = untap_run(&[2, 6], &[], Answers::none());
    assert!(run.requests.is_empty());
    assert_eq!(
        run.calls(),
        step(
            Step::Untap,
            vec![
                TC::Phasing(P2),
                TC::DayNight(None),
                TC::UntapOptions(P2),
                TC::Untap(P2, oids(&[2, 6])),
            ]
        )
    );
    assert_eq!(
        run.segment(),
        SegmentReport {
            segment: Segment::Step(Step::Untap),
            heartbeat_ran: false,
            cleanup_steps: 0
        }
    );

    let run = untap_run(&[], &[], Answers::none());
    assert!(run.requests.is_empty());
    assert!(untap_calls(&run).is_empty());
    assert_eq!(
        run.calls(),
        step(
            Step::Untap,
            vec![TC::Phasing(P2), TC::DayNight(None), TC::UntapOptions(P2)]
        )
    );
}

/// REQ-TURN-002; CR 502.3: with nothing choosable the brief specifies `untap(active, &forced)`:
/// the forced list exactly as the host reported it (no reordering is specified on this path,
/// unlike the choice path, which is "ascending, deduplicated").
#[test]
fn untap_forced_only_passes_the_forced_list_unchanged() {
    let run = untap_run(&[6, 2], &[], Answers::none());
    assert!(run.requests.is_empty());
    assert_eq!(untap_calls(&run), vec![TC::Untap(P2, oids(&[6, 2]))]);
}

/// REQ-TURN-002; CR 502.3: choosable permanents give exactly one CHOOSE_UNTAP_SET
/// request with the specified fields.
#[test]
fn untap_choice_request_fields() {
    let (session, requests) = drive_until(
        untap_world(&[2, 8], &[3, 5, 8]),
        Proc::Segment(Segment::Step(Step::Untap)),
        &standard(),
        Answers::none(),
        |_| true,
    );
    assert_eq!(requests.len(), 1);
    let r = session.pending().expect("pending").clone();
    assert_eq!(r.request_id.get(), 0);
    assert_eq!(r.choice_type, ChoiceType::ChooseUntapSet);
    assert_eq!(r.chooser, P2);
    assert_eq!(r.reason.cr, "CR 502.3");
    assert_eq!(r.min, 0);
    assert_eq!(r.max, 3);
    assert_eq!(
        r.payload,
        ChoicePayload::ChooseUntapSet {
            permanents: oids(&[3, 5, 8])
        }
    );
    assert_eq!(
        r.default_on_timeout,
        ChoiceResponse::SubmitSet {
            objects: oids(&[3, 5, 8])
        }
    );
}

/// REQ-TURN-002; CR 502.3: the untap call gets forced united with the chosen, ascending,
/// deduplicated; none, a subset and all are valid answers; no priority in the untap step.
#[test]
fn untap_choice_answers_are_united_with_forced() {
    let cases: Vec<(Vec<u64>, Vec<u64>)> = vec![
        (vec![], vec![2, 8]),
        (vec![5, 8], vec![2, 5, 8]),
        (vec![8, 3], vec![2, 3, 8]),
        (vec![3, 5, 8], vec![2, 3, 5, 8]),
    ];
    for (answer, untapped) in cases {
        let run = untap_run(&[8, 2], &[3, 5, 8], Answers::none().untap(oids(&answer)));
        assert_eq!(run.types(), vec![ChoiceType::ChooseUntapSet], "{answer:?}");
        assert_eq!(
            run.calls(),
            step(
                Step::Untap,
                vec![
                    TC::Phasing(P2),
                    TC::DayNight(None),
                    TC::UntapOptions(P2),
                    TC::Untap(P2, oids(&untapped)),
                ]
            ),
            "{answer:?}"
        );
        assert!(!run.segment().heartbeat_ran);
    }
}

/// REQ-TURN-002; CR 502.3: the timeout default untaps everything; an empty union makes no
/// untap call.
#[test]
fn untap_default_and_empty_union() {
    let (mut session, _) = drive_until(
        untap_world(&[9], &[3, 5]),
        Proc::Segment(Segment::Step(Step::Untap)),
        &standard(),
        Answers::none(),
        |_| true,
    );
    let id = session.pending().expect("pending").request_id;
    session.submit_default(id).expect("the default is accepted");
    let run = drive_rest(session, Vec::new(), Answers::none(), false);
    assert_eq!(untap_calls(&run), vec![TC::Untap(P2, oids(&[3, 5, 9]))]);

    let run = untap_run(&[], &[3, 5], Answers::none().untap(Vec::new()));
    assert_eq!(run.types(), vec![ChoiceType::ChooseUntapSet]);
    assert!(untap_calls(&run).is_empty());
}

/// REQ-TURN-002; CR 502.3; R12: answers that are not a subset of the offered permanents, or of
/// the wrong kind, are rejected by the session without a panic and change nothing.
#[test]
fn untap_wrong_answers_are_rejected() {
    let (mut session, _) = drive_until(
        untap_world(&[], &[3, 5]),
        Proc::Segment(Segment::Step(Step::Untap)),
        &standard(),
        Answers::none(),
        |_| true,
    );
    let pending = session.pending().expect("pending").clone();
    let hash = session.state_hash();
    let bad = vec![
        ChoiceResponse::SubmitSet {
            objects: oids(&[4]),
        },
        ChoiceResponse::SubmitSet {
            objects: oids(&[3, 3]),
        },
        ChoiceResponse::SubmitSet {
            objects: oids(&[3, 5, 7]),
        },
        ChoiceResponse::PassPriority,
        ChoiceResponse::SubmitObjectSelection {
            objects: oids(&[3]),
        },
    ];
    for answer in bad {
        assert!(
            session.submit(pending.request_id, answer.clone()).is_err(),
            "{answer:?} accepted"
        );
        assert_eq!(session.pending(), Some(&pending));
        assert_eq!(session.state_hash(), hash);
    }
}

// =============================================================================================
// 4. Draw
// =============================================================================================

fn expected_skip(
    rule: FirstTurnDrawRule,
    players: usize,
    turn: u64,
    starting_is_active: bool,
) -> bool {
    turn == 1
        && starting_is_active
        && match rule {
            FirstTurnDrawRule::AlwaysSkip => true,
            FirstTurnDrawRule::CrDefault => players == 2,
            FirstTurnDrawRule::NeverSkip => false,
        }
}

/// REQ-TURN-002; CR 103.8a, 103.8c, 504.1, 504.2: the draw-rule matrix over rule x player count
/// x turn x starting player; the heartbeat runs in the draw step whether or not a card is
/// drawn.
#[test]
fn draw_rule_matrix() {
    let rules = [
        FirstTurnDrawRule::CrDefault,
        FirstTurnDrawRule::NeverSkip,
        FirstTurnDrawRule::AlwaysSkip,
    ];
    let mut cases = 0;
    for rule in rules {
        for players in [2usize, 3] {
            for turn in [1u64, 2] {
                // (starting player, whether the active player is it)
                for (starting, is_active) in [(Some(P1), true), (Some(P2), false), (None, false)] {
                    cases += 1;
                    let mut fake = TurnFake::new(players).at(turn, Some(P1));
                    fake.draw_rule = rule;
                    fake.starting = starting;
                    let skip = expected_skip(rule, players, turn, is_active);
                    assert_eq!(
                        first_turn_draw_skipped(&fake, P1),
                        skip,
                        "{rule:?} {players} players turn {turn} starting {starting:?}"
                    );
                    let run = drive(
                        World::new(fake),
                        Proc::Segment(Segment::Step(Step::Draw)),
                        &standard(),
                        Answers::none(),
                    );
                    let order: Vec<PlayerId> = (1..=players as u64).map(PlayerId::new).collect();
                    let mut body = Vec::new();
                    if !skip {
                        body.push(TC::Draw(P1));
                    }
                    body.extend(hb(&order));
                    assert_eq!(
                        run.calls(),
                        step(Step::Draw, body),
                        "{rule:?} {players} players turn {turn} starting {starting:?}"
                    );
                    assert_eq!(
                        run.segment(),
                        SegmentReport {
                            segment: Segment::Step(Step::Draw),
                            heartbeat_ran: true,
                            cleanup_steps: 0
                        }
                    );
                    assert_eq!(run.requests.len(), players);
                    assert_eq!(run.requests[0].chooser, P1);
                }
            }
        }
    }
    assert_eq!(cases, 36);
}

/// REQ-TURN-002; CR 103.8a, 103.8c: `first_turn_draw_skipped` for a non-starting active player
/// on turn 1 and for the starting player in 3-player CR-default games is false.
#[test]
fn draw_skip_only_for_the_starting_player() {
    let mut fake = TurnFake::new(2).at(1, Some(P2));
    assert!(!first_turn_draw_skipped(&fake, P2));
    assert!(first_turn_draw_skipped(&fake, P1));
    fake.draw_rule = FirstTurnDrawRule::NeverSkip;
    assert!(!first_turn_draw_skipped(&fake, P1));
    let mut fake3 = TurnFake::new(3).at(1, Some(P1));
    assert!(!first_turn_draw_skipped(&fake3, P1));
    fake3.draw_rule = FirstTurnDrawRule::AlwaysSkip;
    assert!(first_turn_draw_skipped(&fake3, P1));
    assert!(!first_turn_draw_skipped(&fake3, P3));
}

// =============================================================================================
// 5. Main phases and the turn-based-action registry
// =============================================================================================

fn custom_pre_a(
    host: &mut dyn TurnHost,
    active: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    host.add_lore_counters(&[oid(4100 + active.get())], choices)
}

fn custom_pre_b(
    host: &mut dyn TurnHost,
    active: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    host.add_lore_counters(&[oid(4200 + active.get())], choices)
}

fn custom_post(
    host: &mut dyn TurnHost,
    active: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    host.add_lore_counters(&[oid(4300 + active.get())], choices)
}

fn action(
    id: &'static str,
    segment: Segment,
    run: fn(&mut dyn TurnHost, PlayerId, &mut ChoiceContext<'_>) -> Result<(), Interrupt>,
) -> TurnBasedAction {
    TurnBasedAction {
        id,
        cr: "CR 703.1",
        segment,
        run,
    }
}

fn ids(entries: &[TurnBasedAction]) -> Vec<&'static str> {
    entries.iter().map(|e| e.id).collect()
}

/// REQ-TURN-002; CR 505.4, 715.3c: lore counters on the precombat main phase only, one call
/// with every saga, then the heartbeat; none in the postcombat main phase.
#[test]
fn lore_counters_on_the_precombat_main_phase_only() {
    let order = [P1, P2];
    let fake = TurnFake::new(2)
        .at(2, Some(P1))
        .with_sagas(P1, oids(&[4, 6, 11]));
    let run = drive(
        World::new(fake.clone()),
        Proc::Phase(Phase::PrecombatMain),
        &standard(),
        Answers::none(),
    );
    assert_eq!(
        run.calls(),
        phase(
            Phase::PrecombatMain,
            cat(vec![vec![TC::Lore(oids(&[4, 6, 11]))], hb(&order)])
        )
    );
    assert_eq!(
        phase_reports(&run.rep()),
        vec![SegmentReport {
            segment: Segment::PrecombatMain,
            heartbeat_ran: true,
            cleanup_steps: 0
        }]
    );
    let run = drive(
        World::new(fake),
        Proc::Phase(Phase::PostcombatMain),
        &standard(),
        Answers::none(),
    );
    assert_eq!(run.calls(), phase(Phase::PostcombatMain, hb(&order)));
}

/// REQ-TURN-002; CR 505.4: no sagas (or only the other player's) means no lore call.
#[test]
fn no_lore_counters_without_sagas() {
    let fake = TurnFake::new(2).at(2, Some(P1)).with_sagas(P2, oids(&[7]));
    let run = drive(
        World::new(fake),
        Proc::Segment(Segment::PrecombatMain),
        &standard(),
        Answers::none(),
    );
    assert_eq!(run.calls(), hb(&[P1, P2]));
}

/// REQ-TURN-002; CR 703.1, 505.4: the registry: `new` is empty, `standard` has exactly the
/// lore-counter entry, a duplicate id is an error, `entries_for` keeps registration order.
#[test]
fn turn_based_registry_contents() {
    let empty = TurnBasedRegistry::new();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    let default = TurnBasedRegistry::default();
    assert!(default.is_empty());

    let std_reg = TurnBasedRegistry::standard();
    assert_eq!(std_reg.len(), 1);
    assert!(!std_reg.is_empty());
    let pre = std_reg.entries_for(Segment::PrecombatMain);
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].id, "lore_counters");
    assert_eq!(pre[0].cr, "CR 505.4");
    assert_eq!(pre[0].segment, Segment::PrecombatMain);
    for phase in [
        Phase::Beginning,
        Phase::Combat,
        Phase::PostcombatMain,
        Phase::Ending,
    ] {
        for fs in [false, true] {
            for s in segments_of(phase, fs) {
                assert!(std_reg.entries_for(s).is_empty(), "{s:?}");
            }
        }
    }

    let mut reg = TurnBasedRegistry::standard();
    reg.register(action(
        "test.custom_a",
        Segment::PrecombatMain,
        custom_pre_a,
    ))
    .expect("new id");
    reg.register(action(
        "test.custom_post",
        Segment::PostcombatMain,
        custom_post,
    ))
    .expect("new id");
    reg.register(action(
        "test.custom_b",
        Segment::PrecombatMain,
        custom_pre_b,
    ))
    .expect("new id");
    assert_eq!(reg.len(), 4);
    assert!(
        reg.register(action(
            "test.custom_a",
            Segment::PostcombatMain,
            custom_post
        ))
        .is_err()
    );
    assert!(
        reg.register(action(
            "lore_counters",
            Segment::PrecombatMain,
            custom_pre_b
        ))
        .is_err()
    );
    assert_eq!(reg.len(), 4);
    assert_eq!(
        ids(&reg.entries_for(Segment::PrecombatMain)),
        vec!["lore_counters", "test.custom_a", "test.custom_b"]
    );
    assert_eq!(
        ids(&reg.entries_for(Segment::PostcombatMain)),
        vec!["test.custom_post"]
    );
    // The clone is independent.
    let clone = reg.clone();
    assert_eq!(clone.len(), 4);
}

/// REQ-TURN-002; CR 703.1, 505.4: custom entries run after lore counters in registration
/// order, with the active player, before the heartbeat; postcombat entries run there.
#[test]
fn custom_turn_based_actions_run_in_registration_order() {
    let mut reg = TurnBasedRegistry::standard();
    reg.register(action(
        "test.custom_a",
        Segment::PrecombatMain,
        custom_pre_a,
    ))
    .expect("new id");
    reg.register(action(
        "test.custom_post",
        Segment::PostcombatMain,
        custom_post,
    ))
    .expect("new id");
    reg.register(action(
        "test.custom_b",
        Segment::PrecombatMain,
        custom_pre_b,
    ))
    .expect("new id");
    let fake = TurnFake::new(2).at(2, Some(P2)).with_sagas(P2, oids(&[6]));
    let order = [P2, P1];
    let run = drive(
        World::new(fake.clone()),
        Proc::Phase(Phase::PrecombatMain),
        &reg,
        Answers::none(),
    );
    assert_eq!(
        run.calls(),
        phase(
            Phase::PrecombatMain,
            cat(vec![
                vec![
                    TC::Lore(oids(&[6])),
                    TC::Lore(oids(&[4102])),
                    TC::Lore(oids(&[4202])),
                ],
                hb(&order)
            ])
        )
    );
    let run = drive(
        World::new(fake),
        Proc::Phase(Phase::PostcombatMain),
        &reg,
        Answers::none(),
    );
    assert_eq!(
        run.calls(),
        phase(
            Phase::PostcombatMain,
            cat(vec![vec![TC::Lore(oids(&[4302]))], hb(&order)])
        )
    );
    // An empty registry runs no lore counters at all.
    let run = drive(
        World::new(TurnFake::new(2).at(2, Some(P2)).with_sagas(P2, oids(&[6]))),
        Proc::Segment(Segment::PrecombatMain),
        &TurnBasedRegistry::new(),
        Answers::none(),
    );
    assert_eq!(run.calls(), hb(&order));
}

// =============================================================================================
// 6. Cleanup
// =============================================================================================

const ORDER: [PlayerId; 2] = [P1, P2];

fn cleanup_fake() -> TurnFake {
    TurnFake::new(2).at(2, Some(P1))
}

fn cleanup_run(fake: TurnFake, answers: Answers) -> Run {
    drive(
        World::new(fake),
        Proc::Segment(Segment::Step(Step::Cleanup)),
        &standard(),
        answers,
    )
}

fn cleanup_report(steps: u32, heartbeat: bool) -> SegmentReport {
    SegmentReport {
        segment: Segment::Step(Step::Cleanup),
        heartbeat_ran: heartbeat,
        cleanup_steps: steps,
    }
}

fn quiet_cleanup() -> Vec<TC> {
    step(Step::Cleanup, quiet_cleanup_body(&ORDER))
}

/// REQ-TURN-002; CR 402.2, 514.1, 514.2, 514.3: a hand at or under the maximum (or no maximum)
/// gives no request and no discard; `end_of_turn_cleanup` still runs; with nothing waiting
/// there is one cleanup step and no priority.
#[test]
fn cleanup_without_excess_cards() {
    for (hand, max) in [
        (vec![1, 2, 3, 4, 5, 6, 7], Some(7)),
        (vec![], Some(0)),
        (vec![1, 2], Some(5)),
        ((1..=12).collect::<Vec<u64>>(), None),
    ] {
        let fake = cleanup_fake().with_hand(P1, oids(&hand)).with_max(P1, max);
        let run = cleanup_run(fake, Answers::none());
        assert!(run.requests.is_empty(), "{hand:?} {max:?}");
        assert_eq!(run.calls(), quiet_cleanup(), "{hand:?} {max:?}");
        assert_eq!(run.segment(), cleanup_report(1, false));
        assert_eq!(run.last.host.hand(P1), oids(&hand));
    }
}

/// REQ-TURN-002; CR 514.1, 402.2: an excess of k cards gives one SELECT_OBJECTS_FROM_ZONE
/// request with min = max = k over the hand; the chosen cards are discarded ascending, then
/// the damage/"until end of turn" cleanup runs.
#[test]
fn cleanup_discards_down_to_the_maximum() {
    let fake = cleanup_fake()
        .with_hand(P1, oids(&[2, 5, 9, 12, 14]))
        .with_max(P1, Some(2));
    let (session, _) = drive_until(
        World::new(fake.clone()),
        Proc::Segment(Segment::Step(Step::Cleanup)),
        &standard(),
        Answers::none(),
        |_| true,
    );
    let r = session.pending().expect("pending").clone();
    assert_eq!(r.request_id.get(), 0);
    assert_eq!(r.choice_type, ChoiceType::SelectObjectsFromZone);
    assert_eq!(r.chooser, P1);
    assert_eq!(r.reason.cr, "CR 514.1");
    assert_eq!(r.min, 3);
    assert_eq!(r.max, 3);
    assert_eq!(
        r.payload,
        ChoicePayload::SelectObjectsFromZone {
            candidates: oids(&[2, 5, 9, 12, 14])
        }
    );

    let run = cleanup_run(fake, Answers::none().discard(oids(&[14, 2, 9])));
    assert_eq!(run.types(), vec![ChoiceType::SelectObjectsFromZone]);
    assert_eq!(
        run.calls(),
        step(
            Step::Cleanup,
            vec![
                TC::Discard(P1, oids(&[2, 9, 14])),
                TC::EotCleanup,
                TC::Sba,
                TC::Place(ORDER.to_vec()),
            ]
        )
    );
    assert_eq!(run.last.host.hand(P1), oids(&[5, 12]));
    assert_eq!(run.segment(), cleanup_report(1, false));
    assert!(run.last.host.anomalies.is_empty());

    // Maximum hand size 0 (CR 402.2): the whole hand.
    let fake = cleanup_fake()
        .with_hand(P1, oids(&[3, 4]))
        .with_max(P1, Some(0));
    let run = cleanup_run(fake, Answers::none());
    assert_eq!(run.requests[0].min, 2);
    assert!(run.calls().contains(&TC::Discard(P1, oids(&[3, 4]))));
    assert!(run.last.host.hand(P1).is_empty());

    // Only the active player discards (CR 514.1).
    let fake = cleanup_fake()
        .with_hand(P2, (1..=10).map(oid).collect())
        .with_hand(P1, oids(&[50]));
    let run = cleanup_run(fake, Answers::none());
    assert!(run.requests.is_empty());
    assert_eq!(run.calls(), quiet_cleanup());
}

/// REQ-TURN-002; CR 514.3a: a trigger put on the stack during the cleanup settle forces the
/// heartbeat (active player first), then another cleanup step.
#[test]
fn cleanup_repeats_when_a_trigger_fires() {
    let fake = cleanup_fake().with_cleanup_work(vec![Some(Work::Trigger)]);
    let run = cleanup_run(fake, Answers::none());
    let first_body = cat(vec![
        vec![
            TC::EotCleanup,
            TC::Sba,
            TC::Place(ORDER.to_vec()),
            TC::Sba,
            TC::Place(ORDER.to_vec()),
        ],
        hb_with_stack(&ORDER),
    ]);
    assert_eq!(
        run.calls(),
        cat(vec![step(Step::Cleanup, first_body), quiet_cleanup()])
    );
    assert_eq!(run.segment(), cleanup_report(2, true));
    assert_eq!(run.requests.len(), 4);
    assert_eq!(run.requests[0].chooser, P1);
    assert!(
        run.requests
            .iter()
            .all(|r| r.choice_type == ChoiceType::PriorityAction)
    );
    prio_requests_match_log(&run);
    assert!(run.last.host.anomalies.is_empty());
    assert_eq!(run.last.host.depth, 0);
}

/// REQ-TURN-002; CR 514.1, 514.3a: per cleanup step the order is begin, begin expiry, discard,
/// cleanup, settle, heartbeat, end expiry, empty mana; the second step discards nothing more.
#[test]
fn cleanup_repeat_with_discard_keeps_the_per_step_order() {
    let fake = cleanup_fake()
        .with_hand(P1, (1..=9).map(oid).collect())
        .with_cleanup_work(vec![Some(Work::Trigger)]);
    let run = cleanup_run(fake, Answers::none().discard(oids(&[8, 1])));
    let first_body = cat(vec![
        vec![
            TC::Discard(P1, oids(&[1, 8])),
            TC::EotCleanup,
            TC::Sba,
            TC::Place(ORDER.to_vec()),
            TC::Sba,
            TC::Place(ORDER.to_vec()),
        ],
        hb_with_stack(&ORDER),
    ]);
    assert_eq!(
        run.calls(),
        cat(vec![step(Step::Cleanup, first_body), quiet_cleanup()])
    );
    assert_eq!(
        run.requests[0].choice_type,
        ChoiceType::SelectObjectsFromZone
    );
    assert_eq!(run.requests.len(), 5);
    assert_eq!(run.segment(), cleanup_report(2, true));
}

/// REQ-TURN-002; CR 514.3a: an SBA performed on the settle also forces priority and a repeat;
/// a trigger then an SBA give three cleanup steps.
#[test]
fn cleanup_repeats_after_a_state_based_action() {
    let fake = cleanup_fake().with_cleanup_work(vec![Some(Work::Sba)]);
    let run = cleanup_run(fake, Answers::none());
    let first_body = cat(vec![
        vec![TC::EotCleanup, TC::Sba, TC::Sba, TC::Place(ORDER.to_vec())],
        hb(&ORDER),
    ]);
    assert_eq!(
        run.calls(),
        cat(vec![step(Step::Cleanup, first_body), quiet_cleanup()])
    );
    assert_eq!(run.segment(), cleanup_report(2, true));

    let fake = cleanup_fake().with_cleanup_work(vec![Some(Work::Trigger), Some(Work::Sba)]);
    let run = cleanup_run(fake, Answers::none());
    assert_eq!(run.segment(), cleanup_report(3, true));
    assert_eq!(count(&run.calls(), &TC::BeginStep(Step::Cleanup)), 3);
    assert_eq!(count(&run.calls(), &TC::EotCleanup), 3);
    assert!(run.calls().ends_with(&quiet_cleanup()));
}

/// REQ-TURN-002; CR 514.3a: a non-empty stack at the check forces the heartbeat too.
#[test]
fn cleanup_repeats_with_a_non_empty_stack() {
    let fake = cleanup_fake().with_cleanup_work(vec![Some(Work::Stack)]);
    let run = cleanup_run(fake, Answers::none());
    let first_body = cat(vec![quiet_cleanup_body(&ORDER), hb_with_stack(&ORDER)]);
    assert_eq!(
        run.calls(),
        cat(vec![step(Step::Cleanup, first_body), quiet_cleanup()])
    );
    assert_eq!(run.segment(), cleanup_report(2, true));
    assert_eq!(run.last.host.depth, 0);
}

/// REQ-TURN-002; CR 514.3a: the number of repeats equals the number of times there is work
/// (0, 1, 3).
#[test]
fn cleanup_repeat_count_follows_the_work() {
    let scripts: Vec<Vec<Option<Work>>> = vec![
        vec![],
        vec![Some(Work::Stack)],
        vec![Some(Work::Trigger), Some(Work::Sba), Some(Work::Stack)],
    ];
    for work in scripts {
        let repeats = work.len() as u32;
        let fake = cleanup_fake().with_cleanup_work(work.clone());
        let run = cleanup_run(fake, Answers::none());
        assert_eq!(
            run.segment(),
            cleanup_report(repeats + 1, repeats > 0),
            "{work:?}"
        );
        let log = run.calls();
        assert_eq!(
            count(&log, &TC::BeginStep(Step::Cleanup)),
            repeats as usize + 1
        );
        assert_eq!(
            count(&log, &TC::Expire(ExpiryPoint::EndStep(Step::Cleanup))),
            repeats as usize + 1
        );
        assert_eq!(count(&log, &TC::EmptyMana), repeats as usize + 1);
        assert_eq!(count(&log, &TC::EotCleanup), repeats as usize + 1);
        assert!(log.ends_with(&quiet_cleanup()));
        // The repeats are complete steps: begin ... end expiry, empty mana.
        for n in 0..=repeats as usize {
            let chunk = between(
                &log,
                &TC::BeginStep(Step::Cleanup),
                n,
                &TC::BeginStep(Step::Cleanup),
            );
            assert_eq!(chunk[1], TC::Expire(ExpiryPoint::BeginStep(Step::Cleanup)));
            assert_eq!(chunk[2], TC::EotCleanup);
            let len = chunk.len();
            assert_eq!(
                chunk[len - 2],
                TC::Expire(ExpiryPoint::EndStep(Step::Cleanup))
            );
            assert_eq!(chunk[len - 1], TC::EmptyMana);
            let prios = chunk.iter().filter(|c| matches!(c, TC::Prio(_))).count();
            if (n as u32) < repeats {
                assert!(prios > 0, "step {n} of {work:?} must give priority");
            } else {
                assert_eq!(prios, 0, "the last step of {work:?} gives no priority");
            }
        }
    }
}

/// REQ-TURN-002; CR 514.3a: more than MAX_CLEANUP_STEPS cleanup steps is a procedure error.
/// The fake keeps an object on the stack forever and every player has left the game, so each
/// heartbeat ends at once with no request (keeps the 10 000 iterations fast).
#[test]
fn cleanup_guard_stops_endless_repetition() {
    let mut fake = cleanup_fake();
    fake.everyone_left = true;
    fake.cleanup_work_forever = Some(Work::Stack);
    let run = cleanup_run(fake, Answers::none());
    assert_procedure_error(&run.out);
    assert!(run.requests.is_empty());
}

/// REQ-TURN-002; CR 514.3a: exactly MAX_CLEANUP_STEPS cleanup steps is still allowed.
#[test]
fn cleanup_guard_allows_exactly_the_maximum() {
    let mut fake = cleanup_fake();
    fake.everyone_left = true;
    fake.cleanup_work = (1..MAX_CLEANUP_STEPS).map(|_| Some(Work::Sba)).collect();
    let run = cleanup_run(fake, Answers::none());
    let report = run.segment();
    assert_eq!(report.segment, Segment::Step(Step::Cleanup));
    assert_eq!(report.cleanup_steps, MAX_CLEANUP_STEPS);
    assert_eq!(
        count(&run.calls(), &TC::BeginStep(Step::Cleanup)),
        MAX_CLEANUP_STEPS as usize
    );
}

// =============================================================================================
// 7. Boundaries per phase, run_segment alone, no active player
// =============================================================================================

/// REQ-TURN-002; CR 500.1, 500.4, 500.5, 500.5a: `run_phase` for each of the five phases.
#[test]
fn run_phase_boundaries_for_each_phase() {
    for fs in [false, true] {
        let mut fake = TurnFake::new(2).at(2, Some(P1));
        fake.first_strike = fs;
        let full = quiet_turn(P1, P2, None, Vec::new(), true, fs);
        // Split the full quiet turn into its five phase blocks.
        let mut blocks = Vec::new();
        for p in [
            Phase::Beginning,
            Phase::PrecombatMain,
            Phase::Combat,
            Phase::PostcombatMain,
            Phase::Ending,
        ] {
            let start = full
                .iter()
                .position(|c| *c == TC::BeginPhase(p))
                .expect("phase present");
            let end = full
                .iter()
                .position(|c| *c == TC::Expire(ExpiryPoint::EndPhase(p)))
                .expect("phase end present")
                + 2;
            blocks.push((p, full[start..end].to_vec()));
        }
        for (p, expected) in blocks {
            let run = drive(
                World::new(fake.clone()),
                Proc::Phase(p),
                &standard(),
                Answers::none(),
            );
            assert_eq!(run.calls(), expected, "{p:?} fs {fs}");
            let segs: Vec<Segment> = phase_reports(&run.rep())
                .iter()
                .map(|s| s.segment)
                .collect();
            assert_eq!(segs, segments_of(p, fs), "{p:?} fs {fs}");
        }
    }
}

/// REQ-TURN-002; CR 500.4, 500.5: `run_segment` does no phase-level work; a main-phase
/// segment does no step-level work either.
#[test]
fn run_segment_alone_does_no_phase_work() {
    let order = [P1, P2];
    let fake = TurnFake::new(2).at(2, Some(P1));
    let run = drive(
        World::new(fake.clone()),
        Proc::Segment(Segment::Step(Step::Upkeep)),
        &standard(),
        Answers::none(),
    );
    assert_eq!(run.calls(), step(Step::Upkeep, hb(&order)));
    assert_eq!(
        run.segment(),
        SegmentReport {
            segment: Segment::Step(Step::Upkeep),
            heartbeat_ran: true,
            cleanup_steps: 0
        }
    );
    for s in [Segment::PrecombatMain, Segment::PostcombatMain] {
        let run = drive(
            World::new(fake.clone()),
            Proc::Segment(s),
            &standard(),
            Answers::none(),
        );
        assert_eq!(run.calls(), hb(&order), "{s:?}");
        assert_eq!(seg_report(&run.rep()).segment, s);
    }
    for s in [
        Step::BeginningOfCombat,
        Step::DeclareAttackers,
        Step::DeclareBlockers,
        Step::FirstStrikeDamage,
        Step::CombatDamage,
        Step::EndOfCombat,
        Step::End,
    ] {
        let run = drive(
            World::new(fake.clone()),
            Proc::Segment(Segment::Step(s)),
            &standard(),
            Answers::none(),
        );
        let mut body = Vec::new();
        if s != Step::End {
            body.push(TC::Combat(s));
        }
        body.extend(hb(&order));
        assert_eq!(run.calls(), step(s, body), "{s:?}");
    }
}

/// REQ-TURN-002: `run_phase` / `run_segment` with no active player is a procedure error.
#[test]
fn no_active_player_is_a_procedure_error() {
    let fake = TurnFake::new(2).at(2, None);
    for proc in [
        Proc::Phase(Phase::Beginning),
        Proc::Phase(Phase::PrecombatMain),
        Proc::Phase(Phase::Ending),
        Proc::Segment(Segment::Step(Step::Upkeep)),
        Proc::Segment(Segment::Step(Step::Untap)),
        Proc::Segment(Segment::Step(Step::Cleanup)),
        Proc::Segment(Segment::PrecombatMain),
    ] {
        let run = drive(
            World::new(fake.clone()),
            proc.clone(),
            &standard(),
            Answers::none(),
        );
        assert_procedure_error(&run.out);
        assert!(run.requests.is_empty(), "{proc:?}");
    }
}

// =============================================================================================
// 8. segments_of and Segment::phase
// =============================================================================================

/// REQ-TURN-002; CR 500.1, 505.1a: the segments of each phase, with and without the
/// first-strike damage step, and the phase of each segment.
#[test]
fn segments_of_each_phase() {
    for fs in [false, true] {
        assert_eq!(
            segments_of(Phase::Beginning, fs),
            vec![
                Segment::Step(Step::Untap),
                Segment::Step(Step::Upkeep),
                Segment::Step(Step::Draw)
            ]
        );
        assert_eq!(
            segments_of(Phase::PrecombatMain, fs),
            vec![Segment::PrecombatMain]
        );
        assert_eq!(
            segments_of(Phase::Combat, fs),
            combat_steps(fs)
                .into_iter()
                .map(Segment::Step)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            segments_of(Phase::PostcombatMain, fs),
            vec![Segment::PostcombatMain]
        );
        assert_eq!(
            segments_of(Phase::Ending, fs),
            vec![Segment::Step(Step::End), Segment::Step(Step::Cleanup)]
        );
        for p in [
            Phase::Beginning,
            Phase::PrecombatMain,
            Phase::Combat,
            Phase::PostcombatMain,
            Phase::Ending,
        ] {
            for s in segments_of(p, fs) {
                assert_eq!(s.phase(), p, "{s:?}");
            }
        }
    }
    assert_eq!(
        segments_of(Phase::Combat, true).len(),
        segments_of(Phase::Combat, false).len() + 1
    );
    assert_eq!(
        Segment::Step(Step::FirstStrikeDamage).phase(),
        Phase::Combat
    );
}

// =============================================================================================
// 9. Interrupts and replay
// =============================================================================================

fn replay_world() -> World {
    World::new(
        TurnFake::new(2)
            .with_untap(P1, oids(&[1]), oids(&[3, 5]))
            .with_hand(P2, (11..=18).map(oid).collect()),
    )
}

fn replay_answers() -> Answers {
    Answers::none()
        .untap(oids(&[5]))
        .discard(vec![oid(FIRST_DRAWN), oid(14)])
}

/// REQ-TURN-002, REQ-CORE-001; CR 502.3, 514.1, 117.3a: a two-turn session with an untap
/// choice, a discard and the priority passes: the request sequence, the effect of the answers,
/// replay from the base and an independent second run all agree.
#[test]
fn session_replay_is_deterministic() {
    let reg = standard();
    let proc = Proc::Turns(vec![P1, P2]);
    let run = drive(replay_world(), proc.clone(), &reg, replay_answers());
    let reqs = run.non_priority();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].choice_type, ChoiceType::ChooseUntapSet);
    assert_eq!(reqs[0].chooser, P1);
    assert_eq!(reqs[1].choice_type, ChoiceType::SelectObjectsFromZone);
    assert_eq!(reqs[1].chooser, P2);
    // The untap choice is the first request of the turn; the discard is the last (the cleanup
    // step after it gives no priority).
    assert_eq!(run.requests[0].choice_type, ChoiceType::ChooseUntapSet);
    assert_eq!(
        run.requests.last().map(|r| r.choice_type),
        Some(ChoiceType::SelectObjectsFromZone)
    );
    prio_requests_match_log(&run);
    let log = run.calls();
    assert!(log.contains(&TC::Untap(P1, oids(&[1, 5]))));
    assert!(log.contains(&TC::Discard(P2, vec![oid(14), oid(FIRST_DRAWN)])));
    assert_eq!(run.last.host.hand(P2).len(), 7);

    // Replay from the base with the recorded answers.
    let again = replay(replay_world(), proc.clone(), &reg, &run.answers);
    assert_eq!(again.hash, run.hash);
    assert_eq!(again.calls(), log);
    assert_eq!(again.out, run.out);
    assert_eq!(again.last, run.last);

    // An independent second run.
    let second = drive(replay_world(), proc, &reg, replay_answers());
    assert_eq!(second.requests, run.requests);
    assert_eq!(second.answers, run.answers);
    assert_eq!(second.calls(), log);
    assert_eq!(second.hash, run.hash);
}

/// REQ-TURN-002, REQ-CORE-001; CR 514.1: `submit_default` on the discard request is accepted
/// and discards exactly the excess from the hand.
#[test]
fn discard_default_on_timeout_is_valid() {
    let fake = cleanup_fake().with_hand(P1, (1..=10).map(oid).collect());
    let (mut session, requests) = drive_until(
        World::new(fake),
        Proc::Segment(Segment::Step(Step::Cleanup)),
        &standard(),
        Answers::none(),
        |r| r.choice_type == ChoiceType::SelectObjectsFromZone,
    );
    assert_eq!(requests.len(), 1);
    let id = session.pending().expect("pending").request_id;
    session.submit_default(id).expect("the default is accepted");
    let run = drive_rest(session, requests, Answers::none(), true);
    let discards: Vec<TC> = run
        .calls()
        .into_iter()
        .filter(|c| matches!(c, TC::Discard(..)))
        .collect();
    assert_eq!(discards.len(), 1);
    let TC::Discard(p, cards) = &discards[0] else {
        unreachable!()
    };
    assert_eq!(*p, P1);
    assert_eq!(cards.len(), 3);
    let mut sorted = cards.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(&sorted, cards);
    assert_eq!(run.last.host.hand(P1).len(), 7);
    assert!(run.last.host.anomalies.is_empty());
}

/// REQ-TURN-002, REQ-NET-003; CR 504.1: a request raised by the host propagates unchanged and
/// the draw happens once it is answered.
#[test]
fn host_request_propagates_unchanged() {
    let mut fake = TurnFake::new(2).at(2, Some(P1));
    fake.draw_asks = true;
    let (session, _) = drive_until(
        World::new(fake.clone()),
        Proc::Segment(Segment::Step(Step::Draw)),
        &standard(),
        Answers::none(),
        |_| true,
    );
    let r = session.pending().expect("pending");
    assert_eq!(r.request_id.get(), 0);
    assert_eq!(r.choice_type, ChoiceType::ChoosePlayer);
    assert_eq!(r.chooser, P1);
    assert_eq!(r.reason.cr, "CR 504.1");
    assert_eq!(r.payload, ChoicePayload::ChoosePlayer { players: vec![P1] });
    let run = drive(
        World::new(fake),
        Proc::Segment(Segment::Step(Step::Draw)),
        &standard(),
        Answers::none(),
    );
    assert_eq!(run.types()[0], ChoiceType::ChoosePlayer);
    assert_eq!(
        run.calls(),
        step(Step::Draw, cat(vec![vec![TC::Draw(P1)], hb(&[P1, P2])]))
    );
}

/// REQ-TURN-002, REQ-CORE-001: a pending session's visible state is the base; the request ids
/// of a full turn increase from 0.
#[test]
fn pending_session_shows_the_base_state() {
    let base = replay_world();
    let (session, requests) = drive_until(
        base.clone(),
        Proc::Turns(vec![P1]),
        &standard(),
        replay_answers(),
        |r| r.choice_type == ChoiceType::PriorityAction,
    );
    assert_eq!(session.base(), &base);
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].request_id.get(), 0);
    assert_eq!(requests[1].request_id.get(), 1);
}

// =============================================================================================
// 11. Robustness
// =============================================================================================

/// REQ-TURN-002; CR 514.1; R12: malformed discard answers (wrong count, not in hand,
/// duplicates, wrong kind) are rejected by the session without a panic.
#[test]
fn discard_wrong_answers_are_rejected() {
    let fake = cleanup_fake()
        .with_hand(P1, oids(&[2, 4, 6, 8, 10, 12, 14, 16, 18]))
        .with_max(P1, Some(7));
    let (mut session, _) = drive_until(
        World::new(fake),
        Proc::Segment(Segment::Step(Step::Cleanup)),
        &standard(),
        Answers::none(),
        |_| true,
    );
    let pending = session.pending().expect("pending").clone();
    assert_eq!(pending.min, 2);
    let hash = session.state_hash();
    let bad = vec![
        ChoiceResponse::SubmitObjectSelection {
            objects: oids(&[2]),
        },
        ChoiceResponse::SubmitObjectSelection {
            objects: oids(&[2, 4, 6]),
        },
        ChoiceResponse::SubmitObjectSelection {
            objects: Vec::new(),
        },
        ChoiceResponse::SubmitObjectSelection {
            objects: oids(&[2, 3]),
        },
        ChoiceResponse::SubmitObjectSelection {
            objects: oids(&[4, 4]),
        },
        ChoiceResponse::SubmitSet {
            objects: oids(&[2, 4]),
        },
        ChoiceResponse::PassPriority,
    ];
    for answer in bad {
        assert!(
            session.submit(pending.request_id, answer.clone()).is_err(),
            "{answer:?} accepted"
        );
        assert_eq!(session.pending(), Some(&pending));
        assert_eq!(session.state_hash(), hash);
    }
    // A stale request id is rejected too.
    assert!(
        session
            .submit(
                fineprint_core::choice::RequestId::new(7),
                ChoiceResponse::SubmitObjectSelection {
                    objects: oids(&[2, 4])
                }
            )
            .is_err()
    );
    // And a valid answer is then still accepted.
    session
        .submit(
            pending.request_id,
            ChoiceResponse::SubmitObjectSelection {
                objects: oids(&[18, 2]),
            },
        )
        .expect("valid");
}
