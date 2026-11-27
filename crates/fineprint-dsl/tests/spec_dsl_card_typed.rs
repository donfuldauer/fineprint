// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Black-box spec tests for #232 FP-164: the card AST positions hold the real typed ASTs and
//! nothing else. Covers REQ-DSL-003 (value expressions), REQ-DSL-010 (abilities and their
//! instructions), REQ-DSL-012 (text token streams) and REQ-NFR-006 (canonical JSON and
//! `definitionHash`). Written from the work-item brief only (contract C1-C5, C7); every
//! definition here is SYNTHETIC (R5) and every pseudo-random input comes from a fixed-seed
//! generator (R3, R7). No CR rule is implemented here, so none is cited (R6).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::sha256;
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::canonical::{CanonicalError, MAX_DEPTH, MAX_SAFE_INTEGER, canonicalize};
use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, CardDefinitionError, CardFaceDSL, ColorBitmask,
    CommanderEligibilityDSL, FaceKind, LayoutBehavior, Legality, Origin, PartnerAbilityDSL,
    ProvenanceDSL, SupportStatus, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::text::{NameRefKind, RewritableCategory, TextToken};
use serde_json::{Value, json};

// ---------------------------------------------------------------- (a) type identity (C1)

fn takes_expr(_: fineprint_dsl::expr::ValueExpressionAST) {}
fn takes_text(_: fineprint_dsl::text::TextTokenStream) {}
fn takes_ability(_: fineprint_dsl::ability::AbilityAST) {}

/// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012 (C1): the three `card::` names are the real types, used
/// at compile time where the real type is expected, built with the real constructors (no tuple
/// syntax), and the face fields hold them (C3).
#[test]
fn req_dsl_003_010_012_card_reexports_are_the_real_types() {
    let v: ValueExpressionAST = ValueExpressionAST::Literal(2);
    takes_expr(v.clone());
    let t: TextTokenStream = TextTokenStream::default();
    takes_text(t.clone());
    let t2: TextTokenStream = TextTokenStream::new(vec![TextToken::Literal {
        id: "t0".into(),
        text: "test text".into(),
    }])
    .unwrap();
    takes_text(t2.clone());
    let a: AbilityAST = serde_json::from_value(kw("a0", "TEST_KEYWORD")).unwrap();
    takes_ability(a.clone());
    // The other direction: real values go into the card positions.
    let mut f = plain_face(FaceKind::Main, "Test Face");
    f.power = Some(fineprint_dsl::expr::ValueExpressionAST::X);
    f.toughness = Some(v);
    f.text = t2;
    f.abilities = vec![a.clone()];
    let p = PartnerAbilityDSL::new(a).unwrap();
    let real: &fineprint_dsl::ability::AbilityAST = p.as_ability();
    assert_eq!(real.kind(), "KEYWORD");
    assert_eq!(
        serde_json::to_value(&f.toughness).unwrap(),
        json!({"LITERAL": 2})
    );
    assert_eq!(serde_json::to_value(&f.power).unwrap(), json!("X"));
}

// ---------------------------------------------------------------- synthetic fixtures

fn kw(id: &str, keyword: &str) -> Value {
    json!({"abilityId": id, "kind": "KEYWORD", "keyword": keyword})
}

fn lit_tok(id: &str, text: &str) -> Value {
    json!({"id": id, "kind": "LITERAL", "text": text})
}

/// One valid ability of each of the six kinds (C3; effect lists use real instructions).
fn ability_of_kind(kind: &str, id: &str) -> Value {
    match kind {
        "SPELL" => json!({"abilityId": id, "kind": "SPELL", "effect": [{"op": "POPULATE"}]}),
        "ACTIVATED" => json!({"abilityId": id, "kind": "ACTIVATED",
            "cost": [{"cost": "TAP_SOURCE"}], "effect": []}),
        "TRIGGERED" => json!({"abilityId": id, "kind": "TRIGGERED",
            "trigger": {"mode": "STATE", "state": {"pred": "IS_CREATURE"}},
            "effect": [{"op": "POPULATE"}]}),
        "STATIC" => json!({"abilityId": id, "kind": "STATIC",
            "effects": [{"type": "RULE", "kind": "SKIP"}]}),
        "REPLACEMENT" => json!({"abilityId": id, "kind": "REPLACEMENT",
            "match": {"event": "DIES", "affected": {"ref": "SELF"}},
            "replaceWith": [{"op": "SKIP"}]}),
        "KEYWORD" => kw(id, "TEST_KEYWORD"),
        other => panic!("unknown kind {other}"),
    }
}

fn plain_face(kind: FaceKind, name: &str) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: kind,
        name: name.to_string(),
        mana_cost: None,
        color_indicator: None,
        supertypes: vec![],
        card_types: vec![],
        subtypes: vec![],
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: TextTokenStream::default(),
        abilities: vec![],
    }
}

