// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Per-object Memory: choices, spell history and the Linked Store (REQ-CORE-007).

use std::collections::BTreeMap;
use std::sync::Arc;

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::{LinkId, ObjectId, PlayerId};

use super::error::MemoryError;
use super::linked::{DurableRef, LinkEntry, LinkedStore};
use crate::zones::{Moved, ZoneKind, Zones};

/// A value chosen for an object, for example as it entered the battlefield or as it resolved
/// (REQ-CORE-007, CR 607.2d, CR 614.12a). Read back by the linked ability that refers to "the
/// chosen [value]".
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChoiceValue {
    /// A chosen color.
    Color(String),
    /// A chosen creature type.
    CreatureType(String),
    /// A chosen card name.
    CardName(String),
    /// A chosen player.
    Player(PlayerId),
    /// A chosen number.
    Number(i64),
    /// A chosen mode (an index).
    Mode(u32),
    /// A chosen anchor word (CR 614.12c).
    Anchor(String),
}

impl CanonicalEncode for ChoiceValue {
    /// A variant tag (`Color` 0, `CreatureType` 1, `CardName` 2, `Player` 3, `Number` 4,
    /// `Mode` 5, `Anchor` 6) then the payload (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            ChoiceValue::Color(s) => {
                put_variant(out, 0);
                s.encode_canonical(out);
            }
            ChoiceValue::CreatureType(s) => {
                put_variant(out, 1);
                s.encode_canonical(out);
            }
            ChoiceValue::CardName(s) => {
                put_variant(out, 2);
                s.encode_canonical(out);
            }
            ChoiceValue::Player(p) => {
                put_variant(out, 3);
                p.encode_canonical(out);
            }
            ChoiceValue::Number(n) => {
                put_variant(out, 4);
                n.encode_canonical(out);
            }
            ChoiceValue::Mode(m) => {
                put_variant(out, 5);
                m.encode_canonical(out);
            }
            ChoiceValue::Anchor(s) => {
                put_variant(out, 6);
                s.encode_canonical(out);
            }
        }
    }
}

/// What a permanent can see about the spell it was (REQ-CORE-007, CR 400.7d): costs paid,
/// mana spent, additional costs, the value of X and the face it was cast with. Keys are
/// generic rules identifiers, never card names (REQ-DSL-013).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpellHistory {
    /// Identifiers of the costs paid, in payment order (CR 400.7d).
    pub costs_paid: Vec<String>,
    /// Mana spent: (color, source type) to amount (CR 400.7d).
    pub mana_spent: BTreeMap<(String, String), i64>,
    /// Whether the intent to pay each additional cost was declared, for example `"kicker"`
    /// (CR 607.2i).
    pub additional_costs: BTreeMap<String, bool>,
    /// The value chosen for X or another variable additional cost as it was cast (CR 607.2j).
    pub x_value: Option<i64>,
    /// The face or alternative characteristics it was cast with.
    pub cast_face: Option<String>,
    /// The cards exiled to pay its cost, by durable reference (CR 607.2q).
    pub exiled_to_pay: Vec<DurableRef>,
}

impl CanonicalEncode for SpellHistory {
    /// Each field in declaration order (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.costs_paid.encode_canonical(out);
        self.mana_spent.encode_canonical(out);
        self.additional_costs.encode_canonical(out);
        self.x_value.encode_canonical(out);
        self.cast_face.encode_canonical(out);
        self.exiled_to_pay.encode_canonical(out);
    }
}

/// The Memory of one object (REQ-CORE-007): choices made for it, choices that persist across
/// zone changes (CR 607.2p), the spell history it can see (CR 400.7d), its Linked Store
/// (CR 607) and the sources of cross-object links (CR 607.1d).
///
/// A key holds one choice: setting a choice, ordinary or persistent, replaces any earlier
/// choice under that key ("the last chosen [value]", CR 607.2d).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    choices: BTreeMap<String, ChoiceValue>,
    persistent_choices: BTreeMap<String, ChoiceValue>,
    spell_history: Option<SpellHistory>,
    linked: LinkedStore,
    link_sources: BTreeMap<LinkId, DurableRef>,
}

impl Memory {
    /// An empty Memory: no choice, no spell history, no link (CR 400.7).
    #[must_use]
    pub fn new() -> Memory {
        Memory::default()
    }

    /// Records an ordinary choice under `key`; it is lost when the object changes zones
    /// (CR 400.7, CR 607.2d).
    pub fn set_choice(&mut self, key: &str, value: ChoiceValue) {
        self.persistent_choices.remove(key);
        self.choices.insert(key.to_owned(), value);
    }

    /// Records a choice under `key` that survives zone changes: the choice for a
    /// characteristic-defining ability made before the game begins (CR 607.2p).
    pub fn set_persistent_choice(&mut self, key: &str, value: ChoiceValue) {
        self.choices.remove(key);
        self.persistent_choices.insert(key.to_owned(), value);
    }

    /// The choice under `key`, persistent or ordinary; `None` means undefined (CR 607.5a).
    #[must_use]
    pub fn choice(&self, key: &str) -> Option<&ChoiceValue> {
        self.persistent_choices
            .get(key)
            .or_else(|| self.choices.get(key))
    }

    /// Sets the spell history the permanent can see (CR 400.7d).
    pub fn set_spell_history(&mut self, h: SpellHistory) {
        self.spell_history = Some(h);
    }

    /// The spell history, if the object has one (CR 400.7d).
    #[must_use]
    pub fn spell_history(&self) -> Option<&SpellHistory> {
        self.spell_history.as_ref()
    }

    /// The object's Linked Store (CR 607).
    #[must_use]
    pub fn linked(&self) -> &LinkedStore {
        &self.linked
    }

