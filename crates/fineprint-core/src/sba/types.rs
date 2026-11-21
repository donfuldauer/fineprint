// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The value types of the state-based action check (REQ-SBA-001): the game variant, findings,
//! the caller-supplied environment and the outcome of one pass.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId};

use crate::events::ExecutedBatch;
use crate::permanent::AttachTarget;

/// The player designation that marks a player who has lost the game (REQ-SBA-001, CR 704.5a,
/// CR 704.5b, CR 704.5c). Its value is `DesignationValue::Flag`. No game-loss event exists yet,
/// so losing the game is recorded as this designation and reported in
/// [`SbaOutcome::lost_players`]; deciding a draw or a winner (CR 104.4a) is the caller's.
pub const LOST_THE_GAME: &str = "lostTheGame";

/// The kind of game whose state-based actions are checked (REQ-SBA-001). Some entries are
/// inactive in some variants (CR 704.5c: ignored in Two-Headed Giant games).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GameVariant {
    /// A game that is not a Two-Headed Giant game.
    Standard,
    /// A Two-Headed Giant game (CR 704.6; the team checks of CR 704.6a and CR 704.6b are not
    /// modelled by this unit).
    TwoHeadedGiant,
}

/// What a state-based action applies to (REQ-SBA-001).
///
/// Ordered players first, then objects, each ascending by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SbaSubject {
    /// A player (CR 704.5a, CR 704.5b, CR 704.5c).
    Player(PlayerId),
    /// An object (CR 704.5d-704.5t).
    Object(ObjectId),
}

/// One applicable state-based action: the registry entry and its subject (REQ-SBA-001).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SbaFinding {
    /// The id of the registry entry, for example `"704.5a"`.
    pub entry_id: &'static str,
    /// The player or object it applies to.
    pub subject: SbaSubject,
}

/// What the state-based action check reads from the caller that the engine state does not
/// hold yet (REQ-SBA-001). Every method has a default (a standard game with no copies, no
/// exemption, every world permanent stamped 0, every Aura, Equipment and Fortification
/// attachment legal as far as effects go, no counter limits,
/// no Saga chapter abilities, no pending chapter or room abilities and no completed dungeon)
/// except the deathtouch pair [`SbaEnvironment::deathtouch_damaged`] and
/// [`SbaEnvironment::clear_deathtouch_damaged`], which every implementation provides together.
///
/// Every method must be a pure function of the caller's state (REQ-CORE-001). The heartbeat
/// and layer-integration units replace these seams with engine state.
///
/// **One zone residency.** The copy, exemption and deathtouch answers are keyed by
/// [`ObjectId`], and an object that changes zones becomes a new object with a new id
/// (CR 400.7), so each answer lasts ONE zone residency. Within one
/// [`perform_all`](super::perform_all) pass the engine follows the moves its own batch makes.
/// Otherwise a caller whose state changes zones outside `perform_all`, or whose object survives
/// a move made during a pass (for example a card copy moved onto the battlefield by a
/// replacement effect), must re-key its environment to the new id before the next check. After
/// a pass, [`SbaOutcome::ceased`] lists the ids that no longer exist and
/// [`SbaOutcome::executed`] (each `Outcome::ZoneChange` with its `old` and `new` ids) tells it
/// what to re-key. A copy of a permanent spell that resolves becomes a permanent, not a copy of
/// a spell: the caller must stop answering `true` from
/// [`SbaEnvironment::is_spell_copy`] for it.
///
/// An environment that does not track deathtouch still states the pair:
///
/// ```
/// use std::collections::BTreeSet;
/// use fineprint_base::ids::ObjectId;
/// use fineprint_core::sba::SbaEnvironment;
///
/// struct NoDeathtouch;
/// impl SbaEnvironment for NoDeathtouch {
///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
///         BTreeSet::new()
///     }
///     fn clear_deathtouch_damaged(&mut self) {}
/// }
/// ```
///
/// Reporting deathtouch damage without the clear does not compile:
///
/// ```compile_fail,E0046
/// use std::collections::BTreeSet;
/// use fineprint_base::ids::ObjectId;
/// use fineprint_core::sba::SbaEnvironment;
///
/// struct ReadOnly;
/// impl SbaEnvironment for ReadOnly {
///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
///         BTreeSet::new()
///     }
/// }
/// ```
pub trait SbaEnvironment {
    /// The game variant (CR 704.5c, CR 704.6). Default: [`GameVariant::Standard`].
    fn variant(&self) -> GameVariant {
        GameVariant::Standard
    }

