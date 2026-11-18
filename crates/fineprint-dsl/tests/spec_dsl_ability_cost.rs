// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::ability` (FP-012 wire contract), part 2: activated abilities
//! (REQ-DSL-007, SRS §14.4.2) and the 18 CostAST nodes, including COMPOSITE nesting against the
//! depth budget (REQ-NFR-006). Black-box: written from the work-item brief only. Synthetic data
//! only (R5).

use std::fmt::Debug;

use fineprint_dsl::ability::{AbilityAST, CostAST};
use fineprint_dsl::canonical::{MAX_DEPTH, canonicalize, to_canonical_vec};
use fineprint_dsl::mana::ManaCostDSL;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const TIMINGS: [&str; 5] = [
    "INSTANT",
    "SORCERY",
    "YOUR_TURN",
    "OPPONENT_TURN",
    "COMBAT_ONLY",
];

/// Mana strings used below; each is checked with `ManaCostDSL::parse` before use.
const MANA_OK: [&str; 3] = ["{2}", "{W}{U}", "{1}"];
const MANA_BAD: [&str; 3] = ["{Q}", "2", "{2"];

// ---------------------------------------------------------------- helpers

fn text(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

/// Decodes through `from_str` and `from_slice`; to_value equals the input; canonical bytes equal
/// `canonicalize` (W4); re-decoding gives the same value.
fn accept<T>(s: &str) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let t: T = match serde_json::from_str::<T>(s) {
        Ok(t) => t,
        Err(e) => panic!("expected {s} to decode, got error: {e}"),
    };
    let t2: T = serde_json::from_slice(s.as_bytes()).unwrap();
    assert_eq!(t2, t);
    let original: Value = serde_json::from_str(s).unwrap();
    assert_eq!(
        serde_json::to_value(&t).unwrap(),
        original,
        "to_value of {s}"
    );
    let canon = canonicalize(s.as_bytes()).unwrap();
    assert_eq!(
        to_canonical_vec(&t).unwrap(),
        canon,
        "canonical bytes of {s}"
    );
    let from_canon: T = serde_json::from_slice(&canon).unwrap();
    assert_eq!(from_canon, t);
    let back: T = serde_json::from_slice(&serde_json::to_vec(&t).unwrap()).unwrap();
    assert_eq!(back, t);
    t
}

fn accept_v<T>(v: &Value) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    accept::<T>(&text(v))
}

fn reject<T: DeserializeOwned + Debug>(s: &str) {
    let r = serde_json::from_str::<T>(s);
    assert!(r.is_err(), "expected rejection of {s}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<T>(s.as_bytes()).is_err(),
        "expected rejection (from_slice) of {s}"
    );
    if canonicalize(s.as_bytes()).is_ok() {
        let v: Value = serde_json::from_str(s).unwrap();
        assert!(
            serde_json::from_value::<T>(v).is_err(),
            "expected rejection (from_value) of {s}"
        );
    }
}

fn reject_v<T: DeserializeOwned + Debug>(v: &Value) {
    reject::<T>(&text(v))
}

fn with(base: &Value, k: &str, x: Value) -> Value {
    let mut v = base.clone();
    v.as_object_mut().unwrap().insert(k.to_string(), x);
    v
}

fn without(base: &Value, k: &str) -> Value {
    let mut v = base.clone();
    v.as_object_mut().unwrap().remove(k);
    v
}

fn activated(cost: Value) -> Value {
    json!({"abilityId":"a","kind":"ACTIVATED","cost":cost,"effect":[]})
}

fn sel() -> Value {
    json!({"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"})
}

fn val() -> Value {
    json!({"LITERAL":1})
}

#[derive(Clone, Copy, PartialEq)]
enum Ty {
    Mana,
    Value,
    Selector,
    Zone,
    Str,
    Bool,
    Costs,
}

