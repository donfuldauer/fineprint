// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The registry of the effects applied after the layers (REQ-LAYER-004, CR 613.10, 613.11) and
//! the environment the passes query.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::{Modification, RuleModification};
use fineprint_dsl::expr::PlayerSelectorAST;

use crate::layers::{EffectOrigin, LayerError};

/// The players an effect after the layers applies to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerAffected {
    /// A fixed set of players (for a resolved player effect, the set chosen at registration,
    /// CR 611.2c). Players the environment does not list are ignored.
    Fixed(BTreeSet<PlayerId>),
    /// A selector matched against every player each time a pass runs
    /// ([`PostLayerEnvironment::matches_player`]).
    Selector(PlayerSelectorAST),
}

/// A continuous effect that affects players, applied in pass A (CR 613.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerPassEffect {
    /// The object whose ability generated the effect, if any.
    pub source: Option<ObjectId>,
    /// The effect's controller.
    pub controller: PlayerId,
    /// The effect's timestamp (CR 613.7).
    pub timestamp: Timestamp,
    /// Static ability or resolved spell or ability.
    pub origin: EffectOrigin,
    /// The players it applies to.
    pub affected: PlayerAffected,
    /// What it does to each of them (the DSL modification, opaque here).
    pub modification: Modification,
}

/// A continuous effect that modifies the rules of the game, applied in pass B (CR 613.11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleEffect {
    /// The object whose ability generated the effect, if any.
    pub source: Option<ObjectId>,
    /// The effect's controller.
    pub controller: PlayerId,
    /// The effect's timestamp (CR 613.7).
    pub timestamp: Timestamp,
    /// Static ability or resolved spell or ability.
    pub origin: EffectOrigin,
    /// The players whose rules are modified. A selector is matched when the rule set is built,
    /// whatever the origin (CR 611.2c: rules-modifying parts are not locked).
    pub players: PlayerAffected,
    /// The rule modification.
    pub modification: RuleModification,
}

/// Every effect applied after the layers, keyed by [`EffectId`]: player effects (pass A) and
/// rule effects (pass B). One id names at most one effect of either kind. Whether an effect is
/// still in force is the caller's job: the caller removes effects that ended.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PostLayerEffects {
    players: BTreeMap<EffectId, PlayerPassEffect>,
    rules: BTreeMap<EffectId, RuleEffect>,
}

impl PostLayerEffects {
    /// An empty registry.
    #[must_use]
    pub fn new() -> PostLayerEffects {
        PostLayerEffects::default()
    }

    /// Whether `id` names an effect of either kind.
    fn contains(&self, id: EffectId) -> bool {
        self.players.contains_key(&id) || self.rules.contains_key(&id)
    }

    /// Registers the player effect `effect` under `id` (pass A, CR 613.10).
    ///
    /// # Errors
    /// [`LayerError::DuplicateEffect`] if `id` names a player or rule effect;
    /// [`LayerError::Unsupported`] if the effect comes from a resolved spell or ability
    /// ([`EffectOrigin::Resolved`]) and has a selector: such an effect applies to the players
    /// fixed when it began (CR 611.2c), so the caller passes [`PlayerAffected::Fixed`]. The
    /// registry is then unchanged.
    pub fn register_player(
        &mut self,
        id: EffectId,
        effect: PlayerPassEffect,
    ) -> Result<(), LayerError> {
        if self.contains(id) {
            return Err(LayerError::DuplicateEffect(id));
        }
        // CR 611.2c: the set affected by a resolved effect is fixed when it begins.
        if effect.origin == EffectOrigin::Resolved
            && matches!(effect.affected, PlayerAffected::Selector(_))
        {
            return Err(LayerError::Unsupported(
                "a resolved player effect with a selector (CR 611.2c: pass a fixed set)",
            ));
        }
        self.players.insert(id, effect);
        Ok(())
    }

    /// Registers the rule effect `effect` under `id` (pass B, CR 613.11). Any origin may use a
    /// selector (CR 611.2c).
    ///
    /// # Errors
    /// [`LayerError::DuplicateEffect`] if `id` names a player or rule effect; the registry is
    /// then unchanged.
    pub fn register_rule(&mut self, id: EffectId, effect: RuleEffect) -> Result<(), LayerError> {
        if self.contains(id) {
            return Err(LayerError::DuplicateEffect(id));
        }
        self.rules.insert(id, effect);
        Ok(())
    }

    /// Removes the effect `id` of either kind; whether it was registered.
    pub fn remove(&mut self, id: EffectId) -> bool {
        self.players.remove(&id).is_some() || self.rules.remove(&id).is_some()
    }

    /// The number of registered effects of both kinds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.players.len() + self.rules.len()
    }

    /// Whether no effect is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.players.is_empty() && self.rules.is_empty()
    }

    /// The player effect `id`, if registered.
    #[must_use]
    pub fn player_effect(&self, id: EffectId) -> Option<&PlayerPassEffect> {
        self.players.get(&id)
    }

    /// The rule effect `id`, if registered.
    #[must_use]
    pub fn rule_effect(&self, id: EffectId) -> Option<&RuleEffect> {
        self.rules.get(&id)
    }

    /// Every player effect with its id, ascending [`EffectId`].
    pub fn player_effects(&self) -> impl Iterator<Item = (EffectId, &PlayerPassEffect)> {
        self.players.iter().map(|(&id, e)| (id, e))
    }

    /// Every rule effect with its id, ascending [`EffectId`].
    pub fn rule_effects(&self) -> impl Iterator<Item = (EffectId, &RuleEffect)> {
        self.rules.iter().map(|(&id, e)| (id, e))
    }
}

/// What the passes after the layers need from the game.
pub trait PostLayerEnvironment {
    /// Every player in the game, ascending.
    fn players(&self) -> Vec<PlayerId>;

    /// Whether `candidate` matches `selector` for an effect controlled by `controller` whose
    /// source is `source`.
    fn matches_player(
        &self,
        selector: &PlayerSelectorAST,
        controller: PlayerId,
        source: Option<ObjectId>,
        candidate: PlayerId,
    ) -> bool;

    /// Whether the effect `id` still exists: `false` when the static ability that generates it
    /// is no longer on its source (CR 613.8a(b)). An effect that does not exist is skipped.
    fn effect_exists(&self, _id: EffectId) -> bool {
        true
    }

    /// Whether effect `a` depends on effect `b` (CR 613.8a), consulted only between effects of
    /// the same pass. The default is no dependency.
    fn depends_on(&self, _a: EffectId, _b: EffectId) -> bool {
        false
    }
}

/// Resolves `affected` to the set of players it applies to now: a fixed set intersected with the
/// players of the game, or every player matching the selector.
pub(super) fn resolve_players(
    affected: &PlayerAffected,
    controller: PlayerId,
    source: Option<ObjectId>,
    players: &BTreeSet<PlayerId>,
    env: &dyn PostLayerEnvironment,
) -> BTreeSet<PlayerId> {
    match affected {
        PlayerAffected::Fixed(set) => set.intersection(players).copied().collect(),
        PlayerAffected::Selector(selector) => players
            .iter()
            .copied()
            .filter(|&p| env.matches_player(selector, controller, source, p))
            .collect(),
    }
}