    /// Whether `object` is a copy of a spell (CR 704.5e, CR 707.10a). Default: `false`.
    fn is_spell_copy(&self, object: ObjectId) -> bool {
        let _ = object;
        false
    }

    /// Whether `object` is a copy of a card (CR 704.5e, CR 707.10a). Default: `false`.
    fn is_card_copy(&self, object: ObjectId) -> bool {
        let _ = object;
        false
    }

    /// Whether `object` is exempt from CR 704.5e (the copy of CR 723.3c). Default: `false`.
    fn exempt_from_copy_sba(&self, object: ObjectId) -> bool {
        let _ = object;
        false
    }

    /// When `object` last gained the world supertype (CR 704.5k, CR 205.4b), as a
    /// caller-supplied monotonic stamp: a LARGER stamp means it gained the supertype LATER, so
    /// it has had the supertype for a SHORTER time. Default: `0` (every world permanent tied).
    ///
    /// ```
    /// use std::collections::BTreeSet;
    /// use fineprint_base::ids::ObjectId;
    /// use fineprint_core::sba::SbaEnvironment;
    ///
    /// struct Plain;
    /// impl SbaEnvironment for Plain {
    ///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
    ///         BTreeSet::new()
    ///     }
    ///     fn clear_deathtouch_damaged(&mut self) {}
    /// }
    /// assert_eq!(Plain.world_supertype_since(ObjectId::new(1)), 0);
    /// ```
    fn world_supertype_since(&self, object: ObjectId) -> u64 {
        let _ = object;
        0
    }

    /// Whether Aura `aura` may enchant `target` as defined by its enchant ability and other
    /// applicable effects (CR 704.5m, CR 303.4c). The engine itself checks the rest of
    /// CR 704.5m (not attached, attached to itself, a target that left the battlefield or a
    /// player who left the game, an Aura that is also a creature). Default: `true`.
    ///
    /// ```
    /// use std::collections::BTreeSet;
    /// use fineprint_base::ids::{ObjectId, PlayerId};
    /// use fineprint_core::permanent::AttachTarget;
    /// use fineprint_core::sba::SbaEnvironment;
    ///
    /// struct Plain;
    /// impl SbaEnvironment for Plain {
    ///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
    ///         BTreeSet::new()
    ///     }
    ///     fn clear_deathtouch_damaged(&mut self) {}
    /// }
    /// assert!(Plain.aura_can_enchant(ObjectId::new(1), AttachTarget::Player(PlayerId::new(1))));
    /// ```
    fn aura_can_enchant(&self, aura: ObjectId, target: AttachTarget) -> bool {
        let _ = (aura, target);
        true
    }

    /// Whether Equipment or Fortification `attachment` may stay attached to `target` as far as
    /// effects are concerned (CR 704.5n, CR 702.16d, CR 301.5c): `false` when protection or
    /// another effect says the permanent can't be equipped or fortified by it. The engine
    /// itself checks the rest of CR 704.5n (attached to a player, to itself, to a permanent
    /// that left the battlefield, an Equipment on a noncreature or a Fortification on a
    /// nonland). An attachment to a phased-out permanent is not checked (CR 702.26b).
    /// Default: `true`.
    ///
    /// ```
    /// use std::collections::BTreeSet;
    /// use fineprint_base::ids::ObjectId;
    /// use fineprint_core::permanent::AttachTarget;
    /// use fineprint_core::sba::SbaEnvironment;
    ///
    /// struct Plain;
    /// impl SbaEnvironment for Plain {
    ///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
    ///         BTreeSet::new()
    ///     }
    ///     fn clear_deathtouch_damaged(&mut self) {}
    /// }
    /// assert!(Plain.equipment_can_attach(
    ///     ObjectId::new(1),
    ///     AttachTarget::Permanent(ObjectId::new(2))
    /// ));
    /// ```
    fn equipment_can_attach(&self, attachment: ObjectId, target: AttachTarget) -> bool {
        let _ = (attachment, target);
        true
    }