fn definition(faces: Vec<CardFaceDSL>) -> CardDefinitionDSL {
    let mut legalities = BTreeMap::new();
    legalities.insert("testformat".to_string(), Legality::Legal);
    CardDefinitionDSL {
        card_def_id: "test.typed_ast".to_string(),
        dsl_version: "1.0".to_string(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "0000".to_string(),
            origin: Origin::Parser,
            parser_version: None,
            override_source: None,
            verified_by: None,
            rules_baseline: "test-baseline".to_string(),
        },
        support_status: SupportStatus::AutoParsed,
        known_gaps: None,
        layout: LayoutBehavior::Normal,
        faces,
        meld_partner: None,
        color_identity: ColorBitmask::NONE,
        commander_eligibility: None,
        legalities,
        digital_only: false,
        related_tokens: None,
    }
}

/// A valid definition with content in all five AST positions: one face with power, toughness,
/// a three-kind text stream and one ability of every kind, and two partner abilities.
fn full() -> CardDefinitionDSL {
    let mut f = plain_face(FaceKind::Main, "Test Typed");
    f.mana_cost = Some(ManaCostDSL::parse("{2}{G}").unwrap());
    f.card_types = vec!["Creature".to_string()];
    f.power = Some(serde_json::from_value(json!({"LITERAL": 2})).unwrap());
    f.toughness = Some(
        serde_json::from_value(json!({"MATH": {"op": "ADD", "lhs": "X",
            "rhs": {"LITERAL": 1}}}))
        .unwrap(),
    );
    f.text = serde_json::from_value(json!([
        lit_tok("t0", "Test words"),
        {"id": "t1", "kind": "WORD", "category": "COLOR_WORD", "value": "Hue"},
        {"id": "t2", "kind": "NAME_REF", "ref": "SELF"},
    ]))
    .unwrap();
    f.abilities = AbilityAST::KINDS
        .iter()
        .enumerate()
        .map(|(i, k)| serde_json::from_value(ability_of_kind(k, &format!("a{i}"))).unwrap())
        .collect();
    let mut d = definition(vec![f]);
    d.commander_eligibility = Some(CommanderEligibilityDSL {
        eligible: true,
        partner_abilities: vec![
            serde_json::from_value(kw("p0", "PARTNER")).unwrap(),
            serde_json::from_value(json!({"abilityId": "p1", "kind": "KEYWORD",
                "keyword": "PARTNER", "params": {"with": "test.partner_other"}}))
            .unwrap(),
        ],
    });
    d
}

fn sealed(d: CardDefinitionDSL) -> CardDefinitionDSL {
    d.seal().unwrap_or_else(|e| panic!("seal: {e:?}"))
}

fn load(bytes: &[u8]) -> Result<CardDefinitionDSL, CardDefinitionError> {
    CardDefinitionDSL::from_json(bytes)
}

/// Independent hash oracle: RFC 8785 + SHA-256 over the document without `definitionHash`.
fn oracle_hash(v: &Value) -> String {
    let mut v = v.clone();
    v.as_object_mut().unwrap().remove("definitionHash");
    sha256(&canonicalize(&serde_json::to_vec(&v).unwrap()).unwrap()).to_hex()
}

/// `v` with `definitionHash` set to the oracle digest of its (patched) content.
fn redigested(mut v: Value) -> Value {
    let h = oracle_hash(&v);
    v["definitionHash"] = json!(h);
    v
}

/// The sealed `full()` document with the JSON-pointer slot `ptr` set to `x` (an array index
/// equal to the length appends) and a correct digest.
fn doc_with(ptr: &str, x: Value) -> Value {
    let mut v = serde_json::to_value(sealed(full())).unwrap();
    let (parent, last) = ptr.rsplit_once('/').unwrap();
    match v.pointer_mut(parent).unwrap() {
        Value::Array(items) => {
            let i: usize = last.parse().unwrap();
            if i == items.len() {
                items.push(x);
            } else {
                items[i] = x;
            }
        }
        Value::Object(m) => {
            m.insert(last.to_string(), x);
        }
        other => panic!("{ptr}: parent {other}"),
    }
    redigested(v)
}

// ---------------------------------------------------------------- (b) plain serde (C3)

/// The invalid contents of C3 per position: `(JSON pointer into a sealed full() document, the
/// value, what it is)`.
fn invalid_cases() -> Vec<(&'static str, Value, &'static str)> {
    let mut unknown_member = kw("a9", "TEST_KEYWORD");
    unknown_member["zzzUnknown"] = json!(1);
    vec![
        ("/faces/0/power", Value::Null, "present null power"),
        ("/faces/0/toughness", Value::Null, "present null toughness"),
        ("/faces/0/power", json!(2), "bare number power"),
        ("/faces/0/toughness", json!(-3), "bare number toughness"),
        (
            "/faces/0/text/3",
            lit_tok("t0", "duplicate id"),
            "duplicate token id",
        ),
        (
            "/faces/0/abilities/6",
            json!({"abilityId": "a9", "kind": "TEST_KIND"}),
            "unknown ability kind",
        ),
        ("/faces/0/abilities/6", unknown_member, "unknown member"),
        (
            "/commanderEligibility/partnerAbilities/2",
            ability_of_kind("STATIC", "p9"),
            "non-KEYWORD partner",
        ),
    ]
}

/// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012 (C3): plain serde of each typed position refuses the
/// invalid content and accepts valid content; a face or a whole definition holding the invalid
/// content does not decode through plain serde either (it did before #232).
#[test]
fn req_dsl_003_010_012_plain_serde_rejects_invalid_positions() {
    // Per type.
    assert!(serde_json::from_value::<ValueExpressionAST>(json!(2)).is_err());
    assert!(serde_json::from_value::<ValueExpressionAST>(Value::Null).is_err());
    assert!(
        serde_json::from_value::<TextTokenStream>(json!([lit_tok("t0", "a"), lit_tok("t0", "b")]))
            .is_err()
    );
    assert!(
        serde_json::from_value::<AbilityAST>(json!({"abilityId": "a", "kind": "TEST_KIND"}))
            .is_err()
    );
    let mut unknown = kw("a", "TEST_KEYWORD");
    unknown["zzzUnknown"] = json!(1);
    assert!(serde_json::from_value::<AbilityAST>(unknown).is_err());
    for k in AbilityAST::KINDS.iter().filter(|k| **k != "KEYWORD") {
        let v = ability_of_kind(k, "p");
        assert!(
            serde_json::from_value::<AbilityAST>(v.clone()).is_ok(),
            "{k}"
        );
        assert!(
            serde_json::from_value::<PartnerAbilityDSL>(v).is_err(),
            "{k} partner"
        );
    }
    // Valid content is accepted per type.
    assert_eq!(
        serde_json::from_value::<ValueExpressionAST>(json!({"LITERAL": 2})).unwrap(),
        ValueExpressionAST::Literal(2)
    );
    assert_eq!(
        serde_json::from_value::<ValueExpressionAST>(json!("X")).unwrap(),
        ValueExpressionAST::X
    );
    assert_eq!(
        serde_json::from_value::<TextTokenStream>(json!([lit_tok("t0", "a"), lit_tok("t1", "b")]))
            .unwrap()
            .len(),
        2
    );
    assert!(serde_json::from_value::<PartnerAbilityDSL>(kw("p", "PARTNER")).is_ok());
    // Per position of a face and of a definition.
    let good = serde_json::to_value(sealed(full())).unwrap();
    assert_eq!(
        serde_json::from_value::<CardDefinitionDSL>(good.clone()).unwrap(),
        sealed(full())
    );
    for (ptr, x, what) in invalid_cases() {
        let doc = doc_with(ptr, x);
        assert!(
            serde_json::from_value::<CardFaceDSL>(doc["faces"][0].clone()).is_err()
                || !ptr.starts_with("/faces"),
            "{what}: face decoded"
        );
        assert!(
            serde_json::from_value::<CardDefinitionDSL>(doc).is_err(),
            "{what}: definition decoded"
        );
    }
}

// ---------------------------------------------------------------- (c) from_json (C4)

/// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012, REQ-NFR-006 (C4): `from_json` refuses every invalid
/// position of C3 with `Schema`, including `Some(null)` power and toughness, with a correctly
/// recomputed digest and with a stale one (schema before hash); the unpatched document loads.
#[test]
fn req_dsl_003_010_012_nfr_006_from_json_schema_per_position() {
    let good = serde_json::to_value(sealed(full())).unwrap();
    assert_eq!(
        load(&serde_json::to_vec(&good).unwrap()),
        Ok(sealed(full()))
    );
    for (ptr, x, what) in invalid_cases() {
        let doc = doc_with(ptr, x);
        let r = load(&serde_json::to_vec(&doc).unwrap());
        assert!(
            matches!(r, Err(CardDefinitionError::Schema(_))),
            "{what}: {r:?}"
        );
        let mut stale = doc.clone();
        stale["definitionHash"] = good["definitionHash"].clone();
        let r = load(&serde_json::to_vec(&stale).unwrap());
        assert!(
            matches!(r, Err(CardDefinitionError::Schema(_))),
            "{what} (stale digest): {r:?}"
        );
    }
    // Some(null) is rejected explicitly in both members; absent members load.
    for member in ["power", "toughness"] {
        let mut v = good.clone();
        v["faces"][0][member] = Value::Null;
        let r = load(&serde_json::to_vec(&redigested(v)).unwrap());
        assert!(matches!(r, Err(CardDefinitionError::Schema(_))), "{member}");
        let mut v = good.clone();
        v["faces"][0].as_object_mut().unwrap().remove(member);
        let d = load(&serde_json::to_vec(&redigested(v)).unwrap()).expect("absent member loads");
        // Exactly the removed member is absent; the other keeps its original value.
        let original = &sealed(full()).faces[0];
        if member == "power" {
            assert_eq!(d.faces[0].power, None, "{member} removed");
            assert!(original.toughness.is_some(), "the fixture has a toughness");
            assert_eq!(d.faces[0].toughness, original.toughness, "{member} removed");
        } else {
            assert_eq!(d.faces[0].toughness, None, "{member} removed");
            assert!(original.power.is_some(), "the fixture has a power");
            assert_eq!(d.faces[0].power, original.power, "{member} removed");
        }
    }
    // An unsafe LITERAL is a document-level error before any AST check (decision #159 FP-140).
    let mut v = good.clone();
    v["faces"][0]["power"] = json!({"LITERAL": 9_007_199_254_740_992_i64});
    assert!(matches!(
        load(&serde_json::to_vec(&v).unwrap()),
        Err(CardDefinitionError::Canonical(
            CanonicalError::UnsafeInteger(_)
        ))
    ));
}

