// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the object SBAs 704.5d-704.5i, indestructible, the shield counter and the
//! last known information of permanents leaving through SBAs (REQ-SBA-001, REQ-TEST-001;
//! CR 704.5d, 704.5e, 704.5f, 704.5g, 704.5h, 704.5i, 704.8, 702.12b, 702.2b, 122.1c, 111.7,
//! 111.8, 707.10a, 723.3c, 306.5c, 704.5q). Black-box: written from the work-item brief only.

#[path = "spec_sba_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::{CounterTarget, GameEvent, ZoneChangeCause};
use fineprint_core::layers::keyword_counter_ability;
use fineprint_core::replacement::{Action, BusEventKind, EventMatcher, ReplacementEffect};
use fineprint_core::zones::{MoveRequest, ZoneId, ZoneKind};
use support::*;

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

/// Asserts that `id` is still on the battlefield, unmoved.
fn assert_stayed(r: &SbaRun, id: ObjectId) {
    assert!(r.after.on_battlefield(id), "{id:?} stayed");
    assert!(r.moved_to(id).is_none(), "{id:?} did not move");
}

/// Asserts that `id` ceased to exist through this pass.
fn assert_ceased(r: &SbaRun, id: ObjectId) {
    assert!(r.before.zones.object(id).is_some());
    assert!(r.after.zones.object(id).is_none(), "{id:?} ceased");
    assert!(r.outcome().ceased.contains(&id), "{id:?} in ceased");
    assert!(r.moved_to(id).is_none(), "{id:?} was not moved");
}

// ---------------------------------------------------------------------------------------------
// 704.5d
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 704.5d, 111.7, 111.8: a token in a graveyard, a hand and exile
// ceases to exist; a token on the battlefield does not; a nontoken card in a graveyard does not.
#[test]
fn req_sba_001_704_5d_token_off_battlefield_ceases() {
    let mut s = Sim::new();
    let in_gy = s.token(P1, ZoneKind::Graveyard);
    let in_hand = s.token(P2, ZoneKind::Hand);
    let in_exile = s.token(P1, ZoneKind::Exile);
    let on_bf = s.token(P1, ZoneKind::Battlefield);
    let card = s.create(
        ZoneKind::Graveyard,
        P1,
        creature_chars("test.vanilla_2_2", 2, 2),
    );
    let mut expected = vec![
        finding("704.5d", obj(in_gy)),
        finding("704.5d", obj(in_hand)),
        finding("704.5d", obj(in_exile)),
    ];
    expected.sort_by_key(|f| f.subject);
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_eq!(r.outcome().findings, expected);
    for t in [in_gy, in_hand, in_exile] {
        assert_ceased(&r, t);
    }
    let mut ceased = vec![in_gy, in_hand, in_exile];
    ceased.sort();
    assert_eq!(r.outcome().ceased, ceased, "ascending");
    assert_stayed(&r, on_bf);
    assert_eq!(r.after.zone_of(card), Some(ZoneKind::Graveyard));
    assert!(r.moves().is_empty());
    assert!(r.outcome().lost_players.is_empty());
}

// ---------------------------------------------------------------------------------------------
// 704.5e
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 704.5e, 707.10a: a copy of a spell off the stack ceases to exist
// (graveyard, hand, exile); on the stack it does not.
#[test]
fn req_sba_001_704_5e_spell_copy_off_stack_ceases() {
    let mut s = Sim::new();
    let gy = s.spell_copy(P1, ZoneKind::Graveyard);
    let hand = s.spell_copy(P1, ZoneKind::Hand);
    let exile = s.spell_copy(P2, ZoneKind::Exile);
    let stack = s.spell_copy(P1, ZoneKind::Stack);
    let expected = vec![
        finding("704.5e", obj(gy)),
        finding("704.5e", obj(hand)),
        finding("704.5e", obj(exile)),
    ];
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, expected);
    for c in [gy, hand, exile] {
        assert_ceased(&r, c);
    }
    assert_eq!(r.outcome().ceased, vec![gy, hand, exile]);
    assert_eq!(r.after.zone_of(stack), Some(ZoneKind::Stack));
}

