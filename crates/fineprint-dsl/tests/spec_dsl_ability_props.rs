// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::ability` (FP-012 wire contract), part 4: strict decoding (W2,
//! REQ-NFR-006, REQ-CORE-006) at AbilityAST level through `from_str` and `from_slice`, and seeded
//! property and random-bytes tests (R12) for AbilityAST, EventMatcher, CostAST, StaticEffect and
//! ReplacementOp (REQ-DSL-005..009). Black-box: written from the work-item brief only. Fixed
//! seeds, no wall clock; synthetic data only (R5).

use std::fmt::Debug;

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::ability::{AbilityAST, CostAST, EventMatcher, ReplacementOp, StaticEffect};
use fineprint_dsl::canonical::{MAX_DEPTH, canonicalize, to_canonical_vec};
use fineprint_dsl::mana::ManaCostDSL;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

const EVENT_KINDS: [&str; 57] = [
    "ZONE_CHANGE",
    "ENTERS",
    "DIES",
    "LEAVES_BATTLEFIELD",
    "PUT_INTO_GRAVEYARD",
    "LEAVES_GRAVEYARD",
    "EXILED",
    "RETURNED_TO_HAND",
    "DRAWN",
    "DISCARDED",
    "MILLED",
    "CYCLED",
    "SACRIFICED",
    "DESTROYED",
    "CAST",
    "COPIED",
    "ACTIVATED",
    "TRIGGERED",
    "RESOLVED",
    "COUNTERED",
    "BECAME_TARGET",
    "ATTACKS",
    "ATTACKS_ALONE",
    "BLOCKS",
    "BECOMES_BLOCKED",
    "ATTACKER_UNBLOCKED",
    "DAMAGE_DEALT",
    "DAMAGE_DEALT_BATCH",
    "LIFE_GAINED",
    "LIFE_LOST",
    "COUNTERS_PUT",
    "COUNTERS_REMOVED",
    "TAPPED",
    "UNTAPPED",
    "TRANSFORMED",
    "CONVERTED",
    "TURNED_FACE_UP",
    "PHASED_OUT",
    "PHASED_IN",
    "ATTACHED",
    "UNATTACHED",
    "CONTROL_CHANGED",
    "TOKEN_CREATED",
    "SEARCHED_LIBRARY",
    "SHUFFLED",
    "SCRY",
    "SURVEIL",
    "EXPLORE",
    "VENTURE",
    "BLIGHT",
    "WATERBEND",
    "DESIGNATION_GAINED",
    "ROLLED_DIE",
    "FLIPPED_COIN",
    "PLAYER_LOST",
    "PLANESWALKED",
    "SPECIAL_ACTION_TAKEN",
];

const RULE_KINDS: [&str; 34] = [
    "COST_INCREASE",
    "COST_REDUCTION",
    "COST_MINIMUM",
    "COST_SET",
    "ALT_COST_GRANT",
    "ADDITIONAL_COST_REQUIRE",
    "PAYMENT_METHOD",
    "CAST_PERMISSION",
    "PLAY_PERMISSION",
    "TIMING_PERMISSION",
    "PROHIBITION",
    "ADDITIONAL_LAND_PLAY",
    "MAX_HAND_SIZE",
    "ATTACK_RESTRICTION",
    "ATTACK_REQUIREMENT",
    "ATTACK_COST",
    "BLOCK_RESTRICTION",
    "BLOCK_REQUIREMENT",
    "BLOCK_COST",
    "BLOCK_CAPACITY",
    "EVASION",
    "HASTE_AS_THOUGH",
    "ATTACK_AS_THOUGH",
    "DAMAGE_ASSIGNMENT",
    "TARGETING_RULE",
    "SKIP",
    "UNTAP_RULE",
    "MANA_RULE",
    "LIFE_RULE",
    "DRAW_RULE",
    "ZONE_RULE",
    "WIN_LOSE_RULE",
    "SBA_RULE",
    "STATION_SUBSTITUTE",
];

const LAYERS: [&str; 11] = ["1a", "1b", "2", "3", "4", "5", "6", "7a", "7b", "7c", "7d"];

const STEPS: [&str; 13] = [
    "UNTAP",
    "UPKEEP",
    "DRAW",
    "PRECOMBAT_MAIN",
    "BEGINNING_OF_COMBAT",
    "DECLARE_ATTACKERS",
    "DECLARE_BLOCKERS",
    "FIRST_STRIKE_DAMAGE",
    "COMBAT_DAMAGE",
    "END_OF_COMBAT",
    "POSTCOMBAT_MAIN",
    "END",
    "CLEANUP",
];

const TIMINGS: [&str; 5] = [
    "INSTANT",
    "SORCERY",
    "YOUR_TURN",
    "OPPONENT_TURN",
    "COMBAT_ONLY",
];

const TIER_HINTS: [&str; 3] = ["CONTROL_ETB", "COPY_ETB", "BACK_FACE_ETB"];

const ZONES: [&str; 9] = [
    "LIBRARY",
    "HAND",
    "BATTLEFIELD",
    "GRAVEYARD",
    "STACK",
    "EXILE",
    "COMMAND",
    "ANTE",
    "OUTSIDE_GAME",
];

const KINDS: [&str; 6] = [
    "SPELL",
    "ACTIVATED",
    "TRIGGERED",
    "STATIC",
    "REPLACEMENT",
    "KEYWORD",
];

/// Verified with `ManaCostDSL::parse` in `req_dsl_007_generator_mana_verified`.
const MANA: [&str; 3] = ["{2}", "{W}{U}", "{1}"];

// ---------------------------------------------------------------- helpers

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
    let canon = canonicalize(s.as_bytes()).unwrap();
    assert_eq!(
        to_canonical_vec(&t).unwrap(),
        canon,
        "canonical bytes of {s}"
    );
    let back: T = serde_json::from_slice(&canon).unwrap();
    assert_eq!(back, t);
    t
}

