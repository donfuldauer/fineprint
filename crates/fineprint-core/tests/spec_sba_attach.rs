// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the attachment SBAs (REQ-SBA-001, REQ-CORE-001; CR 704.5m, 704.5n, 704.5p,
//! 303.4c, 303.4d, 301.5, 301.6, 310.10, 702.26b). Black-box: written from the work-item brief
//! only. Every card is synthetic (R5).
//!
//! CR 704.5m: an Aura attached to an illegal object or player, or not attached, goes to its
//! owner's graveyard. CR 704.5n: an Equipment or Fortification attached to an illegal permanent
//! or to a player becomes unattached and stays. CR 704.5p: an attached battle or creature, or
//! an attached permanent that is none of Aura, Equipment, Fortification, becomes unattached and
//! stays.
//!
//! Not constructible through the public API and so not tested here: an object attached to
//! itself (`Permanents::attach` rejects it) and an attachment to a permanent no longer on the
//! battlefield (`Permanents::leave` unattaches everything attached to it, CR 701.3d; that case
//! is covered as "an Aura whose creature left", which then is not attached).

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::ZoneChangeCause;
use fineprint_core::objects::Characteristics;
use fineprint_core::permanent::AttachTarget;
use fineprint_core::zones::ZoneKind;
use support::*;

/// Asserts `id` was unattached by the pass: listed in `unattached`, unattached in the
/// container, still on the battlefield, not moved, and the pass performed.
fn assert_unattached(r: &SbaRun, id: ObjectId) {
    assert!(
        r.before.permanents.attached_to(id).is_some(),
        "{id:?} was attached"
    );
    assert!(
        r.outcome().unattached.contains(&id),
        "{id:?} in unattached: {:?}",
        r.outcome().unattached
    );
    assert_eq!(
        r.after.permanents.attached_to(id),
        None,
        "{id:?} unattached"
    );
    assert_still_on_battlefield(r, id);
    assert!(r.outcome().performed);
}

/// Asserts `id` is untouched: same attachment as before, on the battlefield, not unattached.
fn assert_untouched(r: &SbaRun, id: ObjectId) {
    assert_eq!(
        r.after.permanents.attached_to(id),
        r.before.permanents.attached_to(id),
        "{id:?} attachment unchanged"
    );
    assert!(
        !r.outcome().unattached.contains(&id),
        "{id:?} not unattached"
    );
    assert_still_on_battlefield(r, id);
}

/// Asserts `unattached` is ascending without duplicates.
fn assert_unattached_ascending(r: &SbaRun) {
    let mut sorted = r.outcome().unattached.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, r.outcome().unattached);
}

// ---------------------------------------------------------------------------------------------
// 704.5m
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5m: a legal Aura attached to a creature stays attached; nothing happens.
#[test]
fn req_sba_001_704_5m_legal_aura_stays() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let aura = s.permanent(P1, P1, aura_chars("test.aura_a"));
    s.attach_to(aura, c);
    // An Aura attached to a player who has not lost stays too.
    let on_player = s.permanent(P1, P1, aura_chars("test.aura_b"));
    s.attach(on_player, AttachTarget::Player(P2));
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert_untouched(&r, aura);
    assert_untouched(&r, on_player);
}

// REQ-SBA-001, CR 704.5m: an Aura not attached to anything goes to its owner's graveyard.
#[test]
fn req_sba_001_704_5m_unattached_aura_goes() {
    let mut s = Sim::new();
    // Owned by P2, controlled by P1.
    let aura = s.permanent(P2, P1, aura_chars("test.aura_a"));
    assert_eq!(s.check(), vec![finding("704.5m", obj(aura))]);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_eq!(r.outcome().findings, vec![finding("704.5m", obj(aura))]);
    assert_to_owners_graveyard(&r, aura, ZoneChangeCause::StateBasedAction);
    assert!(r.outcome().unattached.is_empty());
}

