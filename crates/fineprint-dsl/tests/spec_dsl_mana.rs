// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::mana` (mana symbols and `ManaCostDSL`), REQ-OBJ-001
//! (SRS §4.1: every mana symbol class; "no mana cost" is different from `{0}`) and REQ-DSL-001
//! (the JSON form of `manaCost`). Black-box: written from the work-item brief only.

use std::collections::BTreeSet;

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::mana::{Color, MAX_GENERIC, ManaCostDSL, ManaParseError, ManaSymbol, Variable};

const COLORS: [(Color, char); 5] = [
    (Color::White, 'W'),
    (Color::Blue, 'U'),
    (Color::Black, 'B'),
    (Color::Red, 'R'),
    (Color::Green, 'G'),
];

fn sym(s: &str) -> ManaSymbol {
    match ManaSymbol::parse(s) {
        Ok(m) => m,
        Err(e) => panic!("ManaSymbol::parse({s:?}) failed: {e:?}"),
    }
}

fn cost(s: &str) -> ManaCostDSL {
    match ManaCostDSL::parse(s) {
        Ok(c) => c,
        Err(e) => panic!("ManaCostDSL::parse({s:?}) failed: {e:?}"),
    }
}

/// Parses `text` as a single symbol (all three entry points) and as a one-symbol cost, checks
/// the expected value and that it prints back to exactly `text`.
fn check_symbol(text: &str, expected: ManaSymbol) {
    assert_eq!(sym(text), expected, "{text}");
    assert_eq!(
        text.parse::<ManaSymbol>().ok(),
        Some(expected),
        "{text} FromStr"
    );
    assert_eq!(expected.to_string(), text, "Display of {expected:?}");
    let c = cost(text);
    assert_eq!(c.symbols(), &[expected][..], "{text}");
    assert_eq!(c.to_string(), text);
    assert_eq!(text.parse::<ManaCostDSL>().ok(), Some(c));
}

// ---------------------------------------------------------------- symbol classes

/// REQ-OBJ-001: Color letters and their inverse.
#[test]
fn req_obj_001_color_letters() {
    for (c, l) in COLORS {
        assert_eq!(c.letter(), l);
        assert_eq!(Color::from_letter(l), Some(c));
    }
    for l in ['w', 'C', 'S', 'X', 'P', '2', 'é', ' '] {
        assert_eq!(Color::from_letter(l), None, "{l}");
    }
    assert_eq!(MAX_GENERIC, 1_000_000);
}

/// REQ-OBJ-001: every mana symbol class in SRS §4.1 parses and prints back canonically
/// (acceptance: "every mana symbol class, no-mana-cost distinct from {0}").
#[test]
fn req_obj_001_every_mana_symbol_class() {
    // Generic.
    for n in [0u32, 1, 9, 10, 15, 16, 100, 999_999, 1_000_000] {
        check_symbol(&format!("{{{n}}}"), ManaSymbol::Generic(n));
    }
    // Colored and Phyrexian.
    for (c, l) in COLORS {
        check_symbol(&format!("{{{l}}}"), ManaSymbol::Colored(c));
        check_symbol(&format!("{{{l}/P}}"), ManaSymbol::Phyrexian(c));
    }
    // Colorless, Snow, Variables.
    check_symbol("{C}", ManaSymbol::Colorless);
    check_symbol("{S}", ManaSymbol::Snow);
    check_symbol("{X}", ManaSymbol::Variable(Variable::X));
    check_symbol("{Y}", ManaSymbol::Variable(Variable::Y));
    check_symbol("{Z}", ManaSymbol::Variable(Variable::Z));
    // Hybrid and hybrid Phyrexian: all 20 ordered pairs of distinct colors.
    let mut hybrids = BTreeSet::new();
    for (a, la) in COLORS {
        for (b, lb) in COLORS {
            if a == b {
                continue;
            }
            check_symbol(&format!("{{{la}/{lb}}}"), ManaSymbol::Hybrid(a, b));
            check_symbol(
                &format!("{{{la}/{lb}/P}}"),
                ManaSymbol::HybridPhyrexian(a, b),
            );
            hybrids.insert(ManaSymbol::Hybrid(a, b));
            hybrids.insert(ManaSymbol::HybridPhyrexian(a, b));
        }
    }
    assert_eq!(hybrids.len(), 40);
    // Order is significant: {W/U} and {U/W} are both valid and different.
    assert_ne!(sym("{W/U}"), sym("{U/W}"));
    assert_ne!(sym("{W/U/P}"), sym("{U/W/P}"));
    // Generic hybrid.
    for (c, l) in COLORS {
        check_symbol(&format!("{{2/{l}}}"), ManaSymbol::GenericHybrid(2, c));
        check_symbol(&format!("{{1/{l}}}"), ManaSymbol::GenericHybrid(1, c));
    }
    check_symbol("{10/G}", ManaSymbol::GenericHybrid(10, Color::Green));
    check_symbol(
        "{1000000/U}",
        ManaSymbol::GenericHybrid(1_000_000, Color::Blue),
    );
}

