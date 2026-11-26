// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The cached versus full-recompute differential on `GameState` state hashes (REQ-LAYER-005,
//! SRS 6.5; REQ-CORE-001, REQ-NFR-006). The CI job `differential` runs this target once in the
//! default (cached) mode and once with `--features full-recompute`, each time with
//! `FINEPRINT_DIFF_DIR` set, and requires the two directories of `<scenario>.statehash` files
//! (with the `.digest` files of `spec_layers_cache_diff`) to be byte-identical.
//!
//! The layer cache is not part of `GameState`, and `GameState::state_hash` hashes only the
//! state's own components, so evaluating through a cache cannot change the hash by itself. Each
//! scenario therefore feeds the evaluated characteristics back into the state after every step
//! (each player's life becomes the total evaluated power of the objects they control, and two
//! player counters the number of evaluated abilities and card types): the final hash is a
//! function of every cached evaluation, and a stale cache yields a different hash.
//!
//! The GameState holds no objects yet, so the world is a test double and the scenarios declare
//! the invalidations of the `state::environment` table themselves. Every object, ability and
//! effect is synthetic and neutrally named (R5). `FINEPRINT_DIFF_DIR` is read only in this file
//! (R3); the tests pass when it is unset.

#[path = "spec_layers_cache_support.rs"]
mod cache_support;
#[path = "spec_layers_state_support.rs"]
mod state_support;

use std::collections::{BTreeMap, BTreeSet};

use cache_support::cda_expr;
use cache_support::support::{add_card_types, creature, kw_ability, literal_of, modify_pt, set_pt};
use fineprint_base::hash::Digest;
use fineprint_base::ids::{EffectId, ObjectId, PlayerId};
use fineprint_core::layers::{
    Affected, CounterHolder, Evaluated, Invalidation, LayerCache, LayerModification, MatchContext,
    ObjectBase, StampReason, evaluate_all,
};
use fineprint_core::objects::Characteristics;
use fineprint_core::state::{GameEnvironment, GameState, GeneratingAbility, ObjectWorld};
use fineprint_dsl::card::ValueExpressionAST;
use fineprint_dsl::expr::ObjectSelectorAST;
use state_support::*;

// ---------------------------------------------------------------------------------------------
// the world double: a thin wrapper reading state-side values
// ---------------------------------------------------------------------------------------------

/// Selector: the candidate is tapped (status).
const SEL_IS_TAPPED: u32 = 101;
/// Selector: the candidate carries the designation (designation).
const SEL_IS_DESIGNATED: u32 = 102;
/// Selector: something is attached to the candidate (attachment).
const SEL_IS_ENCHANTED: u32 = 103;
/// Selector: the candidate's controller has `RANK` counters of at least 10 (player value).
const SEL_RANKED_CONTROLLER: u32 = 104;
/// The player counter that selectors and the CDA read (written through `player_mut`).
const RANK: &str = "test.rank";

/// The mutable synthetic world: the shared `World` plus status, designations and attachments.
#[derive(Clone, Default)]
struct Wrap {
    world: World,
    tapped: BTreeSet<ObjectId>,
    designated: BTreeSet<ObjectId>,
    attached: BTreeMap<ObjectId, Vec<ObjectId>>,
    catalog: BTreeMap<ObjectId, (Characteristics, PlayerId)>,
}

impl Wrap {
    fn add(&mut self, n: u64, c: Characteristics, controller: PlayerId) {
        self.catalog.insert(oid(n), (c.clone(), controller));
        self.world.put(oid(n), c, controller);
    }

    fn set_counter(&mut self, id: ObjectId, kind: &str, amount: i64) {
        if amount > 0 {
            self.world.counter(id, kind, amount);
        } else if let Some(m) = self.world.counters.get_mut(&id) {
            m.remove(kind);
        }
    }

    fn set_controller(&mut self, id: ObjectId, p: PlayerId) {
        if let Some(b) = self.world.bases.get_mut(&id) {
            b.controller = Some(p);
        }
    }

