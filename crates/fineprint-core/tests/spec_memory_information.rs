// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `information_for`: current information versus last known information
//! (REQ-CORE-003, CR 608.2h, 608.2b, 113.7a, 604.7). Black-box: written from the work-item
//! brief only.

#[path = "spec_memory_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::memory::{Information, InformationUser, LkiRecord, information_for};
use fineprint_core::zones::{MoveRequest, ObjectRecord, ZoneKind};
use support::*;

fn current<'a>(info: Option<Information<'a>>) -> Option<&'a ObjectRecord> {
    match info {
        Some(Information::Current(r)) => Some(r),
        _ => None,
    }
}

fn lki<'a>(info: Option<Information<'a>>) -> Option<&'a LkiRecord> {
    match info {
        Some(Information::Lki(r)) => Some(r),
        _ => None,
    }
}

fn spell(w: &World, id: ObjectId, expected: ZoneKind) -> Option<Information<'_>> {
    information_for(
        &w.zones,
        &w.lki,
        id,
        expected,
        InformationUser::SpellOrAbility,
    )
}

fn stat(w: &World, id: ObjectId, expected: ZoneKind) -> Option<Information<'_>> {
    information_for(
        &w.zones,
        &w.lki,
        id,
        expected,
        InformationUser::StaticAbility,
    )
}

// REQ-CORE-003, CR 608.2h: an object still in the public zone the effect expects gives its
// current information, for spells/abilities and for static abilities.
#[test]
fn info_current_in_expected_zone() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let r = current(spell(&w, a, ZoneKind::Battlefield)).expect("current");
    assert_eq!(r, w.zones.object(a).unwrap());
    assert_eq!(r.id, a);
    let r = current(stat(&w, a, ZoneKind::Battlefield)).expect("current for static too");
    assert_eq!(r.id, a);
}

// REQ-CORE-003, CR 608.2h, 608.2b, 113.7a: after the object leaves, a spell or ability asking
// about its old id gets the captured LKI.
#[test]
fn info_after_leaving_spell_gets_lki() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.provider.set_chars(a, chars("test.source_2_3", 2, 3));
    let moved = w.move_to(a, ZoneKind::Graveyard);
    let rec = lki(spell(&w, a, ZoneKind::Battlefield)).expect("lki");
    assert_eq!(rec, w.lki.get(a).unwrap());
    assert_eq!(rec.object(), a);
    assert_eq!(rec.characteristics(), &chars("test.source_2_3", 2, 3));
    // CR 608.2b: the source's LKI is what the legality check reads; whatever zone is expected
    // for the old id, it is not current.
    assert!(lki(spell(&w, a, ZoneKind::Graveyard)).is_some());
    // The new object is current where it is.
    let cur = current(spell(&w, moved.new, ZoneKind::Graveyard)).expect("current");
    assert_eq!(cur.id, moved.new);
}

// REQ-CORE-003, CR 604.7: static abilities never use LKI, before or after capture.
#[test]
fn info_static_ability_never_gets_lki() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    // Before capture: moved without capture, old id unknown.
    let m = w.raw_move(a, ZoneKind::Graveyard);
    assert!(stat(&w, a, ZoneKind::Battlefield).is_none());
    assert!(spell(&w, a, ZoneKind::Battlefield).is_none());
    // The graveyard object, expected on the battlefield, not captured: None for both.
    assert!(stat(&w, m.new, ZoneKind::Battlefield).is_none());
    // After capture: static still None, spell gets LKI.
    let b = w.create(ZoneKind::Battlefield, P2);
    w.move_to(b, ZoneKind::Exile);
    assert!(stat(&w, b, ZoneKind::Battlefield).is_none());
    assert!(lki(spell(&w, b, ZoneKind::Battlefield)).is_some());
}

// REQ-CORE-003, CR 608.2h: neither current nor captured gives None.
#[test]
fn info_none_when_nothing_known() {
    let w = World::new();
    assert!(spell(&w, oid(42), ZoneKind::Battlefield).is_none());
    assert!(stat(&w, oid(42), ZoneKind::Battlefield).is_none());
}

// REQ-CORE-003, CR 608.2h: an object in a zone other than the expected one is not current; the
// LKI of that id is used if captured, otherwise there is none.
#[test]
fn info_expected_zone_mismatch_falls_back_to_lki() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P1);
    // Not captured: no information for the wrong zone, current for the right one.
    assert!(spell(&w, g, ZoneKind::Battlefield).is_none());
    assert_eq!(current(spell(&w, g, ZoneKind::Graveyard)).unwrap().id, g);
    // Captured (the object is about to leave): the mismatch falls back to LKI.
    assert_eq!(w.capture(g, &MoveRequest::new(ZoneKind::Exile)), Ok(true));
    let rec = lki(spell(&w, g, ZoneKind::Battlefield)).expect("lki");
    assert_eq!(rec.object(), g);
    assert!(stat(&w, g, ZoneKind::Battlefield).is_none());
    // Still in its zone: current wins over LKI when the zone matches.
    assert_eq!(current(spell(&w, g, ZoneKind::Graveyard)).unwrap().id, g);
}

// REQ-CORE-003, CR 608.2h: an effect that moved the object from a public zone to a hidden one
// uses its LKI.
#[test]
fn info_public_to_hidden_uses_lki() {
    for to in [ZoneKind::Hand, ZoneKind::Library] {
        let mut w = World::new();
        let a = w.create(ZoneKind::Battlefield, P1);
        w.provider.set_counter(a, "+1/+1", 1);
        let m = w.move_to(a, to);
        let rec = lki(spell(&w, a, ZoneKind::Battlefield)).expect("lki");
        assert_eq!(rec.counters().get("+1/+1"), Some(&1));
        assert_eq!(rec.zone().kind(), ZoneKind::Battlefield);
        assert!(lki(spell(&w, a, to)).is_some(), "old id is never current");
        assert!(stat(&w, a, ZoneKind::Battlefield).is_none());
        assert_ne!(m.new, a);
    }
}

// REQ-CORE-003, CR 400.7, 608.2h: the new object's own id is current when its zone matches,
// and has no LKI of its own.
#[test]
fn info_new_object_is_current() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Stack, P1);
    let m = w.move_to(a, ZoneKind::Battlefield);
    let cur = current(spell(&w, m.new, ZoneKind::Battlefield)).expect("current");
    assert_eq!(cur.id, m.new);
    assert_eq!(cur.zone.kind(), ZoneKind::Battlefield);
    assert!(current(stat(&w, m.new, ZoneKind::Battlefield)).is_some());
    assert!(spell(&w, m.new, ZoneKind::Graveyard).is_none());
}
