// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for prevention and shields on the replacement bus, and the counter-backed
//! effects (REQ-EVENT-001; CR 615.1, 615.6, 615.7, 615.8, 614.7, 614.7a, 120.8, 122.1c,
//! 122.1d, 122.1h, 701.26b, 615.13). Simultaneous damage a shield-less prevention effect prevents
//! for one recipient queues its follow-up once per batch (review round 2, G3). Black-box: written
//! from the work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_core::events::{GameEvent, ZoneChangeCause};
use fineprint_core::replacement::{Action, BusEventKind, EventMatcher, ReplacementEffect, Shield};
use fineprint_core::zones::ZoneKind;
use support::*;

fn prevent(n: u64) -> ReplacementEffect {
    fx(n, m_dmg_player(P2), Action::Prevent { then: Vec::new() })
}

/// The state after a run, as the base of the next run.
fn next(r: &Run) -> Bus {
    r.after.clone()
}

// REQ-EVENT-001, CR 615.7, 615.8: a `Uses(2)` prevention shield prevents two damage events, then is
// gone from the registry and the third damage event is dealt.
#[test]
fn req_event_001_uses_shield_consumed_then_removed() {
    let bus = Bus::new(World::new(), vec![prevent(1).with_shield(Shield::Uses(2))]);
    let r1 = run_event(bus, dmg_player(P2, 3), &[]);
    assert_eq!(r1.report().prevented, 3);
    assert_eq!(r1.applications(), eids(&[1]));
    assert_eq!(r1.world().life(P2), STARTING_LIFE);
    assert_eq!(r1.world().log_len(), r1.before.world.log_len());
    assert!(r1.after.registry.get(eid(1)).is_some());

    let r2 = run_event(next(&r1), dmg_player(P2, 3), &[]);
    assert_eq!(r2.report().prevented, 3);
    assert_eq!(r2.world().life(P2), STARTING_LIFE);
    assert!(r2.after.registry.get(eid(1)).is_none());
    assert!(r2.after.registry.is_empty());

    let r3 = run_event(next(&r2), dmg_player(P2, 3), &[]);
    assert_eq!(r3.report().prevented, 0);
    assert!(r3.applications().is_empty());
    assert_eq!(r3.world().life(P2), STARTING_LIFE - 3);
}

// REQ-EVENT-001, CR 615.7: a `Pool(3)` shield prevents 2 of 2 damage (1 left), then 1 of 4
// (3 dealt) and is removed.
#[test]
fn req_event_001_pool_shield_drains() {
    let bus = Bus::new(World::new(), vec![prevent(1).with_shield(Shield::Pool(3))]);
    let initial = bytes(&bus.registry);
    let r1 = run_event(bus, dmg_player(P2, 2), &[]);
    assert_eq!(r1.report().prevented, 2);
    assert_eq!(r1.world().life(P2), STARTING_LIFE);
    assert_eq!(r1.world().log_len(), r1.before.world.log_len());
    assert!(r1.after.registry.get(eid(1)).is_some());
    assert_ne!(
        bytes(&r1.after.registry),
        initial,
        "the pool remaining is encoded"
    );

    let r2 = run_event(next(&r1), dmg_player(P2, 4), &[]);
    assert_eq!(r2.report().prevented, 1);
    assert_eq!(r2.applications(), eids(&[1]));
    assert_eq!(r2.world().life(P2), STARTING_LIFE - 3);
    assert!(r2.after.registry.get(eid(1)).is_none());
}

// REQ-EVENT-001, CR 615.7, 615.6: a pool larger than the damage prevents all of it; the event is
// dropped (nothing logged) and the effect stays.
#[test]
fn req_event_001_pool_larger_than_damage_drops_event() {
    let bus = Bus::new(World::new(), vec![prevent(1).with_shield(Shield::Pool(5))]);
    let r = run_event(bus, dmg_player(P2, 3), &[]);
    assert_eq!(r.report().prevented, 3);
    assert!(r.executed().is_empty());
    assert!(r.new_log().is_empty());
    assert!(r.after.registry.get(eid(1)).is_some());
}

