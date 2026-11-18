// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::state`. Black-box: written from the work-item brief only.
//!
//! Serialization: round-trip property test, snapshot-independent bytes, and malformed-input
//! handling (REQ-CORE-001, REQ-NFR-006, R12).

use fineprint_base::ids::PlayerId;
use fineprint_base::prng::SplitMix64;
use fineprint_core::state::{DesignationValue, GameState, MAX_PLAYERS, StateError};

/// Deterministic generator over `SplitMix64` with literal seeds.
struct Rng(SplitMix64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(SplitMix64::new(seed))
    }
    fn next(&mut self) -> u64 {
        self.0.next_u64()
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn i64(&mut self) -> i64 {
        const EDGES: [i64; 5] = [i64::MIN, -1, 0, 1, i64::MAX];
        if self.below(3) == 0 {
            EDGES[self.below(5) as usize]
        } else {
            self.next() as i64
        }
    }
    fn u64(&mut self) -> u64 {
        const EDGES: [u64; 4] = [0, 1, u64::MAX, u64::MAX - 1];
        if self.below(3) == 0 {
            EDGES[self.below(4) as usize]
        } else {
            self.next()
        }
    }
    fn text(&mut self) -> String {
        const KINDS: [&str; 8] = [
            "",
            "test.counter_a",
            "test.counter_b",
            "\u{fc}n\u{ef}-\u{4e2d}\u{6587}",
            "\u{1f642} test",
            "a",
            "z",
            "test.designation_x",
        ];
        if self.below(10) == 0 {
            "long_".repeat(60)
        } else {
            KINDS[self.below(8) as usize].to_string()
        }
    }
}

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

/// A generated state: 1..=256 players (mostly small), everything randomised.
fn gen_state(rng: &mut Rng) -> GameState {
    let r = rng.below(100);
    let n_raw = if r < 80 {
        1 + rng.below(4)
    } else if r < 97 {
        1 + rng.below(16)
    } else {
        1 + rng.below(MAX_PLAYERS as u64)
    };
    let n = n_raw as usize;
    let first = std::cmp::max(1, n / 2);
    let life = rng.i64();
    let seed = rng.next();
    let mut s = GameState::new(seed, first, life).unwrap();
    for _ in first..n {
        let l = rng.i64();
        s.add_player(l).unwrap();
    }
    for id in 1..=n as u64 {
        let life = rng.i64();
        let counters = rng.below(4);
        let designations = rng.below(4);
        let mut ops: Vec<(String, u64, u64, i64, String)> = Vec::new();
        for _ in 0..counters.max(designations) {
            ops.push((rng.text(), rng.u64(), rng.below(3), rng.i64(), rng.text()));
        }
        let p = s.player_mut(pid(id)).unwrap();
        p.set_life(life);
        for (i, (k, v, variant, int, text)) in ops.into_iter().enumerate() {
            if (i as u64) < counters {
                p.set_counter(&k, v);
            }
            if (i as u64) < designations {
                let d = match variant {
                    0 => DesignationValue::Flag,
                    1 => DesignationValue::Int(int),
                    _ => DesignationValue::Text(text),
                };
                p.set_designation(&k, d);
            }
        }
    }
    for _ in 0..rng.below(6) {
        s.prng_mut().next_u64();
    }
    for _ in 0..rng.below(4) {
        s.issue_card_id().unwrap();
    }
    for _ in 0..rng.below(4) {
        s.issue_object_id().unwrap();
    }
    for _ in 0..rng.below(4) {
        s.issue_ability_instance_id().unwrap();
    }
    for _ in 0..rng.below(4) {
        s.issue_effect_id().unwrap();
    }
    for _ in 0..rng.below(4) {
        s.issue_timestamp().unwrap();
    }
    for _ in 0..rng.below(4) {
        s.issue_link_id().unwrap();
    }
    s.set_turn_number(rng.u64());
    let active = if rng.below(2) == 0 {
        None
    } else {
        Some(pid(1 + rng.below(n as u64)))
    };
    s.set_active_player(active).unwrap();
    let priority = if rng.below(2) == 0 {
        None
    } else {
        Some(pid(1 + rng.below(n as u64)))
    };
    s.set_priority_player(priority).unwrap();
    let mut order: Vec<PlayerId> = s.turn_order().to_vec();
    for i in (1..order.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        order.swap(i, j);
    }
    s.set_turn_order(order).unwrap();
    s
}

fn is_decode_err(r: &Result<GameState, StateError>) -> bool {
    matches!(r, Err(StateError::Decode(_)))
}