    fn set_on_battlefield(&mut self, id: ObjectId, on: bool) {
        if let Some(b) = self.world.bases.get_mut(&id) {
            b.on_battlefield = on;
        }
    }

    /// An object leaves the object list (true) or returns from the catalog (false).
    fn set_present(&mut self, id: ObjectId, present: bool) {
        if present {
            if let Some((c, p)) = self.catalog.get(&id).cloned() {
                self.world.put(id, c, p);
            }
        } else {
            self.world.bases.remove(&id);
        }
    }

    fn toggle(set: &mut BTreeSet<ObjectId>, id: ObjectId) {
        if !set.remove(&id) {
            set.insert(id);
        }
    }

    fn attach(&mut self, host: ObjectId, attachment: ObjectId, on: bool) {
        let v = self.attached.entry(host).or_default();
        v.retain(|a| *a != attachment);
        if on {
            v.push(attachment);
        }
    }
}

/// The world as the layer system reads it: the wrapper plus the state whose player values the
/// selectors and the characteristic-defining value read.
struct View<'a> {
    w: &'a Wrap,
    s: &'a GameState,
}

impl View<'_> {
    fn rank(&self, p: Option<PlayerId>) -> i64 {
        p.and_then(|p| self.s.player(p))
            .map_or(0, |ps| ps.counter(RANK))
    }
}

impl ObjectWorld for View<'_> {
    fn objects(&self) -> Vec<ObjectId> {
        self.w.world.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.w.world.base(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        ObjectWorld::counters(&self.w.world, id)
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.w.attached.get(&id).cloned().unwrap_or_default()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        if *selector == sel(SEL_IS_TAPPED) {
            self.w.tapped.contains(&ctx.candidate)
        } else if *selector == sel(SEL_IS_DESIGNATED) {
            self.w.designated.contains(&ctx.candidate)
        } else if *selector == sel(SEL_IS_ENCHANTED) {
            !self.attached_here(ctx.candidate).is_empty()
        } else if *selector == sel(SEL_RANKED_CONTROLLER) {
            self.rank(ctx.candidate_view.controller) >= 10
        } else {
            self.w.world.matches(selector, ctx)
        }
    }

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        if *expr == cda_expr() {
            let controller = self.w.world.bases.get(&object).and_then(|b| b.controller);
            return Some(self.rank(controller));
        }
        self.w.world.eval_value(expr, object)
    }
}

// ---------------------------------------------------------------------------------------------
// the rig: a cached state and a reference state, kept in step
// ---------------------------------------------------------------------------------------------

/// The generating ability of the static effects of object 7.
fn gen7() -> GeneratingAbility {
    GeneratingAbility::new(oid(7), ability("test.o7_static"))
}

/// Two states receiving the same mutations: `cached` is evaluated through one `LayerCache`,
/// `reference` through the uncached `layers::evaluate_all`; both receive the same feedback
/// writes computed from their own evaluation.
struct Rig {
    cached: GameState,
    reference: GameState,
    w: Wrap,
    cache: LayerCache,
    mismatches: Vec<String>,
    steps: usize,
}

