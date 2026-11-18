// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Seeded property tests for the effect-instruction schema (REQ-DSL-010, REQ-DSL-015,
//! REQ-NFR-006, REQ-CORE-006, R7, R12): generated instruction trees using every op round-trip
//! canonically and independently of member order; random and mutated bytes never panic a
//! decoder. Fixed seeds only, no wall clock.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use fineprint_base::prng::Pcg64Dxsm;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::super::*;
use crate::ability::{
    Affected, CharacteristicEffect, CostAST, EventKind, EventMatcher, EventSubject, Layer,
    Modification, PlayerEffect, RuleKind, RuleModification, StaticEffect, StepId, TargetSlotAST,
    Trigger,
};
use crate::choice::{ChoiceCandidates, ChoiceKind, ChoiceSpec};
use crate::expr::tests::{
    assert_wire_props, decode_never_panics, gen_player, gen_pred, gen_selector, gen_str, gen_value,
    mutate, pick, random_doc,
};
use crate::expr::{PlayerGroup, Zone};

// ---------------------------------------------------------------------------------------------
// Generators (synthetic vocabulary only, R5)

/// A flag that is absent, true or false.
fn flag(rng: &mut Pcg64Dxsm) -> Option<bool> {
    [None, Some(true), Some(false)][pick(rng, 3)]
}

/// `Some(f(rng))` half the time.
fn maybe<T>(rng: &mut Pcg64Dxsm, f: impl FnOnce(&mut Pcg64Dxsm) -> T) -> Option<T> {
    (pick(rng, 2) == 0).then(|| f(rng))
}

/// A small value expression.
fn val(rng: &mut Pcg64Dxsm) -> crate::expr::ValueExpressionAST {
    gen_value(rng, 1)
}

/// A small object selector.
fn sel(rng: &mut Pcg64Dxsm) -> crate::expr::ObjectSelectorAST {
    gen_selector(rng, 1)
}

/// An opaque JSON value (no null) nested at most `depth` levels; member names may be empty.
fn gen_opaque(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    match pick(rng, if depth == 0 { 5 } else { 7 }) {
        0 => json!(false),
        1 => json!(-7),
        2 => json!(-9_007_199_254_740_991_i64),
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
                let key = if pick(rng, 4) == 0 {
                    String::new()
                } else {
                    gen_str(rng)
                };
                m.insert(key, gen_opaque(rng, depth - 1));
            }
            Value::Object(m)
        }
    }
}

/// An opaque object with at least `min` members.
fn gen_opaque_map(rng: &mut Pcg64Dxsm, min: usize) -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    let mut i = 0;
    while m.len() < min + pick(rng, 3) {
        m.insert(format!("k{i}"), gen_opaque(rng, 2));
        i += 1;
    }
    m
}

/// A valid choice spec.
fn gen_choice(rng: &mut Pcg64Dxsm) -> ChoiceSpec {
    let mut c = ChoiceSpec::new(
        gen_player(rng),
        ChoiceKind::ALL[pick(rng, ChoiceKind::ALL.len())],
    );
    c.candidates = match pick(rng, 4) {
        0 => Some(ChoiceCandidates::Selector(sel(rng))),
        1 => Some(ChoiceCandidates::Players(gen_player(rng))),
        2 => Some(ChoiceCandidates::Enum(vec!["a".to_owned(), "b".to_owned()])),
        _ => None,
    };
    if pick(rng, 2) == 0 {
        c.max = Some(crate::expr::ValueExpressionAST::Literal(3));
        c.up_to = Some(true);
    } else {
        c.min = maybe(rng, |_| crate::expr::ValueExpressionAST::Literal(1));
        c.up_to = [None, Some(false)][pick(rng, 2)];
    }
    c.hidden = flag(rng);
    c.random = flag(rng);
    c.reveal_to = maybe(rng, gen_player);
    c
}

/// An optional choice spec.
fn opt_choice(rng: &mut Pcg64Dxsm) -> Option<ChoiceSpec> {
    maybe(rng, gen_choice)
}

