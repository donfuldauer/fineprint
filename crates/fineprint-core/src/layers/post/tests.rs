// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the passes after the layers: REQ-LAYER-004 (pass A, pass B, prohibitions last,
// maximum hand size, land plays, the CR 601.2f hook), REQ-DSL-008 (the DSL PlayerEffect and
// RuleModification shapes), REQ-DSL-013 (no card-specific branch: only kinds and params are
// read), REQ-CORE-001 (registration order never changes a result). Synthetic ids and neutral
// names only.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::ability::{Modification, RuleKind, RuleModification};
use fineprint_dsl::expr::{PlayerGroup, PlayerSelectorAST};

use super::*;
use crate::layers::{EffectOrigin, LayerError};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

fn players<const N: usize>(ids: [u64; N]) -> PlayerAffected {
    PlayerAffected::Fixed(ids.iter().map(|&n| p(n)).collect())
}

fn all_players() -> PlayerAffected {
    PlayerAffected::Selector(PlayerSelectorAST::Players(PlayerGroup::AllPlayers))
}

/// In [`TestEnv`], `TEAMMATES` matches exactly the effect's controller.
fn teammates() -> PlayerAffected {
    PlayerAffected::Selector(PlayerSelectorAST::Players(PlayerGroup::Teammates))
}

const STATIC: EffectOrigin = EffectOrigin::Static { granted_at: None };

/// A synthetic environment: players 1-3, optional missing effects and dependencies.
struct TestEnv {
    players: Vec<PlayerId>,
    missing: BTreeSet<EffectId>,
    /// `(a, b)`: a depends on b.
    deps: BTreeSet<(EffectId, EffectId)>,
}

impl TestEnv {
    fn new() -> TestEnv {
        TestEnv {
            players: vec![p(1), p(2), p(3)],
            missing: BTreeSet::new(),
            deps: BTreeSet::new(),
        }
    }

    fn dep(mut self, a: u64, b: u64) -> TestEnv {
        self.deps.insert((e(a), e(b)));
        self
    }
}

impl PostLayerEnvironment for TestEnv {
    fn players(&self) -> Vec<PlayerId> {
        self.players.clone()
    }

    fn matches_player(
        &self,
        selector: &PlayerSelectorAST,
        controller: PlayerId,
        _source: Option<ObjectId>,
        candidate: PlayerId,
    ) -> bool {
        match selector {
            PlayerSelectorAST::Players(PlayerGroup::AllPlayers) => true,
            PlayerSelectorAST::Players(PlayerGroup::Teammates) => candidate == controller,
            PlayerSelectorAST::Ref(_) => false,
        }
    }

    fn effect_exists(&self, id: EffectId) -> bool {
        !self.missing.contains(&id)
    }

    fn depends_on(&self, a: EffectId, b: EffectId) -> bool {
        self.deps.contains(&(a, b))
    }
}

/// An environment relying on the trait's default `effect_exists` and `depends_on`.
struct PlainEnv;

impl PostLayerEnvironment for PlainEnv {
    fn players(&self) -> Vec<PlayerId> {
        vec![p(1), p(2)]
    }

    fn matches_player(
        &self,
        _selector: &PlayerSelectorAST,
        _controller: PlayerId,
        _source: Option<ObjectId>,
        _candidate: PlayerId,
    ) -> bool {
        true
    }
}

fn player_effect(t: u64, affected: PlayerAffected, op: &str) -> PlayerPassEffect {
    PlayerPassEffect {
        source: Some(ObjectId::new(100)),
        controller: p(2),
        timestamp: ts(t),
        origin: STATIC,
        affected,
        modification: Modification::new(op),
    }
}

fn rule_effect(t: u64, affected: PlayerAffected, modification: RuleModification) -> RuleEffect {
    RuleEffect {
        source: Some(ObjectId::new(200)),
        controller: p(2),
        timestamp: ts(t),
        origin: STATIC,
        players: affected,
        modification,
    }
}

fn with_int(mut m: RuleModification, key: &str, v: i64) -> RuleModification {
    m.params.insert(key.to_owned(), v.into());
    m
}

fn with_str(mut m: RuleModification, key: &str, v: &str) -> RuleModification {
    m.params.insert(key.to_owned(), v.into());
    m
}

fn with_bool(mut m: RuleModification, key: &str, v: bool) -> RuleModification {
    m.params.insert(key.to_owned(), v.into());
    m
}

fn action(kind: RuleKind, act: &str) -> RuleModification {
    with_str(RuleModification::new(kind), "action", act)
}

fn applied_ids(props: &PlayerProperties, player: u64) -> Vec<u64> {
    props
        .applied(p(player))
        .iter()
        .map(|a| a.id.get())
        .collect()
}

fn entry_ids(entries: &[RuleEntry]) -> Vec<u64> {
    entries.iter().map(|x| x.id.get()).collect()
}

fn reg_players(list: Vec<(u64, PlayerPassEffect)>) -> PostLayerEffects {
    let mut fx = PostLayerEffects::new();
    for (id, effect) in list {
        fx.register_player(e(id), effect).expect("register player");
    }
    fx
}

