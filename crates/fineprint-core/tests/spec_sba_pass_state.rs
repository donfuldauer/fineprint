// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for what one SBA pass changes and reports (REQ-SBA-001, REQ-CORE-003,
//! REQ-TEST-001; CR 704.3, 704.5d, 704.5e, 704.5f, 704.5g, 704.5h, 704.8, 702.2b, 702.12b,
//! 707.10a, 400.7, 111.7, 111.8). Black-box: written from the work-item brief only.
//!
//! - a card copy that ceases to exist after an effect moved it earlier in the same pass is
//!   reported (and removed) under its new id; the copy kind persists across that move, and the
//!   battlefield and the copy exemption still protect a card copy;
//! - a token that has left the battlefield can't change zones (CR 111.8): an effect's move of
//!   it does nothing, and it ceases under its original id (CR 111.7, 704.5d);
//! - `SbaOutcome.performed` is true iff the pass changed game state (a move, a cessation, a
//!   lost player, or a replacement-registry change such as a used-up one-use shield), not
//!   merely because there are findings;
//! - the SBAs read toughness through the layer system, whatever `eval_value` says about
//!   literals;
//! - a copy or token that ceases gets a last-known-information record (no zone change);
//! - the deathtouch window closes at every completed check, even one in which the creature was
//!   indestructible;
//! - the test harness forgets the provider descriptions of retired ids.

#[path = "spec_sba_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{ChoiceSession, ChoiceType, Status};
use fineprint_core::events::{EventContext, GameEvent, ZoneChangeCause};
use fineprint_core::layers::{LayerEnvironment, MatchContext, ObjectBase};
use fineprint_core::memory::CharacteristicsProvider;
use fineprint_core::replacement::{
    Action, BusEventKind, EventMatcher, Modification, ReplacementEffect, Shield,
};
use fineprint_core::sba::{SbaRegistry, check};
use fineprint_core::txn::Transactional;
use fineprint_core::zones::{MoveRequest, ZoneId, ZoneKind};
use fineprint_dsl::card::ValueExpressionAST;
use fineprint_dsl::expr::ObjectSelectorAST;
use support::*;

/// Matches the SBA zone change of `object`.
fn sba_move_of(object: ObjectId) -> EventMatcher {
    EventMatcher {
        affected_object: Some(object),
        cause: Some(ZoneChangeCause::StateBasedAction),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    }
}

fn zone_change(object: ObjectId, to: ZoneKind, cause: ZoneChangeCause) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause,
    }
}

/// Asserts that `old` left the battlefield for its owner's graveyard through one SBA move.
fn assert_died(r: &SbaRun, old: ObjectId) {
    let owner = r.before.zones.object(old).expect("existed").owner;
    assert!(r.after.zones.object(old).is_none(), "{old:?} left");
    let moves: Vec<_> = r.moves().into_iter().filter(|m| m.0 == old).collect();
    assert_eq!(moves.len(), 1, "{old:?} moved exactly once");
    let (_, new, kind, cause) = moves[0];
    assert_eq!(kind, ZoneKind::Graveyard);
    assert_eq!(cause, ZoneChangeCause::StateBasedAction);
    assert_eq!(
        r.after.zones.object(new).expect("moved object").zone,
        ZoneId::Graveyard(owner)
    );
}

// ---------------------------------------------------------------------------------------------
// (a) cessation after a move made earlier in the same pass (CR 704.5d, 704.5e, 400.7)
// ---------------------------------------------------------------------------------------------

/// The other object of an F-A board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Other {
    /// A card copy (`is_card_copy`) in exile.
    CardCopyInExile,
    /// A card copy in exile exempt from the copy SBA.
    ExemptCardCopyInExile,
    /// A token in a graveyard.
    TokenInGraveyard,
}

