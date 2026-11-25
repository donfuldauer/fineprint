// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::ability` (FP-141, issue #161), part 5: abilities carry the real
//! `fineprint_dsl::effect::Instruction` in every instruction position (REQ-DSL-010, REQ-NFR-006).
//! Positions: the `effect` list of SPELL, ACTIVATED and TRIGGERED abilities, a mode's `effect`
//! (in a modal SPELL, ACTIVATED and TRIGGERED) and the `instructions` of a REPLACEMENT `INSTEAD`
//! op. Covers valid forms and canonical round trips, rejection of invalid instruction content,
//! order and multiplicity, emptiness, and rule E5 (no look-back members; SRS section 14.4.1)
//! against the real instruction type (REQ-DSL-006). Black-box: written from the work-item brief
//! only. Fixed seeds, no wall clock (R3, R7); synthetic data only (R5).

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::ability::AbilityAST;
use fineprint_dsl::canonical::{MAX_DEPTH, canonicalize, to_canonical_vec};
use fineprint_dsl::effect::Instruction;
use serde_json::{Value, json};

// ---------------------------------------------------------------- positions

/// An instruction position; each template has the placeholder `<<L>>` for the whole list.
const POSITIONS: [(&str, &str); 7] = [
    (
        "SPELL effect",
        r#"{"abilityId":"a","kind":"SPELL","effect":<<L>>}"#,
    ),
    (
        "ACTIVATED effect",
        r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"effect":<<L>>}"#,
    ),
    (
        "TRIGGERED effect",
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"effect":<<L>>}"#,
    ),
    (
        "SPELL mode effect",
        r#"{"abilityId":"a","kind":"SPELL","modes":{"min":1,"max":1,"modes":[{"id":"m","effect":<<L>>}]},"effect":[]}"#,
    ),
    (
        "ACTIVATED mode effect",
        r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"modes":{"min":1,"max":1,"modes":[{"id":"m","effect":<<L>>}]},"effect":[]}"#,
    ),
    (
        "TRIGGERED mode effect",
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"modes":{"min":1,"max":1,"modes":[{"id":"m","effect":<<L>>}]},"effect":[]}"#,
    ),
    (
        "INSTEAD instructions",
        r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"INSTEAD","instructions":<<L>>}]}"#,
    ),
];

/// The positions where an empty list is allowed (everything except `INSTEAD`).
const EMPTY_OK: [usize; 6] = [0, 1, 2, 3, 4, 5];

fn at(pos: usize, list_text: &str) -> String {
    POSITIONS[pos].1.replace("<<L>>", list_text)
}

// ---------------------------------------------------------------- helpers

/// Decodes `text` as `AbilityAST` through `from_str`, `from_slice` and `from_value`; the value
/// equals the input document; canonical bytes equal `canonicalize(text)` (RFC 8785); decode of
/// encode is the same value; encode, decode, encode gives identical bytes.
fn accept_ability(text: &str) -> AbilityAST {
    let a: AbilityAST = match serde_json::from_str(text) {
        Ok(a) => a,
        Err(e) => panic!("expected {text} to decode, got error: {e}"),
    };
    assert_eq!(
        serde_json::from_slice::<AbilityAST>(text.as_bytes()).unwrap(),
        a,
        "from_slice of {text}"
    );
    let doc: Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        serde_json::from_value::<AbilityAST>(doc.clone()).unwrap(),
        a,
        "from_value of {text}"
    );
    let canon = canonicalize(text.as_bytes()).unwrap();
    assert_eq!(
        String::from_utf8(to_canonical_vec(&a).unwrap()).unwrap(),
        String::from_utf8(canon.clone()).unwrap(),
        "canonical bytes of {text}"
    );
    // The canonical form of the decoded value, compared as a document.
    let from_canon: Value = serde_json::from_slice(&to_canonical_vec(&a).unwrap()).unwrap();
    assert_eq!(from_canon, doc, "canonical document of {text}");
    let e1 = serde_json::to_vec(&a).unwrap();
    let b: AbilityAST = serde_json::from_slice(&e1).unwrap();
    assert_eq!(b, a, "decode(encode(x)) != x for {text}");
    let e2 = serde_json::to_vec(&b).unwrap();
    assert_eq!(e1, e2, "re-encoding is unstable for {text}");
    let c: AbilityAST = serde_json::from_slice(&canon).unwrap();
    assert_eq!(c, a, "canonical bytes of {text} decode differently");
    a
}