impl Rig {
    /// Three players, objects 1-6 creatures, 7 an enchantment source, 8 a CDA creature, and
    /// static effects of object 7 reading status, designation, attachment and a player value.
    fn standard() -> Rig {
        let mut w = Wrap::default();
        for n in 1..=6u64 {
            let p = [P1, P2, P3][usize::try_from(n % 3).expect("fits")];
            w.add(
                n,
                creature(&format!("test.body_{n}"), 1 + (n as i64 % 3), 2),
                p,
            );
        }
        w.add(
            7,
            chars(
                "test.source_7",
                &["Enchantment"],
                vec![ability("test.o7_static")],
            ),
            P1,
        );
        let mut cda = creature("test.cda_8", 0, 0);
        cda.power = Some(cda_expr());
        cda.toughness = Some(cda_expr());
        w.add(8, cda, P2);
        let mut state = GameState::new(21, 3, 20).expect("state");
        for _ in 0..8 {
            state.issue_object_id().expect("id");
        }
        let mut rig = Rig {
            reference: state.clone(),
            cached: state,
            w,
            cache: LayerCache::new(),
            mismatches: Vec::new(),
            steps: 0,
        };
        rig.both(|s, _| {
            let t = s
                .stamp_object(oid(7), StampReason::EntersZone)
                .expect("stamp");
            for (slot, layer, m) in [
                (SEL_IS_TAPPED, Layer::L7c, modify_pt(1, 1)),
                (
                    SEL_IS_DESIGNATED,
                    Layer::L6,
                    LayerModification::AddAbility(kw_ability("test.grant.designated", "haste")),
                ),
                (SEL_IS_ENCHANTED, Layer::L7c, modify_pt(2, 0)),
                (SEL_RANKED_CONTROLLER, Layer::L7c, modify_pt(0, 3)),
            ] {
                s.register_effect(static_sel(oid(7), t, layer, slot, m), Some(gen7()))
                    .expect("register");
            }
        });
        rig
    }

    /// Applies `f` to both states; returns the cached state's result.
    fn both<R>(&mut self, f: impl Fn(&mut GameState, &Wrap) -> R) -> R {
        let r = f(&mut self.cached, &self.w);
        f(&mut self.reference, &self.w);
        r
    }

    /// Registers a resolved effect on both states.
    fn register_resolved(&mut self, layer: Layer, objects: &[u64], m: LayerModification) {
        self.both(|s, _| {
            let t = s.stamp_resolved_effect().expect("stamp");
            s.register_effect(resolved(t, layer, objects, m.clone()), None)
                .expect("register");
        });
    }

    /// Like `register_resolved`, returning the id of the effect.
    fn register_resolved_id(
        &mut self,
        layer: Layer,
        objects: &[u64],
        m: LayerModification,
    ) -> EffectId {
        self.both(|s, _| {
            let t = s.stamp_resolved_effect().expect("stamp");
            s.register_effect(resolved(t, layer, objects, m.clone()), None)
                .expect("register")
        })
    }

    /// Declares `invs`, evaluates both ways, compares, feeds the results back, declares the
    /// player-value invalidation of those writes and warms the cache again.
    fn step(&mut self, invs: &[Invalidation]) {
        self.steps += 1;
        for inv in invs {
            self.cache.invalidate(*inv);
        }
        let cached = self.evaluate_cached();
        let reference = self.evaluate_reference();
        if cached != reference {
            self.mismatches
                .push(format!("step {}: evaluated results differ", self.steps));
        }
        feed_back(&mut self.cached, &cached);
        feed_back(&mut self.reference, &reference);
        // The feedback writes are player writes (state::environment table).
        self.cache.invalidate(Invalidation::PlayerValue);
        // A warm cache is what the next mutation is declared against.
        let warm = self.evaluate_cached();
        if warm != self.evaluate_reference() {
            self.mismatches
                .push(format!("step {}: warm results differ", self.steps));
        }
    }

    fn evaluate_cached(&mut self) -> BTreeMap<ObjectId, Evaluated> {
        let view = View {
            w: &self.w,
            s: &self.cached,
        };
        let env = GameEnvironment::new(&self.cached, &view);
        self.cache
            .evaluate_all(self.cached.effects(), &env)
            .expect("cached evaluation")
    }

    fn evaluate_reference(&self) -> BTreeMap<ObjectId, Evaluated> {
        let view = View {
            w: &self.w,
            s: &self.reference,
        };
        let env = GameEnvironment::new(&self.reference, &view);
        evaluate_all(self.reference.effects(), &env).expect("reference evaluation")
    }

