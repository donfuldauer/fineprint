// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared synthetic environment and helpers for the post-layer pass spec tests
//! (REQ-LAYER-004, REQ-DSL-008, REQ-CORE-001; CR 613.10, 613.11). Black-box: written from the
//! work-item brief only.
//!
//! Each `spec_layers_post_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. Nothing here names a real card (R5).
//!
//! The test environment decides what a player selector matches (the brief leaves it to the
//! caller): `ALL_PLAYERS` matches everyone, `{"ref":"CONTROLLER"}` matches the effect's
//! controller, `{"ref":"EACH_OPPONENT"}` matches everyone else, `TEAMMATES` matches nobody, and
//! every other reference matches nobody.
//!
//! JSON parameter values are built through MessagePack (`rmp_serde`), so no `serde_json`
//! dependency is needed: [`jv`] turns any serialisable value into the parameter value type.

#![allow(dead_code)]

use std::collections::BTreeSet;

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    EffectOrigin, PlayerAffected, PlayerPassEffect, PlayerProperties, PostLayerEffects,
    PostLayerEnvironment, RuleEffect, RuleEntry, RuleSet,
};
use fineprint_dsl::ability::{Modification, RuleKind, RuleModification};
use fineprint_dsl::expr::{PlayerGroup, PlayerSelectorAST, ReferenceAST};
use serde::Serialize;
use serde::de::DeserializeOwned;

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);
pub const P4: PlayerId = PlayerId::new(4);
/// A player the environment does not know.
pub const P9: PlayerId = PlayerId::new(9);

pub fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

pub fn eid(n: u64) -> EffectId {
    EffectId::new(n)
}

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

pub fn set(ps: &[PlayerId]) -> BTreeSet<PlayerId> {
    ps.iter().copied().collect()
}

// ---------------------------------------------------------------------------------------------
// JSON parameter values
// ---------------------------------------------------------------------------------------------

/// Converts any serialisable value to a JSON parameter value (via MessagePack).
pub fn jv<T: Serialize + ?Sized, V: DeserializeOwned>(t: &T) -> V {
    let bytes = rmp_serde::to_vec(t).expect("serialise test value");
    rmp_serde::from_slice(&bytes).expect("deserialise test value")
}

// ---------------------------------------------------------------------------------------------
// selectors and the test environment
// ---------------------------------------------------------------------------------------------

pub fn all_players() -> PlayerSelectorAST {
    PlayerSelectorAST::Players(PlayerGroup::AllPlayers)
}

pub fn you() -> PlayerSelectorAST {
    PlayerSelectorAST::Ref(ReferenceAST::Controller)
}

pub fn opponents() -> PlayerSelectorAST {
    PlayerSelectorAST::Ref(ReferenceAST::EachOpponent)
}

pub fn nobody() -> PlayerSelectorAST {
    PlayerSelectorAST::Players(PlayerGroup::Teammates)
}

/// The synthetic post-layer environment.
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// The players in the game, in turn order.
    pub players: Vec<PlayerId>,
    /// Effects whose existence check fails.
    pub missing: BTreeSet<EffectId>,
    /// `(a, b)`: a depends on b (CR 613.8a).
    pub deps: BTreeSet<(EffectId, EffectId)>,
}

impl Env {
    pub fn new(players: &[PlayerId]) -> Env {
        Env {
            players: players.to_vec(),
            missing: BTreeSet::new(),
            deps: BTreeSet::new(),
        }
    }

    pub fn two() -> Env {
        Env::new(&[P1, P2])
    }

    pub fn depends(mut self, a: u64, b: u64) -> Env {
        self.deps.insert((eid(a), eid(b)));
        self
    }

    pub fn without(mut self, id: u64) -> Env {
        self.missing.insert(eid(id));
        self
    }
}

/// What the test environment says a selector matches.
pub fn selector_matches(
    sel: &PlayerSelectorAST,
    controller: PlayerId,
    candidate: PlayerId,
) -> bool {
    match sel {
        PlayerSelectorAST::Players(PlayerGroup::AllPlayers) => true,
        PlayerSelectorAST::Players(PlayerGroup::Teammates) => false,
        PlayerSelectorAST::Ref(ReferenceAST::Controller) => candidate == controller,
        PlayerSelectorAST::Ref(ReferenceAST::EachOpponent) => candidate != controller,
        PlayerSelectorAST::Ref(_) => false,
    }
}

impl PostLayerEnvironment for Env {
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
        self.players.contains(&candidate) && selector_matches(selector, controller, candidate)
    }

    fn effect_exists(&self, id: EffectId) -> bool {
        !self.missing.contains(&id)
    }

    fn depends_on(&self, a: EffectId, b: EffectId) -> bool {
        self.deps.contains(&(a, b))
    }
}

/// An environment that keeps every default hook (no dependency, everything exists).
pub struct PlainEnv(pub Vec<PlayerId>);

impl PostLayerEnvironment for PlainEnv {
    fn players(&self) -> Vec<PlayerId> {
        self.0.clone()
    }

