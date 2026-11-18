// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The registry of the CR 400.7 exceptions (REQ-CORE-002).
//!
//! CR 400.7 makes an object that changes zones a new object with no memory of its previous
//! existence, with the lettered exceptions CR 400.7a-k and CR 400.7m (there is no letter l, so
//! there are twelve). Each exception is a named function taking the old and the new object id
//! from a [`Moved`](super::Moved). The bodies are filled in by the units that need them; until
//! then each returns [`ExceptionOutcome::NotImplemented`].

use fineprint_base::ids::ObjectId;

/// One of the twelve CR 400.7 exceptions (REQ-CORE-002, CR 400.7a-m).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Exception400_7 {
    /// CR 400.7a: Effects that change the characteristics or controller of a permanent spell on the stack continue to apply to the permanent it becomes.
    A,
    /// CR 400.7b: Abilities granted by static abilities to a permanent spell, functioning on the battlefield, continue to apply to the permanent it becomes.
    B,
    /// CR 400.7c: Prevention effects for damage from a permanent spell continue to apply to damage from the permanent it becomes.
    C,
    /// CR 400.7d: A permanent's abilities can reference information about the spell it came from, such as costs paid and mana spent.
    D,
    /// CR 400.7e: Zone-change triggers can find the new object in the destination zone, if that zone is public.
    E,
    /// CR 400.7f: Enchanted-permanent-leaves triggers can find each Aura's new object in its owner's graveyard.
    F,
    /// CR 400.7g: An effect-granted ability allowing a nonland card to be cast keeps applying to the new stack object.
    G,
    /// CR 400.7h: An effect allowing a nonland card to be cast can find the new stack object.
    H,
    /// CR 400.7i: An effect allowing a land card to be played can find the new battlefield object.
    I,
    /// CR 400.7j: An effect (or a cost) that moves an object to a public zone can find that object.
    J,
    /// CR 400.7k: After a madness trigger resolves, effects referencing the discarded card can find it if it went to a public zone uncast.
    K,
    /// CR 400.7m: Stickers are retained on public-to-public moves and their effects continue to apply.
    M,
}

impl Exception400_7 {
    /// All twelve exceptions in rule order (CR 400.7a-k, then CR 400.7m).
    pub const ALL: [Exception400_7; 12] = [
        Exception400_7::A,
        Exception400_7::B,
        Exception400_7::C,
        Exception400_7::D,
        Exception400_7::E,
        Exception400_7::F,
        Exception400_7::G,
        Exception400_7::H,
        Exception400_7::I,
        Exception400_7::J,
        Exception400_7::K,
        Exception400_7::M,
    ];

    /// The CR rule number of this exception, such as `"400.7a"`.
    #[must_use]
    pub fn rule(self) -> &'static str {
        match self {
            Exception400_7::A => "400.7a",
            Exception400_7::B => "400.7b",
            Exception400_7::C => "400.7c",
            Exception400_7::D => "400.7d",
            Exception400_7::E => "400.7e",
            Exception400_7::F => "400.7f",
            Exception400_7::G => "400.7g",
            Exception400_7::H => "400.7h",
            Exception400_7::I => "400.7i",
            Exception400_7::J => "400.7j",
            Exception400_7::K => "400.7k",
            Exception400_7::M => "400.7m",
        }
    }

    /// A unique snake_case name for this exception.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Exception400_7::A => "permanent_spell_effects",
            Exception400_7::B => "static_granted_abilities",
            Exception400_7::C => "prevention_effects",
            Exception400_7::D => "spell_information",
            Exception400_7::E => "leaves_zone_triggers",
            Exception400_7::F => "aura_leaves_triggers",
            Exception400_7::G => "granted_cast_ability",
            Exception400_7::H => "cast_effect_finds_spell",
            Exception400_7::I => "play_land_effect_finds_permanent",
            Exception400_7::J => "effect_finds_moved_object",
            Exception400_7::K => "madness_finds_card",
            Exception400_7::M => "stickers_retained",
        }
    }
}

/// The outcome of applying a CR 400.7 exception to a zone move (REQ-CORE-002).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExceptionOutcome {
    /// The exception's behaviour is not implemented yet.
    NotImplemented(Exception400_7),
}

/// One registry entry: an exception and the function that applies it (REQ-CORE-002).
#[derive(Clone, Copy, Debug)]
pub struct ExceptionEntry {
    /// The exception.
    pub exception: Exception400_7,
    /// Applies the exception to a move from the old object id to the new one.
    pub apply: fn(ObjectId, ObjectId) -> ExceptionOutcome,
}