    /// The state hashes of the cached and the reference state.
    fn hashes(&self) -> (Digest, Digest) {
        (self.cached.state_hash(), self.reference.state_hash())
    }

    /// Writes the cached state hash, then asserts that the two paths agreed throughout and
    /// that the hashes are equal.
    fn finish(self, scenario: &str) -> Digest {
        let (c, r) = self.hashes();
        record(scenario, &c);
        assert!(
            self.mismatches.is_empty(),
            "{scenario}: {:?}",
            self.mismatches
        );
        assert_eq!(c, r, "{scenario}: cached and reference state hashes agree");
        c
    }
}

/// Writes the evaluated characteristics into the state: life = total evaluated power of the
/// objects each player controls; counters = number of evaluated abilities and card types.
fn feed_back(state: &mut GameState, evaluated: &BTreeMap<ObjectId, Evaluated>) {
    let players: Vec<PlayerId> = state.players().map(|p| p.id()).collect();
    for p in players {
        let (mut power, mut abilities, mut types) = (0i64, 0i64, 0i64);
        for e in evaluated.values().filter(|e| e.controller == Some(p)) {
            let c = &e.characteristics;
            power += c.power.as_ref().and_then(literal_of).unwrap_or(0);
            abilities += i64::try_from(c.abilities.len()).expect("fits");
            types += i64::try_from(c.card_types.len()).expect("fits");
        }
        let mut pm = state.player_mut(p).expect("player");
        pm.set_life(20 + power);
        pm.set_counter("test.abilities", abilities);
        pm.set_counter("test.types", types);
    }
}

/// Writes `<scenario>.statehash` into `FINEPRINT_DIFF_DIR` when it is set (the only place the
/// variable is read): the lowercase hex of the state hash and one newline. A relative directory
/// is taken relative to the workspace root, an absolute one is used as is.
fn record(scenario: &str, hash: &Digest) {
    assert!(
        !scenario.is_empty()
            && scenario
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "scenario names are lowercase ASCII digits and `_`"
    );
    if let Some(dir) = std::env::var_os("FINEPRINT_DIFF_DIR") {
        let dir = std::path::PathBuf::from(dir);
        let dir = if dir.is_absolute() {
            dir
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join(dir)
        };
        std::fs::create_dir_all(&dir).expect("create FINEPRINT_DIFF_DIR");
        let path = dir.join(format!("{scenario}.statehash"));
        std::fs::write(&path, format!("{}\n", hash.to_hex())).expect("write state hash file");
    }
}

// ---------------------------------------------------------------------------------------------
// a. effect registration with stamps, both timestamp orders
// ---------------------------------------------------------------------------------------------

/// A layer-6 pair (grant and removal of one keyword) and a layer-7b pair (two set-P/T effects)
/// on the same objects, registered in the order given; the timestamp order decides the result
/// (the layer-timestamp behaviour exercised by the existing layer tests).
fn timestamp_pair(grant_first: bool, scenario: &str) -> Digest {
    let mut rig = Rig::standard();
    rig.step(&[]);
    let grant = LayerModification::AddAbility(kw_ability("test.grant.flying", "flying"));
    let remove = LayerModification::RemoveKeyword("flying".to_string());
    let (a, b) = if grant_first {
        (grant.clone(), remove.clone())
    } else {
        (remove, grant)
    };
    rig.both(|s, _| {
        s.stamp_object(oid(1), StampReason::EntersZone)
            .expect("stamp");
    });
    rig.register_resolved(Layer::L6, &[1, 2], a);
    rig.step(&[Invalidation::Timestamp]);
    rig.register_resolved(Layer::L6, &[1, 2], b);
    rig.step(&[Invalidation::EffectRegistration]);
    let (p, q) = if grant_first { (5, 1) } else { (1, 5) };
    rig.register_resolved(Layer::L7b, &[1, 2, 3], set_pt(Some(p), Some(p)));
    rig.step(&[]);
    rig.register_resolved(Layer::L7b, &[1, 2, 3], set_pt(Some(q), Some(q)));
    rig.step(&[]);
    rig.finish(scenario)
}

