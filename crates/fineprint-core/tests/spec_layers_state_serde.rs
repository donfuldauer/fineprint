// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests: `GameState` serialization (format version 2) carries the continuous effects,
//! the 613.7 timestamp ledger and the effect-to-ability links; round trips are identity on
//! value and hash; malformed, version-1 and inconsistent input is rejected without panicking
//! (T2; REQ-CORE-001, REQ-NFR-006, REQ-LAYER-001, R12). Black-box: written from the work-item
//! brief (#183 FP-144) only.

#[path = "spec_layers_state_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_core::layers::{
    Affected, CounterHolder, EffectOrigin, LayerModification, SimultaneousStamp, StampReason,
};
use fineprint_core::state::{GameState, GeneratingAbility, STATE_FORMAT_VERSION};
use support::*;

/// Asserts `s` survives serialize -> deserialize as identity on value, hash and bytes.
fn assert_round_trip(s: &GameState, what: &str) {
    let b = s.serialize().unwrap();
    assert_eq!(b, s.serialize().unwrap(), "{what}: byte-deterministic");
    let d = GameState::deserialize(&b).unwrap_or_else(|e| panic!("{what}: {e:?}"));
    assert_eq!(&d, s, "{what}: value");
    assert_eq!(d.state_hash(), s.state_hash(), "{what}: hash");
    assert_eq!(d.serialize().unwrap(), b, "{what}: bytes");
    assert_eq!(d.effects(), s.effects(), "{what}: effects");
    assert_eq!(d.timestamps(), s.timestamps(), "{what}: ledger");
}

// REQ-CORE-001, REQ-NFR-006, REQ-LAYER-001, CR 613.1 (T2): states with effects in each layer
// 1a..7d, a multi-layer group registered through `effects_mut().register_group` (CR 613.6),
// ETB links, a ledger with objects, counters (CR 613.7c), stickers (CR 613.7k) and simultaneous
// stamps (CR 613.7m) round-trip as identity on value and hash.
#[test]
fn round_trip_is_identity() {
    assert_round_trip(&GameState::new(1, 1, 20).unwrap(), "fresh");
    assert_round_trip(&base(), "base");
    assert_round_trip(&small_state(), "small");
    let rich = rich_state();
    // The rich state really holds every layer and the group.
    let layers: BTreeSet<String> = rich
        .effects()
        .iter()
        .map(|(_, e)| format!("{:?}", e.layer))
        .collect();
    assert_eq!(layers.len(), 11, "{layers:?}");
    assert!(
        rich.effects()
            .iter()
            .any(|(_, e)| matches!(e.affected, Affected::SameAsLayer(_)))
    );
    assert!(rich.timestamps().sticker_timestamp(oid(2), 7).is_some());
    assert!(
        rich.timestamps()
            .counter_timestamp(CounterHolder::Player(P3), "test.player_counter")
            .is_some()
    );
    assert_round_trip(&rich, "rich");
    // Effects removed, counters cleared, objects forgotten.
    let mut thinned = rich.clone();
    let ids: Vec<_> = thinned.effects().iter().map(|(id, _)| id).collect();
    assert!(thinned.remove_effect(ids[0]));
    thinned.clear_counter_stamp(CounterHolder::Object(oid(1)), "+1/+1");
    thinned.forget_object_stamps(oid(2));
    assert_round_trip(&thinned, "thinned");
}

// REQ-CORE-001, REQ-NFR-006 (T2): the format version is 2 and the top-level members are
// exactly the documented ones.
#[test]
fn format_version_two_and_top_level_members() {
    assert_eq!(STATE_FORMAT_VERSION, 2);
    for s in [GameState::new(42, 2, 20).unwrap(), rich_state()] {
        let v = wire(&s.serialize().unwrap());
        let keys: BTreeSet<String> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            set(&[
                "effect_generators",
                "effects",
                "format_version",
                "history",
                "ids",
                "players",
                "prng",
                "prng_algorithm",
                "timestamps",
                "turn",
            ])
        );
        assert_eq!(v["format_version"], serde_json::json!(2));
    }
}