fn reg_rules(list: Vec<(u64, RuleEffect)>) -> PostLayerEffects {
    let mut fx = PostLayerEffects::new();
    for (id, effect) in list {
        fx.register_rule(e(id), effect).expect("register rule");
    }
    fx
}

fn bounded(rng: &mut Pcg64Dxsm, bound: u64) -> u64 {
    rng.next_bounded(bound).unwrap_or(0)
}

fn idx(n: u64) -> usize {
    usize::try_from(n).expect("small index")
}

// ---------------------------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004: one id names at most one effect across both maps; remove, len, is_empty.
#[test]
fn req_layer_004_registry_duplicates_remove_len() {
    let mut fx = PostLayerEffects::new();
    assert!(fx.is_empty());
    assert_eq!(fx.len(), 0);
    let pe = player_effect(1, players([1]), "TEST_OP");
    let re = rule_effect(1, players([1]), RuleModification::new(RuleKind::Skip));
    fx.register_player(e(1), pe.clone()).unwrap();
    fx.register_rule(e(2), re.clone()).unwrap();
    assert_eq!(fx.len(), 2);
    assert!(!fx.is_empty());
    // Duplicate across the two maps, both directions, and within one map.
    assert_eq!(
        fx.register_rule(e(1), re.clone()),
        Err(LayerError::DuplicateEffect(e(1)))
    );
    assert_eq!(
        fx.register_player(e(2), pe.clone()),
        Err(LayerError::DuplicateEffect(e(2)))
    );
    assert_eq!(
        fx.register_player(e(1), pe.clone()),
        Err(LayerError::DuplicateEffect(e(1)))
    );
    assert_eq!(
        fx.register_rule(e(2), re.clone()),
        Err(LayerError::DuplicateEffect(e(2)))
    );
    assert_eq!(fx.len(), 2);
    assert_eq!(fx.player_effect(e(1)), Some(&pe));
    assert_eq!(fx.rule_effect(e(2)), Some(&re));
    assert_eq!(fx.player_effect(e(2)), None);
    assert_eq!(fx.rule_effect(e(1)), None);
    assert_eq!(fx.player_effects().count(), 1);
    assert_eq!(fx.rule_effects().count(), 1);
    assert!(fx.remove(e(1)));
    assert!(!fx.remove(e(1)));
    assert!(fx.remove(e(2)));
    assert!(!fx.remove(e(3)));
    assert!(fx.is_empty());
    // A removed id can be used again.
    fx.register_rule(e(1), re).unwrap();
    assert_eq!(fx.len(), 1);
}

/// REQ-LAYER-004 (CR 611.2c, D4): a resolved player effect with a selector is rejected and the
/// registry unchanged; a resolved fixed set and a static selector are accepted; rule effects of
/// any origin may use a selector.
#[test]
fn req_layer_004_resolved_player_selector_rejected() {
    let mut fx = PostLayerEffects::new();
    let mut resolved = player_effect(1, all_players(), "TEST_OP");
    resolved.origin = EffectOrigin::Resolved;
    assert!(matches!(
        fx.register_player(e(1), resolved.clone()),
        Err(LayerError::Unsupported(_))
    ));
    assert!(fx.is_empty());
    resolved.affected = players([1]);
    fx.register_player(e(1), resolved).unwrap();
    fx.register_player(e(2), player_effect(1, all_players(), "TEST_OP"))
        .unwrap();
    let mut rule = rule_effect(1, all_players(), action(RuleKind::Prohibition, "CAST"));
    rule.origin = EffectOrigin::Resolved;
    fx.register_rule(e(3), rule).unwrap();
    assert_eq!(fx.len(), 3);
    // The resolved rule effect applies to whatever matches when the set is built (CR 611.2c).
    let set = build_rule_set(&fx, &TestEnv::new());
    assert_eq!(
        set.entries(RuleKind::Prohibition)[0].players,
        [p(1), p(2), p(3)].into_iter().collect::<BTreeSet<_>>()
    );
}

// ---------------------------------------------------------------------------------------------
// Pass A (CR 613.10)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004: pass A applies player effects in timestamp order (CR 613.10, 613.7).
#[test]
fn req_layer_004_pass_a_timestamp_order() {
    let fx = reg_players(vec![
        (1, player_effect(5, players([1]), "OP_A")),
        (2, player_effect(3, players([1]), "OP_B")),
        (3, player_effect(4, players([1]), "OP_C")),
    ]);
    let props = compute_player_properties(&fx, &TestEnv::new());
    assert_eq!(applied_ids(&props, 1), vec![2, 3, 1]);
    let ops: Vec<_> = props
        .applied(p(1))
        .iter()
        .map(|a| a.modification.op.clone())
        .collect();
    assert_eq!(ops, vec!["OP_B", "OP_C", "OP_A"]);
    assert!(props.applied(p(2)).is_empty());
    assert_eq!(props.iter().count(), 1);
}

