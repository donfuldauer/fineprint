// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property-style tests for LKI capture, Memory upkeep and durable references over random
//! sequences of creates and moves (REQ-CORE-003, REQ-CORE-007, REQ-CORE-001, CR 400.7,
//! 400.7d). Plain loops driven by a seeded LCG (R3); no wall clock. Black-box: written from
//! the work-item brief only.

#[path = "spec_memory_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::memory::{ChoiceValue, DurableRef, SpellHistory};
use fineprint_core::zones::{MoveRequest, Position, ZoneKind};
use support::*;

const STEPS: usize = 300;
const SEEDS: [u64; 8] = [1, 2, 3, 17, 99, 1234, 0xDEAD_BEEF, u64::MAX];

/// Runs one random sequence; checks invariants (i)-(iii) after every step and returns the final
/// world for the determinism check (iv).
fn run(seed: u64) -> World {
    let mut rng = Lcg::new(seed);
    let mut w = World::new();
    let mut refs: Vec<DurableRef> = Vec::new();
    let mut live: Vec<ObjectId> = Vec::new();
    let players: [PlayerId; 2] = [P1, P2];

    for step in 0..STEPS {
        let roll = rng.below(10);
        if live.is_empty() || roll < 2 {
            let kind = rng.pick(&ZoneKind::ALL);
            let owner = rng.pick(&players);
            let id = w.create(kind, owner);
            live.push(id);
            refs.push(DurableRef::new(&w.zones, id).expect("present"));
        } else {
            let idx = usize::try_from(rng.below(live.len() as u64)).unwrap();
            let id = live[idx];
            // Sometimes give the object a Memory before it moves.
            match rng.below(4) {
                0 => w
                    .memory_mut(id)
                    .set_choice("c", ChoiceValue::Number(step as i64)),
                1 => w
                    .memory_mut(id)
                    .set_persistent_choice("p", ChoiceValue::Mode(step as u32)),
                2 => w.memory_mut(id).set_spell_history(SpellHistory {
                    x_value: Some(step as i64),
                    ..SpellHistory::default()
                }),
                _ => {}
            }
            let from = w.zones.object(id).expect("live").zone.kind();
            let to = rng.pick(&ZoneKind::ALL);
            let position = match rng.below(3) {
                0 => Position::Top,
                1 => Position::Bottom,
                _ => Position::Index(usize::try_from(rng.below(4)).unwrap()),
            };
            let req = MoveRequest {
                position,
                ..MoveRequest::new(to)
            };
            let had_memory = w.memories.get(id).cloned();
            let lki_before = w.lki.len();
            let moved = w.move_req(id, req);

            // (i) every exit has exactly one record keyed by the old id; none for repositions.
            if expect_capture(from, to) {
                assert_ne!(moved.old, moved.new, "seed {seed} step {step}");
                assert_eq!(w.lki.len(), lki_before + 1, "seed {seed} step {step}");
                let rec = w.lki.get(moved.old).expect("record keyed by old id");
                assert_eq!(rec.object(), moved.old);
                assert_eq!(rec.zone().kind(), from);
                assert!(w.lki.get(moved.new).is_none());
                // Memory carry-over (CR 400.7, 400.7d, 607.2p).
                let new_mem = memory_or_empty(&w.memories, moved.new);
                let carried = from == ZoneKind::Stack && to == ZoneKind::Battlefield;
                let old_hist = had_memory.as_ref().and_then(|m| m.spell_history().cloned());
                if carried {
                    assert_eq!(new_mem.spell_history().cloned(), old_hist);
                } else {
                    assert_eq!(new_mem.spell_history(), None);
                }
                assert_eq!(new_mem.choice("c"), None);
                assert_eq!(
                    new_mem.choice("p"),
                    had_memory.as_ref().and_then(|m| m.choice("p"))
                );
                assert_eq!(rec.memory(), &had_memory.clone().unwrap_or_default());
            } else {
                assert_eq!(moved.old, moved.new, "seed {seed} step {step}");
                assert_eq!(w.lki.len(), lki_before, "seed {seed} step {step}");
                assert_eq!(w.memories.get(id).cloned(), had_memory);
            }
            live[idx] = moved.new;
            refs.push(DurableRef::new(&w.zones, moved.new).expect("present"));
        }

        // (ii) no Memory for any id that is not in a zone.
        for raw in 1..=w.ids.last_issued() {
            let id = oid(raw);
            if w.zones.object(id).is_none() {
                assert!(
                    w.memories.get(id).is_none(),
                    "seed {seed} step {step}: memory kept for {id:?}"
                );
            }
        }
        // LKI ids ascending and all of them gone from the zones.
        let ids = w.lki.ids();
        assert!(ids.windows(2).all(|p| p[0] < p[1]));
        for id in ids {
            assert!(w.zones.object(id).is_none());
        }

        // (iii) a ref resolves iff its generation is still the card's current object.
        for r in &refs {
            let current = w.zones.object_of_card(r.card()) == Some(r.generation());
            assert_eq!(
                r.resolve(&w.zones, None),
                current.then_some(r.generation()),
                "seed {seed} step {step}"
            );
            for k in ZoneKind::ALL {
                let in_k =
                    current && w.zones.object(r.generation()).map(|o| o.zone.kind()) == Some(k);
                assert_eq!(r.resolve(&w.zones, Some(k)), in_k.then_some(r.generation()));
            }
        }
    }
    w
}

// REQ-CORE-003, REQ-CORE-007, CR 400.7, 400.7d, 607.2p: invariants (i)-(iii) hold over random
// sequences.
#[test]
fn property_random_sequences_keep_invariants() {
    for seed in SEEDS {
        let w = run(seed);
        assert!(!w.lki.is_empty(), "seed {seed} exercised exits");
    }
}

// REQ-CORE-001, REQ-CORE-006: running the same sequence twice gives identical LkiStore and
// MemoryTable encodings (and values).
#[test]
fn property_same_sequence_is_deterministic() {
    for seed in SEEDS {
        let a = run(seed);
        let b = run(seed);
        assert_eq!(a.lki, b.lki);
        assert_eq!(a.memories, b.memories);
        assert_eq!(bytes(&a.lki), bytes(&b.lki), "seed {seed}");
        assert_eq!(bytes(&a.memories), bytes(&b.memories), "seed {seed}");
    }
    // Different seeds explore different sequences.
    assert_ne!(bytes(&run(1).lki), bytes(&run(2).lki));
}
