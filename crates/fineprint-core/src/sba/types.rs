// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The value types of the state-based action check (REQ-SBA-001): the game variant, findings,
//! the caller-supplied environment and the outcome of one pass.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId};

use crate::events::ExecutedBatch;

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
    /// An object (CR 704.5d-704.5i).
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
/// hold yet (REQ-SBA-001). Every method has a default (a standard game with no copies and no
/// exemption) except the deathtouch pair [`SbaEnvironment::deathtouch_damaged`] and
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
}

impl Default for StoredSbaEnvironment {
    fn default() -> StoredSbaEnvironment {
        StoredSbaEnvironment {
            variant: GameVariant::Standard,
            spell_copies: BTreeSet::new(),
            card_copies: BTreeSet::new(),
            exempt: BTreeSet::new(),
            deathtouch: BTreeSet::new(),
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
    /// performed as a result of a check, the check is repeated"): a move occurred, an object
    /// ceased to exist, a player lost the game, or the replacement registry changed (for
    /// example a one-use replacement effect was consumed). A finding whose only move a
    /// replacement effect skipped is not performed. Consuming the empty-library-draw and
    /// deathtouch flags alone does not count.
    pub performed: bool,
    /// Every finding of the pass, in registry order, then subject order: what the check
    /// detected, whether or not it was performed.
    pub findings: Vec<SbaFinding>,
    /// The batches the replacement bus executed for the zone changes, in execution order
    /// (empty if there was no move or nothing occurred).
    pub executed: Vec<ExecutedBatch>,
    /// The players who lost the game in this pass, ascending.
    pub lost_players: Vec<PlayerId>,
    /// For each player who lost in this pass, every entry id that applied to them, in registry
    /// order (CR 704.7: one loss per player).
    pub loss_reasons: BTreeMap<PlayerId, Vec<&'static str>>,
    /// The objects that ceased to exist (CR 704.5d, CR 704.5e), ascending. For an object a
    /// replacement effect of the pass moved before it ceased, this is its new id (CR 400.7).
    pub ceased: Vec<ObjectId>,
}
