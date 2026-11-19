// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The history ledger behind `HISTORY(query)` value expressions (REQ-DSL-003).

use std::collections::BTreeMap;
use std::sync::Arc;

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::PlayerId;

/// A statistic the history ledger counts (REQ-DSL-003).
///
/// Canonical tags: `SpellsCast` 0, `LifeGained` 1, `LifeLost` 2, `CardsDrawn` 3,
/// `DamageDealtBy` 4, `DamageDealtToPlayer` 5, `TokensCreated` 6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HistoryStat {
    /// Spells cast by the player (fed by [`History::record_spell_cast`]).
    SpellsCast,
    /// Life gained by the player.
    LifeGained,
    /// Life lost by the player, including life lost to damage (CR 119.2, CR 120.3a).
    LifeLost,
    /// Cards drawn by the player.
    CardsDrawn,
    /// Damage dealt by sources the player controlled.
    DamageDealtBy,
    /// Damage dealt to the player.
    DamageDealtToPlayer,
    /// Tokens the player created (owns).
    TokensCreated,
}

impl HistoryStat {
    fn tag(self) -> u8 {
        match self {
            HistoryStat::SpellsCast => 0,
            HistoryStat::LifeGained => 1,
            HistoryStat::LifeLost => 2,
            HistoryStat::CardsDrawn => 3,
            HistoryStat::DamageDealtBy => 4,
            HistoryStat::DamageDealtToPlayer => 5,
            HistoryStat::TokensCreated => 6,
        }
    }
}

impl CanonicalEncode for HistoryStat {
    /// The one-byte tag listed on the type.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(out, self.tag());
    }
}

/// The time span a history query covers (REQ-DSL-003).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HistoryScope {
    /// One turn, by game turn number.
    Turn(u64),
    /// The whole game.
    Game,
}

/// One `HISTORY(query)` question (REQ-DSL-003).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HistoryQuery {
    /// What is counted.
    pub stat: HistoryStat,
    /// Over which span.
    pub scope: HistoryScope,
    /// For which player; `None` is the sum over all players.
    pub player: Option<PlayerId>,
}

/// Per-turn, per-player counts of what happened in the game (REQ-DSL-003): spells cast, life
/// gained and lost, cards drawn, damage dealt and tokens created.
///
/// Built only from executed events (so a replaced or prevented thing is never counted), keyed
/// by the game turn number at execution time, plus [`History::record_spell_cast`]. Sums
/// saturate. Cloning is a cheap snapshot (`Arc` + `Arc::make_mut`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    counts: Arc<BTreeMap<(u64, HistoryStat, PlayerId), i64>>,
}

impl History {
    /// The answer to `q`: the saturating sum of the matching counts; 0 when nothing matches.
    #[must_use]
    pub fn query(&self, q: &HistoryQuery) -> i64 {
        self.counts
            .iter()
            .filter(|((turn, stat, player), _)| {
                *stat == q.stat
                    && match q.scope {
                        HistoryScope::Turn(t) => *turn == t,
                        HistoryScope::Game => true,
                    }
                    && q.player.is_none_or(|p| p == *player)
            })
            .fold(0_i64, |acc, (_, n)| acc.saturating_add(*n))
    }

    /// Records that `player` cast a spell on turn `turn` (REQ-DSL-003; called by the casting
    /// unit as a spell becomes cast, CR 601.2i). Adds 1 to [`HistoryStat::SpellsCast`].
    pub fn record_spell_cast(&mut self, turn: u64, player: PlayerId) {
        self.add(turn, HistoryStat::SpellsCast, player, 1);
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Adds `amount` (saturating) to one count. Amounts of 0 or less record nothing.
    pub(super) fn add(&mut self, turn: u64, stat: HistoryStat, player: PlayerId, amount: i64) {
        if amount <= 0 {
            return;
        }
        let entry = Arc::make_mut(&mut self.counts)
            .entry((turn, stat, player))
            .or_insert(0);
        *entry = entry.saturating_add(amount);
    }
}

impl CanonicalEncode for History {
    /// The entry count, then each entry ascending by (turn, stat, player): the turn as `u64`,
    /// the stat tag, the player id and the count as `i64` (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.counts.encode_canonical(out);
    }
}
