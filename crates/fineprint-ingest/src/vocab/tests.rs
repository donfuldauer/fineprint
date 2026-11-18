// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the vocabulary generator (REQ-OBJ-001, REQ-SUB-001, REQ-DSL-014).
//!
//! The synthetic CR text is built in code from `(rule, text)` pairs with invented words only
//! (R5); no line of this file starts with a rule number (card-data guard).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::cr::{self, sentences, split_list, strip_parentheticals};
use super::*;

/// A synthetic CR document.
#[derive(Clone)]
pub(crate) struct Synth {
    pub date: bool,
    pub start: bool,
    pub end: bool,
    pub rules: Vec<(String, String)>,
}

pub(crate) fn synth() -> Synth {
    let r = |n: &str, t: &str| (n.to_owned(), t.to_owned());
    Synth {
        date: true,
        start: true,
        end: true,
        rules: vec![
            r("100.", "Invented Opening"),
            r("111.10.", "Invented intro words about tokens."),
            r(
                "111.10a",
                "A Zeta token is a colorless Zeta alpha token with \u{201c}{T}: Quux the frobnicator.\u{201d}",
            ),
            r(
                "111.10b",
                "A Kappa token is a 3/4 red and blue Kappa Omicron Prime beta token named Kappa Prime with gleam, \u{201c}Quux one,\u{201d} and \u{201c}Zorp \u{2018}two.\u{2019}\u{201d} See rule 999.1, \u{201c}Gleam.\u{201d}",
            ),
            r(
                "111.10c",
                "An Eta token is a double-faced token. Its front face is a colorless Eta alpha with \u{201c}{2}: Flip it.\u{201d} Its back face is a 0/0 colorless Kappa alpha beta named Eta Back.",
            ),
            r("122.1.", "Invented counter intro words here."),
            r(
                "122.1a",
                "A +X/+Y counter on a thing does invented things. See rule 999.3.",
            ),
            r(
                "122.1b",
                "A keyword counter does invented things. The keywords that a keyword counter can be are gleam, double shimmer, and glow, as well as any variants of those keywords.",
            ),
            r(
                "122.1c",
                "One or more blorp counters on a thing do invented things (see rule 999.4).",
            ),
            r(
                "205.2a",
                "The card types are alpha, beta, gamma, and delta. See section 9, \u{201c}Types.\u{201d}",
            ),
            r("205.3a", "Invented subtype words here."),
            r(
                "205.3g",
                "Alphas have their own invented subtypes; these subtypes are called alpha types. The alpha types are Zeta (see rule 999.2), Eta, and Mu\u{2019}s-Kin.",
            ),
            r(
                "205.3h",
                "Betas and gammas share invented subtypes; these subtypes are called beta types. One beta type is two words long: Omicron Prime. All other beta types are one word long: Kappa, Omicron, and Rho-Tau.",
            ),
            r(
                "205.3i",
                "One delta card (Qwerty) has a subtype; this subtype is called a delta type. That delta type is Qwerty.",
            ),
            r(
                "205.3j",
                "Gamma cards and others have no invented subtypes.",
            ),
            r(
                "205.4a",
                "An object has invented supertypes. A thing\u{2019}s supertypes are invented. The supertypes are lofty and humble.",
            ),
            r(
                "701.1.",
                "Most invented actions are prose words. Some are keywords and listed here.",
            ),
            r("701.2.", "Frobnicate"),
            r("701.2a", "Frobnicate means invented things."),
            r(
                "702.1.",
                "Most invented abilities are prose words. Some are keywords.",
            ),
            r("702.2.", "Gleam"),
            r("702.3.", "Double Shimmer"),
            r("702.4.", "Glow"),
        ],
    }
}

impl Synth {
    pub(crate) fn render(&self) -> String {
        let mut out = vec!["Invented Rules Title".to_owned(), String::new()];
        if self.date {
            out.push("These rules are effective as of March 4, 2031.".to_owned());
            out.push(String::new());
        }
        out.push("Contents".to_owned());
        if self.start {
            out.push(cr::START_MARKER.to_owned());
        }
        if self.end {
            out.push(cr::END_MARKER.to_owned());
        }
        out.push(String::new());
        if self.start {
            out.push(cr::START_MARKER.to_owned());
            out.push(String::new());
        }
        for (n, t) in &self.rules {
            out.push(format!("{n} {t}"));
            out.push(String::new());
        }
        if self.end {
            out.push(cr::END_MARKER.to_owned());
            out.push(String::new());
        }
        out.push("Gleam".to_owned());
        out.push("Invented glossary words.".to_owned());
        out.join("\n") + "\n"
    }

    fn without(mut self, rule: &str) -> Self {
        self.rules.retain(|(n, _)| n != rule);
        self
    }

    fn set(mut self, rule: &str, text: &str) -> Self {
        if let Some(e) = self.rules.iter_mut().find(|(n, _)| n == rule) {
            e.1 = text.to_owned();
        } else {
            self.rules.push((rule.to_owned(), text.to_owned()));
        }
        self
    }

    fn generate(&self) -> Result<RulesVocabulary, GenError> {
        generate(self.render().as_bytes())
    }
}