// REQ-SBA-001, REQ-TEST-001, CR 704.5e: a copy of a card in a zone other than the stack or the
// battlefield ceases to exist; on the stack and on the battlefield it does not.
#[test]
fn req_sba_001_704_5e_card_copy_outside_stack_and_battlefield_ceases() {
    let mut s = Sim::new();
    let gy = s.card_copy(P1, ZoneKind::Graveyard);
    let hand = s.card_copy(P2, ZoneKind::Hand);
    let exile = s.card_copy(P1, ZoneKind::Exile);
    let library = s.card_copy(P1, ZoneKind::Library);
    let stack = s.card_copy(P1, ZoneKind::Stack);
    let bf = s.card_copy(P1, ZoneKind::Battlefield);
    let expected = vec![
        finding("704.5e", obj(gy)),
        finding("704.5e", obj(hand)),
        finding("704.5e", obj(exile)),
        finding("704.5e", obj(library)),
    ];
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, expected);
    for c in [gy, hand, exile, library] {
        assert_ceased(&r, c);
    }
    assert_eq!(r.outcome().ceased, vec![gy, hand, exile, library]);
    assert_eq!(r.after.zone_of(stack), Some(ZoneKind::Stack));
    assert_stayed(&r, bf);
}

// REQ-SBA-001, CR 704.5e, 723.3c: a copy the environment exempts (the prepared copy in exile)
// survives; the same copy without the exemption ceases.
#[test]
fn req_sba_001_704_5e_exempt_copy_survives() {
    let mut s = Sim::new();
    let prepared = s.card_copy(P1, ZoneKind::Exile);
    let other = s.card_copy(P1, ZoneKind::Exile);
    let spell = s.spell_copy(P1, ZoneKind::Exile);
    s.env.exempt.insert(prepared);
    s.env.exempt.insert(spell);
    assert_eq!(s.check(), vec![finding("704.5e", obj(other))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().ceased, vec![other]);
    assert_eq!(r.after.zone_of(prepared), Some(ZoneKind::Exile));
    assert_eq!(r.after.zone_of(spell), Some(ZoneKind::Exile));
}

// ---------------------------------------------------------------------------------------------
// 704.5f
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 704.5f: a creature with toughness 0 or less goes to its OWNER's
// graveyard (here controlled by the other player); toughness 1 stays.
#[test]
fn req_sba_001_704_5f_zero_or_less_toughness() {
    let mut s = Sim::new();
    let zero = s.create_controlled(
        ZoneKind::Battlefield,
        P2,
        Some(P1),
        creature_chars("test.vanilla_2_0", 2, 0),
    );
    let negative = s.creature(P1, "test.vanilla_1_m1", 1, -1);
    let one = s.creature(P1, "test.vanilla_1_1", 1, 1);
    let expected = vec![
        finding("704.5f", obj(zero)),
        finding("704.5f", obj(negative)),
    ];
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_eq!(r.outcome().findings, expected);
    assert_died(&r, zero);
    assert_died(&r, negative);
    assert_stayed(&r, one);
    let new = r.moved_to(zero).expect("moved");
    assert_eq!(
        r.after.zones.object(new).unwrap().zone,
        ZoneId::Graveyard(P2)
    );
}

// REQ-SBA-001, CR 704.5f, 613.4c: toughness is the layer-evaluated value: a 2/2 under -2/-2
// dies, a 2/2 under -1/-1 stays.
#[test]
fn req_sba_001_704_5f_computed_toughness_from_layer_effect() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let b = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.add_layer_effect(1, modify_pt_effect(&[a], -2, -2, 5));
    s.add_layer_effect(2, modify_pt_effect(&[b], -1, -1, 6));
    assert_eq!(s.check(), vec![finding("704.5f", obj(a))]);
    let r = run_sba(s);
    assert_died(&r, a);
    assert_stayed(&r, b);
}