// ---------------------------------------------------------------- (d) PartnerAbilityDSL (C2)

/// REQ-DSL-010 (C2): `PartnerAbilityDSL::new` accepts a KEYWORD ability and refuses every other
/// kind with the exact message; `as_ability`/`into_ability` give the ability back; plain serde
/// refuses non-KEYWORD partners and serialises transparently; Clone, Debug, PartialEq, Eq.
#[test]
fn req_dsl_010_partner_ability_newtype() {
    let a: AbilityAST = serde_json::from_value(kw("p0", "PARTNER")).unwrap();
    let p = PartnerAbilityDSL::new(a.clone()).expect("KEYWORD accepted");
    assert_eq!(p.as_ability(), &a);
    assert_eq!(p.clone().into_ability(), a);
    assert_eq!(p.clone(), p);
    assert!(!format!("{p:?}").is_empty());
    assert_eq!(
        serde_json::to_value(&p).unwrap(),
        serde_json::to_value(&a).unwrap()
    );
    assert_eq!(serde_json::to_value(&p).unwrap(), kw("p0", "PARTNER"));
    let back: PartnerAbilityDSL = serde_json::from_value(kw("p0", "PARTNER")).unwrap();
    assert_eq!(back, p);
    let other = PartnerAbilityDSL::new(serde_json::from_value(kw("p1", "PARTNER")).unwrap());
    assert_ne!(other.unwrap(), p);
    let mut kinds = BTreeSet::new();
    for kind in AbilityAST::KINDS {
        let v = ability_of_kind(kind, "p9");
        let ability: AbilityAST = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(ability.kind(), kind);
        kinds.insert(kind);
        let r = PartnerAbilityDSL::new(ability.clone());
        let de = serde_json::from_value::<PartnerAbilityDSL>(v);
        if kind == "KEYWORD" {
            assert_eq!(r.as_ref().map(PartnerAbilityDSL::as_ability), Ok(&ability));
            assert_eq!(de.unwrap(), r.unwrap());
        } else {
            assert_eq!(
                r,
                Err(format!(
                    "a partner ability must be a KEYWORD ability, found kind {kind}"
                ))
            );
            assert!(de.is_err(), "{kind}: deserialized");
        }
    }
    assert_eq!(kinds.len(), 6);
}

// ---------------------------------------------------------------- (e) round trip (T5 property)

/// Fixed-seed generator of valid typed content (R3, R7: no clock, no OS randomness).
struct Gen(Pcg64Dxsm);

impl Gen {
    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    fn int(&mut self) -> i64 {
        match self.below(4) {
            0 => MAX_SAFE_INTEGER,
            1 => -MAX_SAFE_INTEGER,
            _ => i64::try_from(self.below(41)).unwrap() - 20,
        }
    }

    fn word(&mut self) -> String {
        const PARTS: [&str; 8] = ["a", "Te", "st", " ", "é", "\u{1F602}", "\"", "\\"];
        let n = 1 + self.below(5);
        let s: String = (0..n)
            .map(|_| PARTS[usize::try_from(self.below(8)).unwrap()])
            .collect();
        format!("test {s}")
    }

    fn value(&mut self, depth: u32) -> Value {
        let k = if depth == 0 {
            self.below(2)
        } else {
            self.below(6)
        };
        match k {
            0 => json!({"LITERAL": self.int()}),
            1 => json!("X"),
            2 => {
                const OPS: [&str; 7] = ["ADD", "SUB", "MUL", "DIV_FLOOR", "DIV_CEIL", "MIN", "MAX"];
                let op = OPS[usize::try_from(self.below(7)).unwrap()];
                json!({"MATH": {"op": op, "lhs": self.value(depth - 1),
                    "rhs": self.value(depth - 1)}})
            }
            3 => json!({"COUNT": {"zones": ["BATTLEFIELD"], "pred": "IS_CREATURE"}}),
            4 => json!({"ATTR": {"ref": "SELF", "attr": "POWER"}}),
            _ => json!({"PLAYER_ATTR": {"ref": "CONTROLLER", "attr": "LIFE"}}),
        }
    }

    fn text(&mut self) -> Vec<Value> {
        const CATS: [&str; 5] = [
            "COLOR_WORD",
            "BASIC_LAND_TYPE",
            "CREATURE_TYPE",
            "NUMBER",
            "CARD_NAME",
        ];
        (0..self.below(6))
            .map(|i| {
                let id = format!("t{i}");
                match self.below(4) {
                    0 => lit_tok(&id, &self.word()),
                    1 => json!({"id": id, "kind": "WORD",
                        "category": CATS[usize::try_from(self.below(5)).unwrap()],
                        "value": self.word()}),
                    2 => json!({"id": id, "kind": "NAME_REF", "ref": "SELF"}),
                    _ => json!({"id": id, "kind": "NAME_REF", "ref": "OTHER",
                        "name": self.word()}),
                }
            })
            .collect()
    }

