// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The special-action registry (CR 116.2, 117.1c): every special-action kind a player may take
//! when they have priority, except playing a land (CR 116.2a), which has its own response.

/// When a special action may be taken (CR 117.1c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialTiming {
    /// Any time the player has priority.
    AnyPriority,
    /// Priority, an empty stack, a main phase of the player's turn.
    SorceryTiming,
    /// Priority during the player's own turn while the stack is empty (any phase).
    OwnTurnStackEmpty,
    /// Priority during the player's own turn (any phase; the stack need not be empty).
    OwnTurn,
    /// Any time the player has priority, but only if they could begin to cast the card.
    AnyPriorityIfCouldBeginToCast,
}

/// One registered special-action kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialActionKind {
    /// The kind id used in `SPECIAL_ACTION` responses.
    pub id: &'static str,
    /// The rule that defines it, for example `"CR 116.2b"`.
    pub cr: &'static str,
    /// When it may be taken.
    pub timing: SpecialTiming,
}

/// The registry of special-action kinds, in a fixed order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecialActionRegistry {
    kinds: Vec<SpecialActionKind>,
}

/// Shorthand for a registry row.
const fn kind(id: &'static str, cr: &'static str, timing: SpecialTiming) -> SpecialActionKind {
    SpecialActionKind { id, cr, timing }
}

/// The standard kinds: CR 116.2b-k and 116.2m (there is no 116.2l; 116.2a, playing a land, is
/// the `PLAY_LAND` response, not a special-action kind).
const STANDARD: [SpecialActionKind; 11] = [
    // CR 116.2b: turning a face-down creature face up, any time with priority.
    kind("TURN_FACE_UP", "CR 116.2b", SpecialTiming::AnyPriority),
    // CR 116.2c: ending an effect or stopping a delayed trigger; the host does not list it when
    // the effect specifies another timing.
    kind("END_EFFECT", "CR 116.2c", SpecialTiming::AnyPriority),
    // CR 116.2d: ignoring the effect of a static ability for a duration.
    kind(
        "IGNORE_STATIC_EFFECT",
        "CR 116.2d",
        SpecialTiming::AnyPriority,
    ),
    // CR 116.2e: discarding a card any time one could cast an instant.
    kind(
        "DISCARD_AS_INSTANT",
        "CR 116.2e",
        SpecialTiming::AnyPriority,
    ),
    // CR 116.2f: exiling a card with suspend, only if it could begin to be cast.
    kind(
        "SUSPEND",
        "CR 116.2f",
        SpecialTiming::AnyPriorityIfCouldBeginToCast,
    ),
    // CR 116.2g: putting the companion into hand, main phase of own turn, empty stack.
    kind("COMPANION", "CR 116.2g", SpecialTiming::SorceryTiming),
    // CR 116.2h: foretell, any time with priority during own turn.
    kind("FORETELL", "CR 116.2h", SpecialTiming::OwnTurn),
    // CR 116.2i: rolling the planar die, main phase of own turn, empty stack.
    kind("ROLL_PLANAR_DIE", "CR 116.2i", SpecialTiming::SorceryTiming),
    // CR 116.2j: turning a face-down conspiracy face up, any time with priority.
    kind(
        "TURN_CONSPIRACY_FACE_UP",
        "CR 116.2j",
        SpecialTiming::AnyPriority,
    ),
    // CR 116.2k, 702.170a: plot, main phase of own turn, empty stack (the keyword rule is the
    // narrower of the two; see DECISIONS.md, Gate 06-turn F3).
    kind("PLOT", "CR 116.2k", SpecialTiming::SorceryTiming),
    // CR 116.2m: paying an unlock cost, main phase of own turn, empty stack.
    kind("UNLOCK", "CR 116.2m", SpecialTiming::SorceryTiming),
];

impl SpecialActionRegistry {
    /// The eleven standard kinds of CR 116.2 (b-k, m), in rule order.
    #[must_use]
    pub fn standard() -> SpecialActionRegistry {
        SpecialActionRegistry {
            kinds: STANDARD.to_vec(),
        }
    }

    /// The kind `id`, if registered.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&SpecialActionKind> {
        self.kinds.iter().find(|k| k.id == id)
    }

    /// Every kind, in the fixed registry order.
    #[must_use]
    pub fn kinds(&self) -> &[SpecialActionKind] {
        &self.kinds
    }
}

impl Default for SpecialActionRegistry {
    fn default() -> Self {
        SpecialActionRegistry::standard()
    }
}
