// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for mana symbols and costs (REQ-OBJ-001, SRS §4.1).

use fineprint_base::prng::Pcg64Dxsm;

use super::*;

/// Every symbol class with its canonical text.
fn samples() -> Vec<(ManaSymbol, &'static str)> {
    use Color::*;
    vec![
        (ManaSymbol::Generic(0), "{0}"),
        (ManaSymbol::Generic(1), "{1}"),
        (ManaSymbol::Generic(10), "{10}"),
        (ManaSymbol::Generic(MAX_GENERIC), "{1000000}"),
        (ManaSymbol::Colored(White), "{W}"),
        (ManaSymbol::Colored(Blue), "{U}"),
        (ManaSymbol::Colored(Black), "{B}"),
        (ManaSymbol::Colored(Red), "{R}"),
        (ManaSymbol::Colored(Green), "{G}"),
        (ManaSymbol::Colorless, "{C}"),
        (ManaSymbol::Snow, "{S}"),
        (ManaSymbol::Variable(Variable::X), "{X}"),
        (ManaSymbol::Variable(Variable::Y), "{Y}"),
        (ManaSymbol::Variable(Variable::Z), "{Z}"),
        (ManaSymbol::Hybrid(White, Blue), "{W/U}"),
        (ManaSymbol::Hybrid(Blue, White), "{U/W}"),
        (ManaSymbol::Hybrid(Red, Green), "{R/G}"),
        (ManaSymbol::GenericHybrid(2, White), "{2/W}"),
        (ManaSymbol::GenericHybrid(1, Green), "{1/G}"),
        (ManaSymbol::GenericHybrid(MAX_GENERIC, Black), "{1000000/B}"),
        (ManaSymbol::Phyrexian(Black), "{B/P}"),
        (ManaSymbol::Phyrexian(White), "{W/P}"),
        (ManaSymbol::HybridPhyrexian(Green, Blue), "{G/U/P}"),
        (ManaSymbol::HybridPhyrexian(Blue, Green), "{U/G/P}"),
    ]
}

/// REQ-OBJ-001: every symbol class of SRS §4.1 parses and prints its canonical text.
#[test]
fn req_obj_001_every_symbol_class_round_trips() {
    for (symbol, text) in samples() {
        assert_eq!(ManaSymbol::parse(text), Ok(symbol), "{text}");
        assert_eq!(text.parse::<ManaSymbol>(), Ok(symbol), "{text}");
        assert_eq!(symbol.to_string(), text);
    }
    assert_ne!(ManaSymbol::parse("{W/U}"), ManaSymbol::parse("{U/W}"));
}

/// REQ-OBJ-001: all ordered pairs of distinct colors are hybrid and hybrid Phyrexian symbols;
/// equal pairs are rejected.
#[test]
fn req_obj_001_all_color_pairs() {
    for a in Color::ALL {
        assert_eq!(Color::from_letter(a.letter()), Some(a));
        assert_eq!(Color::from_letter(a.letter().to_ascii_lowercase()), None);
        for b in Color::ALL {
            let hybrid = format!("{{{}/{}}}", a.letter(), b.letter());
            let phyrexian = format!("{{{}/{}/P}}", a.letter(), b.letter());
            if a == b {
                assert!(matches!(
                    ManaSymbol::parse(&hybrid),
                    Err(ManaParseError::UnknownSymbol { offset: 0, .. })
                ));
                assert!(ManaSymbol::parse(&phyrexian).is_err());
            } else {
                assert_eq!(ManaSymbol::parse(&hybrid), Ok(ManaSymbol::Hybrid(a, b)));
                assert_eq!(
                    ManaSymbol::parse(&phyrexian),
                    Ok(ManaSymbol::HybridPhyrexian(a, b))
                );
            }
        }
    }
    for v in Variable::ALL {
        assert_eq!(
            ManaSymbol::parse(&format!("{{{}}}", v.letter())),
            Ok(ManaSymbol::Variable(v))
        );
    }
}