/// Rejected by both `from_str` and `from_slice`.
fn reject_text<T: DeserializeOwned + Debug>(s: &str) {
    let r = serde_json::from_str::<T>(s);
    assert!(r.is_err(), "expected rejection of {s}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<T>(s.as_bytes()).is_err(),
        "expected rejection (from_slice) of {s}"
    );
}

/// Rejected by `from_str`, `from_slice` and (the text being valid I-JSON) `from_value`.
fn reject_all<T: DeserializeOwned + Debug>(s: &str) {
    reject_text::<T>(s);
    let v: Value = serde_json::from_str(s).unwrap();
    assert!(
        serde_json::from_value::<T>(v).is_err(),
        "expected rejection (from_value) of {s}"
    );
}

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_bounded(n).unwrap()
}

fn pick<'a>(rng: &mut Pcg64Dxsm, items: &'a [&'a str]) -> &'a str {
    items[below(rng, items.len() as u64) as usize]
}

fn coin(rng: &mut Pcg64Dxsm) -> bool {
    below(rng, 2) == 0
}

// ---------------------------------------------------------------- 10. strictness (W2)

/// Templates with a `<<V>>` placeholder; `<<V>>` = "1" gives a valid document.
const NUMBER_TEMPLATES: [&str; 9] = [
    r#"{"abilityId":"a","kind":"SPELL","textSpan":[<<V>>,200],"effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","targets":[{"slot":<<V>>,"selector":{"ref":"SELF"},"min":0,"max":1}],"effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","targets":[{"slot":0,"selector":{"ref":"SELF"},"min":0,"max":<<V>>}],"effect":[]}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"limit":{"perTurn":<<V>>},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","effect":[{"op":"TEST_OP","n":<<V>>}]}"#,
    r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"PAY_LIFE","amount":{"LITERAL":<<V>>}}],"effect":[]}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","filters":{"n":<<V>>}}},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"PREVENT"}],"shield":{"uses":<<V>>}}"#,
    r#"{"abilityId":"a","kind":"STATIC","effects":[{"type":"CHAR","layer":"7b","affected":{"ref":"SELF"},"modification":{"op":"SET_BASE_PT","power":<<V>>,"toughness":1}}]}"#,
];

/// REQ-NFR-006, REQ-CORE-006 (W2, W6): 1.0, 1e2, -0 and 9007199254740992 are rejected in count
/// and value positions; the same document with `1` decodes.
#[test]
fn req_core_006_non_integer_and_unsafe_numbers_rejected() {
    for t in NUMBER_TEMPLATES {
        accept::<AbilityAST>(&t.replace("<<V>>", "1"));
        for bad in [
            "1.0",
            "1e2",
            "1E2",
            "-0",
            "1.5",
            "9007199254740992",
            "-9007199254740992",
            "1e400",
        ] {
            reject_text::<AbilityAST>(&t.replace("<<V>>", bad));
        }
    }
}

/// Templates with a `<<K>>` member-name placeholder; `<<K>>` = "k" gives a valid document.
const NAME_TEMPLATES: [&str; 4] = [
    r#"{"abilityId":"a","kind":"SPELL","effect":[{"op":"TEST_OP","<<K>>":1}]}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","filters":{"<<K>>":true}}},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","bind":{"<<K>>":"$v"}}},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"KEYWORD","keyword":"TEST_KW","params":{"<<K>>":1}}"#,
];

/// Templates with a `<<S>>` string-value placeholder; `<<S>>` = "s" gives a valid document.
const STRING_TEMPLATES: [&str; 6] = [
    r#"{"abilityId":"<<S>>","kind":"SPELL","effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","abilityWord":"<<S>>","effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","linkIds":["<<S>>"],"effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","effect":[{"op":"TEST_OP","s":"<<S>>"}]}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","filters":{"name":"<<S>>"}}},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"PUT_COUNTERS","on":{"ref":"SELF"},"kind":"<<S>>","amount":{"LITERAL":1}}],"effect":[]}"#,
];

/// REQ-NFR-006 (W2): noncharacters (U+FFFE, U+FFFF, U+FDD0), escaped and raw, are rejected in
/// strings and member names; U+FFFD and U+00E9 are fine.
#[test]
fn req_nfr_006_noncharacters_rejected() {
    let okays = ["k", "x\\uFFFDy", "\u{e9}", "\u{FFFD}"];
    let bads = [
        "x\\uFFFE",
        "x\\uFFFF",
        "x\\uFDD0",
        "\\uFDEF",
        "x\u{FFFE}",
        "x\u{FFFF}",
        "x\u{FDD0}",
        "\u{FDEF}",
        "\\uD83F\\uDFFE",
    ];
    for t in NAME_TEMPLATES {
        for o in okays {
            accept::<AbilityAST>(&t.replace("<<K>>", o));
        }
        for b in bads {
            reject_text::<AbilityAST>(&t.replace("<<K>>", b));
        }
    }
    for t in STRING_TEMPLATES {
        for o in okays {
            accept::<AbilityAST>(&t.replace("<<S>>", o));
        }
        for b in bads {
            reject_text::<AbilityAST>(&t.replace("<<S>>", b));
        }
    }
}

/// Valid documents with a `<<D>>` placeholder where a duplicate member is spliced in.
const DUP_TEMPLATES: [(&str, &str); 10] = [
    (
        r#"{"abilityId":"a",<<D>>"kind":"SPELL","effect":[]}"#,
        r#""abilityId":"b","#,
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL",<<D>>"effect":[]}"#,
        r#""kind":"SPELL","#,
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT",<<D>>"event":{"kind":"DIES"}},"effect":[]}"#,
        r#""mode":"EVENT","#,
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{<<D>>"kind":"DIES"}},"effect":[]}"#,
        r#""kind":"CAST","#,
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","filters":{<<D>>"n":1}}},"effect":[]}"#,
        r#""n":2,"#,
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","bind":{<<D>>"p":"$P"}}},"effect":[]}"#,
        r#""p":"$Q","#,
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL","effect":[{"op":"TEST_OP",<<D>>"n":1}]}"#,
        r#""n":1,"#,
    ),
    (
        r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"PAY_MANA",<<D>>"mana":"{2}"}],"effect":[]}"#,
        r#""mana":"{1}","#,
    ),
    (
        r#"{"abilityId":"a","kind":"STATIC","effects":[{"type":"CHAR","layer":"6","affected":{"ref":"SELF"},"modification":{"op":"TEST_OP",<<D>>"x":1}}]}"#,
        r#""x":1,"#,
    ),
    (
        r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES",<<D>>"affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#,
        r#""event":"DIES","#,
    ),
];

