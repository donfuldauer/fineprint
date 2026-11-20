// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for timestamp assignment (REQ-CORE-002, CR 613.7a-n): the `TimestampLedger`,
//! the effect-timestamp functions and re-stamping of static effects. Black-box: written from
//! the work-item brief only.

#[path = "spec_layers_support.rs"]
mod support;

use fineprint_base::ids::{IdGenerator, Timestamp};
use fineprint_core::layers::{
    ContinuousEffects, CounterHolder, EffectOrigin, LayerError, LayerModification,
    SimultaneousStamp, StampReason, TimestampLedger, evaluate, restamp_static_effects,
    stamp_resolved_effect, stamp_static_and_resolved, static_effect_timestamp,
};
use support::*;

fn last(tsg: &IdGenerator<Timestamp>) -> Timestamp {
    ts(tsg.last_issued())
}

/// Every reason that gives an object a new timestamp, with the rule it implements.
fn reasons() -> Vec<(StampReason, &'static str)> {
    vec![
        (StampReason::EntersZone, "613.7d"),
        (StampReason::Attached, "613.7e"),
        (StampReason::TurnedFaceUpOrDown, "613.7f"),
        (StampReason::TransformedOrConverted, "613.7g"),
        (StampReason::SpecialCardTurnedFaceUp, "613.7h/613.7j"),
        (StampReason::GameStart, "613.7i/613.7j"),
        (StampReason::JoinedMergedPermanent, "613.7k"),
    ]
}

// ---------------------------------------------------------------------------------------------
// 613.7d-j: objects
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002, CR 613.7d: an object receives a timestamp when it enters a zone; successive
// entries get increasing timestamps from the caller's generator.
#[test]
fn enters_zone_gives_increasing_timestamps() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    assert_eq!(ledger.object_timestamp(oid(1)), None);
    let t1 = ledger
        .stamp_object(&mut tsg, oid(1), StampReason::EntersZone)
        .unwrap();
    let t2 = ledger
        .stamp_object(&mut tsg, oid(2), StampReason::EntersZone)
        .unwrap();
    let t3 = ledger
        .stamp_object(&mut tsg, oid(3), StampReason::EntersZone)
        .unwrap();
    assert!(t1 < t2 && t2 < t3);
    assert_eq!(t3, last(&tsg), "timestamps come from the generator");
    assert_eq!(ledger.object_timestamp(oid(1)), Some(t1));
    assert_eq!(ledger.object_timestamp(oid(2)), Some(t2));
    assert_eq!(ledger.object_timestamp(oid(3)), Some(t3));
}

// REQ-CORE-002, CR 613.7d: the ledger owns no counter: it continues from where the caller's
// generator stands.
#[test]
fn ledger_uses_the_callers_generator() {
    let mut tsg = IdGenerator::<Timestamp>::from_last_issued(41);
    let mut ledger = TimestampLedger::new();
    let t = ledger
        .stamp_object(&mut tsg, oid(1), StampReason::EntersZone)
        .unwrap();
    assert_eq!(t, ts(42));
    assert_eq!(tsg.last_issued(), 42);
}

// REQ-CORE-002, CR 613.7e, 613.7f, 613.7g, 613.7h, 613.7i, 613.7j: each re-stamp reason gives
// a NEW, strictly later timestamp to an object that already has one.
#[test]
fn every_reason_gives_a_new_strictly_later_timestamp() {
    for (reason, rule) in reasons() {
        let mut tsg = tgen();
        let mut ledger = TimestampLedger::new();
        let old = ledger
            .stamp_object(&mut tsg, oid(5), StampReason::EntersZone)
            .unwrap();
        // an unrelated stamp in between
        ledger
            .stamp_object(&mut tsg, oid(6), StampReason::EntersZone)
            .unwrap();
        let new = ledger.stamp_object(&mut tsg, oid(5), reason).unwrap();
        assert!(new > old, "{rule}");
        assert_eq!(new, last(&tsg), "{rule}");
        assert_eq!(ledger.object_timestamp(oid(5)), Some(new), "{rule}");
        assert!(ledger.object_timestamp(oid(6)).unwrap() < new, "{rule}");
    }
}

