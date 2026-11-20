// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The cached versus full-recompute differential (REQ-LAYER-005, SRS 6.5; REQ-CORE-001). The CI
//! job `differential` runs this target once in the default (cached) mode and once with
//! `--features full-recompute`, each time with `FINEPRINT_DIFF_DIR` set, and requires the two
//! directories of `<scenario>.digest` files to be identical.
//!
//! Each named scenario builds a synthetic board, computes `evaluated_digest` through
//! `LayerCache::evaluate_all` and through the uncached `layers::evaluate_all`, requires them to be
//! equal (in both modes), requires two warm repeated cached calls to give the same digest, and,
//! when `FINEPRINT_DIFF_DIR` is set, writes the lowercase hex digest and one newline to
//! `<scenario>.digest`. The digest is a pure function of the scenario. The variable is read only
//! in this file, at the end of each scenario; the tests pass when it is unset.
//!
//! The stale-cache test shows that the comparison is sensitive: a deliberately broken memo that
//! never invalidates is reported as a mismatch after a mutation, while the real `LayerCache` with
//! the proper invalidation matches. Black-box: written from the FP-035 brief only.

#[path = "spec_layers_cache_support.rs"]
mod cache_support;

use std::collections::BTreeMap;

use cache_support::support::*;
use cache_support::*;
use fineprint_base::hash::Digest;
use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{
    Affected, ContinuousEffects, Evaluated, Invalidation, LayerCache, LayerEnvironment, LayerError,
    LayerModification, evaluate, evaluated_digest,
};
use fineprint_dsl::ability::Layer;

/// Runs the differential for one scenario and returns the digest it agreed on.
fn differential(scenario: &str, effects: &ContinuousEffects, env: &dyn LayerEnvironment) -> Digest {
    assert!(
        !scenario.is_empty()
            && scenario
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "scenario names are lowercase ASCII with `_`"
    );
    let uncached = fresh_digest(effects, env);
    let mut cache = LayerCache::new();
    let cold = cached_digest(&mut cache, effects, env);
    same_digests(scenario, &cold, &uncached).unwrap_or_else(|e| panic!("cold: {e}"));
    let warm1 = cached_digest(&mut cache, effects, env);
    let warm2 = cached_digest(&mut cache, effects, env);
    assert_eq!(warm1, warm2, "{scenario}: two warm calls agree");
    same_digests(scenario, &warm1, &uncached).unwrap_or_else(|e| panic!("warm: {e}"));
    uncached
}

/// Writes `<scenario>.digest` into `FINEPRINT_DIFF_DIR` when it is set (the only place the
/// variable is read). A relative directory is taken relative to the workspace root (where the
/// CI job and the brief's commands run `cargo test` and compare the directories), not to the
/// package directory `cargo test` runs the test binary in; an absolute one is used as is. The
/// workspace root comes from the compile-time `CARGO_MANIFEST_DIR` of this package.
fn record(scenario: &str, digest: &Digest) {
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
        let path = dir.join(format!("{scenario}.digest"));
        std::fs::write(&path, format!("{}\n", digest.to_hex())).expect("write digest file");
    }
}

/// The differential, then the digest file.
fn scenario(name: &str, effects: &ContinuousEffects, env: &dyn LayerEnvironment) -> Digest {
    let d = differential(name, effects, env);
    record(name, &d);
    d
}

// ---------------------------------------------------------------------------------------------
// scenarios
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-005, REQ-CORE-001: the empty board.
#[test]
fn diff_empty_board() {
    let d = scenario("empty_board", &ContinuousEffects::new(), &CacheEnv::new());
    assert_eq!(d, evaluated_digest(&BTreeMap::new()));
}

/// REQ-LAYER-005, REQ-CORE-001: a vanilla board without effects.
#[test]
fn diff_vanilla_board() {
    let mut env = CacheEnv::new();
    for i in 1..=8u64 {
        let p = PLAYERS[usize::try_from(i % 4).expect("fits")];
        env.put(
            i,
            creature(&format!("test.vanilla_{i}"), 1 + (i as i64 % 3), 2),
            p,
        );
    }
    env.put(20, land("test.land"), P1);
    scenario("vanilla_board", &ContinuousEffects::new(), &env);
}

