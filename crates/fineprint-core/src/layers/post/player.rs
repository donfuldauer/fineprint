// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Pass A: the continuous effects that affect players (REQ-LAYER-004, CR 613.10).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, PlayerId};
use fineprint_dsl::ability::Modification;

use super::effects::{PostLayerEffects, PostLayerEnvironment, resolve_players};
use super::order::application_order;

/// One player effect as applied to a player in pass A.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedPlayerEffect {
    /// The effect's id.
    pub id: EffectId,
    /// What it does (the DSL modification; typed properties are derived by later consumers).
    pub modification: Modification,
}

/// The result of pass A: for each player, the player effects that apply to them, in
/// application order (CR 613.10).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PlayerProperties {
    by_player: BTreeMap<PlayerId, Vec<AppliedPlayerEffect>>,
}

impl PlayerProperties {
    /// The effects applied to `p`, in application order; empty for a player no effect applies
    /// to or that is not in the game.
    #[must_use]
    pub fn applied(&self, p: PlayerId) -> &[AppliedPlayerEffect] {
        self.by_player.get(&p).map_or(&[], Vec::as_slice)
    }

    /// Every player some effect applies to, ascending, with their effects in application order.
    pub fn iter(&self) -> impl Iterator<Item = (PlayerId, &[AppliedPlayerEffect])> {
        self.by_player.iter().map(|(&p, v)| (p, v.as_slice()))
    }
}

/// Pass A (CR 613.10): applies the player effects of `effects` after the determination of
/// objects' characteristics, in timestamp order with the dependency hook (CR 613.7, 613.8b).
/// A fixed set is used as stored (players `env` does not list are ignored); a selector is
/// matched against every player of `env` now. Effects whose source no longer has the ability
/// ([`PostLayerEnvironment::effect_exists`]) are skipped.
#[must_use]
pub fn compute_player_properties(
    effects: &PostLayerEffects,
    env: &dyn PostLayerEnvironment,
) -> PlayerProperties {
    let keys: Vec<_> = effects
        .player_effects()
        .map(|(id, e)| (e.timestamp, id))
        .collect();
    let players: BTreeSet<PlayerId> = env.players().into_iter().collect();
    let mut by_player: BTreeMap<PlayerId, Vec<AppliedPlayerEffect>> = BTreeMap::new();
    // CR 613.10: all such effects are applied in timestamp order (with dependency, 613.8).
    for id in application_order(&keys, env) {
        let Some(effect) = effects.player_effect(id) else {
            continue;
        };
        let affected = resolve_players(
            &effect.affected,
            effect.controller,
            effect.source,
            &players,
            env,
        );
        for p in affected {
            by_player.entry(p).or_default().push(AppliedPlayerEffect {
                id,
                modification: effect.modification.clone(),
            });
        }
    }
    PlayerProperties { by_player }
}