/// REQ-LAYER-005, REQ-CORE-001: the pair registered grant-first.
#[test]
fn state_hash_timestamp_order_grant_first() {
    timestamp_pair(true, "timestamp_order_grant_first");
}

/// REQ-LAYER-005, REQ-CORE-001: the pair registered removal-first; the result differs from the
/// other order.
#[test]
fn state_hash_timestamp_order_removal_first() {
    let a = timestamp_pair(false, "timestamp_order_removal_first");
    let b = timestamp_pair(true, "timestamp_order_grant_first");
    assert_ne!(a, b, "the timestamp order changes the state");
}

/// REQ-LAYER-005, REQ-CORE-001: a multi-layer group (`register_effect_group`) and a restamp of
/// the static effects of a moved object.
#[test]
fn state_hash_effect_group_and_restamp() {
    let mut rig = Rig::standard();
    rig.step(&[]);
    rig.both(|s, w| {
        let t = s.stamp_resolved_effect().expect("stamp");
        let snap = s.clone();
        let view = View { w, s: &snap };
        let env = GameEnvironment::new(&snap, &view);
        let parts = vec![
            (
                resolved(t, Layer::L4, &[3, 4], add_card_types(&["Artifact"])),
                None,
            ),
            (
                effect(
                    None,
                    P1,
                    t,
                    Layer::L7b,
                    false,
                    fineprint_core::layers::EffectOrigin::Resolved,
                    Affected::SameAsLayer(Layer::L4),
                    set_pt(Some(7), Some(7)),
                ),
                None,
            ),
        ];
        s.register_effect_group(&env, parts).expect("group");
    });
    rig.step(&[Invalidation::EffectRegistration]);
    // Object 7 moves: its timestamp changes and its static effects are restamped (CR 613.7a as
    // cited by the layer-timestamp tests).
    rig.both(|s, _| {
        let old = s
            .stamp_object(oid(7), StampReason::EntersZone)
            .expect("stamp");
        let new = s
            .stamp_object(oid(7), StampReason::EntersZone)
            .expect("stamp");
        s.restamp_static_effects(oid(7), old, new).expect("restamp");
    });
    rig.step(&[Invalidation::Timestamp]);
    rig.finish("effect_group_and_restamp");
}

// ---------------------------------------------------------------------------------------------
// b. counters
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005, REQ-CORE-001: counters change in the world (`CounterChange`) and counter
/// timestamps change (`Timestamp`).
#[test]
fn state_hash_counter_change() {
    let mut rig = Rig::standard();
    rig.register_resolved(Layer::L7b, &[2], set_pt(Some(0), Some(4)));
    rig.step(&[]);
    rig.w.set_counter(oid(1), "+1/+1", 2);
    rig.w.set_counter(oid(2), "-1/-1", 1);
    rig.both(|s, _| {
        s.stamp_counters(CounterHolder::Object(oid(1)), "+1/+1")
            .expect("stamp");
        s.stamp_counters(CounterHolder::Object(oid(2)), "-1/-1")
            .expect("stamp");
    });
    rig.step(&[Invalidation::CounterChange, Invalidation::Timestamp]);
    rig.w.set_counter(oid(1), "+1/+1", 5);
    rig.step(&[Invalidation::CounterChange]);
    rig.w.set_counter(oid(2), "-1/-1", 0);
    rig.both(|s, _| s.clear_counter_stamp(CounterHolder::Object(oid(2)), "-1/-1"));
    rig.step(&[Invalidation::CounterChange, Invalidation::Timestamp]);
    rig.finish("counter_change");
}