// REQ-CORE-002, CR 613.7h, 613.7i, 613.7j: a game-start stamp and a special card turned face
// up also work for an object with no timestamp yet.
#[test]
fn game_start_and_special_card_stamp_fresh_objects() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let a = ledger
        .stamp_object(&mut tsg, oid(1), StampReason::GameStart)
        .unwrap();
    let b = ledger
        .stamp_object(&mut tsg, oid(2), StampReason::GameStart)
        .unwrap();
    let c = ledger
        .stamp_object(&mut tsg, oid(3), StampReason::SpecialCardTurnedFaceUp)
        .unwrap();
    assert!(a < b && b < c);
    // 613.7j: a face-down conspiracy turned face up gets a new timestamp
    let d = ledger
        .stamp_object(&mut tsg, oid(2), StampReason::SpecialCardTurnedFaceUp)
        .unwrap();
    assert!(d > c);
    assert_eq!(ledger.object_timestamp(oid(2)), Some(d));
}

// REQ-CORE-002: forget_object drops the object's timestamp, its stickers and its counters;
// other objects and player counters are untouched.
#[test]
fn forget_object_drops_everything_of_the_object() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let ta = ledger
        .stamp_object(&mut tsg, oid(1), StampReason::EntersZone)
        .unwrap();
    let tb = ledger
        .stamp_object(&mut tsg, oid(2), StampReason::EntersZone)
        .unwrap();
    ledger.put_sticker(&mut tsg, oid(1), 1).unwrap();
    let sb = ledger.put_sticker(&mut tsg, oid(2), 1).unwrap();
    ledger
        .put_counters(&mut tsg, CounterHolder::Object(oid(1)), "+1/+1")
        .unwrap();
    let cb = ledger
        .put_counters(&mut tsg, CounterHolder::Object(oid(2)), "+1/+1")
        .unwrap();
    let cp = ledger
        .put_counters(&mut tsg, CounterHolder::Player(P1), "poison")
        .unwrap();
    assert_eq!(ledger.object_timestamp(oid(1)), Some(ta));
    ledger.forget_object(oid(1));
    assert_eq!(ledger.object_timestamp(oid(1)), None);
    assert_eq!(ledger.sticker_timestamp(oid(1), 1), None);
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Object(oid(1)), "+1/+1"),
        None
    );
    assert_eq!(ledger.object_timestamp(oid(2)), Some(tb));
    assert_eq!(ledger.sticker_timestamp(oid(2), 1), Some(sb));
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Object(oid(2)), "+1/+1"),
        Some(cb)
    );
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Player(P1), "poison"),
        Some(cp)
    );
}

// REQ-CORE-002: the ledger is a value: Default equals new(), clones compare equal.
#[test]
fn ledger_default_and_clone() {
    assert_eq!(TimestampLedger::default(), TimestampLedger::new());
    let mut tsg = tgen();
    let mut l = TimestampLedger::new();
    l.stamp_object(&mut tsg, oid(1), StampReason::EntersZone)
        .unwrap();
    let c = l.clone();
    assert_eq!(c, l);
    assert_ne!(c, TimestampLedger::new());
}

// ---------------------------------------------------------------------------------------------
// 613.7a: static effects
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002, CR 613.7a: a printed static ability's effect has the object's timestamp; a
// granted one has the later of the object's and the granting effect's; a Resolved origin
// returns the object timestamp unchanged (documented; never panics).
#[test]
fn static_effect_timestamp_rules() {
    let printed = EffectOrigin::Static { granted_at: None };
    assert_eq!(static_effect_timestamp(ts(5), &printed), ts(5));
    let granted_later = EffectOrigin::Static {
        granted_at: Some(ts(9)),
    };
    assert_eq!(static_effect_timestamp(ts(5), &granted_later), ts(9));
    let granted_earlier = EffectOrigin::Static {
        granted_at: Some(ts(2)),
    };
    assert_eq!(static_effect_timestamp(ts(5), &granted_earlier), ts(5));
    let granted_same = EffectOrigin::Static {
        granted_at: Some(ts(5)),
    };
    assert_eq!(static_effect_timestamp(ts(5), &granted_same), ts(5));
    assert_eq!(
        static_effect_timestamp(ts(5), &EffectOrigin::Resolved),
        ts(5)
    );
}

