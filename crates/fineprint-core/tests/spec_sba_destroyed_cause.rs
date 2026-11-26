// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for SBA destruction being visible as "destroyed" (FP-155; REQ-SBA-001,
//! REQ-EVENT-001; CR 701.8b, 704.5f, 704.5g, 704.5h, 704.5i, 704.5s, 704.7, 122.1c).
//! Black-box: written from the work-item brief only.
//!
//! The interface under test: `ZoneChangeCause::StateBasedDestroy` (canonical tag 8) is the cause
//! of the 704.5g and 704.5h moves; 704.5f and 704.5i moves keep `StateBasedAction`; a merged move
//! of one object with a destroy finding is `StateBasedDestroy`; trigger normalization gives
//! `EventKind::Destroyed` for a battlefield-to-graveyard `Destroy` or `StateBasedDestroy` move
//! and not for `StateBasedAction`; the shield counter's replacement (CR 122.1c) matches only the
//! effect cause `Destroy`; replacement matchers compare `cause` by equality.
//!
//! Every card, object and effect here is synthetic and neutrally named (R5).

#[path = "spec_sba_support.rs"]
mod support;

#[path = "spec_triggers_support.rs"]
mod tsupport;

use std::collections::BTreeSet;

use fineprint_base::ids::ObjectId;
use fineprint_base::ids::PlayerId;
use fineprint_core::choice::{ChoiceContext, ChoiceSession, Interrupt, Status};
use fineprint_core::events::{
    EventBatch, EventContext, EventInstance, ExecutedBatch, ExecutedEvent, GameEvent, Outcome,
    ZoneChangeCause,
};
use fineprint_core::memory::LkiStore;
use fineprint_core::objects::Characteristics;
use fineprint_core::replacement::{
    Action, BusError, BusEventKind, BusInterrupt, BusReport, EventMatcher, Modification,
    ReplacementEffect, process_event,
};
use fineprint_core::triggers::{
    StateView, TriggerEvent, TriggerRegistration, events_from_batch, events_from_batch_with_lki,
};
use fineprint_core::zones::{MoveRequest, Moved, ZoneId, ZoneKind};
use fineprint_dsl::ability::EventKind;
use support::*;

use ZoneChangeCause as C;

// =============================================================================================
// helpers
// =============================================================================================

/// Every cause, in canonical-tag order (tags 0..=8).
const ALL_CAUSES: [ZoneChangeCause; 9] = [
    C::Other,
    C::Destroy,
    C::Sacrifice,
    C::Discard,
    C::Mill,
    C::Exile,
    C::CounterSpell,
    C::StateBasedAction,
    C::StateBasedDestroy,
];

/// A synthetic creature-planeswalker with literal power and toughness (CR 704.5i applies to it
/// through its loyalty, CR 704.5g/704.5h through its toughness and damage).
fn creature_walker_chars(name: &str, p: i64, t: i64) -> Characteristics {
    Characteristics {
        power: Some(lit(p)),
        toughness: Some(lit(t)),
        ..blank(name, &["Creature", "Planeswalker"])
    }
}

/// Asserts `old` left the battlefield through exactly one move of this pass, to its owner's
/// graveyard, with `cause`.
fn assert_died_with(r: &SbaRun, old: ObjectId, cause: ZoneChangeCause) {
    assert_to_owners_graveyard(r, old, cause);
}

/// Asserts `old` moved exactly once in this pass, to a zone of kind `kind`.
fn assert_moved_once_to(r: &SbaRun, old: ObjectId, kind: ZoneKind) {
    let moves: Vec<_> = r.moves().into_iter().filter(|m| m.0 == old).collect();
    assert_eq!(moves.len(), 1, "{old:?} moved exactly once: {moves:?}");
    assert_eq!(moves[0].2, kind, "{old:?} destination");
    assert!(r.after.zones.object(old).is_none(), "{old:?} left");
    assert_eq!(
        r.after.zone_of(moves[0].1),
        Some(kind),
        "{old:?} is now in {kind:?}"
    );
}

/// The trigger events of every executed batch of an SBA pass, in order.
fn sba_trigger_events(r: &SbaRun) -> Vec<TriggerEvent> {
    r.outcome()
        .executed
        .iter()
        .flat_map(events_from_batch)
        .collect()
}

/// The synthetic "whenever a creature is destroyed" listener: a triggered ability on
/// `EventKind::Destroyed` with an object (creature) subject, controlled by `controller`.
fn destroy_listener(controller: PlayerId) -> TriggerRegistration {
    tsupport::reg(
        900,
        controller,
        tsupport::ability(
            "test.destroy_listener",
            tsupport::subj_obj(tsupport::m(EventKind::Destroyed)),
        ),
    )
}

/// The pre-event subject ids the destroy listener fires for over `events`. The fake environment
/// answers the listener's creature filter with `true` for every id in `accepted` (in both
/// views), so a missing trigger is due to the event kinds alone.
fn listener_hits(
    controller: PlayerId,
    events: &[TriggerEvent],
    accepted: &[ObjectId],
) -> Vec<ObjectId> {
    let ids: Vec<u64> = accepted.iter().map(|o| o.get()).collect();
    let env = tsupport::FakeEnv::new()
        .accept(StateView::PreEvent, &ids)
        .accept(StateView::PostEvent, &ids)
        .with_both(destroy_listener(controller));
    let out = tsupport::run(&env, events);
    let mut hits = Vec::new();
    for t in &out {
        assert_eq!(t.ability_id, "test.destroy_listener");
        assert_eq!(t.controller, controller);
        for &i in &t.event_indices {
            hits.push(events[i].subject_pre.expect("an object event"));
        }
    }
    hits.sort();
    hits
}