/// The registry: every CR 400.7 exception in rule order, CR 400.7a-k then CR 400.7m
/// (REQ-CORE-002).
pub const EXCEPTIONS: [ExceptionEntry; 12] = [
    ExceptionEntry {
        exception: Exception400_7::A,
        apply: exception_400_7a_permanent_spell_effects,
    },
    ExceptionEntry {
        exception: Exception400_7::B,
        apply: exception_400_7b_static_granted_abilities,
    },
    ExceptionEntry {
        exception: Exception400_7::C,
        apply: exception_400_7c_prevention_effects,
    },
    ExceptionEntry {
        exception: Exception400_7::D,
        apply: exception_400_7d_spell_information,
    },
    ExceptionEntry {
        exception: Exception400_7::E,
        apply: exception_400_7e_leaves_zone_triggers,
    },
    ExceptionEntry {
        exception: Exception400_7::F,
        apply: exception_400_7f_aura_leaves_triggers,
    },
    ExceptionEntry {
        exception: Exception400_7::G,
        apply: exception_400_7g_granted_cast_ability,
    },
    ExceptionEntry {
        exception: Exception400_7::H,
        apply: exception_400_7h_cast_effect_finds_spell,
    },
    ExceptionEntry {
        exception: Exception400_7::I,
        apply: exception_400_7i_play_land_effect_finds_permanent,
    },
    ExceptionEntry {
        exception: Exception400_7::J,
        apply: exception_400_7j_effect_finds_moved_object,
    },
    ExceptionEntry {
        exception: Exception400_7::K,
        apply: exception_400_7k_madness_finds_card,
    },
    ExceptionEntry {
        exception: Exception400_7::M,
        apply: exception_400_7m_stickers_retained,
    },
];

/// CR 400.7a: Effects that change the characteristics or controller of a permanent spell on the stack continue to apply to the permanent it becomes. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7a_permanent_spell_effects(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7a
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::A)
}

/// CR 400.7b: Abilities granted by static abilities to a permanent spell, functioning on the battlefield, continue to apply to the permanent it becomes. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7b_static_granted_abilities(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7b
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::B)
}

/// CR 400.7c: Prevention effects for damage from a permanent spell continue to apply to damage from the permanent it becomes. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7c_prevention_effects(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7c
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::C)
}

/// CR 400.7d: A permanent's abilities can reference information about the spell it came from, such as costs paid and mana spent. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7d_spell_information(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7d
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::D)
}

/// CR 400.7e: Zone-change triggers can find the new object in the destination zone, if that zone is public. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7e_leaves_zone_triggers(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7e
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::E)
}

/// CR 400.7f: Enchanted-permanent-leaves triggers can find each Aura's new object in its owner's graveyard. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7f_aura_leaves_triggers(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7f
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::F)
}

/// CR 400.7g: An effect-granted ability allowing a nonland card to be cast keeps applying to the new stack object. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7g_granted_cast_ability(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7g
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::G)
}

/// CR 400.7h: An effect allowing a nonland card to be cast can find the new stack object. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7h_cast_effect_finds_spell(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7h
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::H)
}

/// CR 400.7i: An effect allowing a land card to be played can find the new battlefield object. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7i_play_land_effect_finds_permanent(
    old: ObjectId,
    new: ObjectId,
) -> ExceptionOutcome {
    // CR 400.7i
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::I)
}

/// CR 400.7j: An effect (or a cost) that moves an object to a public zone can find that object. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7j_effect_finds_moved_object(
    old: ObjectId,
    new: ObjectId,
) -> ExceptionOutcome {
    // CR 400.7j
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::J)
}

/// CR 400.7k: After a madness trigger resolves, effects referencing the discarded card can find it if it went to a public zone uncast. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7k_madness_finds_card(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7k
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::K)
}

/// CR 400.7m: Stickers are retained on public-to-public moves and their effects continue to apply. (REQ-CORE-002)
///
/// Not implemented yet: returns [`ExceptionOutcome::NotImplemented`]. The unit that needs this
/// exception fills in the body.
#[must_use]
pub fn exception_400_7m_stickers_retained(old: ObjectId, new: ObjectId) -> ExceptionOutcome {
    // CR 400.7m
    let _ = (old, new);
    ExceptionOutcome::NotImplemented(Exception400_7::M)
}