/// A new, empty scratch directory created exclusively under the private root
/// `<workspace>/target/fp-vocab-tests` (never a shared, predictable temp path). The leaf name
/// combines the pid, a process-wide counter and the clock (used only for uniqueness, R7);
/// `create_dir` fails if the leaf exists, which is retried a bounded number of times with a new
/// name. Any other error panics.
pub(crate) fn scratch_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fp-vocab-tests");
    std::fs::create_dir_all(&root).expect("create the scratch root");
    for _ in 0..16 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let leaf = root.join(format!("{tag}-{}-{n}-{nanos}", std::process::id()));
        match std::fs::create_dir(&leaf) {
            Ok(()) => return leaf,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => panic!("cannot create scratch dir {}: {e}", leaf.display()),
        }
    }
    panic!(
        "no fresh scratch dir under {} after 16 tries",
        root.display()
    );
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// REQ-SUB-001, REQ-OBJ-001, REQ-DSL-014: the synthetic CR gives exactly the expected
/// vocabulary.
#[test]
fn req_obj_001_synthetic_vocabulary() {
    let v = synth().generate().unwrap();
    assert_eq!(v.cr_effective_date, "2031-03-04");
    assert_eq!(strs(&v.card_types), ["Alpha", "Beta", "Gamma", "Delta"]);
    assert_eq!(strs(&v.supertypes), ["Lofty", "Humble"]);
    let groups: Vec<(&str, Vec<&str>, &str)> = v
        .subtype_groups
        .iter()
        .map(|g| (g.group.as_str(), strs(&g.card_types), g.rule_ref.as_str()))
        .collect();
    assert_eq!(
        groups,
        [
            ("alpha", vec!["Alpha"], "205.3g"),
            ("beta", vec!["Beta", "Gamma"], "205.3h"),
            ("delta", vec!["Delta"], "205.3i"),
        ]
    );
    assert_eq!(
        strs(&v.subtype_groups[0].subtypes),
        ["Zeta", "Eta", "Mu\u{2019}s-Kin"]
    );
    assert_eq!(
        strs(&v.subtype_groups[1].subtypes),
        ["Omicron Prime", "Kappa", "Omicron", "Rho-Tau"]
    );
    assert_eq!(strs(&v.subtype_groups[2].subtypes), ["Qwerty"]);
    assert_eq!(
        v.subtypes_of("Beta").collect::<Vec<_>>(),
        v.subtypes_of("Gamma").collect::<Vec<_>>()
    );
    let kw = |l: &[Keyword]| -> Vec<(String, String, String)> {
        l.iter()
            .map(|k| (k.id.clone(), k.name.clone(), k.rule_ref.clone()))
            .collect()
    };
    let t = |a: &str, b: &str, c: &str| (a.to_owned(), b.to_owned(), c.to_owned());
    assert_eq!(
        kw(&v.keyword_actions),
        [t("FROBNICATE", "Frobnicate", "701.2")]
    );
    assert_eq!(
        kw(&v.keyword_abilities),
        [
            t("GLEAM", "Gleam", "702.2"),
            t("DOUBLE_SHIMMER", "Double Shimmer", "702.3"),
            t("GLOW", "Glow", "702.4"),
        ]
    );
    let counters: Vec<(&str, &str, &str)> = v
        .counter_kinds
        .iter()
        .map(|c| (c.kind.as_str(), c.class.as_str(), c.rule_ref.as_str()))
        .collect();
    assert_eq!(
        counters,
        [
            ("+X/+Y", "POWER_TOUGHNESS", "122.1a"),
            ("gleam", "KEYWORD", "122.1b"),
            ("double shimmer", "KEYWORD", "122.1b"),
            ("glow", "KEYWORD", "122.1b"),
            ("blorp", "BLORP", "122.1c"),
        ]
    );
}

/// REQ-SUB-001: synthetic tokens: name rules, colors, P/T, subtypes, abilities, two faces.
#[test]
fn req_sub_001_synthetic_tokens() {
    let v = synth().generate().unwrap();
    let letters: Vec<&str> = v
        .predefined_tokens
        .iter()
        .map(|t| t.letter.as_str())
        .collect();
    assert_eq!(letters, ["a", "b", "c"]);
    let a = &v.predefined_tokens[0];
    assert_eq!((a.name.as_str(), a.rule_ref.as_str()), ("Zeta", "111.10a"));
    assert_eq!(a.faces.len(), 1);
    let f = &a.faces[0];
    assert_eq!(f.name, "Zeta");
    assert!(f.colors.is_empty());
    assert_eq!(strs(&f.card_types), ["Alpha"]);
    assert_eq!(strs(&f.subtypes), ["Zeta"]);
    assert_eq!((f.power, f.toughness), (None, None));
    assert_eq!(strs(&f.abilities), ["{T}: Quux the frobnicator."]);

    let b = &v.predefined_tokens[1];
    assert_eq!(b.name, "Kappa Prime");
    let f = &b.faces[0];
    assert_eq!(f.name, "Kappa Prime");
    assert_eq!(strs(&f.colors), ["blue", "red"]);
    assert_eq!(strs(&f.card_types), ["Beta"]);
    assert_eq!(strs(&f.subtypes), ["Kappa", "Omicron Prime"]);
    assert_eq!((f.power, f.toughness), (Some(3), Some(4)));
    assert_eq!(
        strs(&f.abilities),
        ["gleam", "Quux one", "Zorp \u{2018}two.\u{2019}"]
    );

    let c = &v.predefined_tokens[2];
    assert_eq!(c.name, "Eta");
    assert_eq!(c.faces.len(), 2);
    assert_eq!(c.faces[0].name, "Eta");
    assert_eq!(strs(&c.faces[0].abilities), ["{2}: Flip it."]);
    assert_eq!(strs(&c.faces[0].subtypes), ["Eta"]);
    let back = &c.faces[1];
    assert_eq!(back.name, "Eta Back");
    assert_eq!((back.power, back.toughness), (Some(0), Some(0)));
    assert_eq!(strs(&back.card_types), ["Alpha", "Beta"]);
    assert_eq!(strs(&back.subtypes), ["Kappa"]);
    assert!(back.abilities.is_empty());
}

/// REQ-SUB-001: the token table follows the text: one token removed and a new letter `y`
/// added give exactly that table (no hard-coded letters).
#[test]
fn req_sub_001_new_letter_flows_through() {
    let s = synth()
        .without("111.10b")
        .set("702.5.", "Indestructible")
        .set(
            "111.10y",
            "A Theta token is a white Zeta alpha token with indestructible.",
        );
    let v = s.generate().unwrap();
    let letters: Vec<&str> = v
        .predefined_tokens
        .iter()
        .map(|t| t.letter.as_str())
        .collect();
    assert_eq!(letters, ["a", "c", "y"]);
    let y = v.token("y").unwrap();
    assert_eq!(y.rule_ref, "111.10y");
    assert_eq!(strs(&y.faces[0].colors), ["white"]);
    assert_eq!(strs(&y.faces[0].abilities), ["indestructible"]);
    assert_eq!(strs(&y.faces[0].subtypes), ["Zeta"]);
    assert_eq!(y.name, "Theta");
}

