// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests: the layer pipeline's effects, 613.7 timestamps and effect-to-ability links are
//! part of `GameState`: hashed in the Merkle tree (T1), registered all-or-nothing with the
//! layer-7a boundary (T6). Black-box: written from the work-item brief (#183 FP-144) only.

#[path = "spec_layers_state_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::encode::{encode_to_vec, put_len};
use fineprint_base::ids::EffectId;
use fineprint_core::layers::{
    Affected, ContinuousEffect, CounterHolder, EffectOrigin, LayerModification, StampReason,
};
use fineprint_core::state::{GameEnvironment, GameState, GeneratingAbility};
use support::*;

/// The root of `GameState::new(42, 2, 20)` under format version 1 (before this work item).
const OLD_GOLDEN_NEW_42_2_20: &str =
    "f32bbec462508986efd89ed35f9184bec86a107f774505e19c4f65d3db4fb0d5";

/// The template effect of the mutation-sensitivity test: a static, printed, layer-6 effect of
/// o1 adding a marker to creatures, timestamp 2.
fn template() -> ContinuousEffect {
    static_sel(
        oid(1),
        ts(2),
        Layer::L6,
        SEL_CREATURE,
        LayerModification::AddAbility(marker()),
    )
}

/// `base()` with `e` registered through `register_effect`, linked to `generator` when the
/// effect is static with a source.
fn with_effect(e: ContinuousEffect) -> GameState {
    let mut s = base();
    let generator = match (&e.origin, e.source) {
        (EffectOrigin::Static { .. }, Some(src)) => {
            Some(GeneratingAbility::new(src, own_ability(src.get())))
        }
        _ => None,
    };
    s.register_effect(e, generator).unwrap();
    s
}

// REQ-NFR-006, REQ-CORE-001, REQ-CORE-006 (T1a): a fresh state hashes the three new leaves as
// `put_len(0)` (four zero bytes) and every pre-existing leaf exactly as its independent
// canonical encoding; its root differs from the format-version-1 golden root.
#[test]
fn fresh_state_new_leaves_are_empty_and_old_leaves_unchanged() {
    let mut zero = Vec::new();
    put_len(&mut zero, 0);
    assert_eq!(zero, vec![0, 0, 0, 0]);
    for (seed, players) in [(42u64, 2usize), (1, 1), (9, 4)] {
        let mut s = GameState::new(seed, players, 20).unwrap();
        s.issue_card_id().unwrap();
        s.set_turn_number(3);
        for l in NEW_LEAVES {
            assert_eq!(leaf(&s, &[l]), Some(zero.clone()), "leaf {l}");
        }
        let old = old_leaves(&s);
        assert_eq!(old["prng"], Some(encode_to_vec(s.prng())));
        let ids = s.ids();
        for (k, v) in [
            ("card", ids.card.last_issued()),
            ("object", ids.object.last_issued()),
            ("ability_instance", ids.ability_instance.last_issued()),
            ("effect", ids.effect.last_issued()),
            ("timestamp", ids.timestamp.last_issued()),
            ("link", ids.link.last_issued()),
            ("player", ids.player.last_issued()),
        ] {
            assert_eq!(old[&format!("ids/{k}")], Some(encode_to_vec(&v)), "ids/{k}");
        }
        assert_eq!(
            old["turn/number"],
            Some(encode_to_vec(&s.turn().turn_number()))
        );
        assert_eq!(
            old["turn/active_player"],
            Some(encode_to_vec(&s.turn().active_player()))
        );
        assert_eq!(
            old["turn/priority_player"],
            Some(encode_to_vec(&s.turn().priority_player()))
        );
        assert_eq!(
            old["turn/order"],
            Some(encode_to_vec(&s.turn_order().to_vec()))
        );
        for p in s.players() {
            let id = p.id().get();
            assert_eq!(
                old[&format!("players/{id}/life")],
                Some(encode_to_vec(&p.life()))
            );
            assert_eq!(
                old[&format!("players/{id}/counters")],
                Some(encode_to_vec(p.counters()))
            );
            assert_eq!(
                old[&format!("players/{id}/designations")],
                Some(zero.clone())
            );
        }
        assert_eq!(old["history"], Some(zero.clone()));
    }
    assert_ne!(
        GameState::new(42, 2, 20).unwrap().state_hash().to_hex(),
        OLD_GOLDEN_NEW_42_2_20,
        "the root gains two leaves, so the format-version-1 golden root no longer holds"
    );
}

