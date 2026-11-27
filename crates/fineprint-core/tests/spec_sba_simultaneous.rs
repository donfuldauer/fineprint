// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for performing all applicable SBAs simultaneously as one event, the CR 704.7
//! object-level merge on the replacement bus, atomicity and replay of a pass suspended on a
//! replacement choice, and the purity of `check` (REQ-SBA-001, REQ-TEST-001, REQ-TURN-001;
//! CR 704.3, 704.4, 704.7, 704.8, 616.1, and 704.5j-704.5t together with 704.5a-704.5i). Black-box: written from the work-item brief only.

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::choice::{ChoicePayload, ChoiceSession, ChoiceType, Status};
use fineprint_core::events::{GameEvent, ZoneChangeCause};
use fineprint_core::replacement::{
    Action, BusEventKind, EventMatcher, Modification, ReplacementEffect, Shield,
};
use fineprint_core::sba::SbaSubject;
use fineprint_core::txn::Transactional;
use fineprint_core::zones::ZoneKind;
use support::*;

/// Matches a permanent going from the battlefield to a graveyard.
fn dies() -> EventMatcher {
    EventMatcher {
        from: Some(ZoneKind::Battlefield),
        to: Some(ZoneKind::Graveyard),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    }
}

/// A ZoneChange matcher for `object`.
fn dies_object(object: ObjectId) -> EventMatcher {
    EventMatcher {
        affected_object: Some(object),
        ..dies()
    }
}

/// An effect `n` that sends a dying permanent to `to` instead.
fn redirect(n: u64, matcher: EventMatcher, to: ZoneKind) -> ReplacementEffect {
    ReplacementEffect::new(
        eid(n),
        matcher,
        Action::Modify(Modification::SetDestination(to)),
    )
    .with_description(format!("test.effect_{n}"))
}

/// What a full board created in `order` produced: the kinds of objects, by role.
struct Board {
    sim: Sim,
    zero_toughness: ObjectId,
    lethal: ObjectId,
    walker: ObjectId,
    token: ObjectId,
    spell_copy: ObjectId,
}

/// A board with a 0-toughness creature, a lethal-damage creature, a 0-loyalty planeswalker, a
/// token in a graveyard, a spell copy off the stack and a 0-life player, the objects created in
/// the role order `order` (a permutation of 0..5).
fn board(order: &[usize]) -> Board {
    let mut s = Sim::new();
    let mut ids = [ObjectId::new(0); 5];
    for &role in order {
        ids[role] = match role {
            0 => s.creature(P1, "test.vanilla_2_0", 2, 0),
            1 => {
                let c = s.creature(P2, "test.vanilla_2_2", 2, 2);
                s.damage(c, 2);
                c
            }
            2 => s.walker(P1, 0),
            3 => s.token(P2, ZoneKind::Graveyard),
            4 => s.spell_copy(P1, ZoneKind::Exile),
            _ => unreachable!(),
        };
    }
    s.set_life(P2, 0);
    Board {
        sim: s,
        zero_toughness: ids[0],
        lethal: ids[1],
        walker: ids[2],
        token: ids[3],
        spell_copy: ids[4],
    }
}