/// REQ-OBJ-001: every rejection example of the brief, as UnknownSymbol at the `{`.
#[test]
fn req_obj_001_unknown_symbols_rejected() {
    for text in [
        "{w}",
        "{u/w}",
        "{x}",
        "{ W}",
        "{W }",
        "{W/W}",
        "{C/W}",
        "{W/C}",
        "{P}",
        "{2/P}",
        "{1000001}",
        "{01}",
        "{00}",
        "{+1}",
        "{-1}",
        "{0/W}",
        "{01/W}",
        "{1000001/W}",
        "{W/U/G}",
        "{W/W/P}",
        "{W/P/P}",
        "{2/W/P}",
        "{C/P}",
        "{S/P}",
        "{X/P}",
        "{H}",
        "{1/2}",
        "{½}",
        "{∞}",
        "{E}",
        "{T}",
        "{WU}",
        "{/}",
        "{W/}",
        "{/W}",
        "{99999999999999999999}",
    ] {
        assert_eq!(
            ManaSymbol::parse(text),
            Err(ManaParseError::UnknownSymbol {
                offset: 0,
                symbol: text.to_owned()
            }),
            "{text}"
        );
    }
    assert_eq!(
        ManaCostDSL::parse("{2}{W/W}"),
        Err(ManaParseError::UnknownSymbol {
            offset: 3,
            symbol: "{W/W}".to_owned()
        })
    );
}

/// REQ-OBJ-001: bracket-structure errors carry the byte offset of the first offending byte.
#[test]
fn req_obj_001_malformed_offsets() {
    let cases = [
        ("{}", 0),
        ("W", 0),
        ("}", 0),
        ("{W", 0),
        ("{W}}", 3),
        ("{W}x", 3),
        (" {W}", 0),
        ("{W} ", 3),
        ("{W{U}}", 2),
        ("{2}{", 3),
        ("{2}{}", 3),
        ("é{W}", 0),
        ("{W}é", 3),
    ];
    for (text, offset) in cases {
        assert_eq!(
            ManaCostDSL::parse(text),
            Err(ManaParseError::Malformed { offset }),
            "{text}"
        );
    }
    // A single-symbol parse rejects trailing text, including a second symbol.
    assert_eq!(
        ManaSymbol::parse("{W}{U}"),
        Err(ManaParseError::Malformed { offset: 3 })
    );
    assert_eq!(
        ManaSymbol::parse("{W"),
        Err(ManaParseError::Malformed { offset: 0 })
    );
    assert_eq!(ManaSymbol::parse(""), Err(ManaParseError::Empty));
}

/// REQ-OBJ-001: costs keep symbol order and print canonically.
#[test]
fn req_obj_001_cost_parse_and_print() {
    let cost = ManaCostDSL::parse("{2}{W}{W}").expect("cost");
    assert_eq!(
        cost.symbols(),
        &[
            ManaSymbol::Generic(2),
            ManaSymbol::Colored(Color::White),
            ManaSymbol::Colored(Color::White)
        ]
    );
    assert_eq!(cost.to_string(), "{2}{W}{W}");
    let reordered = ManaCostDSL::parse("{W}{2}{W}").expect("cost");
    assert_ne!(cost, reordered, "nothing is sorted or merged");
    assert_eq!(
        "{X}{G/U/P}{1/R}"
            .parse::<ManaCostDSL>()
            .expect("ok")
            .to_string(),
        "{X}{G/U/P}{1/R}"
    );

    let built = ManaCostDSL::new(vec![ManaSymbol::Snow, ManaSymbol::Colorless]).expect("non-empty");
    assert_eq!(built.to_string(), "{S}{C}");
    assert_eq!(ManaCostDSL::new(Vec::new()), Err(ManaParseError::Empty));
}

/// REQ-OBJ-001: "no mana cost" (empty text, `None`) is different from `{0}`.
#[test]
fn req_obj_001_no_mana_cost_distinct_from_zero() {
    assert_eq!(ManaCostDSL::parse(""), Err(ManaParseError::Empty));
    assert_eq!(ManaCostDSL::parse_optional(""), Ok(None));
    let zero = ManaCostDSL::parse_optional("{0}")
        .expect("ok")
        .expect("some");
    assert_eq!(zero.symbols(), &[ManaSymbol::Generic(0)]);
    assert_eq!(ManaCostDSL::print_optional(None), "");
    assert_eq!(ManaCostDSL::print_optional(Some(&zero)), "{0}");
    assert!(ManaCostDSL::parse_optional("{0").is_err());
    assert_eq!(
        ManaCostDSL::parse_optional("{1}{U}")
            .expect("ok")
            .as_ref()
            .map(ToString::to_string),
        Some("{1}{U}".to_owned())
    );
}