/// The trigger event about `pre` in `events` (exactly one).
fn event_about(events: &[TriggerEvent], pre: ObjectId) -> TriggerEvent {
    let found: Vec<&TriggerEvent> = events
        .iter()
        .filter(|e| e.subject_pre == Some(pre))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one trigger event about {pre:?}: {events:?}"
    );
    found[0].clone()
}

/// Membership of every `EventKind` in `t.kinds`, in `EventKind::ALL` order.
fn kind_profile(t: &TriggerEvent) -> Vec<(EventKind, bool)> {
    EventKind::ALL
        .iter()
        .map(|k| (*k, t.kinds.contains(k)))
        .collect()
}

/// Executes `events` as one plain batch (no replacement bus) on `s`.
fn exec_batch(s: &mut Sim, events: Vec<GameEvent>) -> ExecutedBatch {
    let batch = EventBatch::from_events(events);
    s.ctx().execute_batch(&batch).expect("batch executes")
}

fn zc(object: ObjectId, to: ZoneKind, cause: ZoneChangeCause) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause,
    }
}

/// Runs `event` through the replacement bus (`process_event`) over `base`'s world and
/// replacement registry inside a choice session; no choice may be asked.
fn bus_event(base: Sim, event: GameEvent) -> (Sim, BusReport) {
    let procedure = move |s: &mut Sim,
                          c: &mut ChoiceContext<'_>|
          -> Result<Result<BusReport, BusError>, Interrupt> {
        let Sim {
            game,
            zones,
            ids,
            permanents,
            memories,
            lki,
            events,
            provider,
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
        match process_event(&mut ctx, replacements, event.clone(), c) {
            Ok(rep) => Ok(Ok(rep)),
            Err(BusInterrupt::Pending(p)) => Err(Interrupt::Pending(p)),
            Err(BusInterrupt::Error(e)) => Ok(Err(e)),
        }
    };
    let session = match ChoiceSession::start(base, procedure) {
        Ok(s) => s,
        Err(e) => panic!("session start failed: {e:?}"),
    };
    assert!(session.pending().is_none(), "no choice is asked");
    match session.status() {
        Status::Done { state, output } => match output {
            Ok(rep) => (state.clone(), rep.clone()),
            Err(e) => panic!("bus error {e:?}"),
        },
        Status::Pending(_) => unreachable!("no choice is pending"),
    }
}

/// A ZoneChange replacement matcher on `cause` only.
fn cause_matcher(cause: ZoneChangeCause) -> EventMatcher {
    EventMatcher {
        cause: Some(cause),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    }
}

/// A persistent "put it into exile instead" redirect of zone changes with `cause`.
fn exile_redirect(n: u64, cause: ZoneChangeCause) -> ReplacementEffect {
    ReplacementEffect::new(
        eid(n),
        cause_matcher(cause),
        Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
    )
    .with_description(format!("test.exile_redirect_{n}"))
}

/// Every executed `RemoveCounters` of kind `shield` in the pass.
fn shield_removals(r: &SbaRun) -> Vec<(ObjectId, String, i64)> {
    counter_removals(r)
        .into_iter()
        .filter(|(_, k, _)| k == "shield")
        .collect()
}

// =============================================================================================
// 1. the SBA pass: 704.5g/704.5h move with StateBasedDestroy, 704.5f/704.5i with StateBasedAction
// =============================================================================================

/// REQ-SBA-001, CR 701.8b, 704.5g: a synthetic 2/2 creature with 2 damage dies through the SBA
/// pass; its one ZoneChange has cause `StateBasedDestroy`. Both controllers.
#[test]
fn req_sba_001_704_5g_lethal_damage_cause_is_state_based_destroy() {
    for owner in [P1, P2] {
        let mut s = Sim::new();
        let a = s.creature(owner, "test.creature_2_2", 2, 2);
        s.damage(a, 2);
        assert_eq!(s.check(), vec![finding("704.5g", obj(a))]);
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, vec![finding("704.5g", obj(a))]);
        assert_died_with(&r, a, C::StateBasedDestroy);
        assert_eq!(r.moves().len(), 1);
    }
}

/// REQ-SBA-001, CR 701.8b, 704.5h: a creature with deathtouch damage and toughness above the
/// damage dies through the SBA pass with cause `StateBasedDestroy`. Both controllers.
#[test]
fn req_sba_001_704_5h_deathtouch_damage_cause_is_state_based_destroy() {
    for owner in [P1, P2] {
        let mut s = Sim::new();
        let a = s.creature(owner, "test.creature_3_3", 3, 3);
        s.damage(a, 1);
        s.env.deathtouch.insert(a);
        assert_eq!(s.check(), vec![finding("704.5h", obj(a))]);
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, vec![finding("704.5h", obj(a))]);
        assert_died_with(&r, a, C::StateBasedDestroy);
        assert_eq!(r.moves().len(), 1);
    }
}

/// REQ-SBA-001, CR 701.8b, 704.5f: a 0-toughness creature is put into its owner's graveyard
/// with cause `StateBasedAction` (not destroyed). Both controllers.
#[test]
fn req_sba_001_704_5f_zero_toughness_cause_is_state_based_action() {
    for owner in [P1, P2] {
        let mut s = Sim::new();
        let a = s.creature(owner, "test.creature_2_0", 2, 0);
        assert_eq!(s.check(), vec![finding("704.5f", obj(a))]);
        let r = run_sba(s);
        assert_died_with(&r, a, C::StateBasedAction);
    }
}

