// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the ability schema (REQ-DSL-003..REQ-DSL-009, REQ-CORE-006, REQ-NFR-006,
//! REQ-NFR-008). Every word, id and keyword is synthetic (R5). The seeded property tests are in
//! the `props` submodule.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;
use crate::choice::{ChoiceKind, ChoiceSpec};
use crate::effect::Instruction;
use crate::expr::tests::{accept, check_wire_enum, reject};
use crate::expr::{
    ObjectSelectorAST, PlayerGroup, PlayerSelectorAST, PredicateAST, ReferenceAST,
    ValueExpressionAST, Zone,
};
use crate::mana::ManaCostDSL;

mod props;
mod replace_text;

// ---------------------------------------------------------------------------------------------
// Helpers

/// Minimal valid SPELL ability.
const MIN_SPELL: &str = r#"{"abilityId":"a","kind":"SPELL","effect":[]}"#;
/// Minimal valid ACTIVATED ability.
const MIN_ACTIVATED: &str =
    r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"effect":[]}"#;
/// Minimal valid TRIGGERED ability.
const MIN_TRIGGERED: &str = r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"STATE","state":{"pred":"IS_CREATURE"}},"effect":[]}"#;
/// Minimal valid STATIC ability.
const MIN_STATIC: &str =
    r#"{"abilityId":"a","kind":"STATIC","effects":[{"type":"RULE","kind":"SKIP"}]}"#;
/// Minimal valid REPLACEMENT ability.
const MIN_REPLACEMENT: &str = r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}"#;
/// Minimal valid KEYWORD ability.
const MIN_KEYWORD: &str = r#"{"abilityId":"a","kind":"KEYWORD","keyword":"TEST_KEYWORD"}"#;

/// Every minimal ability, in `AbilityAST::KINDS` order.
const MINIMAL: [&str; 6] = [
    MIN_SPELL,
    MIN_ACTIVATED,
    MIN_TRIGGERED,
    MIN_STATIC,
    MIN_REPLACEMENT,
    MIN_KEYWORD,
];

/// `doc` (an object) with `extra` members appended.
fn with(doc: &str, extra: &str) -> String {
    format!("{},{extra}}}", &doc[..doc.len() - 1])
}

/// A JSON object with member `key` removed.
fn without(doc: &str, key: &str) -> String {
    let mut v: Value = serde_json::from_str(doc).expect("doc");
    v.as_object_mut().expect("object").remove(key);
    v.to_string()
}

/// Asserts the hand-built value fails to serialize (W3).
fn no_encode<T: serde::Serialize + std::fmt::Debug>(v: &T) {
    assert!(serde_json::to_vec(v).is_err(), "{v:?} was written");
}

/// The four look-back spellings the brief names.
const LOOKBACKS: [&str; 4] = ["lookBack", "lookback", "look_back", "LOOK_BACK"];

/// A minimal valid base.
fn base() -> AbilityBase {
    AbilityBase::new("a1")
}

/// Accepts `doc` both as `AbilityAST` and as the variant struct `T`.
fn accept_both<T>(doc: &str) -> (AbilityAST, T)
where
    T: serde::de::DeserializeOwned + serde::Serialize + PartialEq + std::fmt::Debug,
{
    (accept::<AbilityAST>(doc), accept::<T>(doc))
}

// ---------------------------------------------------------------------------------------------
// SRS examples (§14.4.1–§14.4.4)

/// SRS §14.4.1 example 1.
const EX1: &str = r#"{"abilityId":"a1","kind":"TRIGGERED","functionsIn":["BATTLEFIELD"],"trigger":{"mode":"EVENT","event":{"kind":"DAMAGE_DEALT","subject":{"ref":"SELF"},"filters":{"combat":true,"recipient":{"pred":"IS_PLAYER"}},"bind":{"recipient":"$P"}}},"effect":[{"op":"DISCARD","player":{"ref":"BOUND","var":"$P"},"count":{"LITERAL":1}}]}"#;
/// SRS §14.4.1 example 2.
const EX2: &str = r#"{"abilityId":"a1","kind":"TRIGGERED","functionsIn":["BATTLEFIELD"],"trigger":{"mode":"EVENT","event":{"kind":"ATTACKS","subject":{"ref":"SELF"}}},"effect":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}]}"#;
/// SRS §14.4.2 example (adapted).
const EX3: &str = r#"{"abilityId":"a2","kind":"ACTIVATED","functionsIn":["BATTLEFIELD"],"cost":[{"cost":"PAY_MANA","mana":"{2}"},{"cost":"TAP_SOURCE"}],"timing":"SORCERY","limits":[{"perTurn":1}],"activateOnlyIf":{"pred":"CONTROLS","selector":{"pred":"HAS_SUBTYPE","value":{"name":"TestType"}}},"isLoyalty":false,"isManaAbility":false,"targets":[{"slot":0,"selector":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"min":1,"max":1}],"effect":[{"op":"TAP_OR_UNTAP","action":"TAP","objects":{"ref":"TARGET","slot":0}}]}"#;
/// SRS §14.4.3 attack cost.
const EX4: &str = r#"{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],"effects":[{"type":"RULE","kind":"ATTACK_COST","attackers":{"pred":"IS_CREATURE"},"attackTarget":{"player":{"ref":"CONTROLLER"}},"cost":[{"cost":"PAY_MANA","mana":"{2}"}],"per":"EACH_ATTACKER"}]}"#;
/// SRS §14.4.3 block restriction.
const EX5: &str = r#"{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],"effects":[{"type":"RULE","kind":"BLOCK_RESTRICTION","blockers":{"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"pred":"CONTROLLED_BY","player":"OPPONENT_OF_CONTROLLER"}]},"rule":"CANT_BLOCK"}]}"#;
/// SRS §14.4.3 block capacity.
const EX6: &str = r#"{"abilityId":"s1","kind":"STATIC","effects":[{"type":"RULE","kind":"BLOCK_CAPACITY","blockers":{"ref":"SELF"},"max":"UNBOUNDED"}]}"#;
/// SRS §14.4.3 layered pair with lock-in.
const EX7: &str = r#"{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],"effects":[{"type":"CHAR","layer":"6","affected":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"modification":{"op":"REMOVE_ALL_ABILITIES"}},{"type":"CHAR","layer":"7b","affected":{"sameAsLayer":"6"},"modification":{"op":"SET_BASE_PT","power":1,"toughness":1}}]}"#;
/// SRS §14.4.3 conditional P/T (adapted).
const EX8: &str = r#"{"abilityId":"s2","kind":"STATIC","condition":{"pred":"PLAYER_HAS_MAX_SPEED","player":{"ref":"CONTROLLER"}},"effects":[{"type":"CHAR","layer":"7c","affected":{"zones":["BATTLEFIELD"],"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"pred":"CONTROLLED_BY","player":{"ref":"CONTROLLER"}},{"op":"NOT","condition":{"pred":"SAME_AS","ref":"SELF"}}]},"modification":{"op":"MODIFY_PT","power":2,"toughness":1}}]}"#;
/// SRS §14.4.4 graveyard-to-exile.
const EX9: &str = r#"{"abilityId":"r1","kind":"REPLACEMENT","functionsIn":["BATTLEFIELD"],"match":{"event":"ZONE_CHANGE","filters":{"to":"GRAVEYARD","from":"ANY"},"affected":{"pred":"IS_CARD_OR_TOKEN"}},"replaceWith":[{"op":"MODIFY_EVENT","set":{"to":"EXILE"}}]}"#;
/// SRS §14.4.4 ETB choice (adapted).
const EX10: &str = r#"{"abilityId":"r1","kind":"REPLACEMENT","functionsIn":["STACK","BATTLEFIELD"],"match":{"event":"ENTERS","affected":{"ref":"SELF"}},"replaceWith":[{"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"CREATURE_TYPE"},"store":"chosenType"}]}"#;

/// REQ-DSL-006, REQ-NFR-006: SRS §14.4.1 example 1 (combat damage to a player, bound `$P`).
#[test]
fn req_dsl_006_srs_example_1_damage_trigger() {
    let (a, t) = accept_both::<TriggeredAbilityAST>(EX1);
    assert_eq!(a, AbilityAST::Triggered(t.clone()));
    let Trigger::Event(m) = &t.trigger else {
        panic!("event trigger expected")
    };
    assert_eq!(m.kind, EventKind::DamageDealt);
    assert_eq!(
        m.subject,
        Some(EventSubject::Object(ObjectSelectorAST::Ref(
            ReferenceAST::SelfRef
        )))
    );
    let filters = m.filters.as_ref().expect("filters");
    assert_eq!(filters["combat"], FilterValue::Bool(true));
    assert_eq!(
        filters["recipient"],
        FilterValue::Predicate(PredicateAST::IsPlayer)
    );
    assert_eq!(m.bind.as_ref().expect("bind")["recipient"], "$P");
    assert_eq!(t.effect.len(), 1);
    assert_eq!(t.effect[0].name(), "DISCARD");
    assert_eq!(t.base.functions_in, Some(vec![Zone::Battlefield]));
}

/// REQ-DSL-006, REQ-NFR-006: SRS §14.4.1 example 2 (attack trigger).
#[test]
fn req_dsl_006_srs_example_2_attack_trigger() {
    let (a, t) = accept_both::<TriggeredAbilityAST>(EX2);
    assert_eq!(a.kind(), "TRIGGERED");
    let Trigger::Event(m) = &t.trigger else {
        panic!("event trigger expected")
    };
    assert_eq!(m.kind, EventKind::Attacks);
    assert_eq!(m.filters, None);
    assert_eq!(m.batch, None);
}

/// REQ-DSL-007, REQ-NFR-006: SRS §14.4.2 example (adapted: fixed WordRef).
#[test]
fn req_dsl_007_srs_example_3_activated() {
    let (a, t) = accept_both::<ActivatedAbilityAST>(EX3);
    assert_eq!(a.base().ability_id, "a2");
    assert_eq!(
        t.cost,
        vec![
            CostAST::PayMana {
                mana: ManaCostDSL::parse("{2}").expect("mana")
            },
            CostAST::TapSource
        ]
    );
    assert_eq!(t.timing, Some(ActivationTiming::Sorcery));
    assert_eq!(
        t.limits,
        Some(vec![ActivationLimit {
            per_turn: Some(1),
            per_game: None
        }])
    );
    assert_eq!(t.is_loyalty, Some(false));
    assert_eq!(t.base.is_mana_ability, Some(false));
    assert!(t.activate_only_if.is_some());
    assert_eq!(t.targets.as_ref().expect("targets")[0].slot, 0);
}

/// REQ-DSL-008, REQ-DSL-004, REQ-NFR-006: SRS §14.4.3 attack-cost example; `cost` is typed.
#[test]
fn req_dsl_008_srs_example_4_attack_cost() {
    let (_, s) = accept_both::<StaticAbilityAST>(EX4);
    let StaticEffect::Rule(r) = &s.effects[0] else {
        panic!("RULE expected")
    };
    assert_eq!(r.kind, RuleKind::AttackCost);
    assert_eq!(r.cost.as_ref().expect("cost")[0].name(), "PAY_MANA");
    assert_eq!(
        r.params.keys().collect::<Vec<_>>(),
        ["attackTarget", "attackers", "per"]
    );
}