    /// The limits "can't have more than N counters of a kind on it" of `object`'s abilities
    /// (CR 704.5r): counter kind to N (N >= 0; a negative N counts as 0). Default: none.
    ///
    /// ```
    /// use std::collections::BTreeSet;
    /// use fineprint_base::ids::ObjectId;
    /// use fineprint_core::sba::SbaEnvironment;
    ///
    /// struct Plain;
    /// impl SbaEnvironment for Plain {
    ///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
    ///         BTreeSet::new()
    ///     }
    ///     fn clear_deathtouch_damaged(&mut self) {}
    /// }
    /// assert!(Plain.counter_limits(ObjectId::new(1)).is_empty());
    /// ```
    fn counter_limits(&self, object: ObjectId) -> BTreeMap<String, i64> {
        let _ = object;
        BTreeMap::new()
    }

    /// The final chapter number of `object` (CR 704.5s, CR 715.2d: the greatest value among
    /// its chapter abilities), `Some` ONLY if it has one or more chapter abilities; `None` if
    /// it has none (then CR 704.5s does not apply). Default: `None`.
    ///
    /// ```
    /// use std::collections::BTreeSet;
    /// use fineprint_base::ids::ObjectId;
    /// use fineprint_core::sba::SbaEnvironment;
    ///
    /// struct Plain;
    /// impl SbaEnvironment for Plain {
    ///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
    ///         BTreeSet::new()
    ///     }
    ///     fn clear_deathtouch_damaged(&mut self) {}
    /// }
    /// assert_eq!(Plain.saga_final_chapter(ObjectId::new(1)), None);
    /// ```
    fn saga_final_chapter(&self, object: ObjectId) -> Option<i64> {
        let _ = object;
        None
    }

    /// Whether `object` is the source of a chapter ability that has triggered but not yet left
    /// the stack (CR 704.5s). Default: `false`.
    ///
    /// ```
    /// use std::collections::BTreeSet;
    /// use fineprint_base::ids::ObjectId;
    /// use fineprint_core::sba::SbaEnvironment;
    ///
    /// struct Plain;
    /// impl SbaEnvironment for Plain {
    ///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
    ///         BTreeSet::new()
    ///     }
    ///     fn clear_deathtouch_damaged(&mut self) {}
    /// }
    /// assert!(!Plain.chapter_ability_pending(ObjectId::new(1)));
    /// ```
    fn chapter_ability_pending(&self, object: ObjectId) -> bool {
        let _ = object;
        false
    }

    /// The dungeon cards (objects in the command zone, CR 309.2c) on whose bottommost room
    /// their owner's venture marker is (CR 704.5t). The engine ignores ids that are not
    /// objects in the command zone. Default: none.
    ///
    /// ```
    /// use std::collections::BTreeSet;
    /// use fineprint_base::ids::ObjectId;
    /// use fineprint_core::sba::SbaEnvironment;
    ///
    /// struct Plain;
    /// impl SbaEnvironment for Plain {
    ///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
    ///         BTreeSet::new()
    ///     }
    ///     fn clear_deathtouch_damaged(&mut self) {}
    /// }
    /// assert!(Plain.dungeons_at_bottommost_room().is_empty());
    /// ```
    fn dungeons_at_bottommost_room(&self) -> BTreeSet<ObjectId> {
        BTreeSet::new()
    }

    /// Whether dungeon card `dungeon` is the source of a room ability that has triggered but
    /// not yet left the stack (CR 704.5t). Default: `false`.
    ///
    /// ```
    /// use std::collections::BTreeSet;
    /// use fineprint_base::ids::ObjectId;
    /// use fineprint_core::sba::SbaEnvironment;
    ///
    /// struct Plain;
    /// impl SbaEnvironment for Plain {
    ///     fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
    ///         BTreeSet::new()
    ///     }
    ///     fn clear_deathtouch_damaged(&mut self) {}
    /// }
    /// assert!(!Plain.room_ability_pending(ObjectId::new(1)));
    /// ```
    fn room_ability_pending(&self, dungeon: ObjectId) -> bool {
        let _ = dungeon;
        false
    }

