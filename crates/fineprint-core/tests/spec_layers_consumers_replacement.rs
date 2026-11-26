// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests: the replacement bus keeps evaluated last known information (#184 FP-145;
//! REQ-LAYER-001, REQ-EVENT-001, REQ-CORE-003; CR 614.12, 616.1, 608.2h). A zone change of an
//! object with an effect-granted type and P/T change is redirected by a replacement effect;
//! the executed event is the redirected one and the LKI of the original object holds the
//! evaluated characteristics with the snapshot `LayeredProvider`, the base ones with the
//! base-only control. Black-box: written from the work-item brief only.

mod spec_layers_consumers_support;

use fineprint_base::ids::{EffectId, ObjectId};
use fineprint_core::events::GameEvent;
use fineprint_core::layers::{LayeredProvider, evaluate};
use fineprint_core::memory::CharacteristicsProvider;
use fineprint_core::replacement::{
    Action, BusEventKind, EventMatcher, Modification, ReplacementEffect, ReplacementRegistry,
};
use fineprint_core::state::GameEnvironment;
use fineprint_core::zones::{ZoneId, ZoneKind};
use spec_layers_consumers_support::*;

fn eid(n: u64) -> EffectId {
    EffectId::new(n)
}

/// A ZoneChange matcher for `object`.
fn m_zc(object: ObjectId) -> EventMatcher {
    EventMatcher {
        affected_object: Some(object),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    }
}

/// Replacement effect `n`: a zone change of `object` goes to `to` instead.
fn redirect(n: u64, object: ObjectId, to: ZoneKind) -> ReplacementEffect {
    ReplacementEffect::new(
        eid(n),
        m_zc(object),
        Action::Modify(Modification::SetDestination(to)),
    )
    .with_description(format!("test.redirect_{n}"))
}

fn registry(effects: Vec<ReplacementEffect>) -> ReplacementRegistry {
    let mut r = ReplacementRegistry::new();
    for e in effects {
        r.add(e).expect("effect accepted");
    }
    r
}

/// Runs `process_event(ZoneChange a -> graveyard)` on the fixture with the given replacement
/// effects and answers, through the evaluated provider (`evaluated`) or the base-only control.
fn run_fixture(
    effects: Vec<ReplacementEffect>,
    answers: &[u32],
    evaluated: bool,
) -> (BusRun, Fixture) {
    let f = fixture();
    let snap = f.table.game.clone();
    let env = GameEnvironment::new(&snap, &f.objects);
    let layered = LayeredProvider::new(snap.effects(), &env);
    let base = BaseProvider(&f.objects);
    let provider: &dyn CharacteristicsProvider = if evaluated { &layered } else { &base };
    let bus = Bus {
        table: f.table.clone(),
        registry: registry(effects),
    };
    let r = run_bus(
        bus,
        provider,
        zone_change(f.a, ZoneKind::Graveyard),
        answers,
    );
    (r, f)
}

// REQ-LAYER-001, REQ-EVENT-001, REQ-CORE-003; CR 614.12 (replacement of a zone change), 608.2h,
// 613.1d, 613.4c: a redirect-to-exile replacement applies; the executed event is the
// redirected zone change and the LKI of the original object holds the granted type and the
// modified P/T. Control: the base-only provider gives the base 2/2 creature.
#[test]
fn redirected_zone_change_keeps_evaluated_lki() {
    let (r, f) = run_fixture(vec![redirect(1, fixture().a, ZoneKind::Exile)], &[], true);
    assert_eq!(r.report().applications, vec![eid(1)]);
    let executed = r.executed();
    assert_eq!(executed.len(), 1);
    match executed[0].instance.event() {
        GameEvent::ZoneChange {
            object, request, ..
        } => {
            assert_eq!(*object, f.a);
            assert_eq!(request.to, ZoneKind::Exile, "the redirected event executed");
        }
        other => panic!("expected a ZoneChange, got {other:?}"),
    }
    assert!(executed[0].lki_captured);
    let t = &r.after.table;
    assert_eq!(
        t.zone_of(new_id(&executed[0].outcome)),
        Some(ZoneKind::Exile)
    );
    let rec = t.lki_of(f.a);
    assert_eq!(rec.zone(), ZoneId::Battlefield);
    let snap = f.table.game.clone();
    let env = GameEnvironment::new(&snap, &f.objects);
    let expected = evaluate(snap.effects(), &env, f.a).unwrap().characteristics;
    assert_eq!(*rec.characteristics(), expected);
    assert!(has_type(rec.characteristics(), GRANTED));
    assert_eq!(pt(rec.characteristics()), spt(3, 4));

    // Control.
    let (c, cf) = run_fixture(vec![redirect(1, fixture().a, ZoneKind::Exile)], &[], false);
    let crec = c.after.table.lki_of(cf.a);
    assert!(!has_type(crec.characteristics(), GRANTED), "control");
    assert_eq!(pt(crec.characteristics()), spt(2, 2), "control");
    assert_ne!(*crec.characteristics(), expected, "control discriminates");
}