/// REQ-OBJ-001: every invalid spelling listed in the brief is rejected.
#[test]
fn req_obj_001_invalid_symbols_rejected() {
    let invalid = [
        "{w}",
        "{u}",
        "{c}",
        "{x}",
        "{w/u}",
        "{W/p}",
        "{ W}",
        "{W }",
        "{W/ U}",
        "{W/W}",
        "{G/G/P}",
        "{C/W}",
        "{W/C}",
        "{S/W}",
        "{X/W}",
        "{P}",
        "{2/P}",
        "{0/W}",
        "{00/W}",
        "{02/W}",
        "{1000001/W}",
        "{W/2}",
        "{1000001}",
        "{4294967296}",
        "{99999999999999999999999999}",
        "{01}",
        "{00}",
        "{+1}",
        "{-1}",
        "{1.0}",
        "{}",
        "{W/U/W/P}",
        "{W/U/G}",
        "{W/P/U}",
        "{P/W}",
        "{W/U/P/P}",
        "{W/}",
        "{/W}",
        "{/}",
        "{W//U}",
        "{WU}",
        "{2W}",
        "{H}",
        "{E}",
        "{T}",
        "{Q}",
        "{∞}",
        "{½}",
        "{1/2}",
        "{CHAOS}",
        "W",
        "2",
        "",
        " {W}",
        "{W} ",
        "{W}{U}",
        "{W",
        "W}",
        "{{W}}",
        "}",
        "{",
    ];
    for s in invalid {
        assert!(
            ManaSymbol::parse(s).is_err(),
            "ManaSymbol::parse({s:?}) must fail"
        );
        assert!(s.parse::<ManaSymbol>().is_err(), "FromStr {s:?}");
        if s != "{W}{U}" && !s.is_empty() {
            assert!(
                ManaCostDSL::parse(s).is_err(),
                "ManaCostDSL::parse({s:?}) must fail"
            );
            assert!(
                ManaCostDSL::parse(&format!("{{1}}{s}")).is_err(),
                "ManaCostDSL::parse(\"{{1}}{s}\") must fail"
            );
        }
    }
}

// ---------------------------------------------------------------- error kinds and offsets

fn err(s: &str) -> ManaParseError {
    match ManaCostDSL::parse(s) {
        Ok(c) => panic!("ManaCostDSL::parse({s:?}) unexpectedly Ok: {c:?}"),
        Err(e) => e,
    }
}

fn assert_display(e: &ManaParseError, input: &str) {
    let d = e.to_string();
    assert!(!d.is_empty());
    let _: &dyn std::error::Error = e;
    match e {
        ManaParseError::Empty => {}
        ManaParseError::Malformed { offset } => {
            let offending = input.get(*offset..).and_then(|t| t.chars().next());
            assert!(
                d.contains(&offset.to_string()) || offending.is_some_and(|c| d.contains(c)),
                "{d:?} should mention {offset} or the offending text of {input:?}"
            );
        }
        ManaParseError::UnknownSymbol { offset, symbol } => {
            assert!(
                d.contains(symbol.as_str()) || d.contains(&offset.to_string()),
                "{d:?} should mention {symbol:?} or {offset}"
            );
        }
    }
}

