// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Gate 02-state-objects regression tests for the battle protector and legend-rule requests of
//! one state-based action pass (REQ-SBA-001, REQ-CORE-001, REQ-CORE-006; CR 704.3, 101.4,
//! 704.5j, 704.5w, 704.5x, 704.5y, 614.6). Every card is synthetic (R5).
//!
//! CR 101.4: "If multiple players would make choices and/or take actions at the same time, the
//! active player (the player whose turn it is) makes any choices required, then the next player
//! in turn order (usually the player seated to the active player's left) makes any choices
//! required, followed by the remaining nonactive players in turn order. Then the actions happen
//! simultaneously."

#[path = "spec_sba_support.rs"]
mod support;

use std::cell::Cell;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoiceSession, ChoiceType, Interrupt, PendingChoiceRequest, Status,
};
use fineprint_core::events::{EventContext, ZoneChangeCause};
use fineprint_core::layers::{ContinuousEffects, LayerEnvironment};
use fineprint_core::replacement::{
    Action, BusError, BusEventKind, BusInterrupt, EventMatcher, ReplacementEffect,
};
use fineprint_core::sba::{
    SbaEnvironment, SbaRegistry, perform_all_post_batch, perform_all_relayered,
};
use fineprint_core::txn::Transactional;
use support::*;

const SBA: ZoneChangeCause = ZoneChangeCause::StateBasedAction;

/// The (chooser, kind) of each request, kind `"legend"` or `"protector"`.
fn shape(requests: &[PendingChoiceRequest]) -> Vec<(PlayerId, &'static str)> {
    requests
        .iter()
        .map(|r| {
            let kind = match r.choice_type {
                ChoiceType::ChoosePlayer => "protector",
                _ => "legend",
            };
            (r.chooser, kind)
        })
        .collect()
}

/// REQ-SBA-001, CR 704.5x, 310.12a (gate 02-state-objects F1): one controller (P1) with two
/// Sieges without a protector, both offering the same candidates (P2, P3): the two requests,
/// asked in ascending battle id, carry `reason.source` = their battle, and each answer is
/// recorded for the battle its request names. Both answer assignments.
#[test]
fn req_sba_001_704_5x_protector_request_names_its_battle() {
    for (first, second) in [(P3, P2), (P2, P3)] {
        let mut s = Sim::with_players(3);
        let b1 = s.siege(P1, P1, 3, None);
        let b2 = s.siege(P1, P1, 3, None);
        assert!(b1 < b2);
        let r = drive_answers(s, &[submit_player(first), submit_player(second)]);
        assert_protector_request(&r.requests[0], P1, "CR 704.5x", &[P2, P3]);
        assert_protector_request(&r.requests[1], P1, "CR 704.5x", &[P2, P3]);
        assert_eq!(r.requests[0].reason.source, Some(b1));
        assert_eq!(r.requests[1].reason.source, Some(b2));
        assert_ne!(r.requests[0].reason, r.requests[1].reason);
        assert_eq!(
            r.outcome().protectors_chosen,
            vec![(b1, first), (b2, second)]
        );
        assert_eq!(r.after.env.battle_protectors.get(&b1), Some(&first));
        assert_eq!(r.after.env.battle_protectors.get(&b2), Some(&second));
    }
}

/// Two-player board: `protector_owner` controls a Siege whose protector is itself (illegal,
/// CR 704.5y), `legend_owner` controls two legendary permanents with the same name (CR 704.5j).
/// `legend_first` creates the legends before the Siege (the other id order). Returns
/// (sim, siege, [legend a, legend b]).
fn board(
    protector_owner: PlayerId,
    legend_owner: PlayerId,
    legend_first: bool,
) -> (Sim, ObjectId, [ObjectId; 2]) {
    let mut s = Sim::new();
    let make_legends = |s: &mut Sim| {
        [
            s.legend(legend_owner, legend_owner, "test.legend_gate"),
            s.legend(legend_owner, legend_owner, "test.legend_gate"),
        ]
    };
    let (siege, legends) = if legend_first {
        let l = make_legends(&mut s);
        (
            s.siege(protector_owner, protector_owner, 3, Some(protector_owner)),
            l,
        )
    } else {
        let b = s.siege(protector_owner, protector_owner, 3, Some(protector_owner));
        (b, make_legends(&mut s))
    };
    (s, siege, legends)
}

