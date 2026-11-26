// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property and robustness tests for `fineprint_dsl::lint::lint_definition` (FP-015,
//! REQ-ING-009; R12 untrusted input). A hand-rolled fixed-seed xorshift generator builds
//! several hundred hostile definitions (random JSON in every wrapper position, random layouts,
//! statuses, costs and contexts); every run must return without panicking, with a report that
//! is sorted and de-duplicated per S-0 and whose locations follow the path grammar. Also a
//! deep-nesting case and an empty `faces` list. Black-box: written from the brief only.
//! Synthetic data only (R5).

#[path = "spec_dsl_lint_common/report.rs"]
mod report;
mod spec_dsl_lint_common;

use std::collections::BTreeSet;

use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, ColorBitmask, CommanderEligibilityDSL, FaceKind, LayoutBehavior,
    PartnerAbilityDSL, SupportStatus, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::lint::{
    LintContext, LintId, SourceCharacteristics, SourceFaceCharacteristics, lint_definition,
};
use fineprint_dsl::mana::{Color, MAX_GENERIC, ManaSymbol, Variable};
use report::*;
use serde_json::{Map, Value, json};
use spec_dsl_lint_common::*;

/// xorshift64* with a fixed seed (R3: no ambient randomness).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn coin(&mut self) -> bool {
        self.below(2) == 0
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[usize::try_from(self.below(xs.len() as u64)).expect("small index")]
    }
}

const KEYS: [&str; 30] = [
    "op",
    "maxIter",
    "kind",
    "type",
    "layer",
    "textSpan",
    "linkIds",
    "ref",
    "linkId",
    "tokenDef",
    "lookBack",
    "look_back",
    "isManaAbility",
    "isLoyalty",
    "targets",
    "trigger",
    "event",
    "mode",
    "effect",
    "effects",
    "cost",
    "costs",
    "mana",
    "match",
    "replaceWith",
    "tierHint",
    "abilityId",
    "body",
    "then",
    "id",
];

const STRINGS: [&str; 32] = [
    "",
    "REPEAT_WHILE",
    "CREATE_TOKEN",
    "SEARCH_LIBRARY",
    "DRAW",
    "MILL",
    "LINKED",
    "ACTIVATED",
    "TRIGGERED",
    "SPELL",
    "STATIC",
    "REPLACEMENT",
    "KEYWORD",
    "CHAR",
    "RULE",
    "PLAYER",
    "ENTERS",
    "RESOLVED",
    "SKIP",
    "PREVENT",
    "COPY_ETB",
    "PAY_MANA",
    "COMPOSITE",
    "LOYALTY",
    "{R}",
    "{2/B}{W/U/P}",
    "{",
    "7b",
    "test.token_a",
    "\u{0}\u{ffff}",
    "test.other",
    "faces[0]",
];

const NUMBERS: [&str; 14] = [
    "0",
    "1",
    "-1",
    "2",
    "4294967295",
    "4294967296",
    "9007199254740991",
    "9007199254740993",
    "18446744073709551615",
    "-9223372036854775808",
    "1.5",
    "-0.0",
    "1e308",
    "5e-324",
];

fn rand_value(rng: &mut Rng, depth: u32) -> Value {
    let choice = if depth == 0 {
        rng.below(4)
    } else {
        rng.below(7)
    };
    match choice {
        0 => Value::Null,
        1 => Value::Bool(rng.coin()),
        2 => num(rng.pick(&NUMBERS)),
        3 => Value::String((*rng.pick(&STRINGS)).to_string()),
        4 => {
            let n = rng.below(4);
            Value::Array((0..n).map(|_| rand_value(rng, depth - 1)).collect())
        }
        _ => {
            let n = rng.below(5);
            let mut m = Map::new();
            for _ in 0..n {
                m.insert((*rng.pick(&KEYS)).to_string(), rand_value(rng, depth - 1));
            }
            Value::Object(m)
        }
    }
}

/// A valid fixture with one member replaced or removed at random, or a fully random value.
fn rand_ability(rng: &mut Rng) -> Value {
    let base = match rng.below(10) {
        0 => good_activated_mana("r"),
        1 => good_triggered_mana("r", "RESOLVED"),
        2 => with(
            &replacement("r", "ENTERS", json!([modify_event()])),
            "tierHint",
            json!("COPY_ETB"),
        ),
        3 => static_ab("r", json!([char_effect(Some("6")), rule_effect(None)])),
        4 => spell(
            "r",
            json!([may(json!([repeat_while(
                Some(json!(2)),
                json!([create_token("x")])
            )]))]),
        ),
        5 => linked(&spell("r", json!([tap_linked("l")])), &["l", "m"]),
        6 => spanned(&keyword("r"), rng.below(8), rng.below(8)),
        7 => activated(
            "r",
            json!([{"cost":"COMPOSITE","costs":[{"cost":"PAY_MANA","mana":"{G}"}]}]),
            json!([search_library()]),
        ),
        _ => return rand_value(rng, 4),
    };
    let mut v = base;
    if let Value::Object(m) = &mut v {
        let k = (*rng.pick(&KEYS)).to_string();
        match rng.below(3) {
            0 => {
                m.remove(&k);
            }
            1 => {
                m.insert(k, rand_value(rng, 3));
            }
            _ => {}
        }
    }
    v
}

