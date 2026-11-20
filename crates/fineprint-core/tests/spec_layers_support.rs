// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared synthetic environment and helpers for the `fineprint_core::layers` spec tests
//! (REQ-LAYER-001, REQ-CORE-002). Black-box: written from the work-item brief only.
//!
//! Each `spec_layers_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. Every card here is synthetic and neutrally named (R5).
//!
//! Representation of the DSL placeholders (the `Characteristics` fields wrap a JSON value):
//! a literal power or toughness `ValueExpressionAST::Literal(n)` is the DSL JSON form of the
//! typed `fineprint_dsl::expr::ValueExpressionAST::Literal(n)` (`{"LITERAL": n}`), and a keyword
//! ability `AbilityAST::Keyword` is the DSL JSON form of a keyword instance
//! (`{"kind": "KEYWORD", "abilityId": id, "keyword": kw}`). Both are built and read here through
//! MessagePack, so no `serde_json` dependency is needed.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use fineprint_base::ids::{EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, Evaluated, LayerEnvironment,
    LayerModification, MatchContext, ObjectBase,
};
use fineprint_core::objects::Characteristics;
pub use fineprint_dsl::ability::Layer;
pub use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::{ObjectSelectorAST, ReferenceAST};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn eid(n: u64) -> EffectId {
    EffectId::new(n)
}

pub fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

pub fn tgen() -> IdGenerator<Timestamp> {
    IdGenerator::new()
}

// ---------------------------------------------------------------------------------------------
// DSL placeholder values
// ---------------------------------------------------------------------------------------------

/// Round-trips a serialisable value through MessagePack (maps keep their keys).
pub fn via_msgpack<In: serde::Serialize, Out: serde::de::DeserializeOwned>(value: &In) -> Out {
    let bytes = rmp_serde::to_vec_named(value).expect("encode");
    rmp_serde::from_slice(&bytes).expect("decode")
}

fn try_via_msgpack<In: serde::Serialize, Out: serde::de::DeserializeOwned>(
    value: &In,
) -> Option<Out> {
    let bytes = rmp_serde::to_vec_named(value).ok()?;
    rmp_serde::from_slice(&bytes).ok()
}

/// `ValueExpressionAST::Literal(n)` in the placeholder form held by `Characteristics`.
pub fn lit(n: i64) -> ValueExpressionAST {
    via_msgpack(&fineprint_dsl::expr::ValueExpressionAST::Literal(n))
}

/// A non-literal value expression (`"X"`), used as a CDA-like base power.
pub fn non_literal_x() -> ValueExpressionAST {
    via_msgpack(&fineprint_dsl::expr::ValueExpressionAST::X)
}

/// A second non-literal value expression, distinct from [`non_literal_x`].
pub fn non_literal_memory(key: &str) -> ValueExpressionAST {
    via_msgpack(&fineprint_dsl::expr::ValueExpressionAST::Memory(
        key.to_string(),
    ))
}

/// Reads a placeholder value expression in the DSL form of `Literal(n)` (`{"LITERAL": n}`).
pub fn dsl_literal_of(v: &ValueExpressionAST) -> Option<i64> {
    match try_via_msgpack::<_, fineprint_dsl::expr::ValueExpressionAST>(v)? {
        fineprint_dsl::expr::ValueExpressionAST::Literal(n) => Some(n),
        _ => None,
    }
}

/// A literal as a bare JSON integer `n`: the placeholder form of `ValueExpressionAST::Literal(n)`
/// fixed by decision "#32 FP-031 — Placeholder DSL trees in `Characteristics`" (the evaluator
/// emits it and reads both it and `{"LITERAL": n}`).
pub fn bare(n: i64) -> ValueExpressionAST {
    via_msgpack(&n)
}

/// Reads a placeholder value expression as a literal integer: the DSL form `{"LITERAL": n}`
/// or a bare JSON integer (both are literals per the decision above;
/// `evaluated_pt_uses_the_logged_literal_form` asserts the emitted form on its own).
pub fn literal_of(v: &ValueExpressionAST) -> Option<i64> {
    dsl_literal_of(v).or_else(|| try_via_msgpack::<_, i64>(v))
}

/// `c` with every literal power/toughness (either form) rewritten to [`lit`], so whole
/// characteristics can be compared independently of the literal representation.
pub fn norm(c: &Characteristics) -> Characteristics {
    let fix = |v: &Option<ValueExpressionAST>| {
        v.as_ref()
            .map(|x| literal_of(x).map_or_else(|| x.clone(), lit))
    };
    Characteristics {
        power: fix(&c.power),
        toughness: fix(&c.toughness),
        ..c.clone()
    }
}