// REQ-CORE-002, CR 613.7a (second sentence): when an object with a printed effect (T_h) and a
// granted effect (T_r, later) is re-stamped, both effects get new timestamps, the printed one
// the object's new timestamp and the granted one a later fresh stamp, so printed < granted
// still holds; resolved effects and other sources' effects are untouched.
#[test]
fn restamp_static_effects_keeps_relative_order() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let host = oid(1);
    let other = oid(2);
    let t_h = ledger
        .stamp_object(&mut tsg, host, StampReason::EntersZone)
        .unwrap();
    let t_other = ledger
        .stamp_object(&mut tsg, other, StampReason::EntersZone)
        .unwrap();
    let t_r = stamp_resolved_effect(&mut tsg).unwrap();
    assert!(t_h < t_r);
    let t_res = stamp_resolved_effect(&mut tsg).unwrap();
    let t_early_grant = ts(t_h.get() - 1);

    let printed = static_eff(
        host,
        None,
        Layer::L6,
        t_h.get(),
        objs(&[oid(9)]),
        LayerModification::RemoveKeyword("flying".into()),
    );
    let granted = static_eff(
        host,
        Some(t_r.get()),
        Layer::L6,
        t_r.get(),
        objs(&[oid(9)]),
        LayerModification::AddAbility(kw_ability("g", "flying")),
    );
    // granted by an effect earlier than the object: it has the object's timestamp
    let granted_early = static_eff(
        host,
        Some(t_early_grant.get()),
        Layer::L7c,
        t_h.get(),
        objs(&[oid(9)]),
        modify_pt(1, 1),
    );
    let mut resolved = eff(Layer::L7c, t_res.get(), objs(&[oid(9)]), modify_pt(2, 2));
    resolved.source = Some(host);
    let foreign = static_eff(
        other,
        None,
        Layer::L7c,
        t_other.get(),
        objs(&[oid(9)]),
        modify_pt(3, 3),
    );
    let mut effects = registry(&[
        (1, printed),
        (2, granted),
        (3, granted_early),
        (4, resolved.clone()),
        (5, foreign.clone()),
    ]);

    let new_host = ledger
        .stamp_object(&mut tsg, host, StampReason::Attached)
        .unwrap();
    restamp_static_effects(&mut effects, &mut tsg, host, t_h, new_host).unwrap();

    let t = |id: u64| effects.get(eid(id)).unwrap().timestamp;
    assert_eq!(
        t(1),
        new_host,
        "printed effect gets the object's new timestamp"
    );
    assert!(
        t(2) > new_host,
        "granted effect stays later than the object"
    );
    assert_ne!(t(2), t_r);
    assert!(t(1) < t(2), "relative order unchanged");
    assert!(t(2) <= last(&tsg), "fresh stamps come from the generator");
    assert_eq!(
        t(3),
        new_host,
        "early-granted effect gets the object's new timestamp"
    );
    assert_eq!(
        effects.get(eid(4)),
        Some(&resolved),
        "resolved effect untouched"
    );
    assert_eq!(
        effects.get(eid(5)),
        Some(&foreign),
        "other source untouched"
    );
}

// REQ-CORE-002, CR 613.7a: several granted effects of one object keep their relative order
// (by old timestamp, then EffectId) after a re-stamp, and all follow the object's new stamp.
#[test]
fn restamp_preserves_order_among_granted_effects() {
    let mut tsg = IdGenerator::<Timestamp>::from_last_issued(20);
    let host = oid(1);
    let old = ts(3);
    let g = |granted: u64, t: u64| {
        static_eff(
            host,
            Some(granted),
            Layer::L6,
            t,
            objs(&[oid(9)]),
            LayerModification::RemoveAllAbilities,
        )
    };
    // EffectId order differs from timestamp order on purpose; ids 7 and 8 tie on timestamp.
    let mut effects = registry(&[
        (5, g(12, 12)),
        (6, g(10, 10)),
        (7, g(15, 15)),
        (8, g(15, 15)),
        (9, g(11, 11)),
    ]);
    let new_ts = tsg.issue().unwrap();
    restamp_static_effects(&mut effects, &mut tsg, host, old, new_ts).unwrap();
    let t = |id: u64| effects.get(eid(id)).unwrap().timestamp;
    // old order: 6 (10), 9 (11), 5 (12), 7 (15, id 7), 8 (15, id 8)
    let order = [6u64, 9, 5, 7, 8];
    for w in order.windows(2) {
        assert!(t(w[0]) < t(w[1]), "{} before {}", w[0], w[1]);
    }
    assert!(t(6) > new_ts);
    assert_eq!(t(8), last(&tsg));
}