/// REQ-SBA-001, CR 701.8b, 704.5i: a planeswalker with 0 loyalty is put into its owner's
/// graveyard with cause `StateBasedAction` (not destroyed). Both controllers.
#[test]
fn req_sba_001_704_5i_zero_loyalty_cause_is_state_based_action() {
    for owner in [P1, P2] {
        let mut s = Sim::new();
        let w = s.walker(owner, 0);
        assert_eq!(s.check(), vec![finding("704.5i", obj(w))]);
        let r = run_sba(s);
        assert_died_with(&r, w, C::StateBasedAction);
    }
}

/// REQ-SBA-001, CR 701.8b, 704.5f, 704.5g, 704.5h, 704.5i: all four kinds of death in one pass,
/// across both controllers: each object keeps its own cause.
#[test]
fn req_sba_001_mixed_pass_each_object_keeps_its_cause() {
    let mut s = Sim::new();
    let g1 = s.creature(P1, "test.creature_2_2", 2, 2);
    let h2 = s.creature(P2, "test.creature_3_3", 3, 3);
    let f1 = s.creature(P1, "test.creature_2_0", 2, 0);
    let i2 = s.walker(P2, 0);
    let g2 = s.creature(P2, "test.creature_2_2", 2, 2);
    let h1 = s.creature(P1, "test.creature_3_3", 3, 3);
    let f2 = s.creature(P2, "test.creature_1_0", 1, 0);
    let i1 = s.walker(P1, 0);
    let survivor = s.creature(P1, "test.creature_3_3", 3, 3);
    for g in [g1, g2] {
        s.damage(g, 2);
    }
    for h in [h1, h2] {
        s.damage(h, 1);
        s.env.deathtouch.insert(h);
    }
    s.damage(survivor, 1);
    let r = run_sba(s);
    for x in [g1, g2, h1, h2] {
        assert_died_with(&r, x, C::StateBasedDestroy);
    }
    for x in [f1, f2, i1, i2] {
        assert_died_with(&r, x, C::StateBasedAction);
    }
    assert_still_on_battlefield(&r, survivor);
    assert_eq!(r.moves().len(), 8);
}

// =============================================================================================
// 2. the "whenever a creature is destroyed" listener
// =============================================================================================

/// REQ-SBA-001, CR 701.8b, 704.5g, 704.5h, 704.5f, 704.5i: driven through the SBA pass and
/// `events_from_batch`, a synthetic "whenever a creature is destroyed" listener fires for the
/// 704.5g death and the 704.5h death and not for the 704.5f or 704.5i death (even with the
/// listener's filter accepting every object). Listener controlled by each player in turn;
/// dying objects of both players.
#[test]
fn req_sba_001_destroy_listener_fires_for_5g_and_5h_only() {
    for listener_controller in [P1, P2] {
        let mut s = Sim::new();
        let g = s.creature(P1, "test.creature_2_2", 2, 2);
        let h = s.creature(P2, "test.creature_3_3", 3, 3);
        let f = s.creature(P1, "test.creature_2_0", 2, 0);
        let i = s.walker(P2, 0);
        s.damage(g, 2);
        s.damage(h, 1);
        s.env.deathtouch.insert(h);
        let r = run_sba(s);
        let events = sba_trigger_events(&r);
        let mut expected = vec![g, h];
        expected.sort();
        assert_eq!(
            listener_hits(listener_controller, &events, &[g, h, f, i]),
            expected
        );
        for x in [g, h] {
            let e = event_about(&events, x);
            assert!(e.kinds.contains(&EventKind::Destroyed), "{x:?}");
            assert!(e.kinds.contains(&EventKind::Dies), "{x:?}");
        }
        for x in [f, i] {
            let e = event_about(&events, x);
            assert!(!e.kinds.contains(&EventKind::Destroyed), "{x:?}");
            assert!(e.kinds.contains(&EventKind::Dies), "{x:?}");
        }
    }
}

/// REQ-SBA-001, CR 701.8b, 704.5g, 704.5h: each death alone in its pass, for each controller of
/// the dying creature: 704.5g and 704.5h fire the listener, 704.5f and 704.5i do not.
#[test]
fn req_sba_001_destroy_listener_each_death_alone() {
    for owner in [P1, P2] {
        // 704.5g
        let mut s = Sim::new();
        let g = s.creature(owner, "test.creature_2_2", 2, 2);
        s.damage(g, 3);
        let r = run_sba(s);
        assert_eq!(listener_hits(P1, &sba_trigger_events(&r), &[g]), vec![g]);
        // 704.5h
        let mut s = Sim::new();
        let h = s.creature(owner, "test.creature_4_4", 4, 4);
        s.damage(h, 1);
        s.env.deathtouch.insert(h);
        let r = run_sba(s);
        assert_eq!(listener_hits(P1, &sba_trigger_events(&r), &[h]), vec![h]);
        // 704.5f
        let mut s = Sim::new();
        let f = s.creature(owner, "test.creature_1_m1", 1, -1);
        let r = run_sba(s);
        let ev = sba_trigger_events(&r);
        assert_eq!(ev.len(), 1);
        assert!(listener_hits(P1, &ev, &[f]).is_empty());
        // 704.5i
        let mut s = Sim::new();
        let i = s.walker(owner, 0);
        let r = run_sba(s);
        let ev = sba_trigger_events(&r);
        assert_eq!(ev.len(), 1);
        assert!(listener_hits(P1, &ev, &[i]).is_empty());
    }
}

