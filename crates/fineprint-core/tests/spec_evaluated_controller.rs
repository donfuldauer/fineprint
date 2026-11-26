// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests: the replacement-effect chooser and departure last known information use the
//! controller after layer 2 (#216 FP-160; REQ-LAYER-001, REQ-REPL-* (the replacement chooser,
//! part of the REQ-EVENT-001 bus), REQ-CORE-003, REQ-EVENT-001; CR 613.1b, 616.1, 108.4a,
//! 110.2, 603.10a, 603.3a, 701.21a, 704.5s, 400.3, 613.7).
//!
//! Black-box: written from the work-item brief and the public API only. The chooser is
//! observed as the `chooser` of the `CHOOSE_REPLACEMENT_EFFECT` request a `ChoiceSession`
//! running `replacement::process_event` asks; LKI is observed through `capture_lki_on_move`
//! called directly (the event/SBA paths are out of scope for this unit).

#[path = "spec_evaluated_controller_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::events::{GameEvent, Outcome};
use fineprint_core::layers::{LayerModification, LayeredProvider};
use fineprint_core::memory::CharacteristicsProvider;
use fineprint_core::state::GameEnvironment;
use fineprint_core::zones::{MoveRequest, ZoneId, ZoneKind};
use support::*;

const R1: &str = "test.redirect_1";
const R2: &str = "test.redirect_2";

// ---------------------------------------------------------------------------------------------
// chooser helpers
// ---------------------------------------------------------------------------------------------

/// A table of `players` players with one battlefield creature `a` owned by `owner`, stored
/// controller `stored` (also the layer system's base controller).
fn creature_world(players: usize, owner: PlayerId, stored: PlayerId) -> (Table, Objects, ObjectId) {
    let mut t = Table::new(players);
    let a = t.permanent(owner, stored);
    let mut objs = Objects::new();
    objs.put(a, creature("test.creature_2_2", 2, 2), stored);
    (t, objs, a)
}

/// Two competing "instead" redirects on `a`'s zone change: effect 1 to exile, effect 2 to its
/// owner's hand (CR 616.1: same tier, so the affected object's controller chooses).
fn two_redirects(t: &Table, a: ObjectId) -> Bus {
    Bus {
        table: t.clone(),
        registry: registry(vec![
            redirect(1, a, ZoneKind::Exile),
            redirect(2, a, ZoneKind::Hand),
        ]),
    }
}

/// Runs the zone change `a -> graveyard` (two redirects compete) with `provider`, picking
/// `pick` first; asserts exactly one choice request was asked and returns the run.
fn choose(t: &Table, a: ObjectId, provider: &dyn CharacteristicsProvider, pick: &str) -> BusRun {
    let r = run_bus(
        two_redirects(t, a),
        provider,
        zone_change(a, ZoneKind::Graveyard),
        &[pick],
    );
    assert_eq!(
        r.requests.len(),
        1,
        "one CR 616.1 choice for two competing effects"
    );
    let names = offered(&r.requests[0]);
    assert!(names.contains(&R1.to_string()) && names.contains(&R2.to_string()));
    r
}

/// The single chooser of a run through the LayeredProvider over `t`/`objs`.
fn layered_chooser(t: &Table, objs: &Objects, a: ObjectId) -> PlayerId {
    let r = with_layered(t, objs, |p| choose(t, a, p, R1));
    r.choosers()[0]
}

