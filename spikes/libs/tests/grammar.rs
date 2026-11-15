// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! T5 (REQ-NFR-005): synthetic pest grammar accepts, rejects, and survives random input (R12).

use fineprint_spike_libs::grammar::count_clauses;
use fineprint_spike_libs::prng::Pcg64Dxsm;

#[test]
fn req_nfr_005_pest_grammar_accepts_valid_sentences() {
    assert_eq!(count_clauses("move 3 marbles."), Some(1));
    assert_eq!(count_clauses("count 1 stone to the left bowl."), Some(1));
    assert_eq!(
        count_clauses(
            "move 3 marbles to the right bowl, then stack 12 stones, then count 1 marble."
        ),
        Some(3)
    );
}

#[test]
fn req_nfr_005_pest_grammar_rejects_invalid_sentences() {
    for bad in [
        "",
        "move marbles.",
        "move 3 marbles",
        "throw 3 marbles.",
        "move 3 marbles to the middle bowl.",
        "move 3 marbles, count 1 stone.",
        "move 3 marbles. extra",
    ] {
        assert_eq!(count_clauses(bad), None, "{bad:?}");
    }
}

/// R12: bounded random strings (grammar words, digits, punctuation, arbitrary chars) never panic.
#[test]
fn req_nfr_005_pest_grammar_random_input_never_panics() {
    const WORDS: [&str; 14] = [
        "move ", "count ", "3 ", "12", "marbles", "stone ", "to the ", "left ", "bowl", ",",
        " then ", ".", " ", "é",
    ];
    let mut rng = Pcg64Dxsm::new(90, 12);
    let mut accepted = 0;
    for _ in 0..4000 {
        let input: String = (0..rng.next_u64() % 16)
            .map(|_| {
                let pick = rng.next_u64();
                if pick.is_multiple_of(4) {
                    char::from_u32((pick >> 8) as u32 % 0x11_0000)
                        .unwrap_or('?')
                        .to_string()
                } else {
                    WORDS[(pick % 14) as usize].to_string()
                }
            })
            .collect();
        if count_clauses(&input).is_some() {
            accepted += 1;
        }
    }
    assert!(accepted < 4000);
}
