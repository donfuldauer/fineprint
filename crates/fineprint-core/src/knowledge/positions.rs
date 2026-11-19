// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Position knowledge (`KnownPositions`) and library shuffles (REQ-CORE-004).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use fineprint_base::ids::{ObjectId, PlayerId};

use super::{Knowledge, KnowledgeError, library, record_of, require_player};
use crate::zones::{ZoneId, Zones};

impl Knowledge {
    /// Keeps only the position entries for which `keep(viewer, library_owner, object)` holds at
    /// their index; drops emptied maps. Does not write when nothing changes.
    pub(super) fn retain_positions(
        &mut self,
        mut keep: impl FnMut(PlayerId, PlayerId, ObjectId) -> bool,
    ) {
        self.retain_indexed(|viewer, owner, _, object| keep(viewer, owner, object));
    }

    /// Like [`Knowledge::retain_positions`] with the index available.
    fn retain_indexed(
        &mut self,
        mut keep: impl FnMut(PlayerId, PlayerId, usize, ObjectId) -> bool,
    ) {
        let changes = self
            .positions
            .iter()
            .any(|(&(v, o), entries)| entries.iter().any(|(&i, &obj)| !keep(v, o, i, obj)));
        if !changes {
            return;
        }
        let positions = Arc::make_mut(&mut self.positions);
        for (&(v, o), entries) in positions.iter_mut() {
            entries.retain(|&i, &mut obj| keep(v, o, i, obj));
        }
        positions.retain(|_, entries| !entries.is_empty());
    }

    /// The single position rule for a change of library contents (REQ-CORE-004; a design
    /// decision, not CR-derived, see `docs/DECISIONS.md`, "#25 FP-024: Position knowledge
    /// survives a library change only for viewers who know the index").
    ///
    /// For every library owner in `owners` (the affected libraries) and every viewer NOT in
    /// `index_known_by`: every entry for that library is dropped, because which entries a prune
    /// keeps depends on the side of the hidden change they sit on. For a viewer in
    /// `index_known_by`: an entry for an affected library is kept only if its object is still at
    /// the recorded index of the live library. In addition every
    /// entry of every viewer naming `gone` (the moved or removed object) is dropped. Entries for
    /// libraries not in `owners` are otherwise untouched.
    pub(super) fn apply_library_change(
        &mut self,
        zones: &Zones,
        owners: &[PlayerId],
        index_known_by: &[PlayerId],
        gone: Option<ObjectId>,
    ) {
        let live: BTreeMap<PlayerId, Vec<ObjectId>> = owners
            .iter()
            .map(|&owner| (owner, library(zones, owner)))
            .collect();
        let known: BTreeSet<PlayerId> = index_known_by.iter().copied().collect();
        self.retain_indexed(|viewer, owner, index, object| {
            if gone == Some(object) {
                return false;
            }
            let Some(live) = live.get(&owner) else {
                return true;
            };
            known.contains(&viewer) && live.get(index) == Some(&object)
        });
    }

