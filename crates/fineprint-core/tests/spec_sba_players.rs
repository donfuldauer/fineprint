// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the player SBAs 704.5a, 704.5b and 704.5c, the Two-Headed Giant gate of
//! 704.5c and the same-result merge of game losses (REQ-SBA-001, REQ-TEST-001, REQ-TURN-001;
//! CR 704.3, 704.5a, 704.5b, 704.5c, 704.6b, 704.7, 121.4). Black-box: written from the
//! work-item brief only.

#[path = "spec_sba_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::sba::GameVariant;
use fineprint_core::zones::ZoneKind;
use support::*;

/// The loss reasons `[(player, ids)]` as a map.
fn reasons(
    pairs: &[(fineprint_base::ids::PlayerId, &[&'static str])],
) -> BTreeMap<fineprint_base::ids::PlayerId, Vec<&'static str>> {
    pairs.iter().map(|(p, ids)| (*p, ids.to_vec())).collect()
}

// ---------------------------------------------------------------------------------------------
// 704.5a
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 704.5a: a player with 0 or less life loses the game (life 0 and
// -3); a player at 1 life does not (boundary on both sides).
#[test]
fn req_sba_001_704_5a_zero_or_less_life_loses() {
    for (life, loses) in [(0, true), (-3, true), (1, false), (STARTING_LIFE, false)] {
        let mut s = Sim::new();
        s.set_life(P2, life);
        let expected = if loses {
            vec![finding("704.5a", player(P2))]
        } else {
            vec![]
        };
        assert_eq!(s.check(), expected, "life {life}");
        let r = run_sba(s);
        let o = r.outcome();
        assert_eq!(o.findings, expected, "life {life}");
        assert_eq!(o.performed, loses, "life {life}");
        assert_eq!(r.after.lost(P2), loses, "life {life}");
        assert!(!r.after.lost(P1));
        if loses {
            assert_eq!(o.lost_players, vec![P2]);
            assert_eq!(o.loss_reasons, reasons(&[(P2, &["704.5a"])]));
        } else {
            assert!(o.lost_players.is_empty());
            assert!(o.loss_reasons.is_empty());
        }
        assert!(o.ceased.is_empty());
        // Losing does not change life.
        assert_eq!(r.after.life(P2), life);
    }
}

// REQ-SBA-001, CR 704.5a: a player already carrying the lost-the-game designation is not a
// finding again.
#[test]
fn req_sba_001_704_5a_already_lost_is_not_a_finding() {
    let mut s = Sim::new();
    s.set_life(P1, 0);
    s.mark_lost(P1);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert!(r.outcome().findings.is_empty());
    assert!(r.outcome().lost_players.is_empty());
    assert!(r.after.lost(P1));
}

// ---------------------------------------------------------------------------------------------
// 704.5b
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 704.5b, 121.4: a player who attempted to draw from an empty
// library since the last check loses; the empty-draw flag is consumed.
#[test]
fn req_sba_001_704_5b_empty_library_draw_loses() {
    let mut s = Sim::new();
    s.empty_draw(P1);
    assert_eq!(s.check(), vec![finding("704.5b", player(P1))]);
    let r = run_sba(s);
    let o = r.outcome();
    assert!(o.performed);
    assert_eq!(o.findings, vec![finding("704.5b", player(P1))]);
    assert_eq!(o.lost_players, vec![P1]);
    assert_eq!(o.loss_reasons, reasons(&[(P1, &["704.5b"])]));
    assert!(r.after.lost(P1));
    assert!(!r.after.lost(P2));
    assert!(r.after.events.empty_library_draws().is_empty());
}

// REQ-SBA-001, CR 704.5b: the attempt counts even if the library was refilled since (the flag,
// not the current library, decides).
#[test]
fn req_sba_001_704_5b_flag_counts_even_after_library_refilled() {
    let mut s = Sim::new();
    s.empty_draw(P2);
    s.create(
        ZoneKind::Library,
        P2,
        creature_chars("test.vanilla_2_2", 2, 2),
    );
    s.create(
        ZoneKind::Library,
        P2,
        creature_chars("test.vanilla_2_2", 2, 2),
    );
    assert_eq!(s.check(), vec![finding("704.5b", player(P2))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().lost_players, vec![P2]);
    assert!(r.after.lost(P2));
    assert!(r.after.events.empty_library_draws().is_empty());
}

// REQ-SBA-001, CR 704.5b: an empty library alone (no draw attempted) is no loss (the other side
// of the boundary).
#[test]
fn req_sba_001_704_5b_empty_library_without_draw_is_no_loss() {
    let s = Sim::new();
    assert!(s.zones.is_empty(fineprint_core::zones::ZoneId::Library(P1)));
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert!(!r.after.lost(P1));
}

// REQ-SBA-001, CR 704.5b: the empty-draw flag is consumed even when no loss results (the player
// already carries LOST_THE_GAME): no finding, nothing performed, flag cleared.
#[test]
fn req_sba_001_704_5b_flag_consumed_without_loss() {
    let mut s = Sim::new();
    s.mark_lost(P1);
    s.empty_draw(P1);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    let o = r.outcome();
    assert!(!o.performed);
    assert!(o.findings.is_empty());
    assert!(o.lost_players.is_empty());
    assert!(r.after.events.empty_library_draws().is_empty());
    // A second pass has nothing left to consume.
    let r2 = run_sba(r.after.clone());
    assert!(!r2.outcome().performed);
}

// ---------------------------------------------------------------------------------------------
// 704.5c
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, REQ-TEST-001, CR 704.5c: ten or more poison counters lose the game (10 and 11);
// nine do not.
#[test]
fn req_sba_001_704_5c_ten_or_more_poison_loses() {
    for (poison, loses) in [(10, true), (11, true), (9, false), (0, false)] {
        let mut s = Sim::new();
        s.set_poison(P1, poison);
        let expected = if loses {
            vec![finding("704.5c", player(P1))]
        } else {
            vec![]
        };
        assert_eq!(s.check(), expected, "poison {poison}");
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, expected, "poison {poison}");
        assert_eq!(r.outcome().performed, loses, "poison {poison}");
        assert_eq!(r.after.lost(P1), loses, "poison {poison}");
        if loses {
            assert_eq!(r.outcome().loss_reasons, reasons(&[(P1, &["704.5c"])]));
        }
    }
}

// REQ-SBA-001, CR 704.5c, 704.6b, 704.5a: in Two-Headed Giant ten poison counters produce no
// 704.5c finding while 0 life still produces 704.5a; in a standard game both apply.
#[test]
fn req_sba_001_704_5c_variant_gate() {
    for variant in [GameVariant::Standard, GameVariant::TwoHeadedGiant] {
        let mut s = Sim::new();
        s.env.variant = variant;
        s.set_poison(P1, 10);
        s.set_life(P2, 0);
        let expected = if variant == GameVariant::Standard {
            vec![finding("704.5a", player(P2)), finding("704.5c", player(P1))]
        } else {
            vec![finding("704.5a", player(P2))]
        };
        assert_eq!(s.check(), expected, "{variant:?}");
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, expected, "{variant:?}");
        assert!(r.after.lost(P2), "{variant:?}");
        assert_eq!(
            r.after.lost(P1),
            variant == GameVariant::Standard,
            "{variant:?}"
        );
        let losers = if variant == GameVariant::Standard {
            vec![P1, P2]
        } else {
            vec![P2]
        };
        assert_eq!(r.outcome().lost_players, losers, "{variant:?}");
    }
}

// REQ-SBA-001, CR 704.5c, 704.6b: Two-Headed Giant with poison alone performs nothing.
#[test]
fn req_sba_001_704_5c_two_headed_giant_poison_alone_is_nothing() {
    let mut s = Sim::new();
    s.env.variant = GameVariant::TwoHeadedGiant;
    s.set_poison(P1, 15);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert!(!r.after.lost(P1));
}

// ---------------------------------------------------------------------------------------------
// same result (CR 704.7) and ordering
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.7, 704.5a, 704.5b: a player losing by 704.5a and 704.5b is one loser with
// both reasons (both findings are reported).
#[test]
fn req_sba_001_704_7_same_player_losses_are_one_result() {
    let mut s = Sim::new();
    s.set_life(P1, 0);
    s.empty_draw(P1);
    let r = run_sba(s);
    let o = r.outcome();
    assert_eq!(
        o.findings,
        vec![finding("704.5a", player(P1)), finding("704.5b", player(P1))]
    );
    assert_eq!(o.lost_players, vec![P1]);
    assert_eq!(o.loss_reasons, reasons(&[(P1, &["704.5a", "704.5b"])]));
    assert!(r.after.lost(P1));
}

// REQ-SBA-001, CR 704.7, 704.5a, 704.5b, 704.5c: all three player SBAs on one player: one
// loser, three reasons in registry order.
#[test]
fn req_sba_001_704_7_all_three_reasons_one_loser() {
    let mut s = Sim::new();
    s.set_life(P2, -1);
    s.empty_draw(P2);
    s.set_poison(P2, 12);
    let r = run_sba(s);
    let o = r.outcome();
    assert_eq!(o.lost_players, vec![P2]);
    assert_eq!(
        o.loss_reasons,
        reasons(&[(P2, &["704.5a", "704.5b", "704.5c"])])
    );
    assert_eq!(o.findings.len(), 3);
}

// REQ-SBA-001, CR 704.3, 704.7: two different players losing are two losers (never merged),
// ascending, each with its own reasons.
#[test]
fn req_sba_001_704_7_different_players_are_two_losers() {
    let mut s = Sim::new();
    s.empty_draw(P2);
    s.set_life(P1, 0);
    let r = run_sba(s);
    let o = r.outcome();
    assert_eq!(o.lost_players, vec![P1, P2]);
    assert_eq!(
        o.loss_reasons,
        reasons(&[(P1, &["704.5a"]), (P2, &["704.5b"])])
    );
    assert!(r.after.lost(P1));
    assert!(r.after.lost(P2));
}

// REQ-SBA-001, CR 704.3: findings are sorted by registry order, then by subject.
#[test]
fn req_sba_001_findings_sorted_by_entry_then_subject() {
    let mut s = Sim::with_players(3);
    s.set_poison(P1, 10);
    s.set_life(P3, 0);
    s.set_life(P2, 0);
    s.empty_draw(P3);
    s.empty_draw(P1);
    let expected = vec![
        finding("704.5a", player(P2)),
        finding("704.5a", player(P3)),
        finding("704.5b", player(P1)),
        finding("704.5b", player(P3)),
        finding("704.5c", player(P1)),
    ];
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, expected);
    assert_eq!(r.outcome().lost_players, vec![P1, P2, P3]);
}