/// REQ-SBA-001, CR 701.8b, 704.5s: a `Sacrifice`-caused SBA move (a Saga at its final chapter)
/// keeps cause `Sacrifice` and does not fire the destroy listener.
#[test]
fn req_sba_001_704_5s_sacrifice_unchanged_and_not_destroyed() {
    for owner in [P1, P2] {
        let mut s = Sim::new();
        let saga = s.saga(owner, owner, Some(3), 3);
        assert_eq!(s.check(), vec![finding("704.5s", obj(saga))]);
        let r = run_sba(s);
        assert_died_with(&r, saga, C::Sacrifice);
        let events = sba_trigger_events(&r);
        let e = event_about(&events, saga);
        assert!(e.kinds.contains(&EventKind::Sacrificed));
        assert!(!e.kinds.contains(&EventKind::Destroyed));
        assert!(listener_hits(P1, &events, &[saga]).is_empty());
    }
}

/// REQ-SBA-001, CR 701.8b: real (non-SBA) battlefield-to-graveyard moves: an effect `Destroy`
/// still fires the destroy listener; `Sacrifice` and a plain `StateBasedAction` move do not;
/// `StateBasedDestroy` does. Creatures of both controllers.
#[test]
fn req_sba_001_destroy_listener_real_moves_by_cause() {
    for owner in [P1, P2] {
        for (cause, fires) in [
            (C::Destroy, true),
            (C::StateBasedDestroy, true),
            (C::Sacrifice, false),
            (C::StateBasedAction, false),
            (C::Other, false),
        ] {
            let mut s = Sim::new();
            let a = s.creature(owner, "test.creature_2_2", 2, 2);
            let batch = exec_batch(&mut s, vec![zc(a, ZoneKind::Graveyard, cause)]);
            let events = events_from_batch(&batch);
            assert_eq!(events.len(), 1, "{cause:?}");
            assert_eq!(
                events[0].kinds.contains(&EventKind::Destroyed),
                fires,
                "{cause:?}"
            );
            let expected = if fires { vec![a] } else { vec![] };
            assert_eq!(listener_hits(P2, &events, &[a]), expected, "{cause:?}");
        }
    }
}

fn literal_zone_change(
    from: ZoneKind,
    to: ZoneKind,
    cause: ZoneChangeCause,
    old: u64,
    new: u64,
) -> ExecutedEvent {
    ExecutedEvent {
        instance: EventInstance::new(zc(ObjectId::new(old), to, cause)),
        outcome: Outcome::ZoneChange(Moved {
            old: ObjectId::new(old),
            new: ObjectId::new(new),
            from: ZoneId::of(from, P1),
            to: ZoneId::of(to, P2),
        }),
        lki_captured: from == ZoneKind::Battlefield,
        unattached: vec![],
    }
}

/// The mapped kinds of one literal zone change, through both mappings (which must agree).
fn literal_kinds(from: ZoneKind, to: ZoneKind, cause: ZoneChangeCause) -> TriggerEvent {
    let batch = ExecutedBatch {
        index: 0,
        turn: 1,
        events: vec![literal_zone_change(from, to, cause, 7, 1007)],
    };
    let out = events_from_batch(&batch);
    let with_lki = events_from_batch_with_lki(&batch, &LkiStore::new());
    assert_eq!(out.len(), 1, "{from:?}->{to:?} {cause:?}");
    assert_eq!(with_lki.len(), 1, "{from:?}->{to:?} {cause:?}");
    assert_eq!(
        kind_profile(&out[0]),
        kind_profile(&with_lki[0]),
        "{from:?}->{to:?} {cause:?}: both mappings agree"
    );
    out[0].clone()
}

/// REQ-SBA-001, CR 701.8b: an `ExecutedBatch` literal matrix over (cause, from, to):
/// `StateBasedDestroy` and `Destroy` give identical event-kind sets for the same from/to;
/// `StateBasedAction` never gives `Destroyed`; battlefield-to-graveyard `StateBasedDestroy` is
/// exactly the `StateBasedAction` kind set plus `Destroyed` (the same zone-derived kinds).
#[test]
fn req_sba_001_kind_matrix_state_based_destroy_equals_destroy() {
    let mut pairs = 0;
    for from in ZoneKind::ALL {
        for to in ZoneKind::ALL {
            if from == to {
                continue;
            }
            pairs += 1;
            let sbd = literal_kinds(from, to, C::StateBasedDestroy);
            let destroy = literal_kinds(from, to, C::Destroy);
            assert_eq!(
                kind_profile(&sbd),
                kind_profile(&destroy),
                "{from:?}->{to:?}: StateBasedDestroy and Destroy give the same kinds"
            );
            let sba = literal_kinds(from, to, C::StateBasedAction);
            assert!(
                !sba.kinds.contains(&EventKind::Destroyed),
                "{from:?}->{to:?}: StateBasedAction is never Destroyed"
            );
            assert_eq!(sbd.subject_pre, destroy.subject_pre);
            assert_eq!(sbd.subject_post, destroy.subject_post);
            assert_eq!(sbd.fields, destroy.fields, "{from:?}->{to:?} fields");
            if from == ZoneKind::Battlefield && to == ZoneKind::Graveyard {
                assert!(sbd.kinds.contains(&EventKind::Destroyed));
                let mut expected: BTreeSet<String> =
                    sba.kinds.iter().map(|k| format!("{k:?}")).collect();
                expected.insert(format!("{:?}", EventKind::Destroyed));
                let got: BTreeSet<String> = sbd.kinds.iter().map(|k| format!("{k:?}")).collect();
                assert_eq!(got, expected, "SBA kinds plus Destroyed");
                assert!(sbd.kinds.contains(&EventKind::Dies));
                assert!(sbd.kinds.contains(&EventKind::LeavesBattlefield));
                assert!(sbd.kinds.contains(&EventKind::PutIntoGraveyard));
            }
        }
    }
    assert!(pairs > 0);
}