/// REQ-OBJ-001: a new 205.3 sub-rule and new keyword headings flow through.
#[test]
fn req_obj_001_new_lists_flow_through() {
    let v = synth()
        .set(
            "205.3k",
            "Gammas also have invented subtypes; these subtypes are called gamma types. The gamma types are Pi and Sigma.",
        )
        .set("702.5.", "Shine\u{2019}s Edge")
        .generate()
        .unwrap();
    let g = v.subtype_groups.last().unwrap();
    assert_eq!((g.group.as_str(), g.rule_ref.as_str()), ("gamma", "205.3k"));
    assert_eq!(strs(&g.subtypes), ["Pi", "Sigma"]);
    assert_eq!(strs(&g.card_types), ["Gamma"]);
    assert_eq!(
        v.subtypes_of("Gamma").collect::<Vec<_>>(),
        [
            "Omicron Prime",
            "Kappa",
            "Omicron",
            "Rho-Tau",
            "Pi",
            "Sigma"
        ]
    );
    assert_eq!(v.keyword_ability("SHINES_EDGE").unwrap().rule_ref, "702.5");
}

/// REQ-SUB-001, REQ-OBJ-001, REQ-DSL-014: generation is byte-deterministic and independent of
/// CRLF line endings and a BOM; output is canonical with exactly one trailing newline.
#[test]
fn req_sub_001_determinism_crlf_bom() {
    let lf = synth().render();
    let a = generate_bytes(lf.as_bytes()).unwrap();
    let b = generate_bytes(lf.as_bytes()).unwrap();
    assert_eq!(a, b);
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(generate_bytes(crlf.as_bytes()).unwrap(), a);
    let bom = format!("\u{feff}{crlf}");
    assert_eq!(generate_bytes(bom.as_bytes()).unwrap(), a);
    let cr_only = lf.replace('\n', "\r");
    assert_eq!(generate_bytes(cr_only.as_bytes()).unwrap(), a);
    assert!(a.ends_with(b"}\n"));
    assert_eq!(a.iter().filter(|c| **c == b'\n').count(), 1);
    let v = generate(lf.as_bytes()).unwrap();
    assert_eq!(v.to_canonical_bytes().unwrap(), a);
    let json = &a[..a.len() - 1];
    assert_eq!(fineprint_dsl::canonical::canonicalize(json).unwrap(), json);
}

/// REQ-DSL-014: the keyword id slug rule.
#[test]
fn req_dsl_014_keyword_id_slug() {
    let cases = [
        ("Flying", "FLYING"),
        ("Double Strike", "DOUBLE_STRIKE"),
        ("Tap and Untap", "TAP_AND_UNTAP"),
        ("For Mirrodin!", "FOR_MIRRODIN"),
        ("Start Your Engines!", "START_YOUR_ENGINES"),
        ("\u{221e} (Infinity)", "INFINITY"),
        ("Jump-Start", "JUMP_START"),
        ("Power-up", "POWER_UP"),
        ("Beam me up", "BEAM_ME_UP"),
        ("Shine\u{2019}s Edge", "SHINES_EDGE"),
        ("O'Brien", "OBRIEN"),
        ("Caf\u{e9} Lumi\u{e8}re", "CAFE_LUMIERE"),
        ("\u{c6}ther \u{df}", "AETHER_SS"),
        ("  Spaced   Out  ", "SPACED_OUT"),
        ("Level 2", "LEVEL_2"),
    ];
    for (name, id) in cases {
        assert_eq!(keyword_id(name).as_deref(), Some(id), "{name}");
        assert!(fineprint_dsl::vocab::is_upper_snake(id));
    }
    assert_eq!(keyword_id("!!"), None);
    assert_eq!(keyword_id("\u{221e}"), None);
    assert_eq!(keyword_id(""), None);
}

/// REQ-DSL-014: introductory 701.1/702.1 are skipped; a prose "heading" is an input error,
/// never a keyword.
#[test]
fn req_dsl_014_headings() {
    let v = synth().generate().unwrap();
    assert!(v.keyword_actions.iter().all(|k| k.rule_ref != "701.1"));
    assert!(v.keyword_abilities.iter().all(|k| k.rule_ref != "702.1"));
    for prose in [
        "Invented words that end with a period.",
        "Two sentences. Invented here",
        "One two three four five six seven eight nine",
    ] {
        let e = synth().set("702.5.", prose).generate().unwrap_err();
        assert_eq!(e.exit_code(), 2, "{prose}");
        assert!(e.to_string().contains("702.5"), "{e}");
    }
    let e = synth().set("702.5.", "!!").generate().unwrap_err();
    assert_eq!(e.exit_code(), 2);
    // a heading followed by a continuation line is prose too
    let mut s = synth();
    s.rules.retain(|(n, _)| !n.starts_with("702."));
    s.rules
        .push(("702.2.".to_owned(), "Gleam\nmore invented words".to_owned()));
    assert_eq!(s.generate().unwrap_err().exit_code(), 2);
}

/// REQ-DSL-014: id collisions within a list or across 701/702 exit 1.
#[test]
fn req_dsl_014_collisions_exit_1() {
    let e = synth()
        .set("702.5.", "Double-Shimmer")
        .generate()
        .unwrap_err();
    assert!(matches!(e, GenError::Collision(_)));
    assert_eq!(e.exit_code(), 1);
    assert!(e.to_string().contains("DOUBLE_SHIMMER"));
    let e = synth().set("702.5.", "Frobnicate").generate().unwrap_err();
    assert_eq!(e.exit_code(), 1);
    assert!(e.to_string().contains("701.2") && e.to_string().contains("702.5"));
    let e = synth().set("701.3.", "Frob-nicate!").generate();
    assert!(e.is_ok());
}

/// REQ-SUB-001, REQ-OBJ-001, REQ-DSL-014: every required rule missing is exit 2 naming it.
#[test]
fn req_sub_001_missing_required_rules() {
    let cases: Vec<(Synth, &str)> = vec![
        (
            synth()
                .without("111.10a")
                .without("111.10b")
                .without("111.10c"),
            "111.10a",
        ),
        (synth().without("205.2a"), "205.2a"),
        (synth().without("205.4a"), "205.4a"),
        (
            synth()
                .without("205.3g")
                .without("205.3h")
                .without("205.3i"),
            "205.3g",
        ),
        (synth().without("122.1b"), "122.1b"),
        (synth().without("701.2.").without("701.2a"), "701.2"),
        (
            synth()
                .without("702.2.")
                .without("702.3.")
                .without("702.4."),
            "702.2",
        ),
        (
            Synth {
                date: false,
                ..synth()
            },
            "effective-date",
        ),
        (
            Synth {
                start: false,
                ..synth()
            },
            "START",
        ),
        (
            Synth {
                end: false,
                ..synth()
            },
            "END",
        ),
    ];
    for (s, needle) in cases {
        let e = s.generate().unwrap_err();
        assert_eq!(e.exit_code(), 2, "{needle}: {e}");
        assert!(e.to_string().contains(needle), "{needle}: {e}");
    }
}

