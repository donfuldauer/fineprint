// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests: last known information holds EVALUATED characteristics (#184 FP-145;
//! REQ-LAYER-001, REQ-CORE-003; CR 608.2h, 613.1-613.4). Every test drives the public event
//! layer (`EventContext::execute` / `execute_batch`) with the snapshot `LayeredProvider` and
//! repeats the run with the base-only control provider, which must fail the same assertion.
//! Black-box: written from the work-item brief only.

mod spec_layers_consumers_support;

use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{LayeredProvider, evaluate};
use fineprint_core::memory::{
    CharacteristicsProvider, Information, InformationUser, LkiRecord, information_for,
};
use fineprint_core::permanent::AttachTarget;
use fineprint_core::state::GameEnvironment;
use fineprint_core::zones::{ZoneId, ZoneKind};
use spec_layers_consumers_support::*;

/// Moves `a` battlefield -> graveyard through `ctx.execute` with `provider` and returns the
/// captured LKI record.
fn move_and_capture(
    table: &mut Table,
    a: ObjectId,
    provider: &dyn CharacteristicsProvider,
) -> LkiRecord {
    let done = table.exec(provider, zone_change(a, ZoneKind::Graveyard));
    assert!(done.lki_captured, "a battlefield exit captures LKI");
    assert_eq!(
        table.zone_of(new_id(&done.outcome)),
        Some(ZoneKind::Graveyard)
    );
    table.lki_of(a).clone()
}

// REQ-LAYER-001, REQ-CORE-003; CR 608.2h, 613.1d (layer 4), 613.4c (layer 7c): a zone change
// executed with the evaluated provider stores the granted card type and the modified P/T in
// the LKI record; the base-only control stores the base 2/2 creature.
#[test]
fn lki_holds_granted_type_and_modified_pt() {
    // Evaluated provider.
    let mut f = fixture();
    let snap = f.table.game.clone();
    let env = GameEnvironment::new(&snap, &f.objects);
    let provider = LayeredProvider::new(snap.effects(), &env);
    let expected = evaluate(snap.effects(), &env, f.a)
        .expect("known object")
        .characteristics;
    assert_eq!(expected.card_types, set(&[GRANTED, "Creature"]));
    assert_eq!(pt(&expected), spt(3, 4));

    let rec = move_and_capture(&mut f.table, f.a, &provider);
    assert_eq!(rec.object(), f.a);
    assert_eq!(rec.zone(), ZoneId::Battlefield);
    assert_eq!(*rec.characteristics(), expected);
    assert!(has_type(rec.characteristics(), GRANTED));
    assert_eq!(pt(rec.characteristics()), spt(3, 4));
    assert_ne!(rec.characteristics(), f.objects.base_chars(f.a));

    // CR 608.2h: an effect that expected the object on the battlefield now uses the LKI,
    // which holds the evaluated values.
    match information_for(
        &f.table.zones,
        &f.table.lki,
        f.a,
        ZoneKind::Battlefield,
        InformationUser::SpellOrAbility,
    ) {
        Some(Information::Lki(r)) => {
            assert!(has_type(r.characteristics(), GRANTED));
            assert_eq!(pt(r.characteristics()), spt(3, 4));
        }
        other => panic!("expected LKI, got {other:?}"),
    }

    // Control: the same run with a base-only provider holds the base values.
    let mut g = fixture();
    let base = BaseProvider(&g.objects);
    let rec = move_and_capture(&mut g.table, g.a, &base);
    assert!(!has_type(rec.characteristics(), GRANTED), "control");
    assert_eq!(pt(rec.characteristics()), spt(2, 2), "control");
    assert_ne!(*rec.characteristics(), expected, "control discriminates");
}