fn rand_cost(rng: &mut Rng) -> Option<fineprint_dsl::mana::ManaCostDSL> {
    if rng.below(4) == 0 {
        return None;
    }
    let colors = Color::ALL;
    let n = 1 + rng.below(4);
    let mut syms = Vec::new();
    for _ in 0..n {
        let a = *rng.pick(&colors);
        let b = *rng.pick(&colors);
        let s = match rng.below(10) {
            0 => ManaSymbol::Generic(u32::try_from(rng.below(u64::from(MAX_GENERIC) + 1)).unwrap()),
            1 => ManaSymbol::Colored(a),
            2 => ManaSymbol::Colorless,
            3 => ManaSymbol::Snow,
            4 => ManaSymbol::Variable(*rng.pick(&Variable::ALL)),
            5 if a != b => ManaSymbol::Hybrid(a, b),
            6 => ManaSymbol::GenericHybrid(1 + u32::try_from(rng.below(5)).unwrap(), a),
            7 => ManaSymbol::Phyrexian(a),
            8 if a != b => ManaSymbol::HybridPhyrexian(a, b),
            9 => ManaSymbol::ColorlessHybrid(a),
            _ => ManaSymbol::Colored(b),
        };
        if s.is_valid() {
            syms.push(s);
        }
    }
    if syms.is_empty() {
        None
    } else {
        Some(mana(syms))
    }
}

fn rand_bits(rng: &mut Rng) -> ColorBitmask {
    bits(u8::try_from(rng.below(32)).unwrap())
}

fn rand_def(rng: &mut Rng) -> (CardDefinitionDSL, LintContext) {
    let nf = rng.below(4);
    let mut faces = Vec::new();
    for _ in 0..nf {
        let mut f = face(*rng.pick(&FaceKind::ALL), "Test Random");
        f.mana_cost = rand_cost(rng);
        if rng.coin() {
            f.color_indicator = Some(rand_bits(rng));
        }
        if rng.coin() {
            f.power = Some(ValueExpressionAST(if rng.coin() {
                json!({"LITERAL": 1})
            } else {
                rand_value(rng, 3)
            }));
        }
        if rng.coin() {
            f.toughness = Some(ValueExpressionAST(rand_value(rng, 3)));
        }
        let nt = rng.below(8);
        f.text = TextTokenStream(
            (0..nt)
                .map(|i| {
                    if rng.below(5) == 0 {
                        rand_value(rng, 2)
                    } else {
                        lit(&format!("t{i}"), MARKER)
                    }
                })
                .collect(),
        );
        let na = rng.below(6);
        f.abilities = (0..na).map(|_| AbilityAST(rand_ability(rng))).collect();
        faces.push(f);
    }
    let mut d = def_of(*rng.pick(&LayoutBehavior::ALL), faces);
    d.support_status = *rng.pick(&SupportStatus::ALL);
    d.color_identity = rand_bits(rng);
    if rng.coin() {
        let n = rng.below(4);
        d.related_tokens = Some((0..n).map(|_| (*rng.pick(&STRINGS)).to_string()).collect());
    }
    if rng.coin() {
        let n = rng.below(4);
        d.commander_eligibility = Some(CommanderEligibilityDSL {
            eligible: rng.coin(),
            partner_abilities: (0..n)
                .map(|_| {
                    PartnerAbilityDSL(if rng.coin() {
                        json!({"abilityId":"p","kind":"KEYWORD","keyword":"PARTNER"})
                    } else {
                        rand_value(rng, 3)
                    })
                })
                .collect(),
        });
    }
    let source = if rng.coin() {
        let n = rng.below(5);
        Some(SourceCharacteristics {
            faces: (0..n)
                .map(|_| SourceFaceCharacteristics {
                    mana_value: rng.coin().then(|| {
                        if rng.coin() {
                            u32::MAX
                        } else {
                            u32::try_from(rng.below(20)).unwrap()
                        }
                    }),
                    colors: rng.coin().then(|| rand_bits(rng)),
                })
                .collect(),
            color_identity: rng.coin().then(|| rand_bits(rng)),
        })
    } else {
        None
    };
    let known_token_ids = rng.coin().then(|| {
        let n = rng.below(3);
        (0..n)
            .map(|_| (*rng.pick(&STRINGS)).to_string())
            .collect::<BTreeSet<String>>()
    });
    (
        d,
        LintContext {
            source,
            known_token_ids,
        },
    )
}

