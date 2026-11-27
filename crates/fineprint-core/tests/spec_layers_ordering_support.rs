// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared fixtures for the REQ-LAYER-002 spec tests (ordering within a layer: CDA-first,
//! dependency, dependency loops; CR 613.2, 613.3, 613.4, 613.7, 613.8). Black-box: written from
//! the work-item brief and SRS 6.2 only. Synthetic objects, abilities and selectors only.
//!
//! The test environment interprets selector predicates on the candidate as evaluated so far
//! (`MatchContext::candidate_view`): `IS_CREATURE`, `HAS_CARD_TYPE`, `HAS_SUBTYPE`, `HAS_COLOR`
//! (`red`, `blue`, `green`, `white`, `black`), `HAS_KEYWORD` (some ability whose `keyword` member
//! is the given string), and the boolean `AND`, `OR`, `NOT`. A filter with no predicate matches
//! every object. Anything else does not match.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, Evaluated, LayerEnvironment,
    LayerModification, MatchContext, ObjectBase, OrderItem, OrderRecord, OrderStep, Selection,
    evaluate, evaluate_with_order, literal, literal_of,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, SelectorFilter, WordRef};

// ---------------------------------------------------------------------------------------------
// Ids
// ---------------------------------------------------------------------------------------------

pub fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

pub fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

pub fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

pub fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

// ---------------------------------------------------------------------------------------------
// Objects
// ---------------------------------------------------------------------------------------------

/// The object every scenario evaluates.
pub const SUBJECT: u64 = 1;

/// A synthetic black creature with the given P/T, no subtypes and no abilities.
pub fn creature(power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: set(["test.ordering_subject"]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: BTreeSet::new(),
        card_types: set(["Creature"]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream::default(),
        abilities: Vec::new(),
        power: Some(literal(power)),
        toughness: Some(literal(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A synthetic colorless noncreature of `card_type`, without P/T, subtypes or abilities.
pub fn noncreature(card_type: &str) -> Characteristics {
    let mut c = creature(0, 0);
    c.names = set(["test.ordering_noncreature"]);
    c.colors = ColorBitmask::NONE;
    c.card_types = set([card_type]);
    c.power = None;
    c.toughness = None;
    c
}

/// The DSL keyword name of a synthetic keyword string: upper case, every character outside
/// `[A-Z0-9]` as `_` (#232 FP-164: a typed KEYWORD ability holds `[A-Z][A-Z0-9_]*`; a keyword
/// counter kind such as `first strike` becomes `FIRST_STRIKE`).
pub fn kw_name(kw: &str) -> String {
    let s: String = kw
        .chars()
        .map(|c| {
            let u = c.to_ascii_uppercase();
            if u.is_ascii_uppercase() || u.is_ascii_digit() {
                u
            } else {
                '_'
            }
        })
        .collect();
    if s.starts_with(|c: char| c.is_ascii_uppercase()) {
        s
    } else {
        format!("K{s}")
    }
}

/// A synthetic keyword ability: `{"kind":"KEYWORD","abilityId":id,"keyword":kw_name(kw)}`.
pub fn keyword_ability(kw: &str, id: &str) -> AbilityAST {
    AbilityAST::Keyword(fineprint_dsl::ability::KeywordInstanceAST::new(
        fineprint_dsl::ability::AbilityBase::new(id),
        kw_name(kw),
    ))
}

/// A synthetic rules text of one LITERAL token (#232 FP-164: formerly the bare string).
pub fn text(word: &str) -> TextTokenStream {
    TextTokenStream::new(vec![fineprint_dsl::text::TextToken::Literal {
        id: "w0".to_owned(),
        text: word.to_owned(),
    }])
    .expect("valid synthetic text")
}

/// The literal P/T of `c`, if both are literals.
pub fn pt(c: &Characteristics) -> Option<(i64, i64)> {
    Some((
        literal_of(c.power.as_ref()?)?,
        literal_of(c.toughness.as_ref()?)?,
    ))
}

/// Whether `c` has a KEYWORD ability whose keyword is `kw_name(kw)`.
pub fn has_keyword(c: &Characteristics, kw: &str) -> bool {
    c.abilities
        .iter()
        .any(|a| matches!(a, AbilityAST::Keyword(k) if k.keyword == kw_name(kw)))
}

// ---------------------------------------------------------------------------------------------
// Selectors
// ---------------------------------------------------------------------------------------------

pub fn filter(pred: PredicateAST) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    })
}

/// A selector matching every object.
pub fn any() -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: None,
    })
}

pub fn is_creature() -> PredicateAST {
    PredicateAST::IsCreature
}

pub fn has_card_type(t: &str) -> PredicateAST {
    PredicateAST::HasCardType {
        value: WordRef::Name(t.to_owned()),
    }
}

pub fn has_subtype(t: &str) -> PredicateAST {
    PredicateAST::HasSubtype {
        value: WordRef::Name(t.to_owned()),
    }
}

pub fn has_color(c: &str) -> PredicateAST {
    PredicateAST::HasColor {
        value: WordRef::Name(c.to_owned()),
    }
}