// REQ-SBA-001, CR 704.5f, 122.1a, 704.5q: P/T counters count: a 1/1 with two -1/-1 counters
// dies, a 1/1 with one +1/+1 and one -1/-1 counter stays (and, since FP-041, 704.5q removes
// one counter of each kind from it in the same pass).
#[test]
fn req_sba_001_704_5f_toughness_from_counters() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_1_1", 1, 1);
    s.counters(a, "-1/-1", 2);
    let b = s.creature(P1, "test.vanilla_1_1", 1, 1);
    s.counters(b, "+1/+1", 1);
    s.counters(b, "-1/-1", 1);
    assert_eq!(
        s.check(),
        vec![finding("704.5f", obj(a)), finding("704.5q", obj(b))]
    );
    let r = run_sba(s);
    assert_died(&r, a);
    assert_stayed(&r, b);
    assert_eq!(r.after.counter(b, "+1/+1"), 0);
    assert_eq!(r.after.counter(b, "-1/-1"), 0);
}

// REQ-SBA-001, CR 704.5f, 704.5g: toughness 0 with damage marked is 704.5f only (704.5g needs
// toughness greater than 0).
#[test]
fn req_sba_001_704_5f_zero_toughness_with_damage_is_5f_only() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_2_0", 2, 0);
    s.damage(a, 1);
    assert_eq!(s.check(), vec![finding("704.5f", obj(a))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5f", obj(a))]);
    assert_died(&r, a);
}

// REQ-SBA-001, CR 704.5f: a noncreature on the battlefield without toughness and a creature card
// with toughness 0 in a graveyard are no 704.5f findings.
#[test]
fn req_sba_001_704_5f_only_creatures_on_the_battlefield() {
    let mut s = Sim::new();
    let artifact = s.create(
        ZoneKind::Battlefield,
        P1,
        blank("test.artifact", &["Artifact"]),
    );
    let card = s.create(
        ZoneKind::Graveyard,
        P1,
        creature_chars("test.vanilla_0_0", 0, 0),
    );
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert_stayed(&r, artifact);
    assert_eq!(r.after.zone_of(card), Some(ZoneKind::Graveyard));
}

// ---------------------------------------------------------------------------------------------
// 704.5g
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 704.5g: damage equal to toughness and greater than toughness is
// lethal (destroyed, cause StateBasedAction); less is not.
#[test]
fn req_sba_001_704_5g_lethal_damage() {
    let mut s = Sim::new();
    let equal = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let more = s.creature(P2, "test.vanilla_2_2", 2, 2);
    let less = s.creature(P1, "test.vanilla_3_3", 3, 3);
    s.damage(equal, 2);
    s.damage(more, 5);
    s.damage(less, 2);
    let expected = vec![finding("704.5g", obj(equal)), finding("704.5g", obj(more))];
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, expected);
    assert_died(&r, equal);
    assert_died(&r, more);
    assert_stayed(&r, less);
    assert_eq!(r.after.events.markers().damage(less), 2);
}

// REQ-SBA-001, CR 704.5g, 613.4c: lethal damage is judged against the evaluated toughness: 2
// damage on a 2/2 with +1/+1 from a layer effect is not lethal; on a 3/3 with -1/-1 it is.
#[test]
fn req_sba_001_704_5g_against_evaluated_toughness() {
    let mut s = Sim::new();
    let pumped = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let shrunk = s.creature(P1, "test.vanilla_3_3", 3, 3);
    s.add_layer_effect(1, modify_pt_effect(&[pumped], 1, 1, 3));
    s.add_layer_effect(2, modify_pt_effect(&[shrunk], -1, -1, 4));
    s.damage(pumped, 2);
    s.damage(shrunk, 2);
    assert_eq!(s.check(), vec![finding("704.5g", obj(shrunk))]);
    let r = run_sba(s);
    assert_died(&r, shrunk);
    assert_stayed(&r, pumped);
}