// REQ-CORE-002, CR 613.7a, 613.7e (Rune-of-Flight-like scenario, synthetic): an Aura grants
// an Equipment "equipped creature has flying" later than the Equipment's own "equipped creature
// loses flying"; the creature has flying. After the Equipment is re-stamped (attached again)
// and its static effects re-stamped, the same effect still wins.
#[test]
fn granted_flying_versus_printed_loses_flying_survives_restamp() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let mut env = TestEnv::new();
    let creature_id = env.put(oid(1), creature("test.bearer_2_2", 2, 2));
    let equipment = env.put(oid(2), artifact("test.heavy_equipment_like"));
    let aura = env.put(oid(3), enchantment("test.wing_rune_like"));
    let other_creature = env.put(oid(4), creature("test.second_bearer_1_1", 1, 1));

    let t_equipment = ledger
        .stamp_object(&mut tsg, equipment, StampReason::EntersZone)
        .unwrap();
    ledger
        .stamp_object(&mut tsg, creature_id, StampReason::EntersZone)
        .unwrap();
    let t_aura = ledger
        .stamp_object(&mut tsg, aura, StampReason::Attached)
        .unwrap();
    assert!(t_aura > t_equipment);

    let printed_origin = EffectOrigin::Static { granted_at: None };
    let granted_origin = EffectOrigin::Static {
        granted_at: Some(t_aura),
    };
    let loses = static_eff(
        equipment,
        None,
        Layer::L6,
        static_effect_timestamp(t_equipment, &printed_origin).get(),
        objs(&[creature_id]),
        LayerModification::RemoveKeyword("flying".into()),
    );
    let pump = static_eff(
        equipment,
        None,
        Layer::L7c,
        static_effect_timestamp(t_equipment, &printed_origin).get(),
        objs(&[creature_id]),
        modify_pt(10, 10),
    );
    let has = static_eff(
        equipment,
        Some(t_aura.get()),
        Layer::L6,
        static_effect_timestamp(t_equipment, &granted_origin).get(),
        objs(&[creature_id]),
        LayerModification::AddAbility(kw_ability("test.granted_flying", "flying")),
    );
    assert_eq!(has.timestamp, t_aura);
    let mut effects = registry(&[(1, loses), (2, pump), (3, has)]);

    let before = evaluate(&effects, &env, creature_id).unwrap();
    assert_eq!(count_kw(&before.characteristics, "flying"), 1);
    assert_eq!(pt(&before), spt(12, 12));

    // 613.7e: the Equipment becomes attached again -> a new timestamp; 613.7a: its static
    // effects are re-stamped.
    let t_new = ledger
        .stamp_object(&mut tsg, equipment, StampReason::Attached)
        .unwrap();
    restamp_static_effects(&mut effects, &mut tsg, equipment, t_equipment, t_new).unwrap();
    let t_loses = effects.get(eid(1)).unwrap().timestamp;
    let t_has = effects.get(eid(3)).unwrap().timestamp;
    assert_eq!(t_loses, t_new);
    assert!(t_has > t_new);
    assert!(
        t_loses > t_aura,
        "the printed effect is now later than the aura"
    );

    let after = evaluate(&effects, &env, creature_id).unwrap();
    assert_eq!(count_kw(&after.characteristics, "flying"), 1, "same winner");
    assert_eq!(pt(&after), spt(12, 12));
    // unaffected creature
    assert_eq!(
        norm(
            &evaluate(&effects, &env, other_creature)
                .unwrap()
                .characteristics
        ),
        *env.base_chars(other_creature)
    );

    // control: without the re-stamp of the granted effect (only the printed one moved), the
    // winner would flip; this is what restamp_static_effects prevents.
    let mut wrong: ContinuousEffects = effects.clone();
    wrong.set_timestamp(eid(3), t_aura).unwrap();
    assert!(!has_kw(
        &evaluate(&wrong, &env, creature_id).unwrap().characteristics,
        "flying"
    ));
}

