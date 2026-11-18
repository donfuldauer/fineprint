// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit and property tests for choice specs (REQ-DSL-011, REQ-CORE-006, REQ-NFR-006,
//! REQ-NFR-008). Every word is synthetic (R5).

use fineprint_base::prng::Pcg64Dxsm;

use super::*;
use crate::expr::tests::{
    accept, assert_wire_props, check_wire_enum, decode_never_panics, gen_player, gen_selector,
    gen_str, gen_value, mutate, pick, random_doc, reject,
};
use crate::expr::{PlayerGroup, ReferenceAST};

/// The controller as chooser.
fn controller() -> PlayerSelectorAST {
    PlayerSelectorAST::Ref(ReferenceAST::Controller)
}

/// A generated valid spec.
fn gen_spec(rng: &mut Pcg64Dxsm) -> ChoiceSpec {
    let kind = ChoiceKind::ALL[pick(rng, ChoiceKind::ALL.len())];
    let mut spec = ChoiceSpec::new(gen_player(rng), kind);
    spec.candidates = match pick(rng, 4) {
        0 => None,
        1 => Some(ChoiceCandidates::Selector(gen_selector(rng, 2))),
        2 => Some(ChoiceCandidates::Players(gen_player(rng))),
        _ => {
            let mut items: Vec<String> = Vec::new();
            for _ in 0..1 + pick(rng, 4) {
                let s = gen_str(rng);
                if !items.contains(&s) {
                    items.push(s);
                }
            }
            Some(ChoiceCandidates::Enum(items))
        }
    };
    let bounded = |rng: &mut Pcg64Dxsm| -> ValueExpressionAST {
        if pick(rng, 2) == 0 {
            ValueExpressionAST::Literal(i64::try_from(pick(rng, 5)).expect("small"))
        } else {
            gen_value(rng, 2)
        }
    };
    let flag = |rng: &mut Pcg64Dxsm| [None, Some(true), Some(false)][pick(rng, 3)];
    if pick(rng, 3) == 0 {
        spec.up_to = Some(true);
        spec.max = Some(bounded(rng));
    } else {
        spec.up_to = [None, Some(false)][pick(rng, 2)];
        spec.min = (pick(rng, 2) == 0).then(|| bounded(rng));
        spec.max = (pick(rng, 2) == 0).then(|| bounded(rng));
    }
    // Literal bounds must be non-negative and ordered.
    for bound in [&mut spec.min, &mut spec.max] {
        if let Some(ValueExpressionAST::Literal(i)) = bound {
            *i = i.abs();
        }
    }
    if let (Some(ValueExpressionAST::Literal(lo)), Some(ValueExpressionAST::Literal(hi))) =
        (&spec.min, &spec.max)
        && lo > hi
    {
        std::mem::swap(&mut spec.min, &mut spec.max);
    }
    spec.hidden = flag(rng);
    spec.random = flag(rng);
    spec.reveal_to = (pick(rng, 2) == 0).then(|| gen_player(rng));
    spec
}

/// REQ-DSL-011: every ChoiceKind decodes in a minimal spec; `ChoiceSpec::new` leaves every
/// optional member absent.
#[test]
fn req_dsl_011_every_kind() {
    check_wire_enum(ChoiceKind::ALL, ChoiceKind::as_str, ChoiceKind::from_wire);
    let names: Vec<_> = ChoiceKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        names,
        [
            "OBJECTS",
            "MODE",
            "NUMBER",
            "COLOR",
            "CREATURE_TYPE",
            "CARD_NAME",
            "PLAYER",
            "YES_NO",
            "ORDER",
            "DIVISION",
            "PILE"
        ]
    );
    for kind in ChoiceKind::ALL {
        let spec: ChoiceSpec = accept(&format!(
            "{{\"chooser\":{{\"ref\":\"CONTROLLER\"}},\"kind\":\"{kind}\"}}"
        ));
        assert_eq!(spec, ChoiceSpec::new(controller(), *kind));
    }
}

