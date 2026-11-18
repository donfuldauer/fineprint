// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Seeded property tests for the ability schema (REQ-DSL-004..REQ-DSL-009, REQ-NFR-006,
//! REQ-CORE-006, R7, R12): generated valid abilities of every kind round-trip canonically and
//! independently of member order; required-member drops, unknown members and nulled optionals
//! are rejected; random and mutated bytes never panic a decoder. Fixed seeds only.

use std::collections::BTreeMap;

use fineprint_base::prng::Pcg64Dxsm;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::super::*;
use crate::choice::{ChoiceKind, ChoiceSpec};
use crate::expr::tests::{
    assert_wire_props, decode_never_panics, gen_player, gen_pred, gen_selector, gen_str, gen_value,
    mutate, pick, random_doc,
};
use crate::expr::{PlayerGroup, ValueExpressionAST, Zone};
use crate::mana::ManaCostDSL;

// ---------------------------------------------------------------------------------------------
// Generators (synthetic vocabulary only, R5)

/// A flag that is absent, true or false.
fn flag(rng: &mut Pcg64Dxsm) -> Option<bool> {
    [None, Some(true), Some(false)][pick(rng, 3)]
}

/// A parameter name that no schema reserves and E5 accepts.
fn gen_key(rng: &mut Pcg64Dxsm) -> String {
    const KEYS: [&str; 8] = [
        "player",
        "count",
        "amount",
        "mode",
        "target",
        "\u{e9}x",
        "with space",
        "lookBack",
    ];
    KEYS[pick(rng, KEYS.len())].to_owned()
}

/// An opaque JSON value (no null) nested at most `depth` levels.
fn gen_opaque(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    match pick(rng, if depth == 0 { 5 } else { 7 }) {
        0 => json!(true),
        1 => json!(-7),
        2 => json!(9_007_199_254_740_991_i64),
        3 => Value::String(gen_str(rng)),
        4 => json!(""),
        5 => Value::Array(
            (0..pick(rng, 3))
                .map(|_| gen_opaque(rng, depth - 1))
                .collect(),
        ),
        _ => {
            let mut m = Map::new();
            for _ in 0..pick(rng, 3) {
                let key = if pick(rng, 5) == 0 {
                    String::new()
                } else {
                    gen_key(rng)
                };
                m.insert(key, gen_opaque(rng, depth - 1));
            }
            Value::Object(m)
        }
    }
}

/// Flat parameters (none reserved).
fn gen_params(rng: &mut Pcg64Dxsm) -> BTreeMap<String, Value> {
    (0..pick(rng, 4))
        .map(|_| (gen_key(rng), gen_opaque(rng, 2)))
        .collect()
}

/// An operation or keyword name.
fn gen_upper(rng: &mut Pcg64Dxsm) -> String {
    ["DRAW", "DEAL_DAMAGE", "TEST_OP_9", "X", "A1_"][pick(rng, 5)].to_owned()
}

/// A placeholder instruction.
fn gen_instr(rng: &mut Pcg64Dxsm) -> EffectInstructionAST {
    EffectInstructionAST {
        op: gen_upper(rng),
        params: gen_params(rng),
    }
}

/// Zero to two instructions.
fn gen_instrs(rng: &mut Pcg64Dxsm) -> Vec<EffectInstructionAST> {
    (0..pick(rng, 3)).map(|_| gen_instr(rng)).collect()
}

/// Optional target slots with distinct slot numbers.
fn gen_targets(rng: &mut Pcg64Dxsm) -> Option<Vec<TargetSlotAST>> {
    (pick(rng, 2) == 0).then(|| {
        (0..1 + pick(rng, 3))
            .map(|i| {
                let min = u32::try_from(pick(rng, 3)).expect("small");
                TargetSlotAST {
                    slot: u32::try_from(i * 3 + pick(rng, 3)).expect("small"),
                    selector: gen_selector(rng, 1),
                    min,
                    max: min + u32::try_from(pick(rng, 3)).expect("small"),
                }
            })
            .collect()
    })
}