// ---------------------------------------------------------------------------------------------
// c. zone change
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005, REQ-CORE-001: an object leaves the battlefield, leaves the object list, and
/// returns.
#[test]
fn state_hash_zone_change() {
    let mut rig = Rig::standard();
    // Make the static effects of object 7 apply to objects 1, 3 and 4 first.
    Wrap::toggle(&mut rig.w.tapped, oid(3));
    Wrap::toggle(&mut rig.w.designated, oid(4));
    rig.w.attach(oid(1), oid(7), true);
    rig.step(&[
        Invalidation::Status,
        Invalidation::Designation,
        Invalidation::Attachment,
    ]);
    rig.w.set_on_battlefield(oid(3), false);
    rig.step(&[Invalidation::ZoneChange]);
    rig.w.set_present(oid(4), false);
    rig.both(|s, _| s.forget_object_stamps(oid(4)));
    rig.step(&[Invalidation::ZoneChange, Invalidation::Timestamp]);
    rig.w.set_present(oid(4), true);
    rig.w.set_on_battlefield(oid(3), true);
    rig.both(|s, _| {
        s.stamp_object(oid(4), StampReason::EntersZone)
            .expect("stamp");
    });
    rig.step(&[Invalidation::ZoneChange, Invalidation::Timestamp]);
    // The source of the static effects leaves: they cease to exist (generating ability gone).
    rig.w.set_present(oid(7), false);
    rig.step(&[Invalidation::ZoneChange]);
    rig.finish("zone_change");
}

// ---------------------------------------------------------------------------------------------
// d. control change
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005, REQ-CORE-001: control changes in the world, with a layer-2 effect registered.
#[test]
fn state_hash_control_change() {
    let mut rig = Rig::standard();
    rig.register_resolved(Layer::L2, &[5], LayerModification::SetController(P3));
    rig.step(&[]);
    rig.w.set_controller(oid(1), P3);
    rig.step(&[Invalidation::ControlChange]);
    rig.w.set_controller(oid(5), P1);
    rig.w.set_controller(oid(6), P2);
    rig.step(&[Invalidation::ControlChange]);
    rig.finish("control_change");
}

// ---------------------------------------------------------------------------------------------
// e. attachment, status, designation
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005, REQ-CORE-001: attachment, status and designation changes in the world.
#[test]
fn state_hash_attachment_status_designation() {
    let mut rig = Rig::standard();
    rig.step(&[]);
    rig.w.attach(oid(1), oid(7), true);
    rig.step(&[Invalidation::Attachment]);
    Wrap::toggle(&mut rig.w.tapped, oid(2));
    Wrap::toggle(&mut rig.w.tapped, oid(3));
    rig.step(&[Invalidation::Status]);
    Wrap::toggle(&mut rig.w.designated, oid(4));
    rig.step(&[Invalidation::Designation]);
    rig.w.attach(oid(1), oid(7), false);
    rig.w.attach(oid(5), oid(7), true);
    Wrap::toggle(&mut rig.w.tapped, oid(2));
    rig.step(&[Invalidation::Attachment, Invalidation::Status]);
    rig.finish("attachment_status_designation");
}

// ---------------------------------------------------------------------------------------------
// f. player values
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005, REQ-CORE-001: a player counter that a selector and a CDA read changes
/// (`PlayerValue`).
#[test]
fn state_hash_player_value_change() {
    let mut rig = Rig::standard();
    rig.step(&[]);
    for (p, v) in [(P1, 12), (P2, 4), (P2, 11), (P3, 10)] {
        rig.both(|s, _| s.player_mut(p).expect("player").set_counter(RANK, v));
        rig.step(&[Invalidation::PlayerValue]);
    }
    rig.both(|s, _| s.set_turn_number(3));
    rig.step(&[Invalidation::PlayerValue]);
    rig.finish("player_value_change");
}