/// One member of a cost node: (member, valid value, type, required).
type Member = (&'static str, Value, Ty, bool);

/// (node, members).
fn nodes() -> Vec<(&'static str, Vec<Member>)> {
    vec![
        ("PAY_MANA", vec![("mana", json!("{2}"), Ty::Mana, true)]),
        (
            "PAY_LIFE",
            vec![("amount", json!({"LITERAL":2}), Ty::Value, true)],
        ),
        ("TAP_SOURCE", vec![]),
        ("UNTAP_SOURCE", vec![]),
        (
            "TAP_OBJECTS",
            vec![
                ("selector", sel(), Ty::Selector, true),
                ("count", val(), Ty::Value, true),
            ],
        ),
        (
            "SACRIFICE",
            vec![
                ("selector", sel(), Ty::Selector, true),
                ("count", val(), Ty::Value, true),
            ],
        ),
        (
            "DISCARD",
            vec![
                ("selector", json!({"zones":["HAND"]}), Ty::Selector, true),
                ("count", json!("X"), Ty::Value, true),
                ("random", json!(true), Ty::Bool, false),
            ],
        ),
        (
            "EXILE_FROM_ZONE",
            vec![
                ("zone", json!("GRAVEYARD"), Ty::Zone, true),
                (
                    "selector",
                    json!({"pred":"IS_CREATURE"}),
                    Ty::Selector,
                    true,
                ),
                ("count", val(), Ty::Value, true),
            ],
        ),
        (
            "REMOVE_COUNTERS",
            vec![
                ("from", json!({"ref":"SELF"}), Ty::Selector, true),
                ("kind", json!("test_counter"), Ty::Str, true),
                ("amount", val(), Ty::Value, true),
            ],
        ),
        (
            "PUT_COUNTERS",
            vec![
                ("on", json!({"ref":"SELF"}), Ty::Selector, true),
                ("kind", json!("test_counter"), Ty::Str, true),
                ("amount", json!({"LITERAL":2}), Ty::Value, true),
            ],
        ),
        (
            "RETURN_TO_HAND",
            vec![("selector", sel(), Ty::Selector, true)],
        ),
        (
            "REVEAL",
            vec![("selector", json!({"zones":["HAND"]}), Ty::Selector, true)],
        ),
        (
            "PAY_ENERGY",
            vec![("amount", json!({"LITERAL":3}), Ty::Value, true)],
        ),
        (
            "COLLECT_EVIDENCE",
            vec![("n", json!({"LITERAL":6}), Ty::Value, true)],
        ),
        ("FORAGE", vec![]),
        ("WATERBEND", vec![("mana", json!("{1}"), Ty::Mana, true)]),
        (
            "LOYALTY",
            vec![("delta", json!({"LITERAL":-2}), Ty::Value, true)],
        ),
        (
            "COMPOSITE",
            vec![("costs", json!([{"cost":"TAP_SOURCE"}]), Ty::Costs, true)],
        ),
    ]
}

fn node_doc(name: &str, members: &[Member], include_optional: bool) -> Value {
    let mut v = json!({ "cost": name });
    for (k, x, _, req) in members {
        if *req || include_optional {
            v = with(&v, k, x.clone());
        }
    }
    v
}

/// Invalid values of a member type.
fn bad_values(ty: Ty) -> Vec<Value> {
    match ty {
        Ty::Mana => {
            let mut out = vec![json!(2), json!(null), json!({"mana":"{2}"})];
            for m in MANA_BAD {
                out.push(json!(m));
            }
            out
        }
        Ty::Value => vec![
            json!(1),
            json!("1"),
            json!(null),
            json!({}),
            json!({"LITERAL":"1"}),
            json!({"LITERAL":9007199254740992u64}),
        ],
        Ty::Selector => vec![
            json!("SELF"),
            json!(null),
            json!({}),
            json!({"ref":"NOBODY"}),
        ],
        Ty::Zone => vec![
            json!("NOWHERE"),
            json!("graveyard"),
            json!(""),
            json!(null),
            json!(4),
        ],
        Ty::Str => vec![json!(""), json!(null), json!(1), json!(["test_counter"])],
        Ty::Bool => vec![json!("true"), json!(1), json!(null)],
        Ty::Costs => vec![
            json!([]),
            json!(null),
            json!({"cost":"TAP_SOURCE"}),
            json!([{"cost":"NOPE"}]),
            json!([null]),
        ],
    }
}

// ---------------------------------------------------------------- tests

/// REQ-DSL-007: the mana strings used in these tests are valid / invalid ManaCostDSL text.
#[test]
fn req_dsl_007_mana_fixtures_verified() {
    for m in MANA_OK {
        assert!(ManaCostDSL::parse(m).is_ok(), "{m}");
    }
    for m in MANA_BAD {
        assert!(ManaCostDSL::parse(m).is_err(), "{m}");
    }
}

/// REQ-DSL-007: there are exactly 18 node names in the fixture table, all distinct.
#[test]
fn req_dsl_007_eighteen_cost_nodes() {
    let n = nodes();
    assert_eq!(n.len(), 18);
    let mut names: Vec<&str> = n.iter().map(|(k, _)| *k).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 18);
}

