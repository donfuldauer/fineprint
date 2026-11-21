// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the Saga and dungeon SBAs (REQ-SBA-001, REQ-CORE-001; CR 704.5s, 704.5t,
//! 715.2d, 715.4, 309.2c, 309.7). Black-box: written from the work-item brief only. Every card
//! is synthetic (R5).
//!
//! CR 704.5s: a Saga with one or more chapter abilities whose lore counters are at least its
//! final chapter number, and that is not the source of a chapter ability that has triggered but
//! not yet left the stack, is sacrificed by its controller. CR 704.5t: a dungeon whose venture
//! marker is on its bottommost room, and that is not the source of a pending room ability, is
//! removed from the game by its owner.

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_core::events::ZoneChangeCause;
use fineprint_core::zones::ZoneKind;
use support::*;

// ---------------------------------------------------------------------------------------------
// 704.5s
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5s, 715.4: lore counters equal to and above the final chapter number:
// the Saga is sacrificed (ZoneChange with cause Sacrifice) into its owner's graveyard, also
// when its owner is not its controller.
#[test]
fn req_sba_001_704_5s_saga_sacrificed() {
    for lore in [3, 4] {
        let mut s = Sim::new();
        let saga = s.saga(P2, P1, Some(3), lore);
        assert_eq!(s.check(), vec![finding("704.5s", obj(saga))]);
        let r = run_sba(s);
        assert!(r.outcome().performed);
        assert_eq!(r.outcome().findings, vec![finding("704.5s", obj(saga))]);
        assert_to_owners_graveyard(&r, saga, ZoneChangeCause::Sacrifice);
        assert!(r.outcome().ceased.is_empty());
        assert!(r.outcome().removed_from_game.is_empty());
        assert_eq!(r.nonempty_batches().len(), 1);
    }
}

// REQ-SBA-001, CR 704.5s: fewer lore counters than the final chapter number: untouched.
#[test]
fn req_sba_001_704_5s_below_final_chapter_untouched() {
    for lore in [0, 2] {
        let mut s = Sim::new();
        let saga = s.saga(P1, P1, Some(3), lore);
        assert!(s.check().is_empty());
        let r = run_sba(s);
        assert!(!r.outcome().performed);
        assert_still_on_battlefield(&r, saga);
    }
}

// REQ-SBA-001, CR 704.5s, 715.2d: a Saga with no chapter abilities (`saga_final_chapter` None)
// is untouched whatever its lore counters.
#[test]
fn req_sba_001_704_5s_no_chapter_abilities_untouched() {
    let mut s = Sim::new();
    let saga = s.saga(P1, P1, None, 5);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert_still_on_battlefield(&r, saga);
}

// REQ-SBA-001, CR 704.5s: a Saga that is the source of a chapter ability that has triggered but
// not yet left the stack is untouched.
#[test]
fn req_sba_001_704_5s_pending_chapter_ability_untouched() {
    let mut s = Sim::new();
    let saga = s.saga(P1, P1, Some(2), 2);
    s.env.chapter_pending.insert(saga);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert_still_on_battlefield(&r, saga);
}

// REQ-SBA-001, CR 704.5s: only a permanent with the Saga subtype is sacrificed; an enchantment
// without it, given a final chapter number and lore counters, is untouched.
#[test]
fn req_sba_001_704_5s_requires_saga_subtype() {
    let mut s = Sim::new();
    let id = s.permanent(P1, P1, typed(&["test.saga_a"], &[], &["Enchantment"], &[]));
    s.env.saga_final_chapter.insert(id, 1);
    s.counters(id, "lore", 3);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert_still_on_battlefield(&r, id);
}

// REQ-SBA-001, CR 704.5s, 702.26b: a phased-out Saga is skipped.
#[test]
fn req_sba_001_704_5s_phased_out_skipped() {
    let mut s = Sim::new();
    let saga = s.saga(P1, P1, Some(1), 1);
    s.phase_out(saga);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert_still_on_battlefield(&r, saga);
}