/// The opponent of `p` in a two-player game.
fn other(p: PlayerId) -> PlayerId {
    if p == P1 { P2 } else { P1 }
}

/// REQ-SBA-001, CR 101.4, 704.5j, 704.5y (gate 02-state-objects F2, case a): the ACTIVE player
/// must choose a protector and the nonactive player a legend survivor in the same pass: the
/// FIRST request is the active player's protector request, the second the nonactive player's
/// legend request; the answers are applied together. Both id orders and both seats (P1 or P2
/// active).
#[test]
fn req_sba_001_101_4_active_protector_before_nonactive_legend() {
    for active in [P1, P2] {
        for legend_first in [false, true] {
            let nap = other(active);
            let (mut s, siege, [la, lb]) = board(active, nap, legend_first);
            s.set_turn(&[P1, P2], active);
            let r = drive_answers(s, &[submit_player(nap), keep(lb)]);
            assert_eq!(
                shape(&r.requests),
                vec![(active, "protector"), (nap, "legend")],
                "active {active:?}, legend first {legend_first}"
            );
            assert_protector_request(&r.requests[0], active, "CR 704.5y", &[nap]);
            assert_eq!(r.requests[0].reason.source, Some(siege));
            assert_eq!(r.requests[1].reason.cr, "CR 704.5j");
            assert!(r.requests[0].request_id < r.requests[1].request_id);
            let o = r.outcome();
            assert_eq!(o.protectors_chosen, vec![(siege, nap)]);
            assert_to_owners_graveyard(&r, la, SBA);
            assert_still_on_battlefield(&r, lb);
            assert_eq!(r.nonempty_batches().len(), 1);
        }
    }
}

/// REQ-SBA-001, CR 101.4, 704.5j, 704.5y (gate 02-state-objects F2, case b, the mirror): the
/// ACTIVE player has the legend choice and the nonactive player the protector choice: the
/// legend request comes first. Both id orders and both seats.
#[test]
fn req_sba_001_101_4_active_legend_before_nonactive_protector() {
    for active in [P1, P2] {
        for legend_first in [false, true] {
            let nap = other(active);
            let (mut s, siege, [la, lb]) = board(nap, active, legend_first);
            s.set_turn(&[P1, P2], active);
            let r = drive_answers(s, &[keep(la), submit_player(active)]);
            assert_eq!(
                shape(&r.requests),
                vec![(active, "legend"), (nap, "protector")],
                "active {active:?}, legend first {legend_first}"
            );
            assert_protector_request(&r.requests[1], nap, "CR 704.5y", &[active]);
            assert_eq!(r.requests[1].reason.source, Some(siege));
            let o = r.outcome();
            assert_eq!(o.protectors_chosen, vec![(siege, active)]);
            assert_to_owners_graveyard(&r, lb, SBA);
            assert_still_on_battlefield(&r, la);
        }
    }
}

/// REQ-SBA-001, CR 101.4, 704.5j, 704.5y (gate 02-state-objects F2, case c): one player with
/// both a legend choice and a protector choice: the legend request first, then the protector
/// request (the chosen deterministic order within one player). Both id orders, both seats.
#[test]
fn req_sba_001_101_4_one_player_legend_then_protector() {
    for active in [P1, P2] {
        for legend_first in [false, true] {
            for chooser in [active, other(active)] {
                let (mut s, siege, [la, _]) = board(chooser, chooser, legend_first);
                s.set_turn(&[P1, P2], active);
                let r = drive_answers(s, &[keep(la), submit_player(other(chooser))]);
                assert_eq!(
                    shape(&r.requests),
                    vec![(chooser, "legend"), (chooser, "protector")],
                    "active {active:?}, chooser {chooser:?}, legend first {legend_first}"
                );
                assert_eq!(r.requests[1].reason.source, Some(siege));
                assert_eq!(r.outcome().protectors_chosen, vec![(siege, other(chooser))]);
            }
        }
    }
}