/// REQ-LAYER-004, REQ-CORE-001: equal timestamps are applied by ascending EffectId (CR 613.7).
#[test]
fn req_layer_004_pass_a_tie_broken_by_effect_id() {
    let fx = reg_players(vec![
        (9, player_effect(4, players([1]), "OP")),
        (2, player_effect(4, players([1]), "OP")),
        (5, player_effect(4, players([1]), "OP")),
    ]);
    let props = compute_player_properties(&fx, &TestEnv::new());
    assert_eq!(applied_ids(&props, 1), vec![2, 5, 9]);
}

/// REQ-LAYER-004: fixed sets are used as stored (players not in the game ignored); selectors
/// are matched against every player at compute time (CR 613.10).
#[test]
fn req_layer_004_pass_a_fixed_and_selector() {
    let fx = reg_players(vec![
        (1, player_effect(1, players([1, 99]), "FIXED")),
        (2, player_effect(2, all_players(), "ALL")),
        (3, player_effect(3, teammates(), "TEAM")),
    ]);
    let props = compute_player_properties(&fx, &TestEnv::new());
    assert_eq!(applied_ids(&props, 1), vec![1, 2]);
    assert_eq!(applied_ids(&props, 2), vec![2, 3]);
    assert_eq!(applied_ids(&props, 3), vec![2]);
    assert!(props.applied(p(99)).is_empty());
    // The trait defaults: every effect exists, no dependency.
    let plain = compute_player_properties(&fx, &PlainEnv);
    assert_eq!(applied_ids(&plain, 1), vec![1, 2, 3]);
    assert_eq!(applied_ids(&plain, 2), vec![2, 3]);
}

/// REQ-LAYER-004: an effect whose static ability no longer exists is dropped (CR 613.8a(b)).
#[test]
fn req_layer_004_pass_a_effect_exists_false_drops() {
    let fx = reg_players(vec![
        (1, player_effect(1, players([1]), "OP")),
        (2, player_effect(2, players([1]), "OP")),
    ]);
    let mut env = TestEnv::new();
    env.missing.insert(e(1));
    let props = compute_player_properties(&fx, &env);
    assert_eq!(applied_ids(&props, 1), vec![2]);
}

/// REQ-LAYER-004: a dependent effect waits until just after its dependency even with an
/// earlier timestamp (CR 613.8a, 613.8b).
#[test]
fn req_layer_004_pass_a_dependency_reorders() {
    let fx = reg_players(vec![
        (1, player_effect(1, players([1]), "OP")),
        (2, player_effect(2, players([1]), "OP")),
        (3, player_effect(3, players([1]), "OP")),
    ]);
    let env = TestEnv::new().dep(1, 2);
    assert_eq!(
        applied_ids(&compute_player_properties(&fx, &env), 1),
        vec![2, 1, 3]
    );
    // Depending on several effects: waits for all of them.
    let env = TestEnv::new().dep(1, 2).dep(1, 3);
    assert_eq!(
        applied_ids(&compute_player_properties(&fx, &env), 1),
        vec![2, 3, 1]
    );
    // A dependency on a dropped effect does not hold anything back.
    let mut env = TestEnv::new().dep(1, 3);
    env.missing.insert(e(3));
    assert_eq!(
        applied_ids(&compute_player_properties(&fx, &env), 1),
        vec![1, 2]
    );
    // An effect never waits on itself.
    let env = TestEnv::new().dep(2, 2);
    assert_eq!(
        applied_ids(&compute_player_properties(&fx, &env), 1),
        vec![1, 2, 3]
    );
}

/// REQ-LAYER-004: a dependency loop is applied in timestamp order (CR 613.8b); an effect that
/// depends on a loop member still waits for it.
#[test]
fn req_layer_004_pass_a_dependency_loop_timestamp_order() {
    let fx = reg_players(vec![
        (1, player_effect(2, players([1]), "OP")),
        (2, player_effect(1, players([1]), "OP")),
    ]);
    let env = TestEnv::new().dep(1, 2).dep(2, 1);
    assert_eq!(
        applied_ids(&compute_player_properties(&fx, &env), 1),
        vec![2, 1]
    );
    // Loop {1, 2} with no way out; 3 (earliest timestamp) depends on 1 so it is not a loop
    // member and waits until after 1.
    let fx = reg_players(vec![
        (1, player_effect(3, players([1]), "OP")),
        (2, player_effect(2, players([1]), "OP")),
        (3, player_effect(1, players([1]), "OP")),
    ]);
    let env = TestEnv::new().dep(1, 2).dep(2, 1).dep(3, 1);
    assert_eq!(
        applied_ids(&compute_player_properties(&fx, &env), 1),
        vec![2, 1, 3]
    );
    // A three-member loop is applied in timestamp order.
    let env = TestEnv::new().dep(1, 2).dep(2, 3).dep(3, 1);
    assert_eq!(
        applied_ids(&compute_player_properties(&fx, &env), 1),
        vec![3, 2, 1]
    );
}