/// REQ-SUB-001, REQ-OBJ-001 (R12): malformed rules are input errors, never partial output.
#[test]
fn req_obj_001_malformed_rules_are_errors() {
    let bad: Vec<(&str, &str)> = vec![
        ("205.2a", "The card types are."),
        ("205.2a", "No list sentence here."),
        ("205.2a", "The card types are alpha (unclosed, beta."),
        ("205.2a", "The card types are alpha, alpha, and beta."),
        ("205.4a", "The supertypes are lofty: humble."),
        (
            "205.3g",
            "Alphas are listed; these subtypes are called alpha types. Nothing listed.",
        ),
        (
            "205.3g",
            "Nothing named; these subtypes are called alpha types. The alpha types are Zeta.",
        ),
        (
            "205.3g",
            "Alphas list; these subtypes are called alpha types. The alpha types are \u{201c}Zeta.",
        ),
        (
            "205.3g",
            "Alphas list; these subtypes are called alpha types. The alpha types are Zeta, and Zeta.",
        ),
        ("122.1c", "Invented words without the special noun."),
        ("122.1c", "A counter alone does invented things."),
        ("122.1c", "Unbalanced (words counter."),
        ("122.1a", "Unbalanced (words +X/+Y counter."),
        (
            "122.1b",
            "The keywords that a keyword counter can be are glow: more.",
        ),
        ("111.10a", "Invented words without a framing sentence."),
        ("111.10a", "A Zeta token has no copula here."),
        ("111.10a", "A Zeta token is colorless with nothing."),
        (
            "111.10a",
            "A Zeta token is a colorless Zeta alpha token. An extra invented sentence.",
        ),
        (
            "111.10a",
            "A Zeta token is a colorless Unknownish alpha token.",
        ),
        ("111.10a", "A Zeta token is a colorless Zeta token."),
        ("111.10a", "A Zeta token is a Zeta alpha token."),
        (
            "111.10a",
            "A Zeta token is a colorless red Zeta alpha token.",
        ),
        (
            "111.10a",
            "A Zeta token is a */2 colorless Zeta alpha token.",
        ),
        (
            "111.10a",
            "A Zeta token is a 1/x colorless Zeta alpha token.",
        ),
        ("111.10a", "A Zeta token is a colorless alpha Zeta token."),
        (
            "111.10a",
            "A Zeta token is a colorless Zeta Zeta alpha token.",
        ),
        (
            "111.10a",
            "A Zeta token is a colorless Zeta alpha token named Bad.Name.",
        ),
        (
            "111.10a",
            "A Zeta token is a colorless Zeta alpha token with \u{201c}\u{201d}.",
        ),
        (
            "111.10a",
            "A Zeta token is a colorless Zeta alpha token with \u{201c}open.",
        ),
        (
            "111.10a",
            "A Zeta token is a colorless Zeta alpha token with close.\u{201d} More.",
        ),
        (
            "111.10a",
            "A Zeta token is a double-faced token. Its front face is a colorless Zeta alpha. Its back face is a colorless Zeta alpha. Its third face is a colorless Zeta alpha.",
        ),
        (
            "111.10a",
            "A Zeta token is a double-faced token. Its front face is colorless Zeta alpha.",
        ),
        ("111.10aa", "A Zeta token is a colorless Zeta alpha token."),
    ];
    for (rule, text) in bad {
        let e = synth().set(rule, text).generate().unwrap_err();
        assert_eq!(e.exit_code(), 2, "{rule} {text}: {e}");
    }
    // duplicate rule numbers
    let mut s = synth();
    s.rules
        .push(("205.2a".to_owned(), "The card types are alpha.".to_owned()));
    assert_eq!(s.generate().unwrap_err().exit_code(), 2);
    let mut s = synth();
    s.rules.push(("205.3h".to_owned(), "Invented.".to_owned()));
    assert_eq!(s.generate().unwrap_err().exit_code(), 2);
    let mut s = synth();
    s.rules.push(("702.2.".to_owned(), "Gleam".to_owned()));
    assert!(s.generate().is_err());
    // invalid UTF-8
    let e = generate(b"\xff\xfe invented").unwrap_err();
    assert!(e.to_string().contains("UTF-8"));
    // effective-date line without a parseable date
    for date in [
        "Smarch 4, 2031.",
        "March 4 2031.",
        "March 32, 2031.",
        "March 4, 31.",
        "March x, 2031.",
        "February 29, 2031.",
        "March",
    ] {
        let text = synth().render().replace("March 4, 2031.", date);
        let e = generate(text.as_bytes()).unwrap_err();
        assert!(e.to_string().contains("effective-date"), "{date}: {e}");
    }
    // END before START
    let text = format!(
        "{}\n{}\n{}\n",
        cr::END_MARKER,
        cr::START_MARKER,
        "These rules are effective as of March 4, 2031."
    );
    assert!(
        generate(text.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("END")
    );
}

/// REQ-SUB-001, REQ-OBJ-001: accepted variants of the text shapes.
#[test]
fn req_obj_001_accepted_variants() {
    let v = synth()
        .set("702.5.", "Indestructible")
        .set("205.4a", "The supertypes are lofty.")
        .set("122.1c", "A shine counter on a thing; see invented.")
        .set(
            "111.10a",
            "A Zeta token is a 1/1 green and white lofty Zeta alpha token with indestructible and \"Straight quoted.\"",
        )
        .generate()
        .unwrap();
    assert_eq!(strs(&v.supertypes), ["Lofty"]);
    assert_eq!(v.counter_kind("shine").unwrap().class, "SHINE");
    let f = &v.token("a").unwrap().faces[0];
    assert_eq!(strs(&f.colors), ["white", "green"]);
    assert_eq!(strs(&f.supertypes), ["Lofty"]);
    assert_eq!(strs(&f.abilities), ["indestructible", "Straight quoted."]);
    assert_eq!((f.power, f.toughness), (Some(1), Some(1)));
}

/// REQ-OBJ-001: the shared text helpers.
#[test]
fn req_obj_001_text_helpers() {
    assert_eq!(
        strip_parentheticals("A (x (y)) B, C (z)").unwrap(),
        "A B, C"
    );
    assert!(strip_parentheticals("A (b").is_none());
    assert!(strip_parentheticals("A b)").is_none());
    assert_eq!(
        sentences("One two. \u{201c}Three.\u{201d} Four 1.5 five. Six").unwrap(),
        ["One two", "\u{201c}Three.\u{201d}", "Four 1.5 five", "Six"]
    );
    assert_eq!(sentences("A \"b.\" C.").unwrap(), ["A \"b.\"", "C"]);
    assert!(sentences("A \u{201c}b").is_none());
    assert!(sentences("A b\u{201d}").is_none());
    assert!(sentences("A \"b").is_none());
    assert_eq!(split_list("Zeta").unwrap(), ["Zeta"]);
    assert_eq!(split_list("Zeta and Eta").unwrap(), ["Zeta", "Eta"]);
    assert_eq!(split_list("a, b, or c").unwrap(), ["a", "b", "c"]);
    assert_eq!(
        split_list("a, b, and c, as well as d").unwrap(),
        ["a", "b", "c"]
    );
    assert!(split_list("").is_none());
    assert!(split_list("a, , b").is_none());
    assert!(split_list("one two three four five six seven").is_none());
    assert_eq!(cr::capitalize("\u{e9}lan"), "\u{c9}lan");
    assert_eq!(cr::capitalize(""), "");
}

/// REQ-OBJ-001: rule-start shapes and section headings.
#[test]
fn req_obj_001_rule_starts() {
    let text = Synth {
        rules: vec![
            ("205.2a".to_owned(), "The card types are alpha.".to_owned()),
            ("205.".to_owned(), "Invented Heading".to_owned()),
            (
                "205.2b".to_owned(),
                "Invented.\nExample: invented example.\ncontinued words".to_owned(),
            ),
            ("205.3".to_owned(), String::new()),
        ],
        ..synth()
    }
    .render();
    let doc = cr::parse_doc(text.as_bytes()).unwrap();
    let ids: Vec<String> = doc.rules.iter().map(cr::Rule::id).collect();
    assert_eq!(ids, ["205.2a", "205.2b"]);
    assert_eq!(doc.rules[1].text, "Invented. continued words");
    assert_eq!(doc.rules[1].continuation_lines, 1);
    assert_eq!(doc.rules[0].base(), "205.2");
}

/// Deterministic test PRNG (splitmix64; fixed seed).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
}