/// REQ-OBJ-001: the empty string is `Empty`, not a cost.
#[test]
fn req_obj_001_empty_is_error_kind_empty() {
    assert_eq!(ManaCostDSL::parse(""), Err(ManaParseError::Empty));
    assert_eq!("".parse::<ManaCostDSL>(), Err(ManaParseError::Empty));
    assert_eq!(ManaCostDSL::new(vec![]), Err(ManaParseError::Empty));
    assert_display(&ManaParseError::Empty, "");
}

/// REQ-OBJ-001: bracket-structure errors are `Malformed` with the offset of the first
/// offending byte.
#[test]
fn req_obj_001_malformed_offsets() {
    let cases: &[(&str, usize)] = &[
        ("W", 0),
        ("x{W}", 0),
        (" {W}", 0),
        ("{W}x", 3),
        ("{W} ", 3),
        ("{2}{W}W", 6),
        ("}", 0),
        ("{W}}", 3),
        ("{W}{U}}{B}", 6),
        ("{W{U}}", 2),
        ("{{W}}", 1),
        ("{W}é", 3),
        ("{1}あ{W}", 3),
    ];
    for (s, offset) in cases {
        let e = err(s);
        assert_eq!(e, ManaParseError::Malformed { offset: *offset }, "{s:?}");
        assert_display(&e, s);
    }
    // Unterminated `{` and `{}` are Malformed (offset not fixed by the brief).
    for s in ["{", "{W", "{W}{", "{W}{U", "{}", "{W}{}", "{}{W}"] {
        let e = err(s);
        assert!(
            matches!(e, ManaParseError::Malformed { .. }),
            "{s:?}: {e:?}"
        );
        assert_display(&e, s);
    }
}