/// REQ-LAYER-005, REQ-CORE-001: type changes (layer 4) and keyword grants and removals
/// (layer 6).
#[test]
fn diff_type_change_and_keyword_grants() {
    let mut env = CacheEnv::new();
    env.put(1, enchantment("test.enchantment"), P1);
    env.put(2, creature("test.bear_2_2", 2, 2), P2);
    env.put(
        3,
        with_abilities(
            creature("test.flier_1_1", 1, 1),
            vec![kw_ability("test.flier_1_1.flying", "flying")],
        ),
        P3,
    );
    env.put(4, artifact("test.artifact"), P4);
    let effects = registry(&[
        (
            1,
            eff(
                Layer::L4,
                1,
                by_sel(SEL_NONCREATURE_ENCHANTMENT),
                add_card_types(&["Creature"]),
            ),
        ),
        (
            2,
            eff(
                Layer::L6,
                2,
                by_sel(SEL_CREATURE),
                LayerModification::AddAbility(kw_ability("test.grant.trample", "trample")),
            ),
        ),
        (
            3,
            eff(
                Layer::L6,
                3,
                objs(&[oid(3)]),
                LayerModification::RemoveKeyword("flying".to_string()),
            ),
        ),
        (
            4,
            eff(Layer::L7b, 4, objs(&[oid(1)]), set_pt(Some(3), Some(3))),
        ),
    ]);
    scenario("type_change_and_keyword_grants", &effects, &env);
}

/// REQ-LAYER-005, REQ-CORE-001: a layer-7 chain (7a CDA, 7b, 7c with +1/+1 and -1/-1 counters,
/// 7d).
#[test]
fn diff_layer7_chain_with_counters() {
    let mut env = CacheEnv::new();
    env.put(1, creature("test.chain_a", 1, 1), P1);
    env.put(2, creature("test.chain_b", 2, 5), P2);
    env.put(3, creature("test.chain_c", 0, 0), P3);
    env.inner.counter(oid(1), "+1/+1", 2, 3);
    env.inner.counter(oid(2), "-1/-1", 1, 6);
    env.inner.counter(oid(3), "+1/+1", 1, 1);
    let effects = registry(&[
        (
            1,
            cda_eff(Layer::L7a, 1, objs(&[oid(3)]), set_pt(Some(4), Some(4))),
        ),
        (
            2,
            eff(
                Layer::L7b,
                2,
                objs(&[oid(1), oid(2)]),
                set_pt(Some(0), None),
            ),
        ),
        (3, eff(Layer::L7c, 4, by_sel(SEL_CREATURE), modify_pt(1, 2))),
        (
            4,
            eff(Layer::L7d, 5, objs(&[oid(2)]), LayerModification::SwitchPt),
        ),
    ]);
    scenario("layer7_chain_with_counters", &effects, &env);
}

/// REQ-LAYER-005, REQ-CORE-001: a CDA whose value `eval_value` reads from a player value, and a
/// condition selector reading a player value.
#[test]
fn diff_cda_reads_player_values() {
    let mut env = CacheEnv::new();
    for (i, p) in PLAYERS.iter().enumerate() {
        let id = u64::try_from(i).expect("fits") + 1;
        let mut c = creature(&format!("test.cda_{id}"), 0, 0);
        c.power = Some(cda_expr());
        c.toughness = Some(cda_expr());
        env.put(id, c, *p);
        env.set_player_value(*p, PV_RANK, 7 * i64::try_from(i).expect("fits"));
    }
    let effects = registry(&[(
        1,
        eff(
            Layer::L7c,
            1,
            by_sel(SEL_RANKED_CONTROLLER),
            modify_pt(1, 0),
        ),
    )]);
    scenario("cda_reads_player_values", &effects, &env);
}