/// REQ-SBA-001, REQ-CORE-001, CR 101.4, 704.5j, 704.5x, 704.5y (gate 02-state-objects F2):
/// three players, turn order P3, P1, P2 with P2 active (APNAP: P2, P3, P1, which is neither id
/// order nor turn-order-from-P1). P1 (created first, lowest ids) has a legend group; P2 has a
/// legend group and a Siege without protector; P3 has a Siege with an illegal protector. The
/// requests are P2 legend, P2 protector, P3 protector, P1 legend, with request ids ascending in
/// that order; every answer lands; the same pass twice is identical.
#[test]
fn req_sba_001_101_4_three_players_apnap_not_id_order() {
    let build = || {
        let mut s = Sim::with_players(3);
        s.set_turn(&[P3, P1, P2], P2);
        let p1_legends = [
            s.legend(P1, P1, "test.legend_one"),
            s.legend(P1, P1, "test.legend_one"),
        ];
        let p3_siege = s.siege(P3, P3, 3, Some(P3));
        let p2_siege = s.siege(P2, P2, 3, None);
        let p2_legends = [
            s.legend(P2, P2, "test.legend_two"),
            s.legend(P2, P2, "test.legend_two"),
        ];
        (s, p1_legends, p2_legends, p2_siege, p3_siege)
    };
    let (s, p1_legends, p2_legends, p2_siege, p3_siege) = build();
    let answers = [
        keep(p2_legends[1]),
        submit_player(P3),
        submit_player(P1),
        keep(p1_legends[0]),
    ];
    let r = drive_answers(s, &answers);
    assert_eq!(
        shape(&r.requests),
        vec![
            (P2, "legend"),
            (P2, "protector"),
            (P3, "protector"),
            (P1, "legend"),
        ]
    );
    assert!(
        r.requests
            .windows(2)
            .all(|w| w[0].request_id < w[1].request_id),
        "request ids in asking order"
    );
    assert_protector_request(&r.requests[1], P2, "CR 704.5x", &[P1, P3]);
    assert_eq!(r.requests[1].reason.source, Some(p2_siege));
    assert_protector_request(&r.requests[2], P3, "CR 704.5y", &[P1, P2]);
    assert_eq!(r.requests[2].reason.source, Some(p3_siege));
    let o = r.outcome();
    // Ascending by battle: the P3 Siege was created first.
    assert!(p3_siege < p2_siege);
    assert_eq!(o.protectors_chosen, vec![(p3_siege, P1), (p2_siege, P3)]);
    assert_to_owners_graveyard(&r, p2_legends[0], SBA);
    assert_to_owners_graveyard(&r, p1_legends[1], SBA);
    assert_still_on_battlefield(&r, p2_legends[1]);
    assert_still_on_battlefield(&r, p1_legends[0]);
    assert_eq!(r.nonempty_batches().len(), 1, "one event (CR 704.3)");

    let (again_sim, ..) = build();
    let again = drive_answers(again_sim, &answers);
    assert_eq!(again.requests, r.requests);
    assert_eq!(again.result, r.result);
    assert_eq!(again.hash, r.hash);
}

/// The state-based graveyard move of `object` (cause `StateBasedAction`).
fn sba_move_of(object: ObjectId) -> EventMatcher {
    EventMatcher {
        affected_object: Some(object),
        cause: Some(SBA),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    }
}

/// REQ-SBA-001, REQ-CORE-001, CR 704.5w, 704.5x, 704.3, 614.6 (gate 02-state-objects R3): a
/// typeless battle at defense 0 with no protector whose graveyard move a persistent replacement
/// skips stays on the battlefield; the pass asks its controller ONE protector request after the
/// batch (reason CR 704.5x, source the battle); while it is pending the state is unchanged; the
/// answer is recorded and the pass is performed; the next pass asks nothing and still skips the
/// move.
#[test]
fn req_sba_001_704_5x_retained_zero_defense_battle_gets_its_protector() {
    let mut s = Sim::new();
    let b = s.battle(P1, P1, 0, None);
    s.add_replacement(ReplacementEffect::new(eid(1), sba_move_of(b), Action::Skip));
    let before = s.state_hash();
    let session = ChoiceSession::start(s.clone(), call_sba).unwrap();
    let pending = session.pending().expect("a protector request").clone();
    assert_eq!(session.state_hash(), before);
    assert_protector_request(&pending, P1, "CR 704.5x", &[P1]);
    assert_eq!(pending.reason.source, Some(b));

    let r = drive_answers(s, &[submit_player(P1)]);
    assert_eq!(r.requests, vec![pending]);
    let o = r.outcome();
    assert_eq!(o.protectors_chosen, vec![(b, P1)]);
    assert!(o.performed);
    assert_still_on_battlefield(&r, b);
    assert_eq!(r.after.env.battle_protectors.get(&b), Some(&P1));
    assert_eq!(r.finding_ids(), vec!["704.5w", "704.5x"]);

    let next = run_sba(r.after.clone());
    assert!(next.outcome().protectors_chosen.is_empty());
    assert!(!next.outcome().performed);
    assert_eq!(next.finding_ids(), vec!["704.5w"]);
    assert_still_on_battlefield(&next, b);
}