// ---------------------------------------------------------------------------------------------
// 613.7b, 613.7n: resolved effects
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002, CR 613.7b: a resolved effect gets a new timestamp at creation, strictly
// increasing and shared with the object stamps (one monotonic counter).
#[test]
fn stamp_resolved_effect_is_strictly_increasing() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let mut prev = ts(0);
    for i in 0..20u64 {
        let t = if i % 3 == 0 {
            ledger
                .stamp_object(&mut tsg, oid(i), StampReason::EntersZone)
                .unwrap()
        } else {
            stamp_resolved_effect(&mut tsg).unwrap()
        };
        assert!(t > prev);
        assert_eq!(t, last(&tsg));
        prev = t;
    }
}

// REQ-CORE-002, CR 613.7n: the object's own static-ability effect gets an earlier timestamp
// than a simultaneous resolved effect.
#[test]
fn stamp_static_and_resolved_static_first() {
    let mut tsg = IdGenerator::<Timestamp>::from_last_issued(7);
    let (s, r) = stamp_static_and_resolved(&mut tsg).unwrap();
    assert!(s < r);
    assert!(s > ts(7));
    assert_eq!(r, last(&tsg));
    let (s2, r2) = stamp_static_and_resolved(&mut tsg).unwrap();
    assert!(r < s2 && s2 < r2);
}

// ---------------------------------------------------------------------------------------------
// 613.7c: counters
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002, CR 613.7c: putting a counter of a kind that is already there gives every
// counter of that kind the new timestamp; other kinds and other holders are independent;
// clear_counters forgets the entry.
#[test]
fn counters_of_a_kind_share_the_latest_timestamp() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let a = CounterHolder::Object(oid(1));
    let first = ledger.put_counters(&mut tsg, a, "+1/+1").unwrap();
    let other_kind = ledger
        .put_counters(&mut tsg, CounterHolder::Object(oid(1)), "flying")
        .unwrap();
    let other_holder = ledger
        .put_counters(&mut tsg, CounterHolder::Object(oid(2)), "+1/+1")
        .unwrap();
    let player = ledger
        .put_counters(&mut tsg, CounterHolder::Player(P1), "+1/+1")
        .unwrap();
    let second = ledger
        .put_counters(&mut tsg, CounterHolder::Object(oid(1)), "+1/+1")
        .unwrap();
    assert!(second > first);
    assert_eq!(second, last(&tsg));
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Object(oid(1)), "+1/+1"),
        Some(second)
    );
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Object(oid(1)), "flying"),
        Some(other_kind)
    );
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Object(oid(2)), "+1/+1"),
        Some(other_holder)
    );
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Player(P1), "+1/+1"),
        Some(player)
    );
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Player(P2), "+1/+1"),
        None
    );

    ledger.clear_counters(CounterHolder::Object(oid(1)), "+1/+1");
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Object(oid(1)), "+1/+1"),
        None
    );
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Object(oid(1)), "flying"),
        Some(other_kind)
    );
    assert_eq!(
        ledger.counter_timestamp(CounterHolder::Object(oid(2)), "+1/+1"),
        Some(other_holder)
    );
    // a new counter after clearing gets a fresh timestamp
    let third = ledger
        .put_counters(&mut tsg, CounterHolder::Object(oid(1)), "+1/+1")
        .unwrap();
    assert!(third > second);
}

// REQ-CORE-002, CR 613.7c, 613.1f: the ledger's counter timestamps drive the layer-6 order of
// a keyword counter: a second flying counter put after a removal effect makes the keyword
// survive it.
#[test]
fn ledger_counter_timestamp_drives_keyword_counter_order() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let first = ledger
        .put_counters(&mut tsg, CounterHolder::Object(a), "flying")
        .unwrap();
    env.counter(a, "flying", 1, first.get());
    let removal = stamp_resolved_effect(&mut tsg).unwrap();
    let effects = registry(&[(
        1,
        eff(
            Layer::L6,
            removal.get(),
            objs(&[a]),
            LayerModification::RemoveAllAbilities,
        ),
    )]);
    assert!(!has_kw(
        &evaluate(&effects, &env, a).unwrap().characteristics,
        "flying"
    ));
    let second = ledger
        .put_counters(&mut tsg, CounterHolder::Object(a), "flying")
        .unwrap();
    env.counter(
        a,
        "flying",
        2,
        ledger
            .counter_timestamp(CounterHolder::Object(a), "flying")
            .unwrap()
            .get(),
    );
    assert!(second > removal);
    assert_eq!(
        count_kw(
            &evaluate(&effects, &env, a).unwrap().characteristics,
            "flying"
        ),
        1
    );
}

