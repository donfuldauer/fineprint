// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the SBA pass over seeded random boards (REQ-SBA-001, REQ-TEST-001,
//! REQ-TURN-001; CR 704.3, 704.5a-704.5i, 704.7). The generator is a fixed-seed LCG (R3: no
//! system randomness). Black-box: written from the work-item brief only.

#[path = "spec_sba_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::sba::SbaSubject;
use fineprint_core::txn::Transactional;
use fineprint_core::zones::ZoneKind;
use support::*;

const SEEDS: u64 = 96;

const OFF_BATTLEFIELD: [ZoneKind; 5] = [
    ZoneKind::Hand,
    ZoneKind::Graveyard,
    ZoneKind::Exile,
    ZoneKind::Library,
    ZoneKind::Stack,
];

const ANY_ZONE: [ZoneKind; 6] = [
    ZoneKind::Hand,
    ZoneKind::Graveyard,
    ZoneKind::Exile,
    ZoneKind::Library,
    ZoneKind::Stack,
    ZoneKind::Battlefield,
];

/// A random board for `seed`: 2 or 3 players with random life, poison, empty-draw and lost
/// flags; up to 12 objects among creatures (random toughness, damage, deathtouch,
/// indestructible, layer effects, P/T counters), tokens, planeswalkers and copies.
fn random_board(seed: u64) -> Sim {
    let mut g = Lcg::new(seed);
    let n_players = 2 + usize::try_from(g.below(2)).expect("fits");
    let mut s = Sim::with_players(n_players);
    let players: Vec<PlayerId> = s.game.turn_order().to_vec();
    // Empty-draw attempts first, while every library is still empty (CR 121.4).
    for &p in &players {
        if g.chance(1, 4) {
            s.empty_draw(p);
        }
    }
    let mut effect = 1;
    for _ in 0..g.below(13) {
        let owner = g.pick(&players);
        match g.below(6) {
            0 | 1 => {
                let t = g.range(-1, 3);
                let p = g.range(0, 3);
                let mut c = creature_chars(&format!("test.vanilla_{p}_{t}"), p, t);
                if g.chance(1, 5) {
                    c = with_keyword(c, "indestructible");
                }
                let id = s.creature(owner, "test.placeholder", p, t);
                s.describe(id, c);
                if g.chance(1, 4) {
                    let d = g.range(-2, 2);
                    s.add_layer_effect(effect, modify_pt_effect(&[id], d, d, effect));
                    effect += 1;
                }
                if g.chance(1, 5) {
                    s.counters(id, "-1/-1", g.range(1, 2));
                }
                if g.chance(1, 6) {
                    s.counters(id, "+1/+1", 1);
                }
                if g.chance(1, 2) {
                    s.damage(id, g.range(1, 4));
                }
                if g.chance(1, 4) {
                    s.env.deathtouch.insert(id);
                }
            }
            2 => {
                let kind = g.pick(&[
                    ZoneKind::Battlefield,
                    ZoneKind::Graveyard,
                    ZoneKind::Hand,
                    ZoneKind::Exile,
                ]);
                s.token(owner, kind);
            }
            3 => {
                let loyalty = g.range(0, 2);
                s.walker(owner, loyalty);
            }
            4 => {
                let kind = if g.chance(1, 2) {
                    ZoneKind::Stack
                } else {
                    g.pick(&OFF_BATTLEFIELD)
                };
                let id = s.spell_copy(owner, kind);
                if g.chance(1, 6) {
                    s.env.exempt.insert(id);
                }
            }
            _ => {
                let id = s.card_copy(owner, g.pick(&ANY_ZONE));
                if g.chance(1, 6) {
                    s.env.exempt.insert(id);
                }
            }
        }
    }
    for &p in &players {
        if g.chance(1, 3) {
            s.set_life(p, g.range(-2, 2));
        }
        if g.chance(1, 3) {
            s.set_poison(p, g.range(8, 11));
        }
        if g.chance(1, 6) {
            s.mark_lost(p);
        }
    }
    s
}

// REQ-SBA-001, CR 704.3: the same seed gives an identical outcome and state hash.
#[test]
fn req_sba_001_property_deterministic() {
    for seed in 0..SEEDS {
        let a = run_sba(random_board(seed));
        let b = run_sba(random_board(seed));
        assert_eq!(a.result, b.result, "seed {seed}");
        assert_eq!(a.hash, b.hash, "seed {seed}");
        assert_eq!(a.after.state_hash(), b.after.state_hash(), "seed {seed}");
        assert_eq!(
            a.after.game.state_hash(),
            b.after.game.state_hash(),
            "seed {seed}"
        );
    }
}

