// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the SBA pass over seeded random boards (REQ-SBA-001, REQ-TEST-001,
//! REQ-TURN-001, REQ-CORE-001; CR 704.3, 704.5a-704.5i, 704.5j, 704.5k, 704.5m, 704.5n,
//! 704.5p, 704.5q, 704.5r, 704.5s, 704.5t, 704.7). The generator is a fixed-seed LCG (R3: no
//! system randomness). Black-box: written from the work-item brief only.

#[path = "spec_sba_support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

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
                // CR 704.5q (added by FP-041): a creature with both +1/+1 and -1/-1 counters
                // loses N of each; it leaves only if a 704.5d-i finding also names it.
                SbaSubject::Object(id) if f.entry_id == "704.5q" => {
                    assert!(r.before.counter(id, "+1/+1") > 0, "seed {seed}: {f:?}");
                    assert!(r.before.counter(id, "-1/-1") > 0, "seed {seed}: {f:?}");
                    if r.after.zones.object(id).is_some() {
                        assert!(
                            r.after.counter(id, "+1/+1") == 0 || r.after.counter(id, "-1/-1") == 0,
                            "seed {seed}: {f:?} left both kinds"
                        );
                    }
                }
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
        // A batch holds the moves and (CR 704.5q) the counter removals of the pass.
        assert_eq!(
            r.nonempty_batches().len(),
            usize::from(!moved.is_empty() || !counter_removals(&r).is_empty()),
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

// ---------------------------------------------------------------------------------------------
// CR 704.5j-704.5t (FP-041)
// ---------------------------------------------------------------------------------------------

use fineprint_core::choice::{ChoiceResponse, PendingChoiceRequest};
use fineprint_core::events::ZoneChangeCause;
use fineprint_core::permanent::AttachTarget;

const LEGEND_NAMES: [&str; 3] = ["test.legend_a", "test.legend_b", "test.legend_c"];

/// A random board for `seed` exercising 704.5a-704.5t together: creatures (some with toughness
/// 0 or below), legends, world permanents, Auras, Equipment, Fortifications, other attached
/// permanents, +1/+1 and -1/-1 counters, counter limits, Sagas and dungeons, some phased out.
fn random_new_board(seed: u64) -> Sim {
    let mut g = Lcg::new(seed.wrapping_mul(31).wrapping_add(7));
    let mut s = Sim::with_players(2 + usize::try_from(g.below(2)).expect("fits"));
    let players: Vec<PlayerId> = s.game.turn_order().to_vec();
    let mut hosts: Vec<ObjectId> = Vec::new();
    let mut lands: Vec<ObjectId> = Vec::new();
    let mut world_stamp = 0;
    for _ in 0..(3 + g.below(14)) {
        let owner = g.pick(&players);
        let controller = if g.chance(1, 4) {
            g.pick(&players)
        } else {
            owner
        };
        match g.below(11) {
            0 | 1 => {
                let t = g.range(-1, 3);
                let id = s.permanent(owner, controller, creature_chars("test.vanilla_1_x", 1, t));
                hosts.push(id);
            }
            2 => {
                let id = s.permanent(owner, controller, land_chars("test.land_a"));
                lands.push(id);
            }
            3 => {
                let name = g.pick(&LEGEND_NAMES);
                s.legend(owner, controller, name);
            }
            4 => {
                world_stamp += g.below(2);
                s.world(owner, controller, "test.world_a", Some(world_stamp));
            }
            5 => {
                let id = s.permanent(owner, controller, aura_chars("test.aura_a"));
                match g.below(3) {
                    0 if !hosts.is_empty() => s.attach_to(id, g.pick(&hosts)),
                    1 => s.attach(id, AttachTarget::Player(g.pick(&players))),
                    _ => {}
                }
            }
            6 => {
                let c = if g.chance(1, 2) {
                    equipment_chars("test.equipment_a")
                } else {
                    fortification_chars("test.fortification_a")
                };
                let id = s.permanent(owner, controller, c);
                match g.below(4) {
                    0 if !hosts.is_empty() => s.attach_to(id, g.pick(&hosts)),
                    1 if !lands.is_empty() => s.attach_to(id, g.pick(&lands)),
                    2 => s.attach(id, AttachTarget::Player(g.pick(&players))),
                    _ => {}
                }
            }
            7 => {
                let id = s.permanent(owner, controller, artifact_chars("test.artifact_a"));
                if g.chance(1, 2) && !hosts.is_empty() {
                    s.attach_to(id, g.pick(&hosts));
                }
                if g.chance(1, 2) {
                    s.counters(id, "charge", g.range(1, 4));
                    s.limit(id, "charge", g.range(0, 3));
                }
            }
            8 => {
                let id = s.permanent(owner, controller, creature_chars("test.vanilla_4_4", 4, 4));
                hosts.push(id);
                let plus = g.range(0, 3);
                let minus = g.range(0, 3);
                if plus > 0 {
                    s.counters(id, "+1/+1", plus);
                }
                if minus > 0 {
                    s.counters(id, "-1/-1", minus);
                }
            }
            9 => {
                let last = if g.chance(4, 5) {
                    Some(g.range(1, 3))
                } else {
                    None
                };
                let id = s.saga(owner, controller, last, g.range(0, 4));
                if g.chance(1, 5) {
                    s.env.chapter_pending.insert(id);
                }
            }
            _ => {
                let id = s.dungeon(owner, g.chance(2, 3));
                if g.chance(1, 5) {
                    s.env.room_pending.insert(id);
                }
            }
        }
    }
    // Phase out a few battlefield permanents.
    for id in s.permanents.ids() {
        if g.chance(1, 10) {
            s.phase_out(id);
        }
    }
    if g.chance(1, 4) {
        let p = g.pick(&players);
        s.set_life(p, 0);
    }
    s
}

/// Answers a legend request with the candidate picked by `g`.
fn random_keep(g: &mut Lcg, req: &PendingChoiceRequest) -> ChoiceResponse {
    let c = candidates(req);
    let i = usize::try_from(g.below(c.len() as u64)).expect("fits");
    keep(c[i])
}

// REQ-SBA-001, REQ-CORE-001, CR 704.3, 704.5j-704.5t: (a) the same inputs and answers give an
// identical outcome, state hash and final state twice.
#[test]
fn req_sba_001_property_new_entries_deterministic() {
    for seed in 0..SEEDS {
        let run = |seed| {
            let mut g = Lcg::new(seed + 1000);
            drive_sba(random_new_board(seed), move |_, req| {
                random_keep(&mut g, req)
            })
        };
        let a = run(seed);
        let b = run(seed);
        assert_eq!(a.result, b.result, "seed {seed}");
        assert_eq!(a.hash, b.hash, "seed {seed}");
        assert_eq!(a.answers, b.answers, "seed {seed}");
        assert_eq!(a.after.snap(), b.after.snap(), "seed {seed}");
        assert_eq!(a.after.state_hash(), b.after.state_hash(), "seed {seed}");
    }
}

// REQ-SBA-001, CR 704.5k: (b) the world rule keeps 0 or 1 world permanents: the one with the
// unique largest stamp, or none on a tie; the result never depends on which id holds which
// stamp (the stamps reversed over the ids give the same count, and the survivor is again the
// holder of the largest stamp).
#[test]
fn req_sba_001_property_world_rule() {
    for seed in 0..SEEDS {
        let mut g = Lcg::new(seed ^ 0x77);
        let n = usize::try_from(2 + g.below(4)).expect("fits");
        let stamps: Vec<u64> = (0..n).map(|_| g.below(4)).collect();
        let owners: Vec<PlayerId> = (0..n).map(|_| g.pick(&[P1, P2])).collect();
        let max = *stamps.iter().max().expect("nonempty");
        let unique = stamps.iter().filter(|x| **x == max).count() == 1;
        let mut kept_counts = Vec::new();
        for order in [stamps.clone(), stamps.iter().rev().copied().collect()] {
            let mut s = Sim::new();
            let ids: Vec<ObjectId> = order
                .iter()
                .zip(&owners)
                .map(|(stamp, owner)| s.world(*owner, *owner, "test.world_a", Some(*stamp)))
                .collect();
            let r = run_sba(s);
            let kept: Vec<ObjectId> = ids
                .iter()
                .copied()
                .filter(|id| r.after.on_battlefield(*id))
                .collect();
            assert!(kept.len() <= 1, "seed {seed}: kept {kept:?}");
            assert_eq!(kept.len(), usize::from(unique), "seed {seed}: {order:?}");
            if let Some(k) = kept.first() {
                let i = ids.iter().position(|x| x == k).expect("present");
                assert_eq!(order[i], max, "seed {seed}: the newest stays");
            }
            for id in &ids {
                if !kept.contains(id) {
                    assert_to_owners_graveyard(&r, *id, ZoneChangeCause::StateBasedAction);
                }
            }
            kept_counts.push(kept.len());
        }
        assert_eq!(kept_counts[0], kept_counts[1], "seed {seed}");
    }
}

/// Name pool of the legend property (CR 201.2a: a legend gets 0, 1 or 2 of these).
const PROPERTY_NAMES: [&str; 4] = ["test.name_x", "test.name_y", "test.name_z", "test.name_w"];

// REQ-SBA-001, REQ-CORE-001, CR 704.5j, 201.2a, 101.4, 613.1b: (c) after a legend pass the
// survivors of each component are a maximal independent set. Seeded boards: 2 or 3 players
// with a random turn order and active player; legends with random name sets (0-2 names from a
// pool of 4), random owners and stored controllers, some under a layer 2 control effect. Per
// evaluated controller the components (connected by "shares a name", at least one edge) are
// asked once each, in APNAP order then lowest id, with the contract's request shape; each is
// answered with a random valid survivor set or the default; the battlefield survivors equal the
// chosen set, every other member went to its owner's graveyard, isolated legends stay.
// (PR #214 round 1, F7: this property asserted "each (controller, name) group keeps exactly
// one"; the brief's T2 replaces it with "survivors of each component are a maximal independent
// set".)
#[test]
fn req_sba_001_property_legend_rule() {
    let mut removed_somewhere = 0;
    let mut multi_survivor = 0;
    let mut non_clique = 0;
    let mut defaults = 0;
    for seed in 0..SEEDS {
        let mut g = Lcg::new(seed ^ 0x1E6);
        let n_players = 2 + usize::try_from(g.below(2)).expect("fits");
        let mut s = Sim::with_players(n_players);
        let mut order: Vec<PlayerId> = s.game.turn_order().to_vec();
        // A seeded shuffle of the turn order and a random active player.
        for i in (1..order.len()).rev() {
            let j = usize::try_from(g.below(i as u64 + 1)).expect("fits");
            order.swap(i, j);
        }
        let active = g.pick(&order);
        s.set_turn(&order, active);
        let pos = order.iter().position(|p| *p == active).expect("in order");
        let apnap: Vec<PlayerId> = (0..order.len())
            .map(|k| order[(pos + k) % order.len()])
            .collect();

        let mut evaluated: BTreeMap<ObjectId, PlayerId> = BTreeMap::new();
        let mut all_names = NameMap::new();
        let mut effect = 1;
        for _ in 0..(2 + g.below(8)) {
            let owner = g.pick(&order);
            let stored = g.pick(&order);
            let count = g.pick(&[0u64, 1, 1, 1, 2, 2]);
            let mut names: Vec<&str> = Vec::new();
            for _ in 0..count {
                let n = g.pick(&PROPERTY_NAMES);
                if !names.contains(&n) {
                    names.push(n);
                }
            }
            let id = s.legend_named(owner, stored, &names);
            let mut ctrl = stored;
            if g.chance(1, 5) {
                ctrl = g.pick(&order);
                s.gain_control(effect, id, ctrl);
                effect += 1;
            }
            evaluated.insert(id, ctrl);
            all_names.insert(id, name_set(&names));
        }

        // The oracle: components per evaluated controller, in APNAP then lowest-id order.
        let mut expected: Vec<(PlayerId, Vec<ObjectId>)> = Vec::new();
        for p in &apnap {
            let mine: NameMap = all_names
                .iter()
                .filter(|(id, _)| evaluated[*id] == *p)
                .map(|(id, n)| (*id, n.clone()))
                .collect();
            for comp in conflict_components(&mine) {
                expected.push((*p, comp));
            }
        }
        let in_component: BTreeSet<ObjectId> = expected
            .iter()
            .flat_map(|(_, c)| c.iter().copied())
            .collect();
        let expected_findings: Vec<_> = in_component
            .iter()
            .map(|id| finding("704.5j", obj(*id)))
            .collect();
        assert_eq!(s.check(), expected_findings, "seed {seed}");

        let mut ga = Lcg::new(seed);
        let names_for_answers = all_names.clone();
        let mut used_default = 0;
        let r = drive_sba(s, |_, req| {
            let comp = candidates(req);
            if ga.chance(1, 4) {
                used_default += 1;
                return req.default_on_timeout.clone();
            }
            let valid = all_survivor_sets(&comp, &names_for_answers);
            let i = usize::try_from(ga.below(valid.len() as u64)).expect("fits");
            survivors(&valid[i])
        });
        defaults += used_default;
        assert_eq!(
            r.requests.len(),
            expected.len(),
            "seed {seed}: one request per component"
        );
        assert_eq!(r.outcome().findings, expected_findings, "seed {seed}");
        let mut removed = 0;
        for ((req, (p, comp)), answer) in r.requests.iter().zip(&expected).zip(&r.answers) {
            assert_component_request(req, *p, comp, &all_names);
            if !is_clique(comp, &all_names) {
                non_clique += 1;
            }
            let chosen: Vec<ObjectId> = match answer {
                fineprint_core::choice::ChoiceResponse::SubmitObjectSelection { objects } => {
                    let mut o = objects.clone();
                    o.sort();
                    o
                }
                other => panic!("seed {seed}: unexpected answer {other:?}"),
            };
            let left: Vec<ObjectId> = comp
                .iter()
                .copied()
                .filter(|id| r.after.on_battlefield(*id))
                .collect();
            assert_eq!(left, chosen, "seed {seed}: survivors are the chosen set");
            assert!(
                is_valid_survivor_set(comp, &all_names, &left),
                "seed {seed}: survivors {left:?} of {comp:?} are a maximal independent set"
            );
            if left.len() > 1 {
                multi_survivor += 1;
            }
            for id in comp.iter().filter(|id| !left.contains(id)) {
                assert_to_owners_graveyard(&r, *id, ZoneChangeCause::StateBasedAction);
                removed += 1;
            }
        }
        for id in all_names.keys().filter(|id| !in_component.contains(id)) {
            assert_still_on_battlefield(&r, *id);
        }
        assert_eq!(
            r.moves().len(),
            removed,
            "seed {seed}: only the removed move"
        );
        if removed > 0 {
            removed_somewhere += 1;
            assert_eq!(r.nonempty_batches().len(), 1, "seed {seed}: one event");
        }
    }
    assert!(removed_somewhere > 0, "some iteration removes a legend");
    assert!(
        multi_survivor > 0,
        "some component keeps more than one object"
    );
    assert!(non_clique > 0, "some component is not a clique");
    assert!(defaults > 0, "some request is answered with its default");
}

// REQ-SBA-001, CR 704.5q: (d) a permanent with a +1/+1 and b -1/-1 counters keeps exactly
// |a - b| counters of the larger kind and none of the smaller.
#[test]
fn req_sba_001_property_plus_minus_annihilate() {
    for a in 0..6 {
        for b in 0..6 {
            let mut s = Sim::new();
            let id = s.permanent(P1, P1, artifact_chars("test.artifact_a"));
            if a > 0 {
                s.counters(id, "+1/+1", a);
            }
            if b > 0 {
                s.counters(id, "-1/-1", b);
            }
            let r = run_sba(s);
            let (pa, pb) = (r.after.counter(id, "+1/+1"), r.after.counter(id, "-1/-1"));
            if a > 0 && b > 0 {
                assert_eq!(r.outcome().findings, vec![finding("704.5q", obj(id))]);
                assert_eq!((pa, pb), ((a - b).max(0), (b - a).max(0)), "{a}/{b}");
            } else {
                assert!(r.outcome().findings.is_empty(), "{a}/{b}");
                assert_eq!((pa, pb), (a, b), "{a}/{b}");
            }
        }
    }
}

// REQ-SBA-001, CR 704.3, 704.5a-704.5t: (e) every findings list (from `check` and from the
// pass) is in registry order then subject order, and the pass reports what `check` found.
#[test]
fn req_sba_001_property_findings_order() {
    let mut nonempty = 0;
    for seed in 0..SEEDS {
        let mut s = random_new_board(seed);
        let checked = s.check();
        assert_findings_ordered(&checked);
        let mut g = Lcg::new(seed);
        let r = drive_sba(s, move |_, req| random_keep(&mut g, req));
        assert_findings_ordered(&r.outcome().findings);
        assert_eq!(r.outcome().findings, checked, "seed {seed}");
        assert_eq!(r.outcome().performed, !checked.is_empty(), "seed {seed}");
        for list in [&r.outcome().unattached, &r.outcome().removed_from_game] {
            let mut sorted = list.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(&sorted, list, "seed {seed}: ascending");
        }
        let new_entries = checked
            .iter()
            .filter(|f| NEW_ENTRY_IDS.contains(&f.entry_id))
            .count();
        if new_entries > 0 {
            nonempty += 1;
        }
    }
    assert!(
        nonempty > SEEDS / 2,
        "the generator exercises the new entries ({nonempty})"
    );
}

// REQ-SBA-001, CR 704.3, 704.5f, 704.5j, 704.5n, 704.5q: (f) one `perform_all` handles a
// toughness-0 creature, a legend pair, an unattach and a counter removal together, in one
// event, whatever the creation order.
#[test]
fn req_sba_001_property_mixed_pass_simultaneous() {
    let orders: [[usize; 4]; 4] = [[0, 1, 2, 3], [3, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1]];
    for order in orders {
        let mut s = Sim::new();
        let mut zero = None;
        let mut legends = Vec::new();
        let mut eq = None;
        let mut pt = None;
        for role in order {
            match role {
                0 => zero = Some(s.creature(P1, "test.vanilla_2_0", 2, 0)),
                1 => {
                    legends.push(s.legend(P1, P1, "test.legend_a"));
                    legends.push(s.legend(P2, P1, "test.legend_a"));
                }
                2 => {
                    let id = s.permanent(P2, P2, equipment_chars("test.equipment_a"));
                    s.attach(id, AttachTarget::Player(P1));
                    eq = Some(id);
                }
                _ => {
                    let id = s.creature(P2, "test.vanilla_2_2", 2, 2);
                    s.counters(id, "+1/+1", 2);
                    s.counters(id, "-1/-1", 1);
                    pt = Some(id);
                }
            }
        }
        let (zero, eq, pt) = (zero.unwrap(), eq.unwrap(), pt.unwrap());
        let kept = legends[1];
        let r = drive_sba(s, |_, _| keep(kept));
        assert_eq!(r.requests.len(), 1, "{order:?}");
        let o = r.outcome();
        assert!(o.performed);
        assert_findings_ordered(&o.findings);
        let ids: Vec<&str> = o.findings.iter().map(|f| f.entry_id).collect();
        assert_eq!(
            ids,
            vec!["704.5f", "704.5j", "704.5j", "704.5n", "704.5q"],
            "{order:?}"
        );
        assert_to_owners_graveyard(&r, zero, ZoneChangeCause::StateBasedAction);
        assert_to_owners_graveyard(&r, legends[0], ZoneChangeCause::StateBasedAction);
        assert_still_on_battlefield(&r, kept);
        assert_eq!(o.unattached, vec![eq]);
        assert_eq!(r.after.permanents.attached_to(eq), None);
        assert_eq!(r.after.counter(pt, "+1/+1"), 1);
        assert_eq!(r.after.counter(pt, "-1/-1"), 0);
        assert_eq!(
            r.nonempty_batches().len(),
            1,
            "one event (CR 704.3): {order:?}"
        );
    }
}
