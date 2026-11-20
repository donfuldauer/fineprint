// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Pass B: the effects that modify the rules of the game, as a [`RuleSet`] grouped by
//! [`RuleKind`] (REQ-LAYER-004, CR 613.11), with prohibitions evaluated last (CR 101.2).
//!
//! Parameter conventions of [`RuleModification::params`] read here (the DSL keeps them opaque):
//!
//! - `"action"` (string): the action a `PROHIBITION` or a permission kind is about, compared by
//!   string equality; an entry without a string `"action"` applies to no action.
//! - `ADDITIONAL_LAND_PLAY`: `"count"`, a non-negative JSON integer; absent means 1; any other
//!   value counts 0.
//! - `MAX_HAND_SIZE`: exactly one of `"set": n` (integer), `"delta": n` (integer) or
//!   `"unlimited": true`; any other entry is ignored.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::{RuleKind, RuleModification};

use super::cost::CostStages;
use super::effects::{PostLayerEffects, PostLayerEnvironment, resolve_players};
use super::order::application_order;

/// The maximum hand size before any effect (CR 402.2).
pub const DEFAULT_MAX_HAND_SIZE: i64 = 7;

/// The number of lands a player can play during their turn before any effect (CR 305.2).
pub const DEFAULT_LAND_PLAYS: i64 = 1;

/// The parameter naming the action of a prohibition or permission.
const ACTION: &str = "action";

/// The action of playing a land, as used by [`RuleSet::land_plays_allowed`].
const PLAY_LAND: &str = "PLAY_LAND";

/// One rule effect in the [`RuleSet`], with its players resolved when the set was built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleEntry {
    /// The effect's id.
    pub id: EffectId,
    /// The players whose rules it modifies, resolved when the rule set was built.
    pub players: BTreeSet<PlayerId>,
    /// The effect's controller.
    pub controller: PlayerId,
    /// The object whose ability generated the effect, if any.
    pub source: Option<ObjectId>,
    /// The effect's timestamp (CR 613.7).
    pub timestamp: Timestamp,
    /// The rule modification.
    pub modification: RuleModification,
}

impl RuleEntry {
    /// Whether the entry modifies the rules for `player`.
    #[must_use]
    pub fn applies_to(&self, player: PlayerId) -> bool {
        self.players.contains(&player)
    }

    /// The string `"action"` parameter, if present.
    #[must_use]
    pub fn action(&self) -> Option<&str> {
        self.modification
            .params
            .get(ACTION)
            .and_then(|v| v.as_str())
    }
}

/// The answer of [`RuleSet::query`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A prohibition applies (CR 101.2: "can't" beats "can"); `by` is the earliest applicable
    /// prohibition in application order.
    Prohibited {
        /// The prohibition's id.
        by: EffectId,
    },
    /// No prohibition applies and a permission does; `by` is the earliest applicable one.
    Permitted {
        /// The permission's id.
        by: EffectId,
    },
    /// Neither a prohibition nor a permission applies.
    NoEffect,
}

/// The result of pass B (CR 613.11): every rule effect in force, grouped by [`RuleKind`], each
/// group in application order.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RuleSet {
    groups: BTreeMap<RuleKind, Vec<RuleEntry>>,
    /// The application position of each entry, across all kinds.
    position: BTreeMap<EffectId, usize>,
}

impl RuleSet {
    /// The entries of `kind`, in application order; empty if none.
    #[must_use]
    pub fn entries(&self, kind: RuleKind) -> &[RuleEntry] {
        self.groups.get(&kind).map_or(&[], Vec::as_slice)
    }

