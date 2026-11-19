// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Last known information, per-object Memory and the Linked Store (REQ-CORE-003,
//! REQ-CORE-007, REQ-CORE-002).
//!
//! **Last known information** (CR 608.2h, CR 603.10, glossary "Last Known Information"):
//! [`capture_lki_on_move`] snapshots an object into an immutable [`LkiRecord`] in an
//! [`LkiStore`]. The protocol is *capture before move*: call it before
//! [`Zones::move_object`](crate::zones::Zones::move_object), because afterwards the old object
//! no longer exists. For state-based actions performed at the same time, capture every
//! departing object first and then move them (CR 704.8). [`information_for`] chooses between
//! an object's current information and its LKI (CR 608.2h, CR 113.7a, CR 608.2b); a static
//! ability never gets LKI (CR 604.7). Fully evaluated characteristics come from a
//! [`CharacteristicsProvider`]; the layer system plugs in there later.
//!
//! **Memory** ([`Memory`], [`MemoryTable`]): per object id, the choices made for it, the
//! choices that persist across zone changes (CR 607.2p), the spell history a permanent can see
//! (CR 400.7d, CR 607.2i, CR 607.2j, CR 607.2q), its [`LinkedStore`] (CR 607) and the sources of
//! cross-object links (CR 607.1d). [`MemoryTable::apply_zone_change`] applies CR 400.7 after a
//! move: nothing carries over except the persistent choices and, from the stack to the
//! battlefield, the spell history.
//!
//! **Linked Store** ([`LinkedStore`]): per [`LinkId`](fineprint_base::ids::LinkId), entries in
//! insertion order (CR 607.3); an unwritten link is undefined (CR 607.5a). Objects are held by
//! [`DurableRef`] (card plus the object id at link time), which resolves only while the card's
//! object is still that generation, so a zone change breaks the link (CR 400.7, CR 607.2a).
//! [`MoveFinder`] lets the one effect or cost that moved an object to a public zone find it
//! (CR 400.7j). The registry entries for CR 400.7d and CR 400.7j
//! ([`exception_400_7d_spell_information`](crate::zones::exception_400_7d_spell_information),
//! [`exception_400_7j_effect_finds_moved_object`](crate::zones::exception_400_7j_effect_finds_moved_object))
//! report `Applies` and name the functions here.
//!
//! **Determinism.** Every collection is a `BTreeMap` or a sorted `Vec`; every type has a
//! canonical encoding with a stable tag per enum variant (REQ-CORE-001, REQ-CORE-006,
//! REQ-NFR-006). Components sit behind `Arc` and are written with `Arc::make_mut`, so `clone()`
//! is a cheap, independent snapshot.
//!
//! **Omniscient state.** Memory and LKI expose hidden information (choices, objects that went
//! to a hidden zone, face-down objects); they stay behind the redacted per-player view
//! (REQ-CORE-004).
//!
//! **Standalone scope.** Nothing here is wired into `GameState`, its Merkle layout or its wire
//! form; the integration unit does that.

mod error;
mod linked;
mod lki;
mod record;

#[cfg(test)]
mod tests;

pub use error::MemoryError;
pub use linked::{DurableRef, LinkEntry, LinkedStore, MoveFinder};
pub use lki::{
    CharacteristicsProvider, Information, InformationUser, LkiRecord, LkiStore,
    capture_lki_on_move, information_for,
};
pub use record::{ChoiceValue, Memory, MemoryTable, SpellHistory};