/// A cost component (no nested composites).
fn gen_cost(rng: &mut Pcg64Dxsm) -> CostAST {
    match pick(rng, 6) {
        0 => CostAST::TapSource,
        1 => CostAST::UntapSource,
        2 => CostAST::Forage,
        3 => CostAST::PayLife { amount: val(rng) },
        4 => CostAST::Sacrifice {
            selector: sel(rng),
            count: val(rng),
        },
        _ => CostAST::PayMana {
            mana: crate::mana::ManaCostDSL::parse(["{2}", "{1}{G}", "{X}{R}"][pick(rng, 3)])
                .expect("mana"),
        },
    }
}

/// A non-empty cost list.
fn gen_costs(rng: &mut Pcg64Dxsm) -> Vec<CostAST> {
    (0..1 + pick(rng, 2)).map(|_| gen_cost(rng)).collect()
}

/// An event subject.
fn gen_subject(rng: &mut Pcg64Dxsm) -> EventSubject {
    if pick(rng, 3) == 0 {
        EventSubject::Players(PlayerGroup::ALL[pick(rng, PlayerGroup::ALL.len())])
    } else {
        EventSubject::Object(sel(rng))
    }
}

/// A trigger.
fn gen_trigger(rng: &mut Pcg64Dxsm) -> Trigger {
    match pick(rng, 3) {
        0 => Trigger::Event(EventMatcher::new(
            EventKind::ALL[pick(rng, EventKind::ALL.len())],
        )),
        1 => Trigger::PhaseStep {
            step: StepId::ALL[pick(rng, StepId::ALL.len())],
            whose: gen_player(rng),
        },
        _ => Trigger::State(gen_pred(rng, 1)),
    }
}

/// A static effect (CHAR with a selector or `sameAsLayer`, PLAYER or RULE).
fn gen_static(rng: &mut Pcg64Dxsm) -> StaticEffect {
    match pick(rng, 3) {
        0 => StaticEffect::Char(CharacteristicEffect {
            layer: Layer::ALL[pick(rng, Layer::ALL.len())],
            affected: if pick(rng, 4) == 0 {
                Affected::SameAsLayer(Layer::ALL[pick(rng, Layer::ALL.len())])
            } else {
                Affected::Selector(sel(rng))
            },
            modification: Modification::new(["TEST_OP", "X_1"][pick(rng, 2)]),
            is_cda: flag(rng),
        }),
        1 => StaticEffect::Player(PlayerEffect {
            affected: gen_player(rng),
            modification: Modification::new("TEST_PLAYER_OP"),
        }),
        _ => StaticEffect::Rule(RuleModification::new(
            RuleKind::ALL[pick(rng, RuleKind::ALL.len())],
        )),
    }
}

/// A duration of any kind.
fn gen_duration(rng: &mut Pcg64Dxsm) -> DurationAST {
    match pick(rng, 9) {
        0 => DurationAST::UntilEndOfTurn,
        1 => DurationAST::UntilYourNextTurn,
        2 => DurationAST::UntilEndOfCombat,
        3 => DurationAST::Until {
            step: StepId::ALL[pick(rng, StepId::ALL.len())],
            whose: gen_player(rng),
        },
        4 => DurationAST::ForAsLongAs {
            predicate: gen_pred(rng, 2),
        },
        5 => DurationAST::WhileSourceOnBattlefield,
        6 => DurationAST::WhileControlledBy {
            player: gen_player(rng),
        },
        7 => DurationAST::Permanent,
        _ => DurationAST::Perpetual,
    }
}

/// A non-empty STR.
fn s(rng: &mut Pcg64Dxsm) -> String {
    gen_str(rng)
}

/// A `$` variable name.
fn var(rng: &mut Pcg64Dxsm) -> String {
    ["$x", "$Each_2", "$_", "$9"][pick(rng, 4)].to_owned()
}

/// A non-empty instruction list of depth at most `depth`.
fn gen_instrs(rng: &mut Pcg64Dxsm, depth: u32) -> Vec<Instruction> {
    (0..1 + pick(rng, 2))
        .map(|_| gen_instr(rng, depth))
        .collect()
}

/// Any instruction of depth at most `depth`.
fn gen_instr(rng: &mut Pcg64Dxsm, depth: u32) -> Instruction {
    let op = pick(rng, Instruction::NAMES.len());
    gen_op(rng, op, depth)
}

