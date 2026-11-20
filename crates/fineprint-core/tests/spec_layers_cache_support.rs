// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the layer-cache spec tests (REQ-LAYER-005, REQ-CORE-001). Black-box:
//! written from the FP-035 work-item brief only, against the public API of
//! `fineprint_core::layers` (`LayerCache`, `Invalidation`, `StateVersion`, `CacheStats`,
//! `FULL_RECOMPUTE`, `evaluated_digest`).
//!
//! Each `spec_layers_cache_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. It re-exports the shared layer-test helper as [`support`] and adds a
//! mutable synthetic environment, [`CacheEnv`], that models every input the cache cannot observe
//! by itself: zone (object list and `on_battlefield`), control, counters and counter timestamps,
//! designations, attachment, status, and player values read by a characteristic-defining value
//! (`eval_value`) and by a condition selector. Every card here is synthetic and neutrally named
//! (R5).

#![allow(dead_code)]

#[path = "spec_layers_support.rs"]
pub mod support;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::Digest;
use fineprint_base::ids::{ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, Evaluated, LayerCache, LayerEnvironment,
    LayerError, LayerModification, MatchContext, ObjectBase, evaluate, evaluate_all,
    evaluated_digest,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::ability::Layer;
use fineprint_dsl::expr::ObjectSelectorAST;
use support::*;

pub const P4: PlayerId = PlayerId::new(4);
pub const PLAYERS: [PlayerId; 4] = [P1, P2, P3, P4];

// ---------------------------------------------------------------------------------------------
// selectors of the cache tests (slots above the shared ones)
// ---------------------------------------------------------------------------------------------

/// "tapped permanent": the candidate has the status flag `tapped` in [`CacheEnv::status`].
pub const SEL_TAPPED: u32 = 201;
/// "designated object": the candidate carries the designation [`DESIGNATION`].
pub const SEL_DESIGNATED: u32 = 202;
/// "enchanted object": something is attached to the candidate (`attached_here` non-empty).
pub const SEL_HAS_ATTACHMENT: u32 = 203;
/// "controlled by a player whose [`PV_RANK`] value is at least 10" (a condition reading a
/// player value).
pub const SEL_RANKED_CONTROLLER: u32 = 204;

/// The designation [`SEL_DESIGNATED`] reads (synthetic name).
pub const DESIGNATION: &str = "test.designation";
/// The status flag [`SEL_TAPPED`] reads.
pub const STATUS_TAPPED: &str = "tapped";
/// The player value read by the CDA expression [`cda_expr`] and by [`SEL_RANKED_CONTROLLER`].
pub const PV_RANK: &str = "test.rank";

/// The characteristic-defining value expression whose value is the controller's [`PV_RANK`].
pub fn cda_expr() -> ValueExpressionAST {
    non_literal_memory("test.cda.controller_rank")
}

// ---------------------------------------------------------------------------------------------
// the mutable environment
// ---------------------------------------------------------------------------------------------

/// A mutable synthetic `LayerEnvironment`: the shared [`TestEnv`] plus status flags,
/// designations and player values. All reads are pure functions of the fields.
#[derive(Clone, Default)]
pub struct CacheEnv {
    pub inner: TestEnv,
    pub status: BTreeMap<ObjectId, BTreeSet<String>>,
    pub designations: BTreeSet<(ObjectId, String)>,
    pub player_values: BTreeMap<(PlayerId, String), i64>,
}

impl CacheEnv {
    pub fn new() -> CacheEnv {
        CacheEnv::default()
    }

    pub fn put(&mut self, id: u64, c: Characteristics, controller: PlayerId) -> ObjectId {
        self.inner.put_full(oid(id), c, Some(controller), true)
    }

    pub fn tap(&mut self, id: ObjectId, tapped: bool) {
        let flags = self.status.entry(id).or_default();
        if tapped {
            flags.insert(STATUS_TAPPED.to_string());
        } else {
            flags.remove(STATUS_TAPPED);
        }
    }

    pub fn designate(&mut self, id: ObjectId, on: bool) {
        if on {
            self.designations.insert((id, DESIGNATION.to_string()));
        } else {
            self.designations.remove(&(id, DESIGNATION.to_string()));
        }
    }

    pub fn attach(&mut self, host: ObjectId, attachment: ObjectId) {
        self.inner
            .attached
            .entry(host)
            .or_default()
            .push(attachment);
    }

    pub fn detach_all(&mut self, host: ObjectId) {
        self.inner.attached.remove(&host);
    }

    pub fn set_player_value(&mut self, p: PlayerId, key: &str, v: i64) {
        self.player_values.insert((p, key.to_string()), v);
    }

    fn player_value(&self, p: Option<PlayerId>, key: &str) -> Option<i64> {
        p.and_then(|p| self.player_values.get(&(p, key.to_string())).copied())
    }

    pub fn set_controller(&mut self, id: ObjectId, p: PlayerId) {
        self.inner
            .bases
            .get_mut(&id)
            .expect("object exists")
            .controller = Some(p);
    }

    pub fn set_on_battlefield(&mut self, id: ObjectId, on: bool) {
        self.inner
            .bases
            .get_mut(&id)
            .expect("object exists")
            .on_battlefield = on;
    }
}

impl LayerEnvironment for CacheEnv {
    fn objects(&self) -> Vec<ObjectId> {
        self.inner.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.inner.base(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.inner.counters(id)
    }

    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.inner.counter_timestamp(id, kind)
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.inner.attached_here(id)
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        let id = ctx.candidate;
        if *selector == sel(SEL_TAPPED) {
            return self
                .status
                .get(&id)
                .is_some_and(|f| f.contains(STATUS_TAPPED));
        }
        if *selector == sel(SEL_DESIGNATED) {
            return self.designations.contains(&(id, DESIGNATION.to_string()));
        }
        if *selector == sel(SEL_HAS_ATTACHMENT) {
            return !self.inner.attached_here(id).is_empty();
        }
        if *selector == sel(SEL_RANKED_CONTROLLER) {
            return self
                .player_value(ctx.candidate_view.controller, PV_RANK)
                .is_some_and(|v| v >= 10);
        }
        self.inner.matches(selector, ctx)
    }

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        if *expr == cda_expr() {
            let controller = self.inner.bases.get(&object).and_then(|b| b.controller);
            return Some(self.player_value(controller, PV_RANK).unwrap_or(0));
        }
        self.inner.eval_value(expr, object)
    }
}