/// A 0-toughness creature A (704.5f) and the object `other` C, created in the order given by
/// `creature_first`; an Instead on A's SBA move: [A to the graveyard (SBA), C to `dest`
/// (cause Other)]. Returns the board, A and C.
fn moved_by_instead(
    other: Other,
    dest: ZoneKind,
    creature_first: bool,
) -> (Sim, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let make_other = |s: &mut Sim| match other {
        Other::CardCopyInExile => s.card_copy(P1, ZoneKind::Exile),
        Other::ExemptCardCopyInExile => {
            let c = s.card_copy(P1, ZoneKind::Exile);
            s.env.exempt.insert(c);
            c
        }
        Other::TokenInGraveyard => s.token(P1, ZoneKind::Graveyard),
    };
    let (a, c) = if creature_first {
        let a = s.creature(P1, "test.vanilla_1_0", 1, 0);
        let c = make_other(&mut s);
        (a, c)
    } else {
        let c = make_other(&mut s);
        let a = s.creature(P1, "test.vanilla_1_0", 1, 0);
        (a, c)
    };
    s.add_replacement(ReplacementEffect::new(
        eid(1),
        sba_move_of(a),
        Action::Instead(vec![
            zone_change(a, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
            zone_change(c, dest, ZoneChangeCause::Other),
        ]),
    ));
    (s, a, c)
}

/// Runs the board twice from clones and asserts both runs are identical (CR 704.3, R3).
fn run_deterministic(s: &Sim) -> SbaRun {
    let r1 = run_sba(s.clone());
    let r2 = run_sba(s.clone());
    assert_eq!(r1.result, r2.result);
    assert_eq!(r1.hash, r2.hash);
    assert_eq!(r1.after.state_hash(), r2.after.state_hash());
    assert_eq!(r1.after.canonical(), r2.after.canonical());
    r1
}

/// The ceasing case: C moved to `dest` by the Instead ceases under its new id in the same pass,
/// and nothing of it remains.
fn assert_ceases_under_new_id(other: Other, dest: ZoneKind, creature_first: bool) {
    let (s, a, c) = moved_by_instead(other, dest, creature_first);
    let card = s.zones.object(c).expect("present").card;
    let r = run_deterministic(&s);
    let o = r.outcome();
    let new = r
        .moved_to(c)
        .unwrap_or_else(|| panic!("the Instead moved C: {:?} / {:?}", r.moves(), o));
    assert_ne!(new, c);
    assert_eq!(o.ceased, vec![new], "{other:?} first={creature_first}");
    assert!(!o.ceased.contains(&c));
    assert!(o.performed);
    assert!(r.after.zones.object(new).is_none());
    assert!(r.after.zones.object(c).is_none());
    assert!(r.after.zones.object_of_card(card).is_none());
    let all = r.after.zones.all_object_ids();
    assert!(!all.contains(&new) && !all.contains(&c));
    assert_died(&r, a);
}

/// The surviving case: C moved to `dest` by the Instead stays there.
fn assert_survives_move(other: Other, dest: ZoneKind, creature_first: bool) {
    let (s, a, c) = moved_by_instead(other, dest, creature_first);
    let r = run_deterministic(&s);
    let o = r.outcome();
    let new = r.moved_to(c).expect("the Instead moved C");
    assert!(
        o.ceased.is_empty(),
        "{other:?} first={creature_first}: {:?}",
        o.ceased
    );
    assert!(o.performed);
    assert_eq!(r.after.zone_of(new), Some(dest));
    assert_died(&r, a);
}

// REQ-SBA-001, CR 704.5e, 707.10a, 400.7, 704.3: a card copy in exile moved to a hand by an
// Instead on another object's SBA move ceases in the same pass, reported under its new id;
// both creation orders.
#[test]
fn req_sba_001_card_copy_moved_by_instead_ceases_under_new_id() {
    for creature_first in [true, false] {
        assert_ceases_under_new_id(Other::CardCopyInExile, ZoneKind::Hand, creature_first);
    }
}

// REQ-SBA-001, CR 704.5e: a card copy moved by such an Instead to the battlefield stays (a
// copy of a card may be on the battlefield); both creation orders.
#[test]
fn req_sba_001_card_copy_moved_to_battlefield_by_instead_stays() {
    for creature_first in [true, false] {
        assert_survives_move(
            Other::CardCopyInExile,
            ZoneKind::Battlefield,
            creature_first,
        );
    }
}

// REQ-SBA-001, CR 111.7, 111.8, 704.5d: a token in a graveyard that such an Instead would move
// to exile can't change zones (CR 111.8: it remains in its current zone instead), so no move of
// it is executed and it is never in exile; A still dies, and the token ceases in the same pass
// under its ORIGINAL id (CR 111.7, 704.5d), with LKI recording its graveyard zone; both
// creation orders, deterministic.
#[test]
fn req_sba_001_token_named_by_instead_stays_put_and_ceases() {
    for creature_first in [true, false] {
        let (s, a, t) = moved_by_instead(Other::TokenInGraveyard, ZoneKind::Exile, creature_first);
        let card = s.zones.object(t).expect("present").card;
        assert_eq!(
            s.zones.object(t).expect("present").zone,
            ZoneId::Graveyard(P1)
        );
        let r = run_deterministic(&s);
        let o = r.outcome();
        // CR 111.8: no zone change of the token is executed, and nothing reaches exile.
        assert!(
            r.moves().iter().all(|m| m.0 != t && m.2 != ZoneKind::Exile),
            "first={creature_first}: {:?}",
            r.moves()
        );
        assert!(r.moved_to(t).is_none());
        // CR 111.7, 704.5d: it ceases under its original id in the same pass.
        assert_eq!(o.ceased, vec![t], "first={creature_first}");
        assert!(o.performed);
        assert!(r.after.zones.object(t).is_none());
        assert!(r.after.zones.object_of_card(card).is_none());
        assert!(!r.after.zones.all_object_ids().contains(&t));
        assert!(r.after.zones.contents(ZoneId::Exile).is_empty());
        let lt = r.after.lki.get(t).expect("LKI of the ceased token");
        assert_eq!(lt.zone(), ZoneId::Graveyard(P1));
        assert_eq!(lt.owner(), P1);
        assert_died(&r, a);
    }
}

// REQ-SBA-001, CR 704.5e: an exempt card copy moved to a hand by such an Instead stays; both
// creation orders.
#[test]
fn req_sba_001_exempt_card_copy_moved_by_instead_stays() {
    for creature_first in [true, false] {
        assert_survives_move(Other::ExemptCardCopyInExile, ZoneKind::Hand, creature_first);
    }
}

// ---------------------------------------------------------------------------------------------
// (b) `performed` means "the pass changed game state" (CR 704.3)
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.3, 704.5f: a 0-toughness creature whose SBA move a persistent Skip
// replaces: each pass finds 704.5f, executes nothing, changes nothing (but the deathtouch
// clear call) and reports `performed == false`; five passes in a row.
#[test]
fn req_sba_001_persistent_skip_is_not_performed() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_1_0", 1, 0);
    s.add_replacement(ReplacementEffect::new(eid(1), sba_move_of(a), Action::Skip));
    for i in 0..5 {
        let r = run_sba(s.clone());
        let o = r.outcome();
        assert!(!o.performed, "pass {i}: nothing changed");
        assert_eq!(o.findings, vec![finding("704.5f", obj(a))], "pass {i}");
        assert!(o.executed.is_empty(), "pass {i}: {:?}", o.executed);
        assert!(o.ceased.is_empty());
        assert!(o.lost_players.is_empty());
        assert!(r.after.on_battlefield(a), "pass {i}");
        let mut after = r.after.clone();
        after.env.clear_calls = s.env.clear_calls;
        assert_eq!(
            after.canonical(),
            s.canonical(),
            "pass {i}: state unchanged"
        );
        s = r.after;
    }
}