/// An optional mode set.
fn gen_modes(rng: &mut Pcg64Dxsm) -> Option<ModeSetAST> {
    (pick(rng, 3) == 0).then(|| {
        let n = 1 + pick(rng, 3);
        let modes: Vec<ModeAST> = (0..n)
            .map(|i| ModeAST {
                id: format!("m{i}"),
                targets: gen_targets(rng),
                effect: gen_instrs(rng),
            })
            .collect();
        let max = pick(rng, n + 1);
        let min = pick(rng, max + 1);
        ModeSetAST {
            min: u32::try_from(min).expect("small"),
            max: u32::try_from(max).expect("small"),
            modes,
        }
    })
}

/// A mana cost.
fn gen_mana(rng: &mut Pcg64Dxsm) -> ManaCostDSL {
    let text = ["{2}", "{1}{G}", "{W}{U}", "{0}", "{X}{R}"][pick(rng, 5)];
    ManaCostDSL::parse(text).expect("mana")
}

/// A cost node; COMPOSITE only while `depth > 0`.
fn gen_cost(rng: &mut Pcg64Dxsm, depth: u32) -> CostAST {
    let v = |rng: &mut Pcg64Dxsm| gen_value(rng, 1);
    let s = |rng: &mut Pcg64Dxsm| gen_selector(rng, 1);
    match pick(rng, if depth == 0 { 17 } else { 18 }) {
        0 => CostAST::PayMana {
            mana: gen_mana(rng),
        },
        1 => CostAST::PayLife { amount: v(rng) },
        2 => CostAST::TapSource,
        3 => CostAST::UntapSource,
        4 => CostAST::TapObjects {
            selector: s(rng),
            count: v(rng),
        },
        5 => CostAST::Sacrifice {
            selector: s(rng),
            count: v(rng),
        },
        6 => CostAST::Discard {
            selector: s(rng),
            count: v(rng),
            random: flag(rng),
        },
        7 => {
            let amount = v(rng);
            let by_count = pick(rng, 2) == 0;
            CostAST::ExileFromZone {
                zone: Zone::ALL[pick(rng, Zone::ALL.len())],
                selector: s(rng),
                count: by_count.then(|| amount.clone()),
                min_total_mv: (!by_count).then_some(amount),
            }
        }
        8 => CostAST::RemoveCounters {
            from: s(rng),
            kind: gen_str(rng),
            amount: v(rng),
        },
        9 => CostAST::PutCounters {
            on: s(rng),
            kind: gen_str(rng),
            amount: v(rng),
        },
        10 => CostAST::ReturnToHand { selector: s(rng) },
        11 => CostAST::Reveal { selector: s(rng) },
        12 => CostAST::PayEnergy { amount: v(rng) },
        13 => CostAST::CollectEvidence { n: v(rng) },
        14 => CostAST::Forage,
        15 => CostAST::Waterbend {
            mana: gen_mana(rng),
        },
        16 => CostAST::Loyalty { delta: v(rng) },
        _ => CostAST::Composite {
            costs: (0..1 + pick(rng, 3))
                .map(|_| gen_cost(rng, depth - 1))
                .collect(),
        },
    }
}

/// A non-empty cost list.
fn gen_costs(rng: &mut Pcg64Dxsm) -> Vec<CostAST> {
    (0..1 + pick(rng, 3)).map(|_| gen_cost(rng, 2)).collect()
}

/// An activation limit.
fn gen_limit(rng: &mut Pcg64Dxsm) -> ActivationLimit {
    let n = |rng: &mut Pcg64Dxsm| Some(1 + u32::try_from(pick(rng, 5)).expect("small"));
    match pick(rng, 3) {
        0 => ActivationLimit {
            per_turn: n(rng),
            per_game: None,
        },
        1 => ActivationLimit {
            per_turn: None,
            per_game: n(rng),
        },
        _ => ActivationLimit {
            per_turn: n(rng),
            per_game: Some(u32::MAX),
        },
    }
}