/// The same checks for an `Instruction` standalone.
fn accept_instr(text: &str) -> Instruction {
    let i: Instruction = match serde_json::from_str(text) {
        Ok(i) => i,
        Err(e) => panic!("expected {text} to decode, got error: {e}"),
    };
    assert_eq!(
        serde_json::from_slice::<Instruction>(text.as_bytes()).unwrap(),
        i
    );
    let doc: Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        serde_json::from_value::<Instruction>(doc.clone()).unwrap(),
        i
    );
    let canon = canonicalize(text.as_bytes()).unwrap();
    assert_eq!(
        String::from_utf8(to_canonical_vec(&i).unwrap()).unwrap(),
        String::from_utf8(canon).unwrap(),
        "canonical bytes of {text}"
    );
    let e1 = serde_json::to_vec(&i).unwrap();
    let back: Instruction = serde_json::from_slice(&e1).unwrap();
    assert_eq!(back, i);
    assert_eq!(serde_json::to_vec(&back).unwrap(), e1);
    i
}

/// Rejected by `from_str` and `from_slice` (the only entry points that see duplicate members and
/// non-integer numbers faithfully).
fn reject_text<T: serde::de::DeserializeOwned + std::fmt::Debug>(s: &str) {
    let r = serde_json::from_str::<T>(s);
    assert!(r.is_err(), "expected rejection of {s}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<T>(s.as_bytes()).is_err(),
        "expected rejection (from_slice) of {s}"
    );
}

/// Rejected by `from_str`, `from_slice` and, when the text is a plain JSON value, `from_value`.
fn reject_all<T: serde::de::DeserializeOwned + std::fmt::Debug>(s: &str) {
    reject_text::<T>(s);
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        assert!(
            serde_json::from_value::<T>(v).is_err(),
            "expected rejection (from_value) of {s}"
        );
    }
}

fn list<S: AsRef<str>>(items: &[S]) -> String {
    let v: Vec<&str> = items.iter().map(AsRef::as_ref).collect();
    format!("[{}]", v.join(","))
}

const POPULATE: &str = r#"{"op":"POPULATE"}"#;
const VENTURE: &str = r#"{"op":"VENTURE"}"#;
const DRAW: &str = r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}"#;

/// Valid real instructions, in canonical member order.
const VALID: [&str; 9] = [
    POPULATE,
    VENTURE,
    DRAW,
    r#"{"op":"SHUFFLE","player":{"ref":"CONTROLLER"}}"#,
    r#"{"op":"GAIN_LIFE","player":{"ref":"CONTROLLER"},"amount":{"LITERAL":2}}"#,
    r#"{"op":"IF","cond":{"pred":"IS_CREATURE"},"then":[{"op":"POPULATE"}]}"#,
    r#"{"op":"IF","cond":{"pred":"IS_CREATURE"},"then":[{"op":"IF","cond":{"pred":"TAPPED"},"then":[{"op":"POPULATE"},{"op":"VENTURE"}]}]}"#,
    r#"{"op":"MAY","player":{"ref":"CONTROLLER"},"body":[{"op":"POPULATE"}]}"#,
    r#"{"op":"TAP_OR_UNTAP","action":"TAP","objects":{"ref":"TARGET","slot":0}}"#,
];

// ---------------------------------------------------------------- accept and round trip

/// REQ-DSL-010, REQ-NFR-006: every valid real instruction decodes standalone and in every
/// position, alone and in a list, and round-trips canonically.
#[test]
fn req_dsl_010_valid_instructions_round_trip_in_every_position() {
    for v in VALID {
        accept_instr(v);
    }
    for (p, (name, _)) in POSITIONS.iter().enumerate() {
        for v in VALID {
            let a = accept_ability(&at(p, &list(&[v])));
            assert_ne!(
                a,
                accept_ability(&at(p, &list(&[POPULATE, v, VENTURE]))),
                "{name}: {v}"
            );
        }
        accept_ability(&at(p, &list(&VALID)));
    }
}