/// One run of the two-set-P/T scenario: registers `first` then `second` (each `(timestamp
/// index, power, toughness)`, index 0 = the earlier timestamp) as layer-7b effects on one
/// creature, moves it to the graveyard with the evaluated (or base-only) provider and returns
/// the LKI P/T.
fn two_setters(
    first: (usize, i64, i64),
    second: (usize, i64, i64),
    evaluated: bool,
) -> (Option<i64>, Option<i64>) {
    let mut table = Table::new();
    let a = table.create(ZoneKind::Battlefield, P1);
    let mut objects = Objects::new();
    objects.put(a, creature("test.lki_setters_2_2", 2, 2), P1);
    let stamps = [
        table.game.stamp_resolved_effect().expect("timestamp"),
        table.game.stamp_resolved_effect().expect("timestamp"),
    ];
    assert!(stamps[0] < stamps[1]);
    for (i, p, t) in [first, second] {
        register_at(&mut table.game, stamps[i], Layer::L7b, &[a], set_pt(p, t));
    }
    let snap = table.game.clone();
    let env = GameEnvironment::new(&snap, &objects);
    let layered = LayeredProvider::new(snap.effects(), &env);
    let base = BaseProvider(&objects);
    let provider: &dyn CharacteristicsProvider = if evaluated { &layered } else { &base };
    // The provider answers what `evaluate` answers for the same snapshot.
    let direct = pt(&evaluate(snap.effects(), &env, a).unwrap().characteristics);
    table.exec(provider, zone_change(a, ZoneKind::Graveyard));
    let got = pt(table.lki_of(a).characteristics());
    if evaluated {
        assert_eq!(got, direct, "LKI equals evaluate() of the snapshot");
    }
    got
}

// REQ-LAYER-001, REQ-CORE-003; CR 613.4b, 613.7 (timestamp order within layer 7b), 608.2h: of
// two set-P/T effects the later timestamp's value wins, whichever was registered first, and the
// LKI shows exactly that value. Control: the base-only provider shows the printed 2/2.
#[test]
fn lki_shows_later_timestamp_set_pt_in_both_orders() {
    // Order 1: the earlier-timestamp effect (5/5) is registered first; the later one (1/7) wins.
    assert_eq!(two_setters((0, 5, 5), (1, 1, 7), true), spt(1, 7));
    // Order 2: the later-timestamp effect (1/7) is registered first; it still wins.
    assert_eq!(two_setters((1, 1, 7), (0, 5, 5), true), spt(1, 7));
    // Swapped values: the later timestamp now carries 5/5, in both registration orders.
    assert_eq!(two_setters((0, 1, 7), (1, 5, 5), true), spt(5, 5));
    assert_eq!(two_setters((1, 5, 5), (0, 1, 7), true), spt(5, 5));
    // Control.
    for (first, second) in [
        ((0, 5, 5), (1, 1, 7)),
        ((1, 1, 7), (0, 5, 5)),
        ((0, 1, 7), (1, 5, 5)),
        ((1, 5, 5), (0, 1, 7)),
    ] {
        let got = two_setters(first, second, false);
        assert_eq!(got, spt(2, 2), "control");
        assert_ne!(
            got,
            two_setters(first, second, true),
            "control discriminates"
        );
    }
}

/// The two-object simultaneous batch: `a` (with a printed static ability) grants
/// [`GRANTED`] to every creature through a linked static layer-4 effect, and `b` gets a
/// resolved layer-7c +2/+0. Both leave the battlefield in ONE batch (`a` first, or `b` first).
fn two_object_batch(a_first: bool, evaluated: bool) -> (Table, ObjectId, ObjectId) {
    let mut table = Table::new();
    let a = table.create(ZoneKind::Battlefield, P1);
    let b = table.create(ZoneKind::Battlefield, P2);
    let own = ability("test.grants_type_to_creatures");
    let mut objects = Objects::new();
    objects.put(
        a,
        with_ability(creature("test.lki_granter_1_1", 1, 1), own.clone()),
        P1,
    );
    objects.put(b, creature("test.lki_receiver_2_3", 2, 3), P2);
    register_static(
        &mut table.game,
        a,
        own,
        Layer::L4,
        SEL_CREATURE,
        grant_type(GRANTED),
    );
    register_resolved(&mut table.game, Layer::L7c, &[b], modify_pt(2, 0));
    let snap = table.game.clone();
    let env = GameEnvironment::new(&snap, &objects);
    let layered = LayeredProvider::new(snap.effects(), &env);
    let base = BaseProvider(&objects);
    let provider: &dyn CharacteristicsProvider = if evaluated { &layered } else { &base };
    let events = if a_first {
        vec![
            zone_change(a, ZoneKind::Graveyard),
            zone_change(b, ZoneKind::Graveyard),
        ]
    } else {
        vec![
            zone_change(b, ZoneKind::Graveyard),
            zone_change(a, ZoneKind::Graveyard),
        ]
    };
    let done = table.batch(provider, events);
    assert_eq!(done.events.len(), 2);
    assert!(done.events.iter().all(|e| e.lki_captured));
    (table, a, b)
}