/// REQ-NFR-006 (W2): duplicate member names at top level and nested are rejected.
#[test]
fn req_nfr_006_duplicate_members_rejected() {
    for (t, d) in DUP_TEMPLATES {
        accept::<AbilityAST>(&t.replace("<<D>>", ""));
        reject_text::<AbilityAST>(&t.replace("<<D>>", d));
    }
}

/// Valid documents with a `<<U>>` placeholder right after the `{` of one object of each kind.
const UNKNOWN_TEMPLATES: [&str; 30] = [
    r#"{<<U>>"abilityId":"a","kind":"SPELL","effect":[]}"#,
    r#"{<<U>>"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"effect":[]}"#,
    r#"{<<U>>"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"effect":[]}"#,
    r#"{<<U>>"abilityId":"a","kind":"STATIC","effects":[{"type":"RULE","kind":"SKIP"}]}"#,
    r#"{<<U>>"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#,
    r#"{<<U>>"abilityId":"a","kind":"KEYWORD","keyword":"TEST_KW"}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{<<U>>"mode":"EVENT","event":{"kind":"DIES"}},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{<<U>>"mode":"PHASE_STEP","step":"UPKEEP","whose":{"ref":"CONTROLLER"}},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{<<U>>"mode":"STATE","state":{"pred":"TAPPED"}},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{<<U>>"kind":"DIES"}},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"limit":{<<U>>"perTurn":1},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"limits":[{<<U>>"perGame":1}],"effect":[]}"#,
    r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{<<U>>"cost":"TAP_SOURCE"}],"effect":[]}"#,
    r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"COMPOSITE","costs":[{<<U>>"cost":"FORAGE"}]}],"effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","targets":[{<<U>>"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1}],"effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","modes":{<<U>>"min":1,"max":1,"modes":[{"id":"m","effect":[]}]},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","modes":{"min":1,"max":1,"modes":[{<<U>>"id":"m","effect":[]}]},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"STATIC","effects":[{<<U>>"type":"CHAR","layer":"6","affected":{"ref":"SELF"},"modification":{"op":"TEST_OP"}}]}"#,
    r#"{"abilityId":"a","kind":"STATIC","effects":[{<<U>>"type":"PLAYER","affected":{"ref":"CONTROLLER"},"modification":{"op":"TEST_OP"}}]}"#,
    r#"{"abilityId":"a","kind":"STATIC","effects":[{"type":"CHAR","layer":"6","affected":{"ref":"SELF"},"modification":{"op":"TEST_OP"}},{"type":"CHAR","layer":"7b","affected":{<<U>>"sameAsLayer":"6"},"modification":{"op":"TEST_OP"}}]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{<<U>>"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{<<U>>"op":"SKIP"}]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{<<U>>"op":"PREVENT"}]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{<<U>>"op":"MODIFY_EVENT","set":{"to":"EXILE"}}]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{<<U>>"op":"INSTEAD","instructions":[{"op":"TEST_OP"}]}]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{<<U>>"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"COLOR"},"store":"k"}]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"CHOOSE","choice":{<<U>>"chooser":{"ref":"CONTROLLER"},"kind":"COLOR"},"store":"k"}]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}],"shield":{<<U>>"uses":1}}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","subject":{<<U>>"ref":"SELF"}}},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"SPELL","condition":{<<U>>"pred":"TAPPED"},"effect":[]}"#,
];

/// REQ-NFR-006 (W1): an unknown member is rejected in every object kind of the contract.
#[test]
fn req_nfr_006_unknown_member_in_every_object_rejected() {
    for t in UNKNOWN_TEMPLATES {
        accept::<AbilityAST>(&t.replace("<<U>>", ""));
        for u in [
            r#""zzUnknown":1,"#,
            r#""zzUnknown":{},"#,
            r#""lookBack":true,"#,
        ] {
            reject_all::<AbilityAST>(&t.replace("<<U>>", u));
        }
    }
}