// REQ-EVENT-001, CR 615.1, 615.6: a "prevent all" effect without a shield prevents every damage event
// and stays; the prevented total is reported and nothing is logged.
#[test]
fn req_event_001_prevent_all_without_shield() {
    let bus = Bus::new(World::new(), vec![prevent(1)]);
    let r1 = run_event(bus, dmg_player(P2, 5), &[]);
    assert_eq!(r1.report().prevented, 5);
    assert!(r1.new_log().is_empty());
    assert!(r1.executed().is_empty());
    let r2 = run_event(next(&r1), dmg_player(P2, 7), &[]);
    assert_eq!(r2.report().prevented, 7);
    assert_eq!(r2.world().life(P2), STARTING_LIFE);
    assert!(r2.after.registry.get(eid(1)).is_some());
}

// REQ-EVENT-001, CR 615.1: `BusReport::prevented` is the total over the batch.
#[test]
fn req_event_001_prevented_total_over_batch() {
    let bus = Bus::new(World::new(), vec![prevent(1)]);
    let r = run_batch(
        bus,
        vec![dmg_player(P2, 2), dmg_player(P2, 3), dmg_player(P1, 4)],
        false,
        &[],
    );
    assert_eq!(r.report().prevented, 5);
    assert_eq!(r.applications(), eids(&[1, 1]));
    assert_eq!(r.world().life(P2), STARTING_LIFE);
    assert_eq!(r.world().life(P1), STARTING_LIFE - 4);
}

// REQ-EVENT-001, CR 122.1c, 615.1: the counter-backed damage shield prevents the damage and
// removes exactly one shield counter through a child RemoveCounters event (a second executed
// batch after the parent batch).
#[test]
fn req_event_001_shield_counter_prevents_damage() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(obj, "shield", 2);
    let bus = Bus::new(
        w,
        vec![ReplacementEffect::shield_counter_damage(eid(1), obj)],
    );
    let r = run_batch(bus, vec![dmg_object(obj, 3), dmg_player(P2, 1)], false, &[]);
    assert_eq!(r.report().prevented, 3);
    assert_eq!(r.applications(), eids(&[1]));
    let w2 = r.world();
    assert_eq!(w2.counter(obj, "shield"), 1);
    assert_eq!(w2.events.markers().damage(obj), 0);
    assert_eq!(w2.life(P2), STARTING_LIFE - 1);
    let batches = r.nonempty_batches();
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].events.len(), 1);
    assert_eq!(batches[0].events[0].instance.event(), &dmg_player(P2, 1));
    assert_eq!(batches[1].events.len(), 1);
    assert_eq!(
        batches[1].events[0].instance.event(),
        &remove_counters(obj, "shield", 1)
    );
    assert_eq!(r.new_log().len(), 2);
}

// REQ-EVENT-001, CR 122.1c, 615.1: the damage shield alone on one event: one logged batch, the
// RemoveCounters child; no damage.
#[test]
fn req_event_001_shield_counter_single_event_logs_only_removal() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(obj, "shield", 1);
    let bus = Bus::new(
        w,
        vec![ReplacementEffect::shield_counter_damage(eid(1), obj)],
    );
    let r = run_event(bus, dmg_object(obj, 2), &[]);
    assert_eq!(r.report().prevented, 2);
    assert_eq!(r.world().counter(obj, "shield"), 0);
    let log = r.new_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].events.len(), 1);
    assert_eq!(
        log[0].events[0].instance.event(),
        &remove_counters(obj, "shield", 1)
    );
    assert!(
        r.executed()
            .iter()
            .all(|e| !matches!(e.instance.event(), GameEvent::Damage { .. }))
    );
}

