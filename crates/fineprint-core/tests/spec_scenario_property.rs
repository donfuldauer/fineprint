// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Deterministic no-panic property tests of `fineprint_core::scenario::parse_scenario`
//! (REQ-TEST-001 item 1, R12): random bytes and mutations of every valid sample, from a seeded
//! xorshift generator (no `rand`, no clock).
//! Black-box: written from the work-item brief (FP-049) only.

#[path = "spec_scenario_support.rs"]
mod support;

use std::panic::{AssertUnwindSafe, catch_unwind};

use fineprint_core::scenario::*;
use support::*;

fn hex64(input: &[u8]) -> String {
    input
        .iter()
        .take(64)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses one case under `catch_unwind` and checks the result invariants. Returns whether the
/// input parsed.
fn check_case(label: &str, input: &[u8]) -> bool {
    let r = catch_unwind(AssertUnwindSafe(|| parse_scenario(input)));
    let r = match r {
        Ok(r) => r,
        Err(_) => panic!(
            "parse_scenario panicked on case {label} (len {}): {}",
            input.len(),
            hex64(input)
        ),
    };
    match r {
        Err(e) => {
            assert!(
                !e.message.is_empty(),
                "case {label}: empty message: {}",
                hex64(input)
            );
            assert!(
                e.path.is_empty() || e.path.starts_with('/'),
                "case {label}: path {:?}: {}",
                e.path,
                hex64(input)
            );
            false
        }
        Ok(s) => {
            let bytes = serde_json::to_vec(&s).expect("a parsed scenario serialises");
            let again = parse_scenario(&bytes).unwrap_or_else(|e| {
                panic!("case {label}: re-serialised scenario fails to parse: {e}")
            });
            assert_eq!(again, s, "case {label}: round trip changes the scenario");
            true
        }
    }
}

/// REQ-TEST-001.1, R12: at least 20,000 deterministic cases (random bytes of length 0..4096 and
/// mutations of each valid sample: every prefix of the smaller samples, byte flips, chunk
/// deletion/duplication, byte swaps) never panic; errors are located; successes round-trip.
#[test]
fn req_test_001_property_no_panic_fuzz() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut cases = 0usize;

    // Pure random bytes, plus random bytes biased towards JSON punctuation.
    for i in 0..6_000 {
        let len = rng.below(4096);
        let mut buf = Vec::with_capacity(len);
        let json_bias = i % 2 == 1;
        const ALPHABET: &[u8] = b"{}[]\":,0123456789-.eE tfnrul\\\n";
        for _ in 0..len {
            if json_bias {
                buf.push(ALPHABET[rng.below(ALPHABET.len())]);
            } else {
                buf.push(rng.next_u64() as u8);
            }
        }
        check_case(&format!("random#{i}"), &buf);
        cases += 1;
    }

    let samples: Vec<(&str, Vec<u8>)> = SAMPLES.iter().map(|n| (*n, sample_bytes(n))).collect();
    let mut by_size: Vec<&(&str, Vec<u8>)> = samples.iter().collect();
    by_size.sort_by_key(|(n, b)| (b.len(), *n));

    // Every prefix of the three smallest samples.
    for (name, bytes) in by_size.iter().take(3) {
        for cut in 0..=bytes.len() {
            check_case(&format!("{name}[..{cut}]"), &bytes[..cut]);
            cases += 1;
        }
    }

    // Random mutations of every sample.
    for (name, bytes) in &samples {
        for j in 0..1_000 {
            let mut m = bytes.clone();
            let ops = 1 + rng.below(3);
            for _ in 0..ops {
                if m.is_empty() {
                    break;
                }
                match rng.below(4) {
                    0 => {
                        let at = rng.below(m.len());
                        m[at] ^= 1 << rng.below(8);
                    }
                    1 => {
                        let a = rng.below(m.len());
                        let b = (a + 1 + rng.below(64)).min(m.len());
                        m.drain(a..b);
                    }
                    2 => {
                        let a = rng.below(m.len());
                        let b = (a + 1 + rng.below(64)).min(m.len());
                        let chunk = m[a..b].to_vec();
                        let at = rng.below(m.len() + 1);
                        m.splice(at..at, chunk);
                    }
                    _ => {
                        let a = rng.below(m.len());
                        let b = rng.below(m.len());
                        m.swap(a, b);
                    }
                }
            }
            check_case(&format!("{name}~{j}"), &m);
            cases += 1;
        }
    }
    assert!(cases >= 20_000, "only {cases} cases");
}

/// REQ-TEST-001.1, R12: every prefix-truncation of every valid sample either fails to parse or
/// parses to exactly the full scenario (only trailing whitespace was cut); never `Ok` with
/// different content.
#[test]
fn req_test_001_property_prefix_truncation_never_ok_with_other_content() {
    for name in SAMPLES {
        let bytes = sample_bytes(name);
        let full = parse_ok(&bytes);
        for cut in 0..bytes.len() {
            let prefix = &bytes[..cut];
            let r = catch_unwind(AssertUnwindSafe(|| parse_scenario(prefix)))
                .unwrap_or_else(|_| panic!("{name}[..{cut}] panicked: {}", hex64(prefix)));
            if let Ok(s) = r {
                assert_eq!(s, full, "{name}[..{cut}] parsed to different content");
                assert!(
                    bytes[cut..].iter().all(|b| b.is_ascii_whitespace()),
                    "{name}[..{cut}] parsed although non-whitespace was cut"
                );
            }
        }
    }
}

/// REQ-TEST-001.1, R3: parsing is deterministic: the same bytes give equal results twice
/// (scenario or error, compared through `Debug`).
#[test]
fn req_test_001_property_parse_deterministic() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for name in SAMPLES {
        let bytes = sample_bytes(name);
        for _ in 0..50 {
            let mut m = bytes.clone();
            let at = rng.below(m.len());
            m[at] = rng.next_u64() as u8;
            let a = format!("{:?}", parse_scenario(&m));
            let b = format!("{:?}", parse_scenario(&m));
            assert_eq!(a, b, "{name}");
        }
    }
}