/// REQ-OBJ-001 (R12): 2,000 random byte strings and 2,000 mutations of the synthetic CR never
/// panic the CR vocabulary parser; every success is a valid vocabulary.
#[test]
fn req_obj_001_cr_parser_never_panics() {
    let mut rng = Rng(0x0015_0001);
    for _ in 0..2_000 {
        let len = rng.below(300);
        let bytes: Vec<u8> = (0..len).map(|_| rng.next().to_le_bytes()[0]).collect();
        let _ = generate(&bytes);
    }
    let base = synth().render().into_bytes();
    let pieces: [&[u8]; 12] = [
        b".",
        b" ",
        b"\n",
        b",",
        b" and ",
        b"\xe2\x80\x9c",
        b"\xe2\x80\x9d",
        b"(",
        b")",
        b"1",
        b"a",
        b"\xff",
    ];
    for _ in 0..2_000 {
        let mut bytes = base.clone();
        for _ in 0..=rng.below(4) {
            let i = rng.below(bytes.len());
            match rng.below(3) {
                0 => {
                    let end = (i + rng.below(8)).min(bytes.len());
                    bytes.drain(i..end);
                }
                1 => {
                    let p = pieces[rng.below(pieces.len())];
                    bytes.splice(i..i, p.iter().copied());
                }
                _ => bytes[i] = rng.next().to_le_bytes()[0],
            }
        }
        if let Ok(v) = generate(&bytes) {
            assert!(v.validate().is_ok());
        }
    }
}