// REQ-SBA-001, CR 704.5m, 303.4c: an Aura whose creature left the battlefield (so it is no
// longer attached, CR 701.3d) goes to its owner's graveyard.
#[test]
fn req_sba_001_704_5m_aura_on_creature_that_left() {
    let mut s = Sim::new();
    let c = s.creature(P2, "test.vanilla_2_2", 2, 2);
    let aura = s.permanent(P1, P1, aura_chars("test.aura_a"));
    s.attach_to(aura, c);
    s.move_to(c, ZoneKind::Graveyard);
    assert_eq!(s.check(), vec![finding("704.5m", obj(aura))]);
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, aura, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, CR 704.5m, 303.4c: an Aura attached to a player who has lost the game (the
// `LOST_THE_GAME` designation) goes to its owner's graveyard.
#[test]
fn req_sba_001_704_5m_aura_on_lost_player() {
    let mut s = Sim::with_players(3);
    let aura = s.permanent(P1, P1, aura_chars("test.aura_a"));
    s.attach(aura, AttachTarget::Player(P3));
    s.mark_lost(P3);
    assert_eq!(s.check(), vec![finding("704.5m", obj(aura))]);
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, aura, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, CR 704.5m, 303.4c: an Aura whose enchant ability makes its object illegal
// (`aura_can_enchant` false) goes; one on a legal object in the same pass stays.
#[test]
fn req_sba_001_704_5m_illegal_target_per_environment() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let bad = s.permanent(P2, P2, aura_chars("test.aura_a"));
    let good = s.permanent(P2, P2, aura_chars("test.aura_b"));
    s.attach_to(bad, c);
    s.attach_to(good, c);
    s.env
        .illegal_aura_targets
        .insert((bad, AttachTarget::Permanent(c)));
    // An illegal pair for an Aura attached elsewhere has no effect.
    s.env
        .illegal_aura_targets
        .insert((good, AttachTarget::Player(P1)));
    assert_eq!(s.check(), vec![finding("704.5m", obj(bad))]);
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, bad, ZoneChangeCause::StateBasedAction);
    assert_untouched(&r, good);
    assert_still_on_battlefield(&r, c);
}

// REQ-SBA-001, CR 704.5m, 303.4c: an Aura attached to a player it cannot enchant goes.
#[test]
fn req_sba_001_704_5m_illegal_player_target() {
    let mut s = Sim::new();
    let aura = s.permanent(P1, P1, aura_chars("test.aura_a"));
    s.attach(aura, AttachTarget::Player(P2));
    s.env
        .illegal_aura_targets
        .insert((aura, AttachTarget::Player(P2)));
    assert_eq!(s.check(), vec![finding("704.5m", obj(aura))]);
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, aura, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, CR 704.5m, 303.4d: an Aura that is also a creature can't enchant anything; if
// attached, it goes to its owner's graveyard.
#[test]
fn req_sba_001_704_5m_aura_creature_attached_goes() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let aura = s.permanent(
        P2,
        P1,
        Characteristics {
            card_types: ["Enchantment", "Creature"]
                .iter()
                .map(|t| (*t).to_owned())
                .collect(),
            ..typed_creature(&["test.aura_creature"], &[], &["Aura"], 2, 2)
        },
    );
    s.attach_to(aura, c);
    let findings = s.check();
    assert!(
        findings.contains(&finding("704.5m", obj(aura))),
        "704.5m finding: {findings:?}"
    );
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, aura, ZoneChangeCause::StateBasedAction);
    assert_still_on_battlefield(&r, c);
}