/// A filter value of any variant.
fn gen_filter_value(rng: &mut Pcg64Dxsm) -> FilterValue {
    match pick(rng, 6) {
        0 => FilterValue::Bool(pick(rng, 2) == 0),
        1 => FilterValue::Int(i64::try_from(pick(rng, 7)).expect("small") - 3),
        2 => FilterValue::Any,
        3 => FilterValue::Zone(Zone::ALL[pick(rng, Zone::ALL.len())]),
        4 => FilterValue::Name(gen_str(rng)),
        _ => FilterValue::Predicate(gen_pred(rng, 2)),
    }
}

/// A filter map (possibly empty).
fn gen_filters(rng: &mut Pcg64Dxsm) -> BTreeMap<String, FilterValue> {
    const KEYS: [&str; 6] = ["from", "to", "recipient", "combat", "kind", "source"];
    (0..pick(rng, 4))
        .map(|_| {
            (
                KEYS[pick(rng, KEYS.len())].to_owned(),
                gen_filter_value(rng),
            )
        })
        .collect()
}

/// An event subject.
fn gen_subject(rng: &mut Pcg64Dxsm) -> EventSubject {
    match pick(rng, 3) {
        0 => EventSubject::Players(PlayerGroup::ALL[pick(rng, PlayerGroup::ALL.len())]),
        _ => EventSubject::Object(gen_selector(rng, 2)),
    }
}

/// An event matcher.
fn gen_matcher(rng: &mut Pcg64Dxsm) -> EventMatcher {
    EventMatcher {
        kind: EventKind::ALL[pick(rng, EventKind::ALL.len())],
        subject: (pick(rng, 2) == 0).then(|| gen_subject(rng)),
        filters: (pick(rng, 2) == 0).then(|| gen_filters(rng)),
        bind: (pick(rng, 2) == 0).then(|| {
            (0..pick(rng, 3))
                .map(|_| {
                    (
                        ["recipient", "source", "amount"][pick(rng, 3)].to_owned(),
                        ["$P", "$x_1", "$S"][pick(rng, 3)].to_owned(),
                    )
                })
                .collect()
        }),
        batch: [None, Some(EventBatch::Each), Some(EventBatch::OneOrMore)][pick(rng, 3)],
    }
}

/// A trigger of any mode.
fn gen_trigger(rng: &mut Pcg64Dxsm) -> Trigger {
    match pick(rng, 3) {
        0 => Trigger::Event(gen_matcher(rng)),
        1 => Trigger::PhaseStep {
            step: StepId::ALL[pick(rng, StepId::ALL.len())],
            whose: gen_player(rng),
        },
        _ => Trigger::State(gen_pred(rng, 2)),
    }
}

/// A typed power/toughness.
fn gen_pt(rng: &mut Pcg64Dxsm) -> PtValue {
    if pick(rng, 2) == 0 {
        return PtValue::Int(i64::try_from(pick(rng, 9)).expect("small") - 4);
    }
    loop {
        let v = gen_value(rng, 2);
        if v != ValueExpressionAST::X {
            return PtValue::Expr(v);
        }
    }
}

/// A modification (typed P/T ops or opaque ops).
fn gen_modification(rng: &mut Pcg64Dxsm) -> Modification {
    if pick(rng, 2) == 0 {
        Modification {
            op: ["SET_BASE_PT", "MODIFY_PT"][pick(rng, 2)].to_owned(),
            power: Some(gen_pt(rng)),
            toughness: Some(gen_pt(rng)),
            params: gen_params(rng),
        }
    } else {
        let mut params = gen_params(rng);
        if pick(rng, 3) == 0 {
            params.insert("power".to_owned(), gen_opaque(rng, 1));
        }
        Modification {
            op: ["REMOVE_ALL_ABILITIES", "ADD_ABILITY", "SET_COLOR"][pick(rng, 3)].to_owned(),
            power: None,
            toughness: None,
            params,
        }
    }
}

/// A RULE effect.
fn gen_rule(rng: &mut Pcg64Dxsm) -> RuleModification {
    RuleModification {
        kind: RuleKind::ALL[pick(rng, RuleKind::ALL.len())],
        cost: (pick(rng, 3) == 0).then(|| gen_costs(rng)),
        params: gen_params(rng),
    }
}