// REQ-EVENT-001, CR 122.1c: without a shield counter the damage shield does not apply.
#[test]
fn req_event_001_shield_counter_absent_prevents_nothing() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    let bus = Bus::new(
        w,
        vec![ReplacementEffect::shield_counter_damage(eid(1), obj)],
    );
    let r = run_event(bus, dmg_object(obj, 3), &[]);
    assert_eq!(r.report().prevented, 0);
    assert!(r.applications().is_empty());
    assert_eq!(r.world().events.markers().damage(obj), 3);
}

// REQ-EVENT-001, CR 122.1c: a shield counter replaces destruction: the object stays on the
// battlefield and loses one shield counter. Another cause is not affected.
#[test]
fn req_event_001_shield_counter_replaces_destroy_only() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(obj, "shield", 2);
    let effects = vec![ReplacementEffect::shield_counter_destroy(eid(1), obj)];
    let r = run_event(
        Bus::new(w.clone(), effects.clone()),
        zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
        &[],
    );
    assert_eq!(r.applications(), eids(&[1]));
    assert_eq!(r.world().zone_of(obj), Some(ZoneKind::Battlefield));
    assert_eq!(r.world().counter(obj, "shield"), 1);

    let r = run_event(
        Bus::new(w, effects),
        zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Sacrifice),
        &[],
    );
    assert!(r.applications().is_empty());
    assert_eq!(r.world().zone_of(obj), None);
    let moved = new_id(&r.executed()[0].outcome);
    assert_eq!(r.world().zone_of(moved), Some(ZoneKind::Graveyard));
}

// REQ-EVENT-001, CR 122.1d: a stun counter replaces untapping: the permanent stays tapped and
// loses one stun counter.
#[test]
fn req_event_001_stun_counter_replaces_untap() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.ok(GameEvent::Tap { object: obj });
    w.put_counters(obj, "stun", 2);
    let bus = Bus::new(w, vec![ReplacementEffect::stun_counter(eid(1), obj)]);
    let r = run_event(bus, GameEvent::Untap { object: obj }, &[]);
    assert_eq!(r.applications(), eids(&[1]));
    assert!(r.world().tapped(obj));
    assert_eq!(r.world().counter(obj, "stun"), 1);
}

// REQ-EVENT-001, CR 122.1h: a finality counter turns battlefield-to-graveyard into exile; a
// hand-to-graveyard move is not affected.
#[test]
fn req_event_001_finality_counter() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(obj, "finality", 1);
    let bus = Bus::new(w, vec![ReplacementEffect::finality_counter(eid(1), obj)]);
    let r = run_event(
        bus,
        zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
        &[],
    );
    assert_eq!(r.applications(), eids(&[1]));
    let moved = new_id(&r.executed()[0].outcome);
    assert_eq!(r.world().zone_of(moved), Some(ZoneKind::Exile));

    let mut w = World::new();
    let card = w.create(ZoneKind::Hand, P1);
    w.put_counters(card, "finality", 1);
    let bus = Bus::new(w, vec![ReplacementEffect::finality_counter(eid(1), card)]);
    let r = run_event(
        bus,
        zone_change(card, ZoneKind::Graveyard, ZoneChangeCause::Discard),
        &[],
    );
    assert!(r.applications().is_empty());
    let moved = new_id(&r.executed()[0].outcome);
    assert_eq!(r.world().zone_of(moved), Some(ZoneKind::Graveyard));
}

