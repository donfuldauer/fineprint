// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit and property tests for references, selectors, predicates and value expressions
//! (REQ-DSL-003, REQ-CORE-006, REQ-NFR-006, REQ-NFR-008). Every word is synthetic (R5).
//!
//! The generators and property helpers are shared with the `text` and `choice` tests.

use std::fmt::Debug;

use fineprint_base::prng::Pcg64Dxsm;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::*;
use crate::canonical::{MAX_DEPTH, canonicalize, to_canonical_vec};

// ---------------------------------------------------------------------------------------------
// Shared helpers

/// Decodes `doc`, asserts the RFC 8785 form of `doc` is the canonical encoding of the value and
/// that the value round-trips; returns the value.
pub(crate) fn accept<T>(doc: &str) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let value: T = serde_json::from_str(doc).unwrap_or_else(|e| panic!("{doc} rejected: {e}"));
    let canon = canonicalize(doc.as_bytes()).expect("canonicalize");
    assert_eq!(
        String::from_utf8(to_canonical_vec(&value).expect("encode")).expect("utf8"),
        String::from_utf8(canon).expect("utf8"),
        "canonical form of {doc}"
    );
    let again: T = serde_json::from_slice(&to_canonical_vec(&value).expect("encode"))
        .expect("decode of own encoding");
    assert_eq!(again, value);
    value
}

/// Asserts `doc` is rejected (an error, not a panic).
pub(crate) fn reject<T: DeserializeOwned + Debug>(doc: &str) {
    let r: Result<T, _> = serde_json::from_str(doc);
    assert!(r.is_err(), "{doc} was accepted as {:?}", r.ok());
}

/// A bounded index from the PRNG.
pub(crate) fn pick(rng: &mut Pcg64Dxsm, n: usize) -> usize {
    usize::try_from(rng.next_bounded(n as u64).expect("n > 0")).expect("fits")
}

/// A synthetic free-form string (R5).
pub(crate) fn gen_str(rng: &mut Pcg64Dxsm) -> String {
    const WORDS: [&str; 8] = [
        "a",
        "test.kind",
        "t1",
        "word_two",
        "\u{e9}t\u{e9}",
        "with space",
        "q\"uote",
        "\u{1F600}",
    ];
    WORDS[pick(rng, WORDS.len())].to_owned()
}

/// A generated reference.
pub(crate) fn gen_ref(rng: &mut Pcg64Dxsm) -> ReferenceAST {
    match pick(rng, 16) {
        0 => ReferenceAST::SelfRef,
        1 => ReferenceAST::SourceLki,
        2 => ReferenceAST::Controller,
        3 => ReferenceAST::Owner,
        4 => ReferenceAST::ActivePlayer,
        5 => ReferenceAST::DefendingPlayer,
        6 => ReferenceAST::Enchanted,
        7 => ReferenceAST::Equipped,
        8 => ReferenceAST::AttachedTo,
        9 => ReferenceAST::ThatPlayer,
        10 => ReferenceAST::EachOpponent,
        11 => ReferenceAST::Target {
            slot: [0, 1, 7, u32::MAX][pick(rng, 4)],
        },
        12 => ReferenceAST::Chosen { key: gen_str(rng) },
        13 => ReferenceAST::Bound {
            var: ["$x", "$Var_9", "$_"][pick(rng, 3)].to_owned(),
        },
        14 => ReferenceAST::Linked {
            link_id: gen_str(rng),
        },
        _ => ReferenceAST::Remembered { key: gen_str(rng) },
    }
}

/// A generated word reference.
pub(crate) fn gen_word(rng: &mut Pcg64Dxsm) -> WordRef {
    if pick(rng, 2) == 0 {
        WordRef::TextToken(gen_str(rng))
    } else {
        WordRef::Name(gen_str(rng))
    }
}

/// A generated player selector.
pub(crate) fn gen_player(rng: &mut Pcg64Dxsm) -> PlayerSelectorAST {
    match pick(rng, 3) {
        0 => PlayerSelectorAST::Players(PlayerGroup::AllPlayers),
        1 => PlayerSelectorAST::Players(PlayerGroup::Teammates),
        _ => PlayerSelectorAST::Ref(gen_ref(rng)),
    }
}

/// A generated selector of at most `depth` nested levels.
pub(crate) fn gen_selector(rng: &mut Pcg64Dxsm, depth: u32) -> ObjectSelectorAST {
    if pick(rng, 4) == 0 {
        return ObjectSelectorAST::Ref(gen_ref(rng));
    }
    let zones = if pick(rng, 2) == 0 {
        let mut all = Zone::ALL.to_vec();
        rng.shuffle(&mut all);
        let keep = 1 + pick(rng, all.len());
        all.truncate(keep);
        Some(all)
    } else {
        None
    };
    let predicate = if zones.is_none() || pick(rng, 2) == 0 {
        Some(Box::new(gen_pred(rng, depth.saturating_sub(1))))
    } else {
        None
    };
    let count = (depth > 0 && pick(rng, 3) == 0).then(|| Box::new(gen_value(rng, depth - 1)));
    let ordering = (pick(rng, 3) == 0).then(|| Ordering {
        by: OrderKey::ALL[pick(rng, OrderKey::ALL.len())],
        direction: Direction::ALL[pick(rng, 2)],
    });
    ObjectSelectorAST::Filter(SelectorFilter {
        zones,
        count,
        ordering,
        predicate,
    })
}

/// A generated predicate of at most `depth` nested levels.
pub(crate) fn gen_pred(rng: &mut Pcg64Dxsm, depth: u32) -> PredicateAST {
    const SIMPLE: [PredicateAST; 16] = [
        PredicateAST::IsCreature,
        PredicateAST::IsLand,
        PredicateAST::IsPermanent,
        PredicateAST::IsPlayer,
        PredicateAST::IsCardOrToken,
        PredicateAST::IsToken,
        PredicateAST::IsColorless,
        PredicateAST::IsMulticolored,
        PredicateAST::FaceUp,
        PredicateAST::FaceDown,
        PredicateAST::Tapped,
        PredicateAST::Untapped,
        PredicateAST::Attacking,
        PredicateAST::Blocking,
        PredicateAST::EnteredThisTurn,
        PredicateAST::WasCast,
    ];
    if depth == 0 {
        return match pick(rng, 4) {
            0 => PredicateAST::HasSubtype {
                value: gen_word(rng),
            },
            1 => PredicateAST::TextTokenMatch {
                text_token: gen_str(rng),
                value: gen_str(rng),
            },
            2 => PredicateAST::SameAs {
                reference: gen_ref(rng),
            },
            _ => SIMPLE[pick(rng, SIMPLE.len())].clone(),
        };
    }
    let d = depth - 1;
    let cmp = |rng: &mut Pcg64Dxsm| Cmp::ALL[pick(rng, Cmp::ALL.len())];
    match pick(rng, 22) {
        0 => {
            let n = 1 + pick(rng, 3);
            PredicateAST::And((0..n).map(|_| gen_pred(rng, d)).collect())
        }
        1 => {
            let n = 1 + pick(rng, 3);
            PredicateAST::Or((0..n).map(|_| gen_pred(rng, d)).collect())
        }
        2 => PredicateAST::Not(Box::new(gen_pred(rng, d))),
        3 => PredicateAST::HasCardType {
            value: gen_word(rng),
        },
        4 => PredicateAST::HasSupertype {
            value: gen_word(rng),
        },
        5 => PredicateAST::HasColor {
            value: gen_word(rng),
        },
        6 => PredicateAST::HasKeyword {
            keyword: gen_str(rng),
        },
        7 => PredicateAST::HasDesignation { kind: gen_str(rng) },
        8 => PredicateAST::HasCounter {
            kind: gen_str(rng),
            compare: (pick(rng, 2) == 0).then(|| (cmp(rng), Box::new(gen_value(rng, d)))),
        },
        9 => PredicateAST::ControlledBy {
            player: gen_player(rng),
        },
        10 => PredicateAST::OwnedBy {
            player: gen_player(rng),
        },
        11 => PredicateAST::ManaValue {
            cmp: cmp(rng),
            value: Box::new(gen_value(rng, d)),
        },
        12 => PredicateAST::Power {
            cmp: cmp(rng),
            value: Box::new(gen_value(rng, d)),
        },
        13 => PredicateAST::Toughness {
            cmp: cmp(rng),
            value: Box::new(gen_value(rng, d)),
        },
        14 => PredicateAST::Controls {
            selector: Box::new(gen_selector(rng, d)),
            player: (pick(rng, 2) == 0).then(|| gen_player(rng)),
        },
        15 => PredicateAST::PlayerHasMaxSpeed {
            player: gen_player(rng),
        },
        16 => PredicateAST::Compare {
            cmp: cmp(rng),
            lhs: Box::new(gen_value(rng, d)),
            rhs: Box::new(gen_value(rng, d)),
        },
        _ => gen_pred(rng, 0),
    }
}

