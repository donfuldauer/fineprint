// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Who knows what: identity and position knowledge, shuffles, and the redacted per-player
//! projection of the zones (REQ-CORE-004, REQ-ZONE-001).
//!
//! **Identity knowledge** (SRS 2.4 `KnownTo`). [`Knowledge`] stores, per
//! [`CardId`], the set of players who know that card's identity because it was revealed to
//! them, they looked at it, or they saw it in a public zone and tracked it since. It is stored
//! per card, not per object, because the object id changes at every zone change (CR 400.7)
//! while the players' memory of the card does not (a design decision, see `docs/DECISIONS.md`,
//! "#25 FP-024"). The stored set never includes the implicit entitlements, which
//! [`Knowledge::can_identify`] adds: public information (CR 400.2), the viewer's own hand
//! (CR 402.3) and the controller of a face-down permanent or spell (CR 708.5).
//!
//! **Position knowledge** (SRS 2.4 `KnownPositions`). Per (viewer, library owner), a map from
//! library index (0 is the top) to the object the viewer knows is there. Entries are filtered
//! lazily against the live library ([`Knowledge::position_known`]) and pruned when a library
//! changes through [`Knowledge::on_move`], so a stale entry never reports a position.
//!
//! **Shuffles.** [`Knowledge::shuffle_library`] clears position knowledge of the shuffled
//! cards (CR 701.24a, CR 701.24b) but keeps identity knowledge of the library as a set.
//! [`Knowledge::shuffle_face_down_pile`] ends look permissions on the shuffled face-down cards
//! (CR 701.24a, CR 406.3).
//!
//! **Projection.** [`Knowledge::project`] builds `V(S, P)`, a [`ZonesView`] that contains
//! exactly what the viewer may know: zone counts, the viewer's own hand, public objects, the
//! ids of face-down objects, and the [`CardId`]s the viewer can identify. Library knowledge is
//! a set of card ids, never a position or an object id (SRS 2.4). The wire view schema is later
//! work.
//!
//! **Omniscient state.** `Knowledge` is omniscient engine state: it records what every player
//! knows, including the identities of cards in hidden zones and in other players' minds. Like
//! [`Zones`](crate::zones::Zones) it must stay behind the redacted per-player view and the
//! determinized-fork boundary (REQ-CORE-004, REQ-API-004) and is never handed to a client
//! directly; only a [`ZonesView`] produced by [`Knowledge::project`] may be.
//!
//! **Determinism.** Every collection is a `BTreeMap`, a `BTreeSet` or a `Vec` in a stable order
//! (REQ-CORE-001, REQ-CORE-006); the canonical encoding iterates in ascending key order
//! (REQ-NFR-006). Components sit behind `Arc` and are written with `Arc::make_mut`, so `clone()`
//! is a cheap, independent snapshot.
//!
//! **Standalone scope.** Nothing here is wired into `GameState`, its Merkle layout or its wire
//! form; the integration unit does that. The caller calls [`Knowledge::on_move`] after every
//! [`Zones::move_object`](crate::zones::Zones::move_object).

mod error;
mod known_to;
mod positions;
mod projection;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use fineprint_base::encode::{CanonicalEncode, put_len};
use fineprint_base::ids::{CardId, ObjectId, PlayerId};

use crate::zones::{ObjectRecord, ZoneId, Zones};

pub use error::KnowledgeError;
pub use projection::{HandView, LibraryView, ObjectView, PlayerZonesView, ZonesView};

/// Per (viewer, library owner): library index (0 is the top) to the object known to be there.
type PositionMap = BTreeMap<(PlayerId, PlayerId), BTreeMap<usize, ObjectId>>;

/// Omniscient knowledge state: who knows which card's identity, and who knows which library
/// positions (REQ-CORE-004, SRS 2.4).
///
/// `Clone` is a cheap snapshot (structural sharing behind `Arc`, written with
/// `Arc::make_mut`); mutating a clone never changes the original. `Default` is the empty state,
/// equal to [`Knowledge::new`]. Equality compares values.
///
/// **Omniscient state.** Never hand a `Knowledge` to a client: it holds every player's
/// knowledge. Clients get a [`ZonesView`] from [`Knowledge::project`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Knowledge {
    /// `KnownTo` per card. Empty sets are never stored, so equal knowledge is an equal value.
    known: Arc<BTreeMap<CardId, BTreeSet<PlayerId>>>,
    /// `KnownPositions` per (viewer, library owner). Empty inner maps are never stored.
    positions: Arc<PositionMap>,
}

impl Knowledge {
    /// The empty knowledge state: nobody knows anything beyond the implicit entitlements.
    #[must_use]
    pub fn new() -> Knowledge {
        Knowledge::default()
    }
}

/// The record of `id`, or [`KnowledgeError::UnknownObject`].
fn record_of(zones: &Zones, id: ObjectId) -> Result<ObjectRecord, KnowledgeError> {
    zones
        .object(id)
        .copied()
        .ok_or(KnowledgeError::UnknownObject(id))
}

/// `Ok` if `player` is a player of `zones`, else [`KnowledgeError::UnknownPlayer`].
fn require_player(zones: &Zones, player: PlayerId) -> Result<(), KnowledgeError> {
    if zones.players().contains(&player) {
        Ok(())
    } else {
        Err(KnowledgeError::UnknownPlayer(player))
    }
}

/// The library contents of `owner`, top first.
fn library(zones: &Zones, owner: PlayerId) -> Vec<ObjectId> {
    zones.contents(ZoneId::Library(owner))
}

impl CanonicalEncode for Knowledge {
    /// The `KnownTo` map (a count, then per card in ascending order the card id and its
    /// ascending player set), then the position map (a count, then per (viewer, library owner)
    /// in ascending order both player ids, a count and per ascending index the index as a
    /// length-style prefix and the object id). Appends only (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_len(out, self.known.len());
        for (card, players) in self.known.iter() {
            card.encode_canonical(out);
            players.encode_canonical(out);
        }
        put_len(out, self.positions.len());
        for ((viewer, owner), entries) in self.positions.iter() {
            viewer.encode_canonical(out);
            owner.encode_canonical(out);
            put_len(out, entries.len());
            for (index, object) in entries {
                put_len(out, *index);
                object.encode_canonical(out);
            }
        }
    }
}