fn dependency_board() -> (CacheEnv, ContinuousEffects, ContinuousEffects) {
    let env = standard_board();
    let pair = |r: u64, d: u64| {
        let list: Vec<(u64, _)> = dependency_pair(r, d);
        registry(&list)
    };
    (env, pair(1, 2), pair(2, 1))
}

/// REQ-LAYER-005, REQ-CORE-001: the dependency pair, remover earlier (CR 613.8a, 613.8b as
/// cited by the existing REQ-LAYER-002 spec tests).
#[test]
fn diff_dependency_remover_earlier() {
    let (env, early, _) = dependency_board();
    scenario("dependency_remover_earlier", &early, &env);
}

/// REQ-LAYER-005, REQ-CORE-001: the dependency pair, dependent earlier; the result equals the
/// other timestamp order (dependency decides).
#[test]
fn diff_dependency_dependent_earlier() {
    let (env, early, late) = dependency_board();
    let d = scenario("dependency_dependent_earlier", &late, &env);
    assert_eq!(d, differential("dependency_remover_earlier", &early, &env));
}

/// REQ-LAYER-005, REQ-CORE-001: a multi-layer lock-in group (CR 613.6) on the standard board
/// with all its effects.
#[test]
fn diff_multi_layer_lockin_group() {
    let env = standard_board();
    let effects = standard_effects(&env);
    scenario("multi_layer_lockin_group", &effects, &env);
}

/// REQ-LAYER-005, REQ-CORE-001: a 200-permanent 4-player board.
#[test]
fn diff_board_200_permanents_4_players() {
    let env = big_board(200, 2024);
    assert_eq!(env.objects().len(), 200);
    let effects = big_effects(&env, 200);
    scenario("board_200_permanents_4_players", &effects, &env);
}

/// REQ-LAYER-005, REQ-CORE-001: the standard board after an effect is removed; differs from the
/// board with it.
#[test]
fn diff_after_effect_removed() {
    let env = standard_board();
    let mut effects = standard_effects(&env);
    let with = differential("after_effect_removed", &effects, &env);
    assert!(effects.remove(eid(eids::STEAL)));
    let without = scenario("after_effect_removed", &effects, &env);
    assert_ne!(with, without);
}

/// REQ-LAYER-005, REQ-CORE-001: the standard board after a sequence of declared changes on one
/// warm cache (each step compared with the uncached digest), final digest recorded.
#[test]
fn diff_after_declared_changes() {
    let mut env = standard_board();
    let mut effects = standard_effects(&env);
    let mut cache = LayerCache::new();
    let check =
        |cache: &mut LayerCache, effects: &ContinuousEffects, env: &CacheEnv, what: &str| {
            let c = cached_digest(cache, effects, env);
            let u = fresh_digest(effects, env);
            same_digests(what, &c, &u).unwrap_or_else(|e| panic!("{e}"));
        };
    check(&mut cache, &effects, &env, "start");
    env.set_controller(oid(ids::VANILLA), P2);
    cache.invalidate(Invalidation::ControlChange);
    check(&mut cache, &effects, &env, "control");
    env.tap(oid(ids::HOST), true);
    cache.invalidate(Invalidation::Status);
    check(&mut cache, &effects, &env, "status");
    effects
        .register(
            eid(400),
            eff(Layer::L7c, 90, by_sel(SEL_ALL), modify_pt(1, 1)),
        )
        .expect("register");
    check(&mut cache, &effects, &env, "registration");
    env.set_player_value(P2, PV_RANK, 40);
    cache.invalidate(Invalidation::PlayerValue);
    check(&mut cache, &effects, &env, "player value");
    scenario("after_declared_changes", &effects, &env);
}

// ---------------------------------------------------------------------------------------------
// stale-cache detection
// ---------------------------------------------------------------------------------------------

/// A deliberately broken cache written for this test (not `LayerCache`): it memoises every
/// evaluation by object id and never invalidates.
#[derive(Default)]
struct StaleMemo {
    memo: BTreeMap<ObjectId, Evaluated>,
}