    /// The entries of `kind` that modify the rules for `player`, in application order.
    pub fn applying(
        &self,
        kind: RuleKind,
        player: PlayerId,
    ) -> impl Iterator<Item = &RuleEntry> + '_ {
        self.entries(kind)
            .iter()
            .filter(move |e| e.applies_to(player))
    }

    /// The position of the entry `id` in the application order across all kinds, if present.
    #[must_use]
    pub fn position(&self, id: EffectId) -> Option<usize> {
        self.position.get(&id).copied()
    }

    /// Whether `player` may take `action` as far as `permission` entries and prohibitions say.
    ///
    /// CR 101.2: prohibitions are evaluated last, so any applicable `PROHIBITION` whose
    /// `"action"` is `action` wins over every permission, whatever their timestamps. Otherwise
    /// the earliest applicable entry of kind `permission` with that action permits it.
    #[must_use]
    pub fn query(&self, player: PlayerId, action: &str, permission: RuleKind) -> Verdict {
        let matching = |kind| {
            self.applying(kind, player)
                .find(|e| e.action() == Some(action))
                .map(|e| e.id)
        };
        // CR 101.2: the "can't" effect takes precedence.
        if let Some(by) = matching(RuleKind::Prohibition) {
            return Verdict::Prohibited { by };
        }
        match matching(permission) {
            Some(by) => Verdict::Permitted { by },
            None => Verdict::NoEffect,
        }
    }

    /// The maximum hand size of `player`; `None` means no maximum (CR 402.2).
    ///
    /// Starting from [`DEFAULT_MAX_HAND_SIZE`], the applicable `MAX_HAND_SIZE` entries apply in
    /// application order (CR 613.11): `"set": n` makes it `n`, `"delta": n` adds `n` (no effect
    /// when there is no maximum), `"unlimited": true` removes the maximum until a later set. An
    /// entry with none or more than one of these keys, or a value of the wrong type, is ignored.
    /// A final value below 0 is 0.
    #[must_use]
    pub fn max_hand_size(&self, player: PlayerId) -> Option<i64> {
        let mut size = Some(DEFAULT_MAX_HAND_SIZE);
        for entry in self.applying(RuleKind::MaxHandSize, player) {
            match hand_size_step(&entry.modification) {
                Some(HandSizeStep::Set(n)) => size = Some(n),
                Some(HandSizeStep::Delta(n)) => size = size.map(|s| s.saturating_add(n)),
                Some(HandSizeStep::Unlimited) => size = None,
                None => {}
            }
        }
        size.map(|s| s.max(0))
    }

    /// The number of lands `player` can play this turn (CR 305.2): 0 when a prohibition on the
    /// action `"PLAY_LAND"` applies (CR 305.2b, 101.2); otherwise [`DEFAULT_LAND_PLAYS`] plus
    /// the `"count"` of every applicable `ADDITIONAL_LAND_PLAY` entry.
    #[must_use]
    pub fn land_plays_allowed(&self, player: PlayerId) -> i64 {
        // CR 101.2, 305.2b: "can't play lands" beats any additional land play.
        if let Verdict::Prohibited { .. } = self.query(player, PLAY_LAND, RuleKind::PlayPermission)
        {
            return 0;
        }
        // CR 305.2: continuous effects may increase the number.
        self.applying(RuleKind::AdditionalLandPlay, player)
            .map(|e| land_play_count(&e.modification))
            .fold(DEFAULT_LAND_PLAYS, i64::saturating_add)
    }

    /// The cost effects that apply to `player`, in the stages of CR 601.2b and 601.2f (the hook
    /// for the casting procedure; no cost is computed here).
    #[must_use]
    pub fn cost_stages(&self, player: PlayerId) -> CostStages {
        CostStages {
            grants: self.merged(
                &[RuleKind::AltCostGrant, RuleKind::AdditionalCostRequire],
                player,
            ),
            increases: self.merged(&[RuleKind::CostIncrease], player),
            reductions: self.merged(&[RuleKind::CostReduction], player),
            total_effects: [RuleKind::CostMinimum, RuleKind::CostSet]
                .iter()
                .flat_map(|&k| self.applying(k, player).cloned())
                .collect(),
        }
    }

    /// The entries of `kinds` that apply to `player`, merged in application order.
    fn merged(&self, kinds: &[RuleKind], player: PlayerId) -> Vec<RuleEntry> {
        let mut out: Vec<RuleEntry> = kinds
            .iter()
            .flat_map(|&k| self.applying(k, player).cloned())
            .collect();
        out.sort_by_key(|e| self.position(e.id));
        out
    }
}

/// One step of the maximum hand size computation.
enum HandSizeStep {
    Set(i64),
    Delta(i64),
    Unlimited,
}

/// Reads a `MAX_HAND_SIZE` modification: exactly one recognised key with a valid value.
fn hand_size_step(m: &RuleModification) -> Option<HandSizeStep> {
    let set = m.params.get("set");
    let delta = m.params.get("delta");
    let unlimited = m.params.get("unlimited");
    match (set, delta, unlimited) {
        (Some(v), None, None) => v.as_i64().map(HandSizeStep::Set),
        (None, Some(v), None) => v.as_i64().map(HandSizeStep::Delta),
        (None, None, Some(v)) => (v.as_bool() == Some(true)).then_some(HandSizeStep::Unlimited),
        _ => None,
    }
}

/// The number of additional land plays of an `ADDITIONAL_LAND_PLAY` modification: `"count"`
/// when it is a non-negative integer, 1 when absent, 0 otherwise.
fn land_play_count(m: &RuleModification) -> i64 {
    match m.params.get("count") {
        None => 1,
        Some(v) => v.as_i64().filter(|&n| n >= 0).unwrap_or(0),
    }
}

/// Pass B (CR 613.11): builds the [`RuleSet`] from the rule effects of `effects`, applied after
/// all other continuous effects in timestamp order with the dependency hook (CR 613.7, 613.8b).
/// Each entry's players are resolved now: a fixed set intersected with the players of `env`, a
/// selector matched against them whatever the effect's origin (CR 611.2c). Effects whose source
/// no longer has the ability ([`PostLayerEnvironment::effect_exists`]) are skipped.
#[must_use]
pub fn build_rule_set(effects: &PostLayerEffects, env: &dyn PostLayerEnvironment) -> RuleSet {
    let keys: Vec<_> = effects
        .rule_effects()
        .map(|(id, e)| (e.timestamp, id))
        .collect();
    let players: BTreeSet<PlayerId> = env.players().into_iter().collect();
    let mut set = RuleSet::default();
    // CR 613.11: applied in timestamp order (with dependency, 613.8); cost effects are ordered
    // later by the casting procedure from the stages of `cost_stages` (601.2f).
    for id in application_order(&keys, env) {
        let Some(effect) = effects.rule_effect(id) else {
            continue;
        };
        let entry = RuleEntry {
            id,
            players: resolve_players(
                &effect.players,
                effect.controller,
                effect.source,
                &players,
                env,
            ),
            controller: effect.controller,
            source: effect.source,
            timestamp: effect.timestamp,
            modification: effect.modification.clone(),
        };
        let at = set.position.len();
        set.position.insert(id, at);
        set.groups
            .entry(effect.modification.kind)
            .or_default()
            .push(entry);
    }
    set
}