    /// Like [`Knowledge::on_library_changed_indexed`] with nobody knowing the insertion index:
    /// every viewer loses every position entry for the library of `owner` (REQ-CORE-004). It
    /// only touches position entries, never `KnownTo`, so set knowledge of the library survives.
    ///
    /// **Caller contract.** Call it (or the indexed form) after every [`Zones`] mutation that
    /// inserts into a library outside `on_move`: `Zones::create_object` or
    /// `Zones::enter_from_outside` into a library. Removals (`Zones::send_outside` of a library
    /// or hand object) use [`Knowledge::on_removed_from_hidden_zone`] instead.
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownPlayer`]`(owner)`; the state is then unchanged.
    pub fn on_library_changed(
        &mut self,
        zones: &Zones,
        owner: PlayerId,
    ) -> Result<(), KnowledgeError> {
        self.on_library_changed_indexed(zones, owner, &[])
    }

    /// Re-checks position knowledge of the library of `owner` after an insertion that did not go
    /// through [`Knowledge::on_move_indexed`] (REQ-CORE-004). `index_known_by` are the players
    /// who know the library index at which the card was inserted.
    ///
    /// Every viewer NOT in `index_known_by` loses every position entry for that library: had
    /// the insertion been above or below an entry, keeping or dropping that entry would tell the
    /// viewer which side it went (a design decision, see `docs/DECISIONS.md`, "#25 FP-024:
    /// Position knowledge survives a library change only for viewers who know the index"). For a
    /// viewer in `index_known_by`, after an insertion at index `n` (0 is the top), entries with
    /// an index `< n` are kept and entries with an index `>= n` are dropped, because those
    /// objects shifted. Never touches `KnownTo`.
    ///
    /// A library is one face-down pile whose order players can't look at or change (CR 401.2),
    /// so a position entry is knowledge the engine tracked, not something a player can
    /// re-check. Dropping an entry as soon as it no longer matches is the engine's own
    /// conservative policy, not a CR rule: once stale, an entry is never allowed to become
    /// valid again by coincidence when a later change restores the earlier shape of the
    /// library.
    ///
    /// **Caller contract.** Pass a player in `index_known_by` only when the insertion index is
    /// public to that player (for example a card put "third from the top" in view of all).
    /// An announced identity is not an index.
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownPlayer`]`(owner)`, then [`KnowledgeError::UnknownPlayer`] for the
    /// first player of `index_known_by` not in `zones`. The state is then unchanged.
    pub fn on_library_changed_indexed(
        &mut self,
        zones: &Zones,
        owner: PlayerId,
        index_known_by: &[PlayerId],
    ) -> Result<(), KnowledgeError> {
        require_player(zones, owner)?;
        for &player in index_known_by {
            require_player(zones, player)?;
        }
        self.apply_library_change(zones, &[owner], index_known_by, None);
        Ok(())
    }

    /// Records that `viewer` knows the object `id` is at `index` (0 is the top) of the library
    /// of `library_owner`, and adds `viewer` to the card's `KnownTo` (REQ-CORE-004). Replaces
    /// any earlier entry of the same (viewer, library owner) naming the same object or the same
    /// index.
    ///
    /// A position entry never makes `viewer` an observer of a later move out of that library,
    /// nor a player who knows the index of a later change: pass `viewer` in the `observers` and
    /// `index_known_by` of [`Knowledge::on_move_indexed`] when the moved card and its position
    /// are public to it; otherwise `viewer` loses its entries for that library.
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownPlayer`] (viewer, then library owner),
    /// [`KnowledgeError::UnknownObject`], [`KnowledgeError::NotInLibrary`] if the object is not
    /// in that library, [`KnowledgeError::IndexMismatch`] if it is not at `index` (including an
    /// index past the bottom). The state is then unchanged.
    pub fn set_position_known(
        &mut self,
        zones: &Zones,
        viewer: PlayerId,
        library_owner: PlayerId,
        index: usize,
        id: ObjectId,
    ) -> Result<(), KnowledgeError> {
        // CR 401.4: the owner who arranges cards in a library knows their positions; the caller
        // records that (and any look at library cards) here.
        require_player(zones, viewer)?;
        require_player(zones, library_owner)?;
        let record = record_of(zones, id)?;
        if record.zone != ZoneId::Library(library_owner) {
            return Err(KnowledgeError::NotInLibrary {
                owner: library_owner,
                object: id,
            });
        }
        if library(zones, library_owner).get(index) != Some(&id) {
            return Err(KnowledgeError::IndexMismatch {
                owner: library_owner,
                index,
                object: id,
            });
        }
        let entries = Arc::make_mut(&mut self.positions)
            .entry((viewer, library_owner))
            .or_default();
        entries.retain(|&i, &mut obj| i != index && obj != id);
        entries.insert(index, id);
        self.add_players(record.card, [viewer]);
        Ok(())
    }

    /// The positions `viewer` knows in the library of `library_owner`: index (0 is the top) to
    /// object, only for entries whose object is still at that index of that library in `zones`
    /// (REQ-CORE-004). Empty for unknown players.
    #[must_use]
    pub fn position_known(
        &self,
        zones: &Zones,
        viewer: PlayerId,
        library_owner: PlayerId,
    ) -> BTreeMap<usize, ObjectId> {
        let Some(entries) = self.positions.get(&(viewer, library_owner)) else {
            return BTreeMap::new();
        };
        let live = library(zones, library_owner);
        entries
            .iter()
            .filter(|&(&i, obj)| live.get(i) == Some(obj))
            .map(|(&i, &obj)| (i, obj))
            .collect()
    }

    /// Shuffle of the library of `owner` (CR 701.24a). `excluded` are the CR 701.24b found
    /// cards: they are validated (they must be in that library) but, since every position
    /// entry of the library is cleared, they no longer keep any entry.
    ///
    /// A shuffle clears ALL position entries of that library for EVERY viewer (SRS 2.4,
    /// "Shuffling a library clears all position knowledge for that library"); entries of other
    /// libraries are untouched. It never touches `KnownTo`: identity knowledge of the library as
    /// a set survives the shuffle (SRS 2.4). A found card whose placement is disclosed
    /// afterwards (for example CR 401.4, the owner arranges cards and knows their positions) is
    /// re-established by the caller with [`Knowledge::set_position_known`] after this call (see
    /// `docs/DECISIONS.md`, "#25 FP-024: A shuffle clears all position knowledge of the library").
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownPlayer`]`(owner)`; then [`KnowledgeError::NotInLibrary`] for the
    /// first excluded id that is unknown or not in that library. The state is then unchanged.
    pub fn shuffle_library(
        &mut self,
        zones: &Zones,
        owner: PlayerId,
        excluded: &[ObjectId],
    ) -> Result<(), KnowledgeError> {
        // CR 701.24a: after a shuffle no player knows the order of the shuffled cards.
        // CR 701.24b: found cards are not included in the shuffle (validated here only).
        require_player(zones, owner)?;
        for &id in excluded {
            if zones.object(id).map(|r| r.zone) != Some(ZoneId::Library(owner)) {
                return Err(KnowledgeError::NotInLibrary { owner, object: id });
            }
        }
        self.retain_positions(|_, library_owner, _| library_owner != owner);
        Ok(())
    }
}