/// REQ-ING-009 (R12, S-0): 600 hostile definitions; no panic, sorted, de-duplicated, paths in
/// the grammar with consistent indices, pure.
#[test]
fn req_ing_009_random_definitions_total_and_ordered() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut seen: BTreeSet<LintId> = BTreeSet::new();
    for _ in 0..600 {
        let (d, ctx) = rand_def(&mut rng);
        let r = run(&d, &ctx); // checks the invariants and purity
        for l in &r.lints {
            seen.insert(l.id);
            assert!(LintId::ALL.contains(&l.id));
            assert!(!l.message.contains(MARKER), "message copies text: {l}");
        }
    }
    // The generator exercises most lints (sanity of the generator itself).
    assert!(seen.len() >= 8, "generator too tame: {seen:?}");
}

/// REQ-ING-009 (R12): random raw values in every wrapper position, one position at a time.
#[test]
fn req_ing_009_random_values_in_each_position() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for _ in 0..300 {
        let mut d = clean_def();
        let v = rand_value(&mut rng, 5);
        match rng.below(6) {
            0 => d.faces[0].abilities[1] = AbilityAST(v),
            1 => d.faces[1].text.0.push(v),
            2 => d.faces[0].power = Some(ValueExpressionAST(v)),
            3 => d.faces[1].toughness = Some(ValueExpressionAST(v)),
            4 => {
                d.commander_eligibility
                    .as_mut()
                    .expect("clean_def has partners")
                    .partner_abilities
                    .push(PartnerAbilityDSL(v));
            }
            _ => d.faces[1].abilities.push(AbilityAST(v)),
        }
        run0(&d);
    }
}

/// REQ-ING-009 (R12): an ability nested a few thousand JSON levels deep does not crash the lint.
/// 700 instruction levels are about 2100 nested JSON containers; deeper values overflow the
/// default test-thread stack in serde_json's own recursive `Drop` (measured: dropping 1000
/// instruction levels, or 2000 bare array/object levels, aborts), which the brief says to avoid.
#[test]
fn req_ing_009_deep_nesting() {
    let depth = 700;
    let mut inner = json!({"op":"REPEAT_WHILE","cond":{"pred":"TAPPED"},"body":[draw()]});
    for i in 0..depth {
        inner = if i % 2 == 0 {
            json!({"op":"IF","cond":{"pred":"TAPPED"},"then":[inner]})
        } else {
            json!({"op":"MAY","player":{"ref":"CONTROLLER"},"body":[inner, {"lookBack": true}]})
        };
    }
    let ab_v = spell("deep", json!([inner]));
    let d = def_abilities(vec![ab_v]);
    let r = lint_definition(&d, &ctx0());
    assert_invariants(&d, &r);
    // The innermost REPEAT_WHILE has no maxIter and look-back keys are present.
    assert_at(
        &r,
        LintId::MAX_ITER,
        fineprint_dsl::lint::Severity::Error,
        "faces[0].abilities[0]",
    );
    assert_at(
        &r,
        LintId::LOOK_BACK,
        fineprint_dsl::lint::Severity::Error,
        "faces[0].abilities[0]",
    );
    // Deeply nested arrays and objects of plain data as well.
    let mut raw = Value::Null;
    for i in 0..1200 {
        raw = if i % 2 == 0 {
            Value::Array(vec![raw])
        } else {
            json!({"k": raw})
        };
    }
    let mut d2 = def_abilities(vec![raw.clone()]);
    d2.faces[0].text = TextTokenStream(vec![raw.clone()]);
    d2.faces[0].power = Some(ValueExpressionAST(raw));
    let r2 = lint_definition(&d2, &ctx0());
    assert_invariants(&d2, &r2);
    assert_at(
        &r2,
        LintId::SCHEMA,
        fineprint_dsl::lint::Severity::Error,
        "faces[0].abilities[0]",
    );
}

/// REQ-ING-009 (R12): an empty `faces` list is linted without panicking.
#[test]
fn req_ing_009_empty_faces() {
    let mut d = def_of(LayoutBehavior::Normal, vec![]);
    let r = run0(&d);
    assert!(
        r.lints.iter().all(|l| l.location.face_index.is_none()),
        "{}",
        dump(&r)
    );
    d.color_identity = ColorBitmask::GREEN;
    d.related_tokens = Some(vec!["test.token_zz".to_string()]);
    let ctx = LintContext {
        source: Some(SourceCharacteristics {
            faces: vec![SourceFaceCharacteristics {
                mana_value: Some(3),
                colors: Some(ColorBitmask::RED),
            }],
            color_identity: Some(ColorBitmask::BLUE),
        }),
        known_token_ids: Some(BTreeSet::new()),
    };
    let r = run(&d, &ctx);
    // Declared green is not derivable from no faces: a Warning at "$".
    assert_at(
        &r,
        LintId::COLOR_IDENTITY,
        fineprint_dsl::lint::Severity::Warning,
        "$",
    );
    assert_at(
        &r,
        LintId::TOKEN_REF,
        fineprint_dsl::lint::Severity::Error,
        "relatedTokens[0]",
    );
    assert_none(&r, LintId::MANA_VALUE);
    assert_none(&r, LintId::COLORS);
}
