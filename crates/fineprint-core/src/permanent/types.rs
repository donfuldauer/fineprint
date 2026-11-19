// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Value types of the permanent container (REQ-ZONE-001 item 3).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::{ObjectId, PlayerId};

use crate::state::DesignationValue;

/// The status of a permanent (CR 110.5): one value in each of the four status categories.
///
/// The default is untapped, unflipped, face up and phased in, the status a permanent enters with
/// unless a spell or ability says otherwise (CR 110.5b). Status is not a characteristic
/// (CR 110.5a).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Tapped (`true`) or untapped (`false`).
    pub tapped: bool,
    /// Flipped (`true`) or unflipped (`false`).
    pub flipped: bool,
    /// Face down (`true`) or face up (`false`).
    pub face_down: bool,
    /// Phased out (`true`) or phased in (`false`).
    pub phased_out: bool,
}

impl CanonicalEncode for Status {
    /// The four flags in declaration order, one byte each.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.tapped.encode_canonical(out);
        self.flipped.encode_canonical(out);
        self.face_down.encode_canonical(out);
        self.phased_out.encode_canonical(out);
    }
}

/// What a permanent is attached to: a permanent or a player (REQ-ZONE-001 item 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttachTarget {
    /// Attached to the permanent with this id.
    Permanent(ObjectId),
    /// Attached to this player.
    Player(PlayerId),
}

impl CanonicalEncode for AttachTarget {
    /// `Permanent(id)`: `0x00` then the id; `Player(id)`: `0x01` then the id.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            AttachTarget::Permanent(id) => {
                put_variant(out, 0x00);
                id.encode_canonical(out);
            }
            AttachTarget::Player(id) => {
                put_variant(out, 0x01);
                id.encode_canonical(out);
            }
        }
    }
}

/// What an attacking creature is attacking: a player, or a permanent (a planeswalker or a
/// battle, CR 506.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttackTarget {
    /// Attacking this player.
    Player(PlayerId),
    /// Attacking the permanent (planeswalker or battle) with this id.
    Permanent(ObjectId),
}

impl CanonicalEncode for AttackTarget {
    /// `Player(id)`: `0x00` then the id; `Permanent(id)`: `0x01` then the id.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            AttackTarget::Player(id) => {
                put_variant(out, 0x00);
                id.encode_canonical(out);
            }
            AttackTarget::Permanent(id) => {
                put_variant(out, 0x01);
                id.encode_canonical(out);
            }
        }
    }
}

/// Whether a permanent is attacking, and what.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AttackState {
    /// Not an attacking creature.
    #[default]
    NotAttacking,
    /// An attacking creature. `None` is an attacker whose planeswalker or battle was removed from
    /// combat: it is still attacking, but not attacking any player, planeswalker or battle
    /// (CR 506.4c).
    Attacking(Option<AttackTarget>),
}

impl CanonicalEncode for AttackState {
    /// `NotAttacking`: `0x00`; `Attacking(t)`: `0x01` then the `Option` encoding of `t`.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            AttackState::NotAttacking => put_variant(out, 0x00),
            AttackState::Attacking(target) => {
                put_variant(out, 0x01);
                target.encode_canonical(out);
            }
        }
    }
}

/// The combat state of a permanent (REQ-ZONE-001 item 3).
///
/// The default is not attacking, blocking nothing, not blocked and not removed from combat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CombatState {
    /// Whether it is attacking, and what (CR 508.1k).
    pub attack: AttackState,
    /// The attacking creatures it blocks (CR 509.1g), ascending.
    pub blocking: BTreeSet<ObjectId>,
    /// Whether it is a blocked creature (CR 509.1h). It stays blocked even if all its blockers
    /// are removed from combat.
    pub blocked: bool,
    /// Whether it has been removed from combat (CR 506.4).
    pub removed_from_combat: bool,
}

impl CanonicalEncode for CombatState {
    /// The attack state, the blocking set, then the two flags.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.attack.encode_canonical(out);
        self.blocking.encode_canonical(out);
        self.blocked.encode_canonical(out);
        self.removed_from_combat.encode_canonical(out);
    }
}

/// The non-characteristic state of one permanent (REQ-ZONE-001 item 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permanent {
    pub(super) id: ObjectId,
    pub(super) controller: PlayerId,
    pub(super) status: Status,
    pub(super) designations: BTreeMap<String, DesignationValue>,
    pub(super) controlled_since: u64,
    pub(super) attached_to: Option<AttachTarget>,
    pub(super) combat: CombatState,
}

impl Permanent {
    /// A permanent with default status and combat state, no designations and no attachment,
    /// controlled by `controller` since `turn` (CR 110.2, CR 110.5b).
    pub(super) fn new(id: ObjectId, controller: PlayerId, turn: u64) -> Permanent {
        Permanent {
            id,
            controller,
            status: Status::default(),
            designations: BTreeMap::new(),
            controlled_since: turn,
            attached_to: None,
            combat: CombatState::default(),
        }
    }

    /// The permanent's object id.
    #[must_use]
    pub fn id(&self) -> ObjectId {
        self.id
    }

    /// The permanent's controller (CR 110.2), the result of layer 2 (CR 613.1b).
    #[must_use]
    pub fn controller(&self) -> PlayerId {
        self.controller
    }

    /// The permanent's status (CR 110.5).
    #[must_use]
    pub fn status(&self) -> Status {
        self.status
    }

    /// The value of designation `key`, if the permanent has it.
    #[must_use]
    pub fn designation(&self, key: &str) -> Option<&DesignationValue> {
        self.designations.get(key)
    }

    /// Every designation, in ascending key order.
    #[must_use]
    pub fn designations(&self) -> &BTreeMap<String, DesignationValue> {
        &self.designations
    }

    /// The turn number since which the permanent has been under its current controller's
    /// continuous control.
    #[must_use]
    pub fn controlled_since_turn(&self) -> u64 {
        self.controlled_since
    }

    /// CR 302.6 input: whether the permanent has been under its controller's control
    /// continuously since the start of turn `controller_turn`, the controller's most recent
    /// turn. True exactly when `controlled_since_turn() < controller_turn` (strict). Haste is
    /// NOT handled here.
    #[must_use]
    pub fn controlled_continuously_since_start_of(&self, controller_turn: u64) -> bool {
        // CR 302.6: control must predate the start of the controller's most recent turn.
        self.controlled_since < controller_turn
    }

    /// What the permanent is attached to, if anything.
    #[must_use]
    pub fn attached_to(&self) -> Option<AttachTarget> {
        self.attached_to
    }

    /// The permanent's combat state.
    #[must_use]
    pub fn combat(&self) -> &CombatState {
        &self.combat
    }
}

impl CanonicalEncode for Permanent {
    /// The id, controller, status, designations, controlled-since turn (8 bytes little-endian),
    /// attachment (`Option`) and combat state, in that order.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.id.encode_canonical(out);
        self.controller.encode_canonical(out);
        self.status.encode_canonical(out);
        self.designations.encode_canonical(out);
        self.controlled_since.encode_canonical(out);
        self.attached_to.encode_canonical(out);
        self.combat.encode_canonical(out);
    }
}
