// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! T3 and T6 (REQ-NFR-006): MessagePack round trip of nested enums, a hard-coded byte vector,
//! and bounded random decode input (R12). Transport only: nothing here feeds a hash.

use std::collections::BTreeMap;

use fineprint_spike_libs::prng::Pcg64Dxsm;
use fineprint_spike_libs::transport::{decode, encode};
use serde::{Deserialize, Serialize};

/// Innermost level: every serde variant shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Leaf {
    Unit,
    Newtype(i64),
    Tuple(u64, String),
    Struct { id: u64, tags: Vec<i64> },
}

/// Middle level: enums holding containers of the inner enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Middle {
    Wrap(Leaf),
    Many(Vec<Leaf>),
    Maybe(Option<Leaf>),
    Keyed(BTreeMap<u32, Leaf>),
}

/// Outer level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Outer {
    Plain,
    Extremes(i64, i64, u64),
    Nested { label: String, middle: Middle },
}

fn sample() -> Vec<Outer> {
    let text = "Prüfung ✓ 漢字 🎲".to_string();
    let leaves = vec![
        Leaf::Unit,
        Leaf::Newtype(i64::MIN),
        Leaf::Tuple(u64::MAX, text.clone()),
        Leaf::Struct {
            id: 0,
            tags: vec![i64::MIN, -1, 0, 1, i64::MAX],
        },
    ];
    let keyed = leaves
        .iter()
        .cloned()
        .zip([0, 7, u32::MAX, 300])
        .map(|(leaf, key)| (key, leaf))
        .collect();
    vec![
        Outer::Plain,
        Outer::Extremes(i64::MIN, i64::MAX, u64::MAX),
        Outer::Nested {
            label: text.clone(),
            middle: Middle::Wrap(leaves[3].clone()),
        },
        Outer::Nested {
            label: String::new(),
            middle: Middle::Many(leaves.clone()),
        },
        Outer::Nested {
            label: "none".to_string(),
            middle: Middle::Maybe(None),
        },
        Outer::Nested {
            label: "some".to_string(),
            middle: Middle::Maybe(Some(leaves[2].clone())),
        },
        Outer::Nested {
            label: text,
            middle: Middle::Keyed(keyed),
        },
    ]
}

#[test]
fn req_nfr_006_messagepack_round_trips_nested_enums() {
    let values = sample();
    for value in &values {
        let bytes = encode(value).unwrap_or_default();
        assert_eq!(decode::<Outer>(&bytes).ok().as_ref(), Some(value));
    }
    let bytes = encode(&values).unwrap_or_default();
    assert_eq!(decode::<Vec<Outer>>(&bytes).ok(), Some(values));
}

/// Hard-coded bytes, so every platform must produce the identical encoding.
#[test]
fn req_nfr_006_messagepack_bytes_are_fixed() {
    let unit = encode(&Outer::Plain).unwrap_or_default();
    // fixstr(5) "Plain"
    assert_eq!(unit, b"\xa5Plain");

    let extremes = encode(&Outer::Extremes(i64::MIN, i64::MAX, u64::MAX)).unwrap_or_default();
    let expected: &[u8] = &[
        0x81, // fixmap(1)
        0xa8, b'E', b'x', b't', b'r', b'e', b'm', b'e', b's', // fixstr(8)
        0x93, // fixarray(3)
        0xd3, 0x80, 0, 0, 0, 0, 0, 0, 0, // int64 i64::MIN, big-endian
        0xcf, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // i64::MAX as uint64
        0xcf, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // uint64 u64::MAX
    ];
    assert_eq!(extremes, expected);

    let nested = encode(&Outer::Nested {
        label: "é".to_string(),
        middle: Middle::Wrap(Leaf::Struct {
            id: 1,
            tags: vec![-1],
        }),
    })
    .unwrap_or_default();
    let expected: &[u8] = &[
        0x81, 0xa6, b'N', b'e', b's', b't', b'e', b'd', // {"Nested":
        0x82, // fixmap(2): struct as map with field names
        0xa5, b'l', b'a', b'b', b'e', b'l', 0xa2, 0xc3, 0xa9, // "label": "é"
        0xa6, b'm', b'i', b'd', b'd', b'l', b'e', // "middle":
        0x81, 0xa4, b'W', b'r', b'a', b'p', // {"Wrap":
        0x81, 0xa6, b'S', b't', b'r', b'u', b'c', b't', // {"Struct":
        0x82, 0xa2, b'i', b'd', 0x01, // {"id": 1,
        0xa4, b't', b'a', b'g', b's', 0x91, 0xff, // "tags": [-1]}}}}
    ];
    assert_eq!(nested, expected);
}

/// R12: bounded random and mutated inputs never panic (results may be Ok or Err).
#[test]
fn req_nfr_006_messagepack_random_input_never_panics() {
    let valid = encode(&sample()).unwrap_or_default();
    let mut rng = Pcg64Dxsm::new(56, 78);
    for round in 0..4000 {
        let mut input = if round % 2 == 0 {
            valid.clone()
        } else {
            (0..rng.next_u64() % 64)
                .map(|_| rng.next_u64() as u8)
                .collect()
        };
        if !input.is_empty() {
            for _ in 0..1 + rng.next_u64() % 4 {
                let at = (rng.next_u64() % input.len() as u64) as usize;
                input[at] = rng.next_u64() as u8;
            }
        }
        let _ = decode::<Vec<Outer>>(&input);
        let _ = decode::<Outer>(&input);
    }
}