/// REQ-DSL-011: each candidates form (SELECTOR, PLAYERS, ENUM) and its rejections.
#[test]
fn req_dsl_011_candidates_forms() {
    assert_eq!(
        accept::<ChoiceCandidates>(
            r#"{"SELECTOR":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}}"#
        ),
        ChoiceCandidates::Selector(
            serde_json::from_str(r#"{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}"#)
                .expect("selector")
        )
    );
    assert_eq!(
        accept::<ChoiceCandidates>(r#"{"PLAYERS":{"players":"ALL_PLAYERS"}}"#),
        ChoiceCandidates::Players(PlayerSelectorAST::Players(PlayerGroup::AllPlayers))
    );
    assert_eq!(
        accept::<ChoiceCandidates>(r#"{"ENUM":["mode_a","mode_b"]}"#),
        ChoiceCandidates::Enum(vec!["mode_a".into(), "mode_b".into()])
    );
    accept::<ChoiceSpec>(
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":"MODE","candidates":{"ENUM":["one"]}}"#,
    );
    for doc in [
        r#"{"ENUM":[]}"#,
        r#"{"ENUM":["a","a"]}"#,
        r#"{"ENUM":[""]}"#,
        r#"{"ENUM":[1]}"#,
        r#"{"ENUM":"a"}"#,
        r#"{"SELECTOR":{}}"#,
        r#"{"SELECTOR":{"count":{"LITERAL":1}}}"#,
        r#"{"PLAYERS":{"players":"NOBODY"}}"#,
        r#"{"SELECTOR":{"ref":"SELF"},"PLAYERS":{"ref":"OWNER"}}"#,
        r#"{"selector":{"ref":"SELF"}}"#,
        r#"{"OBJECTS":{"ref":"SELF"}}"#,
        r#"{}"#,
        r#"null"#,
    ] {
        reject::<ChoiceCandidates>(doc);
    }
    assert!(serde_json::to_string(&ChoiceCandidates::Enum(Vec::new())).is_err());
    assert!(serde_json::to_string(&ChoiceCandidates::Enum(vec!["a".into(), "a".into()])).is_err());
}

/// REQ-DSL-011: the min/max/upTo validation matrix, through JSON and through `validate`.
#[test]
fn req_dsl_011_min_max_up_to_matrix() {
    let base = r#""chooser":{"ref":"CONTROLLER"},"kind":"OBJECTS""#;
    let ok = [
        r#""min":{"LITERAL":0}"#,
        r#""max":{"LITERAL":0}"#,
        r#""min":{"LITERAL":1},"max":{"LITERAL":1}"#,
        r#""min":{"LITERAL":0},"max":{"LITERAL":3}"#,
        r#""min":"X","max":{"LITERAL":0}"#,
        r#""min":{"LITERAL":5},"max":"X""#,
        r#""min":{"MATH":{"op":"SUB","lhs":"X","rhs":{"LITERAL":9}}},"max":{"LITERAL":0}"#,
        r#""upTo":true,"max":{"LITERAL":2}"#,
        r#""upTo":true,"max":"X""#,
        r#""upTo":true,"max":{"LITERAL":0}"#,
        r#""upTo":false"#,
        r#""upTo":false,"min":{"LITERAL":1},"max":{"LITERAL":2}"#,
        r#""upTo":false,"min":{"LITERAL":1}"#,
    ];
    for members in ok {
        accept::<ChoiceSpec>(&format!("{{{base},{members}}}"));
    }
    let bad = [
        r#""min":{"LITERAL":-1}"#,
        r#""max":{"LITERAL":-1}"#,
        r#""min":{"LITERAL":-1},"max":"X""#,
        r#""min":"X","max":{"LITERAL":-3}"#,
        r#""min":{"LITERAL":2},"max":{"LITERAL":1}"#,
        r#""upTo":true"#,
        r#""upTo":true,"min":{"LITERAL":0}"#,
        r#""upTo":true,"min":{"LITERAL":0},"max":{"LITERAL":2}"#,
        r#""upTo":true,"min":"X","max":"X""#,
        r#""min":null"#,
        r#""max":null"#,
        r#""upTo":null"#,
        r#""min":1"#,
        r#""max":{"LITERAL":1.0}"#,
        r#""max":{"LITERAL":1e2}"#,
        r#""max":{"LITERAL":-0.0}"#,
        r#""max":{"LITERAL":9007199254740992}"#,
        r#""min":{"LITERAL":"1"}"#,
        r#""max":{"LITERAL":true}"#,
        r#""max":{"LITERAL":null}"#,
        r#""upTo":"true""#,
        r#""upTo":1"#,
    ];
    for members in bad {
        reject::<ChoiceSpec>(&format!("{{{base},{members}}}"));
    }
    let mut spec = ChoiceSpec::new(controller(), ChoiceKind::Number);
    assert_eq!(spec.validate(), Ok(()));
    spec.up_to = Some(true);
    assert!(spec.validate().is_err());
    assert!(serde_json::to_string(&spec).is_err());
    spec.max = Some(ValueExpressionAST::Literal(3));
    assert_eq!(spec.validate(), Ok(()));
    spec.min = Some(ValueExpressionAST::Literal(0));
    assert!(spec.validate().is_err());
    spec.up_to = None;
    assert_eq!(spec.validate(), Ok(()));
    spec.min = Some(ValueExpressionAST::Literal(4));
    assert!(spec.validate().is_err());
    spec.min = Some(ValueExpressionAST::Literal(-1));
    spec.max = None;
    assert!(spec.validate().is_err());
    spec.min = None;
    spec.max = Some(ValueExpressionAST::Literal(-1));
    assert!(spec.validate().is_err());
    spec.max = None;
    spec.candidates = Some(ChoiceCandidates::Enum(Vec::new()));
    assert!(spec.validate().is_err());
}

/// REQ-DSL-011: `upTo`, `hidden` and `random` true and false are distinct, round-trip and differ
/// from omission; `hidden`/`random`/`revealTo` round-trip.
#[test]
fn req_dsl_011_bool_members_and_reveal() {
    let base = r#""chooser":{"ref":"CONTROLLER"},"kind":"YES_NO""#;
    for member in ["hidden", "random"] {
        let t: ChoiceSpec = accept(&format!("{{{base},\"{member}\":true}}"));
        let f: ChoiceSpec = accept(&format!("{{{base},\"{member}\":false}}"));
        let none: ChoiceSpec = accept(&format!("{{{base}}}"));
        assert_ne!(t, f);
        assert_ne!(f, none);
        assert_ne!(t, none);
        let field = |s: &ChoiceSpec| {
            if member == "hidden" {
                s.hidden
            } else {
                s.random
            }
        };
        assert_eq!(field(&t), Some(true));
        assert_eq!(field(&f), Some(false));
        assert_eq!(field(&none), None);
        reject::<ChoiceSpec>(&format!("{{{base},\"{member}\":null}}"));
        reject::<ChoiceSpec>(&format!("{{{base},\"{member}\":0}}"));
    }
    let up_false: ChoiceSpec = accept(&format!("{{{base},\"upTo\":false}}"));
    assert_eq!(up_false.up_to, Some(false));
    let full: ChoiceSpec = accept(
        r#"{"chooser":{"ref":"TARGET","slot":0},"kind":"OBJECTS","candidates":{"SELECTOR":{"zones":["HAND"]}},"max":{"LITERAL":1},"upTo":true,"hidden":true,"random":false,"revealTo":{"players":"ALL_PLAYERS"}}"#,
    );
    assert_eq!(full.hidden, Some(true));
    assert_eq!(full.random, Some(false));
    assert_eq!(
        full.reveal_to,
        Some(PlayerSelectorAST::Players(PlayerGroup::AllPlayers))
    );
    reject::<ChoiceSpec>(&format!("{{{base},\"revealTo\":null}}"));
    reject::<ChoiceSpec>(&format!("{{{base},\"revealTo\":{{}}}}"));
}

/// REQ-DSL-011: spec-level rejections: missing chooser/kind, unknown members, snake_case
/// member names, unknown kind, duplicate members.
#[test]
fn req_dsl_011_spec_rejections() {
    for doc in [
        r#"{"kind":"MODE"}"#,
        r#"{"chooser":{"ref":"CONTROLLER"}}"#,
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":"mode"}"#,
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":"PICK"}"#,
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":"MODE","extra":1}"#,
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":"MODE","up_to":true}"#,
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":"MODE","reveal_to":{"ref":"OWNER"}}"#,
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":"MODE","candidates":null}"#,
        r#"{"chooser":null,"kind":"MODE"}"#,
        r#"{"chooser":{},"kind":"MODE"}"#,
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":"MODE","kind":"MODE"}"#,
        r#"[]"#,
    ] {
        reject::<ChoiceSpec>(doc);
    }
}

/// REQ-DSL-011, REQ-NFR-006: generated specs satisfy every wire invariant.
#[test]
fn req_dsl_011_property_specs() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f11_0011_0001);
    for _ in 0..400 {
        let spec = gen_spec(&mut rng);
        assert_eq!(spec.validate(), Ok(()), "{spec:?}");
        assert_wire_props(&spec, &mut rng);
        if let Some(c) = &spec.candidates {
            assert_wire_props(c, &mut rng);
        }
    }
}

/// REQ-DSL-011, R12: random bytes and mutated specs never panic the decoders; deep input is an
/// error.
#[test]
fn req_dsl_011_random_and_mutated_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f11_0011_0002);
    for _ in 0..2000 {
        let doc = random_doc(&mut rng);
        decode_never_panics::<ChoiceSpec>(&doc);
        decode_never_panics::<ChoiceCandidates>(&doc);
        decode_never_panics::<ChoiceKind>(&doc);
    }
    for _ in 0..500 {
        let doc = serde_json::to_vec(&gen_spec(&mut rng)).expect("encode");
        for _ in 0..4 {
            let m = mutate(&doc, &mut rng);
            decode_never_panics::<ChoiceSpec>(&m);
        }
    }
    reject::<ChoiceSpec>(&r#"{"a":"#.repeat(100_000));
    reject::<ChoiceCandidates>(&"[".repeat(100_000));
}

