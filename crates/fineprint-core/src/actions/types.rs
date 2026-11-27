// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The inputs (the [`ActionHost`] seam, timing facts and candidates) and outputs (legal actions,
//! allowances and reasons) of the legal-action generator (REQ-DSL-013).

use fineprint_base::ids::{EffectId, ObjectId, PlayerId};
use fineprint_dsl::ability::{ActivationLimit, ActivationTiming};
use fineprint_dsl::expr::Zone;

use crate::choice::ChoiceResponse;
use crate::events::{Phase, Step};
use crate::layers::RuleSet;

/// The timing facts the host supplies for the player being asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingContext {
    /// The player with priority.
    pub player: PlayerId,
    /// The active player, if any.
    pub active_player: Option<PlayerId>,
    /// The current phase, if any.
    pub phase: Option<Phase>,
    /// The current step, if any.
    pub step: Option<Step>,
    /// Whether the stack is empty.
    pub stack_empty: bool,
}

impl TimingContext {
    /// Whether it is `player`'s turn (CR 305.3).
    #[must_use]
    pub fn is_own_turn(&self) -> bool {
        self.active_player == Some(self.player)
    }

    /// Whether the phase is a main phase (precombat or postcombat).
    #[must_use]
    pub fn is_main_phase(&self) -> bool {
        matches!(
            self.phase,
            Some(Phase::PrecombatMain | Phase::PostcombatMain)
        )
    }

    /// Whether `player` could act "as a sorcery": their turn, a main phase and an empty stack
    /// (CR 307.5; priority is implied by being asked).
    #[must_use]
    pub fn sorcery_timing(&self) -> bool {
        self.is_own_turn() && self.is_main_phase() && self.stack_empty
    }

    /// Whether the phase is the combat phase.
    #[must_use]
    pub fn in_combat(&self) -> bool {
        self.phase == Some(Phase::Combat)
    }
}

/// A land the player could play (CR 116.2a, 305.1).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LandCandidate {
    /// The land card.
    pub object: ObjectId,
    /// Which face (0-based).
    pub face_index: u32,
    /// The zone it is in.
    pub zone: Zone,
    /// The label of the rule or ability of the card itself that permits playing it from `zone`
    /// (host-supplied), if any.
    pub own_permission: Option<String>,
}

/// One way the player could cast a card: one object, one face, one cast mode (CR 601.2b,
/// 712.8b-c, 709.3a).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CastCandidate {
    /// The card.
    pub object: ObjectId,
    /// Which face (0-based).
    pub face_index: u32,
    /// The zone it is in.
    pub zone: Zone,
    /// The cast mode (normal or one alternative cast) as listed in the response.
    pub cast_mode: String,
    /// The object is a land (and maybe another type): never castable (CR 305.9).
    pub is_land: bool,
    /// Instant or has flash, as evaluated by the host (CR 117.1a, 601.3d).
    pub instant_timing: bool,
    /// The label of the rule or ability that permits casting it from `zone` (host-supplied), if
    /// any.
    pub own_permission: Option<String>,
}

/// One activated ability the player could activate (CR 117.1b, 602.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbilityCandidate {
    /// The ability's source.
    pub object: ObjectId,
    /// The ability's id as listed in the response.
    pub ability_id: String,
    /// The zone the source is in.
    pub zone: Zone,
    /// The activation timing; `None` means `INSTANT`.
    pub timing: Option<ActivationTiming>,
    /// The activation limits, if any (CR 602.5b).
    pub limits: Option<ActivationLimit>,
    /// Activations of this ability this turn.
    pub uses_this_turn: u32,
    /// Activations of this ability this game.
    pub uses_this_game: u32,
    /// A mana ability: activatable whenever the player has priority (CR 117.1d, 605.3a).
    pub is_mana_ability: bool,
    /// `{T}` or `{Q}` in the activation cost (CR 602.5a).
    pub requires_tap_symbol: bool,
    /// Host: the source is a creature without haste that has not been under its controller's
    /// control continuously since the start of their most recent turn (CR 602.5a).
    pub summoning_sick: bool,
    /// The host-evaluated "activate only if" condition holds.
    pub only_if_met: bool,
}

/// Why a permanent or spell is face down, with the facts that decide whether it can be turned
/// face up as a special action (CR 116.2b).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FaceDownOrigin {
    /// Face down with morph (CR 702.37e).
    Morph {
        /// It would have a morph cost if it were face up.
        has_cost: bool,
    },
    /// Face down with disguise (CR 702.168d).
    Disguise {
        /// It would have a disguise cost if it were face up.
        has_cost: bool,
    },
    /// Manifested (CR 701.40b, 701.40g).
    Manifest {
        /// The card representing it is a creature card.
        creature_card: bool,
        /// The card has a mana cost.
        has_mana_cost: bool,
        /// The card is an instant or sorcery card.
        instant_or_sorcery: bool,
    },
    /// Cloaked (CR 701.58b).
    Cloak {
        /// The card representing it is a creature card.
        creature_card: bool,
        /// The card has a mana cost.
        has_mana_cost: bool,
        /// The card is an instant or sorcery card.
        instant_or_sorcery: bool,
    },
    /// Face down by some other rule or effect (CR 708.7).
    Other {
        /// The rule or effect that made it face down allows turning it face up.
        can_turn_face_up: bool,
    },
}