// ---------------------------------------------------------------------------------------------
// g. effect removal and expiry
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005, REQ-CORE-001: `remove_effect` with no declaration (the cache detects registry
/// changes itself).
#[test]
fn state_hash_effect_removal() {
    let mut rig = Rig::standard();
    let id = rig.register_resolved_id(Layer::L7c, &[1, 2, 3], modify_pt(4, 4));
    let other = rig.register_resolved_id(Layer::L6, &[2], LayerModification::RemoveAllAbilities);
    rig.step(&[]);
    assert!(rig.both(|s, _| s.remove_effect(id)));
    rig.step(&[]);
    assert!(rig.both(|s, _| s.remove_effect(other)));
    rig.step(&[]);
    rig.finish("effect_removal");
}

/// REQ-LAYER-005, REQ-CORE-001: a duration-ended effect is removed and `EffectExpiry` declared.
#[test]
fn state_hash_effect_expiry() {
    let mut rig = Rig::standard();
    let id = rig.register_resolved_id(Layer::L7b, &[1, 2], set_pt(Some(9), Some(9)));
    rig.step(&[]);
    assert!(rig.both(|s, _| s.remove_effect(id)));
    rig.step(&[Invalidation::EffectExpiry]);
    rig.finish("effect_expiry");
}

// ---------------------------------------------------------------------------------------------
// property: the cached hash equals the reference hash after every step
// ---------------------------------------------------------------------------------------------

fn random_walk(seed: u64, steps: u32) -> Rig {
    let mut rng = Rng::new(seed);
    let mut rig = Rig::standard();
    let mut ids: Vec<EffectId> = Vec::new();
    rig.step(&[]);
    for _ in 0..steps {
        let obj = |rng: &mut Rng| 1 + rng.below(8);
        let player = |rng: &mut Rng| [P1, P2, P3][usize::try_from(rng.below(3)).expect("fits")];
        let invs: Vec<Invalidation> = match rng.below(12) {
            0 => {
                let (layer, m) = match rng.below(5) {
                    0 => (
                        Layer::L6,
                        LayerModification::AddAbility(kw_ability("test.grant.rw", "flying")),
                    ),
                    1 => (Layer::L6, LayerModification::RemoveKeyword("flying".into())),
                    2 => (Layer::L7b, set_pt(Some(rng.below(5) as i64), Some(3))),
                    3 => (Layer::L7c, modify_pt(rng.below(3) as i64, 1)),
                    _ => (Layer::L4, add_card_types(&["Artifact"])),
                };
                let targets = [obj(&mut rng), obj(&mut rng), obj(&mut rng)];
                ids.push(rig.register_resolved_id(layer, &targets, m));
                Vec::new()
            }
            1 if !ids.is_empty() => {
                let i = usize::try_from(rng.below(ids.len() as u64)).expect("fits");
                let id = ids.remove(i);
                rig.both(|s, _| s.remove_effect(id));
                if rng.below(2) == 0 {
                    vec![Invalidation::EffectExpiry]
                } else {
                    Vec::new()
                }
            }
            2 | 3 => {
                let id = oid(obj(&mut rng));
                let kind = if rng.below(2) == 0 { "+1/+1" } else { "-1/-1" };
                let n = rng.below(4) as i64;
                rig.w.set_counter(id, kind, n);
                rig.both(|s, _| {
                    if n > 0 {
                        s.stamp_counters(CounterHolder::Object(id), kind)
                            .expect("stamp");
                    } else {
                        s.clear_counter_stamp(CounterHolder::Object(id), kind);
                    }
                });
                vec![Invalidation::CounterChange, Invalidation::Timestamp]
            }
            4 => {
                let id = oid(obj(&mut rng));
                if rng.below(2) == 0 {
                    let on = rng.below(2) == 0;
                    rig.w.set_on_battlefield(id, on);
                } else {
                    let present = rng.below(2) == 0;
                    rig.w.set_present(id, present);
                }
                rig.both(|s, _| {
                    s.stamp_object(id, StampReason::EntersZone).expect("stamp");
                });
                vec![Invalidation::ZoneChange, Invalidation::Timestamp]
            }
            5 => {
                let (id, p) = (oid(obj(&mut rng)), player(&mut rng));
                rig.w.set_controller(id, p);
                vec![Invalidation::ControlChange]
            }
            6 => {
                let (host, att) = (oid(obj(&mut rng)), oid(obj(&mut rng)));
                let on = rng.below(2) == 0;
                rig.w.attach(host, att, on);
                vec![Invalidation::Attachment]
            }
            7 => {
                let id = oid(obj(&mut rng));
                Wrap::toggle(&mut rig.w.tapped, id);
                vec![Invalidation::Status]
            }
            8 => {
                let id = oid(obj(&mut rng));
                Wrap::toggle(&mut rig.w.designated, id);
                vec![Invalidation::Designation]
            }
            9 => {
                let (p, v) = (player(&mut rng), rng.below(16) as i64);
                rig.both(|s, _| s.player_mut(p).expect("player").set_counter(RANK, v));
                vec![Invalidation::PlayerValue]
            }
            10 => {
                let id = oid(obj(&mut rng));
                rig.both(|s, _| {
                    s.stamp_object(id, StampReason::Attached).expect("stamp");
                });
                vec![Invalidation::Timestamp]
            }
            _ => {
                let id = oid(obj(&mut rng));
                rig.both(|s, _| s.forget_object_stamps(id));
                vec![Invalidation::Timestamp]
            }
        };
        rig.step(&invs);
        let (c, r) = rig.hashes();
        assert_eq!(c, r, "cached and reference hashes agree after every step");
    }
    rig
}

/// REQ-LAYER-005, REQ-CORE-001, REQ-NFR-006: a seeded random walk of 60 mutation steps drawn
/// from every kind; the cached state hash equals the reference after each; the hash is written,
/// and a second run gives the same bytes (determinism).
#[test]
fn state_hash_random_walk_seed_1() {
    let rig = random_walk(1, 60);
    assert!(rig.steps >= 61);
    let first = rig.finish("random_walk_seed_1");
    let again = random_walk(1, 60).finish("random_walk_seed_1");
    assert_eq!(first, again, "two runs of one scenario agree");
}

/// REQ-LAYER-005, REQ-CORE-001: a different seed gives a different state.
#[test]
fn state_hash_random_walk_seeds_differ() {
    let a = random_walk(2, 40);
    let b = random_walk(3, 40);
    assert_eq!(a.hashes().0, a.hashes().1);
    assert_ne!(a.hashes().0, b.hashes().0);
}

// ---------------------------------------------------------------------------------------------
// sensitivity
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005: a world mutation without its invalidation feeds a stale evaluation into the
/// state, and the state hash differs from the state fed by the fresh evaluation. Not run in
/// full-recompute mode, where the cache never serves stale values. Writes no file.
#[cfg(not(feature = "full-recompute"))]
#[test]
fn stale_cache_changes_the_state_hash() {
    let mut rig = Rig::standard();
    rig.register_resolved(Layer::L7c, &[1], modify_pt(1, 1));
    rig.step(&[]);
    rig.w.set_counter(oid(1), "+1/+1", 3);
    rig.step(&[]);
    let (c, r) = rig.hashes();
    assert_ne!(c, r, "an undeclared counter change goes stale");
    assert!(!rig.mismatches.is_empty());
}

/// REQ-LAYER-005: the same mutation with `CounterChange` declared gives equal hashes (both
/// modes). Writes no file.
#[test]
fn declared_invalidation_keeps_the_state_hash() {
    let mut rig = Rig::standard();
    rig.register_resolved(Layer::L7c, &[1], modify_pt(1, 1));
    rig.step(&[]);
    rig.w.set_counter(oid(1), "+1/+1", 3);
    rig.step(&[Invalidation::CounterChange]);
    let (c, r) = rig.hashes();
    assert_eq!(c, r);
    assert!(rig.mismatches.is_empty());
}