/// REQ-DSL-008, REQ-NFR-006: SRS §14.4.3 block-restriction example (opaque params).
#[test]
fn req_dsl_008_srs_example_5_block_restriction() {
    let (_, s) = accept_both::<StaticAbilityAST>(EX5);
    let StaticEffect::Rule(r) = &s.effects[0] else {
        panic!("RULE expected")
    };
    assert_eq!(r.kind, RuleKind::BlockRestriction);
    assert_eq!(r.params["rule"], json!("CANT_BLOCK"));
    assert_eq!(r.cost, None);
}

/// REQ-DSL-008, REQ-NFR-006: SRS §14.4.3 block-capacity example (no functionsIn).
#[test]
fn req_dsl_008_srs_example_6_block_capacity() {
    let (_, s) = accept_both::<StaticAbilityAST>(EX6);
    assert_eq!(s.base.functions_in, None);
    assert!(matches!(
        &s.effects[0],
        StaticEffect::Rule(RuleModification {
            kind: RuleKind::BlockCapacity,
            ..
        })
    ));
}

/// REQ-DSL-008, REQ-DSL-004, REQ-NFR-006: SRS §14.4.3 layered pair with 613.6 lock-in.
#[test]
fn req_dsl_008_req_dsl_004_srs_example_7_lock_in() {
    let (_, s) = accept_both::<StaticAbilityAST>(EX7);
    let StaticEffect::Char(second) = &s.effects[1] else {
        panic!("CHAR expected")
    };
    assert_eq!(second.layer, Layer::L7b);
    assert_eq!(second.affected, Affected::SameAsLayer(Layer::L6));
    assert_eq!(second.modification.power, Some(PtValue::Int(1)));
    assert_eq!(second.modification.toughness, Some(PtValue::Int(1)));
    let StaticEffect::Char(first) = &s.effects[0] else {
        panic!("CHAR expected")
    };
    assert_eq!(
        first.modification,
        Modification::new("REMOVE_ALL_ABILITIES")
    );
}

/// REQ-DSL-008, REQ-NFR-006: SRS §14.4.3 conditional P/T example (adapted selectors).
#[test]
fn req_dsl_008_srs_example_8_conditional_pt() {
    let (_, s) = accept_both::<StaticAbilityAST>(EX8);
    assert!(matches!(
        s.base.condition,
        Some(PredicateAST::PlayerHasMaxSpeed { .. })
    ));
    let StaticEffect::Char(c) = &s.effects[0] else {
        panic!("CHAR expected")
    };
    assert_eq!(c.layer, Layer::L7c);
    assert_eq!(c.modification.op, "MODIFY_PT");
    assert_eq!(c.modification.power, Some(PtValue::Int(2)));
}

/// REQ-DSL-009, REQ-NFR-006: SRS §14.4.4 graveyard-to-exile replacement example.
#[test]
fn req_dsl_009_srs_example_9_modify_event() {
    let (_, r) = accept_both::<ReplacementAbilityAST>(EX9);
    assert_eq!(r.matcher.event, EventKind::ZoneChange);
    let f = r.matcher.filters.as_ref().expect("filters");
    assert_eq!(f["to"], FilterValue::Zone(Zone::Graveyard));
    assert_eq!(f["from"], FilterValue::Any);
    let set = BTreeMap::from([("to".to_owned(), FilterValue::Zone(Zone::Exile))]);
    assert_eq!(r.replace_with, vec![ReplacementOp::ModifyEvent { set }]);
}

/// REQ-DSL-009, REQ-DSL-011, REQ-NFR-006: SRS §14.4.4 ETB-choice example (adapted: CHOOSE).
#[test]
fn req_dsl_009_srs_example_10_choose() {
    let (_, r) = accept_both::<ReplacementAbilityAST>(EX10);
    assert_eq!(
        r.base.functions_in,
        Some(vec![Zone::Stack, Zone::Battlefield])
    );
    assert_eq!(
        r.replace_with,
        vec![ReplacementOp::Choose {
            choice: Box::new(ChoiceSpec::new(
                PlayerSelectorAST::Ref(ReferenceAST::Controller),
                ChoiceKind::CreatureType
            )),
            store: "chosenType".to_owned()
        }]
    );
    assert_eq!(r.replace_with[0].name(), "CHOOSE");
}

/// REQ-DSL-005: `kind` selects the variant; a variant struct refuses another kind;
/// `AbilityAST::kind`, `base` and `KINDS` agree.
#[test]
fn req_dsl_005_kind_selects_variant() {
    reject::<TriggeredAbilityAST>(EX3);
    reject::<ActivatedAbilityAST>(EX1);
    reject::<StaticAbilityAST>(EX9);
    reject::<ReplacementAbilityAST>(EX7);
    reject::<SpellAbilityAST>(MIN_KEYWORD);
    reject::<KeywordInstanceAST>(MIN_SPELL);
    let kinds: Vec<_> = MINIMAL
        .iter()
        .map(|d| accept::<AbilityAST>(d).kind())
        .collect();
    assert_eq!(kinds, AbilityAST::KINDS);
    accept::<SpellAbilityAST>(MIN_SPELL);
    accept::<KeywordInstanceAST>(MIN_KEYWORD);
    for kind in ["spell", "Spell", "ABILITY", "", "MANA"] {
        reject::<AbilityAST>(&MIN_SPELL.replace("\"SPELL\"", &format!("{kind:?}")));
    }
    reject::<AbilityAST>(&MIN_SPELL.replace("\"SPELL\"", "1"));
    reject::<AbilityAST>(&MIN_SPELL.replace("\"SPELL\"", "null"));
    for doc in MINIMAL {
        let a: AbilityAST = serde_json::from_str(doc).expect("minimal");
        assert_eq!(a.base(), &AbilityBase::new("a"));
    }
    reject::<AbilityAST>("[]");
    reject::<AbilityAST>("null");
}

// ---------------------------------------------------------------------------------------------
// Closed sets