/// The outcome shape independent of concrete ObjectIds: finding entry ids with the subject
/// kind, number of nonempty batches, moves per batch, losers, ceased count.
type Shape = (
    Vec<(&'static str, bool)>,
    usize,
    Vec<usize>,
    Vec<fineprint_base::ids::PlayerId>,
    usize,
);

fn shape(r: &SbaRun) -> Shape {
    let mut f: Vec<(&'static str, bool)> = r
        .outcome()
        .findings
        .iter()
        .map(|f| (f.entry_id, matches!(f.subject, SbaSubject::Player(_))))
        .collect();
    f.sort();
    let batches = r.nonempty_batches();
    (
        f,
        batches.len(),
        batches.iter().map(|b| b.events.len()).collect(),
        r.outcome().lost_players.clone(),
        r.outcome().ceased.len(),
    )
}

// REQ-SBA-001, REQ-TURN-001, CR 704.3, 704.5a, 704.5d, 704.5e, 704.5f, 704.5g, 704.5i: one
// `perform_all` call performs every applicable SBA as one event: one outcome, one executed batch
// holding all moves, all findings, all effects applied. The same shape for three different
// object creation (ObjectId) orders, including the reverse order.
#[test]
fn req_sba_001_all_sbas_performed_simultaneously() {
    let orders: [[usize; 5]; 3] = [[0, 1, 2, 3, 4], [4, 3, 2, 1, 0], [2, 4, 0, 3, 1]];
    let mut shapes = Vec::new();
    for order in orders {
        let b = board(&order);
        let mut expected = vec![
            finding("704.5a", player(P2)),
            finding("704.5d", obj(b.token)),
            finding("704.5e", obj(b.spell_copy)),
            finding("704.5f", obj(b.zero_toughness)),
            finding("704.5g", obj(b.lethal)),
            finding("704.5i", obj(b.walker)),
        ];
        expected.sort_by_key(|f| {
            (
                ENTRY_IDS.iter().position(|id| *id == f.entry_id).unwrap(),
                f.subject,
            )
        });
        let r = run_sba(b.sim);
        let o = r.outcome();
        assert!(o.performed, "{order:?}");
        assert_eq!(o.findings, expected, "{order:?}");
        // One batch holding the three moves, each to its owner's graveyard, as SBA moves.
        let batches = r.nonempty_batches();
        assert_eq!(batches.len(), 1, "{order:?}: one batch");
        assert_eq!(batches[0].events.len(), 3, "{order:?}");
        let moves = r.moves();
        assert_eq!(moves.len(), 3);
        for (old, new, kind, cause) in &moves {
            assert_eq!(*kind, ZoneKind::Graveyard);
            // CR 701.8b: the 704.5g (lethal damage) move destroys; the others are plain SBA moves.
            let expected = if *old == b.lethal {
                ZoneChangeCause::StateBasedDestroy
            } else {
                ZoneChangeCause::StateBasedAction
            };
            assert_eq!(*cause, expected);
            let owner = r.before.zones.object(*old).unwrap().owner;
            assert_eq!(
                r.after.zones.object(*new).unwrap().zone,
                fineprint_core::zones::ZoneId::Graveyard(owner)
            );
        }
        for o2 in [b.zero_toughness, b.lethal, b.walker] {
            assert!(r.after.zones.object(o2).is_none(), "{order:?}");
            assert!(r.moved_to(o2).is_some(), "{order:?}");
        }
        let mut ceased = vec![b.token, b.spell_copy];
        ceased.sort();
        assert_eq!(o.ceased, ceased, "{order:?}");
        assert!(r.after.zones.object(b.token).is_none());
        assert!(r.after.zones.object(b.spell_copy).is_none());
        assert_eq!(o.lost_players, vec![P2]);
        assert!(r.after.lost(P2));
        assert!(!r.after.lost(P1));
        // The batch is logged once.
        assert_eq!(
            r.after.events.log().len(),
            r.before.events.log().len() + 1,
            "{order:?}"
        );
        shapes.push(shape(&r));
    }
    assert!(
        shapes.windows(2).all(|w| w[0] == w[1]),
        "{:?}",
        shapes.len()
    );
}

// REQ-SBA-001, CR 704.3, 704.4: all predicates are evaluated against one snapshot before any
// change: every finding of `check` is in the outcome, and a second pass right after finds
// nothing of the performed kinds.
#[test]
fn req_sba_001_one_snapshot_then_repeat_finds_nothing() {
    let b = board(&[0, 1, 2, 3, 4]);
    let mut s = b.sim;
    let checked = s.check();
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, checked);
    let mut after = r.after.clone();
    after.sync_after(r.outcome());
    assert!(after.check().is_empty());
    let r2 = run_sba(after);
    assert!(!r2.outcome().performed);
}

// REQ-SBA-001, CR 704.3: `check` is pure: the state (including its hash and the empty-draw
// flag) and the env are unchanged afterwards, and it can be repeated with the same result.
#[test]
fn req_sba_001_check_is_pure() {
    let b = board(&[0, 1, 2, 3, 4]);
    let mut s = b.sim;
    s.empty_draw(P1);
    s.env.deathtouch.insert(b.lethal);
    let before = s.snap();
    let hash = s.state_hash();
    let game_hash = s.game.state_hash();
    let first = s.check();
    let second = s.check();
    assert_eq!(first, second);
    assert!(!first.is_empty());
    assert_eq!(s.snap(), before);
    assert_eq!(s.state_hash(), hash);
    assert_eq!(s.game.state_hash(), game_hash);
    assert_eq!(s.events.empty_library_draws(), vec![P1]);
    assert_eq!(s.env.clear_calls, 0);
    assert!(s.env.deathtouch.contains(&b.lethal));
}

// REQ-SBA-001, CR 704.3, 704.5b, 704.5h: `perform_all` with no findings returns
// `performed == false` and changes nothing except consuming the empty-draw flag and clearing
// the deathtouch set (a lost player's draw flag and a deathtouch flag on a card off the
// battlefield).
#[test]
fn req_sba_001_no_findings_changes_only_flags() {
    let mut s = Sim::new();
    let bear = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.damage(bear, 1);
    let card = s.create(
        ZoneKind::Graveyard,
        P2,
        creature_chars("test.vanilla_2_2", 2, 2),
    );
    s.mark_lost(P2);
    s.empty_draw(P2);
    s.env.deathtouch.insert(card);
    assert!(s.check().is_empty());
    let mut expected = s.clone();
    expected.events.take_empty_library_draws();
    expected.env.deathtouch.clear();
    expected.env.clear_calls += 1;
    let r = run_sba(s);
    let o = r.outcome();
    assert!(!o.performed);
    assert!(o.findings.is_empty());
    assert!(r.nonempty_batches().is_empty());
    assert!(o.lost_players.is_empty());
    assert!(o.loss_reasons.is_empty());
    assert!(o.ceased.is_empty());
    assert_eq!(r.after.snap(), expected.snap());
    assert_eq!(r.after.state_hash(), expected.state_hash());

    // An entirely empty pass: only the clear call is observable.
    let r2 = run_sba(Sim::new());
    assert!(!r2.outcome().performed);
    assert_eq!(r2.after.env.clear_calls, 1);
    let mut e2 = Sim::new();
    e2.env.clear_calls = 1;
    assert_eq!(r2.after.snap(), e2.snap());
}

// REQ-SBA-001, CR 704.5h: `clear_deathtouch_damaged` is called after every successful pass.
#[test]
fn req_sba_001_deathtouch_cleared_every_pass() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_3_3", 3, 3);
    s.damage(a, 1);
    s.env.deathtouch.insert(a);
    let r1 = run_sba(s);
    assert!(r1.outcome().performed);
    assert_eq!(r1.after.env.clear_calls, 1);
    assert!(r1.after.env.deathtouch.is_empty());
    let r2 = run_sba(r1.after.clone());
    assert!(!r2.outcome().performed);
    assert_eq!(r2.after.env.clear_calls, 2);
}