// ---------------------------------------------------------------------------------------------
// 704.5n
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5n, 301.5: an Equipment attached to a player becomes unattached and stays.
#[test]
fn req_sba_001_704_5n_equipment_on_player_unattached() {
    let mut s = Sim::new();
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach(eq, AttachTarget::Player(P2));
    assert_eq!(s.check(), vec![finding("704.5n", obj(eq))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5n", obj(eq))]);
    assert_unattached(&r, eq);
    assert_eq!(r.outcome().unattached, vec![eq]);
    assert!(r.outcome().ceased.is_empty());
}

// REQ-SBA-001, CR 704.5n, 301.5: an Equipment attached to a noncreature permanent becomes
// unattached; one attached to a creature is legal and untouched; an unattached Equipment is
// untouched.
#[test]
fn req_sba_001_704_5n_equipment_noncreature_vs_legal() {
    let mut s = Sim::new();
    let land = s.permanent(P1, P1, land_chars("test.land_a"));
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let bad = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    let good = s.permanent(P1, P1, equipment_chars("test.equipment_b"));
    let loose = s.permanent(P1, P1, equipment_chars("test.equipment_c"));
    s.attach_to(bad, land);
    s.attach_to(good, c);
    assert_eq!(s.check(), vec![finding("704.5n", obj(bad))]);
    let r = run_sba(s);
    assert_unattached(&r, bad);
    assert_untouched(&r, good);
    assert_untouched(&r, loose);
    assert_eq!(r.after.permanents.attached_to(loose), None);
    assert_eq!(r.outcome().unattached, vec![bad]);
    assert!(r.moves().is_empty(), "nothing changes zones");
}

// REQ-SBA-001, CR 704.5n, 301.6: a Fortification attached to a land is legal; one attached to
// a nonland permanent or to a player becomes unattached and stays.
#[test]
fn req_sba_001_704_5n_fortification() {
    let mut s = Sim::new();
    let land = s.permanent(P1, P1, land_chars("test.land_a"));
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let good = s.permanent(P1, P1, fortification_chars("test.fortification_a"));
    let on_creature = s.permanent(P1, P1, fortification_chars("test.fortification_b"));
    let on_player = s.permanent(P2, P2, fortification_chars("test.fortification_c"));
    s.attach_to(good, land);
    s.attach_to(on_creature, c);
    s.attach(on_player, AttachTarget::Player(P1));
    assert_eq!(
        s.check(),
        vec![
            finding("704.5n", obj(on_creature)),
            finding("704.5n", obj(on_player))
        ]
    );
    let r = run_sba(s);
    assert_untouched(&r, good);
    assert_unattached(&r, on_creature);
    assert_unattached(&r, on_player);
    assert_eq!(r.outcome().unattached, vec![on_creature, on_player]);
    assert_unattached_ascending(&r);
}

// ---------------------------------------------------------------------------------------------
// 704.5p
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5p: an attached creature becomes unattached and stays.
#[test]
fn req_sba_001_704_5p_creature_attached() {
    let mut s = Sim::new();
    let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let attached = s.creature(P1, "test.vanilla_1_1", 1, 1);
    s.attach_to(attached, host);
    assert_eq!(s.check(), vec![finding("704.5p", obj(attached))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5p", obj(attached))]);
    assert_unattached(&r, attached);
    assert_still_on_battlefield(&r, host);
}

// REQ-SBA-001, CR 704.5p, 310.10: an attached battle becomes unattached and stays, whether
// attached to a permanent or a player.
#[test]
fn req_sba_001_704_5p_battle_attached() {
    let mut s = Sim::new();
    let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let battle = s.permanent(P2, P2, battle_chars("test.battle_a"));
    let battle_on_player = s.permanent(P2, P2, battle_chars("test.battle_b"));
    s.attach_to(battle, host);
    s.attach(battle_on_player, AttachTarget::Player(P1));
    assert_eq!(
        s.check(),
        vec![
            finding("704.5p", obj(battle)),
            finding("704.5p", obj(battle_on_player))
        ]
    );
    let r = run_sba(s);
    assert_unattached(&r, battle);
    assert_unattached(&r, battle_on_player);
    assert_unattached_ascending(&r);
}

// REQ-SBA-001, CR 704.5p: an attached noncreature, nonbattle permanent that is neither an Aura,
// an Equipment nor a Fortification (a plain artifact, a land) becomes unattached and stays.
#[test]
fn req_sba_001_704_5p_other_permanent_attached() {
    let mut s = Sim::new();
    let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let art = s.permanent(P1, P1, artifact_chars("test.artifact_a"));
    let land = s.permanent(P1, P1, land_chars("test.land_a"));
    s.attach_to(art, host);
    s.attach(land, AttachTarget::Player(P2));
    assert_eq!(
        s.check(),
        vec![finding("704.5p", obj(art)), finding("704.5p", obj(land))]
    );
    let r = run_sba(s);
    assert_unattached(&r, art);
    assert_unattached(&r, land);
    assert_eq!(r.outcome().unattached, vec![art, land]);
}

// ---------------------------------------------------------------------------------------------
// phasing (CR 702.26b)
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5m, 704.5n, 704.5p, 702.26b: a phased-out attachment is skipped by all
// three entries (an unattached phased-out Aura stays; a phased-out Equipment on a land and a
// phased-out creature attached to a creature stay attached).
#[test]
fn req_sba_001_attach_phased_out_attachment_skipped() {
    let mut s = Sim::new();
    let land = s.permanent(P1, P1, land_chars("test.land_a"));
    let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let aura = s.permanent(P1, P1, aura_chars("test.aura_a"));
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    let cr = s.creature(P1, "test.vanilla_1_1", 1, 1);
    s.attach_to(eq, land);
    s.attach_to(cr, host);
    for id in [aura, eq, cr] {
        s.phase_out(id);
    }
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    for id in [aura, eq, cr] {
        assert_untouched(&r, id);
    }
}

// REQ-SBA-001, CR 704.5m, 704.5n, 704.5p, 702.26b: an attachment whose target is phased out is
// skipped (not judged): an Equipment on a phased-out land and an artifact on a phased-out
// creature stay attached, as does an Aura on a phased-out creature.
#[test]
fn req_sba_001_attach_phased_out_target_skipped() {
    let mut s = Sim::new();
    let land = s.permanent(P1, P1, land_chars("test.land_a"));
    let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    let art = s.permanent(P1, P1, artifact_chars("test.artifact_a"));
    let aura = s.permanent(P1, P1, aura_chars("test.aura_a"));
    s.attach_to(eq, land);
    s.attach_to(art, host);
    s.attach_to(aura, host);
    s.phase_out(land);
    s.phase_out(host);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    for id in [eq, art, aura] {
        assert_untouched(&r, id);
    }
}

// REQ-SBA-001, CR 704.5m, 704.5n, 704.5p, 704.3: one pass handles an Aura, an Equipment and a
// creature together; findings are in registry order; the Aura moves in one batch.
#[test]
fn req_sba_001_attach_all_three_in_one_pass() {
    let mut s = Sim::new();
    let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let cr = s.creature(P1, "test.vanilla_1_1", 1, 1);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    let aura = s.permanent(P2, P2, aura_chars("test.aura_a"));
    s.attach_to(cr, host);
    s.attach(eq, AttachTarget::Player(P1));
    let expected = vec![
        finding("704.5m", obj(aura)),
        finding("704.5n", obj(eq)),
        finding("704.5p", obj(cr)),
    ];
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, expected);
    assert_to_owners_graveyard(&r, aura, ZoneChangeCause::StateBasedAction);
    assert_unattached(&r, eq);
    assert_unattached(&r, cr);
    assert_eq!(r.outcome().unattached, vec![cr, eq]);
    assert_eq!(r.nonempty_batches().len(), 1);
}

// =============================================================================================
// PR #214 round 1: F5 (Equipment/Fortification legality seam, CR 704.5n, 702.16d) and F2
// (every SBA unattach is an executed `Unattach` event, CR 701.3d, 603.10c)
// =============================================================================================

#[path = "spec_triggers_support.rs"]
mod tsupport;

use fineprint_core::events::GameEvent;
use fineprint_core::sba::{SbaEnvironment, StoredSbaEnvironment};
use fineprint_core::triggers::{LookBackKind, StateView, events_from_batch, look_back_kind};
use fineprint_dsl::ability::EventKind;

// REQ-SBA-001, CR 704.5n, 702.16d (F5): an Equipment attached to a creature listed with it in
// `illegal_equipment_targets` (the stand-in for protection) becomes unattached, stays on the
// battlefield and is listed in `unattached`; an Equipment on another creature not in the set
// stays attached.
#[test]
fn req_sba_001_704_5n_f5_illegal_equipment_target_unattached() {
    let mut s = Sim::new();
    let warded = s.creature(P2, "test.vanilla_2_2", 2, 2);
    let other = s.creature(P1, "test.vanilla_3_3", 3, 3);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    let control = s.permanent(P1, P1, equipment_chars("test.equipment_b"));
    s.attach_to(eq, warded);
    s.attach_to(control, other);
    s.env
        .illegal_equipment_targets
        .insert((eq, AttachTarget::Permanent(warded)));
    assert_eq!(s.check(), vec![finding("704.5n", obj(eq))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5n", obj(eq))]);
    assert_unattached(&r, eq);
    assert_eq!(r.outcome().unattached, vec![eq]);
    assert_untouched(&r, control);
    assert_eq!(
        r.after.permanents.attached_to(control),
        Some(AttachTarget::Permanent(other))
    );
    assert_still_on_battlefield(&r, warded);
    assert!(r.moves().is_empty(), "nothing changes zones");
}

// REQ-SBA-001, CR 704.5n, 702.16d (F5): a Fortification on a land listed with it in
// `illegal_equipment_targets` becomes unattached and stays; a Fortification on another land
// stays attached.
#[test]
fn req_sba_001_704_5n_f5_illegal_fortification_target_unattached() {
    let mut s = Sim::new();
    let land = s.permanent(P1, P1, land_chars("test.land_a"));
    let land2 = s.permanent(P1, P1, land_chars("test.land_b"));
    let fort = s.permanent(P2, P2, fortification_chars("test.fortification_a"));
    let fort2 = s.permanent(P2, P2, fortification_chars("test.fortification_b"));
    s.attach_to(fort, land);
    s.attach_to(fort2, land2);
    s.env
        .illegal_equipment_targets
        .insert((fort, AttachTarget::Permanent(land)));
    assert_eq!(s.check(), vec![finding("704.5n", obj(fort))]);
    let r = run_sba(s);
    assert_unattached(&r, fort);
    assert_untouched(&r, fort2);
    assert_eq!(r.outcome().unattached, vec![fort]);
}

// REQ-SBA-001, CR 704.5n, 702.16d (F5): a pair only matters for its own attachment and its
// current target: an Equipment on a legal creature stays when the set lists a different
// attachment id for that creature, or the same Equipment with a different target.
#[test]
fn req_sba_001_704_5n_f5_pair_is_per_attachment_and_target() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let c2 = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    let unrelated = s.permanent(P1, P1, equipment_chars("test.equipment_b"));
    s.attach_to(eq, c);
    s.env
        .illegal_equipment_targets
        .insert((unrelated, AttachTarget::Permanent(c)));
    s.env
        .illegal_equipment_targets
        .insert((eq, AttachTarget::Permanent(c2)));
    s.env
        .illegal_equipment_targets
        .insert((eq, AttachTarget::Player(P1)));
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert_untouched(&r, eq);
    assert_untouched(&r, unrelated);
    assert!(r.outcome().unattached.is_empty());
}

// REQ-SBA-001, CR 704.5n (F5): an unattached Equipment is untouched even with pairs listed for
// it.
#[test]
fn req_sba_001_704_5n_f5_unattached_equipment_untouched() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.env
        .illegal_equipment_targets
        .insert((eq, AttachTarget::Permanent(c)));
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert_untouched(&r, eq);
    assert_eq!(r.after.permanents.attached_to(eq), None);
}

// REQ-SBA-001, CR 704.5n, 702.26b (F5): an Equipment whose (illegal-pair) target is phased out
// is still skipped: it stays attached and nothing is found.
#[test]
fn req_sba_001_704_5n_f5_phased_out_target_still_skipped() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach_to(eq, c);
    s.env
        .illegal_equipment_targets
        .insert((eq, AttachTarget::Permanent(c)));
    s.phase_out(c);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert_untouched(&r, eq);
}

// REQ-SBA-001, CR 704.5n, 702.16d (F5): the seam itself: `SbaEnvironment::equipment_can_attach`
// on `StoredSbaEnvironment` is true by default and false exactly for the listed pairs.
#[test]
fn req_sba_001_704_5n_f5_stored_environment_seam() {
    let a = oid(10);
    let b = oid(11);
    let t = AttachTarget::Permanent(oid(20));
    let default = StoredSbaEnvironment::default();
    assert!(default.illegal_equipment_targets.is_empty());
    assert!(default.equipment_can_attach(a, t));
    assert!(default.equipment_can_attach(a, AttachTarget::Player(P1)));
    let env = StoredSbaEnvironment {
        illegal_equipment_targets: [(a, t), (b, AttachTarget::Player(P2))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    assert!(!env.equipment_can_attach(a, t));
    assert!(!env.equipment_can_attach(b, AttachTarget::Player(P2)));
    assert!(env.equipment_can_attach(b, t));
    assert!(env.equipment_can_attach(a, AttachTarget::Player(P2)));
    assert!(env.equipment_can_attach(a, AttachTarget::Permanent(oid(21))));
}

/// A board for F2: an Equipment attached to a player (704.5n), a creature attached to a host
/// (704.5p), a 0-toughness creature (704.5f, moved) and a creature with +1/+1 and -1/-1
/// counters (704.5q). Returns the board, the equipment, the attached creature, its host, the
/// 0-toughness creature and the counter creature. `eq_first` puts the Equipment's id first.
fn f2_board(eq_first: bool) -> (Sim, ObjectId, ObjectId, ObjectId, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let host = s.creature(P1, "test.vanilla_3_3", 3, 3);
    let (eq, cr) = if eq_first {
        let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
        let cr = s.creature(P2, "test.vanilla_1_1", 1, 1);
        (eq, cr)
    } else {
        let cr = s.creature(P2, "test.vanilla_1_1", 1, 1);
        let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
        (eq, cr)
    };
    s.attach(eq, AttachTarget::Player(P2));
    s.attach_to(cr, host);
    let zero = s.creature(P2, "test.vanilla_2_0", 2, 0);
    let pt = s.creature(P1, "test.vanilla_4_4", 4, 4);
    s.counters(pt, "+1/+1", 2);
    s.counters(pt, "-1/-1", 1);
    (s, eq, cr, host, zero, pt)
}

// REQ-SBA-001, CR 704.5n, 704.5p, 701.3d, 704.3 (F2): a pass that unattaches an Equipment and a
// creature and moves another object executes exactly one `GameEvent::Unattach` per unattached
// object, in ascending id order among the Unattach events (their absolute position relative to
// other events is set by the replacement bus, outside this unit); `unattached` stays ascending; the container shows both detached. Both id orders.
#[test]
fn req_sba_001_f2_unattach_events_executed() {
    for eq_first in [true, false] {
        let (s, eq, cr, host, zero, pt) = f2_board(eq_first);
        let r = run_sba(s);
        let o = r.outcome();
        assert!(o.performed);
        let mut both = vec![eq, cr];
        both.sort();
        assert_eq!(o.unattached, both, "ascending");
        assert_eq!(unattach_events(&r), both, "one Unattach each, ascending id");
        assert!(
            executed_events(&r).iter().any(|e| matches!(
                e,
                GameEvent::ZoneChange { object, .. } if *object == zero
            )),
            "the move is executed in the same pass"
        );
        assert_eq!(r.nonempty_batches().len(), 1, "one batch (CR 704.3)");
        assert_to_owners_graveyard(&r, zero, ZoneChangeCause::StateBasedAction);
        assert_eq!(r.after.counter(pt, "+1/+1"), 1);
        // The former targets (CR 603.10c looks back to them) and the detached container.
        assert_eq!(
            r.before.permanents.attached_to(eq),
            Some(AttachTarget::Player(P2))
        );
        assert_eq!(
            r.before.permanents.attached_to(cr),
            Some(AttachTarget::Permanent(host))
        );
        assert_unattached(&r, eq);
        assert_unattached(&r, cr);
        assert!(
            !r.after
                .permanents
                .attachments_of(AttachTarget::Player(P2))
                .contains(&eq),
            "the player no longer lists the Equipment"
        );
        assert!(
            !r.after
                .permanents
                .attachments_of(AttachTarget::Permanent(host))
                .contains(&cr),
            "the host no longer lists the creature"
        );
    }
}

// REQ-SBA-001, CR 704.5n (F2): a pass whose only result is one unattach executes exactly that
// one `Unattach` event, in one batch, and the event layer's log holds it.
#[test]
fn req_sba_001_f2_unattach_only_pass_is_one_event() {
    let mut s = Sim::new();
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach(eq, AttachTarget::Player(P2));
    let r = run_sba(s);
    assert_eq!(
        executed_events(&r),
        vec![GameEvent::Unattach { attachment: eq }]
    );
    let batches = r.nonempty_batches();
    assert_eq!(batches.len(), 1);
    let logged = r
        .after
        .events
        .log()
        .batches()
        .last()
        .expect("logged")
        .clone();
    assert_eq!(&logged, &batches[0]);
    assert_unattached(&r, eq);
}

// REQ-SBA-001, CR 704.5n, 704.5p, 603.10c (F2): a trigger of kind `Unattached` matches the
// SBA's Unattach events: the normalised batch has one `Unattached` occurrence per unattached
// object (subject = that object), the matcher kind is a look-back kind (CR 603.10c), and
// `detect` with a registration listed in the pre-event view fires once per unattached object.
#[test]
fn req_sba_001_f2_unattached_trigger_matches() {
    let (s, eq, cr, ..) = f2_board(true);
    let r = run_sba(s);
    let batch = r.nonempty_batches()[0].clone();
    let normalised = events_from_batch(&batch);
    let unattached: Vec<_> = normalised
        .iter()
        .filter(|t| t.kinds.contains(&EventKind::Unattached))
        .collect();
    let mut both = vec![eq, cr];
    both.sort();
    assert_eq!(
        unattached
            .iter()
            .map(|t| t.subject_pre.expect("subject"))
            .collect::<Vec<_>>(),
        both
    );
    assert_eq!(
        look_back_kind(&tsupport::m(EventKind::Unattached)),
        Some(LookBackKind::Unattached)
    );
    let env = tsupport::FakeEnv::new()
        .with_pre(tsupport::reg(
            900,
            P1,
            tsupport::ability(
                "test.on_unattached",
                tsupport::subj_obj(tsupport::m(EventKind::Unattached)),
            ),
        ))
        .accept(StateView::PreEvent, &[eq.get(), cr.get()]);
    let fired = tsupport::run(&env, &normalised);
    assert_eq!(
        fired.len(),
        2,
        "one trigger per unattached object: {fired:?}"
    );
}

// =============================================================================================
// PR #214 round 2: G3 (Equipment that is also a Fortification, D5) and G4 (an illegal pair
// overrides an unevaluable target, D5)
// =============================================================================================

/// An artifact with both the Equipment and the Fortification subtypes (CR 301.5, 301.6).
fn equipment_fortification_chars(name: &str) -> Characteristics {
    typed(&[name], &[], &["Artifact"], &["Equipment", "Fortification"])
}

/// A 2/2 permanent that is both a creature and a land.
fn creature_land_chars(name: &str) -> Characteristics {
    Characteristics {
        power: Some(lit(2)),
        toughness: Some(lit(2)),
        ..typed(&[name], &[], &["Creature", "Land"], &[])
    }
}

// REQ-SBA-001, CR 704.5n, 301.5, 301.6 (G3, D5): an artifact that is both an Equipment and a
// Fortification must satisfy both restrictions: attached to a nonland creature it becomes
// unattached (one Unattach event, listed); attached to a noncreature land it becomes unattached;
// attached to a permanent that is both a creature and a land it stays attached with no event.
#[test]
fn req_sba_001_704_5n_g3_equipment_fortification_needs_creature_land() {
    let mut s = Sim::new();
    let creature = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let land = s.permanent(P1, P1, land_chars("test.land_a"));
    let both = s.permanent(P1, P1, creature_land_chars("test.creature_land_a"));
    let on_creature = s.permanent(P1, P1, equipment_fortification_chars("test.equip_fort_a"));
    let on_land = s.permanent(P1, P1, equipment_fortification_chars("test.equip_fort_b"));
    let on_both = s.permanent(P1, P1, equipment_fortification_chars("test.equip_fort_c"));
    s.attach_to(on_creature, creature);
    s.attach_to(on_land, land);
    s.attach_to(on_both, both);
    assert_eq!(
        s.check(),
        vec![
            finding("704.5n", obj(on_creature)),
            finding("704.5n", obj(on_land))
        ]
    );
    let r = run_sba(s);
    assert_unattached(&r, on_creature);
    assert_unattached(&r, on_land);
    assert_untouched(&r, on_both);
    assert_eq!(
        r.after.permanents.attached_to(on_both),
        Some(AttachTarget::Permanent(both))
    );
    assert_eq!(r.outcome().unattached, vec![on_creature, on_land]);
    assert_eq!(unattach_events(&r), vec![on_creature, on_land]);
    assert_eq!(unattach_count(&r, on_both), 0, "no event for the legal one");
}

// REQ-SBA-001, CR 704.5n, 301.5, 301.6 (G3 control): a plain Equipment on a creature, a plain
// Fortification on a land and a plain Equipment on a creature land stay attached; nothing is
// found or performed.
#[test]
fn req_sba_001_704_5n_g3_plain_subtypes_stay() {
    let mut s = Sim::new();
    let creature = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let land = s.permanent(P1, P1, land_chars("test.land_a"));
    let both = s.permanent(P1, P1, creature_land_chars("test.creature_land_a"));
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    let fort = s.permanent(P1, P1, fortification_chars("test.fortification_a"));
    let fort_both = s.permanent(P1, P1, fortification_chars("test.fortification_b"));
    s.attach_to(eq, creature);
    s.attach_to(fort, land);
    s.attach_to(fort_both, both);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    for id in [eq, fort, fort_both] {
        assert_untouched(&r, id);
    }
    assert!(unattach_events(&r).is_empty());
}

// REQ-SBA-001, CR 704.5n, 702.16d (G4, D5): an Equipment attached to a creature whose facts the
// layer environment cannot evaluate (no base description; the creature is on the battlefield and
// NOT phased out), with the exact pair in `illegal_equipment_targets`: the pair alone makes it
// illegal: unattached, stays on the battlefield, one Unattach event, listed in `unattached`.
#[test]
fn req_sba_001_704_5n_g4_illegal_pair_with_unevaluable_target() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach_to(eq, c);
    s.env
        .illegal_equipment_targets
        .insert((eq, AttachTarget::Permanent(c)));
    s.layer.bases.remove(&c);
    assert!(s.on_battlefield(c));
    assert!(
        !s.permanents
            .get(c)
            .expect("a permanent")
            .status()
            .phased_out
    );
    assert!(s.check().contains(&finding("704.5n", obj(eq))));
    let r = run_sba(s);
    assert_unattached(&r, eq);
    assert_eq!(r.outcome().unattached, vec![eq]);
    assert_eq!(unattach_count(&r, eq), 1);
}

// REQ-SBA-001, CR 704.5n, 702.26b (G4, D5): the same unevaluable target, pair marked, but the
// target is phased out: still skipped: the Equipment stays attached and nothing is found.
#[test]
fn req_sba_001_704_5n_g4_phased_out_unevaluable_target_skipped() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach_to(eq, c);
    s.env
        .illegal_equipment_targets
        .insert((eq, AttachTarget::Permanent(c)));
    s.layer.bases.remove(&c);
    s.phase_out(c);
    assert!(!s.check().contains(&finding("704.5n", obj(eq))));
    let r = run_sba(s);
    assert_untouched(&r, eq);
    assert_eq!(unattach_count(&r, eq), 0);
}