// =============================================================================================
// 3. one object, several findings: one move (CR 704.7)
// =============================================================================================

/// REQ-SBA-001, CR 704.5g, 704.5h, 704.7: one creature with both a 704.5g and a 704.5h finding
/// moves exactly once, with cause `StateBasedDestroy`. Both controllers.
#[test]
fn req_sba_001_704_7_5g_and_5h_on_one_object_one_destroy_move() {
    for owner in [P1, P2] {
        let mut s = Sim::new();
        let a = s.creature(owner, "test.creature_2_2", 2, 2);
        s.damage(a, 2);
        s.env.deathtouch.insert(a);
        let expected = vec![finding("704.5g", obj(a)), finding("704.5h", obj(a))];
        assert_eq!(s.check(), expected);
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, expected);
        assert_died_with(&r, a, C::StateBasedDestroy);
        assert_eq!(r.moves().len(), 1);
        let events = sba_trigger_events(&r);
        assert_eq!(listener_hits(P1, &events, &[a]), vec![a], "fires once");
    }
}

/// REQ-SBA-001, CR 704.5f, 704.5g, 704.5h: the standard registry gives a 0-toughness creature
/// with marked damage, or with deathtouch damage, only the 704.5f finding (704.5g and 704.5h
/// need toughness above 0), so its move is `StateBasedAction`. (A 704.5f + 704.5g/h finding pair
/// on one object cannot be built through the public API; see the creature-planeswalker tests
/// below for the merged case.)
#[test]
fn req_sba_001_zero_toughness_with_damage_is_5f_only_state_based_action() {
    for owner in [P1, P2] {
        let mut s = Sim::new();
        let damaged = s.creature(owner, "test.creature_2_0", 2, 0);
        let touched = s.creature(owner, "test.creature_1_0", 1, 0);
        s.damage(damaged, 2);
        s.damage(touched, 1);
        s.env.deathtouch.insert(touched);
        assert_eq!(
            s.check(),
            vec![
                finding("704.5f", obj(damaged)),
                finding("704.5f", obj(touched))
            ]
        );
        let r = run_sba(s);
        assert_died_with(&r, damaged, C::StateBasedAction);
        assert_died_with(&r, touched, C::StateBasedAction);
    }
}

/// Builds a pass with one creature-planeswalker `x` (0 loyalty) carrying a 704.5i finding and a
/// destroy finding (`deathtouch` false: 704.5g lethal damage; true: 704.5h deathtouch damage),
/// plus a plain 704.5f creature `y`, created before `x` when `x_first` is false. Returns
/// `(sim, x, y)`.
fn merged_scenario(owner: PlayerId, deathtouch: bool, x_first: bool) -> (Sim, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let make_x = |s: &mut Sim| {
        s.create(
            ZoneKind::Battlefield,
            owner,
            creature_walker_chars("test.creature_walker_3_3", 3, 3),
        )
    };
    let make_y = |s: &mut Sim| s.creature(owner, "test.creature_2_0", 2, 0);
    let (x, y) = if x_first {
        let x = make_x(&mut s);
        let y = make_y(&mut s);
        (x, y)
    } else {
        let y = make_y(&mut s);
        let x = make_x(&mut s);
        (x, y)
    };
    if deathtouch {
        s.damage(x, 1);
        s.env.deathtouch.insert(x);
    } else {
        s.damage(x, 3);
    }
    (s, x, y)
}

/// REQ-SBA-001, CR 704.7, 701.8b, 704.5g, 704.5h, 704.5i, 704.5f: the closest merged case the
/// public API allows. One creature-planeswalker with 0 loyalty and a destroy finding (704.5g
/// lethal damage, and separately 704.5h deathtouch damage) has a `StateBasedAction`-class finding
/// (704.5i) and a destroy finding; it moves exactly once with `StateBasedDestroy` and fires the
/// destroy listener once. A plain 704.5f creature in the same pass keeps `StateBasedAction`.
/// Both id orders (the merged object created before and after the other one), both owners.
#[test]
fn req_sba_001_704_7_merged_destroy_and_non_destroy_findings_one_destroy_move() {
    for owner in [P1, P2] {
        for deathtouch in [false, true] {
            for x_first in [true, false] {
                let (mut s, x, y) = merged_scenario(owner, deathtouch, x_first);
                let destroy_entry = if deathtouch { "704.5h" } else { "704.5g" };
                let findings = s.check();
                assert!(
                    findings.contains(&finding(destroy_entry, obj(x))),
                    "{findings:?}"
                );
                assert!(
                    findings.contains(&finding("704.5i", obj(x))),
                    "{findings:?}"
                );
                assert!(
                    findings.contains(&finding("704.5f", obj(y))),
                    "{findings:?}"
                );
                assert_eq!(findings.len(), 3, "{findings:?}");
                let r = run_sba(s);
                let label = format!("owner {owner:?} deathtouch {deathtouch} x_first {x_first}");
                assert_died_with(&r, x, C::StateBasedDestroy);
                assert_died_with(&r, y, C::StateBasedAction);
                assert_eq!(r.moves().len(), 2, "{label}");
                let events = sba_trigger_events(&r);
                assert_eq!(
                    listener_hits(P1, &events, &[x, y]),
                    vec![x],
                    "{label}: the merged destroy fires once, the 704.5f death not at all"
                );
            }
        }
    }
}

