// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Durable references, the Linked Store and the CR 400.7j move finder (REQ-CORE-007).

use std::collections::BTreeMap;
use std::sync::Arc;

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::{CardId, LinkId, ObjectId, PlayerId};

use super::error::MemoryError;
use crate::zones::{Moved, ZoneKind, Zones};

/// A durable reference to an object: the physical card plus the object id it had when the
/// reference was made, its "generation" (REQ-CORE-007, CR 400.7, CR 607.1).
///
/// The card persists across zone changes; the generation does not (CR 400.7: an object that
/// changes zones is a new object). So a durable reference resolves only while the card's
/// current object is still the generation it was linked to; after any zone change it stops
/// resolving, except through the exceptions that allow it ([`MoveFinder`], CR 400.7j).
///
/// Ordered by card, then generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DurableRef {
    card: CardId,
    generation: ObjectId,
}

impl DurableRef {
    /// A durable reference to the object `id` as it is now: its card and `id` as the
    /// generation (REQ-CORE-007, CR 607.1).
    ///
    /// # Errors
    /// [`MemoryError::UnknownObject`] if `id` is not in a zone.
    pub fn new(zones: &Zones, id: ObjectId) -> Result<DurableRef, MemoryError> {
        let record = zones.object(id).ok_or(MemoryError::UnknownObject(id))?;
        Ok(DurableRef {
            card: record.card,
            generation: id,
        })
    }

    /// A durable reference built from its parts (REQ-CORE-007, CR 607.1), for example when
    /// restoring a stored reference.
    #[must_use]
    pub fn from_parts(card: CardId, generation: ObjectId) -> DurableRef {
        DurableRef { card, generation }
    }

    /// The physical card (persists across zone changes, REQ-CORE-007).
    #[must_use]
    pub fn card(&self) -> CardId {
        self.card
    }

    /// The object id the card had when the reference was made (CR 400.7).
    #[must_use]
    pub fn generation(&self) -> ObjectId {
        self.generation
    }

    /// The object this reference names now: `Some(generation)` iff the card's current object
    /// is the generation (it has not changed zones since, CR 400.7) and, when `expected` is
    /// given, that object is in a zone of that kind (CR 607.2a: "the exiled cards" are only
    /// cards in the exile zone). `None` otherwise.
    #[must_use]
    pub fn resolve(&self, zones: &Zones, expected: Option<ZoneKind>) -> Option<ObjectId> {
        // CR 400.7: a zone change makes a new object, so only the same generation resolves.
        if zones.object_of_card(self.card) != Some(self.generation) {
            return None;
        }
        let record = zones.object(self.generation)?;
        match expected {
            Some(kind) if record.zone.kind() != kind => None,
            _ => Some(self.generation),
        }
    }
}

impl CanonicalEncode for DurableRef {
    /// The card id, then the generation (REQ-CORE-007, REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.card.encode_canonical(out);
        self.generation.encode_canonical(out);
    }
}

/// One entry of a link in the Linked Store: an object, a player or a value (REQ-CORE-007,
/// CR 607.1, CR 607.2).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkEntry {
    /// An object affected by the first ability, by durable reference (CR 607.2a).
    Object(DurableRef),
    /// A player affected or chosen by the first ability (CR 607.1).
    Player(PlayerId),
    /// A number recorded by the first ability (CR 607.1).
    Int(i64),
    /// Any other recorded value, such as a chosen word (CR 607.2d).
    Text(String),
}

impl CanonicalEncode for LinkEntry {
    /// A variant tag (`Object` 0, `Player` 1, `Int` 2, `Text` 3) then the payload
    /// (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            LinkEntry::Object(r) => {
                put_variant(out, 0);
                r.encode_canonical(out);
            }
            LinkEntry::Player(p) => {
                put_variant(out, 1);
                p.encode_canonical(out);
            }
            LinkEntry::Int(n) => {
                put_variant(out, 2);
                n.encode_canonical(out);
            }
            LinkEntry::Text(s) => {
                put_variant(out, 3);
                s.encode_canonical(out);
            }
        }
    }
}

/// The Linked Store of one object: per [`LinkId`], what the first of two linked abilities
/// produced, for the second to refer back to (REQ-CORE-007, CR 607.1, CR 607.2).
///
/// Keyed by `LinkId` in ascending order; the entries of one link keep insertion order
/// (CR 607.3). A link that was never written is "undefined" (CR 607.5a): [`LinkedStore::entries`]
/// returns `None` and the resolving functions return an empty list. Keyword macros allocate
/// their own link ids; there is no registry of hardcoded link kinds.
///
/// Cloning is a cheap snapshot (`Arc` + `Arc::make_mut`); mutating a clone never changes the
/// original.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkedStore {
    links: Arc<BTreeMap<LinkId, Vec<LinkEntry>>>,
}

impl LinkedStore {
    /// An empty store: every link undefined (CR 607.5a).
    #[must_use]
    pub fn new() -> LinkedStore {
        LinkedStore::default()
    }

    /// Appends `entry` to `link`, defining the link if it was undefined; insertion order is
    /// kept (CR 607.3).
    pub fn push(&mut self, link: LinkId, entry: LinkEntry) {
        // CR 607.3: a reference to several objects refers to each of them, in order.
        Arc::make_mut(&mut self.links)
            .entry(link)
            .or_default()
            .push(entry);
    }