fn enum_doc(labels: &[String]) -> String {
    let items: Vec<String> = labels.iter().map(|l| format!("{l:?}")).collect();
    format!(r#"{{"ENUM":[{}]}}"#, items.join(","))
}

/// REQ-DSL-011, REQ-NFR-006: a 50,000-label ENUM decodes (set-based duplicate check), keeps its
/// order through canonical export, and duplicates at either end are rejected.
#[test]
fn req_dsl_011_large_enum_linearithmic() {
    let labels: Vec<String> = (0..50_000).map(|i| format!("opt{i}")).collect();
    let doc = enum_doc(&labels);
    let c: ChoiceCandidates = serde_json::from_str(&doc).expect("candidates");
    assert_eq!(c, ChoiceCandidates::Enum(labels.clone()));
    let spec_doc =
        format!(r#"{{"chooser":{{"ref":"CONTROLLER"}},"kind":"MODE","candidates":{doc}}}"#);
    let spec =
        serde_json::from_str::<ChoiceSpec>(&spec_doc).expect("decode ChoiceSpec with large ENUM");
    let bytes = crate::canonical::to_canonical_vec(&spec).expect("export");
    let back: ChoiceSpec = serde_json::from_slice(&bytes).expect("reload");
    assert_eq!(back, spec);
    assert_eq!(
        back.candidates,
        Some(ChoiceCandidates::Enum(labels.clone()))
    );
    let bytes = crate::canonical::to_canonical_vec(&c).expect("export candidates");
    let back: ChoiceCandidates = serde_json::from_slice(&bytes).expect("reload candidates");
    assert_eq!(back, c);

    let mut last_dup = labels.clone();
    last_dup[49_999] = "opt7".to_owned();
    assert!(serde_json::from_str::<ChoiceCandidates>(&enum_doc(&last_dup)).is_err());
    let mut first_dup = labels;
    first_dup[1] = "opt0".to_owned();
    assert!(serde_json::from_str::<ChoiceCandidates>(&enum_doc(&first_dup)).is_err());
}