/// REQ-OBJ-001: the JSON form is the canonical string; empty and invalid strings are rejected.
#[test]
fn req_obj_001_json_form() {
    let cost = ManaCostDSL::parse("{2}{W}{W}").expect("cost");
    assert_eq!(serde_json::to_string(&cost).expect("ser"), "\"{2}{W}{W}\"");
    let back: ManaCostDSL = serde_json::from_str("\"{2}{W}{W}\"").expect("de");
    assert_eq!(back, cost);
    let zero: ManaCostDSL = serde_json::from_str("\"{0}\"").expect("de");
    assert_eq!(serde_json::to_string(&zero).expect("ser"), "\"{0}\"");
    for bad in ["\"\"", "\"{w}\"", "\"{2}{\"", "2", "null", "[\"{W}\"]"] {
        assert!(serde_json::from_str::<ManaCostDSL>(bad).is_err(), "{bad}");
    }
}

/// REQ-OBJ-001: error messages name the problem.
#[test]
fn req_obj_001_error_display() {
    assert!(ManaParseError::Empty.to_string().contains("empty"));
    assert!(
        ManaParseError::Malformed { offset: 7 }
            .to_string()
            .contains('7')
    );
    let unknown = ManaParseError::UnknownSymbol {
        offset: 4,
        symbol: "{Q}".into(),
    };
    let text = unknown.to_string();
    assert!(text.contains("{Q}") && text.contains('4'), "{text}");
    let _: &dyn std::error::Error = &unknown;
}

/// REQ-OBJ-001 property: random costs built from symbols print to text that parses back to the
/// same cost, and `print(parse(s)) == s`.
#[test]
fn req_obj_001_property_print_parse_round_trip() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x000b_1ec7_0001);
    let pool = samples();
    for _ in 0..2000 {
        let len = 1 + (rng.next_u64() % 8) as usize;
        let symbols: Vec<ManaSymbol> = (0..len)
            .map(|_| {
                let r = rng.next_u64();
                match r % 4 {
                    0 => ManaSymbol::Generic(((r >> 8) % u64::from(MAX_GENERIC + 1)) as u32),
                    1 => ManaSymbol::GenericHybrid(
                        1 + ((r >> 8) % u64::from(MAX_GENERIC)) as u32,
                        Color::ALL[((r >> 40) % 5) as usize],
                    ),
                    _ => pool[((r >> 8) % pool.len() as u64) as usize].0,
                }
            })
            .collect();
        let cost = ManaCostDSL::new(symbols).expect("non-empty");
        let text = cost.to_string();
        let parsed = ManaCostDSL::parse(&text).expect("canonical text parses");
        assert_eq!(parsed, cost);
        assert_eq!(parsed.to_string(), text);
        let json = serde_json::to_string(&cost).expect("ser");
        assert_eq!(
            serde_json::from_str::<ManaCostDSL>(&json).expect("de"),
            cost
        );
    }
}

/// REQ-OBJ-001, R12: bounded random strings never panic; anything accepted re-prints exactly.
#[test]
fn req_obj_001_random_strings_never_panic() {
    const ALPHABET: &[&str] = &[
        "{", "}", "/", "W", "U", "B", "R", "G", "C", "S", "X", "P", "0", "1", "2", "9", "w", " ",
        "é", "😂",
    ];
    let mut rng = Pcg64Dxsm::from_u64_seed(0x000b_1ec7_0002);
    let mut accepted = 0u32;
    for _ in 0..4000 {
        let len = (rng.next_u64() % 16) as usize;
        let text: String = (0..len)
            .map(|_| ALPHABET[(rng.next_u64() % ALPHABET.len() as u64) as usize])
            .collect();
        if let Ok(cost) = ManaCostDSL::parse(&text) {
            accepted += 1;
            assert_eq!(cost.to_string(), text);
        }
        if let Ok(symbol) = ManaSymbol::parse(&text) {
            assert_eq!(symbol.to_string(), text);
        }
        let _ = ManaCostDSL::parse_optional(&text);
        // Raw bytes, lossily decoded, must not panic either.
        let raw: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
        let lossy = String::from_utf8_lossy(&raw);
        let _ = ManaCostDSL::parse(&lossy);
        let _ = ManaSymbol::parse(&lossy);
    }
    assert!(accepted > 0);
}