// REQ-EVENT-001, CR 122.1c, SRS 8.1 (child events go through the bus): the child
// RemoveCounters of a shield counter can itself be replaced; skipping it leaves the counter.
#[test]
fn req_event_001_shield_counter_removal_reenters_bus() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(obj, "shield", 1);
    let skip = fx(
        2,
        EventMatcher {
            affected_object: Some(obj),
            counter_kind: Some("shield".to_owned()),
            ..EventMatcher::new(BusEventKind::RemoveCounters)
        },
        Action::Skip,
    );
    let bus = Bus::new(
        w,
        vec![ReplacementEffect::shield_counter_destroy(eid(1), obj), skip],
    );
    let r = run_event(
        bus,
        zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
        &[],
    );
    assert_eq!(r.applications(), eids(&[1, 2]));
    assert_eq!(r.world().zone_of(obj), Some(ZoneKind::Battlefield));
    assert_eq!(r.world().counter(obj, "shield"), 1);
    assert!(r.new_log().is_empty());
}

// ---------------------------------------------------------------------------------------------
// review round 1: events that would not happen (F4, F11)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 614.7a, 120.8, 614.7 (F4): zero damage is no event: in a batch
// [0 damage, 3 damage] a one-use prevention shield is not consumed by the zero event and
// prevents the 3; the zero event is not logged.
#[test]
fn req_event_001_r1_f4_zero_damage_does_not_consume_shield() {
    let bus = Bus::new(World::new(), vec![prevent(1).with_shield(Shield::Uses(1))]);
    let r = run_batch(bus, vec![dmg_player(P2, 0), dmg_player(P2, 3)], false, &[]);
    assert_eq!(r.applications(), eids(&[1]));
    assert_eq!(r.report().prevented, 3);
    assert_eq!(r.world().life(P2), STARTING_LIFE);
    assert!(r.after.registry.is_empty());
    assert!(r.executed().is_empty());
    assert!(r.new_log().is_empty());
}

// REQ-EVENT-001, CR 614.7a, 120.8, 614.7 (F4): a single zero-damage event: no application, the
// shield stays, nothing logged; with two applicable effects nothing is asked.
#[test]
fn req_event_001_r1_f4_single_zero_damage_offers_nothing() {
    let bus = Bus::new(World::new(), vec![prevent(1).with_shield(Shield::Uses(1))]);
    let initial = bytes(&bus.registry);
    let r = run_event(bus, dmg_player(P2, 0), &[]);
    assert!(r.applications().is_empty());
    assert_eq!(r.report().prevented, 0);
    assert_eq!(bytes(&r.after.registry), initial);
    assert!(r.new_log().is_empty());
    assert_eq!(r.world().life(P2), STARTING_LIFE);

    let bus = Bus::new(World::new(), vec![prevent(1), prevent(2)]);
    let r = run_event(bus, dmg_player(P2, 0), &[]);
    assert!(r.requests.is_empty());
    assert!(r.applications().is_empty());
    assert!(r.new_log().is_empty());
}

// REQ-EVENT-001, CR 122.1d, 701.26b (F11): untapping an UNTAPPED permanent is no event: the
// stun effect does not apply, the counter stays, nothing changes.
#[test]
fn req_event_001_r1_f11_stun_ignores_untapped_permanent() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(obj, "stun", 1);
    assert!(!w.tapped(obj));
    let bus = Bus::new(w, vec![ReplacementEffect::stun_counter(eid(1), obj)]);
    let r = run_event(bus, GameEvent::Untap { object: obj }, &[]);
    assert!(r.applications().is_empty());
    assert_eq!(r.world().counter(obj, "stun"), 1);
    assert!(!r.world().tapped(obj));
    assert!(r.new_log().is_empty());
    assert_unchanged(&r);
}

// REQ-EVENT-001, CR 122.1d (F11): a TAPPED permanent with one stun counter stays tapped and
// loses the counter; one application.
#[test]
fn req_event_001_r1_f11_stun_replaces_untap_of_tapped_permanent() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.ok(GameEvent::Tap { object: obj });
    w.put_counters(obj, "stun", 1);
    let bus = Bus::new(w, vec![ReplacementEffect::stun_counter(eid(1), obj)]);
    let r = run_event(bus, GameEvent::Untap { object: obj }, &[]);
    assert_eq!(r.applications(), eids(&[1]));
    assert!(r.world().tapped(obj));
    assert_eq!(r.world().counter(obj, "stun"), 0);
}