/// REQ-CORE-001, REQ-LAYER-004: the order of registration never changes pass A or pass B.
#[test]
fn req_core_001_registration_order_independent() {
    let player_list: Vec<(u64, PlayerPassEffect)> = (1..=8)
        .map(|i| {
            let affected = if i % 3 == 0 {
                all_players()
            } else {
                players([i % 3])
            };
            (i, player_effect(i % 4, affected, "OP"))
        })
        .collect();
    let rule_list: Vec<(u64, RuleEffect)> = (11..=18)
        .map(|i| {
            let kind = RuleKind::ALL[idx(i % 5)];
            (i, rule_effect(i % 3, all_players(), action(kind, "CAST")))
        })
        .collect();
    let env = TestEnv::new().dep(2, 5).dep(13, 17);
    let build = |pl: &[(u64, PlayerPassEffect)], rl: &[(u64, RuleEffect)]| {
        let mut fx = PostLayerEffects::new();
        for (id, x) in pl {
            fx.register_player(e(*id), x.clone()).unwrap();
        }
        for (id, x) in rl {
            fx.register_rule(e(*id), x.clone()).unwrap();
        }
        (
            compute_player_properties(&fx, &env),
            build_rule_set(&fx, &env),
        )
    };
    let reference = build(&player_list, &rule_list);
    for seed in 0..32u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let mut pl = player_list.clone();
        let mut rl = rule_list.clone();
        rng.shuffle(&mut pl);
        rng.shuffle(&mut rl);
        assert_eq!(build(&pl, &rl), reference, "seed {seed}");
    }
}

// ---------------------------------------------------------------------------------------------
// Pass B (CR 613.11)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004, REQ-DSL-008: pass B groups entries by RuleKind, each group in timestamp order
/// (CR 613.11, 613.7).
#[test]
fn req_layer_004_pass_b_grouping_and_order() {
    let r = |t, who, kind| rule_effect(t, who, RuleModification::new(kind));
    let fx = reg_rules(vec![
        (1, r(4, players([1]), RuleKind::Skip)),
        (2, r(2, players([1]), RuleKind::Skip)),
        (3, r(3, players([2]), RuleKind::DrawRule)),
        (4, r(1, players([1]), RuleKind::DrawRule)),
        (5, r(2, players([1]), RuleKind::Skip)),
    ]);
    let set = build_rule_set(&fx, &TestEnv::new());
    assert_eq!(entry_ids(set.entries(RuleKind::Skip)), vec![2, 5, 1]);
    assert_eq!(entry_ids(set.entries(RuleKind::DrawRule)), vec![4, 3]);
    assert!(set.entries(RuleKind::Evasion).is_empty());
    assert_eq!(set.position(e(4)), Some(0));
    assert_eq!(set.position(e(1)), Some(4));
    assert_eq!(set.position(e(9)), None);
    let skip_p1: Vec<_> = set
        .applying(RuleKind::Skip, p(1))
        .map(|x| x.id.get())
        .collect();
    assert_eq!(skip_p1, vec![2, 5, 1]);
    assert_eq!(set.applying(RuleKind::DrawRule, p(2)).count(), 1);
    let entry = &set.entries(RuleKind::DrawRule)[1];
    assert_eq!(entry.timestamp, ts(3));
    assert_eq!(entry.controller, p(2));
    assert_eq!(entry.source, Some(ObjectId::new(200)));
    assert_eq!(entry.players, [p(2)].into_iter().collect::<BTreeSet<_>>());
}

/// REQ-LAYER-004: pass B honours the dependency hook and drops effects that no longer exist
/// (CR 613.11, 613.8b, 613.8a).
#[test]
fn req_layer_004_pass_b_dependency_and_exists() {
    let r = |t| rule_effect(t, players([1]), RuleModification::new(RuleKind::Skip));
    let mut fx = reg_rules(vec![(1, r(1)), (2, r(2)), (3, r(3))]);
    let mut env = TestEnv::new().dep(1, 3);
    env.missing.insert(e(2));
    let set = build_rule_set(&fx, &env);
    assert_eq!(entry_ids(set.entries(RuleKind::Skip)), vec![3, 1]);
    // Dependencies are consulted only within one pass: a rule effect "depending on" a player
    // effect is not held back.
    fx.register_player(e(4), player_effect(9, players([1]), "OP"))
        .unwrap();
    let env = TestEnv::new().dep(1, 4);
    let set = build_rule_set(&fx, &env);
    assert_eq!(entry_ids(set.entries(RuleKind::Skip)), vec![1, 2, 3]);
}

