// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared fixtures for the REQ-LAYER-001 cross-object spec tests (#186: layer-major evaluation
//! across objects, so a characteristic-defining value expression sees the current state of every
//! object; CR 604.3, 613.1b, 613.3, 613.4a, 613.5, 613.6, 613.8a, 611.3, 113.6). Black-box:
//! written from the work-item brief and the public API only. Synthetic objects only (R5).
//!
//! The test environment interprets one non-literal base power/toughness expression,
//! `{"TEST_COUNT_CREATURES_YOU_CONTROL": 0}`: the number of objects `Y` of `view.objects()` whose
//! view controller equals the view controller of the object asked for and whose view card types
//! contain `Creature` (the controller is read from the view, never from the base). It records
//! every `eval_value`, `eval_value_with_view` and `generating_ability_exists` call.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, Evaluated, EvaluatedView,
    LayerCache, LayerEnvironment, LayerModification, MatchContext, ObjectBase, evaluate,
    evaluate_all, evaluate_with_order, literal, literal_of,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::ability::{AbilityBase, KeywordInstanceAST, Layer};
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
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

pub const P1: u64 = 1;
pub const P2: u64 = 2;

/// The key of the counting expression the test environment understands.
pub const COUNT_KEY: &str = "TEST_COUNT_CREATURES_YOU_CONTROL";

// ---------------------------------------------------------------------------------------------
// Objects
// ---------------------------------------------------------------------------------------------

/// The non-literal CDA expression "number of creatures your controller controls" (CR 604.3).
/// #232 FP-164: a valid typed expression the test environment recognises, `{"MEMORY": COUNT_KEY}`
/// (formerly the free-form placeholder `{COUNT_KEY: 0}`).
pub fn count_expr() -> ValueExpressionAST {
    ValueExpressionAST::Memory(COUNT_KEY.to_owned())
}

/// A synthetic colorless creature with literal P/T, no subtypes and no abilities.
pub fn creature(power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: set(["test.cross_object_creature"]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
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

/// A synthetic creature whose power and toughness are both the counting CDA expression.
pub fn cda_counter() -> Characteristics {
    let mut c = creature(0, 0);
    c.names = set(["test.cda_counter"]);
    c.power = Some(count_expr());
    c.toughness = Some(count_expr());
    c
}

/// A synthetic colorless noncreature of `card_type`, without P/T, subtypes or abilities.
pub fn noncreature(card_type: &str) -> Characteristics {
    let mut c = creature(0, 0);
    c.names = set(["test.cross_object_noncreature"]);
    c.card_types = set([card_type]);
    c.power = None;
    c.toughness = None;
    c
}

/// The DSL keyword name of a synthetic keyword string: upper case, every character outside
/// `[A-Z0-9]` as `_` (#232 FP-164: a typed KEYWORD ability holds `[A-Z][A-Z0-9_]*`).
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

/// A synthetic ability `{"kind":"KEYWORD","abilityId":id,"keyword":kw_name(kw)}`.
pub fn keyword_ability(kw: &str, id: &str) -> AbilityAST {
    AbilityAST::Keyword(KeywordInstanceAST::new(AbilityBase::new(id), kw_name(kw)))
}

/// The `abilityId` of every ability, in order.
pub fn ability_ids(c: &Characteristics) -> Vec<String> {
    c.abilities
        .iter()
        .map(|a| a.base().ability_id.clone())
        .collect()
}

/// Rules text of one LITERAL token holding `word` (#232 FP-164: formerly the bare string).
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

/// The literal P/T of an evaluated object (panics if not literal).
pub fn pt_of(ev: &Evaluated) -> (i64, i64) {
    pt(&ev.characteristics).expect("evaluated P/T is literal")
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

pub fn not(q: PredicateAST) -> PredicateAST {
    PredicateAST::Not(Box::new(q))
}

/// The predicate "has no abilities" is expressed by this synthetic keyword test: the
/// environment's `HasKeyword` with this reserved keyword holds iff the object has no ability.
pub const NO_ABILITIES_KW: &str = "test.reserved_no_abilities";

pub fn has_no_abilities() -> PredicateAST {
    PredicateAST::HasKeyword {
        keyword: NO_ABILITIES_KW.to_owned(),
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
        PredicateAST::HasKeyword { keyword } if keyword == NO_ABILITIES_KW => {
            c.abilities.is_empty()
        }
        PredicateAST::HasKeyword { keyword } => c
            .abilities
            .iter()
            .any(|a| matches!(a, AbilityAST::Keyword(k) if k.keyword == kw_name(keyword))),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------------------------

pub fn objs(ids: &[ObjectId]) -> Affected {
    Affected::Objects(ids.iter().copied().collect())
}

pub fn sel(pred: PredicateAST) -> Affected {
    Affected::Selector(filter(pred))
}

/// A resolved, non-CDA effect controlled by player 1 with no source.
pub fn eff(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: p(P1),
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected,
        modification: m,
    }
}

/// A non-CDA static effect of `source`'s printed ability (CR 611.3).
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

pub fn set_controller(n: u64) -> LayerModification {
    LayerModification::SetController(p(n))
}

pub fn add_card_type(t: &str) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set([t]),
        subtypes: BTreeSet::new(),
    }
}

pub fn set_card_types(t: &str) -> LayerModification {
    LayerModification::SetTypes {
        supertypes: None,
        card_types: Some(set([t])),
        subtypes: None,
    }
}

pub fn add_ability(id: &str) -> LayerModification {
    LayerModification::AddAbility(keyword_ability(id, id))
}

pub fn modify_pt(power: i64, toughness: i64) -> LayerModification {
    LayerModification::ModifyPt { power, toughness }
}

/// A registry of `(id, effect)`, registered in the given order.
pub fn registry(list: &[(u64, ContinuousEffect)]) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, effect) in list {
        r.register(e(*id), effect.clone()).unwrap();
    }
    r
}

