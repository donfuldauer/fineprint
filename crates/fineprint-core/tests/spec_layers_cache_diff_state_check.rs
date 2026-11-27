// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The state-hash differential of REQ-LAYER-005 (SRS 6.5), in process: a real `GameState` is
//! driven through every kind of mutation, evaluated through `LayerCache` + `GameEnvironment` and
//! through the uncached `layers::evaluate_all`, and the two state hashes must agree after every
//! step (REQ-NFR-006, REQ-CORE-001). The layer cache is not part of the state, so each step
//! feeds the evaluation back into the state through public API (each player's life and a player
//! counter become functions of the evaluated characteristics of the objects they control): the
//! state hash is then a function of the evaluation and a stale cache changes it. Black-box:
//! written from the FP-148 brief only. Runs in default and in `full-recompute` mode, except
//! the stale-cache sensitivity tests, which are cached-mode only. No file or environment access.
//! Every object and ability is synthetic and neutrally named (R5).

#[path = "spec_layers_state_support.rs"]
mod state_support;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    Affected, CounterHolder, EffectOrigin, Evaluated, Invalidation, LayerCache, LayerModification,
    MatchContext, ObjectBase, StampReason, evaluate_all, evaluated_digest, literal_of,
};
use fineprint_core::state::{GameEnvironment, GameState, GeneratingAbility, ObjectWorld};
use fineprint_dsl::card::ValueExpressionAST;
use fineprint_dsl::expr::ObjectSelectorAST;
use state_support::*;

type Map = BTreeMap<ObjectId, Evaluated>;

/// Selector: the object is tapped (a status read by the world wrapper).
const SEL_TAPPED: u32 = 10;
/// Selector: the object carries the designation.
const SEL_DESIGNATED: u32 = 11;
/// Selector: something is attached to the object.
const SEL_ATTACHED: u32 = 12;
/// Selector: the player value handed to the wrapper is at least 5 (matches every object).
const SEL_PLAYER_VALUE: u32 = 13;

// ---------------------------------------------------------------------------------------------
// the world wrapper
// ---------------------------------------------------------------------------------------------

/// The support `World` plus attachment, status, designation and one player value, all read by
/// `matches` / `attached_here`.
#[derive(Clone, Debug, Default)]
struct Ext {
    world: World,
    attached: BTreeMap<ObjectId, Vec<ObjectId>>,
    tapped: BTreeSet<ObjectId>,
    designated: BTreeSet<ObjectId>,
    player_value: i64,
}

impl ObjectWorld for Ext {
    fn objects(&self) -> Vec<ObjectId> {
        self.world.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.world.base(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.world.counters(id)
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.attached.get(&id).cloned().unwrap_or_default()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        if *selector == sel(SEL_TAPPED) {
            self.tapped.contains(&ctx.candidate)
        } else if *selector == sel(SEL_DESIGNATED) {
            self.designated.contains(&ctx.candidate)
        } else if *selector == sel(SEL_ATTACHED) {
            self.attached
                .get(&ctx.candidate)
                .is_some_and(|v| !v.is_empty())
        } else if *selector == sel(SEL_PLAYER_VALUE) {
            self.player_value >= 5
        } else {
            self.world.matches(selector, ctx)
        }
    }
}

/// The support world with a base power and toughness of 2 on every object.
fn pt_ext() -> Ext {
    let mut world = world();
    for b in world.bases.values_mut() {
        // #232 FP-164: the typed literal `{"LITERAL":2}` (formerly the bare placeholder `2`).
        b.characteristics.power = Some(ValueExpressionAST::Literal(2));
        b.characteristics.toughness = Some(ValueExpressionAST::Literal(2));
    }
    Ext {
        world,
        ..Ext::default()
    }
}

// ---------------------------------------------------------------------------------------------
// feedback and the rig
// ---------------------------------------------------------------------------------------------

/// Writes each player's life and the player counter `test.power` as functions of the evaluation
/// of the objects that player controls (public API only).
fn feedback(s: &mut GameState, m: &Map) {
    let players: Vec<PlayerId> = s.players().map(|p| p.id()).collect();
    for p in players {
        let sub: Map = m
            .iter()
            .filter(|(_, e)| e.controller == Some(p))
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let hex = evaluated_digest(&sub).to_hex();
        let life = i64::from_str_radix(&hex[..12], 16).expect("lowercase hex");
        let power: i64 = sub
            .values()
            .filter_map(|e| e.characteristics.power.as_ref().and_then(literal_of))
            .sum();
        let mut pm = s.player_mut(p).expect("player exists");
        pm.set_life(life);
        pm.set_counter("test.power", power);
    }
}

/// A cached state and a reference state kept in lock step, one cache, one world.
struct Rig {
    cached: GameState,
    reference: GameState,
    cache: LayerCache,
    w: Ext,
    ids: Vec<EffectId>,
    hashes: Vec<String>,
    last: Map,
}

impl Rig {
    /// A rig over `w` with the support `base()` state and no effects; one initial step.
    fn new(w: Ext) -> Rig {
        let mut r = Rig {
            cached: base(),
            reference: base(),
            cache: LayerCache::new(),
            w,
            ids: Vec::new(),
            hashes: Vec::new(),
            last: Map::new(),
        };
        r.step(&[]);
        r
    }

