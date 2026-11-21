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
/// hold yet (REQ-SBA-001). Every method has a default: a standard game with no copies, no
/// exemption and no deathtouch damage.
///
/// Every method must be a pure function of the caller's state (REQ-CORE-001). The heartbeat
/// and layer-integration units replace these seams with engine state.
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
    /// Default: none.
    fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
        BTreeSet::new()
    }

    /// Forgets [`SbaEnvironment::deathtouch_damaged`]: called by
    /// [`perform_all`](super::perform_all) after every successful pass, since the condition is
    /// "since the last time state-based actions were checked" (CR 704.5h). Default: nothing.
    fn clear_deathtouch_damaged(&mut self) {}
}

/// A plain [`SbaEnvironment`] whose answers are stored sets (REQ-SBA-001): for callers and
/// tests that do not derive them from other state.
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
    /// Whether any state-based action applied (`false` if there was no finding at all).
    pub performed: bool,
    /// Every finding of the pass, in registry order, then subject order.
    pub findings: Vec<SbaFinding>,
    /// The batches the replacement bus executed for the zone changes, in execution order
    /// (empty if there was no move or nothing occurred).
    pub executed: Vec<ExecutedBatch>,
    /// The players who lost the game in this pass, ascending.
    pub lost_players: Vec<PlayerId>,
    /// For each player who lost in this pass, every entry id that applied to them, in registry
    /// order (CR 704.7: one loss per player).
    pub loss_reasons: BTreeMap<PlayerId, Vec<&'static str>>,
    /// The objects that ceased to exist (CR 704.5d, CR 704.5e), ascending.
    pub ceased: Vec<ObjectId>,
}