/// One static effect whose `affected` (for CHAR) is a selector.
fn gen_static_effect(rng: &mut Pcg64Dxsm) -> StaticEffect {
    match pick(rng, 3) {
        0 => StaticEffect::Char(CharacteristicEffect {
            layer: Layer::ALL[pick(rng, Layer::ALL.len())],
            affected: Affected::Selector(gen_selector(rng, 2)),
            modification: gen_modification(rng),
            is_cda: flag(rng),
        }),
        1 => StaticEffect::Player(PlayerEffect {
            affected: gen_player(rng),
            modification: gen_modification(rng),
        }),
        _ => StaticEffect::Rule(gen_rule(rng)),
    }
}

/// A valid effect list, sometimes with anchored lock-in dependants (613.6).
fn gen_static_effects(rng: &mut Pcg64Dxsm) -> Vec<StaticEffect> {
    let mut effects: Vec<StaticEffect> = (0..1 + pick(rng, 3))
        .map(|_| gen_static_effect(rng))
        .collect();
    let anchors: Vec<Layer> = effects
        .iter()
        .filter_map(|e| match e {
            StaticEffect::Char(c) if matches!(c.affected, Affected::Selector(_)) => Some(c.layer),
            _ => None,
        })
        .collect();
    if !anchors.is_empty() {
        for _ in 0..pick(rng, 3) {
            let at = pick(rng, effects.len() + 1);
            effects.insert(
                at,
                StaticEffect::Char(CharacteristicEffect {
                    layer: Layer::ALL[pick(rng, Layer::ALL.len())],
                    affected: Affected::SameAsLayer(anchors[pick(rng, anchors.len())]),
                    modification: gen_modification(rng),
                    is_cda: flag(rng),
                }),
            );
        }
    }
    effects
}

/// A replacement op.
fn gen_rep_op(rng: &mut Pcg64Dxsm) -> ReplacementOp {
    match pick(rng, 5) {
        0 => {
            let mut set = gen_filters(rng);
            set.insert("to".to_owned(), gen_filter_value(rng));
            ReplacementOp::ModifyEvent { set }
        }
        1 => ReplacementOp::Instead {
            instructions: (0..1 + pick(rng, 2)).map(|_| gen_instr(rng)).collect(),
        },
        2 => ReplacementOp::Skip,
        3 => ReplacementOp::Prevent {
            amount: (pick(rng, 2) == 0).then(|| gen_value(rng, 1)),
        },
        _ => ReplacementOp::Choose {
            choice: Box::new(ChoiceSpec::new(
                gen_player(rng),
                ChoiceKind::ALL[pick(rng, ChoiceKind::ALL.len())],
            )),
            store: gen_str(rng),
        },
    }
}

/// A shield.
fn gen_shield(rng: &mut Pcg64Dxsm) -> Shield {
    if pick(rng, 2) == 0 {
        Shield {
            uses: ShieldUses::Count(1 + u32::try_from(pick(rng, 3)).expect("small")),
            counter_kind: None,
        }
    } else {
        Shield {
            uses: ShieldUses::CounterBacked,
            counter_kind: (pick(rng, 2) == 0).then(|| gen_str(rng)),
        }
    }
}

/// An envelope with every optional member present or absent at random.
fn gen_base(rng: &mut Pcg64Dxsm) -> AbilityBase {
    let functions_in = (pick(rng, 2) == 0).then(|| {
        let mut all = Zone::ALL.to_vec();
        rng.shuffle(&mut all);
        all.truncate(1 + pick(rng, 3));
        all
    });
    let text_span = (pick(rng, 2) == 0).then(|| {
        let a = u32::try_from(pick(rng, 50)).expect("small");
        (a, a + u32::try_from(pick(rng, 5)).expect("small"))
    });
    let link_ids = (pick(rng, 3) == 0).then(|| {
        (0..1 + pick(rng, 3))
            .map(|i| format!("link{i}"))
            .collect::<Vec<_>>()
    });
    AbilityBase {
        ability_id: gen_str(rng),
        functions_in,
        text_span,
        link_ids,
        ability_word: (pick(rng, 3) == 0).then(|| gen_str(rng)),
        condition: (pick(rng, 3) == 0).then(|| gen_pred(rng, 2)),
        is_mana_ability: flag(rng),
    }
}