// ---------------------------------------------------------------------------------------------
// the standard board
// ---------------------------------------------------------------------------------------------

/// Object ids of the standard board with a role in the effects or the mutations.
pub mod ids {
    /// A vanilla 2/2 of P1 (the subject of control and zone changes).
    pub const VANILLA: u64 = 1;
    /// A 1/1 of P2 that a layer-2 effect hands to P3.
    pub const STOLEN: u64 = 2;
    /// A flier of P1 (dependency pair subject: its flying is removed by a locked-set remover).
    pub const FLIER: u64 = 3;
    /// A creature whose power and toughness come from the controller's player value (CDA).
    pub const CDA_VALUE: u64 = 4;
    /// A creature with a layer-7a characteristic-defining effect.
    pub const CDA_EFFECT: u64 = 5;
    /// A creature with +1/+1 counters.
    pub const COUNTERED: u64 = 6;
    /// A creature with a keyword counter whose timestamp is flipped.
    pub const KW_COUNTER: u64 = 7;
    /// A creature that two layer-7b effects set (timestamp flip with `set_timestamp`).
    pub const TWO_SETTERS: u64 = 8;
    /// A noncreature enchantment (attached to [`HOST`] in some scenarios).
    pub const AURA: u64 = 9;
    /// A creature that receives attachments.
    pub const HOST: u64 = 10;
    /// A creature that is tapped and untapped.
    pub const TAPPABLE: u64 = 11;
    /// A creature that is designated and undesignated.
    pub const DESIGNEE: u64 = 12;
    /// An object off the battlefield.
    pub const OFF_FIELD: u64 = 13;
    /// The removable effect-target of the lock-in group.
    pub const LOCKED: u64 = 14;
}