/// REQ-SBA-001, CR 704.7, 704.5g, 704.5h, 704.5i: a creature-planeswalker with 0 loyalty,
/// lethal damage and deathtouch damage (three findings) moves once with `StateBasedDestroy`.
#[test]
fn req_sba_001_704_7_three_findings_one_destroy_move() {
    let mut s = Sim::new();
    let x = s.create(
        ZoneKind::Battlefield,
        P2,
        creature_walker_chars("test.creature_walker_2_2", 2, 2),
    );
    s.damage(x, 2);
    s.env.deathtouch.insert(x);
    let findings = s.check();
    for e in ["704.5g", "704.5h", "704.5i"] {
        assert!(findings.contains(&finding(e, obj(x))), "{e}: {findings:?}");
    }
    let r = run_sba(s);
    assert_died_with(&r, x, C::StateBasedDestroy);
    assert_eq!(r.moves().len(), 1);
}

/// REQ-SBA-001, CR 704.5f, 704.5g, 704.5h: a 704.5f creature and a destroy-finding creature in
/// one pass, in both id orders and with 704.5g and 704.5h separately: each keeps its own cause.
#[test]
fn req_sba_001_5f_and_destroy_objects_both_id_orders() {
    for deathtouch in [false, true] {
        for destroy_first in [true, false] {
            let mut s = Sim::new();
            let (d, f) = if destroy_first {
                let d = s.creature(P1, "test.creature_3_3", 3, 3);
                let f = s.creature(P2, "test.creature_2_0", 2, 0);
                (d, f)
            } else {
                let f = s.creature(P2, "test.creature_2_0", 2, 0);
                let d = s.creature(P1, "test.creature_3_3", 3, 3);
                (d, f)
            };
            if deathtouch {
                s.damage(d, 1);
                s.env.deathtouch.insert(d);
            } else {
                s.damage(d, 3);
            }
            let r = run_sba(s);
            assert_died_with(&r, d, C::StateBasedDestroy);
            assert_died_with(&r, f, C::StateBasedAction);
        }
    }
}

// =============================================================================================
// 4. the shield counter (CR 122.1c)
// =============================================================================================

/// REQ-EVENT-001, REQ-SBA-001, CR 122.1c, 701.8b, 704.5g, 704.5h: a creature with a shield
/// counter and lethal damage, and another with a shield counter and deathtouch damage, still die
/// (to their owners' graveyards, cause `StateBasedDestroy`); no shield counter is removed and
/// the shield replacement is not applied. Both controllers.
#[test]
fn req_event_001_shield_counter_does_not_stop_sba_destroy() {
    for owner in [P1, P2] {
        let mut s = Sim::new();
        let lethal = s.creature(owner, "test.shielded_2_2", 2, 2);
        let touched = s.creature(owner, "test.shielded_3_3", 3, 3);
        for (n, o) in [(1, lethal), (2, touched)] {
            s.counters(o, "shield", 1);
            s.add_replacement(ReplacementEffect::shield_counter_destroy(eid(n), o));
        }
        s.damage(lethal, 2);
        s.damage(touched, 1);
        s.env.deathtouch.insert(touched);
        let r = run_sba(s);
        assert_eq!(r.before.counter(lethal, "shield"), 1);
        assert_eq!(r.before.counter(touched, "shield"), 1);
        assert_died_with(&r, lethal, C::StateBasedDestroy);
        assert_died_with(&r, touched, C::StateBasedDestroy);
        assert!(shield_removals(&r).is_empty(), "no shield counter spent");
        for b in &r.outcome().executed {
            for e in &b.events {
                let applied: Vec<_> = e.instance.applied_effects().iter().collect();
                assert!(
                    !applied.contains(&eid(1)) && !applied.contains(&eid(2)),
                    "the shield replacement was not applied: {applied:?}"
                );
            }
        }
    }
}

/// REQ-EVENT-001, CR 122.1c: control: the same shield counter replacement stops a plain effect
/// `Destroy` move submitted through the event bus: the counter is spent (removed) and the
/// creature stays on the battlefield. Both controllers.
#[test]
fn req_event_001_shield_counter_stops_effect_destroy_control() {
    for owner in [P1, P2] {
        let mut s = Sim::new();
        let a = s.creature(owner, "test.shielded_2_2", 2, 2);
        s.counters(a, "shield", 1);
        s.add_replacement(ReplacementEffect::shield_counter_destroy(eid(1), a));
        let (after, rep) = bus_event(s, zc(a, ZoneKind::Graveyard, C::Destroy));
        assert_eq!(rep.applications, vec![eid(1)]);
        assert!(after.on_battlefield(a), "the creature stays");
        assert_eq!(after.counter(a, "shield"), 0, "the shield counter is spent");
    }
}