    /// The scenario of most tests: effects in layers 6, 7b and 7c, one selector effect per
    /// world-wrapper input; one initial step.
    fn standard() -> Rig {
        let mut r = Rig {
            cached: base(),
            reference: base(),
            cache: LayerCache::new(),
            w: pt_ext(),
            ids: Vec::new(),
            hashes: Vec::new(),
            last: Map::new(),
        };
        r.reg_resolved(
            Layer::L7b,
            &[1, 2, 3],
            LayerModification::SetPt {
                power: Some(4),
                toughness: Some(4),
            },
        );
        let pt = |p, t| LayerModification::ModifyPt {
            power: p,
            toughness: t,
        };
        r.reg_static(2, Layer::L7c, SEL_TAPPED, pt(1, 1));
        r.reg_static(
            3,
            Layer::L6,
            SEL_DESIGNATED,
            LayerModification::AddAbility(ability("test.designated")),
        );
        r.reg_static(4, Layer::L7c, SEL_ATTACHED, pt(3, 0));
        r.reg_static(1, Layer::L7c, SEL_PLAYER_VALUE, pt(5, 0));
        r.step(&[]);
        r
    }

    fn both(&mut self, f: impl Fn(&mut GameState)) {
        f(&mut self.cached);
        f(&mut self.reference);
    }

    fn reg_resolved(&mut self, layer: Layer, objects: &[u64], m: LayerModification) -> EffectId {
        let mut out = None;
        for s in [&mut self.cached, &mut self.reference] {
            let t = s.stamp_resolved_effect().unwrap();
            out = Some(
                s.register_effect(resolved(t, layer, objects, m.clone()), None)
                    .unwrap(),
            );
        }
        let id = out.unwrap();
        self.ids.push(id);
        id
    }

    fn reg_static(&mut self, source: u64, layer: Layer, slot: u32, m: LayerModification) {
        let mut out = None;
        for s in [&mut self.cached, &mut self.reference] {
            let t = s
                .stamp_object(oid(source), StampReason::EntersZone)
                .unwrap();
            out = Some(
                s.register_effect(
                    static_sel(oid(source), t, layer, slot, m.clone()),
                    Some(GeneratingAbility::new(oid(source), own_ability(source))),
                )
                .unwrap(),
            );
        }
        self.ids.push(out.unwrap());
    }

    /// A two-part group: a layer-4 add on o3 and a layer-7b set on the same set.
    fn reg_group(&mut self) {
        let mut out = Vec::new();
        for s in [&mut self.cached, &mut self.reference] {
            let t = s.stamp_resolved_effect().unwrap();
            let snapshot = s.clone();
            let env = GameEnvironment::new(&snapshot, &self.w);
            let parts = vec![
                (
                    resolved(
                        t,
                        Layer::L4,
                        &[3],
                        LayerModification::AddTypes {
                            supertypes: set(&[]),
                            card_types: set(&["Artifact"]),
                            subtypes: set(&[]),
                        },
                    ),
                    None,
                ),
                (
                    effect(
                        None,
                        P1,
                        t,
                        Layer::L7b,
                        false,
                        EffectOrigin::Resolved,
                        Affected::SameAsLayer(Layer::L4),
                        LayerModification::SetPt {
                            power: Some(9),
                            toughness: Some(9),
                        },
                    ),
                    None,
                ),
            ];
            out = s.register_effect_group(&env, parts).unwrap();
        }
        self.ids.extend(out);
    }