// ---------------------------------------------------------------------------------------------
// Prohibitions last (CR 101.2)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004: a prohibition beats a permission in both timestamp orders (CR 101.2).
#[test]
fn req_layer_004_prohibition_beats_permission_both_orders() {
    for (prohibition_ts, permission_ts) in [(1, 2), (2, 1)] {
        let fx = reg_rules(vec![
            (
                1,
                rule_effect(
                    permission_ts,
                    players([1]),
                    action(RuleKind::CastPermission, "CAST"),
                ),
            ),
            (
                2,
                rule_effect(
                    prohibition_ts,
                    players([1]),
                    action(RuleKind::Prohibition, "CAST"),
                ),
            ),
        ]);
        let set = build_rule_set(&fx, &TestEnv::new());
        assert_eq!(
            set.query(p(1), "CAST", RuleKind::CastPermission),
            Verdict::Prohibited { by: e(2) }
        );
        // Not for another player, nor another action.
        assert_eq!(
            set.query(p(2), "CAST", RuleKind::CastPermission),
            Verdict::NoEffect
        );
        assert_eq!(
            set.query(p(1), "PLAY_LAND", RuleKind::CastPermission),
            Verdict::NoEffect
        );
    }
}

/// REQ-LAYER-004: query details: earliest applicable entry, action equality, entries without an
/// action, and the permission kind asked for (CR 101.2).
#[test]
fn req_layer_004_query_details() {
    let fx = reg_rules(vec![
        (
            1,
            rule_effect(5, players([1]), action(RuleKind::CastPermission, "CAST")),
        ),
        (
            2,
            rule_effect(3, players([1]), action(RuleKind::CastPermission, "CAST")),
        ),
        (
            3,
            rule_effect(
                1,
                players([1]),
                RuleModification::new(RuleKind::Prohibition),
            ),
        ),
        (
            4,
            rule_effect(1, players([1]), action(RuleKind::TimingPermission, "CAST")),
        ),
        (
            5,
            rule_effect(1, players([2]), action(RuleKind::Prohibition, "CAST")),
        ),
        (
            6,
            rule_effect(2, players([2]), action(RuleKind::Prohibition, "CAST")),
        ),
        (
            7,
            rule_effect(
                1,
                players([3]),
                with_int(RuleModification::new(RuleKind::Prohibition), "action", 3),
            ),
        ),
    ]);
    let set = build_rule_set(&fx, &TestEnv::new());
    // The prohibition without an action applies to no action.
    assert_eq!(
        set.query(p(1), "CAST", RuleKind::CastPermission),
        Verdict::Permitted { by: e(2) }
    );
    assert_eq!(
        set.query(p(1), "CAST", RuleKind::TimingPermission),
        Verdict::Permitted { by: e(4) }
    );
    assert_eq!(
        set.query(p(1), "CAST", RuleKind::PlayPermission),
        Verdict::NoEffect
    );
    assert_eq!(
        set.query(p(1), "cast", RuleKind::CastPermission),
        Verdict::NoEffect
    );
    // The earliest applicable prohibition is reported.
    assert_eq!(
        set.query(p(2), "CAST", RuleKind::CastPermission),
        Verdict::Prohibited { by: e(5) }
    );
    // A non-string action matches nothing.
    assert_eq!(
        set.query(p(3), "3", RuleKind::CastPermission),
        Verdict::NoEffect
    );
    let no_action: Vec<_> = set
        .entries(RuleKind::Prohibition)
        .iter()
        .filter(|x| x.action().is_none())
        .map(|x| x.id.get())
        .collect();
    assert_eq!(no_action, vec![3, 7]);
}

/// One random rule effect of the property test.
#[derive(Clone, Debug)]
struct Gen {
    id: u64,
    t: u64,
    kind: RuleKind,
    action: Option<&'static str>,
    players: BTreeSet<u64>,
    all: bool,
    count: i64,
}

const KINDS: [RuleKind; 6] = [
    RuleKind::Prohibition,
    RuleKind::CastPermission,
    RuleKind::PlayPermission,
    RuleKind::TimingPermission,
    RuleKind::AdditionalLandPlay,
    RuleKind::Skip,
];
const ACTIONS: [&str; 3] = ["CAST", "PLAY_LAND", "ATTACK"];

fn random_effects(rng: &mut Pcg64Dxsm) -> Vec<Gen> {
    let n = bounded(rng, 14);
    let mut ids: Vec<u64> = (1..=40).collect();
    rng.shuffle(&mut ids);
    (0..n)
        .map(|i| {
            let kind = KINDS[idx(bounded(rng, 6))];
            let a = bounded(rng, 4);
            let action = (a < 3).then(|| ACTIONS[idx(a)]);
            let players: BTreeSet<u64> = (1..=3).filter(|_| bounded(rng, 2) == 0).collect();
            Gen {
                id: ids[idx(i)],
                t: bounded(rng, 6),
                kind,
                action,
                players,
                all: bounded(rng, 4) == 0,
                count: i64::try_from(bounded(rng, 4)).expect("small"),
            }
        })
        .collect()
}

fn to_effect(g: &Gen) -> RuleEffect {
    let mut m = RuleModification::new(g.kind);
    if let Some(a) = g.action {
        m = with_str(m, "action", a);
    }
    m = with_int(m, "count", g.count);
    let affected = if g.all {
        all_players()
    } else {
        PlayerAffected::Fixed(g.players.iter().map(|&n| p(n)).collect())
    };
    rule_effect(g.t, affected, m)
}