/// A native effect.
fn gen_native(rng: &mut Pcg64Dxsm) -> NativeEffectAST {
    let mut n = NativeEffectAST::new(
        ["test.native", "n", "a.b.c_1"][pick(rng, 3)],
        [1, 2, u32::MAX][pick(rng, 3)],
    );
    n.params = gen_opaque_map(rng, 0);
    n.layers = maybe(rng, |rng| {
        let mut all = Layer::ALL.to_vec();
        rng.shuffle(&mut all);
        all.truncate(1 + pick(rng, 4));
        all
    });
    n.rule_kinds = maybe(rng, |rng| {
        let mut all = RuleKind::ALL.to_vec();
        rng.shuffle(&mut all);
        all.truncate(1 + pick(rng, 4));
        all
    });
    n
}

/// Modes for CHOOSE_MODE with valid bounds.
fn gen_choose_mode(rng: &mut Pcg64Dxsm, depth: u32) -> Instruction {
    let n = 1 + pick(rng, 3);
    let modes: Vec<InstructionMode> = (0..n)
        .map(|i| InstructionMode {
            id: format!("m{i}"),
            targets: maybe(rng, |rng| {
                (0..1 + pick(rng, 2))
                    .map(|j| TargetSlotAST {
                        slot: u32::try_from(j * 2 + pick(rng, 2)).expect("small"),
                        selector: sel(rng),
                        min: 0,
                        max: 1,
                    })
                    .collect()
            }),
            effect: gen_instrs(rng, depth),
        })
        .collect();
    let max = pick(rng, n + 1);
    let min = pick(rng, max + 1);
    Instruction::ChooseMode {
        chooser: gen_player(rng),
        min: u32::try_from(min).expect("small"),
        max: u32::try_from(max).expect("small"),
        modes,
    }
}