/// REQ-DSL-007: every CostAST node with valid members decodes and round-trips, standalone and in
/// an ACTIVATED ability; each missing required member, each invalid member value, each null and
/// each extra member is rejected.
#[test]
fn req_dsl_007_every_cost_node() {
    let mut seen: Vec<CostAST> = Vec::new();
    for (name, members) in nodes() {
        let full = node_doc(name, &members, true);
        let min = node_doc(name, &members, false);
        let c: CostAST = accept_v(&full);
        assert!(!seen.contains(&c), "{name}");
        seen.push(c);
        accept_v::<CostAST>(&min);
        accept_v::<AbilityAST>(&activated(json!([full])));
        accept_v::<AbilityAST>(&activated(json!([min])));
        for (k, _, ty, req) in &members {
            if *req {
                reject_v::<CostAST>(&without(&full, k));
                reject_v::<AbilityAST>(&activated(json!([without(&full, k)])));
            } else {
                accept_v::<CostAST>(&without(&full, k));
            }
            for b in bad_values(*ty) {
                reject_v::<CostAST>(&with(&full, k, b.clone()));
                reject_v::<AbilityAST>(&activated(json!([with(&full, k, b)])));
            }
        }
        // Extra members: an unknown one and one that belongs to another node.
        let foreign = if name == "LOYALTY" {
            ("mana", json!("{1}"))
        } else {
            ("delta", json!({"LITERAL":1}))
        };
        for (k, x) in [("zzUnknown", json!(1)), foreign, ("Cost", json!(name))] {
            reject_v::<CostAST>(&with(&full, k, x.clone()));
            reject_v::<AbilityAST>(&activated(json!([with(&full, k, x)])));
        }
        if !members.iter().any(|(k, ..)| *k == "selector") {
            reject_v::<CostAST>(&with(&full, "selector", sel()));
        }
        // Wrong case and unknown node names.
        reject_v::<CostAST>(&with(&full, "cost", json!(name.to_lowercase())));
    }
    for bad in [
        json!({}),
        json!({"cost":""}),
        json!({"cost":"TAP"}),
        json!({"cost":"NOPE"}),
        json!({"cost":null}),
        json!({"mana":"{2}"}),
        json!("TAP_SOURCE"),
        json!(null),
        json!([]),
    ] {
        reject_v::<CostAST>(&bad);
        reject_v::<AbilityAST>(&activated(json!([bad])));
    }
}

/// REQ-DSL-007: the verified mana strings all work in PAY_MANA and WATERBEND.
#[test]
fn req_dsl_007_mana_strings_in_costs() {
    for m in MANA_OK {
        accept_v::<CostAST>(&json!({"cost":"PAY_MANA","mana":m}));
        accept_v::<CostAST>(&json!({"cost":"WATERBEND","mana":m}));
    }
}

/// REQ-DSL-007: DISCARD `random` false is kept distinct from absent.
#[test]
fn req_dsl_007_discard_random_false_distinct() {
    let base = json!({"cost":"DISCARD","selector":{"zones":["HAND"]},"count":{"LITERAL":1}});
    let a: CostAST = accept_v(&base);
    let f: CostAST = accept_v(&with(&base, "random", json!(false)));
    let t: CostAST = accept_v(&with(&base, "random", json!(true)));
    assert_ne!(a, f);
    assert_ne!(f, t);
    assert_ne!(a, t);
}

/// REQ-DSL-007: EXILE_FROM_ZONE needs exactly one of `count` and `minTotalMV`.
#[test]
fn req_dsl_007_exile_from_zone_count_xor_min_total_mv() {
    let base =
        json!({"cost":"EXILE_FROM_ZONE","zone":"GRAVEYARD","selector":{"pred":"IS_CREATURE"}});
    let c: CostAST = accept_v(&with(&base, "count", json!({"LITERAL":2})));
    let m: CostAST = accept_v(&with(&base, "minTotalMV", json!({"LITERAL":2})));
    assert_ne!(c, m);
    accept_v::<CostAST>(&with(&base, "minTotalMV", json!("X")));
    for z in ["LIBRARY", "HAND", "EXILE", "BATTLEFIELD"] {
        accept_v::<CostAST>(&with(&with(&base, "count", val()), "zone", json!(z)));
    }
    reject_v::<CostAST>(&base);
    let both = with(&with(&base, "count", val()), "minTotalMV", val());
    reject_v::<CostAST>(&both);
    reject_v::<AbilityAST>(&activated(json!([both])));
    reject_v::<AbilityAST>(&activated(json!([base])));
    reject_v::<CostAST>(&with(&base, "minTotalMV", json!(2)));
    reject_v::<CostAST>(&with(&base, "minTotalMV", json!(null)));
    reject_v::<CostAST>(&with(
        &with(&base, "count", val()),
        "minTotalMV",
        json!(null),
    ));
    reject_v::<CostAST>(&with(&base, "min_total_mv", val()));
}