    fn ability(&mut self, id: &str) -> Value {
        let kind = AbilityAST::KINDS[usize::try_from(self.below(6)).unwrap()];
        let mut v = ability_of_kind(kind, id);
        if kind == "KEYWORD" && self.below(2) == 0 {
            v["params"] = json!({"n": self.int(), "s": self.word()});
        }
        v
    }

    fn definition(&mut self) -> CardDefinitionDSL {
        let faces = (0..1 + self.below(2))
            .map(|f| {
                let mut face = plain_face(
                    if f == 0 {
                        FaceKind::Main
                    } else {
                        FaceKind::Back
                    },
                    &format!("Test Face {f}"),
                );
                if self.below(4) != 0 {
                    face.power = Some(serde_json::from_value(self.value(4)).unwrap());
                }
                if self.below(4) != 0 {
                    face.toughness = Some(serde_json::from_value(self.value(4)).unwrap());
                }
                face.text = serde_json::from_value(Value::Array(self.text())).unwrap();
                face.abilities = (0..self.below(5))
                    .map(|i| serde_json::from_value(self.ability(&format!("a{f}_{i}"))).unwrap())
                    .collect();
                face
            })
            .collect::<Vec<_>>();
        let mut d = definition(faces);
        if d.faces.len() == 2 {
            d.layout = LayoutBehavior::DfcNonmodal;
        }
        if self.below(2) == 0 {
            d.commander_eligibility = Some(CommanderEligibilityDSL {
                eligible: self.below(2) == 0,
                partner_abilities: (0..self.below(3))
                    .map(|i| {
                        let mut v = kw(&format!("p{i}"), "PARTNER");
                        if self.below(2) == 0 {
                            v["params"] = json!({"with": self.word()});
                        }
                        serde_json::from_value(v).unwrap()
                    })
                    .collect(),
            });
        }
        d
    }
}

/// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012, REQ-NFR-006 (property, C4): for 400 generated valid
/// definitions (expressions to depth 4, text streams with all three token kinds, abilities of
/// every kind, commander definitions with partner abilities) `from_json(to_canonical_json(
/// seal(d))) == seal(d)`, the digest verifies, equals the independent oracle, and re-sealing is
/// the identity. The generator is checked to cover every ability kind and every token kind.
#[test]
fn req_dsl_003_010_012_nfr_006_property_round_trip() {
    let mut g = Gen(Pcg64Dxsm::from_u64_seed(0x0232_F164_0000_0001));
    let mut kinds = BTreeSet::new();
    let mut tokens = BTreeSet::new();
    let mut partners = 0usize;
    for i in 0..400 {
        let d = g.definition();
        for f in &d.faces {
            for a in &f.abilities {
                kinds.insert(a.kind());
            }
            for t in f.text.tokens() {
                tokens.insert(match t {
                    TextToken::Literal { .. } => "LITERAL",
                    TextToken::Word { .. } => "WORD",
                    TextToken::NameRef { .. } => "NAME_REF",
                });
            }
        }
        partners += d
            .commander_eligibility
            .as_ref()
            .map_or(0, |c| c.partner_abilities.len());
        let s = d
            .clone()
            .seal()
            .unwrap_or_else(|e| panic!("def {i}: {e:?}"));
        let bytes = s.to_canonical_json().unwrap();
        assert_eq!(load(&bytes), Ok(s.clone()), "def {i}: round trip");
        assert_eq!(s.verify_hash(), Ok(()), "def {i}");
        assert_eq!(s.compute_hash().unwrap(), s.definition_hash, "def {i}");
        assert_eq!(
            oracle_hash(&serde_json::to_value(&d).unwrap()),
            s.definition_hash,
            "def {i}: oracle"
        );
        assert_eq!(sealed(s.clone()), s, "def {i}: re-seal");
    }
    assert_eq!(kinds.len(), 6, "{kinds:?}");
    assert_eq!(tokens.len(), 3, "{tokens:?}");
    assert!(partners > 0);
}

// ---------------------------------------------------------------- (f) canonical form

/// Writes `v` as JSON text with every object's members in REVERSE sorted order (a valid but
/// non-canonical member order).
fn reversed_text(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            keys.reverse();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap(),
                        reversed_text(&m[k])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(reversed_text).collect();
            format!("[{}]", parts.join(","))
        }
        other => serde_json::to_string(other).unwrap(),
    }
}

/// Checks that the JSON text `bytes` (the canonical form of `v`) lists every object's members in
/// RFC 8785 order: it equals the output of an independent RFC 8785 implementation for the same
/// value, and canonicalising it again is the identity.
fn assert_members_sorted(v: &Value, bytes: &[u8]) {
    // RFC 8785 output equals the independent canonicaliser's output for the same value.
    let independent = serde_json_canonicalizer::to_vec(v).unwrap();
    assert_eq!(bytes, independent.as_slice());
    // And canonicalising it again is the identity.
    assert_eq!(canonicalize(bytes).unwrap(), bytes);
}