/// A generated history query of at most `depth` nested levels.
pub(crate) fn gen_history(rng: &mut Pcg64Dxsm, depth: u32) -> HistoryQuery {
    let d = depth.saturating_sub(1);
    match pick(rng, 6) {
        0 => HistoryQuery::SpellsCastThisTurn {
            player: (pick(rng, 2) == 0).then(|| gen_player(rng)),
            condition: (pick(rng, 2) == 0).then(|| gen_pred(rng, d)),
        },
        1 => HistoryQuery::LifeLostThisTurn {
            player: gen_player(rng),
        },
        2 => HistoryQuery::LifeGainedThisTurn {
            player: gen_player(rng),
        },
        3 => HistoryQuery::CardsDrawnThisTurn {
            player: gen_player(rng),
        },
        4 => HistoryQuery::CreaturesDiedThisTurn {
            condition: (pick(rng, 2) == 0).then(|| gen_pred(rng, d)),
        },
        _ => HistoryQuery::ManaSpentToCast {
            reference: gen_ref(rng),
            color: (pick(rng, 2) == 0).then(|| gen_word(rng)),
        },
    }
}

/// A generated value expression of at most `depth` nested levels.
pub(crate) fn gen_value(rng: &mut Pcg64Dxsm, depth: u32) -> ValueExpressionAST {
    const MAX_SAFE: i64 = crate::canonical::MAX_SAFE_INTEGER;
    const LITERALS: [i64; 6] = [0, 1, -1, 42, MAX_SAFE, -MAX_SAFE];
    let leaf = |rng: &mut Pcg64Dxsm| match pick(rng, 3) {
        0 => ValueExpressionAST::Literal(LITERALS[pick(rng, LITERALS.len())]),
        1 => ValueExpressionAST::X,
        _ => ValueExpressionAST::Memory(gen_str(rng)),
    };
    if depth == 0 {
        return leaf(rng);
    }
    let d = depth - 1;
    match pick(rng, 8) {
        0 => ValueExpressionAST::Count(Box::new(gen_selector(rng, d))),
        1 => ValueExpressionAST::Attr {
            reference: gen_ref(rng),
            attr: match pick(rng, 7) {
                0 => ObjectAttr::Power,
                1 => ObjectAttr::Toughness,
                2 => ObjectAttr::ManaValue,
                3 => ObjectAttr::Loyalty,
                4 => ObjectAttr::Defense,
                5 => ObjectAttr::ColorCount,
                _ => ObjectAttr::Counters(gen_str(rng)),
            },
        },
        2 => ValueExpressionAST::PlayerAttr {
            reference: gen_ref(rng),
            attr: match pick(rng, 7) {
                0 => PlayerAttr::Life,
                1 => PlayerAttr::Speed,
                2 => PlayerAttr::HandSize,
                3 => PlayerAttr::LibrarySize,
                4 => PlayerAttr::GraveyardSize,
                5 => PlayerAttr::Counters(gen_str(rng)),
                _ => {
                    let n = 1 + pick(rng, 3);
                    PlayerAttr::Devotion((0..n).map(|_| gen_word(rng)).collect())
                }
            },
        },
        3 => ValueExpressionAST::Math {
            op: MathOp::ALL[pick(rng, MathOp::ALL.len())],
            lhs: Box::new(gen_value(rng, d)),
            rhs: Box::new(gen_value(rng, d)),
        },
        4 => ValueExpressionAST::Distinct {
            attr: DistinctAttr::ALL[pick(rng, DistinctAttr::ALL.len())],
            selector: Box::new(gen_selector(rng, d)),
        },
        5 => ValueExpressionAST::History(Box::new(gen_history(rng, d))),
        _ => leaf(rng),
    }
}

/// Writes `v` as JSON with every object's members in a random order and random whitespace.
pub(crate) fn write_shuffled(v: &Value, rng: &mut Pcg64Dxsm, out: &mut String) {
    let ws = |rng: &mut Pcg64Dxsm, out: &mut String| {
        out.push_str([" ", "", "\n", "\t ", ""][pick(rng, 5)]);
    };
    match v {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            rng.shuffle(&mut entries);
            out.push('{');
            for (i, (k, val)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                ws(rng, out);
                out.push_str(&serde_json::to_string(k).expect("key"));
                ws(rng, out);
                out.push(':');
                write_shuffled(val, rng, out);
            }
            ws(rng, out);
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                ws(rng, out);
                write_shuffled(item, rng, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).expect("scalar")),
    }
}

/// Checks every wire invariant on a generated value (REQ-DSL-003, REQ-NFR-006): round trip,
/// canonical byte stability across 10 encodings, `canonicalize(bytes) == to_canonical_vec`, and
/// equality of the decode of a shuffled-member, re-spaced spelling.
pub(crate) fn assert_wire_props<T>(value: &T, rng: &mut Pcg64Dxsm)
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let bytes = serde_json::to_vec(value).expect("encode");
    let decoded: T = serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("{} rejected: {e}", String::from_utf8_lossy(&bytes)));
    assert_eq!(&decoded, value);
    let canon = to_canonical_vec(value).expect("canonical");
    for _ in 0..10 {
        assert_eq!(to_canonical_vec(value).expect("canonical"), canon);
    }
    assert_eq!(canonicalize(&bytes).expect("canonicalize"), canon);
    let tree: Value = serde_json::from_slice(&bytes).expect("value");
    let mut shuffled = String::new();
    write_shuffled(&tree, rng, &mut shuffled);
    let reordered: T = serde_json::from_str(&shuffled).expect("shuffled decode");
    assert_eq!(&reordered, value);
    assert_eq!(to_canonical_vec(&reordered).expect("canonical"), canon);
}

/// Feeds `doc` to `T`'s decoder; whatever decodes must encode and decode to itself.
pub(crate) fn decode_never_panics<T>(doc: &[u8])
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    if let Ok(v) = serde_json::from_slice::<T>(doc) {
        let bytes = to_canonical_vec(&v).expect("decoded value must encode");
        let again: T = serde_json::from_slice(&bytes).expect("own encoding must decode");
        assert_eq!(again, v);
    }
}

/// Random bytes biased toward JSON punctuation and schema words.
pub(crate) fn random_doc(rng: &mut Pcg64Dxsm) -> Vec<u8> {
    const PIECES: [&str; 24] = [
        "{",
        "}",
        "[",
        "]",
        ",",
        ":",
        "\"",
        "\"ref\"",
        "\"pred\"",
        "\"op\"",
        "\"SELF\"",
        "\"X\"",
        "1",
        "-",
        "0",
        ".",
        "e",
        "null",
        "true",
        "\"AND\"",
        "\"LITERAL\"",
        " ",
        "\\",
        "\u{fdd0}",
    ];
    let len = pick(rng, 40);
    let mut out = Vec::new();
    for _ in 0..len {
        if pick(rng, 4) == 0 {
            out.push(u8::try_from(pick(rng, 256)).expect("byte"));
        } else {
            out.extend_from_slice(PIECES[pick(rng, PIECES.len())].as_bytes());
        }
    }
    out
}

/// Mutates a valid document: replaces, deletes or inserts bytes.
pub(crate) fn mutate(doc: &[u8], rng: &mut Pcg64Dxsm) -> Vec<u8> {
    const INSERTS: [&[u8]; 10] = [
        b"null",
        b"1.0",
        b"1e2",
        b"-0",
        b"\"\"",
        b"{}",
        b"[]",
        b",\"x\":1",
        b"9007199254740992",
        b"\"$\"",
    ];
    let mut out = doc.to_vec();
    for _ in 0..1 + pick(rng, 3) {
        if out.is_empty() {
            break;
        }
        let at = pick(rng, out.len());
        match pick(rng, 3) {
            0 => out[at] = u8::try_from(pick(rng, 128)).expect("byte"),
            1 => {
                out.remove(at);
            }
            _ => {
                let ins = INSERTS[pick(rng, INSERTS.len())];
                out.splice(at..at, ins.iter().copied());
            }
        }
    }
    out
}