// REQ-LAYER-001, REQ-CORE-003; CR 608.2h, 613.1d, 613.4c, 704.8 (pre-event capture for a
// simultaneous batch): when the granting object and the object it grants a type to leave in
// the same batch, both LKI records are captured before either move and hold evaluated values,
// in both submission orders. Control: the base-only provider holds the base values.
#[test]
fn simultaneous_batch_captures_both_records_before_either_move() {
    for a_first in [true, false] {
        let (t, a, b) = two_object_batch(a_first, true);
        let ra = t.lki_of(a);
        let rb = t.lki_of(b);
        // Both captured on the battlefield (before the move).
        assert_eq!(ra.zone(), ZoneId::Battlefield, "a_first={a_first}");
        assert_eq!(rb.zone(), ZoneId::Battlefield, "a_first={a_first}");
        // The granted type is on both, the source included; b also has its P/T change.
        assert!(has_type(ra.characteristics(), GRANTED), "a_first={a_first}");
        assert!(has_type(rb.characteristics(), GRANTED), "a_first={a_first}");
        assert_eq!(pt(ra.characteristics()), spt(1, 1));
        assert_eq!(pt(rb.characteristics()), spt(4, 3));
        assert_eq!(t.zone_of(a), None);
        assert_eq!(t.zone_of(b), None);

        // Control.
        let (c, a, b) = two_object_batch(a_first, false);
        assert!(!has_type(c.lki_of(a).characteristics(), GRANTED), "control");
        assert!(!has_type(c.lki_of(b).characteristics(), GRANTED), "control");
        assert_eq!(pt(c.lki_of(b).characteristics()), spt(2, 3), "control");
    }
}

// REQ-CORE-003, REQ-LAYER-001; CR 608.2h, 701.3d (a departing permanent's attachment links
// end), 704.8: in a simultaneous batch where the first departing object `a` is attached to the
// second object `b`, moving `a` removes the attachment from `b`'s live permanent state, yet
// `b`'s LKI (captured before either move) still lists `a` in `attached_here`; the attachments
// are read from the permanent container, not from the snapshot provider, so a capture made
// after `a` left would lose it. Checked in both submission orders.
#[test]
fn simultaneous_batch_lki_keeps_pre_batch_attachment_of_second_object() {
    for a_first in [true, false] {
        let mut table = Table::new();
        let a = table.create(ZoneKind::Battlefield, P1);
        let b = table.create(ZoneKind::Battlefield, P2);
        let mut objects = Objects::new();
        objects.put(a, creature("test.lki_attached_1_1", 1, 1), P1);
        objects.put(b, creature("test.lki_host_2_3", 2, 3), P2);
        table
            .permanents
            .attach(a, AttachTarget::Permanent(b))
            .expect("attach succeeds");
        assert_eq!(
            table.permanents.attachments_of(AttachTarget::Permanent(b)),
            vec![a]
        );
        let snap = table.game.clone();
        let env = GameEnvironment::new(&snap, &objects);
        let layered = LayeredProvider::new(snap.effects(), &env);
        let events = if a_first {
            vec![
                zone_change(a, ZoneKind::Graveyard),
                zone_change(b, ZoneKind::Graveyard),
            ]
        } else {
            vec![
                zone_change(b, ZoneKind::Graveyard),
                zone_change(a, ZoneKind::Graveyard),
            ]
        };
        let done = table.batch(&layered, events);
        assert!(done.events.iter().all(|e| e.lki_captured));
        // The link is gone from the live state after the batch.
        assert!(
            table
                .permanents
                .attachments_of(AttachTarget::Permanent(b))
                .is_empty()
        );
        // The LKI of the host still lists the pre-batch attachment.
        assert_eq!(table.lki_of(b).attached_here(), &[a], "a_first={a_first}");
        assert_eq!(table.lki_of(b).zone(), ZoneId::Battlefield);
    }
}