// REQ-SBA-001, REQ-TEST-001, CR 704.3, 704.5a-704.5i, 704.7: after one pass every object with a
// finding is gone (moved once to its owner's graveyard or ceased), at most one executed batch
// holds the moves, the findings equal `check` before the pass, `check` after the pass reports
// no 704.5d/e/f/i finding, and the losers are exactly the players with a 704.5a/b/c finding.
#[test]
fn req_sba_001_property_pass_invariants() {
    let mut performed = 0;
    for seed in 0..SEEDS {
        let mut base = random_board(seed);
        let checked = base.check();
        let r = run_sba(base);
        let o = r.outcome();
        assert_eq!(o.findings, checked, "seed {seed}");
        assert_eq!(o.performed, !o.findings.is_empty(), "seed {seed}");
        if o.performed {
            performed += 1;
        }

        let mut objects: BTreeSet<ObjectId> = BTreeSet::new();
        let mut losers: BTreeSet<PlayerId> = BTreeSet::new();
        for f in &o.findings {
            match f.subject {
                SbaSubject::Object(id) => {
                    objects.insert(id);
                    assert!(r.after.zones.object(id).is_none(), "seed {seed}: {f:?}");
                    match f.entry_id {
                        "704.5d" | "704.5e" => {
                            assert!(o.ceased.contains(&id), "seed {seed}: {f:?}");
                        }
                        "704.5f" | "704.5g" | "704.5h" | "704.5i" => {
                            let new = r
                                .moved_to(id)
                                .unwrap_or_else(|| panic!("seed {seed}: {f:?} not moved"));
                            let owner = r.before.zones.object(id).unwrap().owner;
                            assert_eq!(
                                r.after.zones.object(new).unwrap().zone,
                                fineprint_core::zones::ZoneId::Graveyard(owner),
                                "seed {seed}: {f:?}"
                            );
                        }
                        other => panic!("seed {seed}: object finding {other}"),
                    }
                }
                SbaSubject::Player(p) => {
                    assert!(
                        ["704.5a", "704.5b", "704.5c"].contains(&f.entry_id),
                        "seed {seed}: {f:?}"
                    );
                    losers.insert(p);
                }
            }
        }
        // One move per object with a destroy/put finding, no other moves.
        let moved: Vec<ObjectId> = r.moves().iter().map(|m| m.0).collect();
        let moved_set: BTreeSet<ObjectId> = moved.iter().copied().collect();
        assert_eq!(moved.len(), moved_set.len(), "seed {seed}: moved once each");
        let ceased: BTreeSet<ObjectId> = o.ceased.iter().copied().collect();
        assert!(moved_set.is_disjoint(&ceased), "seed {seed}");
        let gone: BTreeSet<ObjectId> = moved_set.union(&ceased).copied().collect();
        assert_eq!(gone, objects, "seed {seed}");
        let mut sorted = o.ceased.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, o.ceased, "seed {seed}: ceased ascending");
        assert!(r.nonempty_batches().len() <= 1, "seed {seed}");
        assert_eq!(
            r.nonempty_batches().len(),
            usize::from(!moved.is_empty()),
            "seed {seed}"
        );

        let lost: BTreeSet<PlayerId> = o.lost_players.iter().copied().collect();
        assert_eq!(lost, losers, "seed {seed}");
        assert_eq!(
            o.loss_reasons.keys().copied().collect::<BTreeSet<_>>(),
            losers,
            "seed {seed}"
        );
        for p in &losers {
            assert!(r.after.lost(*p), "seed {seed}");
            assert!(
                !r.before.lost(*p),
                "seed {seed}: already lost is no finding"
            );
        }
        let mut ascending = o.lost_players.clone();
        ascending.sort();
        ascending.dedup();
        assert_eq!(ascending, o.lost_players, "seed {seed}");
        assert!(
            r.after.events.empty_library_draws().is_empty(),
            "seed {seed}"
        );
        assert!(r.after.env.deathtouch.is_empty(), "seed {seed}");

        let mut after = r.after.clone();
        after.sync_after(o);
        let again = after.check();
        for f in &again {
            assert!(
                !["704.5d", "704.5e", "704.5f", "704.5i"].contains(&f.entry_id),
                "seed {seed}: {f:?} after the pass"
            );
        }
    }
    assert!(
        performed > SEEDS / 2,
        "the generator exercises the pass ({performed})"
    );
}

// REQ-SBA-001, CR 704.3: `check` on a random board is pure: the state hash and the empty-draw
// flags are unchanged.
#[test]
fn req_sba_001_property_check_pure() {
    for seed in 0..SEEDS {
        let mut s = random_board(seed);
        let before = s.snap();
        let hash = s.state_hash();
        let a = s.check();
        let b = s.check();
        assert_eq!(a, b, "seed {seed}");
        assert_eq!(s.snap(), before, "seed {seed}");
        assert_eq!(s.state_hash(), hash, "seed {seed}");
    }
}