// R12, REQ-CORE-001 (T2): version-1 bytes are rejected; the hand-crafting technique is
// checked on the unmodified version first.
#[test]
fn version_one_bytes_are_rejected() {
    for s in [GameState::new(42, 2, 20).unwrap(), small_state()] {
        let v = wire(&s.serialize().unwrap());
        // The technique itself round-trips an unmodified state.
        assert_eq!(GameState::deserialize(&unwire(&v)).unwrap(), s);
        for bad in [1u64, 0, 3] {
            let mut w = v.clone();
            w["format_version"] = serde_json::json!(bad);
            assert!(
                is_decode_err(&GameState::deserialize(&unwire(&w))),
                "version {bad}"
            );
        }
        // A version-1 shape: version 1 without the new members.
        let mut old = v.clone();
        old["format_version"] = serde_json::json!(1);
        let m = old.as_object_mut().unwrap();
        m.remove("timestamps");
        m.remove("effect_generators");
        assert!(is_decode_err(&GameState::deserialize(&unwire(&old))));
        // Version 2 without the new members is not a state either.
        let mut missing = v.clone();
        missing.as_object_mut().unwrap().remove("timestamps");
        assert!(is_decode_err(&GameState::deserialize(&unwire(&missing))));
    }
}

// R12, REQ-CORE-001 (T2): every strict prefix and every trailing byte is rejected.
#[test]
fn truncated_and_trailing_bytes_are_rejected() {
    for s in [small_state(), rich_state()] {
        let b = s.serialize().unwrap();
        for len in 0..b.len() {
            assert!(
                is_decode_err(&GameState::deserialize(&b[..len])),
                "prefix {len} of {}",
                b.len()
            );
        }
        for extra in [0u8, 1, 0x90, 0xC0, 0xFF] {
            let mut c = b.clone();
            c.push(extra);
            assert!(is_decode_err(&GameState::deserialize(&c)));
        }
    }
}

// R12, REQ-CORE-001 (T2): bit-flips over every byte of a small valid encoding never panic;
// each either decodes to a self-consistent state or is a decode error.
#[test]
fn bit_flips_over_every_byte_never_panic() {
    let b = small_state().serialize().unwrap();
    let mut errs = 0usize;
    for pos in 0..b.len() {
        for mask in [0x01u8, 0x02, 0x10, 0x80, 0xFF] {
            let mut c = b.clone();
            c[pos] ^= mask;
            match GameState::deserialize(&c) {
                Err(e) => {
                    assert!(
                        matches!(e, fineprint_core::state::StateError::Decode(_)),
                        "{e:?}"
                    );
                    errs += 1;
                }
                Ok(t) => {
                    let b2 = t.serialize().unwrap();
                    let t2 = GameState::deserialize(&b2).unwrap();
                    assert_eq!(t2, t);
                    assert_eq!(t2.state_hash(), t.state_hash());
                }
            }
        }
    }
    assert!(errs > 0);
}

// R12 (T2): bounded random byte buffers (fixed-seed local generator) never panic, raw and
// spliced after a valid prefix.
#[test]
fn random_buffers_never_panic() {
    let valid = rich_state().serialize().unwrap();
    let mut rng = Rng::new(0x0183_F144);
    for i in 0..2_000 {
        let len = rng.below(600) as usize;
        let mut buf: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
        if i % 2 == 1 {
            let cut = rng.below(valid.len() as u64) as usize;
            let mut spliced = valid[..cut].to_vec();
            spliced.extend(buf);
            buf = spliced;
        }
        match GameState::deserialize(&buf) {
            Err(e) => assert!(matches!(e, fineprint_core::state::StateError::Decode(_))),
            Ok(t) => {
                let t2 = GameState::deserialize(&t.serialize().unwrap()).unwrap();
                assert_eq!(t2, t);
            }
        }
    }
}