/// A valid ability of the kind with index `kind` (in `AbilityAST::KINDS` order).
fn gen_ability(rng: &mut Pcg64Dxsm, kind: usize) -> AbilityAST {
    let base = gen_base(rng);
    match kind {
        0 => AbilityAST::Spell(SpellAbilityAST {
            base,
            targets: gen_targets(rng),
            modes: gen_modes(rng),
            effect: gen_instrs(rng),
        }),
        1 => AbilityAST::Activated(ActivatedAbilityAST {
            base,
            cost: gen_costs(rng),
            timing: (pick(rng, 2) == 0)
                .then(|| ActivationTiming::ALL[pick(rng, ActivationTiming::ALL.len())]),
            limits: (pick(rng, 2) == 0)
                .then(|| (0..1 + pick(rng, 2)).map(|_| gen_limit(rng)).collect()),
            activate_only_if: (pick(rng, 3) == 0).then(|| gen_pred(rng, 2)),
            is_loyalty: flag(rng),
            targets: gen_targets(rng),
            modes: gen_modes(rng),
            effect: gen_instrs(rng),
        }),
        2 => AbilityAST::Triggered(TriggeredAbilityAST {
            base,
            trigger: gen_trigger(rng),
            intervening_if: (pick(rng, 3) == 0).then(|| gen_pred(rng, 2)),
            optional: flag(rng),
            limit: (pick(rng, 3) == 0).then(|| gen_limit(rng)),
            targets: gen_targets(rng),
            modes: gen_modes(rng),
            effect: gen_instrs(rng),
        }),
        3 => AbilityAST::Static(StaticAbilityAST {
            base,
            effects: gen_static_effects(rng),
        }),
        4 => AbilityAST::Replacement(ReplacementAbilityAST {
            base,
            is_self_replacement: flag(rng),
            tier_hint: (pick(rng, 2) == 0).then(|| TierHint::ALL[pick(rng, TierHint::ALL.len())]),
            matcher: ReplacementMatch {
                event: EventKind::ALL[pick(rng, EventKind::ALL.len())],
                filters: (pick(rng, 2) == 0).then(|| gen_filters(rng)),
                affected: gen_subject(rng),
            },
            replace_with: (0..1 + pick(rng, 3)).map(|_| gen_rep_op(rng)).collect(),
            optional: flag(rng),
            overrides_614_5: flag(rng),
            shield: (pick(rng, 2) == 0).then(|| gen_shield(rng)),
        }),
        _ => AbilityAST::Keyword(KeywordInstanceAST {
            base,
            keyword: gen_upper(rng),
            params: (pick(rng, 2) == 0).then(|| gen_params(rng)),
        }),
    }
}

/// The required members of each kind, in `AbilityAST::KINDS` order (besides `abilityId` and
/// `kind`).
const REQUIRED: [&[&str]; 6] = [
    &["effect"],
    &["cost", "effect"],
    &["trigger", "effect"],
    &["effects"],
    &["match", "replaceWith"],
    &["keyword"],
];

// ---------------------------------------------------------------------------------------------
// Properties

/// REQ-DSL-005..REQ-DSL-009, REQ-NFR-006, REQ-CORE-006: 200 generated abilities of every kind
/// round-trip (W4), encode identically ten times, match `canonicalize`, and decode identically
/// from a member-shuffled spelling; each also decodes as its own variant struct.
#[test]
fn req_dsl_005_009_property_round_trip_every_kind() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f12_0013_0001);
    for i in 0..1200 {
        let a = gen_ability(&mut rng, i % 6);
        assert_eq!(a.kind(), AbilityAST::KINDS[i % 6]);
        assert_wire_props(&a, &mut rng);
        match &a {
            AbilityAST::Spell(x) => assert_wire_props(x, &mut rng),
            AbilityAST::Activated(x) => assert_wire_props(x, &mut rng),
            AbilityAST::Triggered(x) => assert_wire_props(x, &mut rng),
            AbilityAST::Static(x) => assert_wire_props(x, &mut rng),
            AbilityAST::Replacement(x) => assert_wire_props(x, &mut rng),
            AbilityAST::Keyword(x) => assert_wire_props(x, &mut rng),
        }
        assert_wire_props(a.base(), &mut rng);
    }
}