/// REQ-OBJ-001: well-bracketed but unknown symbols are `UnknownSymbol` with the symbol text
/// and the byte offset of its `{`.
#[test]
fn req_obj_001_unknown_symbol_offsets() {
    let cases: &[(&str, usize, &str)] = &[
        ("{w}", 0, "w"),
        ("{Q}", 0, "Q"),
        ("{W}{Q}", 3, "Q"),
        ("{2}{W}{W/W}", 6, "W/W"),
        ("{X}{X}{0/W}", 6, "0/W"),
        ("{10}{1000001}", 4, "1000001"),
        ("{W}{ W}", 3, " W"),
        ("{∞}", 0, "∞"),
        ("{W}{½}", 3, "½"),
        ("{é}{W}", 0, "é"),
        ("{W}{あ}", 3, "あ"),
        ("{W/U/G}", 0, "W/U/G"),
        ("{1}{1/2}", 3, "1/2"),
    ];
    for (s, offset, inner) in cases {
        match err(s) {
            ManaParseError::UnknownSymbol {
                offset: got,
                symbol,
            } => {
                assert_eq!(got, *offset, "{s:?}");
                assert!(
                    symbol.contains(inner),
                    "{s:?}: symbol {symbol:?} lacks {inner:?}"
                );
                assert_display(
                    &ManaParseError::UnknownSymbol {
                        offset: got,
                        symbol,
                    },
                    s,
                );
            }
            other => panic!("{s:?}: expected UnknownSymbol, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------- costs

/// REQ-OBJ-001: multi-symbol costs keep order and repeats; nothing is sorted or merged.
#[test]
fn req_obj_001_multi_symbol_costs_preserve_order() {
    let c = cost("{2}{W}{W}");
    assert_eq!(
        c.symbols(),
        &[
            ManaSymbol::Generic(2),
            ManaSymbol::Colored(Color::White),
            ManaSymbol::Colored(Color::White)
        ][..]
    );
    let c = cost("{W}{2}");
    assert_eq!(
        c.symbols(),
        &[ManaSymbol::Colored(Color::White), ManaSymbol::Generic(2)][..]
    );
    assert_eq!(c.to_string(), "{W}{2}");
    assert_ne!(cost("{W}{2}"), cost("{2}{W}"));
    let c = cost("{X}{X}{R/P}");
    assert_eq!(
        c.symbols(),
        &[
            ManaSymbol::Variable(Variable::X),
            ManaSymbol::Variable(Variable::X),
            ManaSymbol::Phyrexian(Color::Red)
        ][..]
    );
    let c = cost("{1}{1}");
    assert_eq!(c.symbols().len(), 2);
    assert_eq!(c.to_string(), "{1}{1}");
    let all = "{0}{16}{W}{U}{B}{R}{G}{C}{S}{X}{Y}{Z}{W/U}{U/W}{2/B}{R/P}{G/W/P}";
    let c = cost(all);
    assert_eq!(c.symbols().len(), 17);
    assert_eq!(c.to_string(), all);
    // new() keeps the given order.
    let v = vec![
        ManaSymbol::Snow,
        ManaSymbol::Generic(3),
        ManaSymbol::Snow,
        ManaSymbol::Hybrid(Color::Black, Color::Green),
    ];
    let c = ManaCostDSL::new(v.clone()).unwrap();
    assert_eq!(c.symbols(), &v[..]);
    assert_eq!(c.to_string(), "{S}{3}{S}{B/G}");
}

/// REQ-OBJ-001: "No mana cost" is different from {0} (SRS §4.1)
/// (acceptance: "every mana symbol class, no-mana-cost distinct from {0}").
#[test]
fn req_obj_001_no_mana_cost_distinct_from_zero() {
    assert_eq!(ManaCostDSL::parse(""), Err(ManaParseError::Empty));
    let none = ManaCostDSL::parse_optional("");
    assert_eq!(none, Ok(None));
    let zero = ManaCostDSL::parse_optional("{0}").unwrap();
    let c = zero.clone().expect("{0} is a mana cost");
    assert_eq!(c.symbols(), &[ManaSymbol::Generic(0)][..]);
    assert_ne!(zero, None);
    assert_ne!(Ok(zero.clone()), none);
    assert_eq!(ManaCostDSL::print_optional(None), "");
    assert_eq!(ManaCostDSL::print_optional(Some(&c)), "{0}");
    assert_ne!(
        ManaCostDSL::print_optional(None),
        ManaCostDSL::print_optional(Some(&c))
    );
    // Round trip both ways through the optional forms.
    for s in ["", "{0}", "{2}{W}{W}"] {
        let p = ManaCostDSL::parse_optional(s).unwrap();
        assert_eq!(ManaCostDSL::print_optional(p.as_ref()), s);
    }
    // parse_optional still rejects invalid non-empty text.
    assert!(ManaCostDSL::parse_optional("{Q}").is_err());
    assert!(ManaCostDSL::parse_optional(" ").is_err());
}

// ---------------------------------------------------------------- JSON form

/// REQ-DSL-001 / REQ-OBJ-001: the JSON form of a cost is the canonical text as a JSON string.
#[test]
fn req_dsl_001_mana_cost_json_form() {
    for s in ["{2}{W}{W}", "{0}", "{X}{X}{R/P}", "{W/U/P}{2/G}{C}{S}"] {
        let c = cost(s);
        assert_eq!(serde_json::to_string(&c).unwrap(), format!("\"{s}\""));
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            serde_json::Value::String(s.into())
        );
        let back: ManaCostDSL = serde_json::from_str(&format!("\"{s}\"")).unwrap();
        assert_eq!(back, c);
    }
    for bad in [
        "\"\"",
        "\"{Q}\"",
        "\"{w}\"",
        "\"{W} \"",
        "\"W\"",
        "2",
        "null",
        "[\"{W}\"]",
        "{}",
    ] {
        assert!(
            serde_json::from_str::<ManaCostDSL>(bad).is_err(),
            "{bad} must not deserialise"
        );
    }
}

// ---------------------------------------------------------------- property tests

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_u64() % n
}

fn random_color(rng: &mut Pcg64Dxsm) -> Color {
    COLORS[below(rng, 5) as usize].0
}

fn random_pair(rng: &mut Pcg64Dxsm) -> (Color, Color) {
    let a = random_color(rng);
    loop {
        let b = random_color(rng);
        if b != a {
            return (a, b);
        }
    }
}

fn random_generic(rng: &mut Pcg64Dxsm, min: u32) -> u32 {
    match below(rng, 4) {
        0 => MAX_GENERIC,
        1 => min,
        2 => min + below(rng, 20) as u32,
        _ => min + below(rng, u64::from(MAX_GENERIC - min) + 1) as u32,
    }
}

fn random_symbol(rng: &mut Pcg64Dxsm) -> ManaSymbol {
    match below(rng, 9) {
        0 => ManaSymbol::Generic(random_generic(rng, 0)),
        1 => ManaSymbol::Colored(random_color(rng)),
        2 => ManaSymbol::Colorless,
        3 => ManaSymbol::Snow,
        4 => ManaSymbol::Variable([Variable::X, Variable::Y, Variable::Z][below(rng, 3) as usize]),
        5 => {
            let (a, b) = random_pair(rng);
            ManaSymbol::Hybrid(a, b)
        }
        6 => ManaSymbol::GenericHybrid(random_generic(rng, 1), random_color(rng)),
        7 => ManaSymbol::Phyrexian(random_color(rng)),
        _ => {
            let (a, b) = random_pair(rng);
            ManaSymbol::HybridPhyrexian(a, b)
        }
    }
}

/// REQ-OBJ-001: random valid symbol vectors print and parse back to themselves, and the
/// printed text is a fixed point of print(parse(_)).
#[test]
fn req_obj_001_property_print_parse_identity() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x00AA_0BB0_0CC0_0010);
    for _ in 0..2000 {
        let n = 1 + below(&mut rng, 8) as usize;
        let v: Vec<ManaSymbol> = (0..n).map(|_| random_symbol(&mut rng)).collect();
        let c = ManaCostDSL::new(v.clone()).unwrap();
        assert_eq!(c.symbols(), &v[..]);
        let text = c.to_string();
        let expected_text: String = v.iter().map(|s| s.to_string()).collect();
        assert_eq!(text, expected_text);
        let back = ManaCostDSL::parse(&text).unwrap();
        assert_eq!(back, c, "{text}");
        assert_eq!(back.to_string(), text);
        for s in &v {
            assert_eq!(ManaSymbol::parse(&s.to_string()).ok(), Some(*s));
        }
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<ManaCostDSL>(&json).unwrap(), c);
    }
}