/// A keyword ability `AbilityAST::Keyword { base.ability_id = id, keyword = kw }`.
pub fn kw_ability(id: &str, kw: &str) -> AbilityAST {
    let mut m: BTreeMap<&str, &str> = BTreeMap::new();
    m.insert("kind", "KEYWORD");
    m.insert("abilityId", id);
    m.insert("keyword", kw);
    via_msgpack(&m)
}

/// A non-keyword (static) ability with the given id; its body is opaque here.
pub fn static_ability(id: &str) -> AbilityAST {
    let mut m: BTreeMap<&str, &str> = BTreeMap::new();
    m.insert("kind", "STATIC");
    m.insert("abilityId", id);
    via_msgpack(&m)
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct AbilityProbe {
    kind: Option<String>,
    #[serde(rename = "abilityId")]
    ability_id: Option<String>,
    keyword: Option<String>,
}

/// `(ability_id, keyword)` if `a` is a keyword ability.
pub fn keyword_of(a: &AbilityAST) -> Option<(String, String)> {
    let p: AbilityProbe = try_via_msgpack(a)?;
    if p.kind.as_deref() != Some("KEYWORD") {
        return None;
    }
    Some((p.ability_id?, p.keyword?))
}

/// Every keyword ability of `c`, as `(ability_id, keyword)`, in list order.
pub fn keywords(c: &Characteristics) -> Vec<(String, String)> {
    c.abilities.iter().filter_map(keyword_of).collect()
}

/// How many keyword abilities of `c` have keyword `kw`.
pub fn count_kw(c: &Characteristics, kw: &str) -> usize {
    keywords(c).iter().filter(|(_, k)| k == kw).count()
}

pub fn has_kw(c: &Characteristics, kw: &str) -> bool {
    count_kw(c, kw) > 0
}

/// The evaluated power and toughness as numbers. Panics if a present value is not
/// `ValueExpressionAST::Literal` (the brief: after layer 7a they are literals).
pub fn pt(e: &Evaluated) -> (Option<i64>, Option<i64>) {
    let read = |v: &Option<ValueExpressionAST>, what: &str| {
        v.as_ref().map(|x| {
            literal_of(x).unwrap_or_else(|| {
                panic!("evaluated {what} is not ValueExpressionAST::Literal: {x:?}")
            })
        })
    };
    (
        read(&e.characteristics.power, "power"),
        read(&e.characteristics.toughness, "toughness"),
    )
}

/// `(Some(p), Some(t))` shorthand.
pub fn spt(p: i64, t: i64) -> (Option<i64>, Option<i64>) {
    (Some(p), Some(t))
}

pub fn text(words: &[&str]) -> TextTokenStream {
    TextTokenStream(words.iter().map(|w| (*w).into()).collect())
}

pub fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

pub fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("valid mask")
}

// ---------------------------------------------------------------------------------------------
// synthetic characteristics
// ---------------------------------------------------------------------------------------------

/// An object with the given name and card types and nothing else.
pub fn blank(name: &str, types: &[&str]) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: set(types),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A synthetic creature with literal power and toughness.
pub fn creature(name: &str, p: i64, t: i64) -> Characteristics {
    Characteristics {
        power: Some(lit(p)),
        toughness: Some(lit(t)),
        ..blank(name, &["Creature"])
    }
}

/// A synthetic noncreature enchantment with one static ability.
pub fn enchantment(name: &str) -> Characteristics {
    Characteristics {
        abilities: vec![static_ability(&format!("{name}.static"))],
        ..blank(name, &["Enchantment"])
    }
}

/// A synthetic land with one static ability.
pub fn land(name: &str) -> Characteristics {
    Characteristics {
        abilities: vec![static_ability(&format!("{name}.mana"))],
        ..blank(name, &["Land"])
    }
}

/// A synthetic noncreature artifact.
pub fn artifact(name: &str) -> Characteristics {
    blank(name, &["Artifact"])
}

pub fn with_abilities(mut c: Characteristics, abilities: Vec<AbilityAST>) -> Characteristics {
    c.abilities = abilities;
    c
}

pub fn with_colors(mut c: Characteristics, bits: u8) -> Characteristics {
    c.colors = mask(bits);
    c
}

// ---------------------------------------------------------------------------------------------
// selectors
// ---------------------------------------------------------------------------------------------