    /// The objects that have been dealt damage (more than 0, CR 120.8) by a source with
    /// deathtouch since the last time state-based actions were checked (CR 704.5h, CR 702.2b).
    ///
    /// Required, together with [`SbaEnvironment::clear_deathtouch_damaged`]: an environment
    /// that reports an object here MUST forget it there. An environment that does not track
    /// deathtouch returns an empty set (or uses [`StoredSbaEnvironment`], empty by default).
    fn deathtouch_damaged(&self) -> BTreeSet<ObjectId>;

    /// Forgets every object of [`SbaEnvironment::deathtouch_damaged`]. Required, together with
    /// it. [`perform_all`](super::perform_all) calls it after EVERY successful pass, for every
    /// creature, indestructible and phased-out ones included: the window "since the last time
    /// state-based actions were checked" closes at every check (CR 704.5h). On
    /// `BusInterrupt::Pending` or an error it is not called and the window is unchanged. An
    /// environment that does not track deathtouch does nothing here.
    fn clear_deathtouch_damaged(&mut self);
}

/// A plain [`SbaEnvironment`] whose answers are stored sets (REQ-SBA-001): for callers and
/// tests that do not derive them from other state.
///
/// The sets are keyed by [`ObjectId`] and so last one zone residency (CR 400.7): the caller
/// re-keys them after a zone change, as described on [`SbaEnvironment`] (from
/// [`SbaOutcome::ceased`] and the `Outcome::ZoneChange` entries of [`SbaOutcome::executed`]),
/// and removes a resolved permanent-spell copy from `spell_copies`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredSbaEnvironment {
    /// The game variant.
    pub variant: GameVariant,
    /// The objects that are copies of spells.
    pub spell_copies: BTreeSet<ObjectId>,
    /// The objects that are copies of cards.
    pub card_copies: BTreeSet<ObjectId>,
    /// The objects exempt from CR 704.5e (CR 723.3c).
    pub exempt: BTreeSet<ObjectId>,
    /// The objects dealt damage by a deathtouch source since the last check.
    pub deathtouch: BTreeSet<ObjectId>,
    /// When each object last gained the world supertype (CR 704.5k; larger = later); a
    /// missing object answers 0.
    pub world_since: BTreeMap<ObjectId, u64>,
    /// The (Aura, target) pairs that are illegal (CR 704.5m, CR 303.4c); every other pair is
    /// legal.
    pub illegal_aura_targets: BTreeSet<(ObjectId, AttachTarget)>,
    /// The (Equipment or Fortification, target) pairs that protection or another effect makes
    /// illegal (CR 704.5n, CR 702.16d); every other pair is legal as far as effects go.
    pub illegal_equipment_targets: BTreeSet<(ObjectId, AttachTarget)>,
    /// The counter limits of each object (CR 704.5r): counter kind to N.
    pub counter_limits: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    /// The final chapter number of each object with one or more chapter abilities
    /// (CR 704.5s, CR 715.2d).
    pub saga_final_chapter: BTreeMap<ObjectId, i64>,
    /// The objects that are the source of a chapter ability that has triggered but not yet
    /// left the stack (CR 704.5s).
    pub chapter_pending: BTreeSet<ObjectId>,
    /// The dungeon cards whose owner's venture marker is on the bottommost room (CR 704.5t).
    pub bottommost_dungeons: BTreeSet<ObjectId>,
    /// The dungeon cards that are the source of a room ability that has triggered but not yet
    /// left the stack (CR 704.5t).
    pub room_pending: BTreeSet<ObjectId>,
}

impl Default for StoredSbaEnvironment {
    fn default() -> StoredSbaEnvironment {
        StoredSbaEnvironment {
            variant: GameVariant::Standard,
            spell_copies: BTreeSet::new(),
            card_copies: BTreeSet::new(),
            exempt: BTreeSet::new(),
            deathtouch: BTreeSet::new(),
            world_since: BTreeMap::new(),
            illegal_aura_targets: BTreeSet::new(),
            illegal_equipment_targets: BTreeSet::new(),
            counter_limits: BTreeMap::new(),
            saga_final_chapter: BTreeMap::new(),
            chapter_pending: BTreeSet::new(),
            bottommost_dungeons: BTreeSet::new(),
            room_pending: BTreeSet::new(),
        }
    }
}

impl SbaEnvironment for StoredSbaEnvironment {
    fn variant(&self) -> GameVariant {
        self.variant
    }