/// The standard synthetic board: 4 players, 24 objects (creatures, enchantments, lands,
/// artifacts, one object off the battlefield), +1/+1 counters, a keyword counter, an attachment,
/// a status flag, a designation and player values.
pub fn standard_board() -> CacheEnv {
    use ids::*;
    let mut env = CacheEnv::new();
    env.put(VANILLA, creature("test.vanilla_2_2", 2, 2), P1);
    env.put(STOLEN, creature("test.stolen_1_1", 1, 1), P2);
    env.put(
        FLIER,
        with_abilities(
            creature("test.flier_2_2", 2, 2),
            vec![kw_ability("test.flier_2_2.flying", "flying")],
        ),
        P1,
    );
    let mut cda = creature("test.cda_rank", 0, 0);
    cda.power = Some(cda_expr());
    cda.toughness = Some(cda_expr());
    env.put(CDA_VALUE, cda, P2);
    env.put(CDA_EFFECT, creature("test.cda_effect", 0, 0), P3);
    env.put(COUNTERED, creature("test.countered_1_1", 1, 1), P3);
    env.inner.counter(oid(COUNTERED), "+1/+1", 2, 40);
    env.put(KW_COUNTER, creature("test.kw_counter_3_3", 3, 3), P4);
    env.inner.counter(oid(KW_COUNTER), "flying", 1, 5);
    env.put(TWO_SETTERS, creature("test.two_setters_4_4", 4, 4), P4);
    env.put(AURA, enchantment("test.aura"), P1);
    env.put(HOST, creature("test.host_2_3", 2, 3), P2);
    env.attach(oid(HOST), oid(AURA));
    env.put(TAPPABLE, creature("test.tappable_3_1", 3, 1), P3);
    env.tap(oid(TAPPABLE), true);
    env.put(DESIGNEE, creature("test.designee_1_4", 1, 4), P4);
    env.designate(oid(DESIGNEE), true);
    env.inner.put_full(
        oid(OFF_FIELD),
        creature("test.graveyard_2_2", 2, 2),
        Some(P1),
        false,
    );
    env.put(
        LOCKED,
        with_colors(creature("test.locked_2_2", 2, 2), 1),
        P2,
    );
    // filler permanents for every player
    for i in 0..10u64 {
        let id = 100 + i;
        let owner = PLAYERS[usize::try_from(i % 4).expect("fits")];
        let c = match i % 5 {
            0 => land(&format!("test.land_{i}")),
            1 => artifact(&format!("test.artifact_{i}")),
            2 => enchantment(&format!("test.enchantment_{i}")),
            3 => with_colors(creature(&format!("test.white_creature_{i}"), 1, 1), 1),
            _ => creature(&format!("test.creature_{i}"), 2, 1),
        };
        env.put(id, c, owner);
    }
    env.set_player_value(P1, PV_RANK, 3);
    env.set_player_value(P2, PV_RANK, 5);
    env.set_player_value(P3, PV_RANK, 12);
    env.set_player_value(P4, PV_RANK, 1);
    env
}

/// Effect ids of [`standard_effects`].
pub mod eids {
    pub const STEAL: u64 = 1;
    pub const ANIMATE_ENCHANTMENTS: u64 = 2;
    pub const DEP_REMOVER: u64 = 3;
    pub const DEP_DEPENDENT: u64 = 4;
    pub const CDA_SET: u64 = 5;
    pub const SETTER_A: u64 = 6;
    pub const SETTER_B: u64 = 7;
    pub const ANTHEM: u64 = 8;
    pub const LOCKED_COLORS: u64 = 9;
    pub const KW_REMOVER: u64 = 10;
    pub const TAPPED_SHRINK: u64 = 11;
    pub const DESIGNATED_GROW: u64 = 12;
    pub const ATTACHED_GRANT: u64 = 13;
    pub const RANKED_GROW: u64 = 14;
    pub const P2_GRANT: u64 = 15;
    pub const GROUP_L4: u64 = 20;
    pub const GROUP_L7B: u64 = 21;
}