/// Valid documents with a `<<N>>` placeholder holding the value of one optional member.
const OPTIONAL_TEMPLATES: [(&str, &str); 32] = [
    (
        r#"{"abilityId":"a","kind":"SPELL","functionsIn":<<N>>,"effect":[]}"#,
        r#"["HAND"]"#,
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL","textSpan":<<N>>,"effect":[]}"#,
        "[0,1]",
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL","linkIds":<<N>>,"effect":[]}"#,
        r#"["l"]"#,
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL","abilityWord":<<N>>,"effect":[]}"#,
        r#""w""#,
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL","condition":<<N>>,"effect":[]}"#,
        r#"{"pred":"TAPPED"}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL","isManaAbility":<<N>>,"effect":[]}"#,
        "false",
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL","targets":<<N>>,"effect":[]}"#,
        r#"[{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1}]"#,
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL","modes":<<N>>,"effect":[]}"#,
        r#"{"min":1,"max":1,"modes":[{"id":"m","effect":[]}]}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"SPELL","modes":{"min":1,"max":1,"modes":[{"id":"m","targets":<<N>>,"effect":[]}]},"effect":[]}"#,
        r#"[{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1}]"#,
    ),
    (
        r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"timing":<<N>>,"effect":[]}"#,
        r#""INSTANT""#,
    ),
    (
        r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"limits":<<N>>,"effect":[]}"#,
        r#"[{"perTurn":1}]"#,
    ),
    (
        r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"limits":[{"perTurn":1,"perGame":<<N>>}],"effect":[]}"#,
        "2",
    ),
    (
        r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"activateOnlyIf":<<N>>,"effect":[]}"#,
        r#"{"pred":"TAPPED"}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"isLoyalty":<<N>>,"effect":[]}"#,
        "true",
    ),
    (
        r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"DISCARD","selector":{"zones":["HAND"]},"count":{"LITERAL":1},"random":<<N>>}],"effect":[]}"#,
        "true",
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"interveningIf":<<N>>,"effect":[]}"#,
        r#"{"pred":"TAPPED"}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"optional":<<N>>,"effect":[]}"#,
        "false",
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"limit":<<N>>,"effect":[]}"#,
        r#"{"perGame":1}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"limit":{"perGame":1,"perTurn":<<N>>},"effect":[]}"#,
        "1",
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","subject":<<N>>}},"effect":[]}"#,
        r#"{"ref":"SELF"}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","filters":<<N>>}},"effect":[]}"#,
        r#"{"n":1}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","bind":<<N>>}},"effect":[]}"#,
        r#"{"p":"$P"}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"EVENT","event":{"kind":"DIES","batch":<<N>>}},"effect":[]}"#,
        r#""EACH""#,
    ),
    (
        r#"{"abilityId":"a","kind":"STATIC","effects":[{"type":"CHAR","layer":"6","affected":{"ref":"SELF"},"modification":{"op":"TEST_OP"},"isCDA":<<N>>}]}"#,
        "true",
    ),
    (
        r#"{"abilityId":"a","kind":"REPLACEMENT","isSelfReplacement":<<N>>,"match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#,
        "true",
    ),
    (
        r#"{"abilityId":"a","kind":"REPLACEMENT","tierHint":<<N>>,"match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#,
        r#""CONTROL_ETB""#,
    ),
    (
        r#"{"abilityId":"a","kind":"REPLACEMENT","optional":<<N>>,"match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#,
        "false",
    ),
    (
        r#"{"abilityId":"a","kind":"REPLACEMENT","overrides614_5":<<N>>,"match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#,
        "true",
    ),
    (
        r#"{"abilityId":"a","kind":"REPLACEMENT","shield":<<N>>,"match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#,
        r#"{"uses":2}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"REPLACEMENT","shield":{"uses":"COUNTER_BACKED","counterKind":<<N>>},"match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#,
        r#""test_counter""#,
    ),
    (
        r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","filters":<<N>>,"affected":{"ref":"SELF"}},"replaceWith":[{"op":"PREVENT","amount":{"LITERAL":1}}]}"#,
        r#"{"to":"EXILE"}"#,
    ),
    (
        r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"PREVENT","amount":<<N>>}]}"#,
        r#"{"LITERAL":1}"#,
    ),
];

/// REQ-NFR-006 (W1): an explicit null for every optional member is rejected; the member with a
/// value (and omitted) decodes.
#[test]
fn req_nfr_006_explicit_null_for_optional_members_rejected() {
    for (t, v) in OPTIONAL_TEMPLATES {
        accept::<AbilityAST>(&t.replace("<<N>>", v));
        reject_all::<AbilityAST>(&t.replace("<<N>>", "null"));
    }
    // KEYWORD params as an optional member.
    accept::<AbilityAST>(
        r#"{"abilityId":"a","kind":"KEYWORD","keyword":"TEST_KW","params":{"n":1}}"#,
    );
    reject_all::<AbilityAST>(
        r#"{"abilityId":"a","kind":"KEYWORD","keyword":"TEST_KW","params":null}"#,
    );
}