pub fn has_kw(kw: &str) -> PredicateAST {
    PredicateAST::HasKeyword {
        keyword: kw.to_owned(),
    }
}

pub fn not(q: PredicateAST) -> PredicateAST {
    PredicateAST::Not(Box::new(q))
}

pub fn and(qs: Vec<PredicateAST>) -> PredicateAST {
    PredicateAST::And(qs)
}

pub fn or(qs: Vec<PredicateAST>) -> PredicateAST {
    PredicateAST::Or(qs)
}

/// Exactly one of `a` and `b` holds.
pub fn xor(a: PredicateAST, b: PredicateAST) -> PredicateAST {
    or(vec![
        and(vec![a.clone(), not(b.clone())]),
        and(vec![b, not(a)]),
    ])
}

fn color_bit(color: &str) -> Option<u8> {
    match color {
        "white" => Some(ColorBitmask::WHITE.bits()),
        "blue" => Some(ColorBitmask::BLUE.bits()),
        "black" => Some(ColorBitmask::BLACK.bits()),
        "red" => Some(ColorBitmask::RED.bits()),
        "green" => Some(ColorBitmask::GREEN.bits()),
        _ => None,
    }
}

pub fn pred_holds(pred: &PredicateAST, view: &Evaluated) -> bool {
    let c = &view.characteristics;
    match pred {
        PredicateAST::And(all) => all.iter().all(|q| pred_holds(q, view)),
        PredicateAST::Or(all) => all.iter().any(|q| pred_holds(q, view)),
        PredicateAST::Not(q) => !pred_holds(q, view),
        PredicateAST::IsCreature => c.card_types.contains("Creature"),
        PredicateAST::HasCardType {
            value: WordRef::Name(t),
        } => c.card_types.contains(t),
        PredicateAST::HasSubtype {
            value: WordRef::Name(t),
        } => c.subtypes.contains(t),
        PredicateAST::HasColor {
            value: WordRef::Name(w),
        } => color_bit(w).is_some_and(|b| c.colors.bits() & b != 0),
        PredicateAST::HasKeyword { keyword } => has_keyword(c, keyword),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------------------------

/// The locked set `{SUBJECT}` (CR 611.2c style; applicability never changes).
pub fn subject() -> Affected {
    Affected::Objects([o(SUBJECT)].into_iter().collect())
}

/// A locked set that does not contain the subject.
pub fn elsewhere() -> Affected {
    Affected::Objects([o(99)].into_iter().collect())
}

pub fn sel(pred: PredicateAST) -> Affected {
    Affected::Selector(filter(pred))
}

/// A resolved, non-CDA effect controlled by player 1 with no source.
pub fn eff(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: p(1),
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected,
        modification: m,
    }
}

/// A non-CDA static effect of `source`'s ability (`EffectOrigin::Static { granted_at: None }`,
/// `source: Some(source)`), controlled by player 1. Whether its generating ability still exists
/// is answered by [`Env::generated_by`] (CR 613.8a, existence).
pub fn static_eff(
    layer: Layer,
    t: u64,
    source: ObjectId,
    affected: Affected,
    m: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(source),
        origin: EffectOrigin::Static { granted_at: None },
        ..eff(layer, t, affected, m)
    }
}

/// A characteristic-defining static effect of the subject's own ability (CR 604.3).
pub fn cda(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(o(SUBJECT)),
        is_cda: true,
        origin: EffectOrigin::Static { granted_at: None },
        ..eff(layer, t, affected, m)
    }
}

pub fn add_subtype(s: &str) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: BTreeSet::new(),
        subtypes: set([s]),
    }
}

pub fn add_card_type(t: &str) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set([t]),
        subtypes: BTreeSet::new(),
    }
}

pub fn set_subtypes(s: &str) -> LayerModification {
    LayerModification::SetTypes {
        supertypes: None,
        card_types: None,
        subtypes: Some(set([s])),
    }
}

pub fn set_pt(power: i64, toughness: i64) -> LayerModification {
    LayerModification::SetPt {
        power: Some(power),
        toughness: Some(toughness),
    }
}

pub fn modify_pt(power: i64, toughness: i64) -> LayerModification {
    LayerModification::ModifyPt { power, toughness }
}

pub fn add_ability(kw: &str) -> LayerModification {
    LayerModification::AddAbility(keyword_ability(kw, &format!("test.ability.{kw}")))
}

/// A registry of `(id, effect)`, registered in the given order.
pub fn registry(list: &[(u64, ContinuousEffect)]) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, effect) in list {
        r.register(e(*id), effect.clone()).unwrap();
    }
    r
}

// ---------------------------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------------------------

