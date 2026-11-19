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

    /// Drops every entry for the library of `owner` that does not match the live library.
    pub(super) fn prune_library(&mut self, zones: &Zones, owner: PlayerId) {
        let live = library(zones, owner);
        self.retain_indexed(|_, o, index, object| o != owner || live.get(index) == Some(&object));
    }

    /// Records that `viewer` knows the object `id` is at `index` (0 is the top) of the library
    /// of `library_owner`, and adds `viewer` to the card's `KnownTo` (REQ-CORE-004). Replaces
    /// any earlier entry of the same (viewer, library owner) naming the same object or the same
    /// index.
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

    /// Shuffle of the library of `owner` (REQ-CORE-004). `excluded` are found cards that are
    /// not part of the shuffle. Removes the position knowledge of every viewer for that library,
    /// except entries whose object is in `excluded` and still at the recorded index in `zones`
    /// (pass the zones as they are at shuffle time). Never touches `KnownTo`: identity
    /// knowledge of the library as a set survives the shuffle (SRS 2.4).
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownPlayer`]`(owner)`; [`KnowledgeError::NotInLibrary`] for an
    /// excluded id that is unknown or not in that library. The state is then unchanged.
    pub fn shuffle_library(
        &mut self,
        zones: &Zones,
        owner: PlayerId,
        excluded: &[ObjectId],
    ) -> Result<(), KnowledgeError> {
        // CR 701.24a: after a shuffle no player knows the order of the shuffled cards.
        // CR 701.24b: found cards are not included in the shuffle.
        require_player(zones, owner)?;
        for &id in excluded {
            if zones.object(id).map(|r| r.zone) != Some(ZoneId::Library(owner)) {
                return Err(KnowledgeError::NotInLibrary { owner, object: id });
            }
        }
        let excluded: BTreeSet<ObjectId> = excluded.iter().copied().collect();
        let live = library(zones, owner);
        self.retain_indexed(|_, o, index, object| {
            o != owner || (excluded.contains(&object) && live.get(index) == Some(&object))
        });
        Ok(())
    }
}
