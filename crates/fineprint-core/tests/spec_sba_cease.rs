// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `Zones::cease_to_exist`, the zone operation behind 704.5d and 704.5e
//! (REQ-SBA-001; CR 704.5d, 704.5e, 111.7). Black-box: written from the work-item brief only.

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::zones::{OutsidePool, ZoneError, ZoneId, ZoneKind};
use support::*;

const POOLS: [OutsidePool; 5] = [
    OutsidePool::Sideboard,
    OutsidePool::Companion,
    OutsidePool::Lesson,
    OutsidePool::AttractionDeck,
    OutsidePool::ContraptionDeck,
];

// REQ-SBA-001, CR 704.5d, 704.5e: `cease_to_exist` removes the object from every zone, returns
// its card and puts nothing into an outside-the-game pool; other objects are untouched. Checked
// in every zone kind that can hold one.
#[test]
fn req_sba_001_cease_to_exist_removes_object() {
    for kind in [
        ZoneKind::Library,
        ZoneKind::Hand,
        ZoneKind::Battlefield,
        ZoneKind::Graveyard,
        ZoneKind::Stack,
        ZoneKind::Exile,
    ] {
        let mut s = Sim::new();
        let other = s.create(kind, P1, blank("test.other", &["Artifact"]));
        let id = s.create(kind, P2, blank("test.copy", &["Artifact"]));
        let card = s.zones.object(id).expect("present").card;
        let zone = s.zones.object(id).expect("present").zone;
        assert_eq!(s.zones.cease_to_exist(id), Ok(card), "{kind:?}");
        assert!(s.zones.object(id).is_none(), "{kind:?}");
        assert!(s.zones.object_of_card(card).is_none(), "{kind:?}");
        assert!(!s.zones.contents(zone).contains(&id), "{kind:?}");
        assert!(!s.zones.all_object_ids().contains(&id), "{kind:?}");
        assert!(s.zones.object(other).is_some(), "{kind:?}");
        for p in [P1, P2] {
            for pool in POOLS {
                assert!(s.zones.outside_cards(p, pool).is_empty(), "{kind:?}");
            }
        }
    }
}

// REQ-SBA-001, CR 704.5d: an unknown id is `ZoneError::UnknownObject(id)` and the container is
// unchanged; so is an id that already ceased to exist.
#[test]
fn req_sba_001_cease_to_exist_unknown_id_changes_nothing() {
    let mut s = Sim::new();
    let id = s.create(ZoneKind::Graveyard, P1, blank("test.copy", &["Artifact"]));
    s.create(
        ZoneKind::Battlefield,
        P2,
        blank("test.other", &["Artifact"]),
    );
    let ghost = ObjectId::new(999_999);
    let before = s.zones.clone();
    assert_eq!(
        s.zones.cease_to_exist(ghost),
        Err(ZoneError::UnknownObject(ghost))
    );
    assert_eq!(s.zones, before);

    s.zones.cease_to_exist(id).expect("ceases");
    let after_first = s.zones.clone();
    assert_eq!(
        s.zones.cease_to_exist(id),
        Err(ZoneError::UnknownObject(id))
    );
    assert_eq!(s.zones, after_first);
    assert_eq!(s.zones.len(ZoneId::Graveyard(P1)), 0);
}