/// Every array inside `v`, depth first, as a mutable reference path is awkward in serde_json;
/// this collects the JSON pointers of arrays of exactly `len` elements.
fn arrays_of_len(v: &serde_json::Value, len: usize, at: String, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Array(items) => {
            if items.len() == len {
                out.push(at.clone());
            }
            for (i, x) in items.iter().enumerate() {
                arrays_of_len(x, len, format!("{at}/{i}"), out);
            }
        }
        serde_json::Value::Object(m) => {
            for (k, x) in m {
                arrays_of_len(x, len, format!("{at}/{k}"), out);
            }
        }
        _ => {}
    }
}

/// Replaces every `true` inside `v` with `false`; how many were replaced.
fn flip_trues(v: &mut serde_json::Value) -> usize {
    match v {
        serde_json::Value::Bool(b) if *b => {
            *b = false;
            1
        }
        serde_json::Value::Array(items) => items.iter_mut().map(flip_trues).sum(),
        serde_json::Value::Object(m) => m.values_mut().map(flip_trues).sum(),
        _ => 0,
    }
}

// R12, REQ-CORE-001, REQ-CORE-002, REQ-LAYER-001, CR 613.4a (T2): inconsistent states are
// rejected with a decode error: an effect id above the effect-id generator, a ledger timestamp
// above the timestamp generator, a static effect with a source and no link, a non-CDA layer-7a
// effect, a duplicated effect id.
#[test]
fn inconsistent_states_are_rejected() {
    // Effect id above the generator.
    let mut s = base();
    s.register_effect(
        resolved(
            ts(1),
            Layer::L5,
            &[1],
            LayerModification::AddColors(ColorBitmask::RED),
        ),
        None,
    )
    .unwrap();
    let mut v = wire(&s.serialize().unwrap());
    assert_eq!(GameState::deserialize(&unwire(&v)).unwrap(), s);
    v["ids"]["effect"] = serde_json::json!(0);
    assert!(
        is_decode_err(&GameState::deserialize(&unwire(&v))),
        "effect id"
    );

    // ETB link id above the generator.
    let mut s = base();
    let id = s.issue_effect_id().unwrap();
    s.link_effect_generator(id, GeneratingAbility::new(oid(1), own_ability(1)))
        .unwrap();
    let mut v = wire(&s.serialize().unwrap());
    v["ids"]["effect"] = serde_json::json!(0);
    assert!(
        is_decode_err(&GameState::deserialize(&unwire(&v))),
        "link id"
    );

    // Ledger timestamp above the generator (object, sticker, counter).
    for k in 0..3 {
        let mut s = GameState::new(1, 2, 20).unwrap();
        match k {
            0 => {
                s.stamp_object(oid(1), StampReason::EntersZone).unwrap();
            }
            1 => {
                s.stamp_sticker(oid(1), 1).unwrap();
            }
            _ => {
                s.stamp_counters(CounterHolder::Player(P1), "test.c")
                    .unwrap();
            }
        }
        let mut v = wire(&s.serialize().unwrap());
        assert_eq!(GameState::deserialize(&unwire(&v)).unwrap(), s);
        v["ids"]["timestamp"] = serde_json::json!(0);
        assert!(
            is_decode_err(&GameState::deserialize(&unwire(&v))),
            "ledger kind {k}"
        );
    }

    // Static effect with a source and no link: the links of a fresh state spliced in.
    let s = small_state();
    let empty = wire(&GameState::new(3, 2, 20).unwrap().serialize().unwrap());
    let mut v = wire(&s.serialize().unwrap());
    // Keep the ETB link out of the picture: only the static's link is dropped.
    v["effect_generators"] = empty["effect_generators"].clone();
    assert!(
        is_decode_err(&GameState::deserialize(&unwire(&v))),
        "unlinked static"
    );
    // The same through the registry directly: serialize may refuse; if it does not, the bytes
    // must not decode.
    let mut raw = base();
    let id = raw.issue_effect_id().unwrap();
    raw.effects_mut()
        .register(
            id,
            static_sel(
                oid(1),
                ts(1),
                Layer::L6,
                SEL_ALL,
                LayerModification::RemoveAllAbilities,
            ),
        )
        .unwrap();
    if let Ok(b) = raw.serialize() {
        assert!(
            is_decode_err(&GameState::deserialize(&b)),
            "unlinked static (raw)"
        );
    }

    // Non-CDA layer-7a effect: a CDA's flag flipped in the wire form.
    let mut s = base();
    let mut cda = static_sel(
        oid(2),
        ts(1),
        Layer::L7a,
        SEL_NONE,
        LayerModification::SetPt {
            power: Some(1),
            toughness: Some(1),
        },
    );
    cda.is_cda = true;
    cda.affected = Affected::Objects(objs(&[2]));
    s.register_effect(cda, Some(GeneratingAbility::new(oid(2), own_ability(2))))
        .unwrap();
    let mut v = wire(&s.serialize().unwrap());
    assert_eq!(
        flip_trues(&mut v["effects"]),
        1,
        "exactly one true flag: is_cda"
    );
    assert!(
        is_decode_err(&GameState::deserialize(&unwire(&v))),
        "non-CDA 7a"
    );
    let mut raw = base();
    let id = raw.issue_effect_id().unwrap();
    raw.effects_mut()
        .register(
            id,
            effect(
                None,
                P1,
                ts(1),
                Layer::L7a,
                false,
                EffectOrigin::Resolved,
                Affected::Objects(objs(&[1])),
                LayerModification::SetPt {
                    power: Some(1),
                    toughness: Some(1),
                },
            ),
        )
        .unwrap();
    if let Ok(b) = raw.serialize() {
        assert!(
            is_decode_err(&GameState::deserialize(&b)),
            "non-CDA 7a (raw)"
        );
    }

    // Duplicated effect id: two effects with distinct ids; the entry list's second element
    // replaced by a copy of the first.
    let mut s = base();
    for colors in [ColorBitmask::RED, ColorBitmask::BLUE] {
        s.register_effect(
            resolved(ts(1), Layer::L5, &[1], LayerModification::SetColors(colors)),
            None,
        )
        .unwrap();
    }
    let v = wire(&s.serialize().unwrap());
    let mut pointers = Vec::new();
    arrays_of_len(&v["effects"], 2, String::new(), &mut pointers);
    assert!(
        !pointers.is_empty(),
        "the effects member holds a list of two entries"
    );
    let mut rejected = 0;
    for p in &pointers {
        let mut w = v.clone();
        let arr = w["effects"].pointer_mut(p).unwrap().as_array_mut().unwrap();
        let first = arr[0].clone();
        if arr[1] == first {
            continue;
        }
        arr[1] = first;
        let r = GameState::deserialize(&unwire(&w));
        if is_decode_err(&r) {
            rejected += 1;
        }
    }
    // The outermost two-element list is the entry list.
    let mut w = v.clone();
    let arr = w["effects"]
        .pointer_mut(&pointers[0])
        .unwrap()
        .as_array_mut()
        .unwrap();
    arr[1] = arr[0].clone();
    assert!(
        is_decode_err(&GameState::deserialize(&unwire(&w))),
        "duplicated effect id ({} candidate lists, {rejected} rejected)",
        pointers.len()
    );
}

// REQ-CORE-001, REQ-CORE-002, CR 613.7m (T2): a simultaneous-stamp ledger survives the round
// trip with its order.
#[test]
fn simultaneous_stamps_round_trip_in_order() {
    let mut s = GameState::new(5, 3, 20).unwrap();
    s.set_active_player(Some(P3)).unwrap();
    let out = s
        .stamp_objects_simultaneously(
            &[
                SimultaneousStamp {
                    object: oid(1),
                    player: P1,
                    choice: 0,
                },
                SimultaneousStamp {
                    object: oid(2),
                    player: P3,
                    choice: 0,
                },
            ],
            StampReason::EntersZone,
        )
        .unwrap();
    let d = GameState::deserialize(&s.serialize().unwrap()).unwrap();
    for (o, t) in out {
        assert_eq!(d.timestamps().object_timestamp(o), Some(t));
    }
    assert_eq!(d, s);
}