fn applies(g: &Gen, player: u64) -> bool {
    g.all || g.players.contains(&player)
}

/// The earliest (timestamp, id) generated effect of `kind` with `act` applying to `player`.
fn oracle_first(gens: &[Gen], kind: RuleKind, player: u64, act: &str) -> Option<EffectId> {
    gens.iter()
        .filter(|g| g.kind == kind && g.action == Some(act) && applies(g, player))
        .min_by_key(|g| (g.t, g.id))
        .map(|g| e(g.id))
}

/// REQ-LAYER-004 property (acceptance criterion "Prohibition beats permission in every merged
/// query"): for random permissions, prohibitions and unrelated effects with random timestamps,
/// players and registration order, a matching prohibition always gives `Prohibited`, otherwise
/// a matching permission gives `Permitted`; a "PLAY_LAND" prohibition always gives 0 land plays
/// however many extra land plays exist (CR 101.2, 305.2, 305.2b).
#[test]
fn req_layer_004_property_prohibition_beats_permission() {
    let mut prohibited_seen = 0u32;
    let mut permitted_seen = 0u32;
    let mut land_zero_with_extra = 0u32;
    for seed in 0..512u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let gens = random_effects(&mut rng);
        let mut order = gens.clone();
        rng.shuffle(&mut order);
        let mut fx = PostLayerEffects::new();
        for g in &order {
            fx.register_rule(e(g.id), to_effect(g)).unwrap();
        }
        let set = build_rule_set(&fx, &TestEnv::new());
        for player in 1..=3u64 {
            for act in ACTIONS {
                for permission in [
                    RuleKind::CastPermission,
                    RuleKind::PlayPermission,
                    RuleKind::TimingPermission,
                ] {
                    let verdict = set.query(p(player), act, permission);
                    let expected =
                        if let Some(by) = oracle_first(&gens, RuleKind::Prohibition, player, act) {
                            prohibited_seen += 1;
                            Verdict::Prohibited { by }
                        } else if let Some(by) = oracle_first(&gens, permission, player, act) {
                            permitted_seen += 1;
                            Verdict::Permitted { by }
                        } else {
                            Verdict::NoEffect
                        };
                    assert_eq!(verdict, expected, "seed {seed} player {player} {act}");
                }
            }
            let extra: i64 = gens
                .iter()
                .filter(|g| g.kind == RuleKind::AdditionalLandPlay && applies(g, player))
                .map(|g| g.count)
                .sum();
            let land = set.land_plays_allowed(p(player));
            if oracle_first(&gens, RuleKind::Prohibition, player, "PLAY_LAND").is_some() {
                assert_eq!(land, 0, "seed {seed} player {player}");
                if extra > 0 {
                    land_zero_with_extra += 1;
                }
            } else {
                assert_eq!(land, 1 + extra, "seed {seed} player {player}");
            }
        }
    }
    // The generator exercises every branch.
    assert!(prohibited_seen > 100, "{prohibited_seen}");
    assert!(permitted_seen > 100, "{permitted_seen}");
    assert!(land_zero_with_extra > 10, "{land_zero_with_extra}");
}

// ---------------------------------------------------------------------------------------------
// Maximum hand size (CR 402.2) and land plays (CR 305.2)
// ---------------------------------------------------------------------------------------------

fn hand(t: u64, who: PlayerAffected, key: &str, v: i64) -> RuleEffect {
    rule_effect(
        t,
        who,
        with_int(RuleModification::new(RuleKind::MaxHandSize), key, v),
    )
}

fn unlimited(t: u64, who: PlayerAffected) -> RuleEffect {
    rule_effect(
        t,
        who,
        with_bool(
            RuleModification::new(RuleKind::MaxHandSize),
            "unlimited",
            true,
        ),
    )
}

fn hand_size(list: Vec<(u64, RuleEffect)>, player: u64) -> Option<i64> {
    build_rule_set(&reg_rules(list), &TestEnv::new()).max_hand_size(p(player))
}

/// REQ-LAYER-004: maximum hand size: default 7, set, delta, unlimited (CR 402.2, 613.11).
#[test]
fn req_layer_004_max_hand_size_basic() {
    assert_eq!(hand_size(vec![], 1), Some(7));
    assert_eq!(DEFAULT_MAX_HAND_SIZE, 7);
    assert_eq!(
        hand_size(vec![(1, hand(1, players([1]), "set", 4))], 1),
        Some(4)
    );
    assert_eq!(
        hand_size(vec![(1, hand(1, players([1]), "delta", 2))], 1),
        Some(9)
    );
    assert_eq!(
        hand_size(vec![(1, hand(1, players([1]), "delta", -3))], 1),
        Some(4)
    );
    assert_eq!(hand_size(vec![(1, unlimited(1, players([1])))], 1), None);
}