// REQ-SBA-001, CR 704.3, 704.5f: a one-use Skip on the SBA move: pass 1 uses it up (a registry
// change, so `performed`) and the creature stays; pass 2 moves it (`performed`).
#[test]
fn req_sba_001_one_use_skip_is_performed() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_1_0", 1, 0);
    s.add_replacement(
        ReplacementEffect::new(eid(1), sba_move_of(a), Action::Skip).with_shield(Shield::Uses(1)),
    );
    let r1 = run_sba(s);
    assert!(r1.outcome().performed, "the one-use shield was consumed");
    assert_eq!(r1.outcome().findings, vec![finding("704.5f", obj(a))]);
    assert!(r1.after.on_battlefield(a));
    assert!(r1.after.replacements.is_empty(), "the use is consumed");
    let r2 = run_sba(r1.after.clone());
    assert!(r2.outcome().performed);
    assert_died(&r2, a);
}

// REQ-SBA-001, CR 704.3, 704.5a: a pass whose only effect is a player losing is performed.
#[test]
fn req_sba_001_lost_player_only_is_performed() {
    let mut s = Sim::new();
    s.set_life(P2, 0);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_eq!(r.outcome().lost_players, vec![P2]);
    assert!(r.nonempty_batches().is_empty());
    assert!(r.after.lost(P2));
}