fn moved_to(r: &BusRun) -> ZoneId {
    let ex = r.executed();
    assert_eq!(ex.len(), 1);
    match &ex[0].outcome {
        Outcome::ZoneChange(m) => m.to,
        other => panic!("expected a ZoneChange outcome, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// scenario 1-3: chooser follows layer 2
// ---------------------------------------------------------------------------------------------

// Scenario 1. REQ-REPL-*, REQ-EVENT-001, REQ-LAYER-001; CR 616.1 (the affected object's
// controller chooses), CR 613.1b (layer 2: control-changing effects), CR 110.2, CR 400.3: a
// creature owned and stored-controlled by P1 under a layer-2 `SetController(P2)` sends its
// replacement choice to P2; answering applies the chosen effect first (both picks), and the
// moved card still goes to a zone of its OWNER (P1).
#[test]
fn chooser_is_layer2_controller() {
    let (mut t, objs, a) = creature_world(2, P1, P1);
    give_control(&mut t.game, a, P2);
    with_layered(&t, &objs, |p| {
        assert_eq!(
            p.controller(a),
            Some(P2),
            "LayeredProvider names the layer-2 controller"
        );
    });
    for (pick, first, other) in [(R1, eid(1), eid(2)), (R2, eid(2), eid(1))] {
        let r = with_layered(&t, &objs, |p| choose(&t, a, p, pick));
        assert_eq!(r.choosers(), vec![P2], "pick={pick}");
        let apps = &r.report().applications;
        assert_eq!(
            apps.first(),
            Some(&first),
            "the chosen effect applies first"
        );
        assert!(apps.contains(&other) || apps.len() == 1);
        let to = moved_to(&r);
        if let Some(owner) = to.owner() {
            assert_eq!(owner, P1, "CR 400.3: owner's zone, not the controller's");
        }
    }
}

// Scenario 2. REQ-REPL-*, REQ-EVENT-001, REQ-LAYER-001; CR 616.1, CR 110.2: the control run
// without the layer-2 effect sends the choice to the stored controller P1.
#[test]
fn chooser_without_layer2_is_stored_controller() {
    let (t, objs, a) = creature_world(2, P1, P1);
    assert_eq!(layered_chooser(&t, &objs, a), P1);
    // And with the layer-2 effect added the same world asks P2 (discrimination).
    let mut t2 = t.clone();
    give_control(&mut t2.game, a, P2);
    assert_eq!(layered_chooser(&t2, &objs, a), P2);
}

// Scenario 3. REQ-REPL-*, REQ-EVENT-001, REQ-LAYER-001; CR 616.1, CR 613.1b: stored
// controller P2 (owner P1) with a layer-2 `SetController(P1)`: the choice goes to P1; without
// the effect it goes to P2.
#[test]
fn chooser_reversed_layer2_change() {
    let (mut t, objs, a) = creature_world(2, P1, P2);
    assert_eq!(layered_chooser(&t, &objs, a), P2, "control: stored P2");
    give_control(&mut t.game, a, P1);
    assert_eq!(layered_chooser(&t, &objs, a), P1);
    // Same with owner P2.
    let (mut t, objs, a) = creature_world(2, P2, P2);
    give_control(&mut t.game, a, P1);
    assert_eq!(layered_chooser(&t, &objs, a), P1);
}

// ---------------------------------------------------------------------------------------------
// scenario 4: fallback to the stored controller
// ---------------------------------------------------------------------------------------------

// Scenario 4. REQ-REPL-*, REQ-EVENT-001, REQ-LAYER-001; CR 613.1b (no layer-2 effect), CR
// 616.1, CR 110.2: when the layers name no control change (only layer-4 and layer-7b effects)
// the choice goes to the stored controller; likewise with a fake provider naming none, one
// naming the stored controller, and a base-only provider without a `controller` override. The
// stored controller is P2 (not the active player P1) so the fallback is observable.
#[test]
fn chooser_falls_back_to_stored_controller() {
    let (mut t, objs, a) = creature_world(2, P1, P2);
    register_resolved(&mut t.game, Layer::L4, &[a], add_type("Artifact"));
    register_resolved(&mut t.game, Layer::L7b, &[a], set_pt(5, 5));
    with_layered(&t, &objs, |p| {
        let named = p.controller(a);
        assert!(
            named.is_none() || named == Some(P2),
            "no layer-2 effect: names the stored controller or none, got {named:?}"
        );
    });
    assert_eq!(layered_chooser(&t, &objs, a), P2, "layer 4/7 only");

    let none = FixedController {
        objects: &objs,
        controllers: BTreeMap::new(),
    };
    assert_eq!(
        choose(&t, a, &none, R1).choosers(),
        vec![P2],
        "provider names none"
    );

    let same = FixedController {
        objects: &objs,
        controllers: BTreeMap::from([(a, P2)]),
    };
    assert_eq!(
        choose(&t, a, &same, R1).choosers(),
        vec![P2],
        "provider names stored"
    );

    let base = BaseOnly(&objs);
    assert_eq!(
        choose(&t, a, &base, R1).choosers(),
        vec![P2],
        "base-only provider"
    );

    // Discrimination: a fake provider naming P1 moves the choice to P1.
    let other = FixedController {
        objects: &objs,
        controllers: BTreeMap::from([(a, P1)]),
    };
    assert_eq!(
        choose(&t, a, &other, R1).choosers(),
        vec![P1],
        "provider names P1"
    );
}

// REQ-LAYER-001; CR 613.1b: the LayeredProvider names no controller for an unknown object.
#[test]
fn layered_provider_unknown_object_names_none() {
    let (mut t, objs, a) = creature_world(2, P1, P1);
    give_control(&mut t.game, a, P2);
    let unknown = ObjectId::new(9_999);
    let snap = t.game.clone();
    let env = GameEnvironment::new(&snap, &objs);
    let p = LayeredProvider::new(snap.effects(), &env);
    assert_eq!(p.controller(unknown), None);
    assert_eq!(p.controller(a), Some(P2));
}

// ---------------------------------------------------------------------------------------------
// scenario 5: two layer-2 effects, both timestamp orders
// ---------------------------------------------------------------------------------------------

/// Three players, creature stored-controlled by P1; two layer-2 effects with timestamps
/// `stamps[i]` (index 0 earlier), registered in the order given as `(timestamp index, to)`.
fn two_control_effects(
    first: (usize, PlayerId),
    second: (usize, PlayerId),
) -> (Table, Objects, ObjectId) {
    let (mut t, objs, a) = creature_world(3, P1, P1);
    let stamps = [
        t.game.stamp_resolved_effect().expect("timestamp"),
        t.game.stamp_resolved_effect().expect("timestamp"),
    ];
    assert!(stamps[0] < stamps[1]);
    for (i, to) in [first, second] {
        register_at(
            &mut t.game,
            stamps[i],
            Layer::L2,
            &[a],
            LayerModification::SetController(to),
        );
    }
    (t, objs, a)
}

/// `(timestamp index, new controller)` of one layer-2 effect (index 0 = earlier timestamp).
type Stamped = (usize, PlayerId);

const ORDERS: [(Stamped, Stamped, PlayerId); 4] = [
    // P2 effect earlier & registered first, P3 later: P3 wins.
    ((0, P2), (1, P3), P3),
    // P3 (later timestamp) registered first: P3 still wins.
    ((1, P3), (0, P2), P3),
    // Swapped values: the later timestamp gives P2.
    ((0, P3), (1, P2), P2),
    ((1, P2), (0, P3), P2),
];

// Scenario 5. REQ-REPL-*, REQ-EVENT-001, REQ-LAYER-001; CR 613.7 (timestamp order within a
// layer), CR 613.1b, CR 616.1: with two layer-2 control effects the later timestamp's
// controller receives the choice, in both registration orders (so neither registration order
// nor effect id decides), in a three-player game.
#[test]
fn chooser_two_layer2_effects_later_timestamp_wins_both_orders() {
    for (first, second, winner) in ORDERS {
        let (t, objs, a) = two_control_effects(first, second);
        assert_eq!(
            layered_chooser(&t, &objs, a),
            winner,
            "first={first:?} second={second:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// scenario 6: non-permanent chooses by owner
// ---------------------------------------------------------------------------------------------

// Scenario 6. REQ-REPL-*, REQ-EVENT-001; CR 108.4a (no controller: use the owner), CR 616.1:
// a card in a graveyard affected by a zone change with two competing redirects asks its OWNER,
// even when the provider reports another controller for that id. Both owners are tested so the
// active player (P1) cannot be the reason.
#[test]
fn chooser_for_graveyard_card_is_owner() {
    for (owner, reported) in [(P1, P2), (P2, P1)] {
        let mut t = Table::new(2);
        let g = t.card_in(ZoneKind::Graveyard, owner);
        let mut objs = Objects::new();
        objs.put_off(g, creature("test.creature_1_1", 1, 1));
        let fake = FixedController {
            objects: &objs,
            controllers: BTreeMap::from([(g, reported)]),
        };
        let bus = Bus {
            table: t.clone(),
            registry: registry(vec![
                redirect(1, g, ZoneKind::Exile),
                redirect(2, g, ZoneKind::Hand),
            ]),
        };
        let r = run_bus(bus, &fake, zone_change(g, ZoneKind::Library), &[R2]);
        assert_eq!(r.choosers(), vec![owner], "owner={owner:?}");
        assert_eq!(r.report().applications.first(), Some(&eid(2)));

        // Also through the LayeredProvider with a layer-2 effect naming `reported` on that id.
        register_resolved(
            &mut t.game,
            Layer::L2,
            &[g],
            LayerModification::SetController(reported),
        );
        let bus = Bus {
            table: t.clone(),
            registry: registry(vec![
                redirect(1, g, ZoneKind::Exile),
                redirect(2, g, ZoneKind::Hand),
            ]),
        };
        let r = with_layered(&t, &objs, |p| {
            run_bus(bus, p, zone_change(g, ZoneKind::Library), &[R1])
        });
        assert_eq!(r.choosers(), vec![owner], "layered, owner={owner:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// scenario 7-9, 11: LKI capture on leaving the battlefield
// ---------------------------------------------------------------------------------------------

/// A table with a "Saga"-subtyped permanent `s` (owner `owner`, stored controller `stored`)
/// carrying a lore counter in the layer system's world.
fn saga_world(players: usize, owner: PlayerId, stored: PlayerId) -> (Table, Objects, ObjectId) {
    let mut t = Table::new(players);
    let s = t.permanent(owner, stored);
    let mut objs = Objects::new();
    objs.put(s, saga_like(), stored);
    objs.counter(s, "lore", 3);
    (t, objs, s)
}

fn to_graveyard() -> MoveRequest {
    MoveRequest::new(ZoneKind::Graveyard)
}

/// Captures LKI for `id` -> graveyard through the LayeredProvider; returns the recorded
/// controller.
fn layered_capture(t: &mut Table, objs: &Objects, id: ObjectId) -> Option<PlayerId> {
    let snap = t.game.clone();
    let env = GameEnvironment::new(&snap, objs);
    let p = LayeredProvider::new(snap.effects(), &env);
    assert_eq!(t.capture(&p, id, &to_graveyard()), Ok(true));
    t.lki_of(id).controller()
}

// Scenario 7. REQ-CORE-003, REQ-LAYER-001; CR 603.10a (leaves-the-battlefield and sacrifice
// triggers look back in time), CR 603.3a (a trigger is controlled by its source's controller
// when it triggered), CR 701.21a and CR 704.5s (the Saga's controller sacrifices it; it goes to
// its OWNER's graveyard), CR 613.1b, CR 400.3: a Saga-like permanent owned and stored-controlled
// by P1 under a layer-2 `SetController(P2)` records `controller() == Some(P2)`, owner P1, zone
// battlefield; everything else matches the capture without the effect; the moved card lands in
// P1's graveyard.
#[test]
fn lki_records_layer2_controller_destination_owner_based() {
    let (mut t, objs, s) = saga_world(2, P1, P1);
    let mut control = t.clone();
    give_control(&mut t.game, s, P2);

    assert_eq!(layered_capture(&mut t, &objs, s), Some(P2));
    let rec = t.lki_of(s).clone();
    assert_eq!(rec.object(), s);
    assert_eq!(rec.owner(), P1);
    assert_eq!(rec.zone(), ZoneId::Battlefield);
    assert!(rec.characteristics().subtypes.contains("Saga"));

    // Control capture without the layer-2 effect: the stored controller, and every other field
    // is the same (only the controller is evaluated differently).
    assert_eq!(layered_capture(&mut control, &objs, s), Some(P1));
    let crec = control.lki_of(s).clone();
    assert_eq!(crec.owner(), rec.owner());
    assert_eq!(crec.zone(), rec.zone());
    assert_eq!(crec.characteristics(), rec.characteristics());
    assert_eq!(crec.counters(), rec.counters());
    assert_eq!(crec.attached_here(), rec.attached_here());

    // The zone move: owner-based destination (CR 400.3, CR 701.21a).
    let moved = t
        .zones
        .move_object(&mut t.ids, s, to_graveyard())
        .expect("move succeeds");
    assert_eq!(moved.from, ZoneId::Battlefield);
    assert_eq!(moved.to, ZoneId::Graveyard(P1), "owner's graveyard");
    assert_ne!(
        moved.to,
        ZoneId::Graveyard(P2),
        "not the controller's graveyard"
    );
    assert_eq!(t.zone_of(moved.new), Some(ZoneKind::Graveyard));
    assert_eq!(t.zones.object(moved.new).expect("present").owner, P1);
    // The record is unchanged by the move.
    assert_eq!(*t.lki_of(s), rec);
}

// Scenario 7 (bus side). REQ-REPL-*, REQ-EVENT-001, REQ-LAYER-001; CR 616.1, CR 613.1b, CR
// 400.3: a Saga-like permanent owned by P1 and controlled after layer 2 by P2, sent to the
// graveyard with one redirect-to-hand effect: the redirected card goes to its OWNER's hand.
#[test]
fn bus_destination_stays_owner_based_under_layer2_control() {
    let (mut t, objs, s) = saga_world(2, P1, P1);
    give_control(&mut t.game, s, P2);
    let bus = Bus {
        table: t.clone(),
        registry: registry(vec![redirect(2, s, ZoneKind::Hand)]),
    };
    let r = with_layered(&t, &objs, |p| {
        run_bus(bus, p, zone_change(s, ZoneKind::Graveyard), &[])
    });
    assert!(r.requests.is_empty(), "one effect: no choice");
    let to = moved_to(&r);
    assert_eq!(to.owner(), Some(P1), "owner's hand (CR 400.3), got {to:?}");
    match &r.executed()[0].instance.event() {
        GameEvent::ZoneChange { request, .. } => assert_eq!(request.to, ZoneKind::Hand),
        other => panic!("expected a ZoneChange, got {other:?}"),
    }
}

// Scenario 8. REQ-CORE-003, REQ-LAYER-001; CR 603.10a, CR 110.2: without a layer-2 effect the
// recorded controller is the stored one (P1, and P2 when stored P2); a base-only provider (no
// `controller` override) records the stored controller even when a layer-2 effect exists.
#[test]
fn lki_without_layer2_or_base_only_records_stored_controller() {
    let (mut t, objs, s) = saga_world(2, P1, P1);
    assert_eq!(layered_capture(&mut t, &objs, s), Some(P1));

    let (mut t, objs, s) = saga_world(2, P1, P2);
    assert_eq!(layered_capture(&mut t, &objs, s), Some(P2));

    let (mut t, objs, s) = saga_world(2, P1, P2);
    give_control(&mut t.game, s, P1);
    let base = BaseOnly(&objs);
    assert_eq!(t.capture(&base, s, &to_graveyard()), Ok(true));
    assert_eq!(
        t.lki_of(s).controller(),
        Some(P2),
        "base-only: stored controller"
    );
    assert_eq!(t.lki_of(s).owner(), P1);
}

// Scenario 9. REQ-CORE-003, REQ-LAYER-001; CR 613.1b, CR 110.2: a layer-2 effect giving the
// permanent to its stored controller records that controller; an effect removed before the
// capture leaves the stored controller.
#[test]
fn lki_layer2_to_same_player_and_removed_effect() {
    for stored in [P1, P2] {
        let (mut t, objs, s) = saga_world(2, P1, stored);
        give_control(&mut t.game, s, stored);
        assert_eq!(
            layered_capture(&mut t, &objs, s),
            Some(stored),
            "same player"
        );

        let other = if stored == P1 { P2 } else { P1 };
        let (mut t, objs, s) = saga_world(2, P1, stored);
        let e = give_control(&mut t.game, s, other);
        assert!(t.game.remove_effect(e));
        assert_eq!(
            layered_capture(&mut t, &objs, s),
            Some(stored),
            "removed effect"
        );
    }
}

// Scenario 11. REQ-CORE-003, REQ-LAYER-001; CR 613.7, CR 613.1b, CR 603.10a: with two layer-2
// control effects the later timestamp's controller is recorded, in both registration orders,
// in a three-player game.
#[test]
fn lki_two_layer2_effects_later_timestamp_wins_both_orders() {
    for (first, second, winner) in ORDERS {
        let (mut t, objs, a) = two_control_effects(first, second);
        assert_eq!(
            layered_capture(&mut t, &objs, a),
            Some(winner),
            "first={first:?} second={second:?}"
        );
        assert_eq!(t.lki_of(a).owner(), P1);
    }
}

// ---------------------------------------------------------------------------------------------
// scenario 10: LKI outside the battlefield
// ---------------------------------------------------------------------------------------------

// Scenario 10. REQ-CORE-003; CR 108.4a, CR 109.4 (only battlefield and stack objects have a
// controller): a card leaving a hand or a graveyard records no controller even when the
// provider reports one (fake provider and LayeredProvider with a layer-2 effect on that id); a
// spell leaving the stack records its stored stack controller, not the provider's.
#[test]
fn lki_outside_battlefield_ignores_provider_controller() {
    for kind in [ZoneKind::Hand, ZoneKind::Graveyard] {
        let dest = if kind == ZoneKind::Hand {
            ZoneKind::Graveyard
        } else {
            ZoneKind::Exile
        };
        let mut t = Table::new(2);
        let c = t.card_in(kind, P1);
        let mut objs = Objects::new();
        objs.put_off(c, creature("test.creature_1_1", 1, 1));
        let fake = FixedController {
            objects: &objs,
            controllers: BTreeMap::from([(c, P2)]),
        };
        let mut t2 = t.clone();
        assert_eq!(t.capture(&fake, c, &MoveRequest::new(dest)), Ok(true));
        assert_eq!(t.lki_of(c).controller(), None, "{kind:?}, fake provider");
        assert_eq!(t.lki_of(c).owner(), P1);

        register_resolved(
            &mut t2.game,
            Layer::L2,
            &[c],
            LayerModification::SetController(P2),
        );
        let snap = t2.game.clone();
        let env = GameEnvironment::new(&snap, &objs);
        let p = LayeredProvider::new(snap.effects(), &env);
        assert_eq!(t2.capture(&p, c, &MoveRequest::new(dest)), Ok(true));
        assert_eq!(
            t2.lki_of(c).controller(),
            None,
            "{kind:?}, layered provider"
        );
    }

    let mut t = Table::new(3);
    let sp = t.spell(P1, P2);
    let mut objs = Objects::new();
    objs.put_off(sp, creature("test.creature_3_3", 3, 3));
    let fake = FixedController {
        objects: &objs,
        controllers: BTreeMap::from([(sp, P3)]),
    };
    assert_eq!(t.capture(&fake, sp, &to_graveyard()), Ok(true));
    let rec = t.lki_of(sp);
    assert_eq!(rec.zone(), ZoneId::Stack);
    assert_eq!(
        rec.controller(),
        Some(P2),
        "stack: stored controller, not the provider's"
    );
    assert_eq!(rec.owner(), P1);
}

// ---------------------------------------------------------------------------------------------
// scenario 12: compatibility of providers without `controller`
// ---------------------------------------------------------------------------------------------

// Scenario 12. REQ-CORE-003, REQ-REPL-*, REQ-EVENT-001; CR 616.1, CR 110.2: providers that
// implement only `characteristics` (and optionally `counters`/`attached_here`) still compile
// and behave as before: the chooser and the LKI controller are the stored controller (P2, not
// the active player), whatever layer-2 effect the game holds, and the recorded characteristics
// are the provider's.
#[test]
fn providers_without_controller_keep_old_behaviour() {
    let (mut t, objs, a) = creature_world(2, P1, P2);
    give_control(&mut t.game, a, P1);
    let base = BaseOnly(&objs);
    let full = BaseWithCounters(&objs);
    let providers: [&dyn CharacteristicsProvider; 2] = [&base, &full];
    for (i, p) in providers.into_iter().enumerate() {
        assert_eq!(p.controller(a), None, "trait default, provider {i}");
        assert_eq!(
            choose(&t, a, p, R2).choosers(),
            vec![P2],
            "chooser, provider {i}"
        );
        let mut tc = t.clone();
        assert_eq!(tc.capture(p, a, &to_graveyard()), Ok(true));
        let rec = tc.lki_of(a);
        assert_eq!(rec.controller(), Some(P2), "LKI, provider {i}");
        assert_eq!(rec.owner(), P1);
        assert_eq!(rec.characteristics(), &creature("test.creature_2_2", 2, 2));
    }
}