// ---------------------------------------------------------------------------------------------
// 613.7k: stickers
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002, CR 613.7k: a sticker gets a new timestamp when put on; when the host gets a new
// timestamp, its stickers get new timestamps immediately after it, in their old relative order,
// with no other stamp between.
#[test]
fn stickers_restamped_right_after_their_host() {
    for (reason, rule) in reasons() {
        let mut tsg = tgen();
        let mut ledger = TimestampLedger::new();
        let host = oid(1);
        ledger
            .stamp_object(&mut tsg, host, StampReason::EntersZone)
            .unwrap();
        // sticker 7 put before sticker 3: old relative order 7 < 3
        let s7 = ledger.put_sticker(&mut tsg, host, 7).unwrap();
        let s3 = ledger.put_sticker(&mut tsg, host, 3).unwrap();
        assert!(s7 < s3, "{rule}");
        assert_eq!(s3, last(&tsg), "{rule}");
        assert_eq!(ledger.sticker_timestamp(host, 7), Some(s7));
        assert_eq!(ledger.sticker_timestamp(host, 3), Some(s3));
        // a sticker on another host is not touched
        ledger
            .stamp_object(&mut tsg, oid(2), StampReason::EntersZone)
            .unwrap();
        let s_other = ledger.put_sticker(&mut tsg, oid(2), 1).unwrap();

        let h = ledger.stamp_object(&mut tsg, host, reason).unwrap();
        assert_eq!(ledger.object_timestamp(host), Some(h), "{rule}");
        let n7 = ledger.sticker_timestamp(host, 7).unwrap();
        let n3 = ledger.sticker_timestamp(host, 3).unwrap();
        assert!(h < n7 && n7 < n3, "{rule}: old relative order kept");
        assert_eq!(n7, ts(h.get() + 1), "{rule}: immediately after the host");
        assert_eq!(n3, ts(h.get() + 2), "{rule}: no other stamp between");
        assert_eq!(last(&tsg), n3, "{rule}");
        assert_eq!(ledger.sticker_timestamp(oid(2), 1), Some(s_other), "{rule}");
        assert_eq!(ledger.sticker_timestamp(host, 99), None);
    }
}

// REQ-CORE-002, CR 613.7k: putting a sticker again gives it a new timestamp.
#[test]
fn put_sticker_again_gives_a_new_timestamp() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let a = ledger.put_sticker(&mut tsg, oid(1), 1).unwrap();
    let b = ledger.put_sticker(&mut tsg, oid(1), 1).unwrap();
    assert!(b > a);
    assert_eq!(ledger.sticker_timestamp(oid(1), 1), Some(b));
}

// ---------------------------------------------------------------------------------------------
// 613.7m: simultaneous timestamps
// ---------------------------------------------------------------------------------------------

fn stamp(object: u64, player: fineprint_base::ids::PlayerId, choice: u32) -> SimultaneousStamp {
    SimultaneousStamp {
        object: oid(object),
        player,
        choice,
    }
}

fn batch() -> Vec<SimultaneousStamp> {
    vec![
        stamp(5, P2, 0),
        stamp(3, P1, 2),
        stamp(9, P1, 1),
        stamp(4, P1, 1),
        stamp(8, P2, 1),
        stamp(2, P2, 0),
    ]
}

// REQ-CORE-002, CR 613.7m, 101.4: simultaneous stamps go in APNAP order: the active player's
// objects first in that player's chosen order, then each other player's; strictly increasing
// and returned in that order.
#[test]
fn simultaneous_stamps_follow_apnap_then_choice() {
    let mut tsg = IdGenerator::<Timestamp>::from_last_issued(10);
    let mut ledger = TimestampLedger::new();
    let out = ledger
        .stamp_objects_simultaneously(&mut tsg, &[P1, P2], &batch(), StampReason::EntersZone)
        .unwrap();
    let objects: Vec<u64> = out.iter().map(|(o, _)| o.get()).collect();
    // P1: choice 1 (objects 4, 9 by ObjectId), choice 2 (3); P2: choice 0 (2, 5), choice 1 (8)
    assert_eq!(objects, vec![4, 9, 3, 2, 5, 8]);
    let stamps: Vec<u64> = out.iter().map(|(_, t)| t.get()).collect();
    assert_eq!(stamps, vec![11, 12, 13, 14, 15, 16]);
    for (o, t) in &out {
        assert_eq!(ledger.object_timestamp(*o), Some(*t));
    }
}