// REQ-SBA-001, CR 704.7, 616.1: two different creatures dying to SBAs with a one-use exile
// redirect that matches both: different objects are different results, so the redirect applies
// once (to one of them) and is used up; the other goes to the graveyard.
#[test]
fn req_sba_001_704_7_one_use_redirect_different_objects() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let b = s.creature(P1, "test.vanilla_1_0", 1, 0);
    s.damage(a, 2);
    s.add_replacement(redirect(1, dies(), ZoneKind::Exile).with_shield(Shield::Uses(1)));
    let r = run_sba(s);
    let mut kinds: Vec<ZoneKind> = r.moves().iter().map(|m| m.2).collect();
    kinds.sort();
    let mut expected = vec![ZoneKind::Exile, ZoneKind::Graveyard];
    expected.sort();
    assert_eq!(kinds, expected);
    assert!(r.after.replacements.is_empty(), "the use is consumed");
    assert!(r.after.zones.object(a).is_none());
    assert!(r.after.zones.object(b).is_none());
    assert_eq!(r.nonempty_batches().len(), 1);
}

// REQ-SBA-001, CR 704.7: one creature with both 704.5g and 704.5h is one result: a one-use
// exile redirect applies once, the creature moves once (exiled).
#[test]
fn req_sba_001_704_7_same_object_two_findings_one_application() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.damage(a, 2);
    s.env.deathtouch.insert(a);
    s.add_replacement(redirect(1, dies(), ZoneKind::Exile).with_shield(Shield::Uses(1)));
    let r = run_sba(s);
    assert_eq!(r.outcome().findings.len(), 2);
    let moves = r.moves();
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].0, a);
    assert_eq!(moves[0].2, ZoneKind::Exile);
    assert!(r.after.replacements.is_empty());
}

// ---------------------------------------------------------------------------------------------
// atomicity and replay
// ---------------------------------------------------------------------------------------------