/// A face-down object the player could turn face up (special action `TURN_FACE_UP`, CR 116.2b).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TurnFaceUpCandidate {
    /// The face-down object.
    pub object: ObjectId,
    /// `true` for a permanent; `false` for a face-down spell (CR 708.7).
    pub on_battlefield: bool,
    /// A merged permanent containing an instant or sorcery card (CR 731.2g).
    pub merged_instant_or_sorcery: bool,
    /// Why it is face down.
    pub origin: FaceDownOrigin,
}

/// A special action other than `TURN_FACE_UP` the host offers for checking (CR 116.2c-m).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpecialCandidate {
    /// The special-action kind id (see
    /// [`SpecialActionRegistry`](super::SpecialActionRegistry)).
    pub kind: String,
    /// The object it involves.
    pub object: ObjectId,
    /// Host: the card could begin to be cast (kind `SUSPEND`, CR 116.2f).
    pub could_begin_to_cast: bool,
    /// Host-evaluated base eligibility (for example a companion not yet put into hand this
    /// game).
    pub available: bool,
}

/// What the generator needs from the game (object-safe; used as `&dyn ActionHost`).
pub trait ActionHost {
    /// The rule set of pass B (the rule effects in force).
    fn rule_set(&self) -> &RuleSet;
    /// The timing facts for `player`.
    fn timing(&self, player: PlayerId) -> TimingContext;
    /// The number of lands `player` played this turn (CR 305.2a).
    fn lands_played_this_turn(&self, player: PlayerId) -> i64;
    /// The lands `player` could play.
    fn land_candidates(&self, player: PlayerId) -> Vec<LandCandidate>;
    /// The (object, face, cast mode) combinations `player` could cast.
    fn cast_candidates(&self, player: PlayerId) -> Vec<CastCandidate>;
    /// The activated abilities `player` could activate.
    fn ability_candidates(&self, player: PlayerId) -> Vec<AbilityCandidate>;
    /// The face-down objects `player` could turn face up.
    fn turn_face_up_candidates(&self, player: PlayerId) -> Vec<TurnFaceUpCandidate>;
    /// The other special actions `player` could take.
    fn special_candidates(&self, player: PlayerId) -> Vec<SpecialCandidate>;
}

/// What allowed an action (the basis of `explainLegality`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Allowance {
    /// A rule of the game.
    Rule {
        /// The rule, for example `"CR 305.1"`.
        cr: &'static str,
    },
    /// A rule effect of the rule set (a permission, timing permission or additional land play).
    Effect {
        /// The effect's id.
        id: EffectId,
    },
    /// A host-supplied permission of the object itself.
    Own {
        /// The host's label.
        label: String,
    },
}

/// One legal action with what allowed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegalAction {
    /// The exact response that takes the action.
    pub response: ChoiceResponse,
    /// What allowed it; never empty.
    pub basis: Vec<Allowance>,
}

/// A timing condition an action failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingRequirement {
    /// Main phase of the player's turn with an empty stack (CR 307.5).
    SorceryTiming,
    /// The player's turn.
    OwnTurn,
    /// The player's turn with an empty stack.
    OwnTurnStackEmpty,
    /// An opponent's turn.
    OpponentTurn,
    /// The combat phase.
    Combat,
    /// The card could begin to be cast (CR 116.2f).
    CouldBeginToCast,
}

/// Why a face-down object cannot be turned face up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceUpDenial {
    /// It is a face-down spell, not a permanent (CR 708.7).
    NotOnBattlefield,
    /// It would have no morph or disguise cost face up (CR 702.37e, 702.168d).
    NoTurnFaceUpCost,
    /// The card representing it is not a creature card (CR 701.40b, 701.58b).
    NotCreatureCard,
    /// The card representing it has no mana cost (CR 701.40b, 701.58b).
    NoManaCost,
    /// An instant or sorcery card stays face down (CR 701.40g, 731.2g).
    InstantOrSorcery,
    /// The rule or effect that made it face down does not allow it (CR 708.7).
    NotAllowed,
}

/// Why an action is illegal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Not one of the host's candidates, or not a priority action.
    NoSuchAction,
    /// The earliest applicable `PROHIBITION` in application order (CR 101.2).
    Prohibited {
        /// The prohibition's id.
        by: EffectId,
    },
    /// The zone is not the hand and no permission applies.
    NoPermission,
    /// The player played as many lands as they can this turn (CR 305.2b).
    LandPlaysExhausted {
        /// Lands the player can play this turn.
        allowed: i64,
        /// Lands the player played this turn.
        played: i64,
    },
    /// The object is a land and cannot be cast (CR 305.9).
    IsLand,
    /// A timing condition failed.
    Timing(TimingRequirement),
    /// A per-turn or per-game activation limit is used up (CR 602.5b).
    LimitReached,
    /// A `{T}` / `{Q}` ability of a summoning-sick creature (CR 602.5a).
    SummoningSick,
    /// The "activate only if" condition does not hold.
    ConditionNotMet,
    /// The special-action kind is not in the registry.
    UnknownSpecialKind,
    /// The host reports the special action unavailable.
    NotAvailable,
    /// The object cannot be turned face up.
    FaceUp(FaceUpDenial),
}

/// The answer of [`explain`](super::explain).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Legality {
    /// The action is legal.
    Legal {
        /// What allowed it (empty only for passing priority).
        basis: Vec<Allowance>,
    },
    /// The action is illegal.
    Illegal {
        /// Every failing check, never empty: `Prohibited` first if present, then in the
        /// declaration order of [`Reason`].
        reasons: Vec<Reason>,
    },
}