/// The dependency pair of layer 6 (the shape of the existing REQ-LAYER-002 spec tests,
/// CR 613.8a, 613.8b): a remover of the flying keyword on a locked set and a dependent effect
/// that grants a marker ability to every object with flying.
pub fn dependency_pair(remover_ts: u64, dependent_ts: u64) -> Vec<(u64, ContinuousEffect)> {
    vec![
        (
            eids::DEP_REMOVER,
            eff(
                Layer::L6,
                remover_ts,
                objs(&[oid(ids::FLIER)]),
                LayerModification::RemoveKeyword("flying".to_string()),
            ),
        ),
        (
            eids::DEP_DEPENDENT,
            eff(
                Layer::L6,
                dependent_ts,
                by_sel(SEL_HAS_FLYING),
                LayerModification::AddAbility(static_ability("test.dependent_marker")),
            ),
        ),
    ]
}

/// The multi-layer lock-in group (CR 613.6): a resolved layer-4 part adding the Artifact type
/// to white creatures (locked at registration, CR 611.2c) and a layer-7b part following it.
pub fn lockin_group(t: u64) -> Vec<(fineprint_base::ids::EffectId, ContinuousEffect)> {
    vec![
        (
            eid(eids::GROUP_L4),
            eff(
                Layer::L4,
                t,
                by_sel(SEL_WHITE_CREATURE),
                add_card_types(&["Artifact"]),
            ),
        ),
        (
            eid(eids::GROUP_L7B),
            eff(
                Layer::L7b,
                t,
                Affected::SameAsLayer(Layer::L4),
                set_pt(Some(5), Some(5)),
            ),
        ),
    ]
}

/// The standard effects over `env`: layers 2, 4, 6, 7a (CDA), 7b (two setters), 7c (anthem and
/// selector effects reading status, designation, attachment and player values), a locked-set
/// resolved effect, the dependency pair and the lock-in group.
pub fn standard_effects(env: &CacheEnv) -> ContinuousEffects {
    use eids::*;
    let mut list = vec![
        (
            STEAL,
            eff(
                Layer::L2,
                10,
                objs(&[oid(ids::STOLEN)]),
                LayerModification::SetController(P3),
            ),
        ),
        (
            ANIMATE_ENCHANTMENTS,
            eff(
                Layer::L4,
                11,
                by_sel(SEL_NONCREATURE_ENCHANTMENT),
                add_card_types(&["Creature"]),
            ),
        ),
        (
            CDA_SET,
            cda_eff(
                Layer::L7a,
                12,
                objs(&[oid(ids::CDA_EFFECT)]),
                set_pt(Some(6), Some(6)),
            ),
        ),
        (
            SETTER_A,
            eff(
                Layer::L7b,
                20,
                objs(&[oid(ids::TWO_SETTERS)]),
                set_pt(Some(1), Some(1)),
            ),
        ),
        (
            SETTER_B,
            eff(
                Layer::L7b,
                21,
                objs(&[oid(ids::TWO_SETTERS)]),
                set_pt(Some(7), Some(7)),
            ),
        ),
        (
            ANTHEM,
            eff(
                Layer::L7c,
                22,
                by_sel(SEL_CONTROLLED_BY_P2),
                modify_pt(1, 1),
            ),
        ),
        (
            LOCKED_COLORS,
            eff(
                Layer::L5,
                23,
                objs(&[oid(ids::VANILLA), oid(ids::LOCKED)]),
                LayerModification::AddColors(mask(2)),
            ),
        ),
        (
            KW_REMOVER,
            eff(
                Layer::L6,
                30,
                objs(&[oid(ids::KW_COUNTER)]),
                LayerModification::RemoveAllAbilities,
            ),
        ),
        (
            TAPPED_SHRINK,
            eff(Layer::L7c, 31, by_sel(SEL_TAPPED), modify_pt(-1, 0)),
        ),
        (
            DESIGNATED_GROW,
            eff(Layer::L7c, 32, by_sel(SEL_DESIGNATED), modify_pt(2, 2)),
        ),
        (
            ATTACHED_GRANT,
            eff(
                Layer::L6,
                33,
                by_sel(SEL_HAS_ATTACHMENT),
                LayerModification::AddAbility(kw_ability("test.attached.vigilance", "vigilance")),
            ),
        ),
        (
            RANKED_GROW,
            eff(
                Layer::L7c,
                34,
                by_sel(SEL_RANKED_CONTROLLER),
                modify_pt(0, 3),
            ),
        ),
        (
            P2_GRANT,
            static_eff(
                oid(ids::HOST),
                None,
                Layer::L6,
                35,
                by_sel(SEL_CONTROLLED_BY_P2),
                LayerModification::AddAbility(kw_ability("test.p2.reach", "reach")),
            ),
        ),
    ];
    list.extend(dependency_pair(24, 25));
    let mut r = registry(&list);
    r.register_group(env, lockin_group(26))
        .expect("register_group succeeds");
    r
}

