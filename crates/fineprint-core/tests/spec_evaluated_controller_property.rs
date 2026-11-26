// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property / determinism tests for the evaluated (layer-2) controller (#216 FP-160;
//! REQ-LAYER-001, REQ-REPL-* (the replacement chooser of the REQ-EVENT-001 bus), REQ-CORE-003,
//! REQ-EVENT-001; CR 613.1b, 613.7, 616.1, 603.10a, 400.3). Fixed seeds only (R3, R7).
//! Black-box: written from the work-item brief and the public API only.

#[path = "spec_evaluated_controller_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::layers::LayerModification;
use fineprint_core::zones::{MoveRequest, ZoneId, ZoneKind};
use support::*;

/// One seeded random world: `players` players (2-4), a battlefield creature `a` with a random
/// owner and stored controller, and 0-3 layer-2 `SetController` effects with distinct
/// timestamps registered in a random order. `expected` is the controller of the effect with
/// the greatest timestamp, or the stored controller if there is none.
struct World {
    table: Table,
    objects: Objects,
    a: ObjectId,
    owner: PlayerId,
    stored: PlayerId,
    count: usize,
    expected: PlayerId,
}

fn world(seed: u64) -> World {
    let mut rng = Rng::new(seed);
    let players = 2 + rng.below(3) as usize;
    let mut table = Table::new(players);
    let pick = |rng: &mut Rng| pid(1 + rng.below(players as u64));
    let owner = pick(&mut rng);
    let stored = pick(&mut rng);
    let a = table.permanent(owner, stored);
    let mut objects = Objects::new();
    objects.put(a, creature("test.creature_2_2", 2, 2), stored);

    let count = rng.below(4) as usize;
    // Distinct, increasing timestamps; effect i gets stamps[i] and controller targets[i].
    let stamps: Vec<_> = (0..count)
        .map(|_| table.game.stamp_resolved_effect().expect("timestamp"))
        .collect();
    let targets: Vec<PlayerId> = (0..count).map(|_| pick(&mut rng)).collect();
    let mut order: Vec<usize> = (0..count).collect();
    rng.shuffle(&mut order);
    for &i in &order {
        register_at(
            &mut table.game,
            stamps[i],
            Layer::L2,
            &[a],
            LayerModification::SetController(targets[i]),
        );
    }
    let expected = targets.last().copied().unwrap_or(stored);
    World {
        table,
        objects,
        a,
        owner,
        stored,
        count,
        expected,
    }
}

/// The recorded LKI controller and the moved card's destination zone.
fn lki_outcome(w: &World) -> (Option<PlayerId>, ZoneId) {
    let mut t = w.table.clone();
    let req = MoveRequest::new(ZoneKind::Graveyard);
    let captured = with_layered(&w.table, &w.objects, |p| t.capture(p, w.a, &req));
    assert_eq!(captured, Ok(true));
    let controller = t.lki_of(w.a).controller();
    assert_eq!(t.lki_of(w.a).owner(), w.owner);
    let moved = t.zones.move_object(&mut t.ids, w.a, req).expect("move");
    (controller, moved.to)
}

/// The player asked the CR 616.1 choice between two competing redirects.
fn chooser(w: &World) -> PlayerId {
    let bus = Bus {
        table: w.table.clone(),
        registry: registry(vec![
            redirect(1, w.a, ZoneKind::Exile),
            redirect(2, w.a, ZoneKind::Hand),
        ]),
    };
    let r = with_layered(&w.table, &w.objects, |p| {
        run_bus(bus, p, zone_change(w.a, ZoneKind::Graveyard), &[])
    });
    assert_eq!(r.requests.len(), 1);
    r.choosers()[0]
}

const SEEDS: u64 = 96;

// Scenario 13. REQ-LAYER-001, REQ-REPL-*, REQ-EVENT-001, REQ-CORE-003; CR 613.7 (timestamp
// order within layer 2), CR 613.1b, CR 616.1, CR 603.10a, CR 400.3; R3 determinism: over fixed
// seeds, the recorded LKI controller and the chooser's addressed player both equal the
// controller of the greatest-timestamp layer-2 effect (else the stored controller), the card
// goes to its owner's graveyard, and repeated runs from the same state give identical results.
#[test]
fn evaluated_controller_matches_latest_layer2_effect() {
    let mut with_effects = 0;
    let mut changed = 0;
    for seed in 0..SEEDS {
        let w = world(0x0216_0000 + seed);
        let (lki, to) = lki_outcome(&w);
        assert_eq!(lki, Some(w.expected), "seed {seed}: LKI controller");
        assert_eq!(
            to,
            ZoneId::Graveyard(w.owner),
            "seed {seed}: owner's graveyard"
        );
        let c = chooser(&w);
        assert_eq!(c, w.expected, "seed {seed}: chooser");
        // Determinism: a rebuilt world and a repeated run give the same answers.
        let again = world(0x0216_0000 + seed);
        assert_eq!(again.expected, w.expected);
        assert_eq!(lki_outcome(&again), (lki, to), "seed {seed}: repeat LKI");
        assert_eq!(chooser(&again), c, "seed {seed}: repeat chooser");
        assert_eq!(chooser(&w), c, "seed {seed}: same state, repeat chooser");
        if w.count > 0 {
            with_effects += 1;
        }
        if w.expected != w.stored {
            changed += 1;
        }
    }
    assert!(
        with_effects > SEEDS / 2,
        "the seeds exercise layer-2 effects"
    );
    assert!(
        changed > SEEDS / 4,
        "the seeds exercise real control changes"
    );
}
