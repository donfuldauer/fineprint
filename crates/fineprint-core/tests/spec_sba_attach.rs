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