/// REQ-DSL-010, REQ-NFR-006: member order in the input does not matter; the canonical form is the
/// RFC 8785 form of the document (members sorted), in every position.
#[test]
fn req_nfr_006_non_canonical_member_order_canonicalises_in_every_position() {
    let shuffled = [
        (
            r#"{"count":{"LITERAL":1},"player":{"ref":"CONTROLLER"},"op":"DRAW"}"#,
            DRAW,
        ),
        (
            r#"{ "then" : [ {"op":"POPULATE"} ] , "cond":{"pred":"IS_CREATURE"}, "op":"IF" }"#,
            r#"{"op":"IF","cond":{"pred":"IS_CREATURE"},"then":[{"op":"POPULATE"}]}"#,
        ),
        (
            r#"{"body":[{"op":"POPULATE"}],"player":{"ref":"CONTROLLER"},"op":"MAY"}"#,
            r#"{"op":"MAY","player":{"ref":"CONTROLLER"},"body":[{"op":"POPULATE"}]}"#,
        ),
    ];
    for (p, _) in POSITIONS.iter().enumerate() {
        for (messy, tidy) in shuffled {
            let a = accept_ability(&at(p, &list(&[messy])));
            let b = accept_ability(&at(p, &list(&[tidy])));
            assert_eq!(a, b);
            assert_eq!(to_canonical_vec(&a).unwrap(), to_canonical_vec(&b).unwrap());
        }
    }
    for (messy, tidy) in shuffled {
        assert_eq!(accept_instr(messy), accept_instr(tidy));
    }
}

/// REQ-DSL-005, REQ-DSL-010: `effect: []` is accepted on SPELL, ACTIVATED, TRIGGERED and in a
/// mode; `INSTEAD.instructions` must be non-empty. An empty list differs from a non-empty one.
#[test]
fn req_dsl_010_emptiness_rules() {
    for p in EMPTY_OK {
        let empty = accept_ability(&at(p, "[]"));
        let one = accept_ability(&at(p, &list(&[POPULATE])));
        assert_ne!(empty, one, "{}", POSITIONS[p].0);
    }
    reject_all::<AbilityAST>(&at(6, "[]"));
    accept_ability(&at(6, &list(&[POPULATE])));
}

/// REQ-DSL-010, REQ-NFR-006: the list is an array; an object, string, number or null in its place
/// is rejected in every position.
#[test]
fn req_dsl_010_instruction_list_must_be_array() {
    for (p, (name, _)) in POSITIONS.iter().enumerate() {
        for bad in [POPULATE, r#""POPULATE""#, "1", "null", "true", "{}"] {
            let t = at(p, bad);
            assert!(
                serde_json::from_str::<AbilityAST>(&t).is_err(),
                "{name}: {t}"
            );
            reject_all::<AbilityAST>(&t);
        }
    }
}

// ---------------------------------------------------------------- order and multiplicity

/// REQ-DSL-010, REQ-NFR-006 (both orders): `[POPULATE, VENTURE]` and `[VENTURE, POPULATE]` keep
/// their order through decode and encode and are different values; the same instruction twice
/// keeps both.
#[test]
fn req_dsl_010_instruction_order_and_multiplicity() {
    for (p, (name, _)) in POSITIONS.iter().enumerate() {
        let pv_text = at(p, &list(&[POPULATE, VENTURE]));
        let vp_text = at(p, &list(&[VENTURE, POPULATE]));
        let pv = accept_ability(&pv_text);
        let vp = accept_ability(&vp_text);
        assert_ne!(pv, vp, "{name}: order must matter");
        // Order survives encoding.
        let doc_pv: Value = serde_json::from_str(&pv_text).unwrap();
        let doc_vp: Value = serde_json::from_str(&vp_text).unwrap();
        assert_eq!(serde_json::to_value(&pv).unwrap(), doc_pv, "{name}");
        assert_eq!(serde_json::to_value(&vp).unwrap(), doc_vp, "{name}");
        assert_ne!(
            to_canonical_vec(&pv).unwrap(),
            to_canonical_vec(&vp).unwrap(),
            "{name}"
        );
        // Multiplicity: the same instruction twice keeps both.
        let once = accept_ability(&at(p, &list(&[POPULATE])));
        let twice = accept_ability(&at(p, &list(&[POPULATE, POPULATE])));
        let thrice = accept_ability(&at(p, &list(&[POPULATE, POPULATE, POPULATE])));
        assert_ne!(once, twice, "{name}");
        assert_ne!(twice, thrice, "{name}");
        let twice_doc = serde_json::to_value(&twice).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&at(p, &list(&[POPULATE, POPULATE]))).unwrap(),
            twice_doc,
            "{name}"
        );
        let dd = accept_ability(&at(p, &list(&[DRAW, DRAW])));
        assert_ne!(dd, accept_ability(&at(p, &list(&[DRAW]))), "{name}");
    }
}