/// REQ-LAYER-004: maximum hand size entries apply in timestamp order (CR 613.11, 613.7).
#[test]
fn req_layer_004_max_hand_size_order() {
    // Unlimited, then a later set.
    let list = vec![
        (1, hand(2, players([1]), "set", 5)),
        (2, unlimited(1, players([1]))),
    ];
    assert_eq!(hand_size(list, 1), Some(5));
    // Set, then unlimited.
    let list = vec![
        (1, hand(1, players([1]), "set", 5)),
        (2, unlimited(2, players([1]))),
    ];
    assert_eq!(hand_size(list, 1), None);
    // Set, then a later delta; a delta before the set is overwritten.
    let list = vec![
        (1, hand(2, players([1]), "set", 2)),
        (2, hand(3, players([1]), "delta", 3)),
        (3, hand(1, players([1]), "delta", 10)),
    ];
    assert_eq!(hand_size(list, 1), Some(5));
    // A delta when there is no maximum leaves no maximum.
    let list = vec![
        (1, unlimited(1, players([1]))),
        (2, hand(2, players([1]), "delta", 3)),
    ];
    assert_eq!(hand_size(list, 1), None);
}

/// REQ-LAYER-004: maximum hand size is per player, clamps at 0 and ignores malformed entries
/// (CR 402.2).
#[test]
fn req_layer_004_max_hand_size_scope_clamp_malformed() {
    let list = vec![
        (1, hand(1, players([1]), "set", 3)),
        (2, hand(2, all_players(), "delta", -1)),
    ];
    let set = build_rule_set(&reg_rules(list), &TestEnv::new());
    assert_eq!(set.max_hand_size(p(1)), Some(2));
    assert_eq!(set.max_hand_size(p(2)), Some(6));
    assert_eq!(set.max_hand_size(p(3)), Some(6));
    // Clamp to 0.
    assert_eq!(
        hand_size(vec![(1, hand(1, players([1]), "set", -4))], 1),
        Some(0)
    );
    assert_eq!(
        hand_size(vec![(1, hand(1, players([1]), "delta", -20))], 1),
        Some(0)
    );
    // Saturating arithmetic.
    let list = vec![
        (1, hand(1, players([1]), "set", i64::MAX)),
        (2, hand(2, players([1]), "delta", 5)),
    ];
    assert_eq!(hand_size(list, 1), Some(i64::MAX));
    // Malformed entries are ignored: no key, two keys, wrong types, unlimited false.
    let mhs = || RuleModification::new(RuleKind::MaxHandSize);
    let list = vec![
        (1, rule_effect(1, players([1]), mhs())),
        (
            2,
            rule_effect(
                2,
                players([1]),
                with_int(with_int(mhs(), "set", 1), "delta", 1),
            ),
        ),
        (3, rule_effect(3, players([1]), with_str(mhs(), "set", "2"))),
        (
            4,
            rule_effect(4, players([1]), with_bool(mhs(), "unlimited", false)),
        ),
        (
            5,
            rule_effect(5, players([1]), with_int(mhs(), "unlimited", 1)),
        ),
        (
            6,
            rule_effect(6, players([1]), with_bool(mhs(), "delta", true)),
        ),
    ];
    assert_eq!(hand_size(list, 1), Some(7));
}

fn land(t: u64, who: PlayerAffected, count: Option<i64>) -> RuleEffect {
    let m = RuleModification::new(RuleKind::AdditionalLandPlay);
    let m = match count {
        Some(c) => with_int(m, "count", c),
        None => m,
    };
    rule_effect(t, who, m)
}

/// REQ-LAYER-004: land plays: default 1, count defaulting to 1, sums, per player (CR 305.2).
#[test]
fn req_layer_004_land_plays() {
    let set = build_rule_set(&PostLayerEffects::new(), &TestEnv::new());
    assert_eq!(set.land_plays_allowed(p(1)), 1);
    assert_eq!(DEFAULT_LAND_PLAYS, 1);
    let list = vec![
        (1, land(1, players([1]), None)),
        (2, land(2, players([1]), Some(2))),
        (3, land(3, players([2]), Some(1))),
        (4, land(4, all_players(), Some(0))),
    ];
    let set = build_rule_set(&reg_rules(list), &TestEnv::new());
    assert_eq!(set.land_plays_allowed(p(1)), 4);
    assert_eq!(set.land_plays_allowed(p(2)), 2);
    assert_eq!(set.land_plays_allowed(p(3)), 1);
    // A negative or non-integer count counts 0.
    let odd = with_str(
        RuleModification::new(RuleKind::AdditionalLandPlay),
        "count",
        "two",
    );
    let list = vec![
        (1, land(1, players([1]), Some(-3))),
        (2, rule_effect(2, players([1]), odd)),
    ];
    let set = build_rule_set(&reg_rules(list), &TestEnv::new());
    assert_eq!(set.land_plays_allowed(p(1)), 1);
}