    /// Declares `declare`, evaluates through the cache and the uncached reference, feeds each
    /// back into its state and returns the two evaluated maps and the two state hashes.
    fn step_raw(&mut self, declare: &[Invalidation]) -> (Map, Map, String, String) {
        for d in declare {
            self.cache.invalidate(*d);
        }
        let mc = {
            let env = GameEnvironment::new(&self.cached, &self.w);
            self.cache
                .evaluate_all(self.cached.effects(), &env)
                .unwrap()
        };
        let mr = {
            let env = GameEnvironment::new(&self.reference, &self.w);
            evaluate_all(self.reference.effects(), &env).unwrap()
        };
        feedback(&mut self.cached, &mc);
        feedback(&mut self.reference, &mr);
        let hc = self.cached.state_hash().to_hex();
        let hr = self.reference.state_hash().to_hex();
        self.last = mc.clone();
        self.hashes.push(hc.clone());
        (mc, mr, hc, hr)
    }

    /// As [`Rig::step_raw`], requiring cached == reference for maps and hashes.
    fn step(&mut self, declare: &[Invalidation]) {
        let (mc, mr, hc, hr) = self.step_raw(declare);
        assert_eq!(
            mc,
            mr,
            "evaluated maps differ after step {}",
            self.hashes.len()
        );
        assert_eq!(
            hc,
            hr,
            "state hashes differ after step {}",
            self.hashes.len()
        );
    }