fn spell_with_condition(prefix_nots: usize, ands: usize) -> String {
    let mut s = String::from(r#"{"abilityId":"a","kind":"SPELL","condition":"#);
    for _ in 0..prefix_nots {
        s.push_str(r#"{"op":"NOT","condition":"#);
    }
    for _ in 0..ands {
        s.push_str(r#"{"op":"AND","conditions":["#);
    }
    s.push_str(r#"{"pred":"TAPPED"}"#);
    for _ in 0..ands {
        s.push_str("]}");
    }
    for _ in 0..prefix_nots {
        s.push('}');
    }
    s.push_str(r#","effect":[]}"#);
    s
}

/// REQ-NFR-006 (W2): nesting of 128 open containers is rejected; a legal document exactly at the
/// budget (127) is accepted. Built linearly from an AND chain.
#[test]
fn req_nfr_006_nesting_budget_and_chain() {
    assert_eq!(MAX_DEPTH, 127);
    // Ability (1) + NOT objects + 2 per AND (object + array) + leaf:
    // 1 + nots + 2 * ands + 1.
    accept::<AbilityAST>(&spell_with_condition(1, 62)); // 1 + 1 + 124 + 1 = 127
    reject_text::<AbilityAST>(&spell_with_condition(2, 62)); // 128
    accept::<AbilityAST>(&spell_with_condition(0, 62)); // 126
    reject_text::<AbilityAST>(&spell_with_condition(0, 64)); // 130
    reject_text::<AbilityAST>(&spell_with_condition(0, 50_000));
}

/// A SPELL whose condition is a NOT chain of `levels` (leaf depth 2 + levels), built as a Value
/// from the inside out (no recursion).
fn deep_value(levels: usize) -> Value {
    let mut v = json!({"pred":"TAPPED"});
    for _ in 0..levels {
        let mut m = Map::new();
        m.insert("op".into(), json!("NOT"));
        m.insert("condition".into(), v);
        v = Value::Object(m);
    }
    json!({"abilityId":"a","kind":"SPELL","condition":v,"effect":[]})
}

/// REQ-NFR-006 (W2 "whichever deserializer is used"): `from_value` also enforces the budget.
#[test]
fn req_nfr_006_nesting_budget_from_value() {
    let ok: AbilityAST = serde_json::from_value(deep_value(125)).unwrap();
    let again: AbilityAST = accept(&serde_json::to_string(&deep_value(125)).unwrap());
    assert_eq!(ok, again);
    assert!(serde_json::from_value::<AbilityAST>(deep_value(126)).is_err());
    assert!(serde_json::from_value::<AbilityAST>(deep_value(200)).is_err());
}

// ---------------------------------------------------------------- 11. generator

fn gen_pred(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 5) {
        0 => json!({"pred":"IS_CREATURE"}),
        1 => json!({"pred":"TAPPED"}),
        2 => json!({"pred":"IS_PLAYER"}),
        3 => json!({"op":"NOT","condition":{"pred":"TAPPED"}}),
        _ => {
            json!({"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"op":"NOT","condition":{"pred":"TAPPED"}}]})
        }
    }
}

fn gen_sel(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 6) {
        0 => json!({"ref":"SELF"}),
        1 => json!({"ref":"TARGET","slot":below(rng, 4)}),
        2 => json!({"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}),
        3 => json!({"pred":"IS_CREATURE"}),
        4 => json!({"zones":[pick(rng, &ZONES)]}),
        _ => json!({"ref":"CONTROLLER"}),
    }
}

fn gen_player(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 4) {
        0 => json!({"ref":"CONTROLLER"}),
        1 => json!({"players":"ALL_PLAYERS"}),
        2 => json!({"players":"TEAMMATES"}),
        _ => json!({"ref":"ACTIVE_PLAYER"}),
    }
}

fn gen_subject(rng: &mut Pcg64Dxsm) -> Value {
    if below(rng, 4) == 0 {
        json!({"players": if coin(rng) { "ALL_PLAYERS" } else { "TEAMMATES" }})
    } else {
        gen_sel(rng)
    }
}

fn gen_val(rng: &mut Pcg64Dxsm) -> Value {
    if below(rng, 4) == 0 {
        json!("X")
    } else {
        json!({"LITERAL": below(rng, 10)})
    }
}

fn gen_filter_value(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 7) {
        0 => json!(true),
        1 => json!(false),
        2 => json!(below(rng, 1000) as i64 - 500),
        3 => json!("ANY"),
        4 => json!(pick(rng, &ZONES)),
        5 => json!("TestType"),
        _ => gen_pred(rng),
    }
}

fn gen_filter_obj(rng: &mut Pcg64Dxsm, allow_empty: bool) -> Value {
    const KEYS: [&str; 6] = ["combat", "to", "from", "amount", "who", "name"];
    let lo = u64::from(!allow_empty);
    let n = lo + below(rng, 4 - lo);
    let mut m = Map::new();
    for _ in 0..n {
        m.insert(pick(rng, &KEYS).to_string(), gen_filter_value(rng));
    }
    Value::Object(m)
}

fn gen_instr(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 4) {
        0 => json!({"op":"TEST_OP"}),
        1 => json!({"op":"DRAW_TEST","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}),
        2 => json!({"op":"TEST_OP_2","n":below(rng, 100),"s":"txt","o":{"a":[1,"b",true]}}),
        _ => json!({"op":"OTHER","list":[],"empty":{},"flag":false}),
    }
}

fn gen_instrs(rng: &mut Pcg64Dxsm, min: u64) -> Value {
    let n = min + below(rng, 3);
    Value::Array((0..n).map(|_| gen_instr(rng)).collect())
}

fn gen_targets(rng: &mut Pcg64Dxsm) -> Value {
    let n = 1 + below(rng, 3);
    let mut slots: Vec<u64> = (0..6).collect();
    rng.shuffle(&mut slots);
    let mut out = Vec::new();
    for s in slots.iter().take(n as usize) {
        let max = below(rng, 4);
        let min = below(rng, max + 1);
        out.push(json!({"slot":s,"selector":gen_sel(rng),"min":min,"max":max}));
    }
    Value::Array(out)
}

fn gen_modes(rng: &mut Pcg64Dxsm) -> Value {
    let n = 1 + below(rng, 3);
    let max = below(rng, n + 1);
    let min = below(rng, max + 1);
    let mut modes = Vec::new();
    for i in 0..n {
        let mut m = Map::new();
        m.insert("id".into(), json!(format!("m{i}")));
        if coin(rng) {
            m.insert("targets".into(), gen_targets(rng));
        }
        m.insert("effect".into(), gen_instrs(rng, 0));
        modes.push(Value::Object(m));
    }
    json!({"min":min,"max":max,"modes":modes})
}

fn gen_limit(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 3) {
        0 => json!({"perTurn": 1 + below(rng, 5)}),
        1 => json!({"perGame": 1 + below(rng, 5)}),
        _ => json!({"perTurn": 1 + below(rng, 5), "perGame": 1 + below(rng, 5)}),
    }
}

fn gen_cost(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    match below(rng, if depth < 2 { 18 } else { 17 }) {
        0 => json!({"cost":"PAY_MANA","mana":pick(rng, &MANA)}),
        1 => json!({"cost":"PAY_LIFE","amount":gen_val(rng)}),
        2 => json!({"cost":"TAP_SOURCE"}),
        3 => json!({"cost":"UNTAP_SOURCE"}),
        4 => json!({"cost":"TAP_OBJECTS","selector":gen_sel(rng),"count":gen_val(rng)}),
        5 => json!({"cost":"SACRIFICE","selector":gen_sel(rng),"count":gen_val(rng)}),
        6 => {
            if coin(rng) {
                json!({"cost":"DISCARD","selector":{"zones":["HAND"]},"count":gen_val(rng),"random":coin(rng)})
            } else {
                json!({"cost":"DISCARD","selector":{"zones":["HAND"]},"count":gen_val(rng)})
            }
        }
        7 => {
            let which = if coin(rng) { "count" } else { "minTotalMV" };
            json!({"cost":"EXILE_FROM_ZONE","zone":pick(rng, &ZONES),"selector":gen_sel(rng), which: gen_val(rng)})
        }
        8 => {
            json!({"cost":"REMOVE_COUNTERS","from":gen_sel(rng),"kind":"test_counter","amount":gen_val(rng)})
        }
        9 => {
            json!({"cost":"PUT_COUNTERS","on":gen_sel(rng),"kind":"test_counter","amount":gen_val(rng)})
        }
        10 => json!({"cost":"RETURN_TO_HAND","selector":gen_sel(rng)}),
        11 => json!({"cost":"REVEAL","selector":gen_sel(rng)}),
        12 => json!({"cost":"PAY_ENERGY","amount":gen_val(rng)}),
        13 => json!({"cost":"COLLECT_EVIDENCE","n":gen_val(rng)}),
        14 => json!({"cost":"FORAGE"}),
        15 => json!({"cost":"WATERBEND","mana":pick(rng, &MANA)}),
        16 => json!({"cost":"LOYALTY","delta":{"LITERAL": below(rng, 7) as i64 - 3}}),
        _ => {
            let n = 1 + below(rng, 3);
            let costs: Vec<Value> = (0..n).map(|_| gen_cost(rng, depth + 1)).collect();
            json!({"cost":"COMPOSITE","costs":costs})
        }
    }
}

fn gen_costs(rng: &mut Pcg64Dxsm) -> Value {
    let n = 1 + below(rng, 3);
    Value::Array((0..n).map(|_| gen_cost(rng, 0)).collect())
}

fn gen_matcher(rng: &mut Pcg64Dxsm) -> Value {
    let mut m = Map::new();
    m.insert("kind".into(), json!(pick(rng, &EVENT_KINDS)));
    if coin(rng) {
        m.insert("subject".into(), gen_subject(rng));
    }
    if coin(rng) {
        m.insert("filters".into(), gen_filter_obj(rng, true));
    }
    if coin(rng) {
        let mut b = Map::new();
        for (k, v) in [("recipient", "$P"), ("source", "$src_1"), ("amount", "$N")] {
            if coin(rng) {
                b.insert(k.into(), json!(v));
            }
        }
        m.insert("bind".into(), Value::Object(b));
    }
    if coin(rng) {
        m.insert(
            "batch".into(),
            json!(if coin(rng) { "EACH" } else { "ONE_OR_MORE" }),
        );
    }
    Value::Object(m)
}

fn gen_trigger(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 3) {
        0 => json!({"mode":"EVENT","event":gen_matcher(rng)}),
        1 => json!({"mode":"PHASE_STEP","step":pick(rng, &STEPS),"whose":gen_player(rng)}),
        _ => json!({"mode":"STATE","state":gen_pred(rng)}),
    }
}

fn gen_modification(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 4) {
        0 => json!({"op":"SET_BASE_PT","power":below(rng, 10),"toughness":{"LITERAL":1}}),
        1 => json!({"op":"MODIFY_PT","power":below(rng, 5) as i64 - 2,"toughness":below(rng, 5)}),
        2 => json!({"op":"REMOVE_ALL_ABILITIES"}),
        _ => json!({"op":"TEST_OP","x":1,"note":"t"}),
    }
}

fn gen_static_effects(rng: &mut Pcg64Dxsm) -> Value {
    let n = 1 + below(rng, 3);
    let mut out = Vec::new();
    let mut selector_layers: Vec<&str> = Vec::new();
    for _ in 0..n {
        match below(rng, 3) {
            0 => {
                let layer = pick(rng, &LAYERS);
                let mut m = Map::new();
                m.insert("type".into(), json!("CHAR"));
                m.insert("layer".into(), json!(layer));
                m.insert("affected".into(), gen_sel(rng));
                m.insert("modification".into(), gen_modification(rng));
                if coin(rng) {
                    m.insert("isCDA".into(), json!(coin(rng)));
                }
                selector_layers.push(layer);
                out.push(Value::Object(m));
            }
            1 => out.push(json!({"type":"PLAYER","affected":gen_player(rng),
                "modification":{"op":"TEST_OP","value":below(rng, 9)}})),
            _ => {
                let mut m = Map::new();
                m.insert("type".into(), json!("RULE"));
                m.insert("kind".into(), json!(pick(rng, &RULE_KINDS)));
                if coin(rng) {
                    m.insert("cost".into(), gen_costs(rng));
                }
                if coin(rng) {
                    m.insert("rule".into(), json!("TEST_RULE"));
                    m.insert("who".into(), gen_pred(rng));
                }
                out.push(Value::Object(m));
            }
        }
    }
    if !selector_layers.is_empty() && coin(rng) {
        let l = pick(rng, &selector_layers);
        // The lock-in effect sits at a different layer from the one it refers to.
        let others: Vec<&str> = LAYERS.iter().copied().filter(|x| *x != l).collect();
        let own = pick(rng, &others);
        out.push(
            json!({"type":"CHAR","layer":own,"affected":{"sameAsLayer":l},
            "modification":gen_modification(rng)}),
        );
    }
    Value::Array(out)
}

fn gen_repl_op(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 5) {
        0 => json!({"op":"MODIFY_EVENT","set":gen_filter_obj(rng, false)}),
        1 => json!({"op":"INSTEAD","instructions":gen_instrs(rng, 1)}),
        2 => json!({"op":"SKIP"}),
        3 => {
            if coin(rng) {
                json!({"op":"PREVENT"})
            } else {
                json!({"op":"PREVENT","amount":gen_val(rng)})
            }
        }
        _ => {
            json!({"op":"CHOOSE","choice":{"chooser":gen_player(rng),"kind":"CREATURE_TYPE"},"store":"chosenType"})
        }
    }
}

fn gen_shield(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 3) {
        0 => json!({"uses": 1 + below(rng, 9)}),
        1 => json!({"uses":"COUNTER_BACKED"}),
        _ => json!({"uses":"COUNTER_BACKED","counterKind":"test_counter"}),
    }
}

/// A random valid ability document plus its required and optional top-level member names.
fn gen_ability(rng: &mut Pcg64Dxsm, n: u64) -> (Value, Vec<&'static str>, Vec<&'static str>) {
    let kind = pick(rng, &KINDS);
    let mut m = Map::new();
    let mut req: Vec<&'static str> = vec!["abilityId", "kind"];
    m.insert("abilityId".into(), json!(format!("a{n}")));
    m.insert("kind".into(), json!(kind));
    if coin(rng) {
        let mut zs: Vec<&str> = ZONES.to_vec();
        rng.shuffle(&mut zs);
        zs.truncate(1 + below(rng, 3) as usize);
        m.insert("functionsIn".into(), json!(zs));
    }
    if coin(rng) {
        let a = below(rng, 50);
        m.insert("textSpan".into(), json!([a, a + below(rng, 50)]));
    }
    if below(rng, 4) == 0 {
        let k = 1 + below(rng, 3);
        let ids: Vec<String> = (0..k).map(|i| format!("link{i}")).collect();
        m.insert("linkIds".into(), json!(ids));
    }
    if below(rng, 4) == 0 {
        m.insert("abilityWord".into(), json!("testword"));
    }
    if below(rng, 4) == 0 {
        m.insert("condition".into(), gen_pred(rng));
    }
    if below(rng, 4) == 0 {
        m.insert("isManaAbility".into(), json!(coin(rng)));
    }
    let opt_bool = |rng: &mut Pcg64Dxsm, m: &mut Map<String, Value>, k: &str| {
        if coin(rng) {
            let b = coin(rng);
            m.insert(k.into(), json!(b));
        }
    };
    match kind {
        "SPELL" | "ACTIVATED" | "TRIGGERED" => {
            if kind == "ACTIVATED" {
                req.push("cost");
                m.insert("cost".into(), gen_costs(rng));
                if coin(rng) {
                    m.insert("timing".into(), json!(pick(rng, &TIMINGS)));
                }
                if coin(rng) {
                    let k = 1 + below(rng, 2);
                    let ls: Vec<Value> = (0..k).map(|_| gen_limit(rng)).collect();
                    m.insert("limits".into(), json!(ls));
                }
                if coin(rng) {
                    m.insert("activateOnlyIf".into(), gen_pred(rng));
                }
                opt_bool(rng, &mut m, "isLoyalty");
            }
            if kind == "TRIGGERED" {
                req.push("trigger");
                m.insert("trigger".into(), gen_trigger(rng));
                if coin(rng) {
                    m.insert("interveningIf".into(), gen_pred(rng));
                }
                opt_bool(rng, &mut m, "optional");
                if coin(rng) {
                    m.insert("limit".into(), gen_limit(rng));
                }
            }
            if coin(rng) {
                m.insert("targets".into(), gen_targets(rng));
            }
            if below(rng, 3) == 0 {
                m.insert("modes".into(), gen_modes(rng));
            }
            req.push("effect");
            m.insert("effect".into(), gen_instrs(rng, 0));
        }
        "STATIC" => {
            req.push("effects");
            m.insert("effects".into(), gen_static_effects(rng));
        }
        "REPLACEMENT" => {
            req.push("match");
            req.push("replaceWith");
            let mut mm = Map::new();
            mm.insert("event".into(), json!(pick(rng, &EVENT_KINDS)));
            if coin(rng) {
                mm.insert("filters".into(), gen_filter_obj(rng, true));
            }
            mm.insert("affected".into(), gen_subject(rng));
            m.insert("match".into(), Value::Object(mm));
            let k = 1 + below(rng, 3);
            let ops: Vec<Value> = (0..k).map(|_| gen_repl_op(rng)).collect();
            m.insert("replaceWith".into(), json!(ops));
            opt_bool(rng, &mut m, "isSelfReplacement");
            opt_bool(rng, &mut m, "optional");
            opt_bool(rng, &mut m, "overrides614_5");
            if coin(rng) {
                m.insert("tierHint".into(), json!(pick(rng, &TIER_HINTS)));
            }
            if coin(rng) {
                m.insert("shield".into(), gen_shield(rng));
            }
        }
        _ => {
            req.push("keyword");
            m.insert(
                "keyword".into(),
                json!(pick(rng, &["TEST_KEYWORD", "TEST_KW_2", "K"])),
            );
            if coin(rng) {
                m.insert(
                    "params".into(),
                    json!({"n": below(rng, 9), "s": "x", "o": {"a": [1, true]}}),
                );
            }
        }
    }
    let opt: Vec<&'static str> = [
        "functionsIn",
        "textSpan",
        "linkIds",
        "abilityWord",
        "condition",
        "isManaAbility",
        "timing",
        "limits",
        "activateOnlyIf",
        "isLoyalty",
        "interveningIf",
        "optional",
        "limit",
        "targets",
        "modes",
        "isSelfReplacement",
        "overrides614_5",
        "tierHint",
        "shield",
        "params",
    ]
    .into_iter()
    .filter(|k| m.contains_key(*k))
    .collect();
    (Value::Object(m), req, opt)
}