/// REQ-DSL-007: COMPOSITE: non-empty `costs`; nesting is allowed; order is kept.
#[test]
fn req_dsl_007_composite() {
    let nested = json!({"cost":"COMPOSITE","costs":[
        {"cost":"COMPOSITE","costs":[{"cost":"TAP_SOURCE"}]},
        {"cost":"PAY_LIFE","amount":{"LITERAL":2}}]});
    accept_v::<CostAST>(&nested);
    accept_v::<AbilityAST>(&activated(json!([nested, {"cost":"FORAGE"}])));
    let ab: CostAST =
        accept_v(&json!({"cost":"COMPOSITE","costs":[{"cost":"TAP_SOURCE"},{"cost":"FORAGE"}]}));
    let ba: CostAST =
        accept_v(&json!({"cost":"COMPOSITE","costs":[{"cost":"FORAGE"},{"cost":"TAP_SOURCE"}]}));
    assert_ne!(ab, ba);
    reject_v::<CostAST>(&json!({"cost":"COMPOSITE","costs":[]}));
    reject_v::<CostAST>(&json!({"cost":"COMPOSITE"}));
    reject_v::<CostAST>(&json!({"cost":"COMPOSITE","costs":[{"cost":"COMPOSITE","costs":[]}]}));
    reject_v::<AbilityAST>(&activated(json!([{"cost":"COMPOSITE","costs":[]}])));
}