/// A board whose pass needs a replacement choice: a lethal-damage creature with two competing
/// redirects (exile, hand), plus a 0-life player, a token in a graveyard, an empty-draw flag and
/// a deathtouch flag.
fn choice_board() -> (Sim, ObjectId, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let dying = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.damage(dying, 2);
    let touched = s.creature(P2, "test.vanilla_3_3", 3, 3);
    s.damage(touched, 1);
    s.env.deathtouch.insert(touched);
    let token = s.token(P2, ZoneKind::Graveyard);
    s.set_life(P2, 0);
    s.empty_draw(P1);
    s.add_replacement(redirect(1, dies_object(dying), ZoneKind::Exile));
    s.add_replacement(redirect(2, dies_object(dying), ZoneKind::Hand));
    (s, dying, touched, token)
}

// REQ-SBA-001, CR 704.3, 616.1: a pass that needs a CHOOSE_REPLACEMENT_EFFECT answer suspends
// with nothing changed (designations, tokens, flags, registry: asserted inside the procedure
// by `call_sba`), completes after the answer, and a replay with the same answers reaches an
// identical state hash. Both answers.
#[test]
fn req_sba_001_pending_choice_is_atomic_and_replayable() {
    for answer in [0u32, 1u32] {
        let (base, dying, touched, token) = choice_board();
        let base_snap = base.snap();
        let session = ChoiceSession::start(base.clone(), call_sba).expect("starts");
        let req = session.pending().cloned().expect("a choice is pending");
        assert_eq!(req.choice_type, ChoiceType::ChooseReplacementEffect);
        assert_eq!(req.chooser, P1);
        let offered = match &req.payload {
            ChoicePayload::ChooseReplacementEffect { effects, .. } => effects.clone(),
            other => panic!("unexpected payload {other:?}"),
        };
        assert_eq!(offered.len(), 2);
        // The visible state while pending is the base state.
        assert_eq!(session.base().snap(), base_snap);
        assert!(session.base().events.empty_library_draws().contains(&P1));
        assert!(session.base().zones.object(token).is_some());
        assert!(!session.base().lost(P2));
        assert!(session.base().env.deathtouch.contains(&touched));

        let r = drive_sba(base.clone(), |_, _| select(answer));
        assert_eq!(r.requests.len(), 1);
        let chosen = &offered[answer as usize];
        let dest = if chosen == "test.effect_1" {
            ZoneKind::Exile
        } else {
            ZoneKind::Hand
        };
        let moved = r.moved_to(dying).expect("moved");
        assert_eq!(r.after.zone_of(moved), Some(dest), "answer {answer}");
        assert!(r.moved_to(touched).is_some());
        assert!(r.after.lost(P2));
        assert!(r.after.lost(P1), "the empty draw loses (704.5b)");
        assert!(r.after.zones.object(token).is_none());
        assert!(r.after.events.empty_library_draws().is_empty());
        assert!(r.after.env.deathtouch.is_empty());
        assert_eq!(r.nonempty_batches().len(), 1);

        let replay = ChoiceSession::replay(base, call_sba, &r.answers).expect("replays");
        assert_eq!(replay.state_hash(), r.hash);
        match replay.status() {
            Status::Done { state, output } => {
                assert_eq!(state.snap(), r.after.snap());
                assert_eq!(output, &r.result);
            }
            Status::Pending(p) => panic!("replay still pending: {p:?}"),
        }
    }
}

// REQ-SBA-001, CR 704.3: the same input gives the same outcome and state hash (determinism).
#[test]
fn req_sba_001_perform_all_is_deterministic() {
    let (base, ..) = choice_board();
    let a = drive_sba(base.clone(), |_, _| select(0));
    let b = drive_sba(base, |_, _| select(0));
    assert_eq!(a.result, b.result);
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.after.snap(), b.after.snap());
    let b1 = board(&[0, 1, 2, 3, 4]);
    let b2 = board(&[0, 1, 2, 3, 4]);
    let r1 = run_sba(b1.sim);
    let r2 = run_sba(b2.sim);
    assert_eq!(r1.result, r2.result);
    assert_eq!(r1.hash, r2.hash);
}