/// Asserts the `as_str`/`from_wire`/`Display`/serde behaviour of one string enum.
pub(crate) fn check_wire_enum<T>(
    all: &[T],
    as_str: fn(T) -> &'static str,
    from_wire: fn(&str) -> Option<T>,
) where
    T: Copy + DeserializeOwned + Serialize + PartialEq + Debug + std::fmt::Display,
{
    for &v in all {
        let s = as_str(v);
        assert_eq!(from_wire(s), Some(v));
        assert_eq!(v.to_string(), s);
        assert_eq!(accept::<T>(&format!("\"{s}\"")), v);
        assert_eq!(from_wire(&s.to_lowercase()), None);
        reject::<T>(&format!("\"{}\"", s.to_lowercase()));
        reject::<T>(&format!("{{\"{s}\":null}}"));
    }
    assert_eq!(from_wire(""), None);
    reject::<T>("\"\"");
    reject::<T>("1");
    reject::<T>("null");
}

// ---------------------------------------------------------------------------------------------
// Simple enums

/// REQ-DSL-003: every string enum accepts exactly its UPPER_SNAKE strings (case-sensitive), and
/// `as_str`/`from_wire`/`Display` agree.
#[test]
fn req_dsl_003_string_enums_exact() {
    check_wire_enum(Zone::ALL, Zone::as_str, Zone::from_wire);
    check_wire_enum(Cmp::ALL, Cmp::as_str, Cmp::from_wire);
    check_wire_enum(MathOp::ALL, MathOp::as_str, MathOp::from_wire);
    check_wire_enum(
        DistinctAttr::ALL,
        DistinctAttr::as_str,
        DistinctAttr::from_wire,
    );
    check_wire_enum(OrderKey::ALL, OrderKey::as_str, OrderKey::from_wire);
    check_wire_enum(Direction::ALL, Direction::as_str, Direction::from_wire);
    check_wire_enum(
        PlayerGroup::ALL,
        PlayerGroup::as_str,
        PlayerGroup::from_wire,
    );
    assert_eq!(Zone::ALL.len(), 9);
    assert_eq!(Zone::OutsideGame.as_str(), "OUTSIDE_GAME");
    assert_eq!(Cmp::ALL.len(), 6);
    assert_eq!(MathOp::ALL.len(), 7);
    assert_eq!(DistinctAttr::ALL.len(), 8);
    assert_eq!(OrderKey::ALL.len(), 5);
}

// ---------------------------------------------------------------------------------------------
// ReferenceAST

/// REQ-DSL-003: each parameterless reference name decodes, re-encodes canonically and allows no
/// other member.
#[test]
fn req_dsl_003_reference_parameterless_names() {
    let names = [
        ("SELF", ReferenceAST::SelfRef),
        ("SOURCE_LKI", ReferenceAST::SourceLki),
        ("CONTROLLER", ReferenceAST::Controller),
        ("OWNER", ReferenceAST::Owner),
        ("ACTIVE_PLAYER", ReferenceAST::ActivePlayer),
        ("DEFENDING_PLAYER", ReferenceAST::DefendingPlayer),
        ("ENCHANTED", ReferenceAST::Enchanted),
        ("EQUIPPED", ReferenceAST::Equipped),
        ("ATTACHED_TO", ReferenceAST::AttachedTo),
        ("THAT_PLAYER", ReferenceAST::ThatPlayer),
        ("EACH_OPPONENT", ReferenceAST::EachOpponent),
    ];
    for (name, expected) in names {
        let r: ReferenceAST = accept(&format!("{{\"ref\":\"{name}\"}}"));
        assert_eq!(r, expected);
        assert_eq!(r.name(), name);
        reject::<ReferenceAST>(&format!("{{\"ref\":\"{name}\",\"slot\":0}}"));
        reject::<ReferenceAST>(&format!("{{\"ref\":\"{name}\",\"key\":\"k\"}}"));
        reject::<ReferenceAST>(&format!("{{\"ref\":\"{}\"}}", name.to_lowercase()));
    }
}

/// A reference with the given name and a valid parameter.
fn named_ref(name: &str) -> ReferenceAST {
    serde_json::from_value(match name {
        "TARGET" => json!({"ref": name, "slot": 3}),
        "BOUND" => json!({"ref": name, "var": "$v"}),
        "LINKED" => json!({"ref": name, "linkId": "l"}),
        _ => json!({"ref": name, "key": "k"}),
    })
    .expect("valid reference")
}