fn corpus() -> Vec<GameState> {
    let mut v = Vec::new();
    v.push(GameState::new(1, 1, 20).unwrap());
    let mut two = GameState::new(42, 2, 20).unwrap();
    {
        let p = two.player_mut(pid(2)).unwrap();
        p.set_life(i64::MIN);
        p.set_counter("test.counter_a", u64::MAX);
        p.set_counter("", 3);
        p.set_designation("test.designation_a", DesignationValue::Flag);
        p.set_designation("test.designation_b", DesignationValue::Int(-7));
        p.set_designation(
            "test.designation_c",
            DesignationValue::Text("\u{4e2d}x".to_string()),
        );
    }
    two.prng_mut().next_u64();
    two.issue_card_id().unwrap();
    two.issue_timestamp().unwrap();
    two.set_turn_number(5);
    two.set_active_player(Some(pid(2))).unwrap();
    two.set_priority_player(Some(pid(1))).unwrap();
    two.set_turn_order(vec![pid(2), pid(1)]).unwrap();
    v.push(two);
    let mut rng = Rng::new(0x5EED_0001);
    for _ in 0..3 {
        let mut s = gen_state(&mut rng);
        // Keep the prefix test affordable: small states only.
        while s.player_count() > 8 {
            s = gen_state(&mut rng);
        }
        v.push(s);
    }
    v
}

// REQ-CORE-001, REQ-NFR-006: serialize -> deserialize is identity (value, hash, bytes).
#[test]
fn roundtrip_property_over_generated_states() {
    let mut rng = Rng::new(0xC0FF_EE01);
    let mut prev: Option<GameState> = None;
    for _ in 0..2_000 {
        let s = gen_state(&mut rng);
        let b = s.serialize().unwrap();
        assert_eq!(b, s.serialize().unwrap(), "serialize is byte-deterministic");
        assert_eq!(b, s.clone().serialize().unwrap());
        let d = GameState::deserialize(&b).unwrap();
        assert_eq!(d, s);
        assert_eq!(d.state_hash(), s.state_hash());
        assert_eq!(d.serialize().unwrap(), b);
        if let Some(p) = prev {
            // A serialization of one state never decodes to a different state.
            if p != s {
                assert_ne!(GameState::deserialize(&b).unwrap(), p);
                assert_ne!(b, p.serialize().unwrap());
            }
        }
        prev = Some(s);
    }
}

// REQ-CORE-001: the same literal seed generates byte-identical serializations.
#[test]
fn serialization_is_reproducible_from_a_seed() {
    for seed in [1u64, 2, 0xABCD, u64::MAX] {
        let a = gen_state(&mut Rng::new(seed)).serialize().unwrap();
        let b = gen_state(&mut Rng::new(seed)).serialize().unwrap();
        assert_eq!(a, b);
    }
}

// REQ-CORE-001: a two-player serialization decodes to an equal state.
#[test]
fn two_player_state_decodes_equal() {
    let s = &corpus()[1];
    let d = GameState::deserialize(&s.serialize().unwrap()).unwrap();
    assert_eq!(&d, s);
    assert_eq!(d.player_count(), 2);
}

// R12, REQ-CORE-001: empty input.
#[test]
fn empty_input_is_decode_error() {
    assert!(is_decode_err(&GameState::deserialize(&[])));
}

// R12, REQ-CORE-001: every strict prefix fails.
#[test]
fn every_strict_prefix_is_decode_error() {
    for s in corpus() {
        let b = s.serialize().unwrap();
        for len in 0..b.len() {
            let r = GameState::deserialize(&b[..len]);
            assert!(is_decode_err(&r), "prefix {len} of {} bytes", b.len());
        }
    }
}

// R12, REQ-CORE-001: trailing bytes are rejected.
#[test]
fn trailing_byte_is_decode_error() {
    for s in corpus() {
        let b = s.serialize().unwrap();
        for extra in [0u8, 1, 0xC0, 0xFF] {
            let mut c = b.clone();
            c.push(extra);
            assert!(is_decode_err(&GameState::deserialize(&c)));
        }
    }
}

