// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The passes after layers 1-7 (REQ-LAYER-004, SRS 6.4): pass A applies the continuous effects
//! that affect players (CR 613.10) and pass B builds the [`RuleSet`], the view of the effects
//! that modify the rules of the game (CR 613.11).
//!
//! **Registry.** [`PostLayerEffects`] holds every [`PlayerPassEffect`] and [`RuleEffect`] under
//! its [`EffectId`]; one id is used by at most one effect of either kind. A player effect of a
//! resolved spell or ability applies to a fixed set of players (CR 611.2c); a rule effect is
//! never locked, so it applies to whatever matches when the rule set is built (CR 611.2c).
//!
//! **Order (both passes).** Ascending timestamp, then ascending [`EffectId`] (CR 613.7), with an
//! optional dependency hook ([`PostLayerEnvironment::depends_on`]) with the semantics of
//! CR 613.8b: an effect that depends on others waits until just after all of them; among the
//! effects free to apply the earliest applies; when none is free, the earliest member of a
//! dependency loop with no way out applies. Effects whose
//! [`PostLayerEnvironment::effect_exists`] is false are dropped (CR 613.8a(b)).
//!
//! **Pass A** ([`compute_player_properties`]) returns, per player, the player effects that apply
//! to that player in application order ([`PlayerProperties`]); typed properties are derived by
//! later consumers.
//!
//! **Pass B** ([`build_rule_set`]) resolves every rule effect's players and groups the entries
//! by [`RuleKind`](fineprint_dsl::ability::RuleKind). Prohibitions are evaluated last (CR 101.2): [`RuleSet::query`] returns
//! [`Verdict::Prohibited`] whenever an applicable prohibition exists, whatever the timestamps.
//! [`RuleSet::max_hand_size`] (CR 402.2), [`RuleSet::land_plays_allowed`] (CR 305.2, 305.2b) and
//! [`RuleSet::cost_stages`] (the CR 601.2f hook, [`CostStages`]) read the rule set.
//!
//! **Determinism.** Only `Vec`, `BTreeMap` and `BTreeSet`; no floats, clock or I/O; the order of
//! registration never changes a result (REQ-CORE-001).

mod cost;
mod effects;
mod order;
mod player;
mod rules;

#[cfg(test)]
mod tests;

pub use cost::CostStages;
pub use effects::{
    PlayerAffected, PlayerPassEffect, PostLayerEffects, PostLayerEnvironment, RuleEffect,
};
pub use player::{AppliedPlayerEffect, PlayerProperties, compute_player_properties};
pub use rules::{
    DEFAULT_LAND_PLAYS, DEFAULT_MAX_HAND_SIZE, RuleEntry, RuleSet, Verdict, build_rule_set,
};