/// The instruction whose catalogue index is `op`; the nested-list ops fall back to a leaf
/// when `depth` is 0.
#[allow(clippy::too_many_lines)]
fn gen_op(rng: &mut Pcg64Dxsm, op: usize, depth: u32) -> Instruction {
    use Instruction as I;
    let d = depth.saturating_sub(1);
    if depth == 0 && (60..=68).contains(&op) {
        return I::Populate;
    }
    match op {
        0 => I::Draw {
            player: gen_player(rng),
            count: val(rng),
        },
        1 => I::Mill {
            player: gen_player(rng),
            count: val(rng),
        },
        2 => I::Discard {
            player: gen_player(rng),
            count: val(rng),
            choice: opt_choice(rng),
        },
        3 => I::Move {
            objects: sel(rng),
            to_zone: Zone::ALL[pick(rng, Zone::ALL.len())],
            position: maybe(rng, |rng| Position::ALL[pick(rng, 2)]),
            face_down: flag(rng),
            tapped: flag(rng),
            under_control_of: maybe(rng, gen_player),
            transformed: flag(rng),
            choice: opt_choice(rng),
        },
        4 => I::SearchLibrary {
            player: gen_player(rng),
            selector: sel(rng),
            count: val(rng),
            to_zone: Zone::ALL[pick(rng, Zone::ALL.len())],
            position: maybe(rng, |rng| Position::ALL[pick(rng, 2)]),
            choice: opt_choice(rng),
        },
        5 => I::Shuffle {
            player: gen_player(rng),
        },
        6 => I::Reveal {
            objects: sel(rng),
            to: maybe(rng, gen_player),
            choice: opt_choice(rng),
        },
        7 => I::LookAt {
            objects: sel(rng),
            player: gen_player(rng),
        },
        8 => I::PutOnTopOrBottom {
            objects: sel(rng),
            player: gen_player(rng),
            choice: opt_choice(rng),
        },
        9 => I::ExileUntil {
            objects: sel(rng),
            duration: gen_duration(rng),
            face_down: flag(rng),
        },
        10 => I::PlayPermissionGrant {
            objects: sel(rng),
            duration: gen_duration(rng),
            who: gen_player(rng),
            also_as: maybe(rng, s),
        },
        11 => I::DealDamage {
            source: sel(rng),
            recipients: gen_subject(rng),
            amount: val(rng),
            divided: flag(rng),
            choice: opt_choice(rng),
        },
        12 => I::Fight {
            first: sel(rng),
            second: sel(rng),
        },
        13 => I::Bite {
            source: sel(rng),
            target: sel(rng),
        },
        14 => I::GainLife {
            player: gen_player(rng),
            amount: val(rng),
        },
        15 => I::LoseLife {
            player: gen_player(rng),
            amount: val(rng),
        },
        16 => I::SetLife {
            player: gen_player(rng),
            amount: val(rng),
        },
        17 => I::Prevent {
            duration: gen_duration(rng),
            source: maybe(rng, sel),
            recipients: maybe(rng, gen_subject),
            amount: maybe(rng, val),
            combat_only: flag(rng),
        },
        18 => I::Destroy {
            objects: sel(rng),
            no_regeneration: flag(rng),
        },
        19 => I::Sacrifice {
            player: gen_player(rng),
            selector: sel(rng),
            count: val(rng),
            choice: opt_choice(rng),
        },
        20 => I::Regenerate { objects: sel(rng) },
        21 => I::CounterSpell {
            objects: sel(rng),
            unless: maybe(rng, gen_costs),
        },
        22 => I::PutCounters {
            objects: sel(rng),
            kind: s(rng),
            count: val(rng),
        },
        23 => I::RemoveCounters {
            objects: sel(rng),
            kind: s(rng),
            count: val(rng),
        },
        24 => I::MoveCounters {
            from: sel(rng),
            to: sel(rng),
            kind: s(rng),
            count: val(rng),
        },
        25 => I::Proliferate {
            choice: opt_choice(rng),
        },
        26 => I::CreateToken {
            token: if pick(rng, 2) == 0 {
                TokenSource::Def(s(rng))
            } else {
                TokenSource::Inline(gen_opaque_map(rng, 0))
            },
            count: val(rng),
            modifiers: maybe(rng, |rng| gen_opaque_map(rng, 1)),
            controller: maybe(rng, gen_player),
        },
        27 => I::CreatePredefinedToken {
            kind: s(rng),
            count: maybe(rng, val),
        },
        28 => I::CreateCopyToken {
            of: sel(rng),
            except: maybe(rng, |rng| gen_opaque_map(rng, 1)),
        },
        29 => I::Populate,
        30 => I::Amass { n: val(rng) },
        31 => I::Incubate { n: val(rng) },
        32 => I::CopySpellOrAbility {
            objects: sel(rng),
            new_targets: flag(rng),
        },
        33 => I::ChangeTargets {
            objects: sel(rng),
            choice: opt_choice(rng),
        },
        34 => I::CastFrom {
            objects: sel(rng),
            without_paying: flag(rng),
            alt_cost: maybe(rng, gen_costs),
        },
        35 => I::TapOrUntap {
            objects: sel(rng),
            action: TapAction::ALL[pick(rng, 2)],
        },
        36 => I::Transform { objects: sel(rng) },
        37 => I::Convert { objects: sel(rng) },
        38 => I::TurnFaceUp {
            objects: sel(rng),
            profile: maybe(rng, s),
        },
        39 => I::TurnFaceDown {
            objects: sel(rng),
            profile: maybe(rng, s),
        },
        40 => I::PhaseOut { objects: sel(rng) },
        41 => I::Attach {
            objects: sel(rng),
            to: sel(rng),
        },
        42 => I::GainControl {
            objects: sel(rng),
            duration: gen_duration(rng),
            player: maybe(rng, gen_player),
        },
        43 => I::ExchangeControl {
            first: sel(rng),
            second: sel(rng),
        },
        44 => I::Goad { objects: sel(rng) },
        45 => I::SetDesignation {
            kind: s(rng),
            value: match pick(rng, 3) {
                0 => DesignationValue::Flag(true),
                1 => DesignationValue::Flag(false),
                _ => DesignationValue::Name(s(rng)),
            },
            to: maybe(rng, gen_subject),
        },
        46 => I::Venture,
        47 => I::RingTempts,
        48 => I::IncreaseSpeed {
            player: gen_player(rng),
            amount: maybe(rng, val),
        },
        49 => I::EmpowerJace { n: val(rng) },
        50 => I::Meld { objects: sel(rng) },
        51 => I::MutateMerge {
            objects: sel(rng),
            onto: sel(rng),
        },
        52 => I::Manifest { objects: sel(rng) },
        53 => I::Cloak { objects: sel(rng) },
        54 => I::Explore { objects: sel(rng) },
        55 => I::Connive { objects: sel(rng) },
        56 => I::Discover { n: val(rng) },
        57 => I::Blight { n: val(rng) },
        58 => I::Waterbend {
            cost: gen_costs(rng),
        },
        59 => {
            let effects: Vec<StaticEffect> =
                (0..1 + pick(rng, 3)).map(|_| gen_static(rng)).collect();
            let duration = gen_duration(rng);
            // CR 611.2c: `false` is only valid when no effect is CHAR.
            let mut lock_affected_set = flag(rng);
            if lock_affected_set == Some(false)
                && effects.iter().any(|e| matches!(e, StaticEffect::Char(_)))
            {
                lock_affected_set = if pick(rng, 2) == 0 { None } else { Some(true) };
            }
            I::CreateContinuousEffect {
                effects,
                duration,
                lock_affected_set,
            }
        }
        60 => I::CreateDelayedTrigger {
            trigger: gen_trigger(rng),
            effect: gen_instrs(rng, d),
            captures: maybe(rng, |rng| {
                (0..1 + pick(rng, 2))
                    .map(|_| (var(rng), sel(rng)))
                    .collect()
            }),
            one_shot: pick(rng, 2) == 0,
        },
        61 => I::ReflexiveTrigger {
            if_action_taken: gen_instrs(rng, d),
            effect: gen_instrs(rng, d),
        },
        62 => I::If {
            cond: gen_pred(rng, 1),
            then: gen_instrs(rng, d),
            otherwise: maybe(rng, |rng| gen_instrs(rng, d)),
        },
        63 => I::ForEach {
            iterator: gen_subject(rng),
            bind: maybe(rng, var),
            body: gen_instrs(rng, d),
        },
        64 => I::Repeat {
            times: val(rng),
            body: gen_instrs(rng, d),
        },
        65 => I::RepeatWhile {
            cond: gen_pred(rng, 1),
            max_iter: NonZeroU32::new([1, 2, 100, u32::MAX][pick(rng, 4)]).expect("non-zero"),
            body: gen_instrs(rng, d),
        },
        66 => I::May {
            player: gen_player(rng),
            body: gen_instrs(rng, d),
        },
        67 => I::Unless {
            payer: gen_player(rng),
            cost: gen_costs(rng),
            body: gen_instrs(rng, d),
        },
        68 => gen_choose_mode(rng, d),
        69 => I::Vote {
            voters: gen_player(rng),
            choice: gen_choice(rng),
            store: s(rng),
        },
        70 => I::Remember {
            key: s(rng),
            into: MemoryStore::ALL[pick(rng, 2)],
            source: if pick(rng, 2) == 0 {
                RememberSource::Value(val(rng))
            } else {
                RememberSource::Objects(sel(rng))
            },
        },
        _ => I::NativeEffect(gen_native(rng)),
    }
}

