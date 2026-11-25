// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for colorless-hybrid mana symbols `{C/W}`..`{C/G}` in `fineprint_dsl::mana`
//! (REQ-DSL-002, CR 107.4, CR 107.4e). Black-box: written from the work-item brief only.

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaParseError, ManaSymbol};

const COLORS: [(Color, char); 5] = [
    (Color::White, 'W'),
    (Color::Blue, 'U'),
    (Color::Black, 'B'),
    (Color::Red, 'R'),
    (Color::Green, 'G'),
];

/// REQ-DSL-002, CR 107.4: each `{C/x}` parses, is valid, prints canonically and round-trips.
#[test]
fn req_dsl_002_cr_107_4_colorless_hybrid_parses() {
    for (c, l) in COLORS {
        let text = format!("{{C/{l}}}");
        let s = ManaSymbol::parse(&text).expect("parses");
        assert_eq!(s, ManaSymbol::ColorlessHybrid(c), "{text}");
        assert!(s.is_valid(), "{text}");
        assert_eq!(s.to_string(), text);
        assert_eq!(ManaSymbol::parse(&s.to_string()), Ok(s));
        assert_eq!(text.parse::<ManaSymbol>().ok(), Some(s));
    }
}

/// REQ-DSL-002, CR 107.4: costs holding colorless-hybrid symbols display byte-identically.
#[test]
fn req_dsl_002_cr_107_4_colorless_hybrid_costs_round_trip() {
    for text in [
        "{C/W}",
        "{C/W}{C/W}",
        "{C/W}{1}",
        "{2}{C/G}{U}",
        "{C/W}{C/U}{C/B}{C/R}{C/G}",
        "{X}{C/R}{C/R}",
    ] {
        let c = ManaCostDSL::parse(text).expect("parses");
        assert_eq!(c.to_string(), text);
        assert_eq!(ManaCostDSL::parse(&c.to_string()), Ok(c.clone()));
        assert!(
            c.symbols()
                .iter()
                .any(|s| matches!(s, ManaSymbol::ColorlessHybrid(_))),
            "{text}"
        );
    }
}

/// REQ-DSL-002, CR 107.4: serde form is the canonical string and round-trips.
#[test]
fn req_dsl_002_cr_107_4_colorless_hybrid_serde() {
    for text in ["{C/W}{1}", "{2}{C/G}{U}", "{C/W}{C/U}{C/B}{C/R}{C/G}"] {
        let c = ManaCostDSL::parse(text).unwrap();
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<ManaCostDSL>(&json).unwrap(), c);
    }
}

/// REQ-DSL-002, CR 107.4: look-alike text is an `UnknownSymbol` at the `{`, in a symbol, a
/// cost and serde; never Ok.
#[test]
fn req_dsl_002_cr_107_4_lookalikes_rejected() {
    let bad = [
        "{W/C}", "{C/P}", "{C/C}", "{C/S}", "{C/X}", "{C/2}", "{c/W}", "{C/w}", "{c/w}", "{C/ W}",
        "{ C/W}", "{C/W }", "{C/}", "{/W}", "{C/W/P}", "{C/W/U}",
    ];
    for s in bad {
        assert!(ManaSymbol::parse(s).is_err(), "symbol {s:?}");
        for (text, offset) in [(s.to_string(), 0), (format!("{{1}}{s}"), 3)] {
            match ManaCostDSL::parse(&text) {
                Err(ManaParseError::UnknownSymbol { offset: o, symbol }) => {
                    assert_eq!(o, offset, "{text:?}");
                    assert_eq!(symbol, s, "{text:?}");
                }
                other => panic!("{text:?}: expected UnknownSymbol, got {other:?}"),
            }
            assert!(
                serde_json::from_str::<ManaCostDSL>(&format!("\"{text}\"")).is_err(),
                "serde {text:?}"
            );
        }
    }
}

/// REQ-DSL-002, CR 107.4: neighbouring symbol classes are unchanged and `{C}` stays colorless.
#[test]
fn req_dsl_002_cr_107_4_neighbours_unchanged() {
    assert_eq!(ManaSymbol::parse("{C}"), Ok(ManaSymbol::Colorless));
    assert_eq!(
        ManaSymbol::parse("{2/W}"),
        Ok(ManaSymbol::GenericHybrid(2, Color::White))
    );
    assert_eq!(
        ManaSymbol::parse("{W/U}"),
        Ok(ManaSymbol::Hybrid(Color::White, Color::Blue))
    );
    assert_eq!(
        ManaSymbol::parse("{W/P}"),
        Ok(ManaSymbol::Phyrexian(Color::White))
    );
    assert_eq!(
        ManaSymbol::parse("{G/U/P}"),
        Ok(ManaSymbol::HybridPhyrexian(Color::Green, Color::Blue))
    );
}

/// REQ-DSL-002, CR 107.4, R12: bounded deterministic random strings never panic, and any Ok
/// result displays text that re-parses to the same value.
#[test]
fn req_dsl_002_cr_107_4_random_strings_canonical_round_trip() {
    const ALPHABET: &str = "{}/CWUBRGPSXYZ0123456789abcdefghijklmnopqrstuvwxyz ";
    let alpha: Vec<char> = ALPHABET.chars().collect();
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0169_C0DE_0000_0001);
    let mut ok_with_new = 0u32;
    for case in 0..6000u32 {
        let mut text = String::new();
        let len = rng.next_u64() % 4;
        for _ in 0..len {
            match rng.next_u64() % 3 {
                0 => {
                    let l = COLORS[(rng.next_u64() % 5) as usize].1;
                    text.push_str(&format!("{{C/{l}}}"));
                }
                1 => {
                    text.push('{');
                    for _ in 0..(rng.next_u64() % 5) {
                        text.push(alpha[(rng.next_u64() % alpha.len() as u64) as usize]);
                    }
                    text.push('}');
                }
                _ => {
                    for _ in 0..(rng.next_u64() % 4) {
                        text.push(alpha[(rng.next_u64() % alpha.len() as u64) as usize]);
                    }
                }
            }
        }
        if let Ok(c) = ManaCostDSL::parse(&text) {
            let shown = c.to_string();
            assert_eq!(ManaCostDSL::parse(&shown), Ok(c.clone()), "case {case}");
            if c.symbols()
                .iter()
                .any(|s| matches!(s, ManaSymbol::ColorlessHybrid(_)))
            {
                ok_with_new += 1;
            }
        }
    }
    assert!(ok_with_new > 100, "generator must emit {{C/x}} forms");
}