// ---------------------------------------------------------------------------------------------
// 704.5t
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5t, 309.7: a dungeon on its bottommost room is removed from the game with
// no zone-change event; it is listed in `removed_from_game`, not in `ceased`.
#[test]
fn req_sba_001_704_5t_dungeon_removed() {
    let mut s = Sim::new();
    let d = s.dungeon(P2, true);
    assert_eq!(s.check(), vec![finding("704.5t", obj(d))]);
    let r = run_sba(s);
    let o = r.outcome();
    assert!(o.performed);
    assert_eq!(o.findings, vec![finding("704.5t", obj(d))]);
    assert_eq!(o.removed_from_game, vec![d]);
    assert!(
        o.ceased.is_empty(),
        "removal is not ceasing (704.5d/e only)"
    );
    assert!(r.after.zones.object(d).is_none(), "no zone holds it");
    assert!(r.moved_to(d).is_none(), "no ZoneChange");
    assert!(r.moves().is_empty());
    assert!(
        r.after
            .zones
            .contents(fineprint_core::zones::ZoneId::Command)
            .is_empty(),
        "the command zone is empty"
    );
}

// REQ-SBA-001, CR 704.5t: two players' dungeons on their bottommost rooms are both removed,
// ascending; a dungeon not on its bottommost room stays.
#[test]
fn req_sba_001_704_5t_several_dungeons() {
    let mut s = Sim::new();
    let a = s.dungeon(P1, true);
    let stays = s.dungeon(P1, false);
    let b = s.dungeon(P2, true);
    assert_eq!(
        s.check(),
        vec![finding("704.5t", obj(a)), finding("704.5t", obj(b))]
    );
    let r = run_sba(s);
    assert_eq!(r.outcome().removed_from_game, vec![a, b]);
    assert_eq!(r.after.zone_of(stays), Some(ZoneKind::Command));
}

// REQ-SBA-001, CR 704.5t: a dungeon that is the source of a pending room ability stays.
#[test]
fn req_sba_001_704_5t_pending_room_ability_untouched() {
    let mut s = Sim::new();
    let d = s.dungeon(P1, true);
    s.env.room_pending.insert(d);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert!(r.outcome().removed_from_game.is_empty());
    assert_eq!(r.after.zone_of(d), Some(ZoneKind::Command));
}

// REQ-SBA-001, CR 704.5t: a dungeon not on its bottommost room stays.
#[test]
fn req_sba_001_704_5t_not_bottommost_untouched() {
    let mut s = Sim::new();
    let d = s.dungeon(P1, false);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(r.outcome().removed_from_game.is_empty());
    assert_eq!(r.after.zone_of(d), Some(ZoneKind::Command));
}

// REQ-SBA-001, CR 704.5t, 309.2c: only an object in the command zone is a dungeon card for
// 704.5t: an object outside it listed as bottommost is untouched.
#[test]
fn req_sba_001_704_5t_only_command_zone() {
    let mut s = Sim::new();
    let d = s.create(ZoneKind::Exile, P1, dungeon_chars("test.dungeon_a"));
    s.env.bottommost_dungeons.insert(d);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(r.outcome().removed_from_game.is_empty());
    assert_eq!(r.after.zone_of(d), Some(ZoneKind::Exile));
}

// REQ-SBA-001, CR 704.5s, 704.5t, 704.3: a Saga and a dungeon in one pass: findings in registry
// order; the Saga moves, the dungeon is removed.
#[test]
fn req_sba_001_saga_and_dungeon_one_pass() {
    let mut s = Sim::new();
    let d = s.dungeon(P1, true);
    let saga = s.saga(P1, P1, Some(2), 3);
    let expected = vec![finding("704.5s", obj(saga)), finding("704.5t", obj(d))];
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, expected);
    assert_to_owners_graveyard(&r, saga, ZoneChangeCause::Sacrifice);
    assert_eq!(r.outcome().removed_from_game, vec![d]);
}