// REQ-CORE-003, REQ-LAYER-001; CR 608.2h, 613.4c (counters in layer 7c), 122.2 (counters cease
// on a zone change): counters put on through `AddCounters` events are in the LKI record
// (amounts >= 1, kinds ascending), and a +1/+1 counter that the provider's environment also
// holds is reflected in the evaluated LKI P/T. Control: the base-only provider keeps the base
// P/T, while the LKI counters (from the event layer's markers) are the same in both runs.
#[test]
fn lki_counters_and_counter_adjusted_pt() {
    fn run(evaluated: bool) -> LkiRecord {
        let mut f = fixture();
        // Setup through the event layer (the provider is irrelevant for AddCounters).
        let setup_objects = f.objects.clone();
        let setup = BaseProvider(&setup_objects);
        f.table.exec(&setup, add_counters(f.a, "test.zeta", 1));
        f.table.exec(&setup, add_counters(f.a, "+1/+1", 2));
        f.table.exec(&setup, add_counters(f.a, "test.alpha", 3));
        // The layer system's environment holds the same +1/+1 counters.
        f.objects.counter(f.a, "+1/+1", 2);
        let snap = f.table.game.clone();
        let env = GameEnvironment::new(&snap, &f.objects);
        let layered = LayeredProvider::new(snap.effects(), &env);
        let base = BaseProvider(&f.objects);
        let provider: &dyn CharacteristicsProvider = if evaluated { &layered } else { &base };
        let expected = evaluate(snap.effects(), &env, f.a).unwrap().characteristics;
        let done = f
            .table
            .exec(provider, zone_change(f.a, ZoneKind::Graveyard));
        assert!(done.lki_captured);
        let rec = f.table.lki_of(f.a).clone();
        if evaluated {
            assert_eq!(*rec.characteristics(), expected);
        }
        rec
    }

    let rec = run(true);
    let kinds: Vec<(&str, i64)> = rec
        .counters()
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(
        kinds,
        vec![("+1/+1", 2), ("test.alpha", 3), ("test.zeta", 1)],
        "every added counter, kinds ascending"
    );
    assert!(rec.counters().values().all(|&n| n >= 1));
    // Base 2/2, +1/+2 (layer 7c effect), +2/+2 (two +1/+1 counters): 5/6, artifact creature.
    assert_eq!(pt(rec.characteristics()), spt(5, 6));
    assert!(has_type(rec.characteristics(), GRANTED));

    // Control.
    let control = run(false);
    assert_eq!(pt(control.characteristics()), spt(2, 2), "control");
    assert!(!has_type(control.characteristics(), GRANTED), "control");
    assert_eq!(
        control.counters(),
        rec.counters(),
        "LKI counters come from the event layer, not the provider"
    );
}

// REQ-CORE-003, REQ-LAYER-001; CR 608.2h: the record is immutable and as of immediately before
// the event: removing the granting effects from the game after the move leaves the LKI as it
// was, while the current evaluation of the same object (over a new snapshot) changes.
#[test]
fn lki_is_unchanged_by_later_effect_removal() {
    let mut f = fixture();
    let snap = f.table.game.clone();
    let env = GameEnvironment::new(&snap, &f.objects);
    let provider = LayeredProvider::new(snap.effects(), &env);
    let before = move_and_capture(&mut f.table, f.a, &provider);
    assert!(f.table.game.remove_effect(f.grant));
    assert!(f.table.game.remove_effect(f.pump));
    let snap2 = f.table.game.clone();
    let env2 = GameEnvironment::new(&snap2, &f.objects);
    let now = LayeredProvider::new(snap2.effects(), &env2)
        .characteristics(f.a)
        .expect("the world still knows the base");
    assert!(
        !has_type(&now, GRANTED),
        "control: current evaluation lost the type"
    );
    assert_eq!(pt(&now), spt(2, 2));
    let after = f.table.lki_of(f.a);
    assert_eq!(*after, before);
    assert!(has_type(after.characteristics(), GRANTED));
    assert_eq!(pt(after.characteristics()), spt(3, 4));
}