// REQ-SBA-001, CR 704.3: a pass leaves the executed events in the event log as one batch whose
// members are all SBA ZoneChanges (no other event kinds join the batch).
#[test]
fn req_sba_001_batch_members_are_sba_zone_changes() {
    let b = board(&[1, 0, 2, 3, 4]);
    let lethal = b.lethal;
    let r = run_sba(b.sim);
    let batch = &r.nonempty_batches()[0];
    for e in &batch.events {
        // CR 701.8b: the lethal-damage member is a `StateBasedDestroy` move; the rest are
        // `StateBasedAction` moves.
        let GameEvent::ZoneChange { object, cause, .. } = e.instance.event() else {
            panic!("not a ZoneChange: {e:?}");
        };
        let expected = if *object == lethal {
            ZoneChangeCause::StateBasedDestroy
        } else {
            ZoneChangeCause::StateBasedAction
        };
        assert_eq!(*cause, expected, "{e:?}");
    }
    let logged = r
        .after
        .events
        .log()
        .batches()
        .last()
        .expect("logged")
        .clone();
    assert_eq!(&logged, batch);
}

// ---------------------------------------------------------------------------------------------
// CR 704.5j-704.5t together with 704.5a-704.5i (FP-041)
// ---------------------------------------------------------------------------------------------

/// The roles of [`full_board`], by id.
struct FullBoard {
    sim: Sim,
    token: ObjectId,
    zero: ObjectId,
    legends: [ObjectId; 2],
    worlds: [ObjectId; 2],
    aura: ObjectId,
    eq: ObjectId,
    attached: ObjectId,
    pt: ObjectId,
    limited: ObjectId,
    saga: ObjectId,
    dungeon: ObjectId,
}

/// One board with one result of each of 704.5d, 704.5f and 704.5j-704.5t (and 704.5a when
/// `zero_life`), in `variant`.
fn full_board(variant: fineprint_core::sba::GameVariant, zero_life: bool) -> FullBoard {
    use fineprint_core::permanent::AttachTarget;
    let mut s = Sim::new();
    s.env.variant = variant;
    let token = s.token(P2, ZoneKind::Graveyard);
    let zero = s.creature(P1, "test.vanilla_2_0", 2, 0);
    let legends = [
        s.legend(P1, P1, "test.legend_a"),
        s.legend(P2, P1, "test.legend_a"),
    ];
    let worlds = [
        s.world(P1, P1, "test.world_a", Some(1)),
        s.world(P2, P2, "test.world_b", Some(2)),
    ];
    let aura = s.permanent(P2, P2, aura_chars("test.aura_a"));
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach(eq, AttachTarget::Player(P2));
    let host = s.creature(P2, "test.vanilla_3_3", 3, 3);
    let attached = s.creature(P2, "test.vanilla_1_1", 1, 1);
    s.attach_to(attached, host);
    let pt = s.creature(P1, "test.vanilla_4_4", 4, 4);
    s.counters(pt, "+1/+1", 1);
    s.counters(pt, "-1/-1", 3);
    let limited = s.permanent(P2, P2, artifact_chars("test.artifact_a"));
    s.counters(limited, "charge", 3);
    s.limit(limited, "charge", 1);
    let saga = s.saga(P2, P1, Some(2), 2);
    let dungeon = s.dungeon(P1, true);
    if zero_life {
        s.set_life(P2, 0);
    }
    FullBoard {
        sim: s,
        token,
        zero,
        legends,
        worlds,
        aura,
        eq,
        attached,
        pt,
        limited,
        saga,
        dungeon,
    }
}