/// REQ-OBJ-001 (R12): random strings over the mana alphabet (with multi-byte characters) never
/// panic; when accepted, print(parse(s)) == s.
#[test]
fn req_obj_001_property_random_strings_never_panic() {
    let alphabet: Vec<char> = "{}/ WUBRGCSXYZP0123456789wéあ".chars().collect();
    let mut rng = Pcg64Dxsm::from_u64_seed(0x00AA_0BB0_0CC0_0011);
    let mut accepted = 0usize;
    for i in 0..20_000 {
        let len = below(&mut rng, 16) as usize;
        let mut s = String::new();
        // Bias toward well-bracketed input so the Ok path is exercised too.
        let bracketed = i % 2 == 0;
        for _ in 0..len {
            if bracketed {
                s.push('{');
                for _ in 0..1 + below(&mut rng, 4) {
                    let c = alphabet[below(&mut rng, alphabet.len() as u64) as usize];
                    if c != '{' && c != '}' {
                        s.push(c);
                    }
                }
                s.push('}');
            } else {
                s.push(alphabet[below(&mut rng, alphabet.len() as u64) as usize]);
            }
        }
        let r = ManaCostDSL::parse(&s);
        if let Ok(c) = &r {
            accepted += 1;
            assert_eq!(c.to_string(), s);
        } else if let Err(e) = &r {
            assert!(!e.to_string().is_empty());
            if let ManaParseError::Malformed { offset }
            | ManaParseError::UnknownSymbol { offset, .. } = e
            {
                assert!(*offset <= s.len(), "{s:?}: offset {offset}");
            }
        }
        if let Ok(sym) = ManaSymbol::parse(&s) {
            assert_eq!(sym.to_string(), s);
        }
        if let Ok(Some(c)) = ManaCostDSL::parse_optional(&s) {
            assert_eq!(ManaCostDSL::print_optional(Some(&c)), s);
        }
    }
    assert!(accepted > 0, "the run never exercised the accept path");
}