// ---------------------------------------------------------------------------------------------
// larger boards
// ---------------------------------------------------------------------------------------------

/// A board of `n` permanents spread over the 4 players with counters, a deterministic mix of
/// types and colors, attachments, statuses and designations, generated from `seed`.
pub fn big_board(n: u64, seed: u64) -> CacheEnv {
    let mut rng = Lcg::new(seed);
    let mut env = CacheEnv::new();
    for i in 0..n {
        let id = 1000 + i;
        let owner = PLAYERS[usize::try_from(i % 4).expect("fits")];
        let c = match rng.below(6) {
            0 => land(&format!("test.land_{i}")),
            1 => artifact(&format!("test.artifact_{i}")),
            2 => enchantment(&format!("test.enchantment_{i}")),
            3 => with_abilities(
                creature(&format!("test.flier_{i}"), 2, 2),
                vec![kw_ability(&format!("test.flier_{i}.flying"), "flying")],
            ),
            4 => with_colors(creature(&format!("test.white_{i}"), 1, 3), 1),
            _ => creature(
                &format!("test.creature_{i}"),
                i64::try_from(rng.below(5)).expect("fits"),
                1 + i64::try_from(rng.below(5)).expect("fits"),
            ),
        };
        env.put(id, c, owner);
        match rng.below(8) {
            0 => env.inner.counter(
                oid(id),
                "+1/+1",
                1 + i64::try_from(rng.below(3)).expect("fits"),
                rng.below(100),
            ),
            1 => env.inner.counter(oid(id), "-1/-1", 1, rng.below(100)),
            2 => env.tap(oid(id), true),
            3 => env.designate(oid(id), true),
            _ => {}
        }
        if i > 0 && rng.below(10) == 0 {
            env.attach(oid(id), oid(id - 1));
        }
    }
    for (k, p) in PLAYERS.iter().enumerate() {
        env.set_player_value(*p, PV_RANK, 4 * i64::try_from(k).expect("fits"));
    }
    env
}