// REQ-NFR-006, REQ-LAYER-001, REQ-CORE-002 (T1b): each new component is hashed in its own
// leaf: `register_effect` changes `/effects` (and the effect-id generator), `stamp_object`
// changes `/timestamps` (and the timestamp generator, CR 613.7d), `link_effect_generator`
// changes `/effect_generators` only; a linked static registration also changes
// `/effect_generators`.
#[test]
fn each_new_component_changes_only_its_leaf() {
    let a = base();

    let mut b = a.clone();
    b.register_effect(
        resolved(
            ts(3),
            Layer::L7c,
            &[1],
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
        None,
    )
    .unwrap();
    assert_eq!(changed_leaves(&a, &b), names(&["effects", "ids/effect"]));
    assert_ne!(a.state_hash(), b.state_hash());

    let mut c = a.clone();
    c.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    assert_eq!(
        changed_leaves(&a, &c),
        names(&["timestamps", "ids/timestamp"])
    );
    assert_ne!(a.state_hash(), c.state_hash());

    let mut d = a.clone();
    let id = d.issue_effect_id().unwrap();
    let d0 = d.clone();
    d.link_effect_generator(id, GeneratingAbility::new(oid(2), own_ability(2)))
        .unwrap();
    assert_eq!(changed_leaves(&d0, &d), names(&["effect_generators"]));
    assert_ne!(d0.state_hash(), d.state_hash());

    let e = with_effect(template());
    assert_eq!(
        changed_leaves(&a, &e),
        names(&["effects", "effect_generators", "ids/effect"])
    );
}

// REQ-NFR-006, REQ-CORE-001, CR 613.7d (T1c): stamping A then B differs from B then A (the
// timestamps differ); the same effects and links reached through different call orders hash
// equal.
#[test]
fn equal_content_hashes_equal_and_stamp_order_matters() {
    let mut ab = base();
    ab.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    ab.stamp_object(oid(2), StampReason::EntersZone).unwrap();
    let mut ba = base();
    ba.stamp_object(oid(2), StampReason::EntersZone).unwrap();
    ba.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    assert_ne!(ab.state_hash(), ba.state_hash());
    assert_ne!(ab, ba);

    // Two effects whose ids and timestamps are fixed before registration, registered in
    // opposite orders.
    let e1 = resolved(
        ts(1),
        Layer::L5,
        &[1],
        LayerModification::AddColors(ColorBitmask::RED),
    );
    let e2 = resolved(
        ts(2),
        Layer::L7c,
        &[2],
        LayerModification::ModifyPt {
            power: 2,
            toughness: 0,
        },
    );
    let mut x = base();
    let mut y = base();
    let (x1, x2) = (x.issue_effect_id().unwrap(), x.issue_effect_id().unwrap());
    let (y1, y2) = (y.issue_effect_id().unwrap(), y.issue_effect_id().unwrap());
    assert_eq!((x1, x2), (y1, y2));
    x.effects_mut().register(x1, e1.clone()).unwrap();
    x.effects_mut().register(x2, e2.clone()).unwrap();
    y.effects_mut().register(y2, e2).unwrap();
    y.effects_mut().register(y1, e1).unwrap();
    // Two ETB links made in opposite orders.
    let (x3, x4) = (x.issue_effect_id().unwrap(), x.issue_effect_id().unwrap());
    let (y3, y4) = (y.issue_effect_id().unwrap(), y.issue_effect_id().unwrap());
    x.link_effect_generator(x3, GeneratingAbility::new(oid(3), own_ability(3)))
        .unwrap();
    x.link_effect_generator(x4, GeneratingAbility::new(oid(4), own_ability(4)))
        .unwrap();
    y.link_effect_generator(y4, GeneratingAbility::new(oid(4), own_ability(4)))
        .unwrap();
    y.link_effect_generator(y3, GeneratingAbility::new(oid(3), own_ability(3)))
        .unwrap();
    // Counters stamped on two holders in opposite orders, then each holder re-stamped so the
    // final timestamps agree: x stamps (o1, o2), y stamps (o2, o1), both then stamp o1 and o2
    // again in the same order.
    x.stamp_counters(CounterHolder::Object(oid(1)), "+1/+1")
        .unwrap();
    x.stamp_counters(CounterHolder::Object(oid(2)), "+1/+1")
        .unwrap();
    y.stamp_counters(CounterHolder::Object(oid(2)), "+1/+1")
        .unwrap();
    y.stamp_counters(CounterHolder::Object(oid(1)), "+1/+1")
        .unwrap();
    for s in [&mut x, &mut y] {
        s.stamp_counters(CounterHolder::Object(oid(1)), "+1/+1")
            .unwrap();
        s.stamp_counters(CounterHolder::Object(oid(2)), "+1/+1")
            .unwrap();
    }
    assert_eq!(x, y);
    assert_eq!(x.state_hash(), y.state_hash());
    assert_eq!(x.serialize().unwrap(), y.serialize().unwrap());

    // An effect registered and removed again leaves only the id advance behind.
    let mut z = base();
    let id = z
        .register_effect(
            resolved(
                ts(1),
                Layer::L5,
                &[1],
                LayerModification::AddColors(ColorBitmask::RED),
            ),
            None,
        )
        .unwrap();
    assert!(z.remove_effect(id));
    let mut w = base();
    w.issue_effect_id().unwrap();
    assert_eq!(z, w);
    assert_eq!(z.state_hash(), w.state_hash());
}

// REQ-NFR-006, REQ-CORE-001 (T1d): the hash does not depend on structural sharing: a clone and
// a state rebuilt through serialize/deserialize hash equal.
#[test]
fn hash_is_independent_of_sharing() {
    let s = rich_state();
    let c = s.clone();
    assert_eq!(c.state_hash(), s.state_hash());
    let d = GameState::deserialize(&s.serialize().unwrap()).unwrap();
    assert_eq!(d.state_hash(), s.state_hash());
    assert_eq!(
        d.merkle_tree().full_root_digest(),
        s.merkle_tree().full_root_digest()
    );
    // Mutating the clone does not move the original's hash.
    let before = s.state_hash();
    let mut m = s.clone();
    m.stamp_object(oid(1), StampReason::Attached).unwrap();
    assert_eq!(s.state_hash(), before);
    assert_ne!(m.state_hash(), before);
}

/// Every single-field variant of [`template`] for the mutation-sensitivity test.
fn variants() -> Vec<(&'static str, ContinuousEffect)> {
    let t = template();
    let with = |f: &dyn Fn(&mut ContinuousEffect)| {
        let mut e = t.clone();
        f(&mut e);
        e
    };
    let pt7 = |layer: Layer| {
        with(&|e: &mut ContinuousEffect| {
            e.layer = layer;
            e.is_cda = true;
            e.modification = LayerModification::SetPt {
                power: Some(1),
                toughness: Some(1),
            };
        })
    };
    let mut v: Vec<(&'static str, ContinuousEffect)> = vec![
        ("template", t.clone()),
        ("source", with(&|e| e.source = Some(oid(2)))),
        ("controller", with(&|e| e.controller = P2)),
        ("timestamp", with(&|e| e.timestamp = ts(3))),
        ("timestamp1", with(&|e| e.timestamp = ts(1))),
        ("layer 7a", pt7(Layer::L7a)),
        ("layer 7b", pt7(Layer::L7b)),
        ("is_cda", with(&|e| e.is_cda = true)),
        (
            "granted_at 1",
            with(&|e| {
                e.origin = EffectOrigin::Static {
                    granted_at: Some(ts(1)),
                }
            }),
        ),
        (
            "granted_at 2",
            with(&|e| {
                e.origin = EffectOrigin::Static {
                    granted_at: Some(ts(2)),
                }
            }),
        ),
        (
            "resolved",
            with(&|e| {
                e.origin = EffectOrigin::Resolved;
                e.source = None;
            }),
        ),
        (
            "resolved with source",
            with(&|e| e.origin = EffectOrigin::Resolved),
        ),
        (
            "selector 2",
            with(&|e| e.affected = Affected::Selector(sel(SEL_ALL))),
        ),
        (
            "objects {1}",
            with(&|e| e.affected = Affected::Objects(objs(&[1]))),
        ),
        (
            "objects {1,2}",
            with(&|e| e.affected = Affected::Objects(objs(&[1, 2]))),
        ),
        (
            "objects {}",
            with(&|e| e.affected = Affected::Objects(objs(&[]))),
        ),
        (
            "add other ability",
            with(&|e| e.modification = LayerModification::AddAbility(ability("test.other"))),
        ),
        (
            "remove all",
            with(&|e| e.modification = LayerModification::RemoveAllAbilities),
        ),
        (
            "remove keyword a",
            with(&|e| e.modification = LayerModification::RemoveKeyword("flying".into())),
        ),
        (
            "remove keyword b",
            with(&|e| e.modification = LayerModification::RemoveKeyword("reach".into())),
        ),
    ];
    let in_layer = |layer: Layer, m: LayerModification| {
        with(&move |e: &mut ContinuousEffect| {
            e.layer = layer;
            e.modification = m.clone();
        })
    };
    let more: Vec<(&'static str, ContinuousEffect)> = vec![
        (
            "copy a",
            in_layer(
                Layer::L1a,
                LayerModification::CopyValues(Box::new(chars(
                    "test.copy_a",
                    &["Creature"],
                    vec![],
                ))),
            ),
        ),
        (
            "copy b",
            in_layer(
                Layer::L1a,
                LayerModification::CopyValues(Box::new(chars(
                    "test.copy_b",
                    &["Creature"],
                    vec![],
                ))),
            ),
        ),
        (
            "face down",
            in_layer(
                Layer::L1b,
                LayerModification::FaceDown(Box::new(chars("", &["Creature"], vec![]))),
            ),
        ),
        (
            "control p1",
            in_layer(Layer::L2, LayerModification::SetController(P1)),
        ),
        (
            "control p2",
            in_layer(Layer::L2, LayerModification::SetController(P2)),
        ),
        (
            "text a",
            in_layer(
                Layer::L3,
                LayerModification::SetText(TextTokenStream(vec![serde_json::json!("test.word_a")])),
            ),
        ),
        (
            "text b",
            in_layer(
                Layer::L3,
                LayerModification::SetText(TextTokenStream(vec![serde_json::json!("test.word_b")])),
            ),
        ),
        (
            "add types",
            in_layer(
                Layer::L4,
                LayerModification::AddTypes {
                    supertypes: set(&[]),
                    card_types: set(&["Artifact"]),
                    subtypes: set(&[]),
                },
            ),
        ),
        (
            "add subtypes",
            in_layer(
                Layer::L4,
                LayerModification::AddTypes {
                    supertypes: set(&[]),
                    card_types: set(&[]),
                    subtypes: set(&["Artifact"]),
                },
            ),
        ),
        (
            "set types",
            in_layer(
                Layer::L4,
                LayerModification::SetTypes {
                    supertypes: None,
                    card_types: Some(set(&["Artifact"])),
                    subtypes: None,
                },
            ),
        ),
        (
            "remove subtypes",
            in_layer(
                Layer::L4,
                LayerModification::RemoveSubtypes(set(&["Artifact"])),
            ),
        ),
        (
            "set colors red",
            in_layer(Layer::L5, LayerModification::SetColors(ColorBitmask::RED)),
        ),
        (
            "set colors blue",
            in_layer(Layer::L5, LayerModification::SetColors(ColorBitmask::BLUE)),
        ),
        (
            "add colors red",
            in_layer(Layer::L5, LayerModification::AddColors(ColorBitmask::RED)),
        ),
        (
            "set pt 1/2",
            in_layer(
                Layer::L7b,
                LayerModification::SetPt {
                    power: Some(1),
                    toughness: Some(2),
                },
            ),
        ),
        (
            "set pt 1/-",
            in_layer(
                Layer::L7b,
                LayerModification::SetPt {
                    power: Some(1),
                    toughness: None,
                },
            ),
        ),
        (
            "modify 1/1",
            in_layer(
                Layer::L7c,
                LayerModification::ModifyPt {
                    power: 1,
                    toughness: 1,
                },
            ),
        ),
        (
            "modify 1/2",
            in_layer(
                Layer::L7c,
                LayerModification::ModifyPt {
                    power: 1,
                    toughness: 2,
                },
            ),
        ),
        (
            "modify 2/1",
            in_layer(
                Layer::L7c,
                LayerModification::ModifyPt {
                    power: 2,
                    toughness: 1,
                },
            ),
        ),
        ("switch", in_layer(Layer::L7d, LayerModification::SwitchPt)),
    ];
    v.extend(more);
    v
}

// REQ-NFR-006, REQ-LAYER-001, REQ-DSL-004 (T1e): a state differing from another in one field
// of one registered effect (source, controller, timestamp, layer, is_cda, origin including
// granted_at, affected variant, modification variant and payload) hashes differently.
#[test]
fn every_effect_field_is_hash_sensitive() {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut effects_leaves: BTreeSet<Vec<u8>> = BTreeSet::new();
    for (name, e) in variants() {
        let s = with_effect(e);
        assert!(
            seen.insert(s.state_hash().to_hex()),
            "variant {name} collides with an earlier variant"
        );
        assert!(
            effects_leaves.insert(leaf(&s, &["effects"]).unwrap()),
            "variant {name}: /effects payload collides"
        );
        // Reproducible.
        assert_eq!(
            s.state_hash(),
            with_effect(variants_named(name)).state_hash()
        );
    }
    // A multi-layer group whose second part is linked (SameAsLayer, CR 613.6) hashes
    // differently from the same group with a locked set in its place.
    let linked = group_state(Affected::SameAsLayer(Layer::L4));
    let locked = group_state(Affected::Objects(objs(&[3])));
    assert_ne!(linked.state_hash(), locked.state_hash());
    assert_ne!(leaf(&linked, &["effects"]), leaf(&locked, &["effects"]));
}

fn variants_named(name: &str) -> ContinuousEffect {
    variants()
        .into_iter()
        .find(|(n, _)| *n == name)
        .map(|(_, e)| e)
        .unwrap()
}

/// `base()` with a two-part resolved group (layer 4 anchor on o3, layer-7b part with
/// `second`) registered through `effects_mut().register_group`.
fn group_state(second: Affected) -> GameState {
    let mut s = base();
    let a = s.issue_effect_id().unwrap();
    let b = s.issue_effect_id().unwrap();
    let t = s.stamp_resolved_effect().unwrap();
    let w = world();
    let snapshot = s.clone();
    let env = GameEnvironment::new(&snapshot, &w);
    let first = effect(
        None,
        P1,
        t,
        Layer::L4,
        false,
        EffectOrigin::Resolved,
        Affected::Objects(objs(&[3])),
        LayerModification::AddTypes {
            supertypes: set(&[]),
            card_types: set(&["Artifact"]),
            subtypes: set(&[]),
        },
    );
    let part = effect(
        None,
        P1,
        t,
        Layer::L7b,
        false,
        EffectOrigin::Resolved,
        second,
        LayerModification::SetPt {
            power: Some(2),
            toughness: Some(2),
        },
    );
    s.effects_mut()
        .register_group(&env, vec![(a, first), (b, part)])
        .unwrap();
    s
}

/// The digest of [`small_state`], computed by running this test against the Implementer's code
/// (see the report); it pins the canonical encoding of the three new leaves.
const GOLDEN_SMALL_STATE: &str = "13e35cf7e24c0de507022934867a4fc1e2c8731c449a1ddd2147de6305bc3381";

// REQ-NFR-006, REQ-CORE-001, REQ-CORE-006 (T1f): ten repeated builds hash identically; the
// small state's digest is pinned.
#[test]
fn repeated_builds_hash_identically() {
    let first = rich_state();
    for _ in 0..10 {
        let again = rich_state();
        assert_eq!(again, first);
        assert_eq!(again.state_hash(), first.state_hash());
        assert_eq!(again.serialize().unwrap(), first.serialize().unwrap());
    }
    let small = small_state();
    for _ in 0..10 {
        assert_eq!(small_state().state_hash(), small.state_hash());
    }
    assert_eq!(small.state_hash().to_hex(), GOLDEN_SMALL_STATE);
}

// ---------------------------------------------------------------------------------------------
// T6: register_effect validation and the layer-7a boundary
// ---------------------------------------------------------------------------------------------

/// Asserts `f` fails on a clone of `s` and leaves it (value, hash, effect count, next effect
/// id) unchanged.
fn assert_rejected(s: &GameState, what: &str, f: impl FnOnce(&mut GameState) -> bool) {
    let mut c = s.clone();
    let failed = f(&mut c);
    assert!(failed, "{what}: must be rejected");
    assert_eq!(&c, s, "{what}: state unchanged");
    assert_eq!(c.state_hash(), s.state_hash(), "{what}: hash unchanged");
    assert_eq!(c.effects().len(), s.effects().len(), "{what}: effect count");
    let mut c2 = c.clone();
    let mut s2 = s.clone();
    assert_eq!(
        c2.issue_effect_id().unwrap(),
        s2.issue_effect_id().unwrap(),
        "{what}: next effect id unchanged"
    );
}

fn set_pt(layer: Layer, is_cda: bool) -> ContinuousEffect {
    effect(
        Some(oid(1)),
        P1,
        ts(1),
        layer,
        is_cda,
        EffectOrigin::Static { granted_at: None },
        Affected::Objects(objs(&[1])),
        LayerModification::SetPt {
            power: Some(1),
            toughness: Some(1),
        },
    )
}

// REQ-LAYER-002, REQ-LAYER-001, CR 613.4a (T6): layer 7a consists only of
// characteristic-defining abilities (SRS 6.2 item 1): a non-CDA 7a effect is rejected by
// `register_effect` with the state unchanged; a CDA 7a effect and a non-CDA 7b effect are
// accepted; `effects_mut().register` still accepts a non-CDA 7a effect (existing behaviour).
#[test]
fn layer_7a_boundary() {
    let s = base();
    let g = || Some(GeneratingAbility::new(oid(1), own_ability(1)));
    assert_rejected(&s, "non-CDA 7a", |c| {
        c.register_effect(set_pt(Layer::L7a, false), g()).is_err()
    });
    let mut ok = s.clone();
    let id = ok.register_effect(set_pt(Layer::L7a, true), g()).unwrap();
    assert_eq!(ok.effects().get(id), Some(&set_pt(Layer::L7a, true)));
    assert_eq!(ok.effect_generator(id), g().as_ref());
    let mut ok7b = s.clone();
    let id7b = ok7b
        .register_effect(set_pt(Layer::L7b, false), g())
        .unwrap();
    assert_eq!(ok7b.effects().get(id7b), Some(&set_pt(Layer::L7b, false)));
    // Resolved non-CDA 7a: also rejected.
    let mut r = set_pt(Layer::L7a, false);
    r.origin = EffectOrigin::Resolved;
    r.source = None;
    assert_rejected(&s, "resolved non-CDA 7a", |c| {
        c.register_effect(r, None).is_err()
    });
    // The registry itself keeps accepting it.
    let mut raw = s.clone();
    let id = raw.issue_effect_id().unwrap();
    raw.effects_mut()
        .register(id, set_pt(Layer::L7a, false))
        .unwrap();
    assert_eq!(raw.effects().len(), 1);
}

// REQ-LAYER-001, REQ-CORE-001, REQ-CORE-002 (T6): every pinned rejection of `register_effect`
// and `link_effect_generator` leaves the state, its hash and the effect-id generator unchanged.
#[test]
fn register_and_link_rejections_leave_state_unchanged() {
    let mut s = base();
    let existing = s
        .register_effect(
            resolved(
                ts(1),
                Layer::L5,
                &[1],
                LayerModification::AddColors(ColorBitmask::RED),
            ),
            None,
        )
        .unwrap();
    let st = |src: u64| {
        static_sel(
            oid(src),
            ts(1),
            Layer::L6,
            SEL_ALL,
            LayerModification::AddAbility(marker()),
        )
    };
    // Static with a source and no generator.
    assert_rejected(&s, "static without generator", |c| {
        c.register_effect(st(1), None).is_err()
    });
    // Generator source differs from the effect source.
    assert_rejected(&s, "generator source mismatch", |c| {
        c.register_effect(st(1), Some(GeneratingAbility::new(oid(2), own_ability(2))))
            .is_err()
    });
    // Generator on a resolved effect.
    assert_rejected(&s, "generator on resolved", |c| {
        c.register_effect(
            resolved(
                ts(1),
                Layer::L6,
                &[1],
                LayerModification::AddAbility(marker()),
            ),
            Some(GeneratingAbility::new(oid(1), own_ability(1))),
        )
        .is_err()
    });
    // Generator on a static effect with no source.
    let mut sourceless = st(1);
    sourceless.source = None;
    assert_rejected(&s, "generator on sourceless static", |c| {
        c.register_effect(
            sourceless,
            Some(GeneratingAbility::new(oid(1), own_ability(1))),
        )
        .is_err()
    });
    // Rejections of ContinuousEffects::register: a modification outside its layer, a
    // SameAsLayer affected set, a card-name word replacement.
    let mismatch = resolved(ts(1), Layer::L5, &[1], LayerModification::SwitchPt);
    assert_rejected(&s, "layer mismatch", |c| {
        c.register_effect(mismatch, None).is_err()
    });
    let mut same = resolved(
        ts(1),
        Layer::L7b,
        &[1],
        LayerModification::SetPt {
            power: Some(1),
            toughness: None,
        },
    );
    same.affected = Affected::SameAsLayer(Layer::L4);
    assert_rejected(&s, "SameAsLayer", |c| {
        c.register_effect(same, None).is_err()
    });
    let card_name = resolved(
        ts(1),
        Layer::L3,
        &[1],
        LayerModification::ReplaceTextWord {
            category: fineprint_dsl::text::RewritableCategory::CardName,
            from: "a".into(),
            to: "b".into(),
        },
    );
    assert_rejected(&s, "card-name word", |c| {
        c.register_effect(card_name, None).is_err()
    });

    // link_effect_generator: an id never issued, an id already linked, a registered
    // continuous effect.
    let g = || GeneratingAbility::new(oid(2), own_ability(2));
    let next = EffectId::new(s.ids().effect.last_issued() + 1);
    assert_rejected(&s, "unissued id", |c| {
        c.link_effect_generator(next, g()).is_err()
    });
    assert_rejected(&s, "registered continuous effect", |c| {
        c.link_effect_generator(existing, g()).is_err()
    });
    let etb = s.issue_effect_id().unwrap();
    s.link_effect_generator(etb, g()).unwrap();
    assert_eq!(s.effect_generator(etb), Some(&g()));
    assert_rejected(&s, "already linked", |c| {
        c.link_effect_generator(etb, GeneratingAbility::new(oid(3), own_ability(3)))
            .is_err()
    });
    assert_eq!(s.effect_generator(existing), None);
}

// REQ-LAYER-001 (T6): `remove_effect` removes the effect and its link and reports whether it
// was registered.
#[test]
fn remove_effect_removes_effect_and_link() {
    let mut s = base();
    let id = s
        .register_effect(
            template(),
            Some(GeneratingAbility::new(oid(1), own_ability(1))),
        )
        .unwrap();
    assert!(s.effects().get(id).is_some());
    assert!(s.effect_generator(id).is_some());
    assert!(s.remove_effect(id));
    assert!(s.effects().get(id).is_none());
    assert_eq!(s.effect_generator(id), None);
    assert!(!s.remove_effect(id));
    assert_eq!(leaf(&s, &["effects"]), Some(vec![0, 0, 0, 0]));
    assert_eq!(leaf(&s, &["effect_generators"]), Some(vec![0, 0, 0, 0]));
}