/// REQ-LAYER-004: a "PLAY_LAND" prohibition gives 0 land plays whatever the extra land plays
/// and timestamps (CR 305.2b, 101.2); a prohibition of another action does not.
#[test]
fn req_layer_004_land_plays_prohibited() {
    for prohibition_ts in [1, 10] {
        let list = vec![
            (1, land(5, players([1]), Some(3))),
            (2, land(6, players([1]), None)),
            (
                3,
                rule_effect(
                    prohibition_ts,
                    players([1]),
                    action(RuleKind::Prohibition, "PLAY_LAND"),
                ),
            ),
            (
                4,
                rule_effect(1, players([1, 2]), action(RuleKind::Prohibition, "CAST")),
            ),
            (
                5,
                rule_effect(
                    1,
                    players([1]),
                    action(RuleKind::PlayPermission, "PLAY_LAND"),
                ),
            ),
        ];
        let set = build_rule_set(&reg_rules(list), &TestEnv::new());
        assert_eq!(set.land_plays_allowed(p(1)), 0);
        assert_eq!(set.land_plays_allowed(p(2)), 1);
    }
}

// ---------------------------------------------------------------------------------------------
// The CR 601.2f hook
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004: cost stages separate grants, increases, reductions and total-cost effects,
/// each in application order, for the asking player only (CR 601.2b, 601.2f).
#[test]
fn req_layer_004_cost_stages() {
    let c = |t, who, kind| rule_effect(t, who, RuleModification::new(kind));
    let list = vec![
        (1, c(5, players([1]), RuleKind::CostReduction)),
        (2, c(2, players([1]), RuleKind::CostReduction)),
        (3, c(1, players([1]), RuleKind::CostIncrease)),
        (4, c(1, players([1]), RuleKind::CostSet)),
        (5, c(3, players([1]), RuleKind::CostMinimum)),
        (6, c(2, players([1]), RuleKind::CostMinimum)),
        (7, c(4, players([1]), RuleKind::AdditionalCostRequire)),
        (8, c(3, players([1]), RuleKind::AltCostGrant)),
        (9, c(6, players([1]), RuleKind::AltCostGrant)),
        (10, c(1, players([2]), RuleKind::CostReduction)),
        (11, c(1, players([1]), RuleKind::PaymentMethod)),
    ];
    let set = build_rule_set(&reg_rules(list), &TestEnv::new());
    let stages = set.cost_stages(p(1));
    assert_eq!(entry_ids(&stages.grants), vec![8, 7, 9]);
    assert_eq!(entry_ids(&stages.increases), vec![3]);
    assert_eq!(entry_ids(&stages.reductions), vec![2, 1]);
    assert_eq!(entry_ids(&stages.total_effects), vec![6, 5, 4]);
    let other = set.cost_stages(p(2));
    assert_eq!(entry_ids(&other.reductions), vec![10]);
    assert!(other.grants.is_empty() && other.increases.is_empty());
    assert!(other.total_effects.is_empty());
    assert_eq!(set.cost_stages(p(3)), CostStages::default());
}

/// REQ-LAYER-004: the player may order the reductions (CR 601.2f); a non-permutation is
/// rejected and leaves the list unchanged.
#[test]
fn req_layer_004_reorder_reductions() {
    let c = |t| {
        rule_effect(
            t,
            players([1]),
            RuleModification::new(RuleKind::CostReduction),
        )
    };
    let list = vec![(1, c(1)), (2, c(2)), (3, c(3))];
    let set = build_rule_set(&reg_rules(list), &TestEnv::new());
    let mut stages = set.cost_stages(p(1));
    assert_eq!(entry_ids(&stages.reductions), vec![1, 2, 3]);
    stages.reorder_reductions(&[e(3), e(1), e(2)]).unwrap();
    assert_eq!(entry_ids(&stages.reductions), vec![3, 1, 2]);
    let before = stages.clone();
    for bad in [
        vec![e(3), e(1)],
        vec![e(3), e(1), e(1)],
        vec![e(3), e(1), e(2), e(2)],
        vec![e(3), e(1), e(9)],
        vec![e(3), e(1), e(2), e(9)],
        vec![],
    ] {
        assert!(matches!(
            stages.reorder_reductions(&bad),
            Err(LayerError::Unsupported(_))
        ));
        assert_eq!(stages, before);
    }
    // The empty list accepts the empty permutation.
    let mut empty = CostStages::default();
    empty.reorder_reductions(&[]).unwrap();
    assert!(empty.reductions.is_empty());
}

/// REQ-DSL-013, REQ-DSL-008: kinds and params decide everything; the params map is the DSL's
/// opaque map (CR 101.2).
#[test]
fn req_dsl_013_params_only() {
    let mut m = RuleModification::new(RuleKind::Prohibition);
    let mut params = BTreeMap::new();
    params.insert("action".to_owned(), "CAST".into());
    m.params = params;
    let fx = reg_rules(vec![(1, rule_effect(1, all_players(), m))]);
    let set = build_rule_set(&fx, &TestEnv::new());
    for player in 1..=3 {
        assert_eq!(
            set.query(p(player), "CAST", RuleKind::CastPermission),
            Verdict::Prohibited { by: e(1) }
        );
    }
}