/// Damage `amount` from source object `source` to object `target` (no source controller).
fn dmg_obj_from(
    source: fineprint_base::ids::ObjectId,
    target: fineprint_base::ids::ObjectId,
    amount: i64,
) -> GameEvent {
    GameEvent::Damage {
        source: Some(source),
        source_controller: None,
        target: fineprint_core::events::DamageTarget::Object(target),
        amount,
        combat: false,
    }
}

/// The executed RemoveCounters events of a run.
fn removals(r: &Run) -> Vec<GameEvent> {
    r.executed()
        .iter()
        .filter(|e| matches!(e.instance.event(), GameEvent::RemoveCounters { .. }))
        .map(|e| e.instance.event().clone())
        .collect()
}

/// A permanent with `counters` shield counters and two source objects.
fn shielded(
    counters: i64,
) -> (
    World,
    fineprint_base::ids::ObjectId,
    [fineprint_base::ids::ObjectId; 2],
) {
    let mut w = World::new();
    let x = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(x, "shield", counters);
    let s1 = w.create(ZoneKind::Battlefield, P2);
    let s2 = w.create(ZoneKind::Battlefield, P2);
    (w, x, [s1, s2])
}

// REQ-EVENT-001, CR 122.1c, 615.13 (G3): simultaneous damage from two sources to a permanent with
// two shield counters: both damage events are prevented (2), no damage is marked, and exactly ONE
// shield counter is removed (one follow-up per effect and recipient per batch); `applications`
// lists the effect once per prevented event.
#[test]
fn req_event_001_r2_g3_simultaneous_damage_removes_one_shield_counter() {
    let (w, x, [s1, s2]) = shielded(2);
    let r = run_batch(
        Bus::new(w, vec![ReplacementEffect::shield_counter_damage(eid(1), x)]),
        vec![dmg_obj_from(s1, x, 1), dmg_obj_from(s2, x, 1)],
        false,
        &[],
    );
    assert_eq!(r.report().prevented, 2);
    assert_eq!(r.world().events.markers().damage(x), 0);
    assert_eq!(removals(&r), vec![remove_counters(x, "shield", 1)]);
    assert_eq!(r.world().counter(x, "shield"), 1);
    assert_eq!(r.applications(), eids(&[1, 1]));
    assert!(
        r.executed()
            .iter()
            .all(|e| !matches!(e.instance.event(), GameEvent::Damage { .. }))
    );
}

// REQ-EVENT-001, CR 122.1c, 615.13 (G3): the same two damage events in two separate calls each
// remove a counter: 2 -> 1 -> 0.
#[test]
fn req_event_001_r2_g3_separate_calls_remove_one_each() {
    let (w, x, [s1, s2]) = shielded(2);
    let first = run_event(
        Bus::new(w, vec![ReplacementEffect::shield_counter_damage(eid(1), x)]),
        dmg_obj_from(s1, x, 1),
        &[],
    );
    assert_eq!(first.report().prevented, 1);
    assert_eq!(removals(&first), vec![remove_counters(x, "shield", 1)]);
    assert_eq!(first.world().counter(x, "shield"), 1);
    let second = run_event(next(&first), dmg_obj_from(s2, x, 1), &[]);
    assert_eq!(second.report().prevented, 1);
    assert_eq!(removals(&second), vec![remove_counters(x, "shield", 1)]);
    assert_eq!(second.world().counter(x, "shield"), 0);
    assert_eq!(second.world().events.markers().damage(x), 0);
}