/// REQ-DSL-010, REQ-NFR-006 (R3, seed 0x000F_1410_0000_0001): 300 seeded random lists of valid
/// instructions round-trip canonically in a random position; the reversed list keeps the reversed
/// order and is a different value unless the list is its own reverse.
#[test]
fn req_dsl_010_property_random_lists_round_trip() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x000F_1410_0000_0001);
    for _ in 0..300 {
        let n = 1 + (rng.next_u64() % 4) as usize;
        let items: Vec<&str> = (0..n)
            .map(|_| VALID[(rng.next_u64() % VALID.len() as u64) as usize])
            .collect();
        let p = (rng.next_u64() % POSITIONS.len() as u64) as usize;
        let fwd_text = at(p, &list(&items));
        let a = accept_ability(&fwd_text);
        let mut rev = items.clone();
        rev.reverse();
        let b = accept_ability(&at(p, &list(&rev)));
        if items == rev {
            assert_eq!(a, b);
        } else {
            assert_ne!(a, b, "{fwd_text}");
        }
        let doc: Value = serde_json::from_str(&fwd_text).unwrap();
        assert_eq!(serde_json::to_value(&a).unwrap(), doc);
    }
}

// ---------------------------------------------------------------- reject

/// Instruction texts rejected through every entry point (a plain `Value` can represent them).
const BAD_VALUE_ELEMENTS: [&str; 24] = [
    r#"{"op":"NOT_AN_OP"}"#,
    r#"{"op":"populate"}"#,
    r#"{"op":"Populate"}"#,
    r#"{"op":"DRAW"}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"}}"#,
    r#"{"op":"POPULATE","extra":1}"#,
    r#"{"op":"POPULATE","x":null}"#,
    r#"{"op":"POPULATE","lookBack":true}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1},"extra":1}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":null}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":9007199254740992}}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":-9007199254740992}}"#,
    r#"{"op":"IF","cond":{"pred":"IS_CREATURE"},"then":[]}"#,
    r#"{"op":"IF","cond":{"pred":"IS_CREATURE"},"then":[{"op":"NOT_AN_OP"}]}"#,
    r#"{"op":"MAY","player":{"ref":"CONTROLLER"},"body":[{"op":"POPULATE","extra":1}]}"#,
    r#"{"op":"POPULATE","op2":"POPULATE"}"#,
    r#"{}"#,
    r#"{"player":{"ref":"CONTROLLER"}}"#,
    r#"{"op":1}"#,
    r#"{"op":null}"#,
    r#"{"op":["POPULATE"]}"#,
    r#"{"op":""}"#,
    r#"{"op":"DRAW","op ":"POPULATE"}"#,
    r#"{"OP":"POPULATE"}"#,
];

/// Non-object elements.
const BAD_NON_OBJECTS: [&str; 7] = [
    "1",
    r#""POPULATE""#,
    "[]",
    "null",
    "true",
    "-1",
    r#"[{"op":"POPULATE"}]"#,
];

/// Instruction texts rejected through `from_str` and `from_slice` only (duplicates and
/// non-integers cannot be represented by a pre-parsed `Value`).
const BAD_TEXT_ELEMENTS: [&str; 8] = [
    r#"{"op":"POPULATE","op":"POPULATE"}"#,
    r#"{"op":"POPULATE","op":"VENTURE"}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1.5}}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1.0}}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1e2}}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":-0}}"#,
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}"#,
    r#"{"op":"IF","cond":{"pred":"IS_CREATURE"},"then":[{"op":"POPULATE","op":"POPULATE"}]}"#,
];