// REQ-SBA-001, REQ-TURN-001, CR 704.3, 704.5a, 704.5d, 704.5f, 704.5j, 704.5k, 704.5m, 704.5n,
// 704.5p, 704.5q, 704.5r, 704.5s, 704.5t: one pass performs the old and the new results
// together, in one event; findings are in registry order; the old results are unchanged.
#[test]
fn req_sba_001_old_and_new_entries_in_one_pass() {
    let b = full_board(fineprint_core::sba::GameVariant::Standard, true);
    let expected = vec![
        finding("704.5a", SbaSubject::Player(P2)),
        finding("704.5d", obj(b.token)),
        finding("704.5f", obj(b.zero)),
        finding("704.5j", obj(b.legends[0])),
        finding("704.5j", obj(b.legends[1])),
        finding("704.5k", obj(b.worlds[0])),
        finding("704.5m", obj(b.aura)),
        finding("704.5n", obj(b.eq)),
        finding("704.5p", obj(b.attached)),
        finding("704.5q", obj(b.pt)),
        finding("704.5r", obj(b.limited)),
        finding("704.5s", obj(b.saga)),
        finding("704.5t", obj(b.dungeon)),
    ];
    let mut sim = b.sim.clone();
    assert_eq!(sim.check(), expected);
    let kept = b.legends[1];
    let r = drive_sba(b.sim, |_, _| keep(kept));
    let o = r.outcome();
    assert_eq!(o.findings, expected);
    assert!(o.performed);
    assert_eq!(r.requests.len(), 1);
    // Old results.
    assert!(r.after.lost(P2));
    assert_eq!(o.lost_players, vec![P2]);
    assert_eq!(o.ceased, vec![b.token]);
    assert_to_owners_graveyard(&r, b.zero, ZoneChangeCause::StateBasedAction);
    // New results.
    assert_still_on_battlefield(&r, kept);
    assert_to_owners_graveyard(&r, b.legends[0], ZoneChangeCause::StateBasedAction);
    assert_to_owners_graveyard(&r, b.worlds[0], ZoneChangeCause::StateBasedAction);
    assert_still_on_battlefield(&r, b.worlds[1]);
    assert_to_owners_graveyard(&r, b.aura, ZoneChangeCause::StateBasedAction);
    assert_eq!(o.unattached, vec![b.eq, b.attached]);
    assert_eq!(r.after.permanents.attached_to(b.eq), None);
    assert_eq!(r.after.permanents.attached_to(b.attached), None);
    assert_still_on_battlefield(&r, b.eq);
    assert_still_on_battlefield(&r, b.attached);
    assert_eq!(r.after.counter(b.pt, "+1/+1"), 0);
    assert_eq!(r.after.counter(b.pt, "-1/-1"), 2);
    assert_eq!(r.after.counter(b.limited, "charge"), 1);
    assert_to_owners_graveyard(&r, b.saga, ZoneChangeCause::Sacrifice);
    assert_eq!(o.removed_from_game, vec![b.dungeon]);
    assert!(r.after.zones.object(b.dungeon).is_none());
    assert_eq!(r.nonempty_batches().len(), 1, "one event (CR 704.3)");
    assert_eq!(r.moves().len(), 5, "zero, legend, world, aura, saga");
}

// REQ-SBA-001, CR 704.5j, 704.5k, 704.5m, 704.5n, 704.5p, 704.5q, 704.5r, 704.5s, 704.5t:
// every new entry is active in Two-Headed Giant: `check` and the pass report the same new
// findings as in a standard game.
#[test]
fn req_sba_001_new_entries_active_in_two_headed_giant() {
    use fineprint_core::sba::GameVariant;
    let new_ids = |f: &[fineprint_core::sba::SbaFinding]| -> Vec<&'static str> {
        f.iter()
            .map(|x| x.entry_id)
            .filter(|id| NEW_ENTRY_IDS.contains(id))
            .collect()
    };
    let mut standard = full_board(GameVariant::Standard, false).sim;
    let mut thg = full_board(GameVariant::TwoHeadedGiant, false);
    let a = standard.check();
    let b = thg.sim.check();
    assert_eq!(a, b);
    let mut seen: Vec<&str> = new_ids(&b);
    seen.dedup();
    assert_eq!(seen, NEW_ENTRY_IDS.to_vec());
    let kept = thg.legends[0];
    let r = drive_sba(thg.sim, |_, _| keep(kept));
    assert_eq!(new_ids(&r.outcome().findings), new_ids(&b));
    assert_eq!(r.outcome().removed_from_game, vec![thg.dungeon]);
}

// REQ-SBA-001, CR 704.5n, 704.5p, 701.3d, 704.3 (PR #214 round 1, F2): in the full pass the
// Equipment and the attached creature each get exactly one executed `GameEvent::Unattach`,
// in ascending id order among the Unattach events; no other object gets one; the pass is still one event.
#[test]
fn req_sba_001_f2_full_pass_unattach_events_last() {
    let b = full_board(fineprint_core::sba::GameVariant::Standard, true);
    let kept = b.legends[1];
    let r = drive_sba(b.sim, |_, _| keep(kept));
    let mut both = vec![b.eq, b.attached];
    both.sort();
    assert_eq!(r.outcome().unattached, both);
    assert_eq!(unattach_events(&r), both, "one each, ascending id");
    assert_eq!(r.nonempty_batches().len(), 1, "one event (CR 704.3)");
    assert_eq!(r.after.permanents.attached_to(b.eq), None);
    assert_eq!(r.after.permanents.attached_to(b.attached), None);
}