/// A synthetic environment: bases, counters and counter timestamps; ordered containers only.
#[derive(Default, Clone)]
pub struct Env {
    pub bases: BTreeMap<ObjectId, ObjectBase>,
    pub counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    pub counter_ts: BTreeMap<(ObjectId, String), Timestamp>,
    /// Effect -> keyword of the ability that generates it (CR 613.8a, existence).
    pub generated_by: BTreeMap<EffectId, String>,
    /// Every `generating_ability_exists` call: (effect, source, whether `source_view` was given).
    pub hook_calls: RefCell<Vec<(EffectId, ObjectId, bool)>>,
}

impl Env {
    /// An environment whose subject has characteristics `c`, controller player 1, on the
    /// battlefield.
    pub fn subject(c: Characteristics) -> Env {
        Env::default().with(o(SUBJECT), c)
    }

    pub fn with(mut self, id: ObjectId, c: Characteristics) -> Env {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(p(1)),
                on_battlefield: true,
            },
        );
        self
    }

    /// Puts `amount` counters of `kind` on the subject with counter timestamp `t` (CR 613.7c).
    pub fn counter(mut self, kind: &str, amount: i64, t: u64) -> Env {
        self.counters
            .entry(o(SUBJECT))
            .or_default()
            .insert(kind.to_owned(), amount);
        self.counter_ts.insert((o(SUBJECT), kind.to_owned()), ts(t));
        self
    }

    /// Declares that `effect` is generated by the source's ability with keyword `keyword`: the
    /// effect exists only while the source, as evaluated so far, has such an ability.
    pub fn generated_by(mut self, effect: EffectId, keyword: &str) -> Env {
        self.generated_by.insert(effect, keyword.to_owned());
        self
    }

    /// The effects the `generating_ability_exists` hook was asked about, in call order.
    pub fn hook_effects(&self) -> Vec<EffectId> {
        self.hook_calls.borrow().iter().map(|c| c.0).collect()
    }
}

impl LayerEnvironment for Env {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.counter_ts.get(&(id, kind.to_owned())).copied()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        match selector {
            ObjectSelectorAST::Filter(f) => f
                .predicate
                .as_ref()
                .is_none_or(|q| pred_holds(q, ctx.candidate_view)),
            ObjectSelectorAST::Ref(_) => false,
        }
    }

    /// True when no source view is given or the effect has no declared generating ability;
    /// otherwise true iff the source as evaluated so far has an ability with the declared keyword.
    fn generating_ability_exists(
        &self,
        effect: EffectId,
        source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        self.hook_calls
            .borrow_mut()
            .push((effect, source, source_view.is_some()));
        match (source_view, self.generated_by.get(&effect)) {
            (Some(view), Some(kw)) => has_keyword(&view.characteristics, kw),
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Running and records
// ---------------------------------------------------------------------------------------------

/// Evaluates the subject with its order record, checking the record names the subject and that
/// `evaluate` returns the same `Evaluated` (brief item 9, REQ-LAYER-002).
pub fn run(effects: &ContinuousEffects, env: &Env) -> (Evaluated, OrderRecord) {
    let (evaluated, record) = evaluate_with_order(effects, env, o(SUBJECT)).unwrap();
    assert_eq!(
        record.object,
        o(SUBJECT),
        "the record names the evaluated object"
    );
    assert_eq!(
        evaluate(effects, env, o(SUBJECT)).unwrap(),
        evaluated,
        "evaluate equals evaluate_with_order(..).0"
    );
    (evaluated, record)
}

/// The step of effect `id`.
pub fn fx(layer: Layer, id: u64, selection: Selection, applied: bool) -> OrderStep {
    OrderStep {
        layer,
        item: OrderItem::Effect(e(id)),
        selection,
        applied,
    }
}

/// The step of the counters of `kind`.
pub fn ctr(layer: Layer, kind: &str, selection: Selection, applied: bool) -> OrderStep {
    OrderStep {
        layer,
        item: OrderItem::Counter(kind.to_owned()),
        selection,
        applied,
    }
}

/// The steps of `record` in `layer`, in record order.
pub fn steps_in(record: &OrderRecord, layer: Layer) -> Vec<OrderStep> {
    record
        .steps
        .iter()
        .filter(|s| s.layer == layer)
        .cloned()
        .collect()
}

/// The items of `record` in `layer`, in record order.
pub fn items_in(record: &OrderRecord, layer: Layer) -> Vec<OrderItem> {
    steps_in(record, layer)
        .into_iter()
        .map(|s| s.item)
        .collect()
}

pub fn item(id: u64) -> OrderItem {
    OrderItem::Effect(e(id))
}

/// Asserts the record lists its layers in `Layer::ALL` order.
pub fn assert_layers_in_order(record: &OrderRecord) {
    let index = |l: Layer| Layer::ALL.iter().position(|&x| x == l).unwrap();
    for pair in record.steps.windows(2) {
        assert!(
            index(pair[0].layer) <= index(pair[1].layer),
            "layers out of order in {record:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Seeded generator (R3, R7: no clock)
// ---------------------------------------------------------------------------------------------

/// SplitMix64, seeded by the test.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish in `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let bound = u64::try_from(i + 1).unwrap();
            let j = usize::try_from(self.below(bound)).unwrap();
            items.swap(i, j);
        }
    }
}