// ---------------------------------------------------------------------------------------------
// Properties

/// REQ-DSL-010, REQ-DSL-015, REQ-NFR-006 (acceptance criterion "round-trip property tests over
/// generated instruction trees"): for every op, generated trees round-trip, encode to stable
/// canonical bytes, re-decode from their own encoding and decode independently of member
/// order and whitespace.
#[test]
fn req_dsl_010_property_round_trip_every_op() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f13_0014_0001);
    for round in 0..12 {
        for op in 0..Instruction::NAMES.len() {
            let depth = u32::try_from(round % 4).expect("small");
            let i = gen_op(&mut rng, op, depth);
            if depth > 0 || !(60..=68).contains(&op) {
                assert_eq!(i.name(), Instruction::NAMES[op]);
            }
            assert_wire_props(&i, &mut rng);
        }
    }
}

/// REQ-DSL-010: random deeper trees (any op at every level) round-trip.
#[test]
fn req_dsl_010_property_round_trip_trees() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f13_0014_0002);
    for _ in 0..400 {
        let tree = gen_instrs(&mut rng, 4);
        assert_wire_props(&tree, &mut rng);
    }
}

/// REQ-DSL-010, REQ-DSL-015: the auxiliary types round-trip on their own.
#[test]
fn req_dsl_010_property_round_trip_parts() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f13_0014_0003);
    for _ in 0..300 {
        assert_wire_props(&gen_duration(&mut rng), &mut rng);
        assert_wire_props(&gen_native(&mut rng), &mut rng);
        if let Instruction::ChooseMode { modes, .. } = gen_choose_mode(&mut rng, 1) {
            for m in &modes {
                assert_wire_props(m, &mut rng);
            }
        }
        let t = if pick(&mut rng, 2) == 0 {
            TokenSource::Def(gen_str(&mut rng))
        } else {
            TokenSource::Inline(gen_opaque_map(&mut rng, 0))
        };
        assert_wire_props(&t, &mut rng);
        assert_wire_props(&RememberSource::Objects(sel(&mut rng)), &mut rng);
        assert_wire_props(&RememberSource::Value(val(&mut rng)), &mut rng);
        assert_wire_props(&DesignationValue::Name(gen_str(&mut rng)), &mut rng);
        assert_wire_props(&DesignationValue::Flag(pick(&mut rng, 2) == 0), &mut rng);
    }
}