fn write_shuffled(v: &Value, rng: &mut Pcg64Dxsm, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(&String, &Value)> = m.iter().collect();
            rng.shuffle(&mut entries);
            out.push_str("{ ");
            for (i, (k, val)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(" ,");
                }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push(':');
                write_shuffled(val, rng, out);
            }
            out.push_str("\n}");
        }
        Value::Array(a) => {
            out.push('[');
            for (i, val) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_shuffled(val, rng, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).unwrap()),
    }
}

/// REQ-DSL-007: the mana strings the generator uses are valid ManaCostDSL text.
#[test]
fn req_dsl_007_generator_mana_verified() {
    for m in MANA {
        assert!(ManaCostDSL::parse(m).is_ok(), "{m}");
    }
}

/// REQ-DSL-005..009, REQ-NFR-006 (R12, seed 0x0F12_0000_0000_0001): 400 random valid ability
/// documents of all six kinds decode; canonical re-encoding equals canonicalize(doc); a
/// member-shuffled spelling decodes equal with identical canonical bytes; deleting a required
/// member, adding `zzUnknown` or nulling an optional member is rejected.
#[test]
fn req_dsl_005_property_round_trip_and_mutations() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F12_0000_0000_0001);
    let mut kinds_seen: Vec<String> = Vec::new();
    for n in 0..400 {
        let (v, req, opt) = gen_ability(&mut rng, n);
        let txt = serde_json::to_string(&v).unwrap();
        let a: AbilityAST = accept(&txt);
        assert_eq!(serde_json::to_value(&a).unwrap(), v, "{txt}");
        let k = v["kind"].as_str().unwrap().to_string();
        if !kinds_seen.contains(&k) {
            kinds_seen.push(k);
        }
        let c0 = to_canonical_vec(&a).unwrap();
        let mut sh = String::new();
        write_shuffled(&v, &mut rng, &mut sh);
        let b: AbilityAST = match serde_json::from_str(&sh) {
            Ok(b) => b,
            Err(e) => panic!("shuffled {sh} rejected: {e}"),
        };
        assert_eq!(b, a);
        assert_eq!(to_canonical_vec(&b).unwrap(), c0);
        let b2: AbilityAST = serde_json::from_slice(sh.as_bytes()).unwrap();
        assert_eq!(b2, a);
        // Mutation 1: delete a required member.
        let r = req[below(&mut rng, req.len() as u64) as usize];
        let mut d = v.clone();
        d.as_object_mut().unwrap().remove(r);
        reject_all::<AbilityAST>(&serde_json::to_string(&d).unwrap());
        // Mutation 2: add an unknown member.
        let mut u = v.clone();
        u.as_object_mut()
            .unwrap()
            .insert("zzUnknown".into(), json!(below(&mut rng, 3)));
        reject_all::<AbilityAST>(&serde_json::to_string(&u).unwrap());
        // Mutation 3: replace an optional member by null.
        if !opt.is_empty() {
            let o = opt[below(&mut rng, opt.len() as u64) as usize];
            let mut z = v.clone();
            z.as_object_mut().unwrap().insert(o.into(), Value::Null);
            reject_all::<AbilityAST>(&serde_json::to_string(&z).unwrap());
        }
    }
    assert_eq!(kinds_seen.len(), 6, "{kinds_seen:?}");
}