/// REQ-DSL-006: EventKind is closed with exactly the 57 names of the brief, in order.
#[test]
fn req_dsl_006_event_kind_closed_set() {
    const NAMES: [&str; 57] = [
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
    assert_eq!(EventKind::ALL.len(), 57);
    let names: Vec<_> = EventKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(names, NAMES);
    check_wire_enum(EventKind::ALL, EventKind::as_str, EventKind::from_wire);
    for bad in ["LOOK_BACK", "ZONE_CHANGE_X", "Enters", "SCRY "] {
        assert_eq!(EventKind::from_wire(bad), None);
        reject::<EventKind>(&format!("{bad:?}"));
    }
}

/// REQ-DSL-008: RuleKind is closed with exactly the 34 names of SRS §14.4.3, in order.
#[test]
fn req_dsl_008_rule_kind_closed_set() {
    const NAMES: [&str; 34] = [
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
    assert_eq!(RuleKind::ALL.len(), 34);
    let names: Vec<_> = RuleKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(names, NAMES);
    check_wire_enum(RuleKind::ALL, RuleKind::as_str, RuleKind::from_wire);
    for kind in RuleKind::ALL {
        accept::<StaticEffect>(&format!(r#"{{"type":"RULE","kind":"{kind}"}}"#));
    }
    reject::<StaticEffect>(r#"{"type":"RULE","kind":"COST_DOUBLE"}"#);
}

/// REQ-DSL-008: the 11 layers; wire strings are case-sensitive (`"1A"` is not a layer).
#[test]
fn req_dsl_008_layer_closed_set() {
    let names: Vec<_> = Layer::ALL.iter().map(|l| l.as_str()).collect();
    assert_eq!(
        names,
        ["1a", "1b", "2", "3", "4", "5", "6", "7a", "7b", "7c", "7d"]
    );
    for &l in Layer::ALL {
        assert_eq!(Layer::from_wire(l.as_str()), Some(l));
        assert_eq!(l.to_string(), l.as_str());
        assert_eq!(accept::<Layer>(&format!("\"{l}\"")), l);
        reject::<Layer>(&format!("\"L{l}\""));
    }
    for bad in [
        "\"1A\"", "\"7B\"", "\"1\"", "\"7\"", "\"8\"", "\"0\"", "6", "\"\"", "null",
    ] {
        reject::<Layer>(bad);
    }
    assert_eq!(Layer::from_wire("7D"), None);
}

/// REQ-DSL-006, REQ-DSL-007, REQ-DSL-009: StepId (13), EventBatch (2), ActivationTiming (5)
/// and TierHint (3) are closed, case-sensitive string sets.
#[test]
fn req_dsl_006_007_009_small_closed_sets() {
    assert_eq!(StepId::ALL.len(), 13);
    let steps: Vec<_> = StepId::ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        steps,
        [
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
            "CLEANUP"
        ]
    );
    check_wire_enum(StepId::ALL, StepId::as_str, StepId::from_wire);
    assert_eq!(
        EventBatch::ALL
            .iter()
            .map(|b| b.as_str())
            .collect::<Vec<_>>(),
        ["EACH", "ONE_OR_MORE"]
    );
    check_wire_enum(EventBatch::ALL, EventBatch::as_str, EventBatch::from_wire);
    assert_eq!(ActivationTiming::ALL.len(), 5);
    assert_eq!(
        ActivationTiming::ALL
            .iter()
            .map(|t| t.as_str())
            .collect::<Vec<_>>(),
        [
            "INSTANT",
            "SORCERY",
            "YOUR_TURN",
            "OPPONENT_TURN",
            "COMBAT_ONLY"
        ]
    );
    check_wire_enum(
        ActivationTiming::ALL,
        ActivationTiming::as_str,
        ActivationTiming::from_wire,
    );
    assert_eq!(TierHint::ALL.len(), 3);
    assert_eq!(
        TierHint::ALL.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
        ["CONTROL_ETB", "COPY_ETB", "BACK_FACE_ETB"]
    );
    check_wire_enum(TierHint::ALL, TierHint::as_str, TierHint::from_wire);
}

// ---------------------------------------------------------------------------------------------
// Costs (F)

/// One valid document per cost node, in `CostAST::NAMES` order.
const COST_DOCS: [&str; 18] = [
    r#"{"cost":"PAY_MANA","mana":"{1}{G}"}"#,
    r#"{"cost":"PAY_LIFE","amount":{"LITERAL":2}}"#,
    r#"{"cost":"TAP_SOURCE"}"#,
    r#"{"cost":"UNTAP_SOURCE"}"#,
    r#"{"cost":"TAP_OBJECTS","selector":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"count":{"LITERAL":2}}"#,
    r#"{"cost":"SACRIFICE","selector":{"ref":"SELF"},"count":{"LITERAL":1}}"#,
    r#"{"cost":"DISCARD","selector":{"zones":["HAND"]},"count":"X","random":true}"#,
    r#"{"cost":"EXILE_FROM_ZONE","zone":"GRAVEYARD","selector":{"zones":["GRAVEYARD"]},"minTotalMV":{"LITERAL":4}}"#,
    r#"{"cost":"REMOVE_COUNTERS","from":{"ref":"SELF"},"kind":"test_counter","amount":{"LITERAL":1}}"#,
    r#"{"cost":"PUT_COUNTERS","on":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"kind":"-1/-1","amount":{"LITERAL":1}}"#,
    r#"{"cost":"RETURN_TO_HAND","selector":{"zones":["BATTLEFIELD"],"pred":"IS_LAND"}}"#,
    r#"{"cost":"REVEAL","selector":{"zones":["HAND"]}}"#,
    r#"{"cost":"PAY_ENERGY","amount":{"LITERAL":3}}"#,
    r#"{"cost":"COLLECT_EVIDENCE","n":{"LITERAL":6}}"#,
    r#"{"cost":"FORAGE"}"#,
    r#"{"cost":"WATERBEND","mana":"{3}"}"#,
    r#"{"cost":"LOYALTY","delta":{"LITERAL":-2}}"#,
    r#"{"cost":"COMPOSITE","costs":[{"cost":"TAP_SOURCE"},{"cost":"PAY_LIFE","amount":{"LITERAL":1}}]}"#,
];

/// REQ-DSL-007: one node for each of the 18 cost names; `name()` matches `cost`.
#[test]
fn req_dsl_007_every_cost_node() {
    assert_eq!(CostAST::NAMES.len(), 18);
    for (doc, name) in COST_DOCS.iter().zip(CostAST::NAMES) {
        let c: CostAST = accept(doc);
        assert_eq!(c.name(), name, "{doc}");
        let in_ability = with(
            r#"{"abilityId":"a","kind":"ACTIVATED","effect":[]}"#,
            &format!("\"cost\":[{doc}]"),
        );
        accept::<AbilityAST>(&in_ability);
    }
    accept::<CostAST>(
        r#"{"cost":"EXILE_FROM_ZONE","zone":"GRAVEYARD","selector":{"zones":["GRAVEYARD"]},"count":{"LITERAL":1}}"#,
    );
    let d: CostAST = accept(
        r#"{"cost":"DISCARD","selector":{"zones":["HAND"]},"count":{"LITERAL":1},"random":false}"#,
    );
    assert!(matches!(
        d,
        CostAST::Discard {
            random: Some(false),
            ..
        }
    ));
}

/// REQ-DSL-007: cost rejections: unknown/missing/null members, EXILE_FROM_ZONE with both or
/// neither amount, empty COMPOSITE, empty counter kind, bad mana, unknown and lower-case names.
#[test]
fn req_dsl_007_cost_rejections() {
    for doc in [
        r#"{"cost":"TAP_SOURCE","count":1}"#,
        r#"{"cost":"FORAGE","x":1}"#,
        r#"{"cost":"PAY_MANA"}"#,
        r#"{"cost":"PAY_MANA","mana":""}"#,
        r#"{"cost":"PAY_MANA","mana":"2"}"#,
        r#"{"cost":"PAY_MANA","mana":{"LITERAL":2}}"#,
        r#"{"cost":"WATERBEND","mana":null}"#,
        r#"{"cost":"PAY_LIFE","amount":2}"#,
        r#"{"cost":"PAY_LIFE"}"#,
        r#"{"cost":"TAP_OBJECTS","selector":{"ref":"SELF"}}"#,
        r#"{"cost":"SACRIFICE","count":{"LITERAL":1}}"#,
        r#"{"cost":"DISCARD","selector":{"zones":["HAND"]},"count":"X","random":null}"#,
        r#"{"cost":"DISCARD","selector":{"zones":["HAND"]},"count":"X","random":1}"#,
        r#"{"cost":"EXILE_FROM_ZONE","zone":"GRAVEYARD","selector":{"zones":["GRAVEYARD"]}}"#,
        r#"{"cost":"EXILE_FROM_ZONE","zone":"GRAVEYARD","selector":{"zones":["GRAVEYARD"]},"count":{"LITERAL":1},"minTotalMV":{"LITERAL":4}}"#,
        r#"{"cost":"EXILE_FROM_ZONE","zone":"YARD","selector":{"zones":["GRAVEYARD"]},"count":{"LITERAL":1}}"#,
        r#"{"cost":"EXILE_FROM_ZONE","zone":"GRAVEYARD","selector":{"zones":["GRAVEYARD"]},"minTotalMv":{"LITERAL":4}}"#,
        r#"{"cost":"EXILE_FROM_ZONE","zone":"GRAVEYARD","selector":{"zones":["GRAVEYARD"]},"count":null,"minTotalMV":{"LITERAL":4}}"#,
        r#"{"cost":"REMOVE_COUNTERS","from":{"ref":"SELF"},"kind":"","amount":{"LITERAL":1}}"#,
        r#"{"cost":"PUT_COUNTERS","on":{"ref":"SELF"},"amount":{"LITERAL":1}}"#,
        r#"{"cost":"COLLECT_EVIDENCE","amount":{"LITERAL":6}}"#,
        r#"{"cost":"LOYALTY","delta":-2}"#,
        r#"{"cost":"COMPOSITE","costs":[]}"#,
        r#"{"cost":"COMPOSITE","costs":{"cost":"TAP_SOURCE"}}"#,
        r#"{"cost":"COMPOSITE"}"#,
        r#"{"cost":"tap_source"}"#,
        r#"{"cost":"TAP"}"#,
        r#"{"cost":null}"#,
        r#"{}"#,
        r#""TAP_SOURCE""#,
        r#"{"cost":"TAP_SOURCE","cost":"TAP_SOURCE"}"#,
    ] {
        reject::<CostAST>(doc);
    }
    reject::<AbilityAST>(r#"{"abilityId":"a","kind":"ACTIVATED","cost":[],"effect":[]}"#);
    reject::<AbilityAST>(&without(MIN_ACTIVATED, "cost"));
    reject::<AbilityAST>(
        &MIN_ACTIVATED.replace(r#"[{"cost":"TAP_SOURCE"}]"#, r#"{"cost":"TAP_SOURCE"}"#),
    );
}

/// REQ-DSL-007, REQ-DSL-006: activation timing and limits: limits non-empty, each limit has a
/// member >= 1; a trigger's `limit` is one ActivationLimit.
#[test]
fn req_dsl_007_timing_and_limits() {
    for t in ActivationTiming::ALL {
        accept::<AbilityAST>(&with(MIN_ACTIVATED, &format!("\"timing\":\"{t}\"")));
    }
    accept::<ActivationLimit>(r#"{"perTurn":1}"#);
    accept::<ActivationLimit>(r#"{"perGame":4294967295}"#);
    let both: ActivationLimit = accept(r#"{"perTurn":2,"perGame":3}"#);
    assert_eq!(
        both,
        ActivationLimit {
            per_turn: Some(2),
            per_game: Some(3)
        }
    );
    for doc in [
        "{}",
        r#"{"perTurn":0}"#,
        r#"{"perGame":0}"#,
        r#"{"perTurn":-1}"#,
        r#"{"perTurn":4294967296}"#,
        r#"{"perTurn":null}"#,
        r#"{"perTurn":1,"perRound":1}"#,
        r#"{"per_turn":1}"#,
        "[]",
    ] {
        reject::<ActivationLimit>(doc);
    }
    for extra in [
        r#""timing":"instant""#,
        r#""timing":"ANY_TIME""#,
        r#""timing":null"#,
        r#""limits":[]"#,
        r#""limits":[{}]"#,
        r#""limits":{"perTurn":1}"#,
        r#""isLoyalty":"yes""#,
        r#""activateOnlyIf":{}"#,
    ] {
        reject::<AbilityAST>(&with(MIN_ACTIVATED, extra));
    }
    accept::<AbilityAST>(&with(
        MIN_TRIGGERED,
        r#""limit":{"perTurn":1},"optional":true,"interveningIf":{"pred":"IS_CREATURE"}"#,
    ));
    reject::<AbilityAST>(&with(MIN_TRIGGERED, r#""limit":{}"#));
    reject::<AbilityAST>(&with(MIN_TRIGGERED, r#""limit":[{"perTurn":1}]"#));
    reject::<AbilityAST>(&with(MIN_TRIGGERED, r#""limits":[{"perTurn":1}]"#));
    reject::<AbilityAST>(&with(MIN_TRIGGERED, r#""optional":null"#));
    reject::<AbilityAST>(&with(MIN_TRIGGERED, r#""interveningIf":null"#));
}

// ---------------------------------------------------------------------------------------------
// Look-back (REQ-DSL-006, rule E5)

/// REQ-DSL-006: there is no look-back member on a trigger: as a member of the event matcher,
/// the trigger, the ability (every kind), a replacement match or a typed static effect (CHAR,
/// PLAYER), every spelling fails. The opaque placeholder members (effect instructions,
/// modification and RULE params, keyword params) are opaque by contract (brief H, I), so the
/// name is plain data there and sets nothing.
#[test]
fn req_dsl_006_no_lookback_member_anywhere() {
    for name in LOOKBACKS {
        let member = format!("\"{name}\":true");
        reject::<EventMatcher>(&format!(r#"{{"kind":"DIES",{member}}}"#));
        reject::<AbilityAST>(&EX2.replace(
            r#""subject":{"ref":"SELF"}"#,
            &format!(r#""subject":{{"ref":"SELF"}},{member}"#),
        ));
        reject::<Trigger>(&format!(
            r#"{{"mode":"EVENT","event":{{"kind":"DIES"}},{member}}}"#
        ));
        reject::<Trigger>(&format!(
            r#"{{"mode":"STATE","state":{{"pred":"IS_LAND"}},{member}}}"#
        ));
        reject::<AbilityAST>(&EX2.replace(
            r#""mode":"EVENT","#,
            &format!(r#""mode":"EVENT",{member},"#),
        ));
        for doc in MINIMAL {
            reject::<AbilityAST>(&with(doc, &member));
        }
        reject::<AbilityAST>(&with(EX1, &member));
        reject::<TriggeredAbilityAST>(&with(EX2, &member));
        reject::<StaticEffect>(&format!(
            r#"{{"type":"CHAR","layer":"6","affected":{{"ref":"SELF"}},"modification":{{"op":"REMOVE_ALL_ABILITIES"}},{member}}}"#
        ));
        reject::<StaticEffect>(&format!(
            r#"{{"type":"PLAYER","affected":{{"ref":"CONTROLLER"}},"modification":{{"op":"CANT_LOSE"}},{member}}}"#
        ));
        reject::<ReplacementMatch>(&format!(
            r#"{{"event":"DIES","affected":{{"ref":"SELF"}},{member}}}"#
        ));
        // A real instruction has no free-form member: it is an unknown member (REQ-DSL-010).
        reject::<Instruction>(&format!(r#"{{"op":"POPULATE",{member}}}"#));
        // Opaque placeholders keep the member as data (round-trip unchanged).
        accept::<StaticEffect>(&format!(r#"{{"type":"RULE","kind":"SKIP",{member}}}"#));
        accept::<AbilityAST>(&with(MIN_KEYWORD, &format!(r#""params":{{{member}}}"#)));
    }
}

/// REQ-DSL-006: rule E5 on `filters` and `bind` keys, on decode and on encode.
#[test]
fn req_dsl_006_lookback_keys_rule_e5() {
    for name in LOOKBACKS
        .iter()
        .chain(&["Look-Back", "LOOKBACK_X", "lookback2"])
    {
        reject::<EventMatcher>(&format!(r#"{{"kind":"DIES","filters":{{"{name}":true}}}}"#));
        reject::<EventMatcher>(&format!(r#"{{"kind":"DIES","bind":{{"{name}":"$x"}}}}"#));
        reject::<AbilityAST>(&EX1.replace(r#""combat":true"#, &format!(r#""{name}":true"#)));
        reject::<AbilityAST>(&EX1.replace(r#""recipient":"$P""#, &format!(r#""{name}":"$P""#)));
        reject::<ReplacementMatch>(&format!(
            r#"{{"event":"DIES","filters":{{"{name}":true}},"affected":{{"ref":"SELF"}}}}"#
        ));
        let f = BTreeMap::from([((*name).to_owned(), FilterValue::Bool(true))]);
        let mut m = EventMatcher::new(EventKind::Dies);
        m.filters = Some(f.clone());
        no_encode(&m);
        let mut m = EventMatcher::new(EventKind::Dies);
        m.bind = Some(BTreeMap::from([((*name).to_owned(), "$x".to_owned())]));
        no_encode(&m);
        no_encode(&ReplacementMatch {
            event: EventKind::Dies,
            filters: Some(f),
            affected: EventSubject::Players(PlayerGroup::AllPlayers),
        });
    }
    accept::<EventMatcher>(
        r#"{"kind":"DIES","filters":{"back":true,"look":1},"bind":{"source":"$s"}}"#,
    );
}

// ---------------------------------------------------------------------------------------------
// Envelope (A) and per-kind members (B)

/// REQ-DSL-005: AbilityBase members: accepted forms and every rejection (missing abilityId,
/// nulls, empty/duplicate functionsIn, textSpan shape and order, linkIds rules, abilityWord).
#[test]
fn req_dsl_005_base_members() {
    let full = with(
        MIN_SPELL,
        r#""functionsIn":["HAND","STACK"],"textSpan":[3,3],"linkIds":["l1","l2"],"abilityWord":"Testfall","condition":{"pred":"IS_CREATURE"},"isManaAbility":true"#,
    );
    let a = accept::<AbilityAST>(&full);
    let b = a.base();
    assert_eq!(b.text_span, Some((3, 3)));
    assert_eq!(b.link_ids, Some(vec!["l1".to_owned(), "l2".to_owned()]));
    assert_eq!(b.ability_word.as_deref(), Some("Testfall"));
    assert_eq!(b.is_mana_ability, Some(true));
    let f = accept::<AbilityAST>(&with(MIN_SPELL, r#""isManaAbility":false"#));
    assert_ne!(f, accept::<AbilityAST>(MIN_SPELL));
    let base: AbilityBase = accept(r#"{"abilityId":"x","textSpan":[0,4294967295]}"#);
    assert_eq!(base.text_span, Some((0, u32::MAX)));
    reject::<AbilityBase>(r#"{"abilityId":"x","kind":"SPELL"}"#);
    reject::<AbilityBase>(r#"{}"#);
    for doc in MINIMAL {
        reject::<AbilityAST>(&without(doc, "abilityId"));
        reject::<AbilityAST>(&without(doc, "kind"));
        for extra in [
            r#""functionsIn":[]"#,
            r#""functionsIn":["BATTLEFIELD","BATTLEFIELD"]"#,
            r#""functionsIn":["battlefield"]"#,
            r#""functionsIn":"BATTLEFIELD""#,
            r#""functionsIn":null"#,
            r#""textSpan":[2,1]"#,
            r#""textSpan":[1]"#,
            r#""textSpan":[1,2,3]"#,
            r#""textSpan":[-1,2]"#,
            r#""textSpan":[1,4294967296]"#,
            r#""textSpan":null"#,
            r#""textSpan":{"start":1,"end":2}"#,
            r#""linkIds":[]"#,
            r#""linkIds":["l1","l1"]"#,
            r#""linkIds":[""]"#,
            r#""linkIds":[1]"#,
            r#""linkIds":null"#,
            r#""abilityWord":"""#,
            r#""abilityWord":null"#,
            r#""condition":null"#,
            r#""condition":{}"#,
            r#""isManaAbility":null"#,
            r#""isManaAbility":0"#,
            r#""ability_id":"b""#,
            r#""unknownMember":1"#,
        ] {
            reject::<AbilityAST>(&with(doc, extra));
        }
    }
    reject::<AbilityAST>(&MIN_SPELL.replace(r#""abilityId":"a""#, r#""abilityId":"""#));
    reject::<AbilityAST>(&MIN_SPELL.replace(r#""abilityId":"a""#, r#""abilityId":1"#));
    reject::<AbilityAST>(&MIN_SPELL.replace(r#""abilityId":"a""#, r#""abilityId":null"#));
}

/// REQ-DSL-005: required per-kind members are required, and another kind's members are
/// unknown.
#[test]
fn req_dsl_005_per_kind_required_and_foreign_members() {
    for (doc, required) in [
        (MIN_SPELL, &["effect"][..]),
        (MIN_ACTIVATED, &["cost", "effect"][..]),
        (MIN_TRIGGERED, &["trigger", "effect"][..]),
        (MIN_STATIC, &["effects"][..]),
        (MIN_REPLACEMENT, &["match", "replaceWith"][..]),
        (MIN_KEYWORD, &["keyword"][..]),
    ] {
        for key in required {
            reject::<AbilityAST>(&without(doc, key));
        }
    }
    for (doc, extra) in [
        (MIN_SPELL, r#""cost":[{"cost":"TAP_SOURCE"}]"#),
        (
            MIN_SPELL,
            r#""trigger":{"mode":"STATE","state":{"pred":"IS_CREATURE"}}"#,
        ),
        (MIN_SPELL, r#""activateOnlyIf":{"pred":"IS_CREATURE"}"#),
        (MIN_STATIC, r#""effect":[]"#),
        (MIN_KEYWORD, r#""effect":[]"#),
        (MIN_TRIGGERED, r#""timing":"SORCERY""#),
        (MIN_REPLACEMENT, r#""effects":[]"#),
        (MIN_ACTIVATED, r#""optional":true"#),
        (MIN_STATIC, r#""targets":[]"#),
    ] {
        reject::<AbilityAST>(&with(doc, extra));
    }
    reject::<AbilityAST>(&MIN_SPELL.replace(r#""effect":[]"#, r#""effect":{}"#));
    reject::<AbilityAST>(&MIN_SPELL.replace(r#""effect":[]"#, r#""effect":null"#));
    reject::<AbilityAST>(&MIN_SPELL.replace(r#""effect":[]"#, r#""effect":[{}]"#));
    reject::<AbilityAST>(&MIN_STATIC.replace(r#"[{"type":"RULE","kind":"SKIP"}]"#, "[]"));
}

/// REQ-DSL-006: trigger modes and their rejections.
#[test]
fn req_dsl_006_trigger_modes() {
    let t: Trigger =
        accept(r#"{"mode":"PHASE_STEP","step":"UPKEEP","whose":{"ref":"CONTROLLER"}}"#);
    assert_eq!(
        t,
        Trigger::PhaseStep {
            step: StepId::Upkeep,
            whose: PlayerSelectorAST::Ref(ReferenceAST::Controller)
        }
    );
    accept::<Trigger>(r#"{"mode":"PHASE_STEP","step":"END","whose":{"players":"ALL_PLAYERS"}}"#);
    let s: Trigger = accept(r#"{"mode":"STATE","state":{"pred":"IS_LAND"}}"#);
    assert_eq!(s, Trigger::State(PredicateAST::IsLand));
    let e: Trigger = accept(r#"{"mode":"EVENT","event":{"kind":"CAST","batch":"ONE_OR_MORE"}}"#);
    let Trigger::Event(m) = e else {
        panic!("event trigger expected")
    };
    assert_eq!(m.batch, Some(EventBatch::OneOrMore));
    for doc in [
        r#"{"mode":"EVENT"}"#,
        r#"{"mode":"EVENT","event":{"kind":"CAST"},"step":"UPKEEP"}"#,
        r#"{"mode":"PHASE_STEP","step":"UPKEEP"}"#,
        r#"{"mode":"PHASE_STEP","whose":{"ref":"CONTROLLER"}}"#,
        r#"{"mode":"PHASE_STEP","step":"MAIN","whose":{"ref":"CONTROLLER"}}"#,
        r#"{"mode":"PHASE_STEP","step":"UPKEEP","whose":"CONTROLLER"}"#,
        r#"{"mode":"STATE"}"#,
        r#"{"mode":"STATE","state":{"pred":"IS_LAND"},"event":{"kind":"CAST"}}"#,
        r#"{"mode":"STATIC","state":{"pred":"IS_LAND"}}"#,
        r#"{"mode":"event","event":{"kind":"CAST"}}"#,
        r#"{"event":{"kind":"CAST"}}"#,
        r#"{"mode":null}"#,
        r#"{"mode":"EVENT","event":null}"#,
    ] {
        reject::<Trigger>(doc);
    }
}

/// REQ-DSL-006: EventMatcher members, EventSubject forms, `bind` vars, `batch`; rejections.
#[test]
fn req_dsl_006_event_matcher_members() {
    let m: EventMatcher = accept(r#"{"kind":"DIES"}"#);
    assert_eq!(m, EventMatcher::new(EventKind::Dies));
    let m: EventMatcher = accept(
        r#"{"kind":"LIFE_GAINED","subject":{"players":"TEAMMATES"},"filters":{},"bind":{},"batch":"EACH"}"#,
    );
    assert_eq!(
        m.subject,
        Some(EventSubject::Players(PlayerGroup::Teammates))
    );
    assert_eq!(m.filters, Some(BTreeMap::new()));
    assert_eq!(m.bind, Some(BTreeMap::new()));
    assert_eq!(
        EventSubject::from(PlayerGroup::AllPlayers),
        EventSubject::Players(PlayerGroup::AllPlayers)
    );
    let s: EventSubject = accept(r#"{"ref":"CONTROLLER"}"#);
    assert_eq!(
        s,
        EventSubject::Object(ObjectSelectorAST::Ref(ReferenceAST::Controller))
    );
    accept::<EventSubject>(r#"{"zones":["GRAVEYARD"],"pred":"IS_CREATURE"}"#);
    for doc in [
        r#"{"players":"ALL_PLAYERS","ref":"SELF"}"#,
        r#"{"players":"NOBODY"}"#,
        r#"{"players":null}"#,
        r#"{}"#,
        r#""SELF""#,
        "[]",
    ] {
        reject::<EventSubject>(doc);
    }
    for doc in [
        r#"{}"#,
        r#"{"kind":"dies"}"#,
        r#"{"kind":"DIE"}"#,
        r#"{"kind":"DIES","subject":null}"#,
        r#"{"kind":"DIES","filters":null}"#,
        r#"{"kind":"DIES","filters":[]}"#,
        r#"{"kind":"DIES","filters":{"":true}}"#,
        r#"{"kind":"DIES","filters":{"to":null}}"#,
        r#"{"kind":"DIES","filters":{"to":[]}}"#,
        r#"{"kind":"DIES","filters":{"to":""}}"#,
        r#"{"kind":"DIES","filters":{"to":1.5}}"#,
        r#"{"kind":"DIES","filters":{"to":{"pred":"NOPE"}}}"#,
        r#"{"kind":"DIES","bind":{"x":"x"}}"#,
        r#"{"kind":"DIES","bind":{"x":"$"}}"#,
        r#"{"kind":"DIES","bind":{"x":"$a-b"}}"#,
        r#"{"kind":"DIES","bind":{"x":1}}"#,
        r#"{"kind":"DIES","bind":{"":"$x"}}"#,
        r#"{"kind":"DIES","bind":[]}"#,
        r#"{"kind":"DIES","batch":"ALL"}"#,
        r#"{"kind":"DIES","batch":null}"#,
        r#"{"kind":"DIES","subject":{"players":"ALL_PLAYERS","zones":["HAND"]}}"#,
    ] {
        reject::<EventMatcher>(doc);
    }
}

/// REQ-DSL-006, REQ-DSL-009: FilterValue: the JSON type decides; `Name` excludes `"ANY"` and
/// zone names on construction and on encode.
#[test]
fn req_dsl_006_filter_value_forms() {
    assert_eq!(accept::<FilterValue>("true"), FilterValue::Bool(true));
    assert_eq!(accept::<FilterValue>("false"), FilterValue::Bool(false));
    assert_eq!(accept::<FilterValue>("-3"), FilterValue::Int(-3));
    assert_eq!(accept::<FilterValue>("\"ANY\""), FilterValue::Any);
    for z in Zone::ALL {
        assert_eq!(
            accept::<FilterValue>(&format!("\"{z}\"")),
            FilterValue::Zone(*z)
        );
        assert!(FilterValue::name(z.as_str()).is_err());
        no_encode(&FilterValue::Name(z.as_str().to_owned()));
    }
    assert_eq!(
        accept::<FilterValue>("\"test_word\""),
        FilterValue::Name("test_word".to_owned())
    );
    assert_eq!(
        accept::<FilterValue>("\"any\""),
        FilterValue::Name("any".to_owned())
    );
    assert_eq!(
        accept::<FilterValue>(r#"{"pred":"IS_PLAYER"}"#),
        FilterValue::Predicate(PredicateAST::IsPlayer)
    );
    assert_eq!(
        FilterValue::name("w"),
        Ok(FilterValue::Name("w".to_owned()))
    );
    assert!(FilterValue::name("ANY").is_err());
    assert!(FilterValue::name("").is_err());
    assert!(FilterValue::name("a\u{fdd0}").is_err());
    no_encode(&FilterValue::Name("ANY".to_owned()));
    no_encode(&FilterValue::Name(String::new()));
    no_encode(&FilterValue::Int(9_007_199_254_740_992));
    for doc in [
        "null",
        "[]",
        "\"\"",
        "1.5",
        "1e2",
        "-0",
        "9007199254740992",
        "{}",
        r#"{"ref":"SELF"}"#,
    ] {
        reject::<FilterValue>(doc);
    }
}

/// REQ-DSL-005: target slots: all members required, `min <= max`, slots distinct.
#[test]
fn req_dsl_005_target_slots() {
    let t: TargetSlotAST = accept(r#"{"slot":2,"selector":{"ref":"SELF"},"min":0,"max":3}"#);
    assert_eq!((t.slot, t.min, t.max), (2, 0, 3));
    for doc in [
        r#"{"selector":{"ref":"SELF"},"min":0,"max":3}"#,
        r#"{"slot":0,"min":0,"max":3}"#,
        r#"{"slot":0,"selector":{"ref":"SELF"},"max":3}"#,
        r#"{"slot":0,"selector":{"ref":"SELF"},"min":0}"#,
        r#"{"slot":0,"selector":{"ref":"SELF"},"min":2,"max":1}"#,
        r#"{"slot":-1,"selector":{"ref":"SELF"},"min":0,"max":1}"#,
        r#"{"slot":0,"selector":{"ref":"SELF"},"min":0,"max":1,"optional":true}"#,
        r#"{"slot":0,"selector":{"ref":"SELF"},"min":0,"max":null}"#,
    ] {
        reject::<TargetSlotAST>(doc);
    }
    let slot = |n: u32| format!(r#"{{"slot":{n},"selector":{{"ref":"SELF"}},"min":1,"max":1}}"#);
    accept::<AbilityAST>(&with(
        MIN_SPELL,
        &format!("\"targets\":[{},{}]", slot(0), slot(1)),
    ));
    reject::<AbilityAST>(&with(
        MIN_SPELL,
        &format!("\"targets\":[{},{}]", slot(0), slot(0)),
    ));
    reject::<AbilityAST>(&with(MIN_SPELL, "\"targets\":[]"));
    reject::<AbilityAST>(&with(MIN_SPELL, "\"targets\":null"));
}

/// REQ-DSL-005: mode sets: modes non-empty with distinct ids, `min <= max <= modes`.
#[test]
fn req_dsl_005_mode_sets() {
    let ms: ModeSetAST = accept(
        r#"{"min":1,"max":2,"modes":[{"id":"m1","effect":[]},{"id":"m2","targets":[{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1}],"effect":[{"op":"POPULATE"}]}]}"#,
    );
    assert_eq!(ms.modes.len(), 2);
    let mode: ModeAST = accept(r#"{"id":"m","effect":[]}"#);
    assert_eq!(mode.targets, None);
    for doc in [
        r#"{"min":1,"max":1,"modes":[]}"#,
        r#"{"min":1,"max":2,"modes":[{"id":"m1","effect":[]}]}"#,
        r#"{"min":2,"max":1,"modes":[{"id":"m1","effect":[]},{"id":"m2","effect":[]}]}"#,
        r#"{"min":1,"max":2,"modes":[{"id":"m1","effect":[]},{"id":"m1","effect":[]}]}"#,
        r#"{"min":1,"max":1,"modes":[{"id":"","effect":[]}]}"#,
        r#"{"min":1,"max":1,"modes":[{"id":"m1"}]}"#,
        r#"{"min":1,"max":1,"modes":[{"effect":[]}]}"#,
        r#"{"min":1,"max":1,"modes":[{"id":"m1","effect":[],"targets":[]}]}"#,
        r#"{"min":1,"max":1,"modes":[{"id":"m1","effect":[],"cost":[]}]}"#,
        r#"{"max":1,"modes":[{"id":"m1","effect":[]}]}"#,
        r#"{"min":1,"modes":[{"id":"m1","effect":[]}]}"#,
        r#"{"min":1,"max":1}"#,
    ] {
        reject::<ModeSetAST>(doc);
    }
    accept::<AbilityAST>(&with(
        MIN_TRIGGERED,
        r#""modes":{"min":1,"max":1,"modes":[{"id":"m1","effect":[]}]}"#,
    ));
    reject::<AbilityAST>(&with(MIN_TRIGGERED, r#""modes":null"#));
}

// ---------------------------------------------------------------------------------------------
// Real effect instructions (REQ-DSL-010)

/// A valid DRAW instruction (full form).
const I_DRAW: &str = r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}"#;
/// A valid IF instruction with a nested `then` list.
const I_IF: &str = r#"{"op":"IF","cond":{"pred":"IS_CREATURE"},"then":[{"op":"POPULATE"}]}"#;

/// The five instruction positions: each template holds `@` where the instruction list goes.
const POSITIONS: [&str; 5] = [
    r#"{"abilityId":"a","kind":"SPELL","effect":@}"#,
    r#"{"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"effect":@}"#,
    r#"{"abilityId":"a","kind":"TRIGGERED","trigger":{"mode":"STATE","state":{"pred":"IS_CREATURE"}},"effect":@}"#,
    r#"{"abilityId":"a","kind":"SPELL","modes":{"min":1,"max":1,"modes":[{"id":"m1","effect":@}]},"effect":[]}"#,
    r#"{"abilityId":"a","kind":"REPLACEMENT","match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"INSTEAD","instructions":@}]}"#,
];

/// The instruction list `list` placed in position `i`.
fn at(i: usize, list: &str) -> String {
    POSITIONS[i].replace('@', list)
}

/// REQ-DSL-010, REQ-NFR-006: anything that is not a valid real instruction is rejected in every
/// position (spell, activated, triggered, mode, INSTEAD).
#[test]
fn req_dsl_010_invalid_instructions_rejected_everywhere() {
    let bad = [
        r#"{"op":"NOT_AN_OP"}"#,
        r#"{"op":"draw","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}"#,
        r#"{"op":"DRAW"}"#,
        r#"{"op":"DRAW","player":{"ref":"CONTROLLER"}}"#,
        r#"{"op":"POPULATE","extra":1}"#,
        r#"{"op":"POPULATE","x":null}"#,
        r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1.5}}"#,
        r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":9007199254740992}}"#,
        r#"{}"#,
        r#"{"op":null}"#,
        r#"{"op":1}"#,
        r#"1"#,
        r#""POPULATE""#,
        r#"[]"#,
        r#"null"#,
    ];
    for i in 0..POSITIONS.len() {
        accept::<AbilityAST>(&at(i, r#"[{"op":"POPULATE"}]"#));
        for b in bad {
            reject::<AbilityAST>(&at(i, &format!("[{b}]")));
            reject::<AbilityAST>(&at(i, &format!(r#"[{{"op":"POPULATE"}},{b}]"#)));
        }
    }
    for b in bad {
        reject::<Instruction>(b);
    }
}

/// REQ-DSL-010, REQ-NFR-006: valid instructions in every position round-trip, in both orders,
/// and the canonical bytes are stable (encode, decode, encode).
#[test]
fn req_dsl_010_valid_instructions_round_trip_everywhere() {
    let lists = [
        format!("[{I_DRAW}]"),
        r#"[{"op":"POPULATE"}]"#.to_owned(),
        format!("[{I_IF}]"),
        r#"[{"op":"POPULATE"},{"op":"VENTURE"}]"#.to_owned(),
        r#"[{"op":"VENTURE"},{"op":"POPULATE"}]"#.to_owned(),
    ];
    for i in 0..POSITIONS.len() {
        for l in &lists {
            let a: AbilityAST = accept(&at(i, l));
            let once = crate::canonical::to_canonical_vec(&a).expect("canonical");
            let back: AbilityAST = serde_json::from_slice(&once).expect("reload");
            assert_eq!(back, a);
            assert_eq!(
                crate::canonical::to_canonical_vec(&back).expect("canonical"),
                once
            );
        }
    }
    // Order is kept.
    let a: SpellAbilityAST = accept(&at(0, r#"[{"op":"POPULATE"},{"op":"VENTURE"}]"#));
    assert_eq!(a.effect, vec![Instruction::Populate, Instruction::Venture]);
    let b: SpellAbilityAST = accept(&at(0, r#"[{"op":"VENTURE"},{"op":"POPULATE"}]"#));
    assert_eq!(b.effect, vec![Instruction::Venture, Instruction::Populate]);
}

/// REQ-DSL-010: emptiness is unchanged: spell, activated, triggered and mode effect lists may be
/// empty; `INSTEAD.instructions` may not.
#[test]
fn req_dsl_010_emptiness_rules() {
    for i in 0..4 {
        accept::<AbilityAST>(&at(i, "[]"));
    }
    reject::<AbilityAST>(&at(4, "[]"));
    accept::<AbilityAST>(&at(4, r#"[{"op":"POPULATE"}]"#));
}

/// REQ-DSL-010, REQ-NFR-006: a hand-built instruction that breaks an invariant fails to
/// serialize inside an ability (Serialize re-checks), and a valid one is written.
#[test]
fn req_dsl_010_serialize_rechecks_instructions() {
    let spell = |effect: Vec<Instruction>| {
        AbilityAST::Spell(SpellAbilityAST {
            base: base(),
            targets: None,
            modes: None,
            effect,
        })
    };
    assert!(serde_json::to_vec(&spell(vec![Instruction::Populate])).is_ok());
    let empty_then = Instruction::If {
        cond: PredicateAST::IsCreature,
        then: Vec::new(),
        otherwise: None,
    };
    no_encode(&spell(vec![empty_then.clone()]));
    no_encode(&AbilityAST::Replacement(ReplacementAbilityAST {
        base: base(),
        matcher: ReplacementMatch {
            event: EventKind::Dies,
            affected: EventSubject::Object(ObjectSelectorAST::Ref(ReferenceAST::SelfRef)),
            filters: None,
        },
        replace_with: vec![ReplacementOp::Instead {
            instructions: vec![empty_then],
        }],
        is_self_replacement: None,
        tier_hint: None,
        optional: None,
        overrides_614_5: None,
        shield: None,
    }));
}

/// REQ-DSL-010, REQ-DSL-006 (rule E5 re-check against the real instruction type): a real
/// instruction has no free-form member, so a look-back spelling directly on it is an unknown
/// member.
#[test]
fn req_dsl_010_e5_no_flat_free_form_member_on_instruction() {
    for member in ["lookBack", "look_back", "LOOK-BACK", "lookback"] {
        reject::<Instruction>(&format!(r#"{{"op":"POPULATE","{member}":true}}"#));
        reject::<Instruction>(&format!(
            r#"{{"op":"IF","cond":{{"pred":"IS_CREATURE"}},"then":[{{"op":"POPULATE"}}],"{member}":1}}"#
        ));
        reject::<AbilityAST>(&at(0, &format!(r#"[{{"op":"POPULATE","{member}":true}}]"#)));
    }
}

/// REQ-DSL-010, REQ-DSL-006: a delayed trigger's event matcher still refuses look-back
/// `filters` and `bind` keys (rule E5 holds through the nested `Trigger`).
#[test]
fn req_dsl_010_e5_through_delayed_trigger() {
    let doc = |matcher_extra: &str| {
        format!(
            r#"{{"op":"CREATE_DELAYED_TRIGGER","trigger":{{"mode":"EVENT","event":{{"kind":"ATTACKS","subject":{{"ref":"SELF"}}{matcher_extra}}}}},"effect":[{{"op":"POPULATE"}}],"oneShot":true}}"#
        )
    };
    accept::<Instruction>(&doc(""));
    accept::<Instruction>(&doc(
        r#","filters":{"combat":true},"bind":{"recipient":"$P"}"#,
    ));
    for key in ["lookBack", "look_back", "LOOK-BACK", "lookbackFlag"] {
        reject::<Instruction>(&doc(&format!(r#","filters":{{"{key}":true}}"#)));
        reject::<Instruction>(&doc(&format!(r#","bind":{{"{key}":"$P"}}"#)));
        reject::<AbilityAST>(&at(
            0,
            &format!("[{}]", doc(&format!(r#","filters":{{"{key}":true}}"#))),
        ));
    }
}

/// REQ-DSL-010, REQ-DSL-015: `NATIVE_EFFECT` `params` are opaque by contract: a look-back-like
/// key is data and round-trips unchanged.
#[test]
fn req_dsl_010_e5_native_effect_params_are_opaque() {
    let doc = r#"{"op":"NATIVE_EFFECT","id":"native.t","version":1,"params":{"lookBack":true,"look_back":[1]}}"#;
    let i: Instruction = accept(doc);
    let Instruction::NativeEffect(n) = &i else {
        panic!("native effect expected")
    };
    assert_eq!(n.params["lookBack"], json!(true));
    assert_eq!(n.params["look_back"], json!([1]));
    accept::<AbilityAST>(&at(0, &format!("[{doc}]")));
}

// ---------------------------------------------------------------------------------------------
// Static effects (I)

/// REQ-DSL-008, REQ-DSL-004: CHAR and PLAYER effects: required members, layer, `isCDA`,
/// `affected` forms; each effect struct decodes only its own `type`.
#[test]
fn req_dsl_008_char_and_player_effects() {
    let c: CharacteristicEffect = accept(
        r#"{"type":"CHAR","layer":"7a","affected":{"ref":"SELF"},"modification":{"op":"SET_BASE_PT","power":{"COUNT":{"zones":["HAND"]}},"toughness":0},"isCDA":true}"#,
    );
    assert_eq!(c.is_cda, Some(true));
    assert!(matches!(c.modification.power, Some(PtValue::Expr(_))));
    let p: PlayerEffect = accept(
        r#"{"type":"PLAYER","affected":{"ref":"CONTROLLER"},"modification":{"op":"CANT_LOSE_THE_GAME"}}"#,
    );
    assert_eq!(p.modification.op, "CANT_LOSE_THE_GAME");
    reject::<PlayerEffect>(r#"{"type":"RULE","kind":"SKIP"}"#);
    reject::<CharacteristicEffect>(
        r#"{"type":"PLAYER","affected":{"ref":"CONTROLLER"},"modification":{"op":"X"}}"#,
    );
    let r: RuleModification = accept(r#"{"type":"RULE","kind":"SKIP","step":"UNTAP"}"#);
    assert_eq!(r.kind, RuleKind::Skip);
    reject::<RuleModification>(
        r#"{"type":"PLAYER","affected":{"ref":"CONTROLLER"},"modification":{"op":"X"}}"#,
    );
    for doc in [
        r#"{"layer":"6","affected":{"ref":"SELF"},"modification":{"op":"X"}}"#,
        r#"{"type":"char","layer":"6","affected":{"ref":"SELF"},"modification":{"op":"X"}}"#,
        r#"{"type":"LAYER","layer":"6","affected":{"ref":"SELF"},"modification":{"op":"X"}}"#,
        r#"{"type":"CHAR","affected":{"ref":"SELF"},"modification":{"op":"X"}}"#,
        r#"{"type":"CHAR","layer":"6","modification":{"op":"X"}}"#,
        r#"{"type":"CHAR","layer":"6","affected":{"ref":"SELF"}}"#,
        r#"{"type":"CHAR","layer":"8","affected":{"ref":"SELF"},"modification":{"op":"X"}}"#,
        r#"{"type":"CHAR","layer":6,"affected":{"ref":"SELF"},"modification":{"op":"X"}}"#,
        r#"{"type":"CHAR","layer":"6","affected":{"ref":"SELF"},"modification":{"op":"X"},"isCDA":null}"#,
        r#"{"type":"CHAR","layer":"6","affected":{"ref":"SELF"},"modification":{"op":"X"},"is_cda":true}"#,
        r#"{"type":"CHAR","layer":"6","affected":{"sameAsLayer":"6","ref":"SELF"},"modification":{"op":"X"}}"#,
        r#"{"type":"CHAR","layer":"6","affected":{"sameAsLayer":"9"},"modification":{"op":"X"}}"#,
        r#"{"type":"CHAR","layer":"6","affected":{"sameAsLayer":null},"modification":{"op":"X"}}"#,
        r#"{"type":"CHAR","layer":"6","affected":{"players":"ALL_PLAYERS"},"modification":{"op":"X"}}"#,
        r#"{"type":"CHAR","layer":"6","affected":{"ref":"SELF"},"modification":{"op":"X"},"kind":"SKIP"}"#,
        r#"{"type":"PLAYER","affected":{"zones":["HAND"]},"modification":{"op":"X"}}"#,
        r#"{"type":"PLAYER","modification":{"op":"X"}}"#,
        r#"{"type":"PLAYER","affected":{"ref":"CONTROLLER"}}"#,
        r#"{"type":"PLAYER","affected":{"ref":"CONTROLLER"},"modification":{"op":"X"},"layer":"6"}"#,
        r#"{"type":null}"#,
        r#"{}"#,
        r#"[]"#,
    ] {
        reject::<StaticEffect>(doc);
    }
    let a: Affected = accept(r#"{"sameAsLayer":"1a"}"#);
    assert_eq!(a, Affected::SameAsLayer(Layer::L1a));
    accept::<Affected>(r#"{"zones":["BATTLEFIELD"]}"#);
    reject::<Affected>("[]");
}

/// REQ-DSL-008: Modification: `op` shape; typed power/toughness for SET_BASE_PT and MODIFY_PT;
/// other ops opaque.
#[test]
fn req_dsl_008_modification_params() {
    let m: Modification = accept(r#"{"op":"MODIFY_PT","power":-1,"toughness":{"LITERAL":2}}"#);
    assert_eq!(m.power, Some(PtValue::Int(-1)));
    assert_eq!(
        m.toughness,
        Some(PtValue::Expr(ValueExpressionAST::Literal(2)))
    );
    let other: Modification = accept(r#"{"op":"ADD_ABILITY","power":"anything","extra":[1]}"#);
    assert_eq!(other.power, None);
    assert_eq!(other.params["power"], json!("anything"));
    accept::<Modification>(r#"{"op":"SET_BASE_PT","power":1,"toughness":1,"dependsOn":"layer"}"#);
    for doc in [
        r#"{"op":"SET_BASE_PT","toughness":1}"#,
        r#"{"op":"SET_BASE_PT","power":1}"#,
        r#"{"op":"MODIFY_PT"}"#,
        r#"{"op":"MODIFY_PT","power":"X","toughness":1}"#,
        r#"{"op":"MODIFY_PT","power":"1","toughness":1}"#,
        r#"{"op":"MODIFY_PT","power":1.0,"toughness":1}"#,
        r#"{"op":"MODIFY_PT","power":null,"toughness":1}"#,
        r#"{"op":"MODIFY_PT","power":[1],"toughness":1}"#,
        r#"{"op":"MODIFY_PT","power":{"NOPE":1},"toughness":1}"#,
        r#"{"op":"MODIFY_PT","power":9007199254740992,"toughness":1}"#,
        r#"{"op":"set_base_pt","power":1,"toughness":1}"#,
        r#"{"op":""}"#,
        r#"{}"#,
        r#"{"op":"X","":1}"#,
        r#"{"op":"X","y":null}"#,
    ] {
        reject::<Modification>(doc);
    }
    for doc in [r#""X""#, r#""1""#, "[]", "null", "true"] {
        reject::<PtValue>(doc);
    }
    assert_eq!(accept::<PtValue>("7"), PtValue::Int(7));
}

/// REQ-DSL-008: RULE params: flat opaque members; `kind`/`type` cannot be params; `cost` must
/// be a non-empty CostAST array.
#[test]
fn req_dsl_008_rule_params_and_cost() {
    let r: RuleModification = accept(
        r#"{"type":"RULE","kind":"BLOCK_COST","cost":[{"cost":"PAY_LIFE","amount":{"LITERAL":1}}],"blockers":{"ref":"SELF"}}"#,
    );
    assert_eq!(r.cost.as_ref().expect("cost").len(), 1);
    assert_eq!(
        RuleModification::new(RuleKind::Evasion).params,
        BTreeMap::new()
    );
    for doc in [
        r#"{"type":"RULE"}"#,
        r#"{"type":"RULE","kind":"skip"}"#,
        r#"{"type":"RULE","kind":"SKIP","":1}"#,
        r#"{"type":"RULE","kind":"SKIP","x":null}"#,
        r#"{"type":"RULE","kind":"SKIP","x":[null]}"#,
        r#"{"type":"RULE","kind":"SKIP","cost":null}"#,
        r#"{"type":"RULE","kind":"SKIP","cost":[]}"#,
        r#"{"type":"RULE","kind":"SKIP","cost":[1]}"#,
        r#"{"type":"RULE","kind":"SKIP","cost":{"cost":"TAP_SOURCE"}}"#,
        r#"{"type":"RULE","kind":"SKIP","cost":"{2}"}"#,
        r#"{"type":"RULE","kind":"SKIP","cost":[{"cost":"NOPE"}]}"#,
    ] {
        reject::<StaticEffect>(doc);
    }
    let mut r = RuleModification::new(RuleKind::Skip);
    for key in ["kind", "type", "cost"] {
        r.params = BTreeMap::from([(key.to_owned(), json!(1))]);
        no_encode(&r);
        no_encode(&StaticEffect::Rule(r.clone()));
    }
    r.params = BTreeMap::new();
    r.cost = Some(Vec::new());
    no_encode(&r);
}

/// REQ-DSL-008, REQ-DSL-004: the 613.6 lock-in cross-check: dangling, self-referential and
/// chained `sameAsLayer` are rejected on decode and on encode; anchored lock-in is accepted.
#[test]
fn req_dsl_008_req_dsl_004_lock_in_cross_check() {
    let ch = |layer: &str, affected: &str| {
        format!(
            r#"{{"type":"CHAR","layer":"{layer}","affected":{affected},"modification":{{"op":"TEST_MOD"}}}}"#
        )
    };
    let sel = r#"{"ref":"SELF"}"#;
    let same = |l: &str| format!(r#"{{"sameAsLayer":"{l}"}}"#);
    let stat = |effects: &[String]| {
        format!(
            r#"{{"abilityId":"s","kind":"STATIC","effects":[{}]}}"#,
            effects.join(",")
        )
    };
    accept::<AbilityAST>(&stat(&[ch("6", sel), ch("7b", &same("6"))]));
    accept::<AbilityAST>(&stat(&[ch("7b", &same("6")), ch("6", sel)]));
    accept::<AbilityAST>(&stat(&[
        ch("6", sel),
        ch("7b", &same("6")),
        ch("4", &same("6")),
    ]));
    // dangling
    reject::<AbilityAST>(&stat(&[ch("7b", &same("6"))]));
    reject::<AbilityAST>(&stat(&[ch("4", sel), ch("7b", &same("6"))]));
    reject::<AbilityAST>(&stat(&[
        r#"{"type":"RULE","kind":"SKIP"}"#.to_owned(),
        ch("7b", &same("6")),
    ]));
    // self
    reject::<AbilityAST>(&stat(&[ch("6", &same("6"))]));
    // chained
    reject::<AbilityAST>(&stat(&[
        ch("6", sel),
        ch("7b", &same("6")),
        ch("7c", &same("7b")),
    ]));
    reject::<AbilityAST>(&stat(&[ch("6", &same("7b")), ch("7b", &same("6"))]));
    reject::<StaticAbilityAST>(&stat(&[ch("7b", &same("6"))]));
    // encode side
    let dependant = accept::<StaticEffect>(&ch("7b", &same("6")));
    let anchor = accept::<StaticEffect>(&ch("6", sel));
    let mut s = StaticAbilityAST {
        base: base(),
        effects: vec![anchor, dependant],
    };
    assert!(serde_json::to_vec(&s).is_ok());
    s.effects.remove(0);
    no_encode(&s);
    no_encode(&AbilityAST::Static(s.clone()));
    s.effects = Vec::new();
    no_encode(&s);
}

// ---------------------------------------------------------------------------------------------
// Replacement (J)

/// REQ-DSL-009: replacement members, ops, tier hints and shields; every rejection.
#[test]
fn req_dsl_009_replacement_members() {
    let full = with(
        MIN_REPLACEMENT,
        r#""isSelfReplacement":true,"tierHint":"COPY_ETB","optional":false,"overrides614_5":true,"shield":{"uses":"COUNTER_BACKED","counterKind":"shield"}"#,
    );
    let (_, r) = accept_both::<ReplacementAbilityAST>(&full);
    assert_eq!(r.is_self_replacement, Some(true));
    assert_eq!(r.tier_hint, Some(TierHint::CopyEtb));
    assert_eq!(r.optional, Some(false));
    assert_eq!(r.overrides_614_5, Some(true));
    assert_eq!(
        r.shield,
        Some(Shield {
            uses: ShieldUses::CounterBacked,
            counter_kind: Some("shield".to_owned())
        })
    );
    let s: Shield = accept(r#"{"uses":3}"#);
    assert_eq!(s.uses, ShieldUses::Count(3));
    accept::<Shield>(r#"{"uses":"COUNTER_BACKED"}"#);
    accept::<ShieldUses>("4294967295");
    for doc in [
        r#"{}"#,
        r#"{"uses":0}"#,
        r#"{"uses":-1}"#,
        r#"{"uses":4294967296}"#,
        r#"{"uses":"MANY"}"#,
        r#"{"uses":"counter_backed"}"#,
        r#"{"uses":2,"counterKind":"shield"}"#,
        r#"{"uses":"COUNTER_BACKED","counterKind":""}"#,
        r#"{"uses":"COUNTER_BACKED","counterKind":null}"#,
        r#"{"uses":1,"extra":1}"#,
        r#"{"uses":null}"#,
        r#"{"uses":true}"#,
    ] {
        reject::<Shield>(doc);
    }
    for op in [
        r#"{"op":"SKIP"}"#,
        r#"{"op":"PREVENT"}"#,
        r#"{"op":"PREVENT","amount":{"LITERAL":2}}"#,
        r#"{"op":"INSTEAD","instructions":[{"op":"POPULATE"}]}"#,
        r#"{"op":"MODIFY_EVENT","set":{"amount":2,"tapped":true,"to":"HAND","who":{"pred":"IS_PLAYER"},"label":"w"}}"#,
        r#"{"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"COLOR"},"store":"c"}"#,
    ] {
        accept::<ReplacementOp>(op);
    }
    for doc in [
        r#"{"op":"SKIP","amount":1}"#,
        r#"{"op":"PREVENT","amount":null}"#,
        r#"{"op":"PREVENT","amount":2}"#,
        r#"{"op":"INSTEAD","instructions":[]}"#,
        r#"{"op":"INSTEAD"}"#,
        r#"{"op":"MODIFY_EVENT","set":{}}"#,
        r#"{"op":"MODIFY_EVENT","set":{"":1}}"#,
        r#"{"op":"MODIFY_EVENT","set":{"to":null}}"#,
        r#"{"op":"MODIFY_EVENT"}"#,
        r#"{"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"COLOR"}}"#,
        r#"{"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"COLOR"},"store":""}"#,
        r#"{"op":"CHOOSE","choice":{"kind":"COLOR"},"store":"c"}"#,
        r#"{"op":"REPLACE"}"#,
        r#"{"op":"skip"}"#,
        r#"{}"#,
    ] {
        reject::<ReplacementOp>(doc);
    }
    for extra in [
        r#""tierHint":"CONTROL""#,
        r#""tierHint":null"#,
        r#""isSelfReplacement":null"#,
        r#""overrides614_5":1"#,
        r#""overrides6145":true"#,
        r#""shield":{}"#,
    ] {
        reject::<AbilityAST>(&with(MIN_REPLACEMENT, extra));
    }
    reject::<AbilityAST>(&MIN_REPLACEMENT.replace(r#"[{"op":"SKIP"}]"#, "[]"));
    for doc in [
        r#"{"event":"DIES"}"#,
        r#"{"affected":{"ref":"SELF"}}"#,
        r#"{"event":"DIES","affected":{"ref":"SELF"},"filters":null}"#,
        r#"{"event":"DIES","affected":{"ref":"SELF"},"subject":{"ref":"SELF"}}"#,
        r#"{"event":"dies","affected":{"ref":"SELF"}}"#,
    ] {
        reject::<ReplacementMatch>(doc);
    }
    let m: ReplacementMatch =
        accept(r#"{"event":"LIFE_GAINED","affected":{"players":"ALL_PLAYERS"},"filters":{}}"#);
    assert_eq!(m.affected, EventSubject::Players(PlayerGroup::AllPlayers));
    for (op, name) in [
        (ReplacementOp::Skip, "SKIP"),
        (ReplacementOp::Prevent { amount: None }, "PREVENT"),
        (
            ReplacementOp::Instead {
                instructions: vec![Instruction::Populate],
            },
            "INSTEAD",
        ),
        (
            ReplacementOp::ModifyEvent {
                set: BTreeMap::from([("to".to_owned(), FilterValue::Any)]),
            },
            "MODIFY_EVENT",
        ),
    ] {
        assert_eq!(op.name(), name);
    }
}

/// REQ-DSL-005: keyword instances: name shape, opaque params.
#[test]
fn req_dsl_005_keyword_instances() {
    let k: KeywordInstanceAST = accept(&with(
        MIN_KEYWORD,
        r#""params":{"n":2,"cost":"{1}","x":{"y":[true]}}"#,
    ));
    assert_eq!(k.params.as_ref().expect("params").len(), 3);
    accept::<KeywordInstanceAST>(&with(MIN_KEYWORD, r#""params":{}"#));
    let built = KeywordInstanceAST::new(AbilityBase::new("a"), "TEST_KEYWORD");
    assert_eq!(built, accept::<KeywordInstanceAST>(MIN_KEYWORD));
    for kw in ["", "test", "Test", "1A", "A-B", "A B"] {
        reject::<AbilityAST>(&MIN_KEYWORD.replace("TEST_KEYWORD", kw));
    }
    reject::<AbilityAST>(&MIN_KEYWORD.replace("\"TEST_KEYWORD\"", "1"));
    for extra in [
        r#""params":null"#,
        r#""params":[]"#,
        r#""params":{"":1}"#,
        r#""params":{"x":null}"#,
        r#""params":{"x":[1,null]}"#,
    ] {
        reject::<AbilityAST>(&with(MIN_KEYWORD, extra));
    }
}

// ---------------------------------------------------------------------------------------------
// Strictness (W2)

/// REQ-NFR-006, REQ-CORE-006: AbilityAST decoding rejects duplicates, non-integers, unsafe
/// integers, noncharacters and over-deep input; a document at the depth budget is accepted.
#[test]
fn req_nfr_006_strict_decoding() {
    reject::<AbilityAST>(&with(MIN_SPELL, r#""abilityId":"b""#));
    reject::<AbilityAST>(&with(MIN_SPELL, r#""effect":[]"#));
    for n in [
        "1.0",
        "1e2",
        "-0",
        "9007199254740992",
        "-9007199254740992",
        "1.5",
    ] {
        reject::<AbilityAST>(&with(MIN_SPELL, &format!(r#""textSpan":[{n},{n}]"#)));
        reject::<AbilityAST>(&MIN_SPELL.replace(
            "[]",
            &format!(
                r#"[{{"op":"NATIVE_EFFECT","id":"native.t","version":1,"params":{{"n":{n}}}}}]"#
            ),
        ));
        reject::<AbilityAST>(&with(MIN_KEYWORD, &format!(r#""params":{{"n":{n}}}"#)));
        reject::<AbilityAST>(&EX4.replace(r#""EACH_ATTACKER""#, n));
    }
    accept::<AbilityAST>(&with(MIN_SPELL, r#""textSpan":[0,4294967295]"#));
    accept::<AbilityAST>(&MIN_SPELL.replace(
        "[]",
        r#"[{"op":"NATIVE_EFFECT","id":"native.t","version":1,"params":{"n":-9007199254740991}}]"#,
    ));
    reject::<AbilityAST>(&MIN_SPELL.replace(r#""a""#, r#""a﷐""#));
    reject::<AbilityAST>(&MIN_SPELL.replace(
        "[]",
        r#"[{"op":"NATIVE_EFFECT","id":"native.t","version":1,"params":{"￿":1}}]"#,
    ));
    reject::<AbilityAST>(&MIN_SPELL.replace(
        "[]",
        r#"[{"op":"NATIVE_EFFECT","id":"native.t","version":1,"params":{"x":"﷯"}}]"#,
    ));
    // Nesting through NOT: the ability is depth 1, `condition` depth 2, each NOT one more.
    let nots = |k: usize| {
        let mut p = r#"{"pred":"IS_CREATURE"}"#.to_owned();
        for _ in 0..k {
            p = format!(r#"{{"op":"NOT","condition":{p}}}"#);
        }
        with(MIN_SPELL, &format!(r#""condition":{p}"#))
    };
    accept::<AbilityAST>(&nots(125));
    reject::<AbilityAST>(&nots(126));
    reject::<AbilityAST>(&nots(1000));
    // COMPOSITE chain: the cost array is depth 2; each COMPOSITE adds an object and an array.
    let chain = |n: usize| {
        let mut c = r#"{"cost":"TAP_SOURCE"}"#.to_owned();
        for _ in 0..n {
            c = format!(r#"{{"cost":"COMPOSITE","costs":[{c}]}}"#);
        }
        format!(r#"{{"abilityId":"a","kind":"ACTIVATED","cost":[{c}],"effect":[]}}"#)
    };
    accept::<AbilityAST>(&chain(62));
    reject::<AbilityAST>(&chain(63));
    reject::<AbilityAST>(&chain(5000));
    reject::<CostAST>(&r#"{"cost":"COMPOSITE","costs":["#.repeat(100_000));
    // Opaque values count too: instruction object depth 3, params object 4, then one per array.
    let deep_opaque = |k: usize| {
        MIN_SPELL.replace(
            "[]",
            &format!(
                r#"[{{"op":"NATIVE_EFFECT","id":"native.t","version":1,"params":{{"x":{}1{}}}}}]"#,
                "[".repeat(k),
                "]".repeat(k)
            ),
        )
    };
    accept::<AbilityAST>(&deep_opaque(123));
    reject::<AbilityAST>(&deep_opaque(124));
}

// ---------------------------------------------------------------------------------------------
// Serialize-side checks (W3)

/// REQ-NFR-006: hand-built invalid envelopes, targets and modes fail to serialize.
#[test]
fn req_nfr_006_serialize_rechecks_envelope() {
    let spell = |b: AbilityBase| {
        AbilityAST::Spell(SpellAbilityAST {
            base: b,
            targets: None,
            modes: None,
            effect: Vec::new(),
        })
    };
    assert!(serde_json::to_vec(&spell(base())).is_ok());
    let mut b = base();
    b.ability_id = String::new();
    no_encode(&spell(b));
    let mut b = base();
    b.functions_in = Some(vec![Zone::Hand, Zone::Hand]);
    no_encode(&spell(b.clone()));
    no_encode(&b);
    b.functions_in = Some(Vec::new());
    no_encode(&b);
    let mut b = base();
    b.text_span = Some((5, 4));
    no_encode(&b);
    let mut b = base();
    b.link_ids = Some(Vec::new());
    no_encode(&b);
    b.link_ids = Some(vec!["x".into(), "x".into()]);
    no_encode(&b);
    let mut b = base();
    b.ability_word = Some(String::new());
    no_encode(&b);

    let slot = TargetSlotAST {
        slot: 0,
        selector: ObjectSelectorAST::Ref(ReferenceAST::SelfRef),
        min: 1,
        max: 1,
    };
    let mut bad_slot = slot.clone();
    bad_slot.min = 2;
    no_encode(&bad_slot);
    let mut s = SpellAbilityAST {
        base: base(),
        targets: Some(vec![slot.clone(), slot.clone()]),
        modes: None,
        effect: Vec::new(),
    };
    no_encode(&s);
    s.targets = Some(Vec::new());
    no_encode(&s);
    s.targets = None;
    let mode = ModeAST {
        id: "m".into(),
        targets: None,
        effect: Vec::new(),
    };
    s.modes = Some(ModeSetAST {
        min: 1,
        max: 2,
        modes: vec![mode.clone()],
    });
    no_encode(&s);
    s.modes = Some(ModeSetAST {
        min: 1,
        max: 1,
        modes: vec![mode.clone(), mode.clone()],
    });
    no_encode(&s);
    s.modes = Some(ModeSetAST {
        min: 0,
        max: 0,
        modes: Vec::new(),
    });
    no_encode(&s);
    s.modes = Some(ModeSetAST {
        min: 1,
        max: 0,
        modes: vec![mode.clone()],
    });
    no_encode(&s);
    let mut bad_mode = mode;
    bad_mode.id = String::new();
    no_encode(&bad_mode);
    bad_mode.id = "m".into();
    bad_mode.targets = Some(Vec::new());
    no_encode(&bad_mode);

    let mut act = ActivatedAbilityAST {
        base: base(),
        cost: Vec::new(),
        timing: None,
        limits: None,
        activate_only_if: None,
        is_loyalty: None,
        targets: None,
        modes: None,
        effect: Vec::new(),
    };
    no_encode(&act);
    act.cost = vec![CostAST::TapSource];
    assert!(serde_json::to_vec(&act).is_ok());
    act.limits = Some(Vec::new());
    no_encode(&act);
    act.limits = Some(vec![ActivationLimit {
        per_turn: None,
        per_game: None,
    }]);
    no_encode(&act);
    act.limits = Some(vec![ActivationLimit {
        per_turn: Some(0),
        per_game: None,
    }]);
    no_encode(&act);
    act.limits = Some(vec![ActivationLimit {
        per_turn: Some(1),
        per_game: Some(0),
    }]);
    no_encode(&act);

    let mut k = KeywordInstanceAST::new(base(), "lower");
    no_encode(&k);
    k.keyword = "OK_KEYWORD".into();
    k.params = Some(BTreeMap::from([(String::new(), json!(1))]));
    no_encode(&k);
    k.params = Some(BTreeMap::from([("x".to_owned(), Value::Null)]));
    no_encode(&AbilityAST::Keyword(k));

    let mut em = EventMatcher::new(EventKind::Cast);
    em.bind = Some(BTreeMap::from([("x".to_owned(), "nodollar".to_owned())]));
    no_encode(&em);
    em.bind = None;
    em.filters = Some(BTreeMap::from([(String::new(), FilterValue::Any)]));
    no_encode(&em);
    no_encode(&Trigger::Event(em));
}

/// REQ-NFR-006: hand-built invalid costs, instructions, modifications and replacements fail to
/// serialize.
#[test]
fn req_nfr_006_serialize_rechecks_parts() {
    let sel = ObjectSelectorAST::Ref(ReferenceAST::SelfRef);
    for c in [
        CostAST::ExileFromZone {
            zone: Zone::Graveyard,
            selector: sel.clone(),
            count: None,
            min_total_mv: None,
        },
        CostAST::ExileFromZone {
            zone: Zone::Graveyard,
            selector: sel.clone(),
            count: Some(ValueExpressionAST::X),
            min_total_mv: Some(ValueExpressionAST::X),
        },
        CostAST::RemoveCounters {
            from: sel.clone(),
            kind: String::new(),
            amount: ValueExpressionAST::X,
        },
        CostAST::PutCounters {
            on: sel.clone(),
            kind: String::new(),
            amount: ValueExpressionAST::X,
        },
        CostAST::Composite { costs: Vec::new() },
        CostAST::Composite {
            costs: vec![CostAST::Composite { costs: Vec::new() }],
        },
    ] {
        no_encode(&c);
    }

    let mut m = Modification::new("SET_BASE_PT");
    no_encode(&m);
    m.power = Some(PtValue::Int(1));
    no_encode(&m);
    m.toughness = Some(PtValue::Expr(ValueExpressionAST::X));
    no_encode(&m);
    no_encode(&PtValue::Expr(ValueExpressionAST::X));
    no_encode(&PtValue::Int(i64::MAX));
    m.toughness = Some(PtValue::Int(1));
    assert!(serde_json::to_vec(&m).is_ok());
    m.params = BTreeMap::from([("power".to_owned(), json!(1))]);
    no_encode(&m);
    let mut other = Modification::new("ADD_ABILITY");
    other.power = Some(PtValue::Int(1));
    no_encode(&other);
    other.power = None;
    other.params = BTreeMap::from([("op".to_owned(), json!(1))]);
    no_encode(&other);
    no_encode(&Modification::new("bad op"));
    no_encode(&StaticEffect::Char(CharacteristicEffect {
        layer: Layer::L6,
        affected: Affected::SameAsLayer(Layer::L6),
        modification: Modification::new("x"),
        is_cda: None,
    }));
    no_encode(&StaticEffect::Player(PlayerEffect {
        affected: PlayerSelectorAST::Ref(ReferenceAST::Controller),
        modification: Modification::new(""),
    }));

    let mut rep = ReplacementAbilityAST {
        base: base(),
        is_self_replacement: None,
        tier_hint: None,
        matcher: ReplacementMatch {
            event: EventKind::Dies,
            filters: None,
            affected: EventSubject::Object(sel),
        },
        replace_with: Vec::new(),
        optional: None,
        overrides_614_5: None,
        shield: None,
    };
    no_encode(&rep);
    rep.replace_with = vec![ReplacementOp::Skip];
    assert!(serde_json::to_vec(&rep).is_ok());
    rep.shield = Some(Shield {
        uses: ShieldUses::Count(2),
        counter_kind: Some("shield".into()),
    });
    no_encode(&rep);
    rep.shield = Some(Shield {
        uses: ShieldUses::Count(0),
        counter_kind: None,
    });
    no_encode(&rep);
    rep.shield = Some(Shield {
        uses: ShieldUses::CounterBacked,
        counter_kind: Some(String::new()),
    });
    no_encode(&rep);
    for op in [
        ReplacementOp::ModifyEvent {
            set: BTreeMap::new(),
        },
        ReplacementOp::ModifyEvent {
            set: BTreeMap::from([(String::new(), FilterValue::Any)]),
        },
        ReplacementOp::ModifyEvent {
            set: BTreeMap::from([("to".to_owned(), FilterValue::Name("EXILE".into()))]),
        },
        ReplacementOp::Instead {
            instructions: Vec::new(),
        },
        ReplacementOp::Choose {
            choice: Box::new(ChoiceSpec::new(
                PlayerSelectorAST::Ref(ReferenceAST::Controller),
                ChoiceKind::Color,
            )),
            store: String::new(),
        },
    ] {
        no_encode(&op);
    }
}

// ---------------------------------------------------------------------------------------------
// PR #162 review round 1 regressions

/// REQ-DSL-008, REQ-DSL-004, CR 613.6: lock-in anchors may come later in the list, and a large
/// list is handled in one pass (decode and encode); a missing anchor is still rejected.
#[test]
fn req_dsl_008_lock_in_forward_reference_and_large_list() {
    let lock = r#"{"type":"CHAR","layer":"7b","affected":{"sameAsLayer":"6"},"modification":{"op":"SET_BASE_PT","power":1,"toughness":1}}"#;
    let anchor = r#"{"type":"CHAR","layer":"6","affected":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"modification":{"op":"REMOVE_ALL_ABILITIES"}}"#;
    let doc = |n: usize, with_anchor: bool| {
        let mut effects = vec![lock; n];
        if with_anchor {
            effects.push(anchor);
        }
        format!(
            r#"{{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],"effects":[{}]}}"#,
            effects.join(",")
        )
    };
    accept::<AbilityAST>(&doc(1, true));
    accept::<AbilityAST>(&doc(3000, true));
    reject::<AbilityAST>(&doc(1, false));
    reject::<AbilityAST>(&doc(3000, false));
}

/// REQ-DSL-009, REQ-NFR-006: ShieldUses enforces a positive count on both codec paths.
#[test]
fn req_dsl_009_shield_uses_codecs_enforce_positive_count() {
    assert!(serde_json::from_str::<ShieldUses>("0").is_err());
    assert!(serde_json::to_string(&ShieldUses::Count(0)).is_err());
    assert_eq!(
        serde_json::from_str::<ShieldUses>("1").expect("1"),
        ShieldUses::Count(1)
    );
    assert_eq!(
        serde_json::to_string(&ShieldUses::Count(1)).expect("ser"),
        "1"
    );
    assert_eq!(
        serde_json::from_str::<ShieldUses>(r#""COUNTER_BACKED""#).expect("cb"),
        ShieldUses::CounterBacked
    );
    assert_eq!(
        serde_json::to_string(&ShieldUses::CounterBacked).expect("ser"),
        r#""COUNTER_BACKED""#
    );
}