// ---------------------------------------------------------------------------------------------
// 704.5h
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 704.5h, 702.2b: a creature with toughness > 0 dealt damage by a
// deathtouch source since the last check is destroyed; a damaged creature not in the deathtouch
// set is not; the set is cleared after the pass.
#[test]
fn req_sba_001_704_5h_deathtouch_damage() {
    let mut s = Sim::new();
    let touched = s.creature(P1, "test.vanilla_3_3", 3, 3);
    let plain = s.creature(P1, "test.vanilla_3_3", 3, 3);
    s.damage(touched, 1);
    s.damage(plain, 1);
    s.env.deathtouch.insert(touched);
    assert_eq!(s.check(), vec![finding("704.5h", obj(touched))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5h", obj(touched))]);
    assert_died(&r, touched);
    assert_stayed(&r, plain);
    assert!(r.after.env.deathtouch.is_empty());
    assert_eq!(r.after.env.clear_calls, 1);
}

// REQ-SBA-001, CR 704.5h, 704.5f: the deathtouch flag on a toughness-0 creature gives 704.5f
// only (704.5h needs toughness greater than 0).
#[test]
fn req_sba_001_704_5h_zero_toughness_is_5f_only() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_1_0", 1, 0);
    s.env.deathtouch.insert(a);
    assert_eq!(s.check(), vec![finding("704.5f", obj(a))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5f", obj(a))]);
    assert_died(&r, a);
}

// REQ-SBA-001, CR 704.5g, 704.5h, 704.7: a creature with lethal damage from a deathtouch source
// has two findings (704.5g and 704.5h) and moves once.
#[test]
fn req_sba_001_704_5g_and_5h_on_one_creature_move_once() {
    let mut s = Sim::new();
    let a = s.creature(P2, "test.vanilla_2_2", 2, 2);
    s.damage(a, 2);
    s.env.deathtouch.insert(a);
    let expected = vec![finding("704.5g", obj(a)), finding("704.5h", obj(a))];
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, expected);
    assert_died(&r, a);
    assert_eq!(r.moves().len(), 1);
    let batches = r.nonempty_batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].events.len(), 1, "one member per object");
}