fn try_all(bytes: &[u8]) {
    fn one<T>(bytes: &[u8])
    where
        T: DeserializeOwned + Serialize + PartialEq + Debug,
    {
        let from_bytes = serde_json::from_slice::<T>(bytes);
        if let Ok(t) = &from_bytes {
            let enc = serde_json::to_vec(t).unwrap();
            let t2: T = serde_json::from_slice(&enc).unwrap();
            assert_eq!(&t2, t);
        }
        if let Ok(s) = std::str::from_utf8(bytes) {
            let from_text = serde_json::from_str::<T>(s);
            match (&from_bytes, &from_text) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "from_slice and from_str decode differently"),
                (Err(_), Err(_)) => {}
                _ => panic!("from_slice and from_str disagree on acceptance"),
            }
        }
    }
    one::<AbilityAST>(bytes);
    one::<EventMatcher>(bytes);
    one::<CostAST>(bytes);
    one::<StaticEffect>(bytes);
    one::<ReplacementOp>(bytes);
}

/// A sub-document of a generated ability that matches one of the five types, or the whole.
fn sample_doc(rng: &mut Pcg64Dxsm, n: u64) -> Vec<u8> {
    let (v, ..) = gen_ability(rng, n);
    let kind = v["kind"].as_str().unwrap().to_string();
    let sub = match kind.as_str() {
        "TRIGGERED" if v["trigger"]["mode"] == "EVENT" && coin(rng) => {
            v["trigger"]["event"].clone()
        }
        "ACTIVATED" if coin(rng) => v["cost"][0].clone(),
        "STATIC" if coin(rng) => v["effects"][0].clone(),
        "REPLACEMENT" if coin(rng) => v["replaceWith"][0].clone(),
        _ => v,
    };
    serde_json::to_vec(&sub).unwrap()
}