/// A COMPOSITE chain of `levels` around `inner`, as text (built linearly).
fn composite_chain(levels: usize, inner: &str) -> String {
    let mut s = String::with_capacity(levels * 32 + inner.len());
    for _ in 0..levels {
        s.push_str(r#"{"cost":"COMPOSITE","costs":["#);
    }
    s.push_str(inner);
    for _ in 0..levels {
        s.push_str("]}");
    }
    s
}

fn activated_text(cost_item: &str) -> String {
    format!(r#"{{"abilityId":"a","kind":"ACTIVATED","cost":[{cost_item}],"effect":[]}}"#)
}

/// A NOT chain of `levels` around `{"pred":"TAPPED"}`, as text (built linearly).
fn not_chain(levels: usize) -> String {
    let mut s = String::new();
    for _ in 0..levels {
        s.push_str(r#"{"op":"NOT","condition":"#);
    }
    s.push_str(r#"{"pred":"TAPPED"}"#);
    for _ in 0..levels {
        s.push('}');
    }
    s
}

fn reject_text<T: DeserializeOwned + Debug>(s: &str) {
    assert!(serde_json::from_str::<T>(s).is_err(), "from_str accepted");
    assert!(
        serde_json::from_slice::<T>(s.as_bytes()).is_err(),
        "from_slice accepted"
    );
}

/// REQ-DSL-007, REQ-NFR-006: a COMPOSITE chain that reaches exactly MAX_DEPTH (127) open
/// containers is accepted; one more level (128) or a much deeper chain is rejected, without a
/// stack overflow.
#[test]
fn req_dsl_007_composite_depth_budget() {
    assert_eq!(MAX_DEPTH, 127);
    // Ability object (1) + cost array (2) + 62 x (COMPOSITE object + costs array) + leaf object:
    // 2 + 124 + 1 = 127.
    let at_budget = activated_text(&composite_chain(62, r#"{"cost":"TAP_SOURCE"}"#));
    accept::<AbilityAST>(&at_budget);
    // Same with a PAY_LIFE leaf whose amount object opens container 128.
    let over = activated_text(&composite_chain(
        62,
        r#"{"cost":"PAY_LIFE","amount":{"LITERAL":1}}"#,
    ));
    reject_text::<AbilityAST>(&over);
    // Control: one level less with the same leaf is fine.
    accept::<AbilityAST>(&activated_text(&composite_chain(
        61,
        r#"{"cost":"PAY_LIFE","amount":{"LITERAL":1}}"#,
    )));
    reject_text::<AbilityAST>(&activated_text(&composite_chain(
        63,
        r#"{"cost":"TAP_SOURCE"}"#,
    )));
    // Standalone CostAST: 63 levels + leaf = 127 accepted; 64 levels = 129 rejected.
    accept::<CostAST>(&composite_chain(63, r#"{"cost":"TAP_SOURCE"}"#));
    reject_text::<CostAST>(&composite_chain(64, r#"{"cost":"TAP_SOURCE"}"#));
    for levels in [100usize, 1000, 100_000] {
        reject_text::<CostAST>(&composite_chain(levels, r#"{"cost":"TAP_SOURCE"}"#));
        reject_text::<AbilityAST>(&activated_text(&composite_chain(
            levels,
            r#"{"cost":"TAP_SOURCE"}"#,
        )));
    }
}

/// REQ-DSL-007, REQ-NFR-006: an activateOnlyIf NOT chain at exactly 127 is accepted, at 128
/// rejected.
#[test]
fn req_dsl_007_activate_only_if_depth_budget() {
    let doc = |levels: usize| {
        format!(
            r#"{{"abilityId":"a","kind":"ACTIVATED","cost":[{{"cost":"TAP_SOURCE"}}],"activateOnlyIf":{},"effect":[]}}"#,
            not_chain(levels)
        )
    };
    // Ability (1) + first NOT object (2) + ... leaf at depth 2 + levels.
    accept::<AbilityAST>(&doc(125));
    reject_text::<AbilityAST>(&doc(126));
    reject_text::<AbilityAST>(&doc(50_000));
}

/// REQ-DSL-007: ACTIVATED members: cost required non-empty, timing values, limits, flags.
#[test]
fn req_dsl_007_activated_members() {
    let base = activated(json!([{"cost":"TAP_SOURCE"}]));
    let plain: AbilityAST = accept_v(&base);
    reject_v::<AbilityAST>(&without(&base, "cost"));
    reject_v::<AbilityAST>(&with(&base, "cost", json!([])));
    reject_v::<AbilityAST>(&with(&base, "cost", json!(null)));
    reject_v::<AbilityAST>(&with(&base, "cost", json!({"cost":"TAP_SOURCE"})));
    reject_v::<AbilityAST>(&with(&base, "cost", json!("{2}")));
    reject_v::<AbilityAST>(&without(&base, "effect"));
    let mut seen = vec![plain.clone()];
    for t in TIMINGS {
        let a: AbilityAST = accept_v(&with(&base, "timing", json!(t)));
        assert!(!seen.contains(&a), "{t}");
        seen.push(a);
    }
    for t in [
        json!("instant"),
        json!("ANY_TIME"),
        json!(""),
        json!(null),
        json!(1),
    ] {
        reject_v::<AbilityAST>(&with(&base, "timing", t));
    }
    for l in [
        json!([{"perTurn":1}]),
        json!([{"perGame":1}]),
        json!([{"perTurn":2,"perGame":5}]),
        json!([{"perTurn":1},{"perGame":2}]),
    ] {
        let a: AbilityAST = accept_v(&with(&base, "limits", l));
        assert_ne!(a, plain);
    }
    for l in [
        json!([]),
        json!([{}]),
        json!([{"perTurn":0}]),
        json!([{"perGame":0}]),
        json!([{"perTurn":1},{"perTurn":0}]),
        json!([{"perTurn":-1}]),
        json!([{"perTurn":4294967296u64}]),
        json!([{"perTurn":null}]),
        json!([{"perTurn":1,"zzUnknown":1}]),
        json!([null]),
        json!({"perTurn":1}),
        json!(null),
    ] {
        reject_v::<AbilityAST>(&with(&base, "limits", l));
    }
    for (k, v) in [
        ("activateOnlyIf", json!({"pred":"TAPPED"})),
        (
            "activateOnlyIf",
            json!({"op":"NOT","condition":{"pred":"TAPPED"}}),
        ),
        ("isLoyalty", json!(true)),
        ("isLoyalty", json!(false)),
        ("isManaAbility", json!(true)),
        ("isManaAbility", json!(false)),
    ] {
        let a: AbilityAST = accept_v(&with(&base, k, v));
        assert_ne!(a, plain, "{k}");
    }
    let lf: AbilityAST = accept_v(&with(&base, "isLoyalty", json!(false)));
    let lt: AbilityAST = accept_v(&with(&base, "isLoyalty", json!(true)));
    assert_ne!(lf, lt);
    for (k, v) in [
        ("activateOnlyIf", json!({})),
        ("activateOnlyIf", json!(null)),
        ("isLoyalty", json!("false")),
        ("isLoyalty", json!(null)),
        ("limit", json!({"perTurn":1})),
        ("trigger", json!({"mode":"EVENT","event":{"kind":"DIES"}})),
        ("zzUnknown", json!(true)),
    ] {
        reject_v::<AbilityAST>(&with(&base, k, v));
    }
    // A cost list mixing several nodes, with a loyalty cost.
    accept_v::<AbilityAST>(&with(
        &with(&base, "isLoyalty", json!(true)),
        "cost",
        json!([{"cost":"LOYALTY","delta":{"LITERAL":1}},{"cost":"PAY_MANA","mana":"{W}{U}"}]),
    ));
}