// R12, REQ-CORE-001: single-byte flips never panic; Ok results are self-consistent.
#[test]
fn single_byte_flips_never_panic() {
    let states = corpus();
    let bytes: Vec<Vec<u8>> = states.iter().map(|s| s.serialize().unwrap()).collect();
    let mut rng = Rng::new(0xF11B_0001);
    let mut errs = 0u32;
    for _ in 0..5_000 {
        let k = rng.below(bytes.len() as u64) as usize;
        let mut b = bytes[k].clone();
        let pos = rng.below(b.len() as u64) as usize;
        b[pos] ^= 1 + rng.below(255) as u8;
        match GameState::deserialize(&b) {
            Err(e) => {
                assert!(matches!(e, StateError::Decode(_)), "got {e:?}");
                errs += 1;
            }
            Ok(t) => {
                let b2 = t.serialize().unwrap();
                let t2 = GameState::deserialize(&b2).unwrap();
                assert_eq!(t2, t);
                assert_eq!(t2.state_hash(), t.state_hash());
                assert_eq!(t2.serialize().unwrap(), b2);
            }
        }
    }
    assert!(errs > 0, "at least some flips must be rejected");
}

// R12: bounded random bytes.
#[test]
fn random_buffers_are_decode_errors() {
    let mut rng = Rng::new(0xBAD_B0F5);
    for _ in 0..5_000 {
        let len = rng.below(513) as usize;
        let buf: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        assert!(is_decode_err(&GameState::deserialize(&buf)), "{buf:?}");
    }
}

// R12: fixed pathological buffers, including huge length claims.
#[test]
fn pathological_buffers_are_decode_errors() {
    for fill in [0x00u8, 0xFF, 0xC1, 0xC0, 0x80, 0x90] {
        for len in 1..=64usize {
            let buf = vec![fill; len];
            assert!(
                is_decode_err(&GameState::deserialize(&buf)),
                "fill {fill:#x} len {len}"
            );
        }
    }
    // array32 / map32 / str32 / bin32 claiming ~4 billion elements.
    for tag in [0xDDu8, 0xDF, 0xDB, 0xC6] {
        for tail in [0usize, 1, 16, 300] {
            let mut buf = vec![tag, 0xFF, 0xFF, 0xFF, 0xFF];
            buf.extend(std::iter::repeat_n(0xA1u8, tail));
            assert!(
                is_decode_err(&GameState::deserialize(&buf)),
                "tag {tag:#x} tail {tail}"
            );
        }
    }
    // A valid-looking map header with a huge nested claim.
    let valid = GameState::new(1, 1, 20).unwrap().serialize().unwrap();
    let mut buf = valid.clone();
    buf.truncate(valid.len() / 2);
    buf.extend([0xDD, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert!(is_decode_err(&GameState::deserialize(&buf)));
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

// R12, REQ-CORE-001: a changed format_version is rejected.
#[test]
fn wrong_format_version_is_decode_error() {
    let b = GameState::new(42, 2, 20).unwrap().serialize().unwrap();
    let name = b"format_version";
    let at = find(&b, name).expect("named-field MessagePack keeps `format_version`");
    let vpos = at + name.len();
    assert_eq!(b[vpos], 0x01, "format_version 1 is a positive fixint");
    for bad in [0x02u8, 0x00, 0x7F] {
        let mut c = b.clone();
        c[vpos] = bad;
        assert!(
            is_decode_err(&GameState::deserialize(&c)),
            "version byte {bad:#x}"
        );
    }
    // Widened encodings of other versions: uint8 form of 2.
    let mut c = b[..vpos].to_vec();
    c.extend([0xCC, 0x02]);
    c.extend(&b[vpos + 1..]);
    assert!(is_decode_err(&GameState::deserialize(&c)));
}

// R12, REQ-CORE-001: a changed PRNG algorithm name is rejected.
#[test]
fn wrong_prng_algorithm_is_decode_error() {
    let b = GameState::new(42, 2, 20).unwrap().serialize().unwrap();
    assert!(find(&b, b"prng_algorithm").is_some());
    let name = b"PCG64-DXSM";
    let at = find(&b, name).expect("the algorithm name appears as a string");
    assert_eq!(b[at - 1], 0xA0 | 10, "fixstr header of length 10");
    // Same length, one character changed.
    let mut c = b.clone();
    c[at + 9] = b'N';
    assert!(is_decode_err(&GameState::deserialize(&c)));
    // Different length.
    let mut c = b[..at - 1].to_vec();
    c.push(0xA0 | 11);
    c.extend(b"PCG64-DXSMX");
    c.extend(&b[at + 10..]);
    assert!(is_decode_err(&GameState::deserialize(&c)));
    // Empty name.
    let mut c = b[..at - 1].to_vec();
    c.push(0xA0);
    c.extend(&b[at + 10..]);
    assert!(is_decode_err(&GameState::deserialize(&c)));
}