/// REQ-DSL-006..REQ-DSL-009, REQ-NFR-006: the part types round-trip on their own.
#[test]
fn req_dsl_006_009_property_round_trip_parts() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f12_0013_0002);
    for _ in 0..400 {
        assert_wire_props(&gen_cost(&mut rng, 3), &mut rng);
        assert_wire_props(&gen_matcher(&mut rng), &mut rng);
        assert_wire_props(&gen_trigger(&mut rng), &mut rng);
        assert_wire_props(&gen_static_effect(&mut rng), &mut rng);
        assert_wire_props(&gen_modification(&mut rng), &mut rng);
        assert_wire_props(&gen_rep_op(&mut rng), &mut rng);
        assert_wire_props(&gen_shield(&mut rng), &mut rng);
        assert_wire_props(&gen_limit(&mut rng), &mut rng);
        assert_wire_props(&gen_filter_value(&mut rng), &mut rng);
        assert_wire_props(&gen_subject(&mut rng), &mut rng);
        assert_wire_props(&gen_instr(&mut rng), &mut rng);
        assert_wire_props(&gen_pt(&mut rng), &mut rng);
        if let Some(t) = gen_targets(&mut rng) {
            assert_wire_props(&t[0], &mut rng);
        }
        if let Some(m) = gen_modes(&mut rng) {
            assert_wire_props(&m, &mut rng);
            assert_wire_props(&m.modes[0], &mut rng);
        }
    }
}

/// Asserts `doc` (a JSON value) is rejected by `T`.
fn rejected<T: DeserializeOwned + std::fmt::Debug>(doc: &Value) {
    let text = doc.to_string();
    let r: Result<T, _> = serde_json::from_str(&text);
    assert!(r.is_err(), "{text} was accepted as {:?}", r.ok());
}

/// Applies the three mutations to the object `v` (required keys `required`): drop each
/// required member, add an unknown member, null each optional member; all must be rejected.
fn assert_mutations_rejected<T: DeserializeOwned + std::fmt::Debug>(v: &Value, required: &[&str]) {
    let map = v.as_object().expect("object");
    for key in required {
        let mut m = map.clone();
        assert!(m.remove(*key).is_some(), "{key} missing from {v}");
        rejected::<T>(&Value::Object(m));
    }
    let mut m = map.clone();
    m.insert("zzUnknownMember".to_owned(), json!(1));
    rejected::<T>(&Value::Object(m));
    for key in map.keys().filter(|k| !required.contains(&k.as_str())) {
        let mut m = map.clone();
        m.insert(key.clone(), Value::Null);
        rejected::<T>(&Value::Object(m));
    }
}

/// REQ-DSL-005..REQ-DSL-009, REQ-NFR-006: dropping a required member, adding an unknown member
/// or nulling an optional one is rejected, for generated abilities of every kind and for the
/// strict part types.
#[test]
fn req_dsl_005_009_property_mutations_rejected() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f12_0013_0003);
    for i in 0..600 {
        let a = gen_ability(&mut rng, i % 6);
        let v = serde_json::to_value(&a).expect("encode");
        let mut required = vec!["abilityId", "kind"];
        required.extend_from_slice(REQUIRED[i % 6]);
        assert_mutations_rejected::<AbilityAST>(&v, &required);

        let m = serde_json::to_value(gen_matcher(&mut rng)).expect("encode");
        assert_mutations_rejected::<EventMatcher>(&m, &["kind"]);
        let s = serde_json::to_value(gen_shield(&mut rng)).expect("encode");
        assert_mutations_rejected::<Shield>(&s, &["uses"]);
        let t = serde_json::to_value(gen_trigger(&mut rng)).expect("encode");
        let t_required: Vec<&str> = t
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_mutations_rejected::<Trigger>(&t, &t_required);
        let c = gen_cost(&mut rng, 1);
        if !matches!(c, CostAST::ExileFromZone { .. }) {
            let cv = serde_json::to_value(&c).expect("encode");
            let optional = ["random"];
            let c_required: Vec<&str> = cv
                .as_object()
                .expect("object")
                .keys()
                .map(String::as_str)
                .filter(|k| !optional.contains(k))
                .collect();
            assert_mutations_rejected::<CostAST>(&cv, &c_required);
        }
    }
}