/// The same registry registered in reverse order.
pub fn registry_rev(list: &[(u64, ContinuousEffect)]) -> ContinuousEffects {
    let mut v = list.to_vec();
    v.reverse();
    registry(&v)
}

// ---------------------------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------------------------

/// How the environment answers `generating_ability_exists`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HookMode {
    /// Always true (existence not modelled); calls are still recorded.
    #[default]
    AlwaysTrue,
    /// Only from `source_view`: false if `None`; true iff some ability of the view has the
    /// `abilityId` declared in [`Env::generated_by`] for the effect (CR 613.8a, 611.3, 113.6).
    FromView,
}

/// How the environment answers `eval_value_with_view` for a non-literal expression.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ValueMode {
    /// Interprets [`COUNT_KEY`]; anything else is `None`.
    #[default]
    Count,
    /// Answers the given value for any expression.
    Fixed(Option<i64>),
}

/// What the test records of an object in a view.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Snap {
    pub controller: Option<PlayerId>,
    pub card_types: BTreeSet<String>,
    pub colors: u8,
    pub abilities: Vec<String>,
}

impl Snap {
    pub fn of(ev: &Evaluated) -> Snap {
        Snap {
            controller: ev.controller,
            card_types: ev.characteristics.card_types.clone(),
            colors: ev.characteristics.colors.bits(),
            abilities: ability_ids(&ev.characteristics),
        }
    }
}

/// One `eval_value_with_view` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewCall {
    pub object: ObjectId,
    pub layer: Layer,
    pub objects: Vec<ObjectId>,
    /// `view.get(Y)` for every `Y` of `view.objects()` (`None` if absent).
    pub snaps: BTreeMap<ObjectId, Option<Snap>>,
}

/// One `generating_ability_exists` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookCall {
    pub effect: EffectId,
    pub source: ObjectId,
    pub view: Option<Snap>,
}

/// A synthetic environment: bases, an optional explicit `objects()` list, counters; ordered
/// containers only (R3).
#[derive(Default, Clone)]
pub struct Env {
    pub bases: BTreeMap<ObjectId, ObjectBase>,
    /// What `objects()` returns; `None` means the ascending base keys.
    pub listed: Option<Vec<ObjectId>>,
    pub counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    pub counter_ts: BTreeMap<(ObjectId, String), Timestamp>,
    pub hook_mode: HookMode,
    /// Effect -> `abilityId` of its generating ability ([`HookMode::FromView`]).
    pub generated_by: BTreeMap<EffectId, String>,
    pub value_mode: ValueMode,
    /// Objects of every plain `eval_value` call.
    pub plain_calls: RefCell<Vec<ObjectId>>,
    pub view_calls: RefCell<Vec<ViewCall>>,
    pub hook_calls: RefCell<Vec<HookCall>>,
}

impl Env {
    pub fn new() -> Env {
        Env::default()
    }

    /// Adds object `id` with characteristics `c`, controller `controller`, on the battlefield.
    pub fn with(mut self, id: ObjectId, controller: u64, c: Characteristics) -> Env {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(p(controller)),
                on_battlefield: true,
            },
        );
        self
    }

    pub fn listing(mut self, list: Vec<ObjectId>) -> Env {
        self.listed = Some(list);
        self
    }

    pub fn counter(mut self, id: ObjectId, kind: &str, amount: i64, t: u64) -> Env {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_owned(), amount);
        self.counter_ts.insert((id, kind.to_owned()), ts(t));
        self
    }

    pub fn clear_records(&self) {
        self.plain_calls.borrow_mut().clear();
        self.view_calls.borrow_mut().clear();
        self.hook_calls.borrow_mut().clear();
    }

    /// Total value-expression calls (plain and with view).
    pub fn value_call_count(&self) -> usize {
        self.plain_calls.borrow().len() + self.view_calls.borrow().len()
    }

    /// A copy with fresh records and `objects()` returning `list`.
    pub fn relisted(&self, list: Vec<ObjectId>) -> Env {
        let mut c = self.clone();
        c.clear_records();
        c.listed = Some(list);
        c
    }

    /// The ascending ids of every base.
    pub fn base_ids(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }
}