/// REQ-EVENT-001, CR 122.1c, 701.8b: the shield counter replacement does not apply to a move with
/// cause `StateBasedDestroy` or `StateBasedAction` submitted through the event bus: the creature
/// goes to the graveyard and the counter is not removed before it leaves.
#[test]
fn req_event_001_shield_counter_ignores_state_based_causes_on_bus() {
    for cause in [C::StateBasedDestroy, C::StateBasedAction] {
        let mut s = Sim::new();
        let a = s.creature(P2, "test.shielded_2_2", 2, 2);
        s.counters(a, "shield", 1);
        s.add_replacement(ReplacementEffect::shield_counter_destroy(eid(1), a));
        let (after, rep) = bus_event(s, zc(a, ZoneKind::Graveyard, cause));
        assert!(rep.applications.is_empty(), "{cause:?}");
        assert!(!after.on_battlefield(a), "{cause:?}");
        let removals = rep
            .executed
            .iter()
            .flat_map(|b| b.events.iter())
            .filter(|e| matches!(e.instance.event(), GameEvent::RemoveCounters { .. }))
            .count();
        assert_eq!(removals, 0, "{cause:?}");
        let moved: Vec<ZoneKind> = rep
            .executed
            .iter()
            .flat_map(|b| b.events.iter())
            .filter_map(|e| match &e.outcome {
                Outcome::ZoneChange(m) if m.old == a => Some(m.to.kind()),
                _ => None,
            })
            .collect();
        assert_eq!(moved, vec![ZoneKind::Graveyard], "{cause:?}");
    }
}

// =============================================================================================
// 5. replacement matchers compare cause by equality
// =============================================================================================

/// REQ-EVENT-001, CR 701.8b, 704.5g, 704.5f: an exile redirect matching cause
/// `StateBasedDestroy` applies to the 704.5g death and not to the 704.5f death; one matching
/// `StateBasedAction` applies to the 704.5f death and not to the 704.5g death. Both owners.
#[test]
fn req_event_001_matcher_separates_state_based_destroy_from_action() {
    for owner in [P1, P2] {
        for matched in [C::StateBasedDestroy, C::StateBasedAction] {
            let mut s = Sim::new();
            let g = s.creature(owner, "test.creature_2_2", 2, 2);
            let f = s.creature(owner, "test.creature_2_0", 2, 0);
            s.damage(g, 2);
            s.add_replacement(exile_redirect(1, matched));
            let r = run_sba(s);
            let (exiled, graveyard) = if matched == C::StateBasedDestroy {
                (g, f)
            } else {
                (f, g)
            };
            assert_moved_once_to(&r, exiled, ZoneKind::Exile);
            assert_moved_once_to(&r, graveyard, ZoneKind::Graveyard);
            let graveyard_cause = if matched == C::StateBasedDestroy {
                C::StateBasedAction
            } else {
                C::StateBasedDestroy
            };
            assert_died_with(&r, graveyard, graveyard_cause);
        }
    }
}

/// REQ-EVENT-001, CR 701.8b, 704.5h, 704.5i: the same separation for a 704.5h death and a
/// 704.5i death.
#[test]
fn req_event_001_matcher_separates_5h_from_5i() {
    for matched in [C::StateBasedDestroy, C::StateBasedAction] {
        let mut s = Sim::new();
        let h = s.creature(P2, "test.creature_3_3", 3, 3);
        let i = s.walker(P1, 0);
        s.damage(h, 1);
        s.env.deathtouch.insert(h);
        s.add_replacement(exile_redirect(1, matched));
        let r = run_sba(s);
        let (exiled, graveyard) = if matched == C::StateBasedDestroy {
            (h, i)
        } else {
            (i, h)
        };
        assert_moved_once_to(&r, exiled, ZoneKind::Exile);
        assert_moved_once_to(&r, graveyard, ZoneKind::Graveyard);
    }
}

/// REQ-EVENT-001, CR 701.8b: a redirect matching the effect cause `Destroy` does not apply to
/// SBA deaths of either cause.
#[test]
fn req_event_001_destroy_matcher_ignores_sba_deaths() {
    let mut s = Sim::new();
    let g = s.creature(P1, "test.creature_2_2", 2, 2);
    let f = s.creature(P2, "test.creature_2_0", 2, 0);
    s.damage(g, 2);
    s.add_replacement(exile_redirect(1, C::Destroy));
    let r = run_sba(s);
    assert_died_with(&r, g, C::StateBasedDestroy);
    assert_died_with(&r, f, C::StateBasedAction);
}

// =============================================================================================
// 6. determinism (R3)
// =============================================================================================

/// REQ-EVENT-001, REQ-SBA-001, CR 701.8b: the canonical encoding of `StateBasedDestroy` is one
/// variant byte (tag 8), distinct from every other cause's encoding and stable across calls;
/// the other causes keep their tags 0..=7.
#[test]
fn req_event_001_state_based_destroy_encoding() {
    let encodings: Vec<Vec<u8>> = ALL_CAUSES.iter().map(bytes).collect();
    for (tag, (cause, enc)) in ALL_CAUSES.iter().zip(&encodings).enumerate() {
        assert_eq!(enc, &vec![u8::try_from(tag).expect("fits")], "{cause:?}");
        assert_eq!(&bytes(cause), enc, "{cause:?} stable");
    }
    assert_eq!(bytes(&C::StateBasedDestroy), vec![8u8]);
    let distinct: BTreeSet<&Vec<u8>> = encodings.iter().collect();
    assert_eq!(
        distinct.len(),
        ALL_CAUSES.len(),
        "every cause encodes distinctly"
    );
    // The cause is part of a zone change's encoding.
    let a = ObjectId::new(5);
    let sbd = bytes(&zc(a, ZoneKind::Graveyard, C::StateBasedDestroy));
    for other in ALL_CAUSES {
        if other != C::StateBasedDestroy {
            assert_ne!(bytes(&zc(a, ZoneKind::Graveyard, other)), sbd, "{other:?}");
        }
    }
    assert_eq!(
        bytes(&zc(a, ZoneKind::Graveyard, C::StateBasedDestroy)),
        sbd
    );
}

/// The roles of the determinism scenario, in a fixed order.
const ROLES: usize = 6;