    /// The object's Linked Store, for writing (CR 607).
    pub fn linked_mut(&mut self) -> &mut LinkedStore {
        &mut self.linked
    }

    /// The object whose ability created or put this object onto the battlefield under `link`,
    /// for a link across two objects (CR 607.1d); `None` if there is none.
    #[must_use]
    pub fn link_source(&self, link: LinkId) -> Option<DurableRef> {
        self.link_sources.get(&link).copied()
    }

    /// Records that `link` on this object comes from `source` (CR 607.1d). Usually called
    /// through [`MemoryTable::link_created`].
    pub fn set_link_source(&mut self, link: LinkId, source: DurableRef) {
        self.link_sources.insert(link, source);
    }

    /// Whether the Memory holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.choices.is_empty()
            && self.persistent_choices.is_empty()
            && self.spell_history.is_none()
            && self.linked.is_empty()
            && self.link_sources.is_empty()
    }
}

impl CanonicalEncode for Memory {
    /// The ordinary choices, the persistent choices (each a count and ascending key/value
    /// pairs), the spell history (`Option`), the Linked Store and the link sources (a count
    /// and ascending link/reference pairs) (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.choices.encode_canonical(out);
        self.persistent_choices.encode_canonical(out);
        self.spell_history.encode_canonical(out);
        self.linked.encode_canonical(out);
        self.link_sources.encode_canonical(out);
    }
}

/// The Memory of every object that has one, keyed by object id (REQ-CORE-007).
///
/// Cloning is a cheap snapshot (`Arc` + `Arc::make_mut`); mutating a clone never changes the
/// original.
///
/// **Omniscient state.** Memory can hold hidden information (for example a choice made face
/// down, or the memory of an object in a hand); the table stays behind the redacted per-player
/// view (REQ-CORE-004).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryTable {
    memories: Arc<BTreeMap<ObjectId, Memory>>,
}

impl MemoryTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> MemoryTable {
        MemoryTable::default()
    }

    /// The Memory of `id`, if it has one.
    #[must_use]
    pub fn get(&self, id: ObjectId) -> Option<&Memory> {
        self.memories.get(&id)
    }

    /// The Memory of `id` for writing, created empty if it has none (REQ-CORE-007).
    pub fn get_mut_or_default(&mut self, id: ObjectId) -> &mut Memory {
        Arc::make_mut(&mut self.memories).entry(id).or_default()
    }

    /// The number of objects with a Memory.
    #[must_use]
    pub fn len(&self) -> usize {
        self.memories.len()
    }

    /// Whether no object has a Memory.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.memories.is_empty()
    }

    /// The ids of the objects with a Memory, ascending.
    #[must_use]
    pub fn ids(&self) -> Vec<ObjectId> {
        self.memories.keys().copied().collect()
    }

    /// Links two objects (CR 607.1d): stores on `source`, under `link`, an object entry naming
    /// `created`, and records on `created` that `link` comes from `source`. Both references are
    /// made with [`DurableRef::new`].
    ///
    /// # Errors
    /// [`MemoryError::UnknownObject`] if `source` or `created` is not in a zone; the table is
    /// then unchanged.
    pub fn link_created(
        &mut self,
        zones: &Zones,
        source: ObjectId,
        created: ObjectId,
        link: LinkId,
    ) -> Result<(), MemoryError> {
        let source_ref = DurableRef::new(zones, source)?;
        let created_ref = DurableRef::new(zones, created)?;
        // CR 607.1d: the link lives on the source; the created object knows where it comes from.
        self.get_mut_or_default(source)
            .linked_mut()
            .push(link, LinkEntry::Object(created_ref));
        self.get_mut_or_default(created)
            .set_link_source(link, source_ref);
        Ok(())
    }

    /// Applies CR 400.7 to the table after [`Zones::move_object`] returned `moved`
    /// (REQ-CORE-007).
    ///
    /// - A same-zone reposition (`moved.old == moved.new`) is not a zone change: nothing
    ///   changes.
    /// - Otherwise the old object's Memory is removed (CR 400.7: the new object has no memory
    ///   of its previous existence) and, if there was one, the new object gets a fresh Memory
    ///   holding only: the persistent choices, always (CR 607.2p); and, only for a move from the
    ///   stack to the battlefield, the spell history (CR 400.7d), including the cards exiled to
    ///   pay its cost (CR 607.2q). Ordinary choices, the Linked Store and link sources never
    ///   carry over.
    ///
    /// Returns `true` iff the move was a real zone change (`moved.old != moved.new`) and the old
    /// object had a Memory; a same-zone reposition returns `false`.
    pub fn apply_zone_change(&mut self, moved: &Moved) -> bool {
        if moved.old == moved.new {
            // CR 400.7: not a move from one zone to another.
            return false;
        }
        if !self.memories.contains_key(&moved.old) {
            return false;
        }
        let memories = Arc::make_mut(&mut self.memories);
        let Some(old) = memories.remove(&moved.old) else {
            return false;
        };
        let mut fresh = Memory {
            // CR 607.2p: the choice for a characteristic-defining ability keeps applying.
            persistent_choices: old.persistent_choices,
            ..Memory::default()
        };
        // CR 400.7d: the permanent can see the spell it was; CR 607.2q: including the cards
        // exiled to pay its cost.
        if moved.from.kind() == ZoneKind::Stack && moved.to.kind() == ZoneKind::Battlefield {
            fresh.spell_history = old.spell_history;
        }
        memories.insert(moved.new, fresh);
        true
    }
}

impl CanonicalEncode for MemoryTable {
    /// The number of entries, then each object id ascending with its Memory (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.memories.encode_canonical(out);
    }
}