    fn is_spell_copy(&self, object: ObjectId) -> bool {
        self.spell_copies.contains(&object)
    }

    fn is_card_copy(&self, object: ObjectId) -> bool {
        self.card_copies.contains(&object)
    }

    fn exempt_from_copy_sba(&self, object: ObjectId) -> bool {
        self.exempt.contains(&object)
    }

    fn world_supertype_since(&self, object: ObjectId) -> u64 {
        self.world_since.get(&object).copied().unwrap_or(0)
    }

    fn aura_can_enchant(&self, aura: ObjectId, target: AttachTarget) -> bool {
        !self.illegal_aura_targets.contains(&(aura, target))
    }

    fn equipment_can_attach(&self, attachment: ObjectId, target: AttachTarget) -> bool {
        !self
            .illegal_equipment_targets
            .contains(&(attachment, target))
    }

    fn counter_limits(&self, object: ObjectId) -> BTreeMap<String, i64> {
        self.counter_limits
            .get(&object)
            .cloned()
            .unwrap_or_default()
    }

    fn saga_final_chapter(&self, object: ObjectId) -> Option<i64> {
        self.saga_final_chapter.get(&object).copied()
    }

    fn chapter_ability_pending(&self, object: ObjectId) -> bool {
        self.chapter_pending.contains(&object)
    }

    fn dungeons_at_bottommost_room(&self) -> BTreeSet<ObjectId> {
        self.bottommost_dungeons.clone()
    }

    fn room_ability_pending(&self, dungeon: ObjectId) -> bool {
        self.room_pending.contains(&dungeon)
    }

    fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
        self.deathtouch.clone()
    }

    fn clear_deathtouch_damaged(&mut self) {
        self.deathtouch.clear();
    }
}

/// The result of one [`perform_all`](super::perform_all) pass (REQ-SBA-001, CR 704.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SbaOutcome {
    /// Whether the pass changed the game state (CR 704.3: "If any state-based actions are
    /// performed as a result of a check, the check is repeated"): a move, a counter removal or
    /// an unattach event occurred, an object ceased to exist or was removed from the game, a
    /// player lost the game, or the replacement registry changed (for
    /// example a one-use replacement effect was consumed). A finding whose only move a
    /// replacement effect skipped, with nothing else to do, is not performed. Consuming the empty-library-draw and
    /// deathtouch flags alone does not count.
    pub performed: bool,
    /// Every finding of the pass, in registry order, then subject order: what the check
    /// detected, whether or not it was performed.
    pub findings: Vec<SbaFinding>,
    /// The batches the replacement bus executed (zone changes, counter removals and unattach
    /// events) for the pass's ONE batch, in execution order (empty if nothing occurred).
    pub executed: Vec<ExecutedBatch>,
    /// The players who lost the game in this pass, ascending.
    pub lost_players: Vec<PlayerId>,
    /// For each player who lost in this pass, every entry id that applied to them, in registry
    /// order (CR 704.7: one loss per player).
    pub loss_reasons: BTreeMap<PlayerId, Vec<&'static str>>,
    /// The objects that ceased to exist (CR 704.5d, CR 704.5e), ascending. For an object a
    /// replacement effect of the pass moved before it ceased, this is its new id (CR 400.7).
    pub ceased: Vec<ObjectId>,
    /// The permanents that became unattached and remain on the battlefield (CR 704.5n,
    /// CR 704.5p, CR 701.3d), ascending, without duplicates: the attachment of every
    /// `GameEvent::Unattach` event of the batch that occurred (it is in
    /// [`SbaOutcome::executed`]; an unattach that found nothing attached is no event), planned
    /// by the pass or produced by a replacement effect (for example an `Instead` of another
    /// event of the batch), that is still on the battlefield as the same object at the END of
    /// the pass. A permanent whose unattach event ran but that left the battlefield in the
    /// same pass (its own state-based move, or a replacement effect that moved it) is not
    /// listed, though its event stays in `executed`; one whose move a replacement effect
    /// prevented is listed (it stays).
    pub unattached: Vec<ObjectId>,
    /// The dungeon cards their owners removed from the game (CR 704.5t, CR 309.7), ascending.
    /// Removing a dungeon is not a zone change: no `ZoneChange` event, nothing in `executed`.
    pub removed_from_game: Vec<ObjectId>,
}