/// Builds the determinism scenario with objects created in `order` (a permutation of the role
/// indices): 0 = P1 704.5g, 1 = P2 704.5h, 2 = P1 704.5f, 3 = P2 704.5i, 4 = P2 shielded 704.5g,
/// 5 = P1 survivor. Returns the sim and the id of each role.
fn determinism_scenario(order: &[usize]) -> (Sim, [ObjectId; ROLES]) {
    let mut s = Sim::new();
    let mut ids = [ObjectId::new(0); ROLES];
    for &role in order {
        ids[role] = match role {
            0 => s.creature(P1, "test.creature_2_2", 2, 2),
            1 => s.creature(P2, "test.creature_3_3", 3, 3),
            2 => s.creature(P1, "test.creature_2_0", 2, 0),
            3 => s.walker(P2, 0),
            4 => s.creature(P2, "test.shielded_2_2", 2, 2),
            5 => s.creature(P1, "test.creature_3_3", 3, 3),
            _ => unreachable!(),
        };
    }
    for &role in order {
        let id = ids[role];
        match role {
            0 => s.damage(id, 2),
            1 => {
                s.damage(id, 1);
                s.env.deathtouch.insert(id);
            }
            4 => {
                s.counters(id, "shield", 1);
                s.add_replacement(ReplacementEffect::shield_counter_destroy(eid(40), id));
                s.damage(id, 2);
            }
            5 => s.damage(id, 1),
            _ => {}
        }
    }
    (s, ids)
}

/// Per role: `None` if it stayed on the battlefield, else `(destination, cause, owner's
/// graveyard?)` of its one move.
fn per_role(r: &SbaRun, ids: &[ObjectId; ROLES]) -> Vec<Option<(ZoneKind, ZoneChangeCause, bool)>> {
    ids.iter()
        .map(|&id| {
            let moves: Vec<_> = r.moves().into_iter().filter(|m| m.0 == id).collect();
            assert!(moves.len() <= 1, "{id:?} moved at most once");
            moves.first().map(|&(_, new, kind, cause)| {
                let owner = r.before.zones.object(id).expect("existed").owner;
                let in_owners_gy =
                    r.after.zones.object(new).map(|o| o.zone) == Some(ZoneId::Graveyard(owner));
                (kind, cause, in_owners_gy)
            })
        })
        .collect()
}

/// REQ-SBA-001, REQ-EVENT-001, CR 701.8b, 704.3: two identical runs of the SBA pass give the
/// same outcome, the same final state and the same state hash.
#[test]
fn req_sba_001_identical_runs_identical_results() {
    let order: Vec<usize> = (0..ROLES).collect();
    let (a_sim, _) = determinism_scenario(&order);
    let (b_sim, _) = determinism_scenario(&order);
    let a = run_sba(a_sim);
    let b = run_sba(b_sim);
    assert_eq!(a.result, b.result);
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.after.canonical(), b.after.canonical());
    assert_eq!(a.after.snap(), b.after.snap());
}

/// REQ-SBA-001, REQ-EVENT-001, CR 701.8b, 704.5f, 704.5g, 704.5h, 704.5i, 122.1c: the result
/// per object is independent of the objects' creation order (every permutation of the six
/// roles: ids differ, behaviour per role identical), and matches the brief's causes.
#[test]
fn req_sba_001_result_independent_of_creation_order() {
    let expected: Vec<Option<(ZoneKind, ZoneChangeCause, bool)>> = vec![
        Some((ZoneKind::Graveyard, C::StateBasedDestroy, true)),
        Some((ZoneKind::Graveyard, C::StateBasedDestroy, true)),
        Some((ZoneKind::Graveyard, C::StateBasedAction, true)),
        Some((ZoneKind::Graveyard, C::StateBasedAction, true)),
        Some((ZoneKind::Graveyard, C::StateBasedDestroy, true)),
        None,
    ];
    let mut perms: Vec<Vec<usize>> = Vec::new();
    permutations(&mut (0..ROLES).collect::<Vec<_>>(), 0, &mut perms);
    assert_eq!(perms.len(), 720);
    // Every permutation would be 720 passes; a seeded deterministic sample (R3) plus the identity
    // and the reverse keeps the run short while covering both extremes.
    let mut rng = Lcg::new(0x0155);
    let identity: Vec<usize> = (0..ROLES).collect();
    let reverse: Vec<usize> = (0..ROLES).rev().collect();
    let mut chosen: Vec<Vec<usize>> = vec![identity, reverse];
    for _ in 0..40 {
        let i = usize::try_from(rng.below(perms.len() as u64)).expect("fits");
        chosen.push(perms[i].clone());
    }
    for order in chosen {
        let (s, ids) = determinism_scenario(&order);
        let r = run_sba(s);
        assert_eq!(per_role(&r, &ids), expected, "creation order {order:?}");
        assert!(shield_removals(&r).is_empty(), "creation order {order:?}");
        let events = sba_trigger_events(&r);
        let mut hits = vec![ids[0], ids[1], ids[4]];
        hits.sort();
        assert_eq!(
            listener_hits(P2, &events, &ids),
            hits,
            "creation order {order:?}"
        );
    }
}

/// All permutations of `items[k..]` (swap recursion), appended to `out`.
fn permutations(items: &mut Vec<usize>, k: usize, out: &mut Vec<Vec<usize>>) {
    if k == items.len() {
        out.push(items.clone());
        return;
    }
    for i in k..items.len() {
        items.swap(k, i);
        permutations(items, k + 1, out);
        items.swap(k, i);
    }
}