/// One `perform_all_relayered` pass over `s` through the public API, whose provider hands back
/// the same layer inputs (`s.effects`, `s.layer`) and counts its calls in `calls`. A pending
/// request must leave the state unchanged.
fn call_sba_relayered(
    s: &mut Sim,
    c: &mut ChoiceContext<'_>,
    calls: &Cell<usize>,
) -> Result<SbaOut, Interrupt> {
    let registry = SbaRegistry::standard();
    let before = s.snap();
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
    let mut post_batch =
        |_: &EventContext<'_>, view: &mut dyn FnMut(&ContinuousEffects, &dyn LayerEnvironment)| {
            calls.set(calls.get() + 1);
            view(effects, layer);
        };
    let r = perform_all_relayered(
        &registry,
        &mut ctx,
        replacements,
        effects,
        layer,
        env,
        c,
        &mut post_batch,
    );
    match r {
        Ok(o) => Ok(Ok(o)),
        Err(BusInterrupt::Pending(p)) => {
            assert_eq!(s.snap(), before, "a pending pass changed the state");
            Err(Interrupt::Pending(p))
        }
        Err(BusInterrupt::Error(BusError::Choice(e @ ChoiceError::Invalid { .. }))) => {
            Err(Interrupt::Error(e))
        }
        Err(BusInterrupt::Error(e)) => Ok(Err(e)),
    }
}

/// REQ-SBA-001, REQ-LAYER-001, CR 704.5w, 704.5x, 614.6, 704.3 (gate 02-state-objects R3b): the
/// re-exported `perform_all_relayered`, with a provider that hands back the same layer inputs,
/// asks the controller of a retained zero-defense battle exactly the request `perform_all`
/// asks, after the batch (the provider called once per run); while it is pending the state is
/// unchanged; the answer is recorded.
#[test]
fn req_sba_001_704_5x_relayered_entry_point_asks_the_retained_battle() {
    let mut s = Sim::new();
    let b = s.battle(P1, P1, 0, None);
    s.add_replacement(ReplacementEffect::new(eid(1), sba_move_of(b), Action::Skip));
    let before = s.state_hash();
    let calls = Cell::new(0);
    let mut session = ChoiceSession::start(s.clone(), |s: &mut Sim, c: &mut ChoiceContext<'_>| {
        call_sba_relayered(s, c, &calls)
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    let pending = session.pending().expect("a protector request").clone();
    assert_eq!(session.state_hash(), before);
    assert_protector_request(&pending, P1, "CR 704.5x", &[P1]);
    assert_eq!(pending.reason.source, Some(b));
    assert_eq!(
        drive_answers(s, &[submit_player(P1)]).requests,
        vec![pending.clone()]
    );

    session
        .submit(pending.request_id, submit_player(P1))
        .unwrap();
    assert_eq!(calls.get(), 2);
    let Status::Done { state, output } = session.status() else {
        panic!("the pass is still pending");
    };
    let o = output.as_ref().expect("an SBA outcome");
    assert_eq!(o.protectors_chosen, vec![(b, P1)]);
    assert!(o.performed);
    assert_eq!(state.env.battle_protectors.get(&b), Some(&P1));
}

/// One `perform_all_post_batch` pass over `s` through the public API: the provider hands back
/// the same layer inputs and a read-only protector view that is a copy of `s.env` without the
/// (battle, player) pairs of `eligible_after` in its illegal protectors (the caller's state
/// after the batch). A pending request must leave the state unchanged.
fn call_sba_post_batch(
    s: &mut Sim,
    c: &mut ChoiceContext<'_>,
    eligible_after: &[(ObjectId, PlayerId)],
) -> Result<SbaOut, Interrupt> {
    let registry = SbaRegistry::standard();
    let before = s.snap();
    let mut view_env = s.env.clone();
    for pair in eligible_after {
        view_env.illegal_protectors.remove(pair);
    }
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
    let mut post_batch = |_: &EventContext<'_>,
                          view: &mut dyn FnMut(
        &ContinuousEffects,
        &dyn LayerEnvironment,
        &dyn SbaEnvironment,
    )| { view(effects, layer, &view_env) };
    let r = perform_all_post_batch(
        &registry,
        &mut ctx,
        replacements,
        effects,
        layer,
        env,
        c,
        &mut post_batch,
    );
    match r {
        Ok(o) => Ok(Ok(o)),
        Err(BusInterrupt::Pending(p)) => {
            assert_eq!(s.snap(), before, "a pending pass changed the state");
            Err(Interrupt::Pending(p))
        }
        Err(BusInterrupt::Error(BusError::Choice(e @ ChoiceError::Invalid { .. }))) => {
            Err(Interrupt::Error(e))
        }
        Err(BusInterrupt::Error(e)) => Ok(Err(e)),
    }
}

/// Starts a `perform_all_post_batch` session over `s` and answers each request with
/// `answers` in order; returns the requests, the final state and the outcome.
fn drive_post_batch(
    s: &Sim,
    answers: &[PlayerId],
    eligible_after: &[(ObjectId, PlayerId)],
) -> (Vec<PendingChoiceRequest>, Sim, SbaOut) {
    let mut session = ChoiceSession::start(s.clone(), |s: &mut Sim, c: &mut ChoiceContext<'_>| {
        call_sba_post_batch(s, c, eligible_after)
    })
    .unwrap();
    let mut requests = Vec::new();
    let mut answers = answers.iter();
    loop {
        let pending = match session.status() {
            Status::Done { state, output } => {
                assert!(answers.next().is_none(), "unused answers");
                return (requests, state.clone(), output.clone());
            }
            Status::Pending(r) => r.clone(),
        };
        let answer = *answers.next().expect("more requests than answers");
        session
            .submit(pending.request_id, submit_player(answer))
            .unwrap();
        requests.push(pending);
    }
}