    /// The last step's hash differs from the one before it.
    fn assert_hash_changed(&self, what: &str) {
        let n = self.hashes.len();
        assert!(n >= 2, "need two steps");
        assert_ne!(
            self.hashes[n - 1],
            self.hashes[n - 2],
            "hash unchanged by {what}"
        );
    }
}

use Invalidation as Inv;

/// CR 613.7a (as cited by the layer-timestamp procedures): re-stamps `source` from its
/// previous object timestamp (read BEFORE the new stamp) to a new one and checks the registry
/// changed accordingly: every printed static effect of `source` carries exactly the new object
/// timestamp, and every granted effect that was later than the previous object timestamp has a
/// fresh timestamp later than the new object stamp, in its previous relative order. Returns the
/// number of granted effects that were restamped.
fn restamp_checked(s: &mut GameState, source: ObjectId) -> usize {
    let old = s
        .timestamps()
        .object_timestamp(source)
        .expect("the source holds an object timestamp");
    let mut granted_before: Vec<(Timestamp, EffectId)> = s
        .effects()
        .iter()
        .filter(|(_, e)| {
            e.source == Some(source)
                && matches!(
                    e.origin,
                    EffectOrigin::Static {
                        granted_at: Some(_)
                    }
                )
                && e.timestamp > old
        })
        .map(|(id, e)| (e.timestamp, id))
        .collect();
    granted_before.sort_unstable();
    let new = s
        .stamp_object(source, StampReason::TurnedFaceUpOrDown)
        .expect("stamp");
    assert!(new > old);
    s.restamp_static_effects(source, old, new).expect("restamp");
    let mut printed = 0;
    for (_, e) in s.effects().iter() {
        if e.source == Some(source) && e.origin == (EffectOrigin::Static { granted_at: None }) {
            assert_eq!(
                e.timestamp, new,
                "a printed effect takes the new object stamp"
            );
            printed += 1;
        }
    }
    assert!(printed > 0, "the source has printed static effects");
    let mut after: Vec<Timestamp> = Vec::new();
    for (_, id) in &granted_before {
        let e = s.effects().get(*id).expect("the granted effect remains");
        assert!(
            e.timestamp > new,
            "a granted effect is later than the new source stamp"
        );
        after.push(e.timestamp);
    }
    assert!(
        after.windows(2).all(|w| w[0] < w[1]),
        "relative order is kept"
    );
    granted_before.len()
}

// ---------------------------------------------------------------------------------------------
// requirement 1: one test per mutation kind
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005, REQ-NFR-006: effect registration, single and group, declared or detected.
#[test]
fn req_layer_005_effect_registration_state_hash_matches() {
    let mut r = Rig::standard();
    r.reg_resolved(
        Layer::L6,
        &[1, 2],
        LayerModification::AddAbility(ability("test.granted")),
    );
    r.step(&[]); // detected by the cache itself
    r.assert_hash_changed("registration");
    r.reg_resolved(
        Layer::L7c,
        &[1, 4],
        LayerModification::ModifyPt {
            power: 2,
            toughness: 2,
        },
    );
    r.step(&[Inv::EffectRegistration]);
    r.assert_hash_changed("registration");
    r.reg_group();
    r.step(&[]);
    r.assert_hash_changed("group registration");
    r.reg_group();
    r.step(&[Inv::EffectRegistration]);
    r.assert_hash_changed("group registration");
}

/// REQ-LAYER-005, REQ-NFR-006: effect removal, with and without a declared expiry.
#[test]
fn req_layer_005_effect_removal_state_hash_matches() {
    let mut r = Rig::standard();
    let a = r.reg_resolved(
        Layer::L7c,
        &[1, 2],
        LayerModification::ModifyPt {
            power: 7,
            toughness: 7,
        },
    );
    let b = r.reg_resolved(
        Layer::L6,
        &[3],
        LayerModification::AddAbility(ability("test.temp")),
    );
    r.step(&[]);
    r.both(|s| assert!(s.remove_effect(a)));
    r.step(&[]); // removal detected by the cache
    r.assert_hash_changed("removal");
    r.both(|s| assert!(s.remove_effect(b)));
    r.step(&[Inv::EffectExpiry]);
    r.assert_hash_changed("removal");
    // removing a static continuous effect (and its link) too
    let st = r.ids[1];
    r.both(|s| assert!(s.remove_effect(st)));
    r.step(&[Inv::EffectExpiry]);
    r.assert_hash_changed("removal of a static effect");
}

/// REQ-LAYER-005, REQ-CORE-002 (CR 613.7a as cited by `restamp_static_effects`): object
/// timestamp stamping, re-stamping static effects (the printed effect takes the new object
/// stamp, a later granted effect a fresh one after it), forgetting stamps.
#[test]
fn req_layer_005_object_timestamp_state_hash_matches() {
    let mut r = Rig::standard();
    for n in [1u64, 2, 3] {
        r.both(|s| {
            s.stamp_object(oid(n), StampReason::Attached).unwrap();
        });
        r.step(&[Inv::Timestamp]);
        r.assert_hash_changed("stamp_object");
    }
    // A granted effect of object 2, later than object 2's current timestamp.
    r.both(|s| {
        let g = s.stamp_resolved_effect().unwrap();
        let mut granted = static_sel(
            oid(2),
            g,
            Layer::L7c,
            SEL_TAPPED,
            LayerModification::ModifyPt {
                power: 0,
                toughness: 1,
            },
        );
        granted.origin = EffectOrigin::Static {
            granted_at: Some(g),
        };
        s.register_effect(
            granted,
            Some(GeneratingAbility::new(
                oid(2),
                ability("test.o2_granted_static"),
            )),
        )
        .unwrap();
    });
    r.step(&[Inv::EffectRegistration]);
    r.both(|s| {
        assert_eq!(restamp_checked(s, oid(2)), 1, "the granted effect restamps");
    });
    r.step(&[Inv::Timestamp]);
    r.assert_hash_changed("restamp");
    r.both(|s| s.forget_object_stamps(oid(1)));
    r.step(&[Inv::Timestamp]);
    r.assert_hash_changed("forget_object_stamps");
}

/// REQ-LAYER-005: counter stamping and clearing, and a world counter change.
#[test]
fn req_layer_005_counter_state_hash_matches() {
    let mut r = Rig::standard();
    r.both(|s| {
        s.stamp_counters(CounterHolder::Object(oid(1)), "+1/+1")
            .unwrap();
    });
    r.w.world.counter(oid(1), "+1/+1", 2);
    r.step(&[Inv::Timestamp, Inv::CounterChange]);
    r.assert_hash_changed("stamp_counters");
    // a world counter change alone
    r.w.world.counter(oid(1), "+1/+1", 3);
    r.step(&[Inv::CounterChange]);
    r.w.world.counter(oid(2), "-1/-1", 1);
    r.both(|s| {
        s.stamp_counters(CounterHolder::Object(oid(2)), "-1/-1")
            .unwrap();
    });
    r.step(&[Inv::Timestamp, Inv::CounterChange]);
    r.assert_hash_changed("stamp_counters");
    // clearing
    r.both(|s| s.clear_counter_stamp(CounterHolder::Object(oid(1)), "+1/+1"));
    r.w.world.counters.remove(&oid(1));
    r.step(&[Inv::Timestamp, Inv::CounterChange]);
    r.assert_hash_changed("clear_counter_stamp");
    // a player's counters
    r.both(|s| {
        s.stamp_counters(CounterHolder::Player(P2), "test.player_counter")
            .unwrap();
    });
    r.step(&[Inv::Timestamp]);
    r.assert_hash_changed("player counter stamp");
}

/// REQ-LAYER-005: zone changes (an object enters, changes zone, leaves).
#[test]
fn req_layer_005_zone_change_state_hash_matches() {
    let mut r = Rig::standard();
    r.both(|s| {
        s.issue_object_id().unwrap();
    });
    r.w.world
        .put_off(oid(5), chars("test.object_5", &["Creature"], vec![]), P2);
    r.step(&[Inv::ZoneChange]);
    r.w.world.bases.get_mut(&oid(5)).unwrap().on_battlefield = true;
    r.both(|s| {
        s.stamp_object(oid(5), StampReason::EntersZone).unwrap();
    });
    r.step(&[Inv::ZoneChange, Inv::Timestamp]);
    r.assert_hash_changed("stamp of the entering object");
    r.w.world.bases.get_mut(&oid(2)).unwrap().on_battlefield = false;
    r.step(&[Inv::ZoneChange]);
    r.w.world.bases.remove(&oid(5));
    r.both(|s| s.forget_object_stamps(oid(5)));
    r.step(&[Inv::ZoneChange, Inv::Timestamp]);
}

/// REQ-LAYER-005: control change.
#[test]
fn req_layer_005_control_change_state_hash_matches() {
    let mut r = Rig::standard();
    let before = r.hashes.last().cloned().unwrap();
    r.w.world.bases.get_mut(&oid(1)).unwrap().controller = Some(P2);
    r.step(&[Inv::ControlChange]);
    assert_ne!(
        before,
        *r.hashes.last().unwrap(),
        "a control change reaches the hash through the feedback"
    );
    r.w.world.bases.get_mut(&oid(3)).unwrap().controller = Some(P2);
    r.step(&[Inv::ControlChange]);
    r.w.world.bases.get_mut(&oid(1)).unwrap().controller = Some(P1);
    r.step(&[Inv::ControlChange]);
}

/// REQ-LAYER-005: attachment change.
#[test]
fn req_layer_005_attachment_change_state_hash_matches() {
    let mut r = Rig::standard();
    r.w.attached.insert(oid(1), vec![oid(2)]);
    r.step(&[Inv::Attachment]);
    r.both(|s| {
        s.stamp_object(oid(2), StampReason::Attached).unwrap();
    });
    r.w.attached.insert(oid(3), vec![oid(4)]);
    r.step(&[Inv::Attachment, Inv::Timestamp]);
    r.assert_hash_changed("attachment stamp");
    r.w.attached.remove(&oid(1));
    r.step(&[Inv::Attachment]);
}

/// REQ-LAYER-005: status change.
#[test]
fn req_layer_005_status_change_state_hash_matches() {
    let mut r = Rig::standard();
    r.w.tapped.insert(oid(1));
    r.step(&[Inv::Status]);
    r.w.tapped.insert(oid(3));
    r.step(&[Inv::Status]);
    r.w.tapped.remove(&oid(1));
    r.step(&[Inv::Status]);
}

/// REQ-LAYER-005: designation change.
#[test]
fn req_layer_005_designation_change_state_hash_matches() {
    let mut r = Rig::standard();
    r.w.designated.insert(oid(1));
    r.step(&[Inv::Designation]);
    r.w.designated.insert(oid(4));
    r.step(&[Inv::Designation]);
    r.w.designated.remove(&oid(1));
    r.step(&[Inv::Designation]);
}

/// REQ-LAYER-005: a player value read by the world wrapper changes.
#[test]
fn req_layer_005_player_value_change_state_hash_matches() {
    let mut r = Rig::standard();
    r.w.player_value = 9;
    r.step(&[Inv::PlayerValue]);
    r.w.player_value = 1;
    r.step(&[Inv::PlayerValue]);
    // a write to the state's own player value and the turn, declared as the table says
    r.both(|s| s.set_turn_number(12));
    r.step(&[Inv::PlayerValue]);
    r.assert_hash_changed("set_turn_number");
    r.both(|s| s.set_active_player(Some(P2)).unwrap());
    r.step(&[Inv::PlayerValue]);
    r.assert_hash_changed("set_active_player");
}

// ---------------------------------------------------------------------------------------------
// requirement 2: ordering
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005: a layer-6 remove-all and a layer-6 grant, in both timestamp orders. Both orders
/// agree cached == reference at every step. The two are resolved effects with locked-in sets, so
/// the later timestamp applies last: remove-then-grant leaves the marker, grant-then-remove
/// leaves no abilities.
#[test]
fn req_layer_005_layer6_pair_both_timestamp_orders() {
    let remove = LayerModification::RemoveAllAbilities;
    let grant = LayerModification::AddAbility(marker());
    let mut results = Vec::new();
    for remove_first in [true, false] {
        let mut r = Rig::new(pt_ext());
        if remove_first {
            r.reg_resolved(Layer::L6, &[1, 2], remove.clone());
            r.step(&[]);
            r.reg_resolved(Layer::L6, &[1, 2], grant.clone());
        } else {
            r.reg_resolved(Layer::L6, &[1, 2], grant.clone());
            r.step(&[]);
            r.reg_resolved(Layer::L6, &[1, 2], remove.clone());
        }
        r.step(&[]);
        r.w.tapped.insert(oid(1));
        r.step(&[Inv::Status]);
        let has = has_ability(&r.last[&oid(1)].characteristics, &marker());
        results.push((has, r.hashes.last().cloned().unwrap(), r.last.clone()));
    }
    assert!(results[0].0, "remove then grant: the marker is present");
    assert!(!results[1].0, "grant then remove: no marker");
    assert_ne!(
        results[0].2, results[1].2,
        "the two orders evaluate differently"
    );
    assert_ne!(
        results[0].1, results[1].1,
        "the two orders hash differently"
    );
}

/// REQ-LAYER-005: two layer-7b sets on one object in both timestamp orders: the later wins.
#[test]
fn req_layer_005_layer7b_pair_both_timestamp_orders() {
    let small = LayerModification::SetPt {
        power: Some(1),
        toughness: Some(1),
    };
    let large = LayerModification::SetPt {
        power: Some(5),
        toughness: Some(5),
    };
    let mut powers = Vec::new();
    let mut hashes = Vec::new();
    for small_first in [true, false] {
        let mut r = Rig::new(pt_ext());
        let (first, second) = if small_first {
            (small.clone(), large.clone())
        } else {
            (large.clone(), small.clone())
        };
        r.reg_resolved(Layer::L7b, &[1], first);
        r.step(&[]);
        r.reg_resolved(Layer::L7b, &[1], second);
        r.step(&[]);
        r.w.tapped.insert(oid(2));
        r.step(&[Inv::Status]);
        powers.push(
            r.last[&oid(1)]
                .characteristics
                .power
                .as_ref()
                .and_then(literal_of),
        );
        hashes.push(r.hashes.last().cloned().unwrap());
    }
    assert_eq!(
        powers[0],
        Some(5),
        "small then large: the later (large) set applies last"
    );
    assert_eq!(
        powers[1],
        Some(1),
        "large then small: the later (small) set applies last"
    );
    assert_ne!(hashes[0], hashes[1]);
}

// ---------------------------------------------------------------------------------------------
// requirement 3: evaluation never changes the hash by itself
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005, REQ-NFR-006: cold, warm and post-invalidate evaluations leave the hash alone.
#[test]
fn req_layer_005_evaluation_does_not_change_state_hash() {
    let r = Rig::standard();
    let state = r.cached.clone();
    let h0 = state.state_hash();
    let mut cache = LayerCache::new();
    let eval = |cache: &mut LayerCache| {
        let env = GameEnvironment::new(&state, &r.w);
        cache.evaluate_all(state.effects(), &env).unwrap()
    };
    let cold = eval(&mut cache);
    assert_eq!(state.state_hash(), h0, "cold evaluation");
    let warm = eval(&mut cache);
    assert_eq!(state.state_hash(), h0, "warm evaluation");
    cache.invalidate(Inv::ZoneChange);
    let again = eval(&mut cache);
    assert_eq!(state.state_hash(), h0, "evaluation after invalidate");
    assert_eq!(cold, warm);
    assert_eq!(cold, again);
    let env = GameEnvironment::new(&state, &r.w);
    assert_eq!(evaluate_all(state.effects(), &env).unwrap(), cold);
    assert_eq!(state.state_hash(), h0, "uncached evaluation");
}

// ---------------------------------------------------------------------------------------------
// requirement 4: determinism and hash movement
// ---------------------------------------------------------------------------------------------

/// A fixed script touching hashed components; returns the hash after each step.
fn script() -> Vec<String> {
    let mut r = Rig::standard();
    let e = r.reg_resolved(
        Layer::L7c,
        &[2, 3],
        LayerModification::ModifyPt {
            power: 1,
            toughness: 1,
        },
    );
    r.step(&[]);
    r.assert_hash_changed("registration");
    r.both(|s| {
        s.stamp_object(oid(4), StampReason::EntersZone).unwrap();
    });
    r.step(&[Inv::Timestamp]);
    r.assert_hash_changed("stamp_object");
    r.both(|s| {
        s.stamp_counters(CounterHolder::Object(oid(3)), "+1/+1")
            .unwrap();
    });
    r.w.world.counter(oid(3), "+1/+1", 1);
    r.step(&[Inv::Timestamp, Inv::CounterChange]);
    r.assert_hash_changed("stamp_counters");
    r.both(|s| {
        s.player_mut(P2).unwrap().set_counter("test.misc", 5);
    });
    r.step(&[Inv::PlayerValue]);
    r.assert_hash_changed("player write");
    r.both(|s| assert!(s.remove_effect(e)));
    r.step(&[]);
    r.assert_hash_changed("removal");
    r.w.world.bases.get_mut(&oid(2)).unwrap().controller = Some(P2);
    r.step(&[Inv::ControlChange]);
    r.hashes
}

/// REQ-NFR-006, REQ-CORE-001: the same scenario built twice gives identical hashes at every
/// step; each hashed-component write moves the hash (asserted inside `script`).
#[test]
fn req_nfr_006_req_core_001_state_hash_deterministic() {
    let a = script();
    let b = script();
    assert_eq!(a.len(), b.len());
    assert_eq!(a, b);
    let distinct: BTreeSet<&String> = a.iter().collect();
    assert!(distinct.len() >= 6, "the script moves the hash");
}

// ---------------------------------------------------------------------------------------------
// requirement 5: stale-cache sensitivity
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005: a control change WITHOUT its invalidation leaves the cached hash different from
/// the reference one (cached mode only).
#[cfg(not(feature = "full-recompute"))]
#[test]
fn req_layer_005_stale_control_change_is_detected() {
    let mut r = Rig::standard();
    r.w.world.bases.get_mut(&oid(1)).unwrap().controller = Some(P2);
    let (mc, mr, hc, hr) = r.step_raw(&[]);
    assert_ne!(mc, mr, "the stale cache serves the old evaluation");
    assert_ne!(hc, hr, "the stale cache changes the state hash");
}

/// REQ-LAYER-005: the same control change WITH ControlChange declared: hashes equal.
#[test]
fn req_layer_005_declared_control_change_hashes_equal() {
    let mut r = Rig::standard();
    r.w.world.bases.get_mut(&oid(1)).unwrap().controller = Some(P2);
    let (mc, mr, hc, hr) = r.step_raw(&[Inv::ControlChange]);
    assert_eq!(mc, mr);
    assert_eq!(hc, hr);
}

/// REQ-LAYER-005: a world counter change WITH CounterChange declared: hashes equal.
#[test]
fn req_layer_005_declared_counter_change_hashes_equal() {
    let mut r = Rig::standard();
    r.w.world.counter(oid(2), "+1/+1", 3);
    let (mc, mr, hc, hr) = r.step_raw(&[Inv::CounterChange]);
    assert_eq!(mc, mr);
    assert_eq!(hc, hr);
}

/// REQ-LAYER-005: a player value read by the world wrapper changes WITHOUT PlayerValue (cached
/// mode only): the state hashes differ.
#[cfg(not(feature = "full-recompute"))]
#[test]
fn req_layer_005_stale_player_value_is_detected() {
    let mut r = Rig::standard();
    r.w.player_value = 9;
    let (mc, mr, hc, hr) = r.step_raw(&[]);
    assert_ne!(mc, mr);
    assert_ne!(hc, hr, "the stale cache changes the state hash");
}

/// REQ-LAYER-005: the same player value change WITH PlayerValue declared: hashes equal.
#[test]
fn req_layer_005_declared_player_value_hashes_equal() {
    let mut r = Rig::standard();
    r.w.player_value = 9;
    let (mc, mr, hc, hr) = r.step_raw(&[Inv::PlayerValue]);
    assert_eq!(mc, mr);
    assert_eq!(hc, hr);
}

// ---------------------------------------------------------------------------------------------
// requirement 6: property test
// ---------------------------------------------------------------------------------------------

/// One random mutation of `r`, declaring the table's invalidations; returns them.
fn mutate(r: &mut Rig, rng: &mut Rng, step_no: u64) -> Vec<Invalidation> {
    let a = 1 + rng.below(4);
    let b = 1 + rng.below(4);
    match rng.below(14) {
        0 => {
            let (layer, m) = match rng.below(3) {
                0 => (
                    Layer::L6,
                    LayerModification::AddAbility(ability(&format!("test.p{step_no}"))),
                ),
                1 => (
                    Layer::L7c,
                    LayerModification::ModifyPt {
                        power: rng.below(5) as i64 - 2,
                        toughness: rng.below(5) as i64 - 2,
                    },
                ),
                _ => (
                    Layer::L7b,
                    LayerModification::SetPt {
                        power: Some(rng.below(6) as i64),
                        toughness: Some(rng.below(6) as i64),
                    },
                ),
            };
            r.reg_resolved(layer, &[a, b], m);
            Vec::new()
        }
        1 => {
            if r.ids.is_empty() {
                return Vec::new();
            }
            let i = rng.below(r.ids.len() as u64) as usize;
            let id = r.ids.remove(i);
            r.both(|s| {
                s.remove_effect(id);
            });
            if rng.below(2) == 0 {
                vec![Inv::EffectExpiry]
            } else {
                Vec::new()
            }
        }
        2 => {
            r.both(|s| {
                s.stamp_object(oid(a), StampReason::EntersZone).unwrap();
            });
            vec![Inv::Timestamp]
        }
        3 => {
            r.both(|s| {
                s.stamp_counters(CounterHolder::Object(oid(a)), "+1/+1")
                    .unwrap();
            });
            r.w.world.counter(oid(a), "+1/+1", 1 + rng.below(3) as i64);
            vec![Inv::Timestamp, Inv::CounterChange]
        }
        4 => {
            r.both(|s| s.clear_counter_stamp(CounterHolder::Object(oid(a)), "+1/+1"));
            r.w.world.counters.remove(&oid(a));
            vec![Inv::Timestamp, Inv::CounterChange]
        }
        5 => {
            r.w.world.counter(oid(a), "-1/-1", 1 + rng.below(3) as i64);
            vec![Inv::CounterChange]
        }
        6 => {
            let on = &mut r.w.world.bases.get_mut(&oid(a)).unwrap().on_battlefield;
            *on = !*on;
            vec![Inv::ZoneChange]
        }
        7 => {
            r.w.world.bases.get_mut(&oid(a)).unwrap().controller =
                Some(if rng.below(2) == 0 { P1 } else { P2 });
            vec![Inv::ControlChange]
        }
        8 => {
            if r.w.attached.remove(&oid(a)).is_none() {
                r.w.attached.insert(oid(a), vec![oid(b)]);
            }
            vec![Inv::Attachment]
        }
        9 => {
            if !r.w.tapped.remove(&oid(a)) {
                r.w.tapped.insert(oid(a));
            }
            vec![Inv::Status]
        }
        10 => {
            if !r.w.designated.remove(&oid(a)) {
                r.w.designated.insert(oid(a));
            }
            vec![Inv::Designation]
        }
        11 => {
            r.w.player_value = rng.below(10) as i64;
            vec![Inv::PlayerValue]
        }
        12 => {
            r.both(|s| s.forget_object_stamps(oid(a)));
            vec![Inv::Timestamp]
        }
        _ => Vec::new(),
    }
}

/// REQ-LAYER-005, REQ-CORE-001: eight fixed seeds, thirty random mutations each, cached state
/// hash == reference state hash after every step; the whole run is deterministic.
#[test]
fn req_layer_005_req_core_001_property_random_mutations() {
    let run = |seed: u64| -> Vec<String> {
        let mut rng = Rng::new(seed);
        let mut r = Rig::standard();
        for step_no in 0..30 {
            let declare = mutate(&mut r, &mut rng, step_no);
            r.step(&declare);
        }
        r.hashes
    };
    for seed in [1u64, 2, 3, 5, 8, 13, 21, 0xDEAD_BEEF] {
        let first = run(seed);
        assert_eq!(first.len(), 31, "initial step plus thirty");
        assert_eq!(first, run(seed), "seed {seed} is reproducible");
    }
}

// ---------------------------------------------------------------------------------------------
// requirement 7: warm cache
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005: with nothing declared and nothing changed, a second evaluation gives the same
/// map; in cached mode `CacheStats::hits` grows.
#[test]
fn req_layer_005_warm_cache_second_evaluation_equal() {
    let r = Rig::standard();
    let env = GameEnvironment::new(&r.cached, &r.w);
    let mut cache = LayerCache::new();
    let first = cache.evaluate_all(r.cached.effects(), &env).unwrap();
    let hits_before = cache.stats().hits;
    let second = cache.evaluate_all(r.cached.effects(), &env).unwrap();
    assert_eq!(first, second);
    assert_eq!(first, evaluate_all(r.cached.effects(), &env).unwrap());
    #[cfg(not(feature = "full-recompute"))]
    assert!(
        cache.stats().hits > hits_before,
        "the second evaluation is served from the cache"
    );
    #[cfg(feature = "full-recompute")]
    let _ = hits_before;
}