// REQ-EVENT-001, CR 122.1c, 615.13 (G3): one damage event removes one counter (1 of 2 left).
#[test]
fn req_event_001_r2_g3_single_event_removes_one() {
    let (w, x, [s1, _]) = shielded(2);
    let r = run_event(
        Bus::new(w, vec![ReplacementEffect::shield_counter_damage(eid(1), x)]),
        dmg_obj_from(s1, x, 3),
        &[],
    );
    assert_eq!(r.report().prevented, 3);
    assert_eq!(removals(&r), vec![remove_counters(x, "shield", 1)]);
    assert_eq!(r.world().counter(x, "shield"), 1);
    assert_eq!(r.applications(), eids(&[1]));
}

// REQ-EVENT-001, CR 122.1c, 615.13 (G3): one shield counter and two simultaneous damage events:
// both prevented, one removal, no counter left, no damage marked.
#[test]
fn req_event_001_r2_g3_one_counter_two_simultaneous_damages() {
    let (w, x, [s1, s2]) = shielded(1);
    let r = run_batch(
        Bus::new(w, vec![ReplacementEffect::shield_counter_damage(eid(1), x)]),
        vec![dmg_obj_from(s1, x, 2), dmg_obj_from(s2, x, 1)],
        false,
        &[],
    );
    assert_eq!(r.report().prevented, 3);
    assert_eq!(removals(&r), vec![remove_counters(x, "shield", 1)]);
    assert_eq!(r.world().counter(x, "shield"), 0);
    assert_eq!(r.world().events.markers().damage(x), 0);
    assert_eq!(r.applications(), eids(&[1, 1]));
}

// REQ-EVENT-001, CR 122.1c, 615.13 (G3): two different permanents, each with one shield counter
// and its own damage shield, damaged simultaneously: one removal each.
#[test]
fn req_event_001_r2_g3_different_permanents_one_removal_each() {
    let (mut w, x, [s1, s2]) = shielded(1);
    let y = w.create(ZoneKind::Battlefield, P2);
    w.put_counters(y, "shield", 1);
    let r = run_batch(
        Bus::new(
            w,
            vec![
                ReplacementEffect::shield_counter_damage(eid(1), x),
                ReplacementEffect::shield_counter_damage(eid(2), y),
            ],
        ),
        vec![dmg_obj_from(s1, x, 1), dmg_obj_from(s2, y, 1)],
        false,
        &[],
    );
    assert_eq!(r.report().prevented, 2);
    let mut got = removals(&r);
    got.sort_by_key(|e| match e {
        GameEvent::RemoveCounters { target, .. } => format!("{target:?}"),
        _ => String::new(),
    });
    let mut expected = vec![
        remove_counters(x, "shield", 1),
        remove_counters(y, "shield", 1),
    ];
    expected.sort_by_key(|e| match e {
        GameEvent::RemoveCounters { target, .. } => format!("{target:?}"),
        _ => String::new(),
    });
    assert_eq!(got, expected);
    assert_eq!(r.world().counter(x, "shield"), 0);
    assert_eq!(r.world().counter(y, "shield"), 0);
}

// REQ-EVENT-001, CR 615.13 (G3): a generic shield-less `Prevent { then: [LifeChange P1 +1] }` on
// damage to player 2 from two simultaneous sources: both prevented, the follow-up executes once.
#[test]
fn req_event_001_r2_g3_generic_follow_up_once_per_batch() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P1);
    let effect = fx(
        1,
        m_dmg_player(P2),
        Action::Prevent {
            then: vec![life_change(P1, 1)],
        },
    );
    let r = run_batch(
        Bus::new(w, vec![effect]),
        vec![dmg_from(a, P2, 2), dmg_from(b, P2, 3)],
        false,
        &[],
    );
    assert_eq!(r.report().prevented, 5);
    assert_eq!(r.world().life(P2), STARTING_LIFE);
    assert_eq!(r.world().life(P1), STARTING_LIFE + 1);
    let gains = r
        .executed()
        .iter()
        .filter(|e| e.instance.event() == &life_change(P1, 1))
        .count();
    assert_eq!(gains, 1);
    assert_eq!(r.applications(), eids(&[1, 1]));
}