// ---------------------------------------------------------------------------------------------
// 704.5i
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 704.5i, 306.5c: a planeswalker with 0 loyalty goes to its
// owner's graveyard; with 1 loyalty it stays; a creature without loyalty is no 704.5i finding.
#[test]
fn req_sba_001_704_5i_zero_loyalty() {
    let mut s = Sim::new();
    let zero = s.walker(P2, 0);
    let one = s.walker(P1, 1);
    let three = s.walker(P1, 3);
    let creature = s.creature(P1, "test.vanilla_2_2", 2, 2);
    assert_eq!(s.check(), vec![finding("704.5i", obj(zero))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5i", obj(zero))]);
    assert_died(&r, zero);
    assert_stayed(&r, one);
    assert_stayed(&r, three);
    assert_stayed(&r, creature);
}

// REQ-SBA-001, CR 704.5i, 306.5c: loyalty is the number of loyalty counters: a planeswalker
// whose counters were all removed has loyalty 0.
#[test]
fn req_sba_001_704_5i_loyalty_counters_removed() {
    let mut s = Sim::new();
    let w = s.walker(P1, 2);
    s.layer
        .counters
        .get_mut(&w)
        .expect("counters")
        .remove("loyalty");
    s.ok(fineprint_core::events::GameEvent::RemoveCounters {
        target: fineprint_core::events::CounterTarget::Object(w),
        kind: "loyalty".to_owned(),
        amount: 2,
    });
    assert_eq!(s.check(), vec![finding("704.5i", obj(w))]);
    let r = run_sba(s);
    assert_died(&r, w);
}

// ---------------------------------------------------------------------------------------------
// indestructible
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 702.12b, 704.5g, 704.5h: an indestructible creature (base keyword) ignores
// lethal damage and deathtouch damage.
#[test]
fn req_sba_001_indestructible_survives_5g_and_5h() {
    let mut s = Sim::new();
    let lethal = s.create(
        ZoneKind::Battlefield,
        P1,
        with_keyword(
            creature_chars("test.indestructible_2_2", 2, 2),
            "indestructible",
        ),
    );
    let touched = s.create(
        ZoneKind::Battlefield,
        P1,
        with_keyword(
            creature_chars("test.indestructible_2_2", 2, 2),
            "indestructible",
        ),
    );
    s.damage(lethal, 4);
    s.damage(touched, 1);
    s.env.deathtouch.insert(touched);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert_stayed(&r, lethal);
    assert_stayed(&r, touched);
}

// REQ-SBA-001, CR 702.12b, 122.1b, 704.5g: an indestructible counter (keyword from layer 6)
// also ignores lethal damage.
#[test]
fn req_sba_001_indestructible_counter_survives_5g() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.counters(a, "indestructible", 1);
    s.damage(a, 3);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert_stayed(&r, a);
}

// REQ-SBA-001, CR 702.12b, 704.5f: an indestructible creature with toughness 0 still dies
// (704.5f is not destruction); the keyword-counter ability form is used here.
#[test]
fn req_sba_001_indestructible_dies_to_5f() {
    let mut s = Sim::new();
    let mut c = creature_chars("test.indestructible_1_1", 1, 1);
    c.abilities.push(keyword_counter_ability("indestructible"));
    let a = s.create(ZoneKind::Battlefield, P1, c);
    s.add_layer_effect(1, modify_pt_effect(&[a], -1, -1, 2));
    s.damage(a, 1);
    s.env.deathtouch.insert(a);
    assert_eq!(s.check(), vec![finding("704.5f", obj(a))]);
    let r = run_sba(s);
    assert_died(&r, a);
}

// ---------------------------------------------------------------------------------------------
// shield counter
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 122.1c, 704.5g, 704.5h: a shield counter's destroy replacement does not stop
// SBA destruction (it matches only `Destroy`; SBA moves use cause StateBasedAction): both the
// lethal-damage and the deathtouch creature go to the graveyard and keep no shield use.
#[test]
fn req_sba_001_shield_counter_does_not_stop_5g_or_5h() {
    let mut s = Sim::new();
    let lethal = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let touched = s.creature(P1, "test.vanilla_3_3", 3, 3);
    for (n, o) in [(1, lethal), (2, touched)] {
        s.counters(o, "shield", 1);
        s.add_replacement(ReplacementEffect::shield_counter_destroy(eid(n), o));
    }
    s.damage(lethal, 2);
    s.damage(touched, 1);
    s.env.deathtouch.insert(touched);
    let r = run_sba(s);
    assert_eq!(
        r.outcome().findings,
        vec![
            finding("704.5g", obj(lethal)),
            finding("704.5h", obj(touched))
        ]
    );
    assert_died(&r, lethal);
    assert_died(&r, touched);
}

// ---------------------------------------------------------------------------------------------
// last known information (CR 704.8)
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.8, 704.5f, 704.5g: a creature with a +1/+1 counter killed by -1/-1
// counters leaves in the same pass as a lethal-damage creature; both LKI records describe the
// permanents as they were before the pass (battlefield, counters, permanent state).
#[test]
fn req_sba_001_704_8_lki_from_before_the_pass() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_1_1", 1, 1);
    s.counters(a, "+1/+1", 1);
    s.counters(a, "-1/-1", 2);
    let b = s.creature(P2, "test.vanilla_2_2", 2, 2);
    s.damage(b, 2);
    assert!(s.lki.get(a).is_none());
    let r = run_sba(s);
    // CR 704.5q (FP-041) also names `a`, which has both kinds of P/T counters.
    assert_eq!(
        r.outcome().findings,
        vec![
            finding("704.5f", obj(a)),
            finding("704.5g", obj(b)),
            finding("704.5q", obj(a))
        ]
    );
    assert_died(&r, a);
    assert_died(&r, b);
    let la = r.after.lki.get(a).expect("LKI of a");
    assert_eq!(la.zone(), ZoneId::Battlefield);
    assert_eq!(la.owner(), P1);
    let mut counters = BTreeMap::new();
    counters.insert("+1/+1".to_owned(), 1);
    counters.insert("-1/-1".to_owned(), 2);
    assert_eq!(la.counters(), &counters);
    assert_eq!(
        la.characteristics(),
        &creature_chars("test.vanilla_1_1", 1, 1)
    );
    assert!(la.permanent().is_some(), "permanent state captured");
    let lb = r.after.lki.get(b).expect("LKI of b");
    assert_eq!(lb.zone(), ZoneId::Battlefield);
    assert_eq!(lb.controller(), Some(P2));
    assert!(lb.permanent().is_some());
    // Every executed move captured LKI. (Only zone changes: whether the 704.5q removal on `a`,
    // which also leaves in this pass, appears as an event is left open by the FP-041 brief.)
    let mut moves = 0;
    for batch in &r.outcome().executed {
        for e in &batch.events {
            if matches!(e.instance.event(), GameEvent::ZoneChange { .. }) {
                assert!(e.lki_captured, "{e:?}");
                moves += 1;
            }
        }
    }
    assert_eq!(moves, 2);
}