/// REQ-NFR-006 (R12, seed 0x0F12_0000_0000_0002): 3,600 bounded inputs (1,200 random byte
/// strings, 1,200 truncated valid documents, 1,200 byte-flipped valid documents) never panic when
/// decoded as AbilityAST, EventMatcher, CostAST, StaticEffect or ReplacementOp; whatever is
/// accepted round-trips.
#[test]
fn req_nfr_006_ability_random_bytes_never_panic() {
    const ALPHABET: &[u8] =
        b"{}[]\":,0123456789-.e abilityIdkindSPELLeffectcostTAP_SOURCEoptypeRULEmatcheventDIES$";
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F12_0000_0000_0002);
    let mut count = 0u32;
    for i in 0..1200u64 {
        let len = below(&mut rng, 120) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                if below(&mut rng, 4) == 0 {
                    rng.next_u64() as u8
                } else {
                    ALPHABET[below(&mut rng, ALPHABET.len() as u64) as usize]
                }
            })
            .collect();
        try_all(&bytes);
        count += 1;
        let doc = sample_doc(&mut rng, i);
        let cut = below(&mut rng, doc.len() as u64) as usize;
        try_all(&doc[..cut]);
        count += 1;
        let mut flipped = sample_doc(&mut rng, i + 10_000);
        let flips = 1 + below(&mut rng, 3);
        for _ in 0..flips {
            let j = below(&mut rng, flipped.len() as u64) as usize;
            match below(&mut rng, 3) {
                0 => flipped[j] ^= 1 << below(&mut rng, 8),
                1 => {
                    flipped.remove(j);
                }
                _ => flipped.insert(j, b"{}[]\":,-.e1 tfn"[below(&mut rng, 15) as usize]),
            }
            if flipped.is_empty() {
                break;
            }
        }
        try_all(&flipped);
        count += 1;
    }
    assert!(count >= 3000);
}