/// REQ-OBJ-001: `ManaCostDSL::new` applies the parser's rules and reports the first invalid
/// symbol as `UnknownSymbol` with its offset in the canonical text.
#[test]
fn req_obj_001_new_validates_symbols() {
    use Color::*;
    for bad in [
        ManaSymbol::GenericHybrid(0, White),
        ManaSymbol::Generic(MAX_GENERIC + 1),
        ManaSymbol::GenericHybrid(MAX_GENERIC + 1, Blue),
        ManaSymbol::Hybrid(Red, Red),
        ManaSymbol::HybridPhyrexian(Green, Green),
    ] {
        assert!(!bad.is_valid(), "{bad}");
        assert_eq!(
            ManaCostDSL::new(vec![bad]),
            Err(ManaParseError::UnknownSymbol {
                offset: 0,
                symbol: bad.to_string()
            })
        );
    }
    assert_eq!(
        ManaCostDSL::new(vec![
            ManaSymbol::Generic(2),
            ManaSymbol::GenericHybrid(0, White)
        ]),
        Err(ManaParseError::UnknownSymbol {
            offset: 3,
            symbol: "{0/W}".into()
        })
    );
    assert_eq!(
        ManaCostDSL::new(vec![
            ManaSymbol::Colored(Blue),
            ManaSymbol::Hybrid(White, Blue),
            ManaSymbol::Hybrid(Black, Black),
            ManaSymbol::Generic(MAX_GENERIC + 1),
        ]),
        Err(ManaParseError::UnknownSymbol {
            offset: 8,
            symbol: "{B/B}".into()
        })
    );
    assert_eq!(ManaCostDSL::new(Vec::new()), Err(ManaParseError::Empty));
    for good in [
        ManaSymbol::Generic(0),
        ManaSymbol::GenericHybrid(1, White),
        ManaSymbol::Generic(MAX_GENERIC),
        ManaSymbol::GenericHybrid(MAX_GENERIC, Green),
    ] {
        let cost = ManaCostDSL::new(vec![good]).expect("valid");
        let text = cost.to_string();
        assert_eq!(ManaCostDSL::parse(&text), Ok(cost.clone()), "{text}");
        let json = serde_json::to_value(&cost).expect("ser");
        assert_eq!(
            serde_json::from_value::<ManaCostDSL>(json).expect("de"),
            cost
        );
    }
}

/// REQ-OBJ-001: the parser and the constructor cannot disagree: for a spread of symbols,
/// parsing the printed symbol succeeds exactly when `new` accepts it, with the same symbol.
#[test]
fn req_obj_001_new_agrees_with_parser() {
    let amounts = [
        0u32,
        1,
        2,
        9,
        10,
        999_999,
        MAX_GENERIC,
        MAX_GENERIC + 1,
        u32::MAX,
    ];
    let mut symbols = vec![
        ManaSymbol::Colorless,
        ManaSymbol::Snow,
        ManaSymbol::Variable(Variable::X),
    ];
    for &n in &amounts {
        symbols.push(ManaSymbol::Generic(n));
        for c in Color::ALL {
            symbols.push(ManaSymbol::GenericHybrid(n, c));
        }
    }
    for a in Color::ALL {
        symbols.push(ManaSymbol::Colored(a));
        symbols.push(ManaSymbol::Phyrexian(a));
        for b in Color::ALL {
            symbols.push(ManaSymbol::Hybrid(a, b));
            symbols.push(ManaSymbol::HybridPhyrexian(a, b));
        }
    }
    let mut valid = 0;
    for symbol in symbols {
        let parsed = ManaSymbol::parse(&symbol.to_string());
        let built = ManaCostDSL::new(vec![symbol]);
        assert_eq!(parsed.is_ok(), built.is_ok(), "{symbol}");
        assert_eq!(parsed.is_ok(), symbol.is_valid(), "{symbol}");
        if let Ok(p) = parsed {
            valid += 1;
            assert_eq!(p, symbol);
        }
    }
    assert!(valid > 0);
}