/// An opaque selector key: `ObjectSelectorAST::Ref(ReferenceAST::Target { slot })`.
pub fn sel(slot: u32) -> ObjectSelectorAST {
    ObjectSelectorAST::Ref(ReferenceAST::Target { slot })
}

pub const SEL_CREATURE: u32 = 1;
pub const SEL_WHITE_CREATURE: u32 = 2;
pub const SEL_NONCREATURE_ENCHANTMENT: u32 = 3;
pub const SEL_ENCHANTMENT_CREATURE: u32 = 4;
pub const SEL_CONTROLLED_BY_P2: u32 = 5;
pub const SEL_HAS_FLYING: u32 = 6;
pub const SEL_ALL: u32 = 7;
pub const SEL_NONE: u32 = 8;
pub const SEL_ENCHANTMENT: u32 = 9;

pub type Matcher = Rc<dyn Fn(&MatchContext<'_>) -> bool>;

fn is_type(ctx: &MatchContext<'_>, t: &str) -> bool {
    ctx.candidate_view.characteristics.card_types.contains(t)
}

/// The standard selectors of [`TestEnv::new`].
pub fn standard_selectors() -> Vec<(ObjectSelectorAST, Matcher)> {
    let v: Vec<(u32, Matcher)> = vec![
        (SEL_CREATURE, Rc::new(|c| is_type(c, "Creature"))),
        (
            SEL_WHITE_CREATURE,
            Rc::new(|c| {
                is_type(c, "Creature") && c.candidate_view.characteristics.colors.bits() & 1 == 1
            }),
        ),
        (
            SEL_NONCREATURE_ENCHANTMENT,
            Rc::new(|c| is_type(c, "Enchantment") && !is_type(c, "Creature")),
        ),
        (
            SEL_ENCHANTMENT_CREATURE,
            Rc::new(|c| is_type(c, "Enchantment") && is_type(c, "Creature")),
        ),
        (
            SEL_CONTROLLED_BY_P2,
            Rc::new(|c| c.candidate_view.controller == Some(P2)),
        ),
        (
            SEL_HAS_FLYING,
            Rc::new(|c| has_kw(&c.candidate_view.characteristics, "flying")),
        ),
        (SEL_ALL, Rc::new(|_| true)),
        (SEL_NONE, Rc::new(|_| false)),
        (SEL_ENCHANTMENT, Rc::new(|c| is_type(c, "Enchantment"))),
    ];
    v.into_iter().map(|(s, m)| (sel(s), m)).collect()
}

// ---------------------------------------------------------------------------------------------
// the environment
// ---------------------------------------------------------------------------------------------

/// The base of one object as the test stores it (`ObjectBase` is rebuilt on demand).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseSpec {
    pub characteristics: Characteristics,
    pub controller: Option<PlayerId>,
    pub on_battlefield: bool,
}

/// A synthetic `LayerEnvironment` over ordered maps.
#[derive(Clone)]
pub struct TestEnv {
    pub bases: BTreeMap<ObjectId, BaseSpec>,
    pub counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    pub counter_ts: BTreeMap<(ObjectId, String), Timestamp>,
    pub attached: BTreeMap<ObjectId, Vec<ObjectId>>,
    pub selectors: Vec<(ObjectSelectorAST, Matcher)>,
    /// `eval_value` overrides: (expression, object) -> value.
    pub values: Vec<(ValueExpressionAST, ObjectId, Option<i64>)>,
}

impl Default for TestEnv {
    fn default() -> TestEnv {
        TestEnv::new()
    }
}

impl TestEnv {
    pub fn new() -> TestEnv {
        TestEnv {
            bases: BTreeMap::new(),
            counters: BTreeMap::new(),
            counter_ts: BTreeMap::new(),
            attached: BTreeMap::new(),
            selectors: standard_selectors(),
            values: Vec::new(),
        }
    }

    /// Adds a battlefield permanent controlled by P1.
    pub fn put(&mut self, id: ObjectId, c: Characteristics) -> ObjectId {
        self.put_full(id, c, Some(P1), true)
    }

    pub fn put_full(
        &mut self,
        id: ObjectId,
        c: Characteristics,
        controller: Option<PlayerId>,
        on_battlefield: bool,
    ) -> ObjectId {
        self.bases.insert(
            id,
            BaseSpec {
                characteristics: c,
                controller,
                on_battlefield,
            },
        );
        id
    }