// REQ-SBA-001, CR 704.3, 704.5d: a pass whose only effect is a token ceasing is performed.
#[test]
fn req_sba_001_ceased_only_is_performed() {
    let mut s = Sim::new();
    let t = s.token(P1, ZoneKind::Hand);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_eq!(r.outcome().ceased, vec![t]);
    assert!(r.nonempty_batches().is_empty());
    assert!(r.after.zones.object(t).is_none());
}

// ---------------------------------------------------------------------------------------------
// (c) toughness comes from the layer system, not from `eval_value` (CR 704.5f, 704.5g)
// ---------------------------------------------------------------------------------------------

/// The support layer environment with `matches` false and `eval_value` always `None`.
struct NoneEval(LayerEnv);

impl LayerEnvironment for NoneEval {
    fn objects(&self) -> Vec<ObjectId> {
        self.0.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.0.base(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.0.counters(id)
    }

    fn matches(&self, _selector: &ObjectSelectorAST, _ctx: &MatchContext<'_>) -> bool {
        false
    }

    fn eval_value(&self, _expr: &ValueExpressionAST, _object: ObjectId) -> Option<i64> {
        None
    }
}

// REQ-SBA-001, CR 704.5f, 704.5g: with a layer environment whose `eval_value` returns `None`
// even for literals, `check` still reports 704.5f for a 0-toughness creature and 704.5g for a
// lethal-damage one: the same findings as the plain environment.
#[test]
fn req_sba_001_check_independent_of_eval_value_for_literals() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_1_0", 1, 0);
    let b = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.damage(b, 2);
    let plain = s.check();
    assert_eq!(
        plain,
        vec![finding("704.5f", obj(a)), finding("704.5g", obj(b))]
    );
    let wrapped = NoneEval(s.layer.clone());
    let Sim {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        provider,
        effects,
        env,
        ..
    } = &mut s;
    let ctx = EventContext {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        provider,
    };
    let with_none = check(&SbaRegistry::standard(), &ctx, effects, &wrapped, env);
    assert_eq!(with_none, plain);
}

// ---------------------------------------------------------------------------------------------
// (d) last known information of objects that cease (CR 704.8, REQ-CORE-003)
// ---------------------------------------------------------------------------------------------

/// A spell copy on the battlefield owned by `owner`, described as a 2/2 creature with a
/// +1/+1 counter.
fn battlefield_spell_copy(s: &mut Sim, owner: PlayerId) -> ObjectId {
    let id = s.spell_copy(owner, ZoneKind::Battlefield);
    s.describe(id, creature_chars("test.spell_copy_2_2", 2, 2));
    s.counters(id, "+1/+1", 1);
    id
}