impl StaleMemo {
    fn evaluate_all(
        &mut self,
        effects: &ContinuousEffects,
        env: &dyn LayerEnvironment,
    ) -> Result<BTreeMap<ObjectId, Evaluated>, LayerError> {
        let mut out = BTreeMap::new();
        for id in env.objects() {
            let v = match self.memo.get(&id) {
                Some(v) => v.clone(),
                None => {
                    let v = evaluate(effects, env, id)?;
                    self.memo.insert(id, v.clone());
                    v
                }
            };
            out.insert(id, v);
        }
        Ok(out)
    }
}

/// REQ-LAYER-005: the differential comparison detects a stale cache. After warming and a
/// mutation (a control change, then an added effect), the stale memo's digest is reported as a
/// mismatch, while the real `LayerCache` with the proper invalidation matches.
#[test]
fn stale_cache_is_detected_by_the_differential() {
    let mut env = standard_board();
    let mut effects = standard_effects(&env);
    let mut stale = StaleMemo::default();
    let mut real = LayerCache::new();
    let d0 = fresh_digest(&effects, &env);
    let s0 = evaluated_digest(&stale.evaluate_all(&effects, &env).expect("ok"));
    assert!(
        same_digests("warm", &s0, &d0).is_ok(),
        "the memo is right before any change"
    );
    assert!(same_digests("warm", &cached_digest(&mut real, &effects, &env), &d0).is_ok());

    // control change
    env.set_controller(oid(ids::VANILLA), P2);
    let d1 = fresh_digest(&effects, &env);
    assert_ne!(d0, d1, "the change is observable");
    let s1 = evaluated_digest(&stale.evaluate_all(&effects, &env).expect("ok"));
    let mismatch = same_digests("control change", &s1, &d1);
    assert!(mismatch.is_err(), "the stale memo must be reported");
    assert!(mismatch.unwrap_err().contains("control change"));
    real.invalidate(Invalidation::ControlChange);
    assert_eq!(
        same_digests(
            "control change",
            &cached_digest(&mut real, &effects, &env),
            &d1
        ),
        Ok(())
    );

    // an added effect
    effects
        .register(
            eid(401),
            eff(Layer::L7c, 91, by_sel(SEL_CREATURE), modify_pt(3, 0)),
        )
        .expect("register");
    let d2 = fresh_digest(&effects, &env);
    assert_ne!(d1, d2);
    let s2 = evaluated_digest(&stale.evaluate_all(&effects, &env).expect("ok"));
    assert!(same_digests("added effect", &s2, &d2).is_err());
    assert_eq!(
        same_digests(
            "added effect",
            &cached_digest(&mut real, &effects, &env),
            &d2
        ),
        Ok(())
    );
}

/// REQ-LAYER-005: a registry change (a new layer-2 effect on a locked set) after warming: the
/// stale memo is reported as a mismatch, while the real `LayerCache` detects the registry change
/// by itself (no declaration needed) and matches.
#[test]
fn stale_memo_mismatch_on_registry_change() {
    let env = standard_board();
    let mut effects = standard_effects(&env);
    let mut stale = StaleMemo::default();
    let mut real = LayerCache::new();
    stale.evaluate_all(&effects, &env).expect("ok");
    real.evaluate_all(&effects, &env).expect("ok");
    // the registry changes: the real cache detects it on its own, the memo does not
    effects
        .register(
            eid(402),
            eff(
                Layer::L2,
                92,
                Affected::Objects([oid(ids::FLIER)].into_iter().collect()),
                LayerModification::SetController(P4),
            ),
        )
        .expect("register");
    let truth = fresh_digest(&effects, &env);
    let s = evaluated_digest(&stale.evaluate_all(&effects, &env).expect("ok"));
    assert!(same_digests("registry", &s, &truth).is_err());
    assert_eq!(
        same_digests(
            "registry",
            &cached_digest(&mut real, &effects, &env),
            &truth
        ),
        Ok(())
    );
}