/// REQ-DSL-010, REQ-NFR-006: invalid instruction content is rejected standalone and in every
/// position: unknown op, lower-case op, missing required member, unknown member, null member,
/// missing or non-string `op`, unsafe integers, nested invalid instruction, non-object element.
#[test]
fn req_dsl_010_invalid_instruction_rejected_in_every_position() {
    for bad in BAD_VALUE_ELEMENTS {
        reject_all::<Instruction>(bad);
        for (p, (name, _)) in POSITIONS.iter().enumerate() {
            let t = at(p, &list(&[bad]));
            assert!(
                serde_json::from_str::<AbilityAST>(&t).is_err(),
                "{name}: {t}"
            );
            reject_all::<AbilityAST>(&t);
            // A bad instruction poisons a list of good ones, at either end.
            reject_all::<AbilityAST>(&at(p, &list(&[POPULATE, bad])));
            reject_all::<AbilityAST>(&at(p, &list(&[bad, POPULATE])));
        }
    }
    for bad in BAD_NON_OBJECTS {
        reject_all::<Instruction>(bad);
        for (p, _) in POSITIONS.iter().enumerate() {
            reject_all::<AbilityAST>(&at(p, &list(&[bad])));
            reject_all::<AbilityAST>(&at(p, &list(&[POPULATE, bad])));
        }
    }
}

/// REQ-DSL-010, REQ-NFR-006 (W2): a duplicate member and a non-integer number inside an
/// instruction are rejected when decoding from text or bytes, in every position.
#[test]
fn req_nfr_006_duplicates_and_non_integers_in_instructions_rejected() {
    for bad in BAD_TEXT_ELEMENTS {
        reject_text::<Instruction>(bad);
        for (p, _) in POSITIONS.iter().enumerate() {
            reject_text::<AbilityAST>(&at(p, &list(&[bad])));
            reject_text::<AbilityAST>(&at(p, &list(&[POPULATE, bad])));
        }
    }
}