// REQ-SBA-001, REQ-CORE-003, CR 704.5e, 704.8, 707.10a: a spell copy on the battlefield that
// ceases has LKI from before the pass (battlefield, owner, counters, permanent state); a token
// in a graveyard that ceases has LKI with zone Graveyard; the cessation is no zone change.
#[test]
fn req_sba_001_ceased_objects_have_lki() {
    let mut s = Sim::new();
    let copy = battlefield_spell_copy(&mut s, P2);
    let token = s.token(P1, ZoneKind::Graveyard);
    assert!(s.lki.get(copy).is_none());
    let r = run_sba(s);
    let o = r.outcome();
    let mut ceased = o.ceased.clone();
    ceased.sort();
    let mut expected = vec![copy, token];
    expected.sort();
    assert_eq!(ceased, expected);
    assert!(r.moved_to(copy).is_none(), "ceasing is not a zone change");
    assert!(r.moved_to(token).is_none(), "ceasing is not a zone change");
    assert!(r.moves().is_empty());

    let lc = r.after.lki.get(copy).expect("LKI of the ceased spell copy");
    assert_eq!(lc.zone(), ZoneId::Battlefield);
    assert_eq!(lc.owner(), P2);
    assert_eq!(lc.counters().get("+1/+1"), Some(&1));
    assert!(lc.permanent().is_some(), "permanent state captured");

    let lt = r.after.lki.get(token).expect("LKI of the ceased token");
    assert_eq!(lt.zone(), ZoneId::Graveyard(P1));
    assert_eq!(lt.owner(), P1);
}

// REQ-SBA-001, REQ-CORE-003, CR 704.5e: a ceasing card copy the characteristics provider does
// not describe ceases without an LKI record and without an error.
#[test]
fn req_sba_001_undescribed_ceased_object_has_no_lki() {
    let mut s = Sim::new();
    let card = s.fresh_card();
    let id = s
        .zones
        .create_object(&mut s.ids, card, P1, MoveRequest::new(ZoneKind::Graveyard))
        .expect("create succeeds");
    // Known to the layer environment only; the provider has no description and no fallback.
    s.layer.bases.insert(
        id,
        ObjectBase {
            characteristics: creature_chars("test.card_copy_2_2", 2, 2),
            controller: Some(P1),
            on_battlefield: false,
        },
    );
    s.env.card_copies.insert(id);
    s.provider.clear_fallback();
    assert!(s.provider.characteristics(id).is_none());
    let lki_before = bytes(&s.lki);
    let r = run_sba(s);
    let o = r.outcome();
    assert_eq!(o.ceased, vec![id]);
    assert!(r.after.zones.object(id).is_none());
    assert!(
        r.after.lki.get(id).is_none(),
        "no LKI without a description"
    );
    assert_eq!(bytes(&r.after.lki), lki_before);
}