    /// Defines `link` with no entries if it is undefined (the first ability resolved but
    /// affected nothing); an existing link is left as it is (CR 607.5a, CR 607.2a).
    pub fn define(&mut self, link: LinkId) {
        if !self.links.contains_key(&link) {
            Arc::make_mut(&mut self.links).insert(link, Vec::new());
        }
    }

    /// The entries of `link` in insertion order; `None` if the link is undefined (CR 607.5a).
    #[must_use]
    pub fn entries(&self, link: LinkId) -> Option<&[LinkEntry]> {
        self.links.get(&link).map(Vec::as_slice)
    }

    /// Makes `link` undefined again; returns whether it was defined (CR 607.5a).
    pub fn clear_link(&mut self, link: LinkId) -> bool {
        if self.links.contains_key(&link) {
            Arc::make_mut(&mut self.links).remove(&link);
            true
        } else {
            false
        }
    }

    /// Every defined link, ascending (REQ-CORE-006).
    #[must_use]
    pub fn links(&self) -> Vec<LinkId> {
        self.links.keys().copied().collect()
    }

    /// Whether no link is defined.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// The object entries of `link` that resolve now ([`DurableRef::resolve`] with
    /// `expected`), in insertion order; non-object entries are skipped and an undefined link
    /// gives an empty list (CR 607.2a, CR 607.5a, CR 400.7).
    #[must_use]
    pub fn resolve_objects(
        &self,
        link: LinkId,
        zones: &Zones,
        expected: Option<ZoneKind>,
    ) -> Vec<ObjectId> {
        // CR 607.2a: only objects still where the first ability put them (CR 400.7).
        self.object_refs(link)
            .filter_map(|r| r.resolve(zones, expected))
            .collect()
    }

    /// As [`LinkedStore::resolve_objects`], but a reference that no longer resolves is also
    /// followed through `finder`, the moves recorded by the effect or cost that is asking
    /// (CR 400.7j).
    #[must_use]
    pub fn resolve_objects_with(
        &self,
        link: LinkId,
        zones: &Zones,
        expected: Option<ZoneKind>,
        finder: &MoveFinder,
    ) -> Vec<ObjectId> {
        // CR 400.7j: the effect that moved the object can find it in the public zone.
        self.object_refs(link)
            .filter_map(|r| finder.resolve(r, zones, expected))
            .collect()
    }

    fn object_refs(&self, link: LinkId) -> impl Iterator<Item = &DurableRef> {
        self.links
            .get(&link)
            .into_iter()
            .flatten()
            .filter_map(|e| match e {
                LinkEntry::Object(r) => Some(r),
                _ => None,
            })
    }
}

impl CanonicalEncode for LinkedStore {
    /// The number of links, then each link ascending: its id, the number of entries and each
    /// entry in insertion order (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.links.encode_canonical(out);
    }
}

/// The moves one effect, triggered ability or cost made, so that it can find the objects it
/// moved (CR 400.7j; generic enough for CR 400.7e/h/i/k later) (REQ-CORE-007, REQ-CORE-002).
///
/// A finder is scoped to one effect or cost: it knows only the moves recorded into it, so a
/// different effect (with its own finder) cannot find those objects. It maps an old generation
/// to the latest object it became, and only through moves into public zones.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MoveFinder {
    found: BTreeMap<ObjectId, ObjectId>,
}

impl MoveFinder {
    /// A finder that has recorded no move.
    #[must_use]
    pub fn new() -> MoveFinder {
        MoveFinder::default()
    }

    /// Records a move made by this effect or cost (CR 400.7j). It is recorded only when it made
    /// a new object (`moved.old != moved.new`) in a public zone; returns whether it was
    /// recorded. A later move of an object this finder already tracks extends its chain to the
    /// newest object when the destination is again public; a move into a hidden zone drops the
    /// chain, since CR 400.7j finds objects only in a public zone.
    pub fn record(&mut self, moved: &Moved) -> bool {
        if moved.old == moved.new {
            // CR 400.7: a same-zone reposition is not a zone change; nothing new to find.
            return false;
        }
        if !moved.to.kind().is_public() {
            // CR 400.7j: "to a public zone"; an object moved into a hidden zone can't be found.
            self.found.retain(|k, v| *k != moved.old && *v != moved.old);
            return false;
        }
        for latest in self.found.values_mut() {
            if *latest == moved.old {
                *latest = moved.new;
            }
        }
        self.found.insert(moved.old, moved.new);
        true
    }

    /// The latest object recorded for `generation`, following the chain; `None` if nothing was
    /// recorded for it (CR 400.7j).
    #[must_use]
    pub fn find(&self, generation: ObjectId) -> Option<ObjectId> {
        self.found.get(&generation).copied()
    }

    /// The object `r` names for this effect: [`DurableRef::resolve`] first; else the object
    /// [`MoveFinder::find`] gives for `r`'s generation, if it is still in a zone, is the same
    /// card and (when `expected` is given) is in a zone of that kind (CR 400.7j).
    #[must_use]
    pub fn resolve(
        &self,
        r: &DurableRef,
        zones: &Zones,
        expected: Option<ZoneKind>,
    ) -> Option<ObjectId> {
        if let Some(id) = r.resolve(zones, expected) {
            return Some(id);
        }
        // CR 400.7j: the effect that moved the object can find the new object.
        let id = self.find(r.generation())?;
        let record = zones.object(id)?;
        if record.card != r.card() {
            return None;
        }
        match expected {
            Some(kind) if record.zone.kind() != kind => None,
            _ => Some(id),
        }
    }
}