/// One row of the independent member table: op, required members, optional members and
/// exactly-one groups. Written from the wire catalogue (docs/DECISIONS.md "#14 FP-013 — Wire
/// catalogue as implemented" and the op docs), not from the decoder. `op` itself is required
/// for every row and is added by the lookup.
type Shape = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [[&'static str; 2]],
);

/// The independent table (REQ-DSL-010).
const SHAPES: [Shape; 72] = [
    ("DRAW", &["player", "count"], &[], &[]),
    ("MILL", &["player", "count"], &[], &[]),
    ("DISCARD", &["player", "count"], &["choice"], &[]),
    (
        "MOVE",
        &["objects", "toZone"],
        &[
            "position",
            "faceDown",
            "tapped",
            "underControlOf",
            "transformed",
            "choice",
        ],
        &[],
    ),
    (
        "SEARCH_LIBRARY",
        &["player", "selector", "count", "toZone"],
        &["position", "choice"],
        &[],
    ),
    ("SHUFFLE", &["player"], &[], &[]),
    ("REVEAL", &["objects"], &["to", "choice"], &[]),
    ("LOOK_AT", &["objects", "player"], &[], &[]),
    (
        "PUT_ON_TOP_OR_BOTTOM",
        &["objects", "player"],
        &["choice"],
        &[],
    ),
    ("EXILE_UNTIL", &["objects", "duration"], &["faceDown"], &[]),
    (
        "PLAY_PERMISSION_GRANT",
        &["objects", "duration", "who"],
        &["alsoAs"],
        &[],
    ),
    (
        "DEAL_DAMAGE",
        &["source", "recipients", "amount"],
        &["divided", "choice"],
        &[],
    ),
    ("FIGHT", &["first", "second"], &[], &[]),
    ("BITE", &["source", "target"], &[], &[]),
    ("GAIN_LIFE", &["player", "amount"], &[], &[]),
    ("LOSE_LIFE", &["player", "amount"], &[], &[]),
    ("SET_LIFE", &["player", "amount"], &[], &[]),
    (
        "PREVENT",
        &["duration"],
        &["source", "recipients", "amount", "combatOnly"],
        &[],
    ),
    ("DESTROY", &["objects"], &["noRegeneration"], &[]),
    (
        "SACRIFICE",
        &["player", "selector", "count"],
        &["choice"],
        &[],
    ),
    ("REGENERATE", &["objects"], &[], &[]),
    ("COUNTER_SPELL", &["objects"], &["unless"], &[]),
    ("PUT_COUNTERS", &["objects", "kind", "count"], &[], &[]),
    ("REMOVE_COUNTERS", &["objects", "kind", "count"], &[], &[]),
    ("MOVE_COUNTERS", &["from", "to", "kind", "count"], &[], &[]),
    ("PROLIFERATE", &[], &["choice"], &[]),
    (
        "CREATE_TOKEN",
        &["count"],
        &["modifiers", "controller"],
        &[["tokenDef", "inline"]],
    ),
    ("CREATE_PREDEFINED_TOKEN", &["kind"], &["count"], &[]),
    ("CREATE_COPY_TOKEN", &["of"], &["except"], &[]),
    ("POPULATE", &[], &[], &[]),
    ("AMASS", &["n"], &[], &[]),
    ("INCUBATE", &["n"], &[], &[]),
    ("COPY_SPELL_OR_ABILITY", &["objects"], &["newTargets"], &[]),
    ("CHANGE_TARGETS", &["objects"], &["choice"], &[]),
    (
        "CAST_FROM",
        &["objects"],
        &["withoutPaying", "altCost"],
        &[],
    ),
    ("TAP_OR_UNTAP", &["objects", "action"], &[], &[]),
    ("TRANSFORM", &["objects"], &[], &[]),
    ("CONVERT", &["objects"], &[], &[]),
    ("TURN_FACE_UP", &["objects"], &["profile"], &[]),
    ("TURN_FACE_DOWN", &["objects"], &["profile"], &[]),
    ("PHASE_OUT", &["objects"], &[], &[]),
    ("ATTACH", &["objects", "to"], &[], &[]),
    ("GAIN_CONTROL", &["objects", "duration"], &["player"], &[]),
    ("EXCHANGE_CONTROL", &["first", "second"], &[], &[]),
    ("GOAD", &["objects"], &[], &[]),
    ("SET_DESIGNATION", &["kind", "value"], &["to"], &[]),
    ("VENTURE", &[], &[], &[]),
    ("RING_TEMPTS", &[], &[], &[]),
    ("INCREASE_SPEED", &["player"], &["amount"], &[]),
    ("EMPOWER_JACE", &["n"], &[], &[]),
    ("MELD", &["objects"], &[], &[]),
    ("MUTATE_MERGE", &["objects", "onto"], &[], &[]),
    ("MANIFEST", &["objects"], &[], &[]),
    ("CLOAK", &["objects"], &[], &[]),
    ("EXPLORE", &["objects"], &[], &[]),
    ("CONNIVE", &["objects"], &[], &[]),
    ("DISCOVER", &["n"], &[], &[]),
    ("BLIGHT", &["n"], &[], &[]),
    ("WATERBEND", &["cost"], &[], &[]),
    (
        "CREATE_CONTINUOUS_EFFECT",
        &["effects", "duration"],
        &["lockAffectedSet"],
        &[],
    ),
    (
        "CREATE_DELAYED_TRIGGER",
        &["trigger", "effect", "oneShot"],
        &["captures"],
        &[],
    ),
    ("REFLEXIVE_TRIGGER", &["ifActionTaken", "effect"], &[], &[]),
    ("IF", &["cond", "then"], &["else"], &[]),
    ("FOR_EACH", &["iterator", "body"], &["bind"], &[]),
    ("REPEAT", &["times", "body"], &[], &[]),
    ("REPEAT_WHILE", &["cond", "maxIter", "body"], &[], &[]),
    ("MAY", &["player", "body"], &[], &[]),
    ("UNLESS", &["payer", "cost", "body"], &[], &[]),
    ("CHOOSE_MODE", &["chooser", "min", "max", "modes"], &[], &[]),
    ("VOTE", &["voters", "choice", "store"], &[], &[]),
    ("REMEMBER", &["key", "into"], &[], &[["value", "objects"]]),
    (
        "NATIVE_EFFECT",
        &["id", "version", "params"],
        &["layers", "ruleKinds"],
        &[],
    ),
];

/// REQ-DSL-010: removing any one member of a generated instruction's top-level object is
/// classified from the independent [`SHAPES`] table. Removing a required member, or the present
/// arm of an exactly-one pair, is rejected; removing an optional member is accepted and the
/// canonical re-encoding equals the edited document. Nulling any member and adding an unknown
/// one are rejected.
#[test]
fn req_dsl_010_property_member_removal() {
    // The table covers every op exactly once, in catalogue order.
    for (row, name) in SHAPES.iter().zip(Instruction::NAMES) {
        assert_eq!(row.0, name, "table row out of step with Instruction::NAMES");
    }
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f13_0014_0004);
    for _ in 0..3 {
        for op in 0..Instruction::NAMES.len() {
            let i = gen_op(&mut rng, op, 1);
            let v = serde_json::to_value(&i).expect("encode");
            let map = v.as_object().expect("object");
            let name = map["op"].as_str().expect("op name");
            let (_, required, optional, groups) = SHAPES
                .iter()
                .find(|row| row.0 == name)
                .unwrap_or_else(|| panic!("{name} missing from the table"));
            for key in map.keys() {
                let k = key.as_str();
                let is_group = groups.iter().any(|g| g.contains(&k));
                assert!(
                    k == "op" || required.contains(&k) || optional.contains(&k) || is_group,
                    "{name}: member {k} is not in the table"
                );
            }
            for key in map.keys() {
                let k = key.as_str();
                let mut m = map.clone();
                m.remove(key);
                let edited = Value::Object(m.clone());
                let result = serde_json::from_value::<Instruction>(edited.clone());
                let arm_removed = groups
                    .iter()
                    .any(|g| g.contains(&k) && g.iter().all(|a| *a == k || !map.contains_key(*a)));
                if k == "op" || required.contains(&k) || arm_removed {
                    assert!(result.is_err(), "{name}: removing {k} was accepted");
                } else {
                    assert!(
                        optional.contains(&k),
                        "{name}: {k} is neither required nor optional"
                    );
                    let other = result
                        .unwrap_or_else(|e| panic!("{name}: removing optional {k} failed: {e}"));
                    let bytes = crate::canonical::to_canonical_vec(&other).expect("encode");
                    let back: Value = serde_json::from_slice(&bytes).expect("json");
                    assert_eq!(
                        back, edited,
                        "{name}: {k} removal did not re-encode as edited"
                    );
                }
                m.insert(key.clone(), Value::Null);
                assert!(
                    serde_json::from_value::<Instruction>(Value::Object(m)).is_err(),
                    "{key} null accepted"
                );
            }
            let mut extra = map.clone();
            extra.insert("zzExtra".to_owned(), json!(1));
            assert!(serde_json::from_value::<Instruction>(Value::Object(extra)).is_err());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Never panic (R12)

/// Schema words that make random input reach deeper decoder paths.
fn effect_doc(rng: &mut Pcg64Dxsm) -> Vec<u8> {
    const PIECES: [&str; 24] = [
        "{\"op\":",
        "\"REPEAT_WHILE\"",
        "\"NATIVE_EFFECT\"",
        "\"CREATE_TOKEN\"",
        "\"CHOOSE_MODE\"",
        "\"maxIter\":",
        "\"body\":[",
        "\"params\":{",
        "\"layers\":[\"6\"",
        "\"duration\":{\"kind\":\"UNTIL\"",
        "\"tokenDef\":\"t\"",
        "\"inline\":{}",
        "\"objects\":{\"ref\":\"SELF\"}",
        "\"player\":{\"ref\":\"CONTROLLER\"}",
        "\"count\":{\"LITERAL\":1}",
        "\"cond\":{\"pred\":\"IS_LAND\"}",
        "\"id\":\"n\",\"version\":1",
        "\"modes\":[{\"id\":\"a\",\"effect\":[",
        "\"captures\":{\"$x\":",
        "4294967295",
        "}",
        "]",
        ",",
        "0",
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

/// Runs every decoder of the module on `doc`.
fn decode_all(doc: &[u8]) {
    decode_never_panics::<Instruction>(doc);
    decode_never_panics::<DurationAST>(doc);
    decode_never_panics::<NativeEffectAST>(doc);
    decode_never_panics::<InstructionMode>(doc);
    decode_never_panics::<TokenSource>(doc);
    decode_never_panics::<RememberSource>(doc);
    decode_never_panics::<DesignationValue>(doc);
    decode_never_panics::<Position>(doc);
    decode_never_panics::<TapAction>(doc);
    decode_never_panics::<MemoryStore>(doc);
}

/// REQ-NFR-006, R12: bounded random bytes (3,000 generic and 3,000 schema-biased inputs) never
/// panic any decoder; whatever decodes re-encodes to itself.
#[test]
fn req_nfr_006_random_bytes_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f13_0014_0005);
    for _ in 0..3000 {
        decode_all(&random_doc(&mut rng));
        decode_all(&effect_doc(&mut rng));
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

/// REQ-NFR-006, R12: byte-mutated valid documents of every op never panic.
#[test]
fn req_nfr_006_mutated_docs_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f13_0014_0006);
    for round in 0..8 {
        for op in 0..Instruction::NAMES.len() {
            let i = gen_op(&mut rng, op, round % 3);
            mutated::<Instruction>(&i, 4, &mut rng);
        }
        for _ in 0..40 {
            mutated::<DurationAST>(&gen_duration(&mut rng), 4, &mut rng);
            mutated::<NativeEffectAST>(&gen_native(&mut rng), 4, &mut rng);
        }
    }
}