/// REQ-NFR-006 (C4, C7): the canonical JSON of a sealed definition with typed ASTs is RFC 8785
/// (members sorted, equal to an independent canonicaliser), the canonical JSON of each typed
/// AST equals its wire form, `definitionHash` is SHA-256 of the canonical bytes without
/// `definitionHash`, and two input documents that differ only in AST member order load to the
/// same definition with the same hash.
#[test]
fn req_nfr_006_canonical_form_and_member_order() {
    let s = sealed(full());
    let bytes = s.to_canonical_json().unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_members_sorted(&v, &bytes);
    let mut without = v.clone();
    without.as_object_mut().unwrap().remove("definitionHash");
    let without_bytes = serde_json_canonicalizer::to_vec(&without).unwrap();
    assert_eq!(sha256(&without_bytes).to_hex(), s.definition_hash);
    assert_eq!(s.compute_hash().unwrap(), s.definition_hash);
    // Each typed AST's canonical JSON equals its wire form.
    let face = &s.faces[0];
    for (wire, typed) in [
        (
            serde_json::to_value(face.power.as_ref().unwrap()).unwrap(),
            &v["faces"][0]["power"],
        ),
        (
            serde_json::to_value(&face.text).unwrap(),
            &v["faces"][0]["text"],
        ),
        (
            serde_json::to_value(&face.abilities).unwrap(),
            &v["faces"][0]["abilities"],
        ),
    ] {
        assert_eq!(&wire, typed);
        assert_eq!(
            fineprint_dsl::canonical::to_canonical_vec(&wire).unwrap(),
            serde_json_canonicalizer::to_vec(typed).unwrap()
        );
    }
    // Reverse member order everywhere (ASTs included): same definition, same hash.
    let reversed = reversed_text(&v);
    assert_ne!(reversed.as_bytes(), bytes.as_slice());
    let loaded = load(reversed.as_bytes()).expect("member order does not matter");
    assert_eq!(loaded, s);
    assert_eq!(sealed(loaded).definition_hash, s.definition_hash);
    // The same for a reversed AST alone in an otherwise canonical document.
    let math = r#"{"MATH":{"lhs":"X","op":"ADD","rhs":{"LITERAL":1}}}"#;
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains(math), "{text}");
    let doc = text.replacen(
        math,
        r#"{"MATH":{"rhs":{"LITERAL":1},"op":"ADD","lhs":"X"}}"#,
        1,
    );
    assert_eq!(load(doc.as_bytes()), Ok(s.clone()));
}

// ---------------------------------------------------------------- (g) non-canonical spelling

/// REQ-NFR-006, REQ-DSL-010 (C4): `from_json` never keeps a document whose decoded value would
/// re-encode differently (lossless decode). The real AST types have a single spelling (module
/// docs of `expr`, `ability`, `text`: unknown members, present `null` optionals and positional
/// members are refused), so the documented candidates for a second spelling are each either
/// refused with `Schema` or reproduced exactly; and a positional (array) spelling of the object
/// that holds the partner abilities, which a derived decoder may accept but never reproduces,
/// is refused with `Schema` (docs/DECISIONS.md "#232 FP-164 — Spec-test migration").
#[test]
fn req_nfr_006_non_canonical_spellings_are_refused() {
    let base = serde_json::to_value(sealed(full())).unwrap();
    // Candidate second spellings at AST positions.
    let mut activated_instant = ability_of_kind("ACTIVATED", "a1");
    activated_instant["timing"] = json!("INSTANT");
    let mut kw_empty_params = kw("a5", "TEST_KEYWORD");
    kw_empty_params["params"] = json!({});
    let mut kw_false = kw("a5", "TEST_KEYWORD");
    kw_false["isManaAbility"] = json!(false);
    let mut kw_null = kw("a5", "TEST_KEYWORD");
    kw_null["abilityWord"] = Value::Null;
    let candidates: Vec<(&str, Value)> = vec![
        ("/faces/0/abilities/1", activated_instant),
        ("/faces/0/abilities/5", kw_empty_params),
        ("/faces/0/abilities/5", kw_false),
        ("/faces/0/abilities/5", kw_null),
        ("/faces/0/power", json!([{"LITERAL": 2}])),
        ("/faces/0/power", json!({"LITERAL": [2]})),
        (
            "/faces/0/text/2",
            json!({"id": "t2", "kind": "NAME_REF", "ref": "SELF", "name": null}),
        ),
    ];
    for (ptr, x) in candidates {
        let doc = doc_with(ptr, x.clone());
        let bytes = serde_json::to_vec(&doc).unwrap();
        match load(&bytes) {
            Err(CardDefinitionError::Schema(_)) => {}
            Ok(d) => {
                // Accepted: then it is reproduced exactly (a single spelling).
                let canon = canonicalize(&bytes).unwrap();
                assert_eq!(d.to_canonical_json().unwrap(), canon, "{ptr} = {x}");
            }
            other => panic!("{ptr} = {x}: {other:?}"),
        }
    }
    // A positional spelling of the commander eligibility object.
    let ce = base["commanderEligibility"].clone();
    let positional = json!([ce["eligible"], ce["partnerAbilities"]]);
    let mut v = base.clone();
    v["commanderEligibility"] = positional;
    let v = redigested(v);
    let r = load(&serde_json::to_vec(&v).unwrap());
    assert!(
        matches!(r, Err(CardDefinitionError::Schema(_))),
        "positional commanderEligibility: {r:?}"
    );
    if let Ok(d) = serde_json::from_value::<CardDefinitionDSL>(v.clone()) {
        // A derived decoder took it: the decode is not lossless, which is why it is refused.
        assert_ne!(serde_json::to_value(&d).unwrap(), v);
    }
}