// REQ-CORE-002, CR 613.7m, 101.4: with apnap [P2, P1] the player blocks are reversed.
#[test]
fn simultaneous_stamps_with_other_active_player() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let out = ledger
        .stamp_objects_simultaneously(&mut tsg, &[P2, P1], &batch(), StampReason::Attached)
        .unwrap();
    let objects: Vec<u64> = out.iter().map(|(o, _)| o.get()).collect();
    assert_eq!(objects, vec![2, 5, 8, 4, 9, 3]);
    for w in out.windows(2) {
        assert!(w[0].1 < w[1].1);
    }
    assert_eq!(out.last().unwrap().1, last(&tsg));
}

// REQ-CORE-002, CR 613.7m: every input order of the batch gives the same result.
#[test]
fn simultaneous_stamps_ignore_batch_input_order() {
    let base = batch();
    let mut expected = None;
    for perm in permutations(base.len()).into_iter().step_by(7) {
        let items: Vec<SimultaneousStamp> = perm
            .iter()
            .map(|i| stamp(base[*i].object.get(), base[*i].player, base[*i].choice))
            .collect();
        let mut tsg = tgen();
        let mut ledger = TimestampLedger::new();
        let out = ledger
            .stamp_objects_simultaneously(&mut tsg, &[P1, P2], &items, StampReason::EntersZone)
            .unwrap();
        match &expected {
            None => expected = Some((out, ledger)),
            Some((o, l)) => {
                assert_eq!(&out, o);
                assert_eq!(&ledger, l);
            }
        }
    }
}

// REQ-CORE-002, CR 613.7m: a player not in apnap is UnknownPlayer; an object twice is
// DuplicateObject; an empty batch returns no pairs and issues nothing.
#[test]
fn simultaneous_stamps_errors_and_empty_batch() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let res = ledger.stamp_objects_simultaneously(
        &mut tsg,
        &[P1, P2],
        &[stamp(1, P1, 0), stamp(2, P3, 0)],
        StampReason::EntersZone,
    );
    assert_eq!(res, Err(LayerError::UnknownPlayer(P3)));
    let res = ledger.stamp_objects_simultaneously(
        &mut tsg,
        &[P1, P2],
        &[stamp(1, P1, 0), stamp(2, P2, 0), stamp(1, P2, 1)],
        StampReason::EntersZone,
    );
    assert_eq!(res, Err(LayerError::DuplicateObject(oid(1))));
    let before_gen = tsg.last_issued();
    let before = ledger.clone();
    let res =
        ledger.stamp_objects_simultaneously(&mut tsg, &[P1, P2], &[], StampReason::EntersZone);
    assert_eq!(res, Ok(vec![]));
    assert_eq!(tsg.last_issued(), before_gen);
    assert_eq!(ledger, before);
}

// REQ-CORE-002, CR 613.7m: simultaneous stamps are later than every earlier stamp and replace
// an object's previous timestamp.
#[test]
fn simultaneous_stamps_are_later_than_everything_before() {
    let mut tsg = tgen();
    let mut ledger = TimestampLedger::new();
    let t0 = ledger
        .stamp_object(&mut tsg, oid(1), StampReason::EntersZone)
        .unwrap();
    let out = ledger
        .stamp_objects_simultaneously(
            &mut tsg,
            &[P1, P2],
            &[stamp(1, P1, 0), stamp(2, P2, 0)],
            StampReason::EntersZone,
        )
        .unwrap();
    assert!(out.iter().all(|(_, t)| *t > t0));
    assert_eq!(ledger.object_timestamp(oid(1)), Some(out[0].1));
}

// ---------------------------------------------------------------------------------------------
// exhaustion
// ---------------------------------------------------------------------------------------------