/// REQ-SBA-001, REQ-LAYER-001, CR 704.5v, 704.5x, 614.6, 704.3 (gate 02-state-objects
/// R3c, threads 4698236604 and 4698236625): through the re-exported `perform_all_post_batch`,
/// the post-batch request of a retained zero-defense Siege takes who can be its protector from
/// the provider's read-only view (P2, ineligible in the pass's own environment, is a candidate);
/// the answer is recorded through the pass's own environment, whose eligibility facts are
/// untouched. A retained Siege whose controller loses the game in the same pass gets no
/// request.
#[test]
fn req_sba_001_704_5x_post_batch_entry_point_reads_the_eligibility_view() {
    let mut s = Sim::with_players(3);
    let b = s.siege(P1, P1, 0, None);
    s.add_replacement(ReplacementEffect::new(eid(1), sba_move_of(b), Action::Skip));
    s.env.illegal_protectors.insert((b, P2));

    let (requests, after, out) = drive_post_batch(&s, &[P2], &[(b, P2)]);
    assert_eq!(requests.len(), 1);
    assert_protector_request(&requests[0], P1, "CR 704.5x", &[P2, P3]);
    assert_eq!(requests[0].reason.source, Some(b));
    assert_eq!(
        out.expect("an SBA outcome").protectors_chosen,
        vec![(b, P2)]
    );
    assert_eq!(after.env.battle_protectors.get(&b), Some(&P2));
    assert!(after.env.illegal_protectors.contains(&(b, P2)));

    // The pass's own environment (no view change): P3 only.
    let (requests, _, _) = drive_post_batch(&s, &[P3], &[]);
    assert_protector_request(&requests[0], P1, "CR 704.5x", &[P3]);

    // The controller loses the game in the same pass: no request.
    s.set_life(P1, 0);
    let (requests, after, out) = drive_post_batch(&s, &[], &[(b, P2)]);
    assert!(requests.is_empty());
    let o = out.expect("an SBA outcome");
    assert_eq!(o.lost_players, vec![P1]);
    assert!(o.protectors_chosen.is_empty());
    assert!(!after.env.battle_protectors.contains_key(&b));
}