/// REQ-OBJ-001: the CLI on synthetic input (exit codes 0/1/2, --out -, --check).
#[test]
fn req_obj_001_cli_unit() {
    let dir = scratch_dir("cli-unit");
    let cr_path = dir.join("rules.txt");
    std::fs::write(&cr_path, synth().render()).unwrap();
    let out_path = dir.join("vocab.json");
    let run = |args: &[&std::ffi::OsStr]| {
        let args: Vec<OsString> = args.iter().map(|a| (*a).to_owned()).collect();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let code = cli::run(&args, &mut o, &mut e);
        (
            code,
            String::from_utf8(o).unwrap(),
            String::from_utf8(e).unwrap(),
        )
    };
    let crp = cr_path.as_os_str();
    let outp = out_path.as_os_str();
    let (c, o, _) = run(&["--cr".as_ref(), crp, "--out".as_ref(), outp]);
    assert_eq!(c, 0);
    assert_eq!(o.lines().count(), 1);
    assert!(o.contains("3 predefined tokens"));
    let written = std::fs::read(&out_path).unwrap();
    assert_eq!(
        written,
        generate_bytes(synth().render().as_bytes()).unwrap()
    );
    let (c, o, _) = run(&[
        "--cr".as_ref(),
        crp,
        "--out".as_ref(),
        outp,
        "--check".as_ref(),
    ]);
    assert_eq!(c, 0, "{o}");
    let (c, o, e) = run(&["--cr".as_ref(), crp, "--out".as_ref(), "-".as_ref()]);
    assert_eq!(c, 0);
    assert_eq!(o.as_bytes(), written.as_slice());
    assert!(e.contains("wrote stdout"));
    // a changed name: first differing section named, no CR text
    let changed = String::from_utf8(written.clone())
        .unwrap()
        .replace("Frobnicate", "Frobnicatx");
    std::fs::write(&out_path, changed).unwrap();
    let (c, _, e) = run(&[
        "--cr".as_ref(),
        crp,
        "--out".as_ref(),
        outp,
        "--check".as_ref(),
    ]);
    assert_eq!(c, 1);
    assert!(e.contains("keywordActions"), "{e}");
    // same content, different bytes
    let mut spaced = written.clone();
    spaced.insert(1, b' ');
    std::fs::write(&out_path, &spaced).unwrap();
    let (c, _, e) = run(&[
        "--cr".as_ref(),
        crp,
        "--out".as_ref(),
        outp,
        "--check".as_ref(),
    ]);
    assert_eq!(c, 1);
    assert!(e.contains("not canonical"), "{e}");
    // unloadable committed file
    std::fs::write(&out_path, b"{").unwrap();
    let (c, _, e) = run(&[
        "--cr".as_ref(),
        crp,
        "--out".as_ref(),
        outp,
        "--check".as_ref(),
    ]);
    assert_eq!(c, 1);
    assert!(e.contains("does not load"), "{e}");
    // missing committed file
    let missing = dir.join("missing.json");
    let (c, _, _) = run(&[
        "--cr".as_ref(),
        crp,
        "--out".as_ref(),
        missing.as_os_str(),
        "--check".as_ref(),
    ]);
    assert_eq!(c, 1);
    // usage and input errors
    for args in [
        vec![],
        vec!["--cr".as_ref()],
        vec!["--out".as_ref()],
        vec!["--bogus".as_ref()],
        vec!["--cr".as_ref(), crp, "--cr".as_ref(), crp],
        vec![
            "--cr".as_ref(),
            crp,
            "--out".as_ref(),
            outp,
            "--out".as_ref(),
            outp,
        ],
        vec![
            "--cr".as_ref(),
            crp,
            "--check".as_ref(),
            "--out".as_ref(),
            "-".as_ref(),
        ],
        vec!["--cr".as_ref(), dir.join("nope.txt").as_os_str()],
        vec![
            "--cr".as_ref(),
            crp,
            "--out".as_ref(),
            dir.join("no/such/dir.json").as_os_str(),
        ],
    ] {
        let (c, _, e) = run(&args);
        assert_eq!(c, 2, "{args:?}: {e}");
    }
    // a collision exits 1, a bad CR exits 2
    std::fs::write(&cr_path, synth().set("702.5.", "Glow!").render()).unwrap();
    assert_eq!(run(&["--cr".as_ref(), crp, "--out".as_ref(), outp]).0, 1);
    std::fs::write(&cr_path, synth().without("205.2a").render()).unwrap();
    assert_eq!(run(&["--cr".as_ref(), crp, "--out".as_ref(), outp]).0, 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-OBJ-001: `first_difference` reports the first differing section in wire order.
#[test]
fn req_obj_001_first_difference() {
    let v = synth().generate().unwrap();
    let mut other = v.clone();
    other.cr_effective_date = "2031-03-05".to_owned();
    other.predefined_tokens.pop();
    let bytes = other.to_canonical_bytes().unwrap();
    assert_eq!(
        cli::first_difference(&bytes, &v),
        "first differing section `crEffectiveDate`"
    );
}

/// Runs the `vocab` CLI in process: (exit code, stdout, stderr).
fn run_cli(args: &[&std::ffi::OsStr]) -> (i32, String, String) {
    let args: Vec<OsString> = args.iter().map(|a| (*a).to_owned()).collect();
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = cli::run(&args, &mut o, &mut e);
    (
        code,
        String::from_utf8(o).unwrap(),
        String::from_utf8(e).unwrap(),
    )
}

/// A scratch dir holding the synthetic CR as `rules.txt`; returns (dir, cr path).
fn cli_fixture(tag: &str) -> (PathBuf, PathBuf) {
    let dir = scratch_dir(tag);
    let cr_path = dir.join("rules.txt");
    std::fs::write(&cr_path, synth().render()).unwrap();
    (dir, cr_path)
}

/// REQ-OBJ-001: a path option followed by one of the CLI's own options is a usage error naming
/// the option without a value; a path that merely contains an option name is a path.
#[test]
fn req_obj_001_cli_path_option_followed_by_option() {
    let (dir, cr_path) = cli_fixture("cli-flags");
    let crp = cr_path.as_os_str();
    let (c, _, e) = run_cli(&["--cr".as_ref(), crp, "--out".as_ref(), "--check".as_ref()]);
    assert_eq!(c, 2, "{e}");
    assert!(e.contains("--out needs a path"), "{e}");
    let (c, _, e) = run_cli(&[
        "--cr".as_ref(),
        "--check".as_ref(),
        "--out".as_ref(),
        "-".as_ref(),
    ]);
    assert_eq!(c, 2, "{e}");
    assert!(e.contains("--cr needs a path"), "{e}");
    let (c, _, e) = run_cli(&["--out".as_ref(), "--cr".as_ref(), "--cr".as_ref(), crp]);
    assert_eq!(c, 2, "{e}");
    assert!(e.contains("--out needs a path"), "{e}");
    // `./--check` is a path: with --check it is compared (missing file: exit 1, not usage 2)
    let (c, _, e) = run_cli(&[
        "--cr".as_ref(),
        crp,
        "--out".as_ref(),
        "./--check".as_ref(),
        "--check".as_ref(),
    ]);
    assert_eq!(c, 1, "{e}");
    assert!(e.contains("./--check is missing or unreadable"), "{e}");
    // and a file named `--check` is written when given as a path
    let flag_named = dir.join("--check");
    let (c, _, e) = run_cli(&[
        "--cr".as_ref(),
        crp,
        "--out".as_ref(),
        flag_named.as_os_str(),
    ]);
    assert_eq!(c, 0, "{e}");
    assert!(flag_named.is_file());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-OBJ-001: the output is written atomically; a failed write leaves no temporary file and
/// the destination unchanged, and never follows or overwrites an existing temporary path.
#[test]
fn req_obj_001_cli_atomic_write() {
    let dir = scratch_dir("cli-atomic");
    let entries = |d: &Path| {
        let mut v: Vec<OsString> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        v.sort();
        v
    };
    // success: replaces an existing file, no temporary file left
    let dest = dir.join("vocab.json");
    std::fs::write(&dest, b"old").unwrap();
    cli::write_atomic(&dest, b"new").unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"new");
    assert_eq!(entries(&dir), [OsString::from("vocab.json")]);
    // the destination is a directory: the rename fails, the temp file is removed
    let as_dir = dir.join("as-dir.json");
    std::fs::create_dir(&as_dir).unwrap();
    std::fs::write(as_dir.join("inside"), b"kept").unwrap();
    assert!(cli::write_atomic(&as_dir, b"new").is_err());
    assert!(!cli::temp_path_for(&as_dir).unwrap().exists());
    assert_eq!(std::fs::read(as_dir.join("inside")).unwrap(), b"kept");
    assert_eq!(
        entries(&dir),
        [OsString::from("as-dir.json"), OsString::from("vocab.json")]
    );
    // the parent does not exist
    let orphan = dir.join("no-such-dir").join("vocab.json");
    assert!(cli::write_atomic(&orphan, b"new").is_err());
    assert!(!dir.join("no-such-dir").exists());
    // an existing path at the temporary name is neither followed nor clobbered, and the
    // destination keeps its bytes
    let tmp = cli::temp_path_for(&dest).unwrap();
    std::fs::write(&tmp, b"foreign").unwrap();
    let e = cli::write_atomic(&dest, b"newer").unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&dest).unwrap(), b"new");
    assert_eq!(std::fs::read(&tmp).unwrap(), b"foreign");
    std::fs::remove_file(&tmp).unwrap();
    // through the CLI: exit 2, the directory destination is untouched
    let cr_path = dir.join("rules.txt");
    std::fs::write(&cr_path, synth().render()).unwrap();
    let (c, _, e) = run_cli(&[
        "--cr".as_ref(),
        cr_path.as_os_str(),
        "--out".as_ref(),
        as_dir.as_os_str(),
    ]);
    assert_eq!(c, 2, "{e}");
    assert!(as_dir.is_dir());
    assert!(!cli::temp_path_for(&as_dir).unwrap().exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-OBJ-001: a missing default (bundled) vocabulary must be restored from git; a missing
/// custom `--out` file keeps the regenerate instruction.
#[test]
fn req_obj_001_cli_missing_file_diagnostics() {
    let (dir, cr_path) = cli_fixture("cli-missing");
    let crp = cr_path.as_os_str();
    // the tests run in the package directory, where the default relative path does not exist
    assert!(!Path::new(cli::DEFAULT_OUT).exists());
    let (c, _, e) = run_cli(&["--cr".as_ref(), crp, "--check".as_ref()]);
    assert_eq!(c, 1, "{e}");
    assert!(e.contains("missing or unreadable"), "{e}");
    assert!(
        e.contains("restore it from git")
            && e.contains("git checkout -- crates/fineprint-dsl/data/vocab.json"),
        "{e}"
    );
    assert!(!e.contains("cargo run"), "{e}");
    let custom = dir.join("custom.json");
    let (c, _, e) = run_cli(&[
        "--cr".as_ref(),
        crp,
        "--out".as_ref(),
        custom.as_os_str(),
        "--check".as_ref(),
    ]);
    assert_eq!(c, 1, "{e}");
    assert!(
        e.contains("missing or unreadable") && e.contains("regenerate it"),
        "{e}"
    );
    assert!(!e.contains("git checkout"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-OBJ-001: the mismatch hint keeps the selected `--cr` and `--out`.
#[test]
fn req_obj_001_cli_mismatch_hint_keeps_paths() {
    let (dir, cr_path) = cli_fixture("cli-hint");
    let custom_cr = dir.join("custom-rules.txt");
    std::fs::rename(&cr_path, &custom_cr).unwrap();
    let custom_out = dir.join("custom-vocab.json");
    let mut v = synth().generate().unwrap();
    v.keyword_actions[0].name = "Frobnicatx".to_owned();
    v.keyword_actions[0].id = "FROBNICATX".to_owned();
    std::fs::write(&custom_out, v.to_canonical_bytes().unwrap()).unwrap();
    let (c, _, e) = run_cli(&[
        "--cr".as_ref(),
        custom_cr.as_os_str(),
        "--out".as_ref(),
        custom_out.as_os_str(),
        "--check".as_ref(),
    ]);
    assert_eq!(c, 1, "{e}");
    assert!(e.contains("rerun the same command without --check"), "{e}");
    assert!(e.contains(&format!("--cr {}", custom_cr.display())), "{e}");
    assert!(
        e.contains(&format!("--out {}", custom_out.display())),
        "{e}"
    );
    assert!(!e.contains("<rules.txt>"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-OBJ-001 (R12): a comparison file that does not load is described by a fixed category
/// and field name only, never by a value from the file or the loader's error text.
#[test]
fn req_obj_001_cli_unloadable_file_never_echoes_values() {
    const MARKER: &str = "zqx-marker-7731";
    let v = synth().generate().unwrap();
    let mut value = serde_json::to_value(&v).unwrap();
    value["vocabularyVersion"] = serde_json::json!(MARKER);
    let bytes = serde_json::to_vec(&value).unwrap();
    let d = cli::first_difference(&bytes, &v);
    assert!(!d.contains(MARKER), "{d}");
    assert!(
        d.contains("not a valid RulesVocabulary (field: vocabularyVersion)"),
        "{d}"
    );
    // an unknown member named by the marker (shape error)
    let mut value = serde_json::to_value(&v).unwrap();
    value[MARKER] = serde_json::json!(1);
    let d = cli::first_difference(&serde_json::to_vec(&value).unwrap(), &v);
    assert!(!d.contains(MARKER), "{d}");
    assert!(d.contains("wrong shape"), "{d}");
    // a value that breaks a rule (the marker as a duplicate card type)
    let mut value = serde_json::to_value(&v).unwrap();
    value["cardTypes"] = serde_json::json!([MARKER, MARKER]);
    let d = cli::first_difference(&serde_json::to_vec(&value).unwrap(), &v);
    assert!(!d.contains(MARKER), "{d}");
    assert!(d.contains("(field: cardTypes"), "{d}");
    // unparseable JSON
    let d = cli::first_difference(format!("{{\"{MARKER}").as_bytes(), &v);
    assert!(!d.contains(MARKER), "{d}");
    assert!(d.contains("(unparseable JSON)"), "{d}");
    // through the CLI
    let (dir, cr_path) = cli_fixture("cli-echo");
    let out = dir.join("vocab.json");
    std::fs::write(&out, &bytes).unwrap();
    let (c, o, e) = run_cli(&[
        "--cr".as_ref(),
        cr_path.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--check".as_ref(),
    ]);
    assert_eq!(c, 1, "{e}");
    assert!(!e.contains(MARKER) && !o.contains(MARKER), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// REQ-DSL-014: CR 701.1, CR 702.1: N = 0 is an input error, N = 1 is the skipped
/// introduction, N >= 2 is a keyword; the number is compared as an integer.
#[test]
fn req_dsl_014_keyword_rule_number_guard() {
    for rule in ["702.0.", "701.0.", "702.00."] {
        let e = synth().set(rule, "Invented").generate().unwrap_err();
        assert_eq!(e.exit_code(), 2, "{rule}: {e}");
        assert!(
            e.to_string().contains("not a valid keyword rule number"),
            "{e}"
        );
    }
    // 702.1 is skipped even when heading-shaped; 702.01 is the same rule
    for rule in ["702.1.", "702.01."] {
        let v = synth().set(rule, "Shimmerless").generate().unwrap();
        assert!(
            v.keyword_abilities.iter().all(|k| k.id != "SHIMMERLESS"),
            "{rule}"
        );
    }
    let v = synth().set("702.2.", "Gleam").generate().unwrap();
    assert_eq!(v.keyword_ability("GLEAM").unwrap().rule_ref, "702.2");
}

/// The synthetic double-faced token rule with the given face sentences.
fn dfc(sentences: &str) -> Synth {
    synth().set(
        "111.10c",
        &format!("An Eta token is a double-faced token.{sentences}"),
    )
}

const FRONT: &str = " Its front face is a colorless Eta alpha with \u{201c}{2}: Flip it.\u{201d}";
const BACK: &str = " Its back face is a 0/0 colorless Kappa alpha beta named Eta Back.";

/// REQ-SUB-001: CR 111.10i: a double-faced token has one front and one back face sentence,
/// front first; reversed, repeated, missing or unknown labels are input errors.
#[test]
fn req_sub_001_double_faced_labels() {
    let v = dfc(&format!("{FRONT}{BACK}")).generate().unwrap();
    let c = v.token("c").unwrap();
    assert_eq!(c.faces.len(), 2);
    assert_eq!(
        (c.faces[0].name.as_str(), c.faces[1].name.as_str()),
        ("Eta", "Eta Back")
    );
    for (what, sentences, needle) in [
        ("reversed", format!("{BACK}{FRONT}"), "precedes"),
        ("front only", FRONT.to_owned(), "one front and one back"),
        ("back only", BACK.to_owned(), "one front and one back"),
        ("no face", String::new(), "one front and one back"),
        ("two fronts", format!("{FRONT}{FRONT}{BACK}"), "repeats"),
        ("two backs", format!("{FRONT}{BACK}{BACK}"), "repeats"),
        (
            "unknown label",
            format!("{FRONT} Its side face is a colorless Eta alpha."),
            "label",
        ),
        (
            "upper-case label",
            format!("{FRONT} Its Back face is a colorless Eta alpha."),
            "label",
        ),
    ] {
        let e = dfc(&sentences).generate().unwrap_err();
        assert_eq!(e.exit_code(), 2, "{what}: {e}");
        assert!(e.to_string().contains("111.10c"), "{what}: {e}");
        assert!(e.to_string().contains(needle), "{what}: {e}");
    }
}

/// REQ-SUB-001: CR 111.10i: the face count follows the declared layout: a single-faced token
/// with a face sentence is an input error; a single-faced token without one has one face.
#[test]
fn req_sub_001_face_count_matches_layout() {
    let e = synth()
        .set(
            "111.10a",
            &format!("A Zeta token is a colorless Zeta alpha token.{BACK}"),
        )
        .generate()
        .unwrap_err();
    assert_eq!(e.exit_code(), 2);
    assert!(e.to_string().contains("not declared double-faced"), "{e}");
    let e = synth()
        .set(
            "111.10a",
            &format!("A Zeta token is a colorless Zeta alpha token.{FRONT}{BACK}"),
        )
        .generate()
        .unwrap_err();
    assert_eq!(e.exit_code(), 2);
    let v = synth()
        .set("111.10a", "A Zeta token is a colorless Zeta alpha token.")
        .generate()
        .unwrap();
    assert_eq!(v.token("a").unwrap().faces.len(), 1);
}

/// The synthetic single-faced token rule with the given `with ...` phrase.
fn with_abilities(phrase: &str) -> Result<RulesVocabulary, GenError> {
    synth()
        .set(
            "111.10a",
            &format!("A Zeta token is a colorless Zeta alpha token with {phrase}"),
        )
        .generate()
}

/// REQ-SUB-001, REQ-DSL-014, REQ-NFR-004: CR 702: an unquoted token ability must be a keyword
/// ability phrase (a keyword name, alone or with one parameter); quoted abilities are kept
/// as written.
#[test]
fn req_sub_001_token_ability_grammar() {
    let abilities = |phrase: &str| {
        let v = with_abilities(phrase).unwrap();
        v.token("a").unwrap().faces[0].abilities.clone()
    };
    // keywords, case-insensitively, including a two-word name
    assert_eq!(abilities("gleam."), ["gleam"]);
    assert_eq!(
        abilities("Gleam and double shimmer."),
        ["Gleam", "double shimmer"]
    );
    // parameterised keywords: a card type, a number or X, a mana cost
    assert_eq!(abilities("gleam alpha."), ["gleam alpha"]);
    assert_eq!(abilities("glow 3 and glow X."), ["glow 3", "glow X"]);
    assert_eq!(abilities("gleam {2}{R/G}."), ["gleam {2}{R/G}"]);
    // quoted abilities with arbitrary text
    assert_eq!(
        abilities("glow and \u{201c}this sentence is not a keyword.\u{201d}"),
        ["glow", "this sentence is not a keyword."]
    );
    // empty fragments are still skipped
    assert_eq!(abilities("gleam,, and glow."), ["gleam", "glow"]);
    // anything else is an input error naming the rule
    for phrase in [
        "this sentence is not a keyword.",
        "gleam and invented prose.",
        "gleamy.",
        "gleam beyond.",
        "gleam alpha beta.",
        "gleam {}.",
        "gleam {2}x.",
        "frobnicate.",
    ] {
        let e = with_abilities(phrase).unwrap_err();
        assert_eq!(e.exit_code(), 2, "{phrase}: {e}");
        let m = e.to_string();
        assert!(
            m.contains("111.10a") && m.contains("not a keyword ability phrase"),
            "{m}"
        );
    }
}