// ---------------------------------------------------------------- (h) robustness (R12)

/// REQ-NFR-006, REQ-DSL-010 (R12): bounded random bytes and mutations of a sealed typed
/// definition never panic `from_json`; anything accepted verifies and round-trips.
#[test]
fn req_nfr_006_from_json_random_bytes_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0232_F164_BAD0_0002);
    let mut below = |n: u64| rng.next_u64() % n;
    let check = |bytes: &[u8]| {
        if let Ok(d) = load(bytes) {
            assert_eq!(d.verify_hash(), Ok(()));
            assert_eq!(load(&d.to_canonical_json().unwrap()), Ok(d.clone()));
        }
    };
    const ALPHABET: &[u8] = b"{}[]\":,0123456789-.eE trufalsnLITERALMATHXkindKEYWORDidtext";
    for _ in 0..1500 {
        let len = usize::try_from(below(160)).unwrap();
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                if below(4) == 0 {
                    u8::try_from(below(256)).unwrap()
                } else {
                    ALPHABET[usize::try_from(below(ALPHABET.len() as u64)).unwrap()]
                }
            })
            .collect();
        check(&bytes);
    }
    let seed = sealed(full()).to_canonical_json().unwrap();
    for _ in 0..1500 {
        let mut bytes = seed.clone();
        for _ in 0..1 + below(3) {
            let j = usize::try_from(below(bytes.len() as u64)).unwrap();
            match below(3) {
                0 => bytes[j] ^= 1 << below(8),
                1 => bytes.insert(j, b"{}[]\",: 9"[usize::try_from(below(9)).unwrap()]),
                _ => {
                    bytes.remove(j);
                    if bytes.is_empty() {
                        bytes.push(b'{');
                    }
                }
            }
        }
        check(&bytes);
    }
}