/// A varied set of effects over a [`big_board`] of `n` permanents.
pub fn big_effects(env: &CacheEnv, n: u64) -> ContinuousEffects {
    let first = 1000;
    let some: Vec<ObjectId> = (0..n).step_by(7).map(|i| oid(first + i)).collect();
    let mut r = registry(&[
        (
            1,
            eff(
                Layer::L2,
                1,
                Affected::Objects(some.iter().copied().take(5).collect()),
                LayerModification::SetController(P2),
            ),
        ),
        (
            2,
            eff(
                Layer::L4,
                2,
                by_sel(SEL_NONCREATURE_ENCHANTMENT),
                add_card_types(&["Creature"]),
            ),
        ),
        (
            3,
            eff(
                Layer::L6,
                3,
                by_sel(SEL_TAPPED),
                LayerModification::RemoveKeyword("flying".to_string()),
            ),
        ),
        (
            4,
            eff(
                Layer::L6,
                4,
                by_sel(SEL_HAS_FLYING),
                LayerModification::AddAbility(static_ability("test.big.flier_marker")),
            ),
        ),
        (
            5,
            eff(Layer::L7b, 5, by_sel(SEL_DESIGNATED), set_pt(Some(3), None)),
        ),
        (
            6,
            eff(Layer::L7c, 6, by_sel(SEL_CONTROLLED_BY_P2), modify_pt(1, 1)),
        ),
        (
            7,
            eff(
                Layer::L7c,
                7,
                by_sel(SEL_RANKED_CONTROLLER),
                modify_pt(0, 2),
            ),
        ),
        (
            8,
            eff(
                Layer::L5,
                8,
                Affected::Objects(some.iter().copied().skip(5).collect()),
                LayerModification::SetColors(mask(4)),
            ),
        ),
        (
            9,
            eff(
                Layer::L6,
                9,
                by_sel(SEL_HAS_ATTACHMENT),
                LayerModification::AddAbility(kw_ability("test.big.vigilance", "vigilance")),
            ),
        ),
        (
            10,
            eff(
                Layer::L7d,
                10,
                by_sel(SEL_WHITE_CREATURE),
                LayerModification::SwitchPt,
            ),
        ),
    ]);
    r.register_group(env, lockin_group(11))
        .expect("register_group succeeds");
    r
}

// ---------------------------------------------------------------------------------------------
// comparisons
// ---------------------------------------------------------------------------------------------

/// The uncached reference: `layers::evaluate_all`.
pub fn fresh_all(
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
) -> BTreeMap<ObjectId, Evaluated> {
    evaluate_all(effects, env).expect("fresh evaluate_all succeeds")
}

/// The uncached digest: `evaluated_digest(layers::evaluate_all(..))`.
pub fn fresh_digest(effects: &ContinuousEffects, env: &dyn LayerEnvironment) -> Digest {
    evaluated_digest(&fresh_all(effects, env))
}

/// The cached digest: `evaluated_digest(cache.evaluate_all(..))`.
pub fn cached_digest(
    cache: &mut LayerCache,
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
) -> Digest {
    evaluated_digest(
        &cache
            .evaluate_all(effects, env)
            .expect("cached evaluate_all succeeds"),
    )
}

/// The differential comparison: `Ok` when the two digests are equal, otherwise a description
/// of the mismatch naming the scenario.
pub fn same_digests(scenario: &str, cached: &Digest, uncached: &Digest) -> Result<(), String> {
    if cached == uncached {
        Ok(())
    } else {
        Err(format!(
            "{scenario}: cached digest {} != uncached digest {}",
            cached.to_hex(),
            uncached.to_hex()
        ))
    }
}

/// Asserts that the cache gives a fresh `layers::evaluate` for every object of `env` (queried
/// in `order`, or ascending when `None`), and that `cache.evaluate_all` and its digest equal
/// the uncached ones.
pub fn assert_cache_fresh(
    what: &str,
    cache: &mut LayerCache,
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
) {
    let expected = fresh_all(effects, env);
    let got = cache
        .evaluate_all(effects, env)
        .unwrap_or_else(|e| panic!("{what}: cached evaluate_all failed: {e:?}"));
    assert_eq!(
        got, expected,
        "{what}: cache.evaluate_all != layers::evaluate_all"
    );
    for id in env.objects() {
        let one: Result<Evaluated, LayerError> = cache.evaluate(effects, env, id);
        assert_eq!(
            one,
            evaluate(effects, env, id),
            "{what}: cache.evaluate({id:?}) != layers::evaluate"
        );
    }
    assert_eq!(
        evaluated_digest(&got),
        evaluated_digest(&expected),
        "{what}: digests differ"
    );
}