// REQ-SBA-001, REQ-CORE-003, CR 704.8, 704.5f, 704.5g: LKI is captured before the pass, not
// just before each move. A (toughness 0) and B (2/2, 3 damage, a +1/+1 counter) are found in
// one pass; an Instead on A's SBA move first removes B's +1/+1 counter, then moves A. B leaves
// after that removal, yet its LKI still holds the counter: a per-move capture would record no
// counter and fail this test.
#[test]
fn req_sba_001_704_8_lki_captured_before_an_earlier_batch_member_changes_it() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_1_0", 1, 0);
    let b = s.creature(P2, "test.vanilla_2_2", 2, 2);
    s.counters(b, "+1/+1", 1);
    s.damage(b, 3);
    s.add_replacement(ReplacementEffect::new(
        eid(1),
        EventMatcher {
            affected_object: Some(a),
            cause: Some(ZoneChangeCause::StateBasedAction),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        },
        Action::Instead(vec![
            GameEvent::RemoveCounters {
                target: CounterTarget::Object(b),
                kind: "+1/+1".into(),
                amount: 1,
            },
            GameEvent::ZoneChange {
                object: a,
                request: MoveRequest::new(ZoneKind::Graveyard),
                cause: ZoneChangeCause::StateBasedAction,
            },
        ]),
    ));
    assert_eq!(
        s.check(),
        vec![finding("704.5f", obj(a)), finding("704.5g", obj(b))]
    );
    let r = run_sba(s);
    assert_eq!(
        r.outcome().findings,
        vec![finding("704.5f", obj(a)), finding("704.5g", obj(b))]
    );
    assert_died(&r, a);
    assert_died(&r, b);
    // The removal really happened in the pass, before B's move.
    let removed = r
        .outcome()
        .executed
        .iter()
        .flat_map(|batch| batch.events.iter())
        .any(|e| matches!(e.instance.event(), GameEvent::RemoveCounters { .. }));
    assert!(removed, "the Instead removed B's counter in the same pass");
    let lb = r.after.lki.get(b).expect("LKI of b");
    assert_eq!(lb.zone(), ZoneId::Battlefield);
    assert_eq!(lb.owner(), P2);
    assert_eq!(
        lb.counters().get("+1/+1"),
        Some(&1),
        "B's LKI is from before the pass (CR 704.8)"
    );
    let la = r.after.lki.get(a).expect("LKI of a");
    assert_eq!(la.zone(), ZoneId::Battlefield);
    assert_eq!(la.owner(), P1);
}