// REQ-SBA-001, REQ-CORE-003, CR 704.3, 704.8: a pass that also ceases a battlefield spell copy
// and suspends on a CHOOSE_REPLACEMENT_EFFECT leaves the LKI store unchanged while pending;
// after the answer the copy has LKI, and a replay reaches an identical state hash.
#[test]
fn req_sba_001_ceased_lki_atomic_on_pending_choice() {
    let mut s = Sim::new();
    let dying = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.damage(dying, 2);
    let copy = battlefield_spell_copy(&mut s, P2);
    for (n, to) in [(1, ZoneKind::Exile), (2, ZoneKind::Hand)] {
        s.add_replacement(
            ReplacementEffect::new(
                eid(n),
                EventMatcher {
                    affected_object: Some(dying),
                    from: Some(ZoneKind::Battlefield),
                    to: Some(ZoneKind::Graveyard),
                    ..EventMatcher::new(BusEventKind::ZoneChange)
                },
                Action::Modify(Modification::SetDestination(to)),
            )
            .with_description(format!("test.effect_{n}")),
        );
    }
    let base = s;
    let lki_before = bytes(&base.lki);
    // `call_sba` asserts every component (the LKI store included) is unchanged when pending.
    let session = ChoiceSession::start(base.clone(), call_sba).expect("starts");
    let req = session.pending().cloned().expect("a choice is pending");
    assert_eq!(req.choice_type, ChoiceType::ChooseReplacementEffect);
    assert_eq!(bytes(&session.base().lki), lki_before);
    assert!(session.base().lki.get(copy).is_none());

    let r = drive_sba(base.clone(), |_, _| select(0));
    assert_eq!(r.requests.len(), 1);
    assert!(r.outcome().ceased.contains(&copy));
    let lc = r.after.lki.get(copy).expect("LKI of the ceased copy");
    assert_eq!(lc.zone(), ZoneId::Battlefield);
    assert_eq!(lc.counters().get("+1/+1"), Some(&1));

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

// ---------------------------------------------------------------------------------------------
// (e) the deathtouch window closes at every completed check (CR 704.5h, 702.2b, 702.12b)
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5h, 702.2b, 702.12b: an indestructible 2/2 flagged as dealt deathtouch
// damage (no marked damage) survives pass 1, which consumes the flag. With indestructible
// removed, pass 2 does not destroy it (no stale 704.5h); a new deathtouch hit then does.
#[test]
fn req_sba_001_deathtouch_window_closes_while_indestructible() {
    let mut s = Sim::new();
    let a = s.create(
        ZoneKind::Battlefield,
        P1,
        with_keyword(
            creature_chars("test.indestructible_2_2", 2, 2),
            "indestructible",
        ),
    );
    s.env.deathtouch.insert(a);
    let calls = s.env.clear_calls;
    let r1 = run_sba(s);
    assert!(r1.after.on_battlefield(a));
    assert!(r1.moved_to(a).is_none());
    assert!(
        r1.after.env.deathtouch.is_empty(),
        "the check closed the window"
    );
    assert_eq!(r1.after.env.clear_calls, calls + 1);

    let mut s = r1.after;
    // Indestructible ends (rebuild its abilities without the keyword).
    s.describe(a, creature_chars("test.indestructible_2_2", 2, 2));
    assert!(s.check().is_empty(), "no stale 704.5h");
    let r2 = run_sba(s);
    assert!(!r2.outcome().performed);
    assert!(r2.outcome().findings.is_empty());
    assert!(r2.after.on_battlefield(a), "not destroyed");

    let mut s = r2.after;
    s.env.deathtouch.insert(a);
    assert_eq!(s.check(), vec![finding("704.5h", obj(a))]);
    let r3 = run_sba(s);
    assert!(r3.outcome().performed);
    assert_died(&r3, a);
}

// ---------------------------------------------------------------------------------------------
// the harness: `Sim::sync_after` retires provider descriptions
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 400.7: after a pass in which one object moved (704.5f) and one
// ceased (704.5d), `sync_after` leaves the provider with no characteristics and no counters for
// either old id; the moved object's new id is described.
#[test]
fn req_sba_001_sync_after_retires_provider_descriptions() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_1_0", 1, 0);
    s.counters(a, "-1/-1", 1);
    let t = s.token(P2, ZoneKind::Hand);
    assert!(s.provider.characteristics(a).is_some());
    assert!(!s.provider.counters(a).is_empty());
    let r = run_sba(s);
    let outcome = r.outcome().clone();
    assert_eq!(outcome.ceased, vec![t]);
    let new = r.moved_to(a).expect("a died");
    let mut after = r.after.clone();
    after.sync_after(&outcome);
    for old in [a, t] {
        assert!(after.provider.characteristics(old).is_none(), "{old:?}");
        assert!(after.provider.counters(old).is_empty(), "{old:?}");
        assert!(!after.layer.bases.contains_key(&old));
        assert!(!after.layer.counters.contains_key(&old));
    }
    assert_eq!(
        after.provider.characteristics(new),
        Some(creature_chars("test.vanilla_1_0", 1, 0))
    );
    assert!(after.provider.counters(new).is_empty(), "CR 400.7");
}