/// REQ-DSL-010, REQ-NFR-006: an `op` of the closed set with a different case is rejected for
/// every name used here, and an unknown `op` shaped like a real one is rejected.
#[test]
fn req_dsl_010_op_names_are_exact() {
    for op in [
        "populate",
        "Populate",
        "venture",
        "Venture",
        "draw",
        "DRAW ",
        " DRAW",
        "DRAW\n",
        "POPULATE2",
        "POPULATE_",
        "POP",
        "TEST_OP",
        "OTHER_OP",
        "DRAW_TEST",
    ] {
        let t = format!(r#"{{"op":{}}}"#, serde_json::to_string(op).unwrap());
        reject_all::<Instruction>(&t);
        for (p, _) in POSITIONS.iter().enumerate() {
            reject_all::<AbilityAST>(&at(p, &list(&[&t])));
        }
    }
}

/// Builds `n` nested `IF` instructions around a `POPULATE`, as text.
fn nested_if_text(n: usize) -> String {
    let mut s = String::from(POPULATE);
    for _ in 0..n {
        s = format!(r#"{{"op":"IF","cond":{{"pred":"IS_CREATURE"}},"then":[{s}]}}"#);
    }
    s
}

/// The same nesting as a `Value`.
fn nested_if_value(n: usize) -> Value {
    let mut v = json!({"op":"POPULATE"});
    for _ in 0..n {
        v = json!({"op":"IF","cond":{"pred":"IS_CREATURE"},"then":[v]});
    }
    v
}

/// REQ-NFR-006, REQ-DSL-010 (R12): nesting beyond `canonical::MAX_DEPTH` is rejected in every
/// position and standalone, through `from_str`, `from_slice` and `from_value`; a modest nesting
/// is accepted and round-trips canonically.
#[test]
fn req_nfr_006_instruction_nesting_beyond_depth_limit_rejected() {
    assert_eq!(MAX_DEPTH, 127);
    // Each IF adds an object level and an array level, so 70 IFs exceed 127 on their own.
    for n in [70usize, 200] {
        let text = nested_if_text(n);
        reject_text::<Instruction>(&text);
        let val = nested_if_value(n);
        assert!(serde_json::from_value::<Instruction>(val.clone()).is_err());
        for (p, _) in POSITIONS.iter().enumerate() {
            let t = at(p, &list(&[&text]));
            reject_text::<AbilityAST>(&t);
            let doc: Value = serde_json::from_str(&at(p, "[]")).unwrap();
            let doc = splice(&doc, p, &val);
            assert!(
                serde_json::from_value::<AbilityAST>(doc).is_err(),
                "from_value at depth {n} in position {p}"
            );
        }
    }
    for n in [1usize, 5, 20] {
        let text = nested_if_text(n);
        accept_instr(&text);
        for (p, _) in POSITIONS.iter().enumerate() {
            accept_ability(&at(p, &list(&[&text])));
        }
    }
}

/// `doc` (an ability with an empty list in position `p`, or an `INSTEAD` with `[]`) with the list
/// of position `p` replaced by `[v]`.
fn splice(doc: &Value, p: usize, v: &Value) -> Value {
    let mut d = doc.clone();
    let l = Value::Array(vec![v.clone()]);
    match p {
        0..=2 => d["effect"] = l,
        3..=5 => d["modes"]["modes"][0]["effect"] = l,
        _ => d["replaceWith"][0]["instructions"] = l,
    }
    d
}

// ---------------------------------------------------------------- rule E5

/// Member names a look-back flag could hide behind (rule E5 normalisation: lower-case, strip `_`
/// and `-`, starts with `lookback`).
const LOOK_BACK_NAMES: [&str; 6] = [
    "lookBack",
    "look_back",
    "LOOK-BACK",
    "lookback",
    "LookBackFlag",
    "l_o_o_k-b_a_c_k",
];

/// REQ-DSL-006, REQ-DSL-010, REQ-NFR-006 (rule E5, SRS section 14.4.1): the real instruction type
/// has no look-back member, so one on an otherwise valid instruction is an unknown member and is
/// rejected, standalone and in every position.
#[test]
fn req_dsl_006_look_back_member_on_instruction_rejected() {
    for name in LOOK_BACK_NAMES {
        for val in ["true", "false", "1", r#""YES""#] {
            for base in [
                POPULATE,
                VENTURE,
                DRAW,
                r#"{"op":"SHUFFLE","player":{"ref":"CONTROLLER"}}"#,
            ] {
                let t = format!(
                    "{},{}:{}}}",
                    &base[..base.len() - 1],
                    serde_json::to_string(name).unwrap(),
                    val
                );
                reject_all::<Instruction>(&t);
                for (p, _) in POSITIONS.iter().enumerate() {
                    reject_all::<AbilityAST>(&at(p, &list(&[&t])));
                }
            }
        }
    }
}

/// REQ-DSL-006, REQ-NFR-006 (rule E5): look-back keys in an event matcher's `filters` and `bind`
/// are still errors when the ability's effect list holds real instructions; the same documents
/// with an ordinary key decode.
#[test]
fn req_dsl_006_look_back_filter_and_bind_keys_rejected_with_real_effects() {
    let tpl_filters = r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","filters":{"<<K>>":true}}},"effect":[{"op":"POPULATE"},{"op":"VENTURE"}]}"#;
    let tpl_bind = r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","bind":{"<<K>>":"$v"}}},"effect":[{"op":"POPULATE"}]}"#;
    let tpl_mode = r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","filters":{"<<K>>":true}}},"modes":{"min":1,"max":1,"modes":[{"id":"m","effect":[{"op":"POPULATE"}]}]},"effect":[]}"#;
    for tpl in [tpl_filters, tpl_bind, tpl_mode] {
        accept_ability(&tpl.replace("<<K>>", "combat"));
        for name in LOOK_BACK_NAMES {
            reject_all::<AbilityAST>(&tpl.replace("<<K>>", name));
        }
    }
}

/// REQ-DSL-006, REQ-DSL-010, REQ-NFR-006 (rule E5): a look-back key in the event matcher of a
/// trigger nested inside an instruction (`CREATE_DELAYED_TRIGGER`) is rejected, standalone and
/// in every position; the baseline without the key decodes.
#[test]
fn req_dsl_006_look_back_key_in_nested_delayed_trigger_rejected() {
    let tpl = r#"{"op":"CREATE_DELAYED_TRIGGER","trigger":{"mode":"EVENT","event":{"kind":"DIES",<<M>>}},"effect":[{"op":"POPULATE"}],"oneShot":false}"#;
    let ok_member = r#""filters":{"combat":true}"#;
    let base = tpl.replace("<<M>>", ok_member);
    accept_instr(&base);
    for (p, _) in POSITIONS.iter().enumerate() {
        accept_ability(&at(p, &list(&[&base])));
    }
    for name in LOOK_BACK_NAMES {
        for member in [
            format!(r#""filters":{{"{name}":true}}"#),
            format!(r#""bind":{{"{name}":"$v"}}"#),
        ] {
            let bad = tpl.replace("<<M>>", &member);
            reject_all::<Instruction>(&bad);
            for (p, _) in POSITIONS.iter().enumerate() {
                reject_all::<AbilityAST>(&at(p, &list(&[&bad])));
                reject_all::<AbilityAST>(&at(p, &list(&[POPULATE, &bad])));
            }
        }
        // A look-back member directly on the nested trigger or on the instruction.
        let on_matcher = tpl.replace("<<M>>", &format!(r#""{name}":true"#));
        reject_all::<Instruction>(&on_matcher);
        let on_instr = format!(
            r#"{{"op":"CREATE_DELAYED_TRIGGER","trigger":{{"mode":"EVENT","event":{{"kind":"DIES"}}}},"effect":[{{"op":"POPULATE"}}],"oneShot":false,"{name}":true}}"#
        );
        reject_all::<Instruction>(&on_instr);
        // And inside the nested instruction list of the delayed trigger.
        let nested = format!(
            r#"{{"op":"CREATE_DELAYED_TRIGGER","trigger":{{"mode":"EVENT","event":{{"kind":"DIES"}}}},"effect":[{{"op":"POPULATE","{name}":true}}],"oneShot":false}}"#
        );
        reject_all::<Instruction>(&nested);
    }
}

// ---------------------------------------------------------------- SRS 14.4 examples

/// REQ-DSL-010, REQ-NFR-006: the SRS section 14.4 triggered DRAW example (EX2) in its real
/// instruction form round-trips canonically; the DRAW instruction in it is a valid
/// `effect::Instruction`.
#[test]
fn req_dsl_010_srs_triggered_draw_example_round_trips() {
    let ex2 = r#"{"abilityId":"a1","kind":"TRIGGERED","functionsIn":["BATTLEFIELD"],"trigger":{"mode":"EVENT","event":{"kind":"ATTACKS","subject":{"ref":"SELF"}}},"effect":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}]}"#;
    accept_ability(ex2);
    accept_instr(DRAW);
}

/// REQ-DSL-010, REQ-NFR-006: the SRS section 14.4 activated TAP_OR_UNTAP example (EX3) uses the
/// real members `action` and `objects` (the SRS shorthand `mode` is not a real member and is
/// rejected as an unknown member).
#[test]
fn req_dsl_010_srs_activated_tap_example_uses_real_members() {
    let real = r#"{"op":"TAP_OR_UNTAP","action":"TAP","objects":{"ref":"TARGET","slot":0}}"#;
    let shorthand = r#"{"op":"TAP_OR_UNTAP","mode":"TAP","objects":{"ref":"TARGET","slot":0}}"#;
    accept_instr(real);
    reject_all::<Instruction>(shorthand);
    let ex3 = r#"{"abilityId":"a2","kind":"ACTIVATED","functionsIn":["BATTLEFIELD"],"cost":[{"cost":"PAY_MANA","mana":"{2}"},{"cost":"TAP_SOURCE"}],"timing":"SORCERY","limits":[{"perTurn":1}],"activateOnlyIf":{"pred":"CONTROLS","selector":{"pred":"HAS_SUBTYPE","value":{"name":"TestType"}}},"isLoyalty":false,"isManaAbility":false,"targets":[{"slot":0,"selector":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"min":1,"max":1}],"effect":[{"op":"TAP_OR_UNTAP","action":"TAP","objects":{"ref":"TARGET","slot":0}}]}"#;
    accept_ability(ex3);
    reject_all::<AbilityAST>(&ex3.replace(r#""action":"TAP""#, r#""mode":"TAP""#));
}

// ---------------------------------------------------------------- non-instruction ops stay

/// REQ-DSL-009, REQ-DSL-010: replacement and modification ops are not instructions; they remain
/// valid where they were and are rejected as instructions.
#[test]
fn req_dsl_010_replacement_ops_are_not_instructions() {
    for op in [
        r#"{"op":"SKIP"}"#,
        r#"{"op":"MODIFY_EVENT","set":{"to":"EXILE"}}"#,
        r#"{"op":"INSTEAD","instructions":[{"op":"POPULATE"}]}"#,
        r#"{"op":"MODIFY_PT","power":1,"toughness":1}"#,
        r#"{"op":"SET_BASE_PT","power":1,"toughness":1}"#,
        r#"{"op":"REMOVE_ALL_ABILITIES"}"#,
    ] {
        reject_all::<Instruction>(op);
        for (p, _) in POSITIONS.iter().enumerate() {
            reject_all::<AbilityAST>(&at(p, &list(&[op])));
        }
    }
    accept_ability(
        r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"},{"op":"INSTEAD","instructions":[{"op":"POPULATE"}]}]}"#,
    );
}