/// REQ-DSL-003: TARGET, CHOSEN, BOUND, LINKED and REMEMBERED carry their one parameter member.
#[test]
fn req_dsl_003_reference_parameterised_names() {
    assert_eq!(
        accept::<ReferenceAST>(r#"{"ref":"TARGET","slot":0}"#),
        ReferenceAST::Target { slot: 0 }
    );
    assert_eq!(
        accept::<ReferenceAST>(r#"{"ref":"TARGET","slot":4294967295}"#),
        ReferenceAST::Target { slot: u32::MAX }
    );
    assert_eq!(
        accept::<ReferenceAST>(r#"{"ref":"CHOSEN","key":"k1"}"#),
        ReferenceAST::Chosen { key: "k1".into() }
    );
    assert_eq!(
        accept::<ReferenceAST>(r#"{"ref":"BOUND","var":"$it_2"}"#),
        ReferenceAST::Bound {
            var: "$it_2".into()
        }
    );
    assert_eq!(
        accept::<ReferenceAST>(r#"{"ref":"LINKED","linkId":"l1"}"#),
        ReferenceAST::Linked {
            link_id: "l1".into()
        }
    );
    assert_eq!(
        accept::<ReferenceAST>(r#"{"ref":"REMEMBERED","key":"m"}"#),
        ReferenceAST::Remembered { key: "m".into() }
    );
    for (name, member) in [
        ("TARGET", "slot"),
        ("CHOSEN", "key"),
        ("BOUND", "var"),
        ("LINKED", "linkId"),
        ("REMEMBERED", "key"),
    ] {
        assert_eq!(named_ref(name).name(), name);
        reject::<ReferenceAST>(&format!("{{\"ref\":\"{name}\"}}"));
        reject::<ReferenceAST>(&format!("{{\"ref\":\"{name}\",\"{member}\":null}}"));
    }
}

/// REQ-DSL-003: reference rejection rules (unknown name, extra member, empty strings, bad
/// `$var`, wrong member types).
#[test]
fn req_dsl_003_reference_rejections() {
    for doc in [
        r#"{"ref":"NOBODY"}"#,
        r#"{"ref":"Self"}"#,
        r#"{}"#,
        r#"{"ref":1}"#,
        r#"{"ref":null}"#,
        r#""SELF""#,
        r#"{"ref":"SELF","extra":1}"#,
        r#"{"ref":"TARGET","slot":0,"key":"k"}"#,
        r#"{"ref":"TARGET","slot":"0"}"#,
        r#"{"ref":"TARGET","slot":-1}"#,
        r#"{"ref":"TARGET","slot":4294967296}"#,
        r#"{"ref":"TARGET","slot":1.0}"#,
        r#"{"ref":"TARGET","slot":1e0}"#,
        r#"{"ref":"TARGET","slot":true}"#,
        r#"{"ref":"CHOSEN","key":""}"#,
        r#"{"ref":"CHOSEN","key":1}"#,
        r#"{"ref":"LINKED","linkId":""}"#,
        r#"{"ref":"LINKED","link_id":"l"}"#,
        r#"{"ref":"REMEMBERED","key":""}"#,
        r#"{"ref":"BOUND","var":"x"}"#,
        r#"{"ref":"BOUND","var":"$"}"#,
        r#"{"ref":"BOUND","var":"$a-b"}"#,
        r#"{"ref":"BOUND","var":"$a b"}"#,
        r#"{"ref":"BOUND","var":"$é"}"#,
        r#"{"ref":"BOUND","var":""}"#,
        r#"{"ref":"BOUND","var":7}"#,
        r#"{"ref":"SELF","ref":"SELF"}"#,
    ] {
        reject::<ReferenceAST>(doc);
    }
}

// ---------------------------------------------------------------------------------------------
// PlayerSelectorAST

/// REQ-DSL-003: a player selector is a reference or a player group, never both.
#[test]
fn req_dsl_003_player_selector_forms() {
    assert_eq!(
        accept::<PlayerSelectorAST>(r#"{"ref":"CONTROLLER"}"#),
        PlayerSelectorAST::Ref(ReferenceAST::Controller)
    );
    assert_eq!(
        accept::<PlayerSelectorAST>(r#"{"ref":"TARGET","slot":1}"#),
        PlayerSelectorAST::Ref(ReferenceAST::Target { slot: 1 })
    );
    assert_eq!(
        accept::<PlayerSelectorAST>(r#"{"players":"ALL_PLAYERS"}"#),
        PlayerSelectorAST::Players(PlayerGroup::AllPlayers)
    );
    assert_eq!(
        accept::<PlayerSelectorAST>(r#"{"players":"TEAMMATES"}"#),
        PlayerSelectorAST::Players(PlayerGroup::Teammates)
    );
    for doc in [
        r#"{}"#,
        r#"{"players":"EVERYONE"}"#,
        r#"{"players":"all_players"}"#,
        r#"{"players":null}"#,
        r#"{"ref":"SELF","players":"ALL_PLAYERS"}"#,
        r#"{"players":"ALL_PLAYERS","zones":["HAND"]}"#,
        r#""ALL_PLAYERS""#,
    ] {
        reject::<PlayerSelectorAST>(doc);
    }
}

// ---------------------------------------------------------------------------------------------
// ObjectSelectorAST

/// REQ-DSL-003: the SRS selector examples and both selector forms decode and re-encode
/// canonically; predicate members are written flat.
#[test]
fn req_dsl_003_object_selector_forms() {
    let sel: ObjectSelectorAST = accept(r#"{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}"#);
    assert_eq!(
        sel,
        ObjectSelectorAST::Filter(SelectorFilter {
            zones: Some(vec![Zone::Battlefield]),
            count: None,
            ordering: None,
            predicate: Some(Box::new(PredicateAST::IsCreature)),
        })
    );
    assert_eq!(
        serde_json::to_value(&sel).expect("encode"),
        json!({"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"})
    );
    assert_eq!(
        accept::<ObjectSelectorAST>(r#"{"ref":"TARGET","slot":0}"#),
        ObjectSelectorAST::Ref(ReferenceAST::Target { slot: 0 })
    );
    accept::<ObjectSelectorAST>(r#"{"pred":"HAS_SUBTYPE","value":{"textToken":"t14"}}"#);
    accept::<ObjectSelectorAST>(r#"{"zones":["GRAVEYARD","EXILE"]}"#);
    accept::<ObjectSelectorAST>(
        r#"{"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"pred":"CONTROLLED_BY","player":{"ref":"CONTROLLER"}}]}"#,
    );
    let full: ObjectSelectorAST = accept(
        r#"{"zones":["HAND","LIBRARY"],"count":{"LITERAL":2},"ordering":{"by":"MANA_VALUE","direction":"DESC"},"pred":"IS_LAND"}"#,
    );
    let ObjectSelectorAST::Filter(f) = full else {
        panic!("filter expected")
    };
    assert_eq!(f.zones, Some(vec![Zone::Hand, Zone::Library]));
    assert_eq!(f.count, Some(Box::new(ValueExpressionAST::Literal(2))));
    assert_eq!(
        f.ordering,
        Some(Ordering {
            by: OrderKey::ManaValue,
            direction: Direction::Desc
        })
    );
    // A SAME_AS predicate is a filter, not the reference form.
    assert!(matches!(
        accept::<ObjectSelectorAST>(r#"{"pred":"SAME_AS","ref":"SELF"}"#),
        ObjectSelectorAST::Filter(_)
    ));
    for key in OrderKey::ALL {
        for dir in Direction::ALL {
            accept::<ObjectSelectorAST>(&format!(
                "{{\"zones\":[\"STACK\"],\"ordering\":{{\"by\":\"{key}\",\"direction\":\"{dir}\"}}}}"
            ));
        }
    }
}

/// REQ-DSL-003: selector rejections: count/ordering alone, `{}`, ref plus filter members,
/// duplicate or empty zones, bad ordering, nulls, unknown members.
#[test]
fn req_dsl_003_object_selector_rejections() {
    for doc in [
        r#"{}"#,
        r#"{"count":{"LITERAL":1}}"#,
        r#"{"ordering":{"by":"POWER","direction":"ASC"}}"#,
        r#"{"count":{"LITERAL":1},"ordering":{"by":"POWER","direction":"ASC"}}"#,
        r#"{"ref":"SELF","zones":["HAND"]}"#,
        r#"{"ref":"SELF","count":{"LITERAL":1}}"#,
        r#"{"ref":"SELF","ordering":{"by":"POWER","direction":"ASC"}}"#,
        r#"{"zones":["HAND","HAND"]}"#,
        r#"{"zones":[]}"#,
        r#"{"zones":"HAND"}"#,
        r#"{"zones":["hand"]}"#,
        r#"{"zones":null,"pred":"IS_CREATURE"}"#,
        r#"{"zones":["HAND"],"count":null}"#,
        r#"{"zones":["HAND"],"ordering":null}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"POWER"}}"#,
        r#"{"zones":["HAND"],"ordering":{"direction":"ASC"}}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"COLOR","direction":"ASC"}}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"POWER","direction":"UP"}}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"POWER","direction":"ASC","x":1}}"#,
        r#"{"zones":["HAND"],"extra":1}"#,
        r#"{"zones":["HAND"],"value":{"name":"a"}}"#,
        r#"{"zones":["HAND"],"pred":"TAPPED","op":"AND"}"#,
        r#"{"pred":"IS_CREATURE","value":{"name":"a"}}"#,
        r#"{"zones":["HAND"],"count":{"LITERAL":1.0}}"#,
        r#"[]"#,
        r#"null"#,
    ] {
        reject::<ObjectSelectorAST>(doc);
    }
}

/// REQ-DSL-003: `SelectorFilter` and `Ordering` on their own; hand-built invalid filters refuse
/// to serialise.
#[test]
fn req_dsl_003_selector_filter_standalone() {
    let f: SelectorFilter = accept(r#"{"zones":["ANTE","COMMAND","OUTSIDE_GAME"]}"#);
    assert_eq!(
        f.zones,
        Some(vec![Zone::Ante, Zone::Command, Zone::OutsideGame])
    );
    reject::<SelectorFilter>(r#"{"ref":"SELF"}"#);
    reject::<SelectorFilter>(r#"{}"#);
    let ord: Ordering = accept(r#"{"by":"ZONE_POSITION","direction":"ASC"}"#);
    assert_eq!(ord.by, OrderKey::ZonePosition);
    let empty = SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: None,
    };
    assert!(serde_json::to_string(&empty).is_err());
    let dup = SelectorFilter {
        zones: Some(vec![Zone::Hand, Zone::Hand]),
        ..empty.clone()
    };
    assert!(serde_json::to_string(&dup).is_err());
    let none = SelectorFilter {
        zones: Some(Vec::new()),
        ..empty
    };
    assert!(serde_json::to_string(&ObjectSelectorAST::Filter(none)).is_err());
}

// ---------------------------------------------------------------------------------------------
// PredicateAST

/// REQ-DSL-003: every parameterless predicate name decodes and allows no other member.
#[test]
fn req_dsl_003_predicate_parameterless_names() {
    let names = [
        "IS_CREATURE",
        "IS_LAND",
        "IS_PERMANENT",
        "IS_PLAYER",
        "IS_CARD_OR_TOKEN",
        "IS_TOKEN",
        "IS_COLORLESS",
        "IS_MULTICOLORED",
        "FACE_UP",
        "FACE_DOWN",
        "TAPPED",
        "UNTAPPED",
        "ATTACKING",
        "BLOCKING",
        "ENTERED_THIS_TURN",
        "WAS_CAST",
    ];
    for name in names {
        let p: PredicateAST = accept(&format!("{{\"pred\":\"{name}\"}}"));
        assert_eq!(p.name(), name);
        reject::<PredicateAST>(&format!(
            "{{\"pred\":\"{name}\",\"value\":{{\"name\":\"a\"}}}}"
        ));
        reject::<PredicateAST>(&format!("{{\"pred\":\"{}\"}}", name.to_lowercase()));
    }
}

/// REQ-DSL-003: every predicate with members decodes from its wire form.
#[test]
fn req_dsl_003_predicate_member_names() {
    let docs = [
        (
            "HAS_CARD_TYPE",
            r#"{"pred":"HAS_CARD_TYPE","value":{"name":"test_type"}}"#,
        ),
        (
            "HAS_SUBTYPE",
            r#"{"pred":"HAS_SUBTYPE","value":{"textToken":"t14"}}"#,
        ),
        (
            "HAS_SUPERTYPE",
            r#"{"pred":"HAS_SUPERTYPE","value":{"name":"test_super"}}"#,
        ),
        (
            "HAS_COLOR",
            r#"{"pred":"HAS_COLOR","value":{"textToken":"t2"}}"#,
        ),
        (
            "HAS_KEYWORD",
            r#"{"pred":"HAS_KEYWORD","keyword":"TEST_KEYWORD"}"#,
        ),
        (
            "HAS_DESIGNATION",
            r#"{"pred":"HAS_DESIGNATION","kind":"test_designation"}"#,
        ),
        (
            "HAS_COUNTER",
            r#"{"pred":"HAS_COUNTER","kind":"test_counter"}"#,
        ),
        (
            "HAS_COUNTER",
            r#"{"pred":"HAS_COUNTER","kind":"test_counter","cmp":"GE","value":{"LITERAL":3}}"#,
        ),
        (
            "CONTROLLED_BY",
            r#"{"pred":"CONTROLLED_BY","player":{"ref":"CONTROLLER"}}"#,
        ),
        (
            "OWNED_BY",
            r#"{"pred":"OWNED_BY","player":{"players":"TEAMMATES"}}"#,
        ),
        (
            "MANA_VALUE",
            r#"{"pred":"MANA_VALUE","cmp":"LE","value":"X"}"#,
        ),
        (
            "POWER",
            r#"{"pred":"POWER","cmp":"GT","value":{"LITERAL":2}}"#,
        ),
        (
            "TOUGHNESS",
            r#"{"pred":"TOUGHNESS","cmp":"LT","value":{"MEMORY":"m"}}"#,
        ),
        ("SAME_AS", r#"{"pred":"SAME_AS","ref":"TARGET","slot":0}"#),
        ("SAME_AS", r#"{"pred":"SAME_AS","ref":"SELF"}"#),
        (
            "CONTROLS",
            r#"{"pred":"CONTROLS","selector":{"pred":"HAS_SUBTYPE","value":{"textToken":"t14"}}}"#,
        ),
        (
            "CONTROLS",
            r#"{"pred":"CONTROLS","selector":{"zones":["BATTLEFIELD"]},"player":{"ref":"THAT_PLAYER"}}"#,
        ),
        (
            "PLAYER_HAS_MAX_SPEED",
            r#"{"pred":"PLAYER_HAS_MAX_SPEED","player":{"ref":"OWNER"}}"#,
        ),
        (
            "COMPARE",
            r#"{"pred":"COMPARE","cmp":"NE","lhs":{"LITERAL":1},"rhs":"X"}"#,
        ),
        (
            "TEXT_TOKEN_MATCH",
            r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"t3","value":"w"}"#,
        ),
    ];
    for (name, doc) in docs {
        let p: PredicateAST = accept(doc);
        assert_eq!(p.name(), name, "{doc}");
    }
    assert_eq!(
        accept::<PredicateAST>(r#"{"pred":"SAME_AS","ref":"TARGET","slot":0}"#),
        PredicateAST::SameAs {
            reference: ReferenceAST::Target { slot: 0 }
        }
    );
    for cmp in Cmp::ALL {
        accept::<PredicateAST>(&format!(
            "{{\"pred\":\"POWER\",\"cmp\":\"{cmp}\",\"value\":{{\"LITERAL\":0}}}}"
        ));
    }
}

/// REQ-DSL-003: boolean nodes AND, OR (at least one condition) and NOT.
#[test]
fn req_dsl_003_predicate_boolean_ops() {
    let p: PredicateAST = accept(
        r#"{"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"pred":"CONTROLLED_BY","player":{"ref":"CONTROLLER"}}]}"#,
    );
    assert_eq!(p.name(), "AND");
    assert_eq!(
        accept::<PredicateAST>(r#"{"op":"OR","conditions":[{"pred":"TAPPED"}]}"#),
        PredicateAST::Or(vec![PredicateAST::Tapped])
    );
    let not: PredicateAST = accept(r#"{"op":"NOT","condition":{"pred":"IS_TOKEN"}}"#);
    assert_eq!(not, PredicateAST::Not(Box::new(PredicateAST::IsToken)));
    assert_eq!(not.name(), "NOT");
    assert!(serde_json::to_string(&PredicateAST::And(Vec::new())).is_err());
    assert!(serde_json::to_string(&PredicateAST::Or(Vec::new())).is_err());
}

/// REQ-DSL-003: predicate rejections: unknown names, extra/missing members, nulls, empty
/// strings, empty AND/OR, bad HAS_COUNTER pairing, `op` and `pred` together.
#[test]
fn req_dsl_003_predicate_rejections() {
    for doc in [
        r#"{}"#,
        r#"{"pred":"IS_WIZARD"}"#,
        r#"{"op":"XOR","conditions":[{"pred":"TAPPED"}]}"#,
        r#"{"op":"and","conditions":[{"pred":"TAPPED"}]}"#,
        r#"{"op":"AND","conditions":[]}"#,
        r#"{"op":"OR","conditions":[]}"#,
        r#"{"op":"AND"}"#,
        r#"{"op":"AND","conditions":{"pred":"TAPPED"}}"#,
        r#"{"op":"AND","condition":{"pred":"TAPPED"}}"#,
        r#"{"op":"NOT","conditions":[{"pred":"TAPPED"}]}"#,
        r#"{"op":"NOT","condition":{"pred":"TAPPED"},"conditions":[{"pred":"TAPPED"}]}"#,
        r#"{"op":"NOT"}"#,
        r#"{"op":"AND","pred":"TAPPED","conditions":[{"pred":"TAPPED"}]}"#,
        r#"{"op":"AND","conditions":[{"pred":"TAPPED"}],"zones":["HAND"]}"#,
        r#"{"pred":"HAS_SUBTYPE"}"#,
        r#"{"pred":"HAS_SUBTYPE","value":"t14"}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"textToken":""}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"name":""}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"textToken":"t1","name":"a"}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"textToken":null}}"#,
        r#"{"pred":"HAS_KEYWORD","keyword":""}"#,
        r#"{"pred":"HAS_KEYWORD"}"#,
        r#"{"pred":"HAS_DESIGNATION","kind":""}"#,
        r#"{"pred":"HAS_COUNTER"}"#,
        r#"{"pred":"HAS_COUNTER","kind":""}"#,
        r#"{"pred":"HAS_COUNTER","kind":"c","cmp":"GE"}"#,
        r#"{"pred":"HAS_COUNTER","kind":"c","value":{"LITERAL":1}}"#,
        r#"{"pred":"HAS_COUNTER","kind":"c","cmp":null,"value":null}"#,
        r#"{"pred":"HAS_COUNTER","kind":"c","cmp":"GE","value":null}"#,
        r#"{"pred":"HAS_COUNTER","kind":"c","cmp":"ge","value":{"LITERAL":1}}"#,
        r#"{"pred":"CONTROLLED_BY"}"#,
        r#"{"pred":"CONTROLLED_BY","player":{"ref":"BOGUS"}}"#,
        r#"{"pred":"MANA_VALUE","cmp":"EQ"}"#,
        r#"{"pred":"MANA_VALUE","value":{"LITERAL":1}}"#,
        r#"{"pred":"MANA_VALUE","cmp":"EQUAL","value":{"LITERAL":1}}"#,
        r#"{"pred":"POWER","cmp":"EQ","value":1}"#,
        r#"{"pred":"SAME_AS"}"#,
        r#"{"pred":"SAME_AS","ref":"TARGET"}"#,
        r#"{"pred":"SAME_AS","ref":"SELF","slot":0}"#,
        r#"{"pred":"CONTROLS"}"#,
        r#"{"pred":"CONTROLS","selector":{}}"#,
        r#"{"pred":"CONTROLS","selector":{"zones":["HAND"]},"player":null}"#,
        r#"{"pred":"PLAYER_HAS_MAX_SPEED"}"#,
        r#"{"pred":"COMPARE","cmp":"EQ","lhs":"X"}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"t1"}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"","value":"w"}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"t1","value":""}"#,
        r#"{"pred":"TAPPED","extra":true}"#,
        r#"{"pred":null}"#,
        r#"{"pred":1}"#,
        r#""IS_CREATURE""#,
        r#"[{"pred":"TAPPED"}]"#,
    ] {
        reject::<PredicateAST>(doc);
    }
}

/// REQ-DSL-003: hand-built predicates with invalid strings refuse to serialise.
#[test]
fn req_dsl_003_predicate_serialise_rejects_invalid() {
    let bad = [
        PredicateAST::HasKeyword {
            keyword: String::new(),
        },
        PredicateAST::HasDesignation {
            kind: String::new(),
        },
        PredicateAST::HasCounter {
            kind: String::new(),
            compare: None,
        },
        PredicateAST::TextTokenMatch {
            text_token: String::new(),
            value: "v".into(),
        },
        PredicateAST::TextTokenMatch {
            text_token: "t".into(),
            value: String::new(),
        },
        PredicateAST::HasColor {
            value: WordRef::Name(String::new()),
        },
        PredicateAST::HasColor {
            value: WordRef::TextToken(String::new()),
        },
        PredicateAST::SameAs {
            reference: ReferenceAST::Bound { var: "x".into() },
        },
        PredicateAST::SameAs {
            reference: ReferenceAST::Chosen { key: String::new() },
        },
        PredicateAST::SameAs {
            reference: ReferenceAST::Linked {
                link_id: String::new(),
            },
        },
        PredicateAST::HasKeyword {
            keyword: "\u{fffe}".into(),
        },
    ];
    for p in bad {
        assert!(serde_json::to_string(&p).is_err(), "{p:?}");
        assert!(to_canonical_vec(&p).is_err(), "{p:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// ValueExpressionAST

/// REQ-DSL-003, REQ-CORE-006: every value expression variant decodes from its wire form.
#[test]
fn req_dsl_003_value_variants() {
    assert_eq!(
        accept::<ValueExpressionAST>(r#"{"LITERAL":1}"#),
        ValueExpressionAST::Literal(1)
    );
    assert_eq!(
        accept::<ValueExpressionAST>(r#""X""#),
        ValueExpressionAST::X
    );
    let docs = [
        ("LITERAL", r#"{"LITERAL":-5}"#),
        ("X", r#""X""#),
        (
            "COUNT",
            r#"{"COUNT":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}}"#,
        ),
        ("COUNT", r#"{"COUNT":{"ref":"SELF"}}"#),
        (
            "ATTR",
            r#"{"ATTR":{"ref":"TARGET","slot":0,"attr":"POWER"}}"#,
        ),
        (
            "PLAYER_ATTR",
            r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":"LIFE"}}"#,
        ),
        (
            "MATH",
            r#"{"MATH":{"op":"ADD","lhs":{"LITERAL":1},"rhs":"X"}}"#,
        ),
        (
            "DISTINCT",
            r#"{"DISTINCT":{"attr":"CARD_TYPE","selector":{"zones":["GRAVEYARD"]}}}"#,
        ),
        ("HISTORY", r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":{}}}"#),
        ("MEMORY", r#"{"MEMORY":"m1"}"#),
    ];
    for (name, doc) in docs {
        let v: ValueExpressionAST = accept(doc);
        assert_eq!(v.name(), name, "{doc}");
    }
    for op in MathOp::ALL {
        accept::<ValueExpressionAST>(&format!(
            "{{\"MATH\":{{\"op\":\"{op}\",\"lhs\":\"X\",\"rhs\":{{\"LITERAL\":2}}}}}}"
        ));
    }
    for attr in DistinctAttr::ALL {
        accept::<ValueExpressionAST>(&format!(
            "{{\"DISTINCT\":{{\"attr\":\"{attr}\",\"selector\":{{\"pred\":\"IS_CARD_OR_TOKEN\"}}}}}}"
        ));
    }
}

/// REQ-DSL-003: every ObjectAttr and PlayerAttr form, standalone and inside ATTR/PLAYER_ATTR.
#[test]
fn req_dsl_003_value_attrs() {
    for (doc, attr) in [
        (r#""POWER""#, ObjectAttr::Power),
        (r#""TOUGHNESS""#, ObjectAttr::Toughness),
        (r#""MANA_VALUE""#, ObjectAttr::ManaValue),
        (r#""LOYALTY""#, ObjectAttr::Loyalty),
        (r#""DEFENSE""#, ObjectAttr::Defense),
        (r#""COLOR_COUNT""#, ObjectAttr::ColorCount),
        (
            r#"{"COUNTERS":"test_counter"}"#,
            ObjectAttr::Counters("test_counter".into()),
        ),
    ] {
        assert_eq!(accept::<ObjectAttr>(doc), attr);
        accept::<ValueExpressionAST>(&format!("{{\"ATTR\":{{\"ref\":\"SELF\",\"attr\":{doc}}}}}"));
    }
    for (doc, attr) in [
        (r#""LIFE""#, PlayerAttr::Life),
        (r#""SPEED""#, PlayerAttr::Speed),
        (r#""HAND_SIZE""#, PlayerAttr::HandSize),
        (r#""LIBRARY_SIZE""#, PlayerAttr::LibrarySize),
        (r#""GRAVEYARD_SIZE""#, PlayerAttr::GraveyardSize),
        (
            r#"{"COUNTERS":"test_counter"}"#,
            PlayerAttr::Counters("test_counter".into()),
        ),
        (
            r#"{"DEVOTION":[{"textToken":"t1"},{"name":"test_color"}]}"#,
            PlayerAttr::Devotion(vec![
                WordRef::TextToken("t1".into()),
                WordRef::Name("test_color".into()),
            ]),
        ),
    ] {
        assert_eq!(accept::<PlayerAttr>(doc), attr);
        accept::<ValueExpressionAST>(&format!(
            "{{\"PLAYER_ATTR\":{{\"ref\":\"BOUND\",\"var\":\"$p\",\"attr\":{doc}}}}}"
        ));
    }
    for doc in [
        r#""power""#,
        r#""LIFE""#,
        r#"{"COUNTERS":""}"#,
        r#"{"COUNTERS":1}"#,
        r#"{"DEVOTION":[]}"#,
        r#"{"COUNTERS":"a","DEVOTION":[]}"#,
        r#"{}"#,
        r#"null"#,
    ] {
        reject::<ObjectAttr>(doc);
    }
    for doc in [
        r#""POWER""#,
        r#"{"DEVOTION":[]}"#,
        r#"{"DEVOTION":{"name":"a"}}"#,
        r#"{"DEVOTION":[{"name":""}]}"#,
        r#"{"COUNTERS":""}"#,
        r#"{"POWER":1}"#,
        r#"7"#,
    ] {
        reject::<PlayerAttr>(doc);
    }
    assert!(serde_json::to_string(&PlayerAttr::Devotion(Vec::new())).is_err());
    assert!(serde_json::to_string(&PlayerAttr::Counters(String::new())).is_err());
    assert!(serde_json::to_string(&ObjectAttr::Counters(String::new())).is_err());
}

/// REQ-DSL-003: every HistoryQuery variant, with and without its optional members.
#[test]
fn req_dsl_003_history_variants() {
    let docs = [
        ("SPELLS_CAST_THIS_TURN", r#"{"SPELLS_CAST_THIS_TURN":{}}"#),
        (
            "SPELLS_CAST_THIS_TURN",
            r#"{"SPELLS_CAST_THIS_TURN":{"player":{"ref":"CONTROLLER"},"where":{"pred":"IS_CREATURE"}}}"#,
        ),
        (
            "LIFE_LOST_THIS_TURN",
            r#"{"LIFE_LOST_THIS_TURN":{"player":{"ref":"EACH_OPPONENT"}}}"#,
        ),
        (
            "LIFE_GAINED_THIS_TURN",
            r#"{"LIFE_GAINED_THIS_TURN":{"player":{"ref":"CONTROLLER"}}}"#,
        ),
        (
            "CARDS_DRAWN_THIS_TURN",
            r#"{"CARDS_DRAWN_THIS_TURN":{"player":{"players":"ALL_PLAYERS"}}}"#,
        ),
        (
            "CREATURES_DIED_THIS_TURN",
            r#"{"CREATURES_DIED_THIS_TURN":{}}"#,
        ),
        (
            "CREATURES_DIED_THIS_TURN",
            r#"{"CREATURES_DIED_THIS_TURN":{"where":{"pred":"IS_TOKEN"}}}"#,
        ),
        (
            "MANA_SPENT_TO_CAST",
            r#"{"MANA_SPENT_TO_CAST":{"ref":"SELF"}}"#,
        ),
        (
            "MANA_SPENT_TO_CAST",
            r#"{"MANA_SPENT_TO_CAST":{"ref":"TARGET","slot":2,"color":{"textToken":"t5"}}}"#,
        ),
    ];
    for (name, doc) in docs {
        let q: HistoryQuery = accept(doc);
        assert_eq!(q.name(), name);
        accept::<ValueExpressionAST>(&format!("{{\"HISTORY\":{doc}}}"));
    }
    for doc in [
        r#"{"LIFE_LOST_THIS_TURN":{}}"#,
        r#"{"LIFE_GAINED_THIS_TURN":{}}"#,
        r#"{"CARDS_DRAWN_THIS_TURN":{}}"#,
        r#"{"MANA_SPENT_TO_CAST":{}}"#,
        r#"{"MANA_SPENT_TO_CAST":{"ref":"SELF","color":null}}"#,
        r#"{"SPELLS_CAST_THIS_TURN":{"player":null}}"#,
        r#"{"SPELLS_CAST_THIS_TURN":{"where":null}}"#,
        r#"{"SPELLS_CAST_THIS_TURN":{"condition":{"pred":"TAPPED"}}}"#,
        r#"{"CREATURES_DIED_THIS_TURN":{"player":{"ref":"CONTROLLER"}}}"#,
        r#"{"LIFE_LOST_THIS_TURN":{"player":{"ref":"CONTROLLER"},"where":{"pred":"TAPPED"}}}"#,
        r#"{"SPELLS_CAST_THIS_TURN":null}"#,
        r#"{"SPELLS_CAST_THIS_TURN":{},"CREATURES_DIED_THIS_TURN":{}}"#,
        r#"{"STORM":{}}"#,
        r#""SPELLS_CAST_THIS_TURN""#,
    ] {
        reject::<HistoryQuery>(doc);
    }
}

/// REQ-DSL-003: value expression rejections: aliases of X, unknown tags, extra members, nulls,
/// empty MEMORY, missing MATH operands.
#[test]
fn req_dsl_003_value_rejections() {
    for doc in [
        r#""x""#,
        r#"{"X":null}"#,
        r#"{"X":{}}"#,
        r#""LITERAL""#,
        r#"{}"#,
        r#"{"CONST":1}"#,
        r#"{"literal":1}"#,
        r#"{"LITERAL":1,"MEMORY":"m"}"#,
        r#"{"MEMORY":""}"#,
        r#"{"MEMORY":1}"#,
        r#"{"COUNT":{}}"#,
        r#"{"COUNT":null}"#,
        r#"{"ATTR":{"ref":"SELF"}}"#,
        r#"{"ATTR":{"attr":"POWER"}}"#,
        r#"{"ATTR":{"ref":{"ref":"SELF"},"attr":"POWER"}}"#,
        r#"{"ATTR":{"ref":"SELF","attr":"LIFE"}}"#,
        r#"{"ATTR":{"ref":"SELF","attr":"POWER","x":1}}"#,
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":"POWER"}}"#,
        r#"{"MATH":{"op":"ADD","lhs":"X"}}"#,
        r#"{"MATH":{"op":"POW","lhs":"X","rhs":"X"}}"#,
        r#"{"MATH":{"op":"DIV","lhs":"X","rhs":"X"}}"#,
        r#"{"MATH":{"op":"ADD","lhs":"X","rhs":"X","extra":"X"}}"#,
        r#"{"DISTINCT":{"attr":"LIFE","selector":{"zones":["HAND"]}}}"#,
        r#"{"DISTINCT":{"attr":"COLOR"}}"#,
        r#"{"HISTORY":{}}"#,
        r#"[1]"#,
        r#"1"#,
        r#"null"#,
    ] {
        reject::<ValueExpressionAST>(doc);
    }
    assert!(serde_json::to_string(&ValueExpressionAST::Memory(String::new())).is_err());
    assert!(serde_json::to_string(&ValueExpressionAST::Literal(i64::MAX)).is_err());
    assert!(serde_json::to_string(&ValueExpressionAST::Literal(i64::MIN)).is_err());
}

/// The positions where a value expression can appear, as templates with `{V}`.
const VALUE_POSITIONS: [&str; 9] = [
    "{V}",
    r#"{"MATH":{"op":"ADD","lhs":{V},"rhs":"X"}}"#,
    r#"{"MATH":{"op":"SUB","lhs":"X","rhs":{V}}}"#,
    r#"{"COUNT":{"zones":["HAND"],"count":{V}}}"#,
    r#"{"COUNT":{"pred":"MANA_VALUE","cmp":"EQ","value":{V}}}"#,
    r#"{"COUNT":{"pred":"COMPARE","cmp":"EQ","lhs":{V},"rhs":"X"}}"#,
    r#"{"COUNT":{"pred":"COMPARE","cmp":"EQ","lhs":"X","rhs":{V}}}"#,
    r#"{"COUNT":{"pred":"HAS_COUNTER","kind":"c","cmp":"EQ","value":{V}}}"#,
    r#"{"COUNT":{"pred":"POWER","cmp":"EQ","value":{V}}}"#,
];

/// REQ-DSL-003, REQ-CORE-006: LITERAL is an integer within ±(2^53 − 1) in every value position;
/// fractions, exponents, `-0.0`, out-of-range numbers, strings, booleans and null are rejected.
#[test]
fn req_core_006_literal_integer_only_everywhere() {
    for template in VALUE_POSITIONS {
        for good in ["0", "-1", "9007199254740991", "-9007199254740991"] {
            let doc = template.replace("{V}", &format!("{{\"LITERAL\":{good}}}"));
            accept::<ValueExpressionAST>(&doc);
        }
        for bad in [
            "9007199254740992",
            "-9007199254740992",
            "18446744073709551616",
            "1.0",
            "1e2",
            "1E2",
            "-0.0",
            "-0",
            "0.5",
            "\"1\"",
            "true",
            "null",
            "[]",
            "{}",
        ] {
            let doc = template.replace("{V}", &format!("{{\"LITERAL\":{bad}}}"));
            reject::<ValueExpressionAST>(&doc);
        }
    }
    let lit: ValueExpressionAST = accept(r#"{"LITERAL":-9007199254740991}"#);
    assert_eq!(lit, ValueExpressionAST::Literal(-9_007_199_254_740_991));
}

/// REQ-CORE-006: `slot` is an integer in 0..=4294967295 wherever a reference appears.
#[test]
fn req_core_006_slot_integer_only() {
    let templates = [
        r#"{"COUNT":{"ref":"TARGET","slot":{S}}}"#,
        r#"{"ATTR":{"ref":"TARGET","slot":{S},"attr":"POWER"}}"#,
        r#"{"PLAYER_ATTR":{"ref":"TARGET","slot":{S},"attr":"LIFE"}}"#,
        r#"{"HISTORY":{"MANA_SPENT_TO_CAST":{"ref":"TARGET","slot":{S}}}}"#,
        r#"{"COUNT":{"pred":"SAME_AS","ref":"TARGET","slot":{S}}}"#,
    ];
    for t in templates {
        for good in ["0", "1", "4294967295"] {
            accept::<ValueExpressionAST>(&t.replace("{S}", good));
        }
        for bad in [
            "-1",
            "4294967296",
            "1.0",
            "1e0",
            "-0",
            "\"0\"",
            "true",
            "null",
            "9007199254740992",
        ] {
            reject::<ValueExpressionAST>(&t.replace("{S}", bad));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Strictness of the buffering layer

/// REQ-DSL-003, REQ-NFR-006: duplicate member names, noncharacters, and trailing garbage are
/// rejected; an escaped spelling of a string decodes to the same value; the same rules hold
/// through a `serde_json::Value` deserializer.
#[test]
fn req_nfr_006_strict_buffering() {
    reject::<ReferenceAST>(r#"{"ref":"SELF","ref":"OWNER"}"#);
    reject::<ValueExpressionAST>(r#"{"LITERAL":1,"LITERAL":1}"#);
    reject::<PredicateAST>(r#"{"pred":"HAS_KEYWORD","keyword":"﷐"}"#);
    reject::<PredicateAST>(r#"{"pred":"HAS_KEYWORD","keyword":"a￿"}"#);
    reject::<PredicateAST>("{\"pred\":\"TAPPED\",\"\u{fdef}\":1}");
    reject::<PredicateAST>(r#"{"pred":"TAPPED"} x"#);
    let escaped = accept::<ReferenceAST>(r#"{"ref":"\u0053ELF"}"#);
    assert_eq!(escaped, ReferenceAST::SelfRef);
    let ok: PredicateAST =
        serde_json::from_value(json!({"pred":"POWER","cmp":"EQ","value":{"LITERAL":3}}))
            .expect("from value");
    assert_eq!(
        ok,
        PredicateAST::Power {
            cmp: Cmp::Eq,
            value: Box::new(ValueExpressionAST::Literal(3))
        }
    );
    let half: Value = serde_json::from_str(r#"{"LITERAL":1.5}"#).expect("parse");
    assert!(serde_json::from_value::<ValueExpressionAST>(half).is_err());
    assert!(serde_json::from_value::<ValueExpressionAST>(json!({"LITERAL": u64::MAX})).is_err());
    assert!(serde_json::from_value::<ValueExpressionAST>(json!({"LITERAL": i64::MIN})).is_err());
    assert!(serde_json::from_value::<ValueExpressionAST>(json!({"MEMORY": "\u{fdd0}"})).is_err());
    assert!(json::is_noncharacter('\u{10ffff}'));
    assert!(json::is_noncharacter('\u{fdd0}'));
    assert!(!json::is_noncharacter('a'));
}

// ---------------------------------------------------------------------------------------------
// Depth (G6)

/// `n` nested AND nodes around a TAPPED leaf: 2n + 1 open containers.
fn nested_and(n: usize) -> String {
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(r#"{"op":"AND","conditions":["#);
    }
    s.push_str(r#"{"pred":"TAPPED"}"#);
    for _ in 0..n {
        s.push_str("]}");
    }
    s
}

/// `n` nested MATH nodes (lhs side) around `leaf`: 2n + depth(leaf) open containers.
fn nested_math(n: usize, leaf: &str) -> String {
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(r#"{"MATH":{"op":"ADD","lhs":"#);
    }
    s.push_str(leaf);
    for _ in 0..n {
        s.push_str(r#","rhs":"X"}}"#);
    }
    s
}

/// REQ-NFR-006, R12: exactly MAX_DEPTH (127) open containers decode; 128 is an error, both for
/// nested AND predicates and for nested MATH values, from text and from a `serde_json::Value`.
#[test]
fn req_nfr_006_depth_127_ok_128_error() {
    assert_eq!(MAX_DEPTH, 127);
    // 2 * 63 + 1 = 127.
    let ok_and = nested_and(63);
    accept::<PredicateAST>(&ok_and);
    // 128: wrap the 127-deep AND in a NOT.
    let deep_and = format!(r#"{{"op":"NOT","condition":{ok_and}}}"#);
    reject::<PredicateAST>(&deep_and);
    // 2 * 63 + 1 = 127 for MATH with a LITERAL leaf.
    let ok_math = nested_math(63, r#"{"LITERAL":1}"#);
    accept::<ValueExpressionAST>(&ok_math);
    // 2 * 63 + 2 = 128 with a COUNT leaf.
    let deep_math = nested_math(63, r#"{"COUNT":{"pred":"TAPPED"}}"#);
    reject::<ValueExpressionAST>(&deep_math);
    // The same through serde_json::Value, whose deserializer has no depth limit of its own.
    let v: Value = serde_json::from_str(&ok_math).expect("parse");
    assert!(serde_json::from_value::<ValueExpressionAST>(v.clone()).is_ok());
    let wrapped = json!({"MATH": {"op": "MUL", "lhs": v, "rhs": "X"}});
    assert!(serde_json::from_value::<ValueExpressionAST>(wrapped).is_err());
    let and_v: Value = serde_json::from_str(&ok_and).expect("parse");
    let not_v = json!({"op": "NOT", "condition": and_v});
    assert!(serde_json::from_value::<PredicateAST>(not_v).is_err());
    let arr = json!([[[[1]]]]);
    let mut deep_arr = arr;
    for _ in 0..130 {
        deep_arr = json!([deep_arr]);
    }
    assert!(serde_json::from_value::<ValueExpressionAST>(deep_arr).is_err());
}

/// REQ-NFR-006, R12: 100000-deep inputs of every shape fail with an error, never a panic or a
/// stack overflow.
#[test]
fn req_nfr_006_very_deep_input_is_error() {
    const N: usize = 100_000;
    let deep_and = nested_and(N);
    reject::<PredicateAST>(&deep_and);
    reject::<ObjectSelectorAST>(&deep_and);
    let deep_math = nested_math(N, r#"{"LITERAL":1}"#);
    reject::<ValueExpressionAST>(&deep_math);
    let brackets = "[".repeat(N);
    let braces = r#"{"a":"#.repeat(N);
    for doc in [&brackets, &braces] {
        reject::<ReferenceAST>(doc);
        reject::<PlayerSelectorAST>(doc);
        reject::<ObjectSelectorAST>(doc);
        reject::<PredicateAST>(doc);
        reject::<ValueExpressionAST>(doc);
        reject::<HistoryQuery>(doc);
        reject::<WordRef>(doc);
    }
    // A Rust-built tree deeper than MAX_DEPTH cannot be exported canonically.
    let mut p = PredicateAST::Tapped;
    for _ in 0..200 {
        p = PredicateAST::Not(Box::new(p));
    }
    assert!(to_canonical_vec(&p).is_err());
}

// ---------------------------------------------------------------------------------------------
// Property tests

/// REQ-DSL-003, REQ-NFR-006: generated value expressions satisfy every wire invariant.
#[test]
fn req_dsl_003_property_values() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f11_0003_0001);
    for _ in 0..400 {
        let v = gen_value(&mut rng, 4);
        assert_wire_props(&v, &mut rng);
    }
}

/// REQ-DSL-003, REQ-NFR-006: generated predicates, selectors, references, player selectors,
/// history queries and word references satisfy every wire invariant.
#[test]
fn req_dsl_003_property_predicates_and_selectors() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f11_0003_0002);
    for _ in 0..300 {
        let p = gen_pred(&mut rng, 4);
        assert_wire_props(&p, &mut rng);
        let s = gen_selector(&mut rng, 3);
        assert_wire_props(&s, &mut rng);
        let r = gen_ref(&mut rng);
        assert_wire_props(&r, &mut rng);
        let pl = gen_player(&mut rng);
        assert_wire_props(&pl, &mut rng);
        let h = gen_history(&mut rng, 3);
        assert_wire_props(&h, &mut rng);
        let w = gen_word(&mut rng);
        assert_wire_props(&w, &mut rng);
    }
}

/// REQ-DSL-003, R12: bounded random bytes never panic any decoder.
#[test]
fn req_dsl_003_random_bytes_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f11_0003_0003);
    for _ in 0..3000 {
        let doc = random_doc(&mut rng);
        decode_never_panics::<ReferenceAST>(&doc);
        decode_never_panics::<PlayerSelectorAST>(&doc);
        decode_never_panics::<ObjectSelectorAST>(&doc);
        decode_never_panics::<SelectorFilter>(&doc);
        decode_never_panics::<PredicateAST>(&doc);
        decode_never_panics::<ValueExpressionAST>(&doc);
        decode_never_panics::<HistoryQuery>(&doc);
        decode_never_panics::<ObjectAttr>(&doc);
        decode_never_panics::<PlayerAttr>(&doc);
        decode_never_panics::<WordRef>(&doc);
        decode_never_panics::<Ordering>(&doc);
        decode_never_panics::<Zone>(&doc);
        decode_never_panics::<Cmp>(&doc);
    }
}

/// REQ-DSL-003, R12: mutated valid documents never panic; anything that still decodes encodes
/// and decodes to itself.
#[test]
fn req_dsl_003_mutated_documents_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f11_0003_0004);
    for _ in 0..600 {
        let v = serde_json::to_vec(&gen_value(&mut rng, 3)).expect("encode");
        let p = serde_json::to_vec(&gen_pred(&mut rng, 3)).expect("encode");
        let s = serde_json::to_vec(&gen_selector(&mut rng, 2)).expect("encode");
        for doc in [v, p, s] {
            for _ in 0..4 {
                let m = mutate(&doc, &mut rng);
                decode_never_panics::<ValueExpressionAST>(&m);
                decode_never_panics::<PredicateAST>(&m);
                decode_never_panics::<ObjectSelectorAST>(&m);
                decode_never_panics::<HistoryQuery>(&m);
            }
        }
    }
}