/// REQ-NFR-006, REQ-DSL-010, REQ-DSL-012 (R12): over-deep nesting in `power`, `text`,
/// `abilities` and `partnerAbilities`, built as text far beyond `MAX_DEPTH`, is an error from
/// `from_json`, never a stack overflow (decision #161 FP-141: no exact boundary is pinned).
#[test]
fn req_nfr_006_over_deep_positions_are_errors() {
    let text = String::from_utf8(sealed(full()).to_canonical_json().unwrap()).unwrap();
    let deep_math = |n: usize| {
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(r#"{"MATH":{"op":"ADD","rhs":"X","lhs":"#);
        }
        s.push_str(r#"{"LITERAL":1}"#);
        for _ in 0..n {
            s.push_str("}}");
        }
        s
    };
    let deep_arrays = |n: usize| "[".repeat(n) + &"]".repeat(n);
    let deep_params = |n: usize| {
        format!(
            r#"{{"abilityId":"z","keyword":"TEST_KEYWORD","kind":"KEYWORD","params":{{"d":{}}}}}"#,
            deep_arrays(n)
        )
    };
    let cases: Vec<(&str, String)> = vec![
        (
            r#""power":{"LITERAL":2}"#,
            format!("\"power\":{}", deep_math(MAX_DEPTH * 4)),
        ),
        (
            r#""power":{"LITERAL":2}"#,
            format!("\"power\":{}", deep_arrays(50_000)),
        ),
        (
            r#""text":["#,
            format!("\"text\":[{},", deep_arrays(MAX_DEPTH * 3)),
        ),
        (
            r#""abilities":["#,
            format!("\"abilities\":[{},", deep_params(MAX_DEPTH * 3)),
        ),
        (
            r#""abilities":["#,
            format!("\"abilities\":[{},", deep_arrays(20_000)),
        ),
        (
            r#""partnerAbilities":["#,
            format!("\"partnerAbilities\":[{},", deep_params(10_000)),
        ),
    ];
    for (needle, replacement) in &cases {
        assert!(text.contains(needle), "{needle}");
        let doc = text.replacen(needle, replacement, 1);
        // The canonical nesting-depth error (the parser's recursion limit, which equals
        // `MAX_DEPTH`), not a later schema or hash error.
        match load(doc.as_bytes()) {
            Err(CardDefinitionError::Canonical(CanonicalError::Syntax(msg)))
                if msg.contains("recursion limit") => {}
            other => panic!("{needle}: expected the nesting-depth error, got {other:?}"),
        }
    }
    // Controls: shallow valid forms of the same nestings load (re-digested, since the content
    // changed), so the errors above come from the depth alone.
    for (needle, replacement, what) in [
        (
            r#""power":{"LITERAL":2}"#,
            format!("\"power\":{}", deep_math(3)),
            "MATH",
        ),
        (
            r#""abilities":["#,
            format!("\"abilities\":[{},", deep_params(3)),
            "keyword params",
        ),
    ] {
        assert!(text.contains(needle), "{needle}");
        let doc: Value = serde_json::from_str(&text.replacen(needle, &replacement, 1))
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        let d = load(&serde_json::to_vec(&redigested(doc)).unwrap())
            .unwrap_or_else(|e| panic!("{what}: shallow control refused: {e:?}"));
        if what == "MATH" {
            assert_eq!(
                serde_json::from_str::<Value>(&deep_math(3)).unwrap(),
                serde_json::to_value(&d.faces[0].power).unwrap(),
                "{what}"
            );
        }
    }
}

// ---------------------------------------------------------------- (i) pinned hash (C7)

/// Canonical JSON of the pinned minimal definition without `definitionHash` (the same pin as
/// `spec_dsl_card.rs`).
const PINNED_WITHOUT_HASH: &str = concat!(
    r#"{"cardDefId":"test.vanilla_1_1","colorIdentity":16,"digitalOnly":false,"dslVersion":"1.0","#,
    r#""faces":[{"abilities":[],"cardTypes":["Creature"],"faceKind":"MAIN","manaCost":"{1}{G}","#,
    r#""name":"Test Vanilla","power":{"LITERAL":1},"subtypes":[],"supertypes":[],"text":[],"#,
    r#""toughness":{"LITERAL":1}}],"#,
    r#""layout":"NORMAL","legalities":{"testformat":"LEGAL"},"#,
    r#""source":{"oracleTextHash":"0000","origin":"PARSER","rulesBaseline":"test-baseline"},"#,
    r#""supportStatus":"AUTO_PARSED"}"#
);

/// `printf %s "$PINNED_WITHOUT_HASH" | sha256sum` (C7, unchanged by #232).
const PINNED_HASH: &str = "e930e89d0aec056f27c5a0bac081553c5435af9dbebf4b13f42d65b294d2b770";

/// REQ-NFR-006 (C7): the pinned definitionHash of valid content is unchanged by the typed
/// storage: the minimal definition built from typed values seals to it, its canonical JSON is
/// the pinned bytes plus the digest, and the pinned bytes load.
#[test]
fn req_nfr_006_pinned_hash_unchanged() {
    assert_eq!(sha256(PINNED_WITHOUT_HASH.as_bytes()).to_hex(), PINNED_HASH);
    let mut f = plain_face(FaceKind::Main, "Test Vanilla");
    f.mana_cost = Some(ManaCostDSL::parse("{1}{G}").unwrap());
    f.card_types = vec!["Creature".to_string()];
    f.power = Some(ValueExpressionAST::Literal(1));
    f.toughness = Some(ValueExpressionAST::Literal(1));
    let mut d = definition(vec![f]);
    d.card_def_id = "test.vanilla_1_1".to_string();
    d.color_identity = ColorBitmask::new(16).unwrap();
    let s = sealed(d);
    assert_eq!(s.definition_hash, PINNED_HASH);
    let with_hash = PINNED_WITHOUT_HASH.replacen(
        r#""digitalOnly""#,
        &format!(r#""definitionHash":"{PINNED_HASH}","digitalOnly""#),
        1,
    );
    assert_eq!(
        String::from_utf8(s.to_canonical_json().unwrap()).unwrap(),
        with_hash
    );
    assert_eq!(load(with_hash.as_bytes()), Ok(s));
}

// ---------------------------------------------------------------- typed construction

/// REQ-DSL-012 (C1, C3): text streams built with `TextTokenStream::new` from every token kind
/// are accepted in the face and survive seal and load; `new` refuses a duplicate id.
#[test]
fn req_dsl_012_typed_text_in_definitions() {
    let tokens = vec![
        TextToken::Literal {
            id: "t0".into(),
            text: "test literal".into(),
        },
        TextToken::Word {
            id: "t1".into(),
            category: RewritableCategory::CreatureType,
            value: "Testkin".into(),
        },
        TextToken::NameRef {
            id: "t2".into(),
            kind: NameRefKind::Other,
            name: Some("Test Other".into()),
        },
    ];
    let mut dup = tokens.clone();
    dup.push(tokens[0].clone());
    assert!(TextTokenStream::new(dup).is_err());
    let mut f = plain_face(FaceKind::Main, "Test Text");
    f.text = TextTokenStream::new(tokens).unwrap();
    let s = sealed(definition(vec![f]));
    assert_eq!(load(&s.to_canonical_json().unwrap()), Ok(s.clone()));
    assert_eq!(s.faces[0].text.len(), 3);
}