    /// Sets `amount` counters of `kind` on `id` with counter timestamp `t`.
    pub fn counter(&mut self, id: ObjectId, kind: &str, amount: i64, t: u64) {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_string(), amount);
        self.counter_ts.insert((id, kind.to_string()), ts(t));
    }

    /// Sets `amount` counters of `kind` on `id` without a counter timestamp (any earlier
    /// timestamp for `(id, kind)` is dropped, so the timestamp-zero fallback applies).
    pub fn counter_no_ts(&mut self, id: ObjectId, kind: &str, amount: i64) {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_string(), amount);
        self.counter_ts.remove(&(id, kind.to_string()));
    }

    pub fn add_selector(&mut self, s: ObjectSelectorAST, m: Matcher) {
        self.selectors.push((s, m));
    }

    pub fn base_chars(&self, id: ObjectId) -> &Characteristics {
        &self.bases[&id].characteristics
    }
}

impl LayerEnvironment for TestEnv {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).map(|b| ObjectBase {
            characteristics: b.characteristics.clone(),
            controller: b.controller,
            on_battlefield: b.on_battlefield,
        })
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.counter_ts.get(&(id, kind.to_string())).copied()
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.attached.get(&id).cloned().unwrap_or_default()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        self.selectors
            .iter()
            .find(|(s, _)| s == selector)
            .is_some_and(|(_, m)| m(ctx))
    }

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        if let Some((_, _, v)) = self
            .values
            .iter()
            .find(|(e, o, _)| e == expr && *o == object)
        {
            return *v;
        }
        literal_of(expr)
    }
}

// ---------------------------------------------------------------------------------------------
// effects
// ---------------------------------------------------------------------------------------------

/// A resolved effect controlled by P1 with no source.
pub fn eff(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: P1,
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected,
        modification: m,
    }
}

/// A static characteristic-defining ability (CR 613.4a, 604.3): a static effect with
/// `is_cda: true`, no source, intended for layer 7a with a `SetPt` modification.
pub fn cda_eff(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: P1,
        timestamp: ts(t),
        layer,
        is_cda: true,
        origin: EffectOrigin::Static { granted_at: None },
        affected,
        modification: m,
    }
}

/// A static effect of `source` (printed when `granted_at` is `None`).
pub fn static_eff(
    source: ObjectId,
    granted_at: Option<u64>,
    layer: Layer,
    t: u64,
    affected: Affected,
    m: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(source),
        controller: P1,
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Static {
            granted_at: granted_at.map(ts),
        },
        affected,
        modification: m,
    }
}

/// A locked set `Affected::Objects`.
pub fn objs(ids: &[ObjectId]) -> Affected {
    Affected::Objects(ids.iter().copied().collect())
}

/// `Affected::Selector(sel(slot))`.
pub fn by_sel(slot: u32) -> Affected {
    Affected::Selector(sel(slot))
}

pub fn set_pt(p: Option<i64>, t: Option<i64>) -> LayerModification {
    LayerModification::SetPt {
        power: p,
        toughness: t,
    }
}

pub fn modify_pt(p: i64, t: i64) -> LayerModification {
    LayerModification::ModifyPt {
        power: p,
        toughness: t,
    }
}

pub fn add_card_types(types: &[&str]) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set(types),
        subtypes: BTreeSet::new(),
    }
}

/// A registry with the given (effect id, effect) pairs, registered in slice order.
pub fn registry(effects: &[(u64, ContinuousEffect)]) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, e) in effects {
        r.register(eid(*id), e.clone()).expect("register succeeds");
    }
    r
}

/// Every permutation of `0..n` (Heap's algorithm; n is small in tests).
pub fn permutations(n: usize) -> Vec<Vec<usize>> {
    fn heap(k: usize, a: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if k <= 1 {
            out.push(a.clone());
            return;
        }
        for i in 0..k {
            heap(k - 1, a, out);
            if k.is_multiple_of(2) {
                a.swap(i, k - 1);
            } else {
                a.swap(0, k - 1);
            }
        }
    }
    let mut a: Vec<usize> = (0..n).collect();
    let mut out = Vec::new();
    heap(n, &mut a, &mut out);
    out
}

// ---------------------------------------------------------------------------------------------
// deterministic pseudo-random numbers (no `rand`, R3)
// ---------------------------------------------------------------------------------------------

/// A 64-bit linear congruential generator (Knuth MMIX constants).
#[derive(Clone, Debug)]
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A value in `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = usize::try_from(self.below(i as u64 + 1)).expect("fits");
            v.swap(i, j);
        }
    }
}