// REQ-LAYER-001, REQ-EVENT-001, REQ-CORE-003; CR 616.1 (the affected object's controller
// chooses the order of applicable replacement effects), 614.12, 608.2h: with two redirects the
// choice decides the final destination, in either order, and the LKI of the original object
// holds the evaluated characteristics either way. Control: the base-only provider gives the
// base values in both orders.
#[test]
fn chosen_order_of_two_redirects_keeps_evaluated_lki() {
    let a = fixture().a;
    let effects = || {
        vec![
            redirect(1, a, ZoneKind::Exile),
            redirect(2, a, ZoneKind::Hand),
        ]
    };
    let mut destinations = Vec::new();
    for first in [0u32, 1u32] {
        for evaluated in [true, false] {
            let (r, f) = run_fixture(effects(), &[first], evaluated);
            assert_eq!(r.requests, 1);
            let apps = &r.report().applications;
            assert_eq!(apps.len(), 2, "each redirect applies once (CR 614.5)");
            let executed = r.executed();
            assert_eq!(executed.len(), 1);
            let dest = r.after.table.zone_of(new_id(&executed[0].outcome));
            let last = apps[1];
            let expected_dest = if last == eid(1) {
                ZoneKind::Exile
            } else {
                ZoneKind::Hand
            };
            assert_eq!(
                dest,
                Some(expected_dest),
                "the last-applied redirect decides"
            );
            if evaluated {
                destinations.push(expected_dest);
            }
            let rec = r.after.table.lki_of(f.a);
            if evaluated {
                assert!(has_type(rec.characteristics(), GRANTED), "first={first}");
                assert_eq!(pt(rec.characteristics()), spt(3, 4), "first={first}");
            } else {
                assert!(!has_type(rec.characteristics(), GRANTED), "control");
                assert_eq!(pt(rec.characteristics()), spt(2, 2), "control");
            }
        }
    }
    assert_eq!(destinations.len(), 2);
    assert_ne!(
        destinations[0], destinations[1],
        "the two choices give the two destinations"
    );
}

/// Effect 1: while `object` has a `kind` counter (by the event layer's markers), its zone
/// change goes to exile instead.
fn counter_backed(object: ObjectId, kind: &str) -> ReplacementEffect {
    ReplacementEffect::new(
        eid(1),
        EventMatcher {
            requires_counter: Some((object, kind.to_owned())),
            ..m_zc(object)
        },
        Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
    )
    .with_description("test.counter_backed_redirect")
}

/// Runs the counter-backed redirect on the fixture (granted type, P/T change) with the
/// evaluated provider; `marker_counters` are put on through `AddCounters` events, and
/// `world_counters` only into the layer system's environment.
fn run_counter_backed(
    marker_counters: &[(&str, i64)],
    world_counters: &[(&str, i64)],
) -> (BusRun, ObjectId) {
    let mut f = fixture();
    {
        let setup_objects = f.objects.clone();
        let setup = BaseProvider(&setup_objects);
        for (k, n) in marker_counters {
            f.table.exec(&setup, add_counters(f.a, k, *n));
        }
    }
    for (k, n) in world_counters {
        f.objects.counter(f.a, k, *n);
    }
    let snap = f.table.game.clone();
    let env = GameEnvironment::new(&snap, &f.objects);
    let layered = LayeredProvider::new(snap.effects(), &env);
    // The granted type is really there in the evaluated view.
    let now = layered.characteristics(f.a).expect("known");
    assert!(has_type(&now, GRANTED));
    let bus = Bus {
        table: f.table.clone(),
        registry: registry(vec![counter_backed(f.a, "test.charge")]),
    };
    let r = run_bus(bus, &layered, zone_change(f.a, ZoneKind::Graveyard), &[]);
    (r, f.a)
}

// REQ-EVENT-001, REQ-LAYER-001; CR 614.12, 613.1d: a replacement matcher with
// `requires_counter` keys off the event layer's counter markers and is unaffected by
// evaluation: a granted type creates no counter (the effect does not apply), a counter put on
// through `AddCounters` makes it apply, and a counter that exists only in the layer system's
// environment (not in the markers) does not.
#[test]
fn requires_counter_keys_off_markers_not_evaluation() {
    // No counter anywhere: the granted type does not create one.
    let (r, _) = run_counter_backed(&[], &[]);
    assert!(r.report().applications.is_empty());
    let ex = r.executed();
    assert_eq!(
        r.after.table.zone_of(new_id(&ex[0].outcome)),
        Some(ZoneKind::Graveyard)
    );

    // A marker counter: the effect applies (redirect to exile).
    let (r, a) = run_counter_backed(&[("test.charge", 1)], &[]);
    assert_eq!(r.report().applications, vec![eid(1)]);
    let ex = r.executed();
    assert_eq!(
        r.after.table.zone_of(new_id(&ex[0].outcome)),
        Some(ZoneKind::Exile)
    );
    // The LKI is still the evaluated one, and carries the marker counter.
    let rec = r.after.table.lki_of(a);
    assert!(has_type(rec.characteristics(), GRANTED));
    assert_eq!(rec.counters().get("test.charge"), Some(&1));

    // A counter only in the layer environment: not a marker, so the effect does not apply.
    let (r, _) = run_counter_backed(&[], &[("test.charge", 1)]);
    assert!(r.report().applications.is_empty());
    let ex = r.executed();
    assert_eq!(
        r.after.table.zone_of(new_id(&ex[0].outcome)),
        Some(ZoneKind::Graveyard)
    );
}