/// Schema words that make random input reach deeper decoder paths.
fn ability_doc(rng: &mut Pcg64Dxsm) -> Vec<u8> {
    const PIECES: [&str; 22] = [
        "{\"abilityId\":\"a\",",
        "\"kind\":",
        "\"TRIGGERED\"",
        "\"STATIC\"",
        "\"cost\":",
        "\"TAP_SOURCE\"",
        "\"COMPOSITE\"",
        "\"costs\":[",
        "\"effects\":[",
        "{\"type\":\"CHAR\",",
        "\"layer\":\"6\"",
        "{\"sameAsLayer\":\"6\"}",
        "\"op\":\"SKIP\"",
        "\"replaceWith\":[",
        "\"trigger\":{\"mode\":\"EVENT\",\"event\":{\"kind\":\"DIES\"",
        "\"filters\":{",
        "\"lookBack\":",
        "\"bind\":{\"x\":\"$x\"}",
        "\"effect\":[]",
        "}",
        "]",
        ",",
    ];
    let mut out = Vec::new();
    for _ in 0..pick(rng, 16) {
        if pick(rng, 3) == 0 {
            out.extend_from_slice(&random_doc(rng));
        } else {
            out.extend_from_slice(PIECES[pick(rng, PIECES.len())].as_bytes());
        }
    }
    out
}

/// Runs every decoder on `doc`.
fn decode_all(doc: &[u8]) {
    decode_never_panics::<AbilityAST>(doc);
    decode_never_panics::<EventMatcher>(doc);
    decode_never_panics::<CostAST>(doc);
    decode_never_panics::<StaticEffect>(doc);
    decode_never_panics::<ReplacementOp>(doc);
    decode_never_panics::<Trigger>(doc);
    decode_never_panics::<FilterValue>(doc);
    decode_never_panics::<EffectInstructionAST>(doc);
    decode_never_panics::<Modification>(doc);
    decode_never_panics::<Shield>(doc);
    decode_never_panics::<AbilityBase>(doc);
}

/// REQ-NFR-006, R12: random bytes (2,500 generic and 2,500 schema-biased inputs) never panic
/// any decoder; whatever decodes re-encodes to itself.
#[test]
fn req_nfr_006_random_bytes_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f12_0013_0004);
    for _ in 0..2500 {
        decode_all(&random_doc(&mut rng));
        decode_all(&ability_doc(&mut rng));
    }
}

/// Encodes `v` and feeds `n` byte-level mutations of it to `T`'s decoder.
fn mutated<T>(v: &impl Serialize, n: usize, rng: &mut Pcg64Dxsm)
where
    T: DeserializeOwned + Serialize + PartialEq + std::fmt::Debug,
{
    let doc = serde_json::to_vec(v).expect("encode");
    for _ in 0..n {
        decode_never_panics::<T>(&mutate(&doc, rng));
    }
}

/// REQ-NFR-006, R12: byte-mutated valid documents (2,000 or more per type) never panic.
#[test]
fn req_nfr_006_mutated_docs_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f12_0013_0005);
    for i in 0..600 {
        let a = gen_ability(&mut rng, i % 6);
        mutated::<AbilityAST>(&a, 4, &mut rng);
        let m = gen_matcher(&mut rng);
        mutated::<EventMatcher>(&m, 4, &mut rng);
        let c = gen_cost(&mut rng, 2);
        mutated::<CostAST>(&c, 4, &mut rng);
        let s = gen_static_effect(&mut rng);
        mutated::<StaticEffect>(&s, 4, &mut rng);
        let r = gen_rep_op(&mut rng);
        mutated::<ReplacementOp>(&r, 4, &mut rng);
    }
}