impl LayerEnvironment for Env {
    fn objects(&self) -> Vec<ObjectId> {
        self.listed
            .clone()
            .unwrap_or_else(|| self.bases.keys().copied().collect())
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

    fn eval_value(&self, _expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        self.plain_calls.borrow_mut().push(object);
        None
    }

    fn eval_value_with_view(
        &self,
        expr: &ValueExpressionAST,
        object: ObjectId,
        view: &EvaluatedView<'_>,
    ) -> Option<i64> {
        let objects = view.objects();
        let snaps = objects
            .iter()
            .map(|y| (*y, view.get(*y).map(Snap::of)))
            .collect();
        self.view_calls.borrow_mut().push(ViewCall {
            object,
            layer: view.layer(),
            objects: objects.clone(),
            snaps,
        });
        match self.value_mode {
            ValueMode::Fixed(v) => v,
            ValueMode::Count => {
                if *expr != count_expr() {
                    return None;
                }
                let me = view.get(object)?.controller;
                let n = objects
                    .iter()
                    .filter_map(|y| view.get(*y))
                    .filter(|v| v.controller == me)
                    .filter(|v| v.characteristics.card_types.contains("Creature"))
                    .count();
                Some(i64::try_from(n).unwrap())
            }
        }
    }

    fn generating_ability_exists(
        &self,
        effect: EffectId,
        source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        self.hook_calls.borrow_mut().push(HookCall {
            effect,
            source,
            view: source_view.map(Snap::of),
        });
        match self.hook_mode {
            HookMode::AlwaysTrue => true,
            HookMode::FromView => match (source_view, self.generated_by.get(&effect)) {
                (Some(v), Some(id)) => ability_ids(&v.characteristics).contains(id),
                _ => false,
            },
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Entry-point equality (B-3) and order independence (B-4)
// ---------------------------------------------------------------------------------------------

/// Evaluates every id of `ids` through `evaluate`, `evaluate_with_order(..).0`,
/// `LayerCache::evaluate` and compares with `evaluate_all` and `LayerCache::evaluate_all`
/// (REQ-LAYER-001, REQ-LAYER-005). Returns the `evaluate_all` map.
pub fn all_entry_points(
    effects: &ContinuousEffects,
    env: &Env,
    ctx: &str,
) -> BTreeMap<ObjectId, Evaluated> {
    let all = evaluate_all(effects, env).unwrap_or_else(|err| panic!("{ctx}: {err:?}"));
    let mut cache = LayerCache::new();
    let cached_all = cache.evaluate_all(effects, env).unwrap();
    assert_eq!(
        cached_all, all,
        "{ctx}: LayerCache::evaluate_all == evaluate_all"
    );
    let mut cache2 = LayerCache::new();
    for id in env.objects() {
        let one = evaluate(effects, env, id).unwrap();
        assert_eq!(
            Some(&one),
            all.get(&id),
            "{ctx}: evaluate == evaluate_all[{id:?}]"
        );
        let (with_order, record) = evaluate_with_order(effects, env, id).unwrap();
        assert_eq!(record.object, id);
        assert_eq!(
            with_order, one,
            "{ctx}: evaluate_with_order.0 == evaluate {id:?}"
        );
        assert_eq!(
            cache2.evaluate(effects, env, id).unwrap(),
            one,
            "{ctx}: LayerCache::evaluate (miss) {id:?}"
        );
        assert_eq!(
            cache.evaluate(effects, env, id).unwrap(),
            one,
            "{ctx}: LayerCache::evaluate after evaluate_all {id:?}"
        );
    }
    // Every listed id is in the map and nothing else.
    let listed: BTreeSet<ObjectId> = env.objects().into_iter().collect();
    assert_eq!(
        all.keys().copied().collect::<BTreeSet<_>>(),
        listed,
        "{ctx}"
    );
    all
}

/// Checks B-3 and B-4 for `list` (registered in given and reverse order) and the `objects()`
/// list ascending, reversed, shuffled and duplicated; returns the canonical result.
pub fn check_invariant(
    list: &[(u64, ContinuousEffect)],
    env: &Env,
    ctx: &str,
) -> BTreeMap<ObjectId, Evaluated> {
    let asc = env.objects();
    let base_env = env.relisted(asc.clone());
    let canonical = all_entry_points(&registry(list), &base_env, ctx);
    let mut rng = Rng::new(0x186 ^ u64::try_from(list.len()).unwrap());
    let mut lists = vec![asc.clone()];
    let mut rev = asc.clone();
    rev.reverse();
    lists.push(rev);
    for _ in 0..2 {
        let mut s = asc.clone();
        rng.shuffle(&mut s);
        lists.push(s.clone());
        let mut d = s.clone();
        d.extend(s.iter().rev().copied());
        rng.shuffle(&mut d);
        lists.push(d);
    }
    for (i, l) in lists.into_iter().enumerate() {
        for (j, reg) in [registry(list), registry_rev(list)].iter().enumerate() {
            let env2 = env.relisted(l.clone());
            let got = all_entry_points(reg, &env2, &format!("{ctx} list#{i} reg#{j}"));
            assert_eq!(got, canonical, "{ctx}: list {l:?} registration #{j}");
        }
    }
    canonical
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

    /// In `0..n` (`n > 0`).
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