    fn matches_player(
        &self,
        selector: &PlayerSelectorAST,
        controller: PlayerId,
        _source: Option<ObjectId>,
        candidate: PlayerId,
    ) -> bool {
        self.0.contains(&candidate) && selector_matches(selector, controller, candidate)
    }
}

// ---------------------------------------------------------------------------------------------
// effect builders
// ---------------------------------------------------------------------------------------------

pub fn statik() -> EffectOrigin {
    EffectOrigin::Static { granted_at: None }
}

/// A pass A modification with a synthetic op.
pub fn pmod(op: &str) -> Modification {
    Modification::new(op)
}

/// A pass A effect from a static ability, controlled by `P1`, from object 100.
pub fn player_effect(t: u64, affected: PlayerAffected, op: &str) -> PlayerPassEffect {
    PlayerPassEffect {
        source: Some(oid(100)),
        controller: P1,
        timestamp: ts(t),
        origin: statik(),
        affected,
        modification: pmod(op),
    }
}

pub fn fixed(ps: &[PlayerId]) -> PlayerAffected {
    PlayerAffected::Fixed(set(ps))
}

pub fn sel(s: PlayerSelectorAST) -> PlayerAffected {
    PlayerAffected::Selector(s)
}

/// A rule modification of `kind` with no params.
pub fn rmod(kind: RuleKind) -> RuleModification {
    RuleModification::new(kind)
}

/// A rule modification with a string `"action"`.
pub fn rmod_action(kind: RuleKind, action: &str) -> RuleModification {
    let mut m = RuleModification::new(kind);
    m.params.insert("action".to_owned(), jv(action));
    m
}

/// A rule modification with one integer param.
pub fn rmod_int(kind: RuleKind, key: &str, n: i64) -> RuleModification {
    let mut m = RuleModification::new(kind);
    m.params.insert(key.to_owned(), jv(&n));
    m
}

/// A rule effect from a static ability controlled by `P1`, from object 100.
pub fn rule_effect(t: u64, players: PlayerAffected, modification: RuleModification) -> RuleEffect {
    RuleEffect {
        source: Some(oid(100)),
        controller: P1,
        timestamp: ts(t),
        origin: statik(),
        players,
        modification,
    }
}

/// Registers rule effects `(id, effect)` in the given order; panics on an error.
pub fn rules_registry(items: Vec<(u64, RuleEffect)>) -> PostLayerEffects {
    let mut reg = PostLayerEffects::new();
    for (id, e) in items {
        reg.register_rule(eid(id), e).expect("register rule effect");
    }
    reg
}

// ---------------------------------------------------------------------------------------------
// observations (compared without relying on derives the brief does not promise)
// ---------------------------------------------------------------------------------------------

/// The ids applied to `p`, in application order.
pub fn applied_ids(props: &PlayerProperties, p: PlayerId) -> Vec<EffectId> {
    props.applied(p).iter().map(|a| a.id).collect()
}

/// `(id, modification)` applied to `p`, in application order.
pub fn applied_full(props: &PlayerProperties, p: PlayerId) -> Vec<(EffectId, Modification)> {
    props
        .applied(p)
        .iter()
        .map(|a| (a.id, a.modification.clone()))
        .collect()
}

/// Every field of a rule entry.
pub type EntryView = (
    EffectId,
    BTreeSet<PlayerId>,
    PlayerId,
    Option<ObjectId>,
    Timestamp,
    RuleModification,
);

pub fn entry_view(e: &RuleEntry) -> EntryView {
    (
        e.id,
        e.players.clone(),
        e.controller,
        e.source,
        e.timestamp,
        e.modification.clone(),
    )
}

pub fn entries_view(rs: &RuleSet, kind: RuleKind) -> Vec<EntryView> {
    rs.entries(kind).iter().map(entry_view).collect()
}

pub fn entry_ids(rs: &RuleSet, kind: RuleKind) -> Vec<EffectId> {
    rs.entries(kind).iter().map(|e| e.id).collect()
}

pub fn ids(es: &[RuleEntry]) -> Vec<EffectId> {
    es.iter().map(|e| e.id).collect()
}

pub fn eids(ns: &[u64]) -> Vec<EffectId> {
    ns.iter().copied().map(eid).collect()
}

// ---------------------------------------------------------------------------------------------
// seeded PRNG (SplitMix64; no clock, R3)
// ---------------------------------------------------------------------------------------------

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.below(u64::try_from(len).unwrap())).unwrap()
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.index(i + 1);
            items.swap(i, j);
        }
    }
}

/// Every permutation of `0..n` (small `n`).
pub fn permutations(n: usize) -> Vec<Vec<usize>> {
    fn go(prefix: &mut Vec<usize>, rest: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if rest.is_empty() {
            out.push(prefix.clone());
            return;
        }
        for i in 0..rest.len() {
            let x = rest.remove(i);
            prefix.push(x);
            go(prefix, rest, out);
            prefix.pop();
            rest.insert(i, x);
        }
    }
    let mut out = Vec::new();
    go(&mut Vec::new(), &mut (0..n).collect(), &mut out);
    out
}