/// A ledger with some content, for the exhaustion tests.
fn populated() -> TimestampLedger {
    let mut tsg = tgen();
    let mut l = TimestampLedger::new();
    l.stamp_object(&mut tsg, oid(1), StampReason::EntersZone)
        .unwrap();
    l.put_sticker(&mut tsg, oid(1), 1).unwrap();
    l.put_counters(&mut tsg, CounterHolder::Object(oid(1)), "+1/+1")
        .unwrap();
    l
}

// REQ-CORE-002: with an exhausted generator every issuing call returns IdExhausted and
// leaves the ledger (and generator) unchanged.
#[test]
fn exhausted_generator_every_call_fails_and_changes_nothing() {
    let mut tsg = IdGenerator::<Timestamp>::from_last_issued(u64::MAX);
    let mut ledger = populated();
    let before = ledger.clone();
    for (reason, rule) in reasons() {
        assert_eq!(
            ledger.stamp_object(&mut tsg, oid(1), reason),
            Err(LayerError::IdExhausted),
            "{rule}"
        );
        assert_eq!(ledger, before, "{rule}");
    }
    assert_eq!(
        ledger.stamp_object(&mut tsg, oid(5), StampReason::EntersZone),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(
        ledger.stamp_objects_simultaneously(
            &mut tsg,
            &[P1],
            &[stamp(5, P1, 0)],
            StampReason::EntersZone
        ),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(
        ledger.put_counters(&mut tsg, CounterHolder::Object(oid(1)), "+1/+1"),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(
        ledger.put_counters(&mut tsg, CounterHolder::Player(P1), "poison"),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(
        ledger.put_sticker(&mut tsg, oid(1), 2),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(ledger, before);
    assert_eq!(
        stamp_resolved_effect(&mut tsg),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(
        stamp_static_and_resolved(&mut tsg),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(tsg.last_issued(), u64::MAX);

    // restamp needs a fresh stamp for a granted effect later than the old object timestamp
    let mut effects = registry(&[
        (
            1,
            static_eff(
                oid(1),
                None,
                Layer::L7c,
                5,
                objs(&[oid(1)]),
                modify_pt(1, 1),
            ),
        ),
        (
            2,
            static_eff(
                oid(1),
                Some(8),
                Layer::L7c,
                8,
                objs(&[oid(1)]),
                modify_pt(1, 1),
            ),
        ),
    ]);
    let effects_before = effects.clone();
    assert_eq!(
        restamp_static_effects(&mut effects, &mut tsg, oid(1), ts(5), ts(9)),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(effects, effects_before, "effects unchanged on exhaustion");
}

// REQ-CORE-002: a generator with too few stamps left for a whole call (a batch of two, a host
// with a sticker, a static/resolved pair) fails with IdExhausted and leaves the ledger
// unchanged (no partial update).
#[test]
fn nearly_exhausted_generator_is_all_or_nothing() {
    let mut ledger = populated();
    let before = ledger.clone();

    let mut tsg = IdGenerator::<Timestamp>::from_last_issued(u64::MAX - 1);
    assert_eq!(
        ledger.stamp_objects_simultaneously(
            &mut tsg,
            &[P1, P2],
            &[stamp(5, P1, 0), stamp(6, P2, 0)],
            StampReason::EntersZone
        ),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(ledger, before);

    // object 1 has one sticker: the host stamp and the sticker stamp need two values
    let mut tsg = IdGenerator::<Timestamp>::from_last_issued(u64::MAX - 1);
    assert_eq!(
        ledger.stamp_object(&mut tsg, oid(1), StampReason::Attached),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(ledger, before);

    let mut tsg = IdGenerator::<Timestamp>::from_last_issued(u64::MAX - 1);
    assert_eq!(
        stamp_static_and_resolved(&mut tsg),
        Err(LayerError::IdExhausted)
    );

    // with exactly enough left it succeeds
    let mut tsg = IdGenerator::<Timestamp>::from_last_issued(u64::MAX - 2);
    let h = ledger
        .stamp_object(&mut tsg, oid(1), StampReason::Attached)
        .unwrap();
    assert_eq!(h, ts(u64::MAX - 1));
    assert_eq!(ledger.sticker_timestamp(oid(1), 1), Some(ts(u64::MAX)));
}
