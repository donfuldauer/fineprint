// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property test: last known information equals the layer system's evaluation (#184 FP-145;
//! REQ-LAYER-001, REQ-CORE-003; CR 608.2h, 613.1-613.4, 613.7). For generated sequences of
//! (grant type, set P/T, modify P/T) effects with random timestamps, the LKI characteristics
//! captured by `EventContext::execute` with the snapshot `LayeredProvider` equal
//! `layers::evaluate(effects, env, id)` of the same snapshot; the base-only control equals the
//! base. Fixed seeds only (R3): no clock, no entropy. Black-box: written from the brief only.

mod spec_layers_consumers_support;

use fineprint_base::ids::Timestamp;
use fineprint_core::layers::{LayeredProvider, evaluate};
use fineprint_core::state::GameEnvironment;
use fineprint_core::zones::ZoneKind;
use spec_layers_consumers_support::*;

const CASES: u64 = 200;
const GRANTABLE_TYPES: [&str; 3] = ["Artifact", "Enchantment", "Land"];

/// One generated case: returns (LKI bytes with the evaluated provider, whether the evaluation
/// differs from the base).
fn one_case(seed: u64) -> (Vec<u8>, bool) {
    let mut rng = Rng::new(seed);
    let mut t = Table::new();
    let a = t.create(ZoneKind::Battlefield, P1);
    let mut objects = Objects::new();
    let (bp, bt) = (rng.range(0, 5), rng.range(1, 5));
    objects.put(a, creature("test.prop_subject", bp, bt), P1);
    if rng.below(3) == 0 {
        objects.counter(a, "+1/+1", rng.range(1, 3));
    }
    // REQ-CORE-002: the effects' timestamps 1..=40 are issued by the state's generator first.
    for _ in 0..40 {
        t.game.issue_timestamp().expect("timestamp");
    }
    let n = rng.below(7);
    for _ in 0..n {
        let ts = Timestamp::new(1 + rng.below(40));
        match rng.below(3) {
            0 => {
                let ty = GRANTABLE_TYPES[usize::try_from(rng.below(3)).expect("fits")];
                register_at(&mut t.game, ts, Layer::L4, &[a], grant_type(ty));
            }
            1 => {
                let (p, q) = (rng.range(0, 9), rng.range(0, 9));
                register_at(&mut t.game, ts, Layer::L7b, &[a], set_pt(p, q));
            }
            _ => {
                let (p, q) = (rng.range(-3, 3), rng.range(-3, 3));
                register_at(&mut t.game, ts, Layer::L7c, &[a], modify_pt(p, q));
            }
        }
    }
    let snap = t.game.clone();
    let env = GameEnvironment::new(&snap, &objects);
    let provider = LayeredProvider::new(snap.effects(), &env);
    let expected = evaluate(snap.effects(), &env, a)
        .expect("known object")
        .characteristics;

    // Control first, on a clone: the base-only provider stores the base.
    let mut control = t.clone();
    control.exec(&BaseProvider(&objects), zone_change(a, ZoneKind::Graveyard));
    assert_eq!(
        control.lki_of(a).characteristics(),
        objects.base_chars(a),
        "seed {seed}: control"
    );

    t.exec(&provider, zone_change(a, ZoneKind::Graveyard));
    let got = t.lki_of(a).characteristics().clone();
    assert_eq!(got, expected, "seed {seed}: LKI equals evaluate()");
    let differs = expected != *objects.base_chars(a);
    if differs {
        assert_ne!(
            control.lki_of(a).characteristics(),
            &got,
            "seed {seed}: control discriminates"
        );
    }
    (bytes(&t.lki), differs)
}

// REQ-LAYER-001, REQ-CORE-003; CR 608.2h, 613.1d, 613.4b, 613.4c, 613.7: LKI == evaluate() for
// generated effect sequences, deterministic across reruns of the same seed.
#[test]
fn lki_equals_evaluate_for_generated_effect_sequences() {
    let mut discriminating = 0u64;
    for seed in 0..CASES {
        let s = 0x0184_0000 + seed;
        let (first, differs) = one_case(s);
        let (second, _) = one_case(s);
        assert_eq!(first, second, "seed {s}: deterministic");
        if differs {
            discriminating += 1;
        }
    }
    // The generator really exercises the evaluated path (most cases change something).
    assert!(discriminating >= CASES / 2, "{discriminating} of {CASES}");
}
