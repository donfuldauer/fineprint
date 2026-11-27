// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Equivalence and determinism spec tests for the layer-evaluation cost reduction (#190 FP-147;
//! REQ-LAYER-005, REQ-LAYER-001, REQ-CORE-001). Black-box: written from the work-item brief and
//! the public API of `fineprint_core::layers` only.
//!
//! The optimisation must not change any evaluated result. These tests pin the observable
//! behaviour on fixed boards:
//!
//! - golden `evaluated_digest` values and golden `OrderRecord` step hashes, **recorded at commit
//!   1360900** (the unmodified base, before the optimisation) and hard-coded below; the goldens
//!   are the only expectations taken from running code, everything else comes from the brief;
//! - equivalence of uncached `evaluate_all`, per-object `evaluate`, and `LayerCache` (cold, warm,
//!   after an explicit invalidation, after a registry change);
//! - the documented cache bookkeeping, in cached and full-recompute mode (`FULL_RECOMPUTE`);
//! - the documented universe and first-error rules (unsorted, duplicated and baseless ids, an
//!   asked object that is not listed) and order independence.
//!
//! Every board comes from a deterministic generator (no randomness, clock or environment; R3).
//! Every object, ability, subtype and keyword is synthetic and neutrally named (R5). No test
//! asserts time (R7). The file passes with and without `--features full-recompute`; only the
//! bookkeeping assertions branch on `FULL_RECOMPUTE`.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::sha256;
use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, Evaluated, EvaluatedView,
    FULL_RECOMPUTE, Invalidation, LayerCache, LayerEnvironment, LayerError, LayerModification,
    MatchContext, ObjectBase, evaluate, evaluate_all, evaluate_with_order, evaluated_digest,
    literal, literal_of,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, SelectorFilter, WordRef};

// =============================================================================================
// Goldens (recorded at commit 1360900, the unmodified base; identical in both modes)
// =============================================================================================
//
// #232 FP-164: the five `GOLDEN_DIGEST_*` values were re-recorded when the boards' content moved
// from invalid placeholders to valid typed ASTs (literal P/T from the bare JSON integer to
// `{"LITERAL": n}`, keyword abilities to upper-case keyword names, the counting CDA from
// `{COUNT_KEY: 0}` to `{"MEMORY": COUNT_KEY}`); the `GOLDEN_ORDER_*` values (application order)
// are unchanged. Old and new values: docs/DECISIONS.md "#232 FP-164 — Golden changes and replaced
// tests".

/// `evaluated_digest(evaluate_all)` of board a ("bench-like", 200 permanents).
const GOLDEN_DIGEST_A: &str = "a4d874274de7e5f515d13433bb364ea4ebeba558f511bc2f054fde5deffd7687";
/// `evaluated_digest(evaluate_all)` of board b ("foreign-source"), timestamp order 1.
const GOLDEN_DIGEST_B1: &str = "6f351ff80d9fc0880e18cc0fb3a100adecd8bc7c7d45244ff11bf097140711d3";
/// `evaluated_digest(evaluate_all)` of board b, timestamps swapped (order 2).
const GOLDEN_DIGEST_B2: &str = "c52b0fcf6fe49236ac0d9ae397fbb7d7249d6de0732b756a045c50fc8102e5ab";
/// `evaluated_digest(evaluate_all)` of board c ("CDA and view"), timestamp order 1.
const GOLDEN_DIGEST_C1: &str = "42b5e16f9cca6ee0c296bacab878e83ee6b220b9c95c67d5f13deaf67724e6aa";
/// `evaluated_digest(evaluate_all)` of board c, timestamps swapped (order 2).
const GOLDEN_DIGEST_C2: &str = "9c3ba17860422d9cb892bb758938b2fc491edcaf2c5b3770c2e93279f2261287";

/// SHA-256 (hex) over `"<id>:<Debug of OrderRecord::steps>\n"` for the sample objects of board a
/// (see [`Board::sample`]), in sample order.
const GOLDEN_ORDER_A: &str = "4917788dfadc516fab0d7e8b75f62fdfdf9576cfda7d2455572be911be165120";
/// The same for board b, order 1.
const GOLDEN_ORDER_B1: &str = "da09f1838063243332c3571c4a2149233626a2862a72d402bb81ae2f05d9fefe";
/// The same for board b, order 2.
const GOLDEN_ORDER_B2: &str = "1e4c0a94a283d6c837d564699b3727e988c7a518958931c95d5113049e58e04a";
/// The same for board c, order 1.
const GOLDEN_ORDER_C1: &str = "e51f4fe3e5e3f3f521179f6249f37130c7ee51699dcb3e011188ba8888a07080";
/// The same for board c, order 2.
const GOLDEN_ORDER_C2: &str = "ad35ea3c4787844b048e8862dc3000dedaa1d6ee4d52d200fa9140a3dc76186c";

// =============================================================================================
// Ids and synthetic objects
// =============================================================================================

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

fn words(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// The key of the one non-literal value expression the test environment interprets: the number
/// of objects of the view that have the asked object's (view) controller and are creatures.
const COUNT_KEY: &str = "TEST_COUNT_CREATURES_CONTROLLED";

/// The synthetic characteristic-defining expression "creatures its controller controls".
/// #232 FP-164: the valid typed expression `{"MEMORY": COUNT_KEY}` (formerly the free-form
/// placeholder `{COUNT_KEY: 0}`).
fn count_expr() -> ValueExpressionAST {
    ValueExpressionAST::Memory(COUNT_KEY.to_owned())
}

/// The DSL keyword name of a synthetic keyword word: upper case, every character outside
/// `[A-Z0-9]` as `_` (#232 FP-164: a typed KEYWORD holds `[A-Z][A-Z0-9_]*`).
fn kw_name(word: &str) -> String {
    let s: String = word
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

/// A synthetic keyword ability whose `abilityId` is the word and whose keyword is its DSL name.
fn kw(word: &str) -> AbilityAST {
    AbilityAST::Keyword(fineprint_dsl::ability::KeywordInstanceAST::new(
        fineprint_dsl::ability::AbilityBase::new(word),
        kw_name(word),
    ))
}

fn ability_ids(c: &Characteristics) -> Vec<String> {
    c.abilities
        .iter()
        .map(|a| a.base().ability_id.clone())
        .collect()
}

/// A synthetic object: card types, subtypes, keyword abilities and literal P/T (`None` for a
/// noncreature).
fn card(
    name: &str,
    types: &[&str],
    subtypes: &[&str],
    abilities: &[&str],
    pt: Option<(i64, i64)>,
) -> Characteristics {
    Characteristics {
        names: words(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: words(types),
        subtypes: words(subtypes),
        rules_text: TextTokenStream::default(),
        abilities: abilities.iter().map(|w| kw(w)).collect(),
        power: pt.map(|(pw, _)| literal(pw)),
        toughness: pt.map(|(_, t)| literal(t)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn creature(name: &str, subtypes: &[&str], abilities: &[&str], pt: (i64, i64)) -> Characteristics {
    card(name, &["Creature"], subtypes, abilities, Some(pt))
}

/// A synthetic creature whose power and toughness are both [`count_expr`] (CDA).
fn cda_creature(name: &str, subtypes: &[&str], abilities: &[&str]) -> Characteristics {
    let mut c = creature(name, subtypes, abilities, (0, 0));
    c.power = Some(count_expr());
    c.toughness = Some(count_expr());
    c
}

// =============================================================================================
// Selectors and effects
// =============================================================================================

fn selector(pred: PredicateAST) -> Affected {
    Affected::Selector(ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    }))
}

fn creatures() -> Affected {
    selector(PredicateAST::IsCreature)
}

fn of_type(t: &str) -> Affected {
    selector(PredicateAST::HasCardType {
        value: WordRef::Name(t.to_owned()),
    })
}

fn of_subtype(t: &str) -> Affected {
    selector(PredicateAST::HasSubtype {
        value: WordRef::Name(t.to_owned()),
    })
}

fn with_keyword(k: &str) -> Affected {
    selector(PredicateAST::HasKeyword {
        keyword: k.to_owned(),
    })
}

fn locked(ids: &[u64]) -> Affected {
    Affected::Objects(ids.iter().map(|n| o(*n)).collect())
}

fn holds(pred: &PredicateAST, view: &Evaluated) -> bool {
    let c = &view.characteristics;
    match pred {
        PredicateAST::And(all) => all.iter().all(|q| holds(q, view)),
        PredicateAST::Or(all) => all.iter().any(|q| holds(q, view)),
        PredicateAST::Not(q) => !holds(q, view),
        PredicateAST::IsCreature => c.card_types.contains("Creature"),
        PredicateAST::HasCardType {
            value: WordRef::Name(t),
        } => c.card_types.contains(t),
        PredicateAST::HasSubtype {
            value: WordRef::Name(t),
        } => c.subtypes.contains(t),
        PredicateAST::HasKeyword { keyword } => c
            .abilities
            .iter()
            .any(|a| matches!(a, AbilityAST::Keyword(k) if k.keyword == kw_name(keyword))),
        _ => false,
    }
}

/// A static (non-CDA) effect of `source`'s own ability, controlled by `controller`.
fn stat(
    source: u64,
    controller: u64,
    t: u64,
    layer: Layer,
    affected: Affected,
    m: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(o(source)),
        controller: p(controller),
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Static { granted_at: None },
        affected,
        modification: m,
    }
}

/// A resolved effect with no source, controlled by player 1.
fn resolved(t: u64, layer: Layer, affected: Affected, m: LayerModification) -> ContinuousEffect {
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

fn add_types(card_types: &[&str], subtypes: &[&str]) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: words(card_types),
        subtypes: words(subtypes),
    }
}

fn set_card_types(card_types: &[&str]) -> LayerModification {
    LayerModification::SetTypes {
        supertypes: None,
        card_types: Some(words(card_types)),
        subtypes: None,
    }
}

fn grant(word: &str) -> LayerModification {
    LayerModification::AddAbility(kw(word))
}

fn set_pt(power: i64, toughness: i64) -> LayerModification {
    LayerModification::SetPt {
        power: Some(power),
        toughness: Some(toughness),
    }
}

fn modify_pt(power: i64, toughness: i64) -> LayerModification {
    LayerModification::ModifyPt { power, toughness }
}

// =============================================================================================
// The environment
// =============================================================================================

/// A synthetic, pure `LayerEnvironment` over ordered containers (R3).
#[derive(Clone, Default)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
    /// What `objects()` returns; `None` means the ascending base keys.
    listed: Option<Vec<ObjectId>>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    counter_ts: BTreeMap<(ObjectId, String), Timestamp>,
    /// Effect -> the `abilityId` its source must still have (in the source view the engine
    /// passes to `generating_ability_exists`) for the effect to exist. Effects not listed exist.
    generated_by: BTreeMap<EffectId, String>,
}

impl Env {
    fn put(&mut self, id: u64, controller: u64, c: Characteristics) {
        self.bases.insert(
            o(id),
            ObjectBase {
                characteristics: c,
                controller: Some(p(controller)),
                on_battlefield: true,
            },
        );
    }

    fn counter(&mut self, id: u64, kind: &str, amount: i64, t: u64) {
        self.counters
            .entry(o(id))
            .or_default()
            .insert(kind.to_owned(), amount);
        self.counter_ts.insert((o(id), kind.to_owned()), ts(t));
    }

    fn relisted(&self, list: Vec<ObjectId>) -> Env {
        let mut c = self.clone();
        c.listed = Some(list);
        c
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
                .is_none_or(|q| holds(q, ctx.candidate_view)),
            ObjectSelectorAST::Ref(_) => false,
        }
    }

    fn eval_value(&self, expr: &ValueExpressionAST, _object: ObjectId) -> Option<i64> {
        literal_of(expr)
    }

    fn eval_value_with_view(
        &self,
        expr: &ValueExpressionAST,
        object: ObjectId,
        view: &EvaluatedView<'_>,
    ) -> Option<i64> {
        if *expr != count_expr() {
            return literal_of(expr);
        }
        let me = view.get(object)?.controller;
        let n = view
            .objects()
            .iter()
            .filter_map(|y| view.get(*y))
            .filter(|v| v.controller == me && v.characteristics.card_types.contains("Creature"))
            .count();
        i64::try_from(n).ok()
    }

    fn generating_ability_exists(
        &self,
        effect: EffectId,
        _source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        match self.generated_by.get(&effect) {
            None => true,
            Some(id) => source_view.is_some_and(|v| ability_ids(&v.characteristics).contains(id)),
        }
    }
}

/// A fixed board: the environment and the registered effects (ids `1..`, in list order).
#[derive(Clone)]
struct Board {
    /// A label for failure messages only (never branched on).
    label: &'static str,
    env: Env,
    effects: Vec<ContinuousEffect>,
    /// The golden `evaluated_digest` of `evaluate_all` (empty if the board has none).
    golden_digest: &'static str,
    /// The golden hash of the `OrderRecord` steps of [`Board::sample`] (empty if none).
    golden_order: &'static str,
    /// The objects checked one by one (`evaluate`, `evaluate_with_order`,
    /// `LayerCache::evaluate`) and whose `OrderRecord` steps are pinned; every object unless
    /// set otherwise.
    sample: Vec<ObjectId>,
    /// A creature whose counters requirement 4 (iv) changes and that the effect added for (v)
    /// affects.
    changed: u64,
    /// Whether a cold cache fill is costly (board a): filled as rarely as possible.
    large: bool,
}

impl Board {
    fn new(label: &'static str, env: Env, effects: Vec<ContinuousEffect>) -> Board {
        let sample = env.bases.keys().copied().collect();
        Board {
            label,
            env,
            effects,
            golden_digest: "",
            golden_order: "",
            sample,
            changed: 0,
            large: false,
        }
    }

    fn registry(&self) -> ContinuousEffects {
        registry_of(&self.effects)
    }

    fn ids(&self) -> Vec<ObjectId> {
        self.env.bases.keys().copied().collect()
    }
}

fn registry_of(effects: &[ContinuousEffect]) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, effect) in (1_u64..).zip(effects) {
        r.register(EffectId::new(id), effect.clone())
            .expect("valid synthetic effect");
    }
    r
}

// =============================================================================================
// Board a: "bench-like" (200 permanents, 4 players)
// =============================================================================================

const A_PERMANENTS: u64 = 200;
const A_PLAYERS: u64 = 4;
const A_SUBTYPES: [&str; 4] = ["TestKindA", "TestKindB", "TestKindC", "TestKindD"];

/// Permanent `i` (1-based) of board a: the first of each player is a creature with a
/// characteristic-defining P/T ([`count_expr`]); every tenth is an artifact and every tenth plus
/// five a land; the rest are creatures of their controller's subtype with a keyword.
fn board_a_permanent(i: u64) -> (u64, Characteristics) {
    let player = (i - 1) % A_PLAYERS;
    let sub = A_SUBTYPES[usize::try_from(player).expect("small")];
    let size = i64::try_from(i % 5).expect("small") + 1;
    let name = format!("test.bench_permanent_{i}");
    let mut c = if i <= A_PLAYERS {
        cda_creature(&name, &[sub], &["test.vigilant"])
    } else if i.is_multiple_of(10) {
        card(&name, &["Artifact"], &[], &[], None)
    } else if i.is_multiple_of(5) {
        card(&name, &["Land"], &[], &[], None)
    } else {
        creature(&name, &[sub], &["test.vigilant"], (size, size + 1))
    };
    c.colors = ColorBitmask::new(1 << player).unwrap_or(ColorBitmask::NONE);
    (player + 1, c)
}

fn board_a() -> Board {
    let mut env = Env::default();
    for i in 1..=A_PERMANENTS {
        let (controller, c) = board_a_permanent(i);
        env.put(i, controller, c);
    }
    // Counters: every 7th permanent has two P/T counters or (every 21st) a keyword counter;
    // every 11th has one -1/-1 counter.
    for i in (7..=A_PERMANENTS).step_by(7) {
        let kind = if i.is_multiple_of(3) {
            "flying"
        } else {
            "+1/+1"
        };
        env.counter(i, kind, 2, 1_000 + i);
    }
    for i in (11..=A_PERMANENTS).step_by(11) {
        env.counter(i, "-1/-1", 1, 2_000 + i);
    }
    let mut fx = Vec::new();
    // Per player: layer-4 subtype grant to every creature (its selector reads the card type the
    // layer-4 artifact animation adds, so it depends on it), a layer-6 keyword grant and a
    // layer-7c boost to that subtype.
    for (n, word) in (1_u64..).zip(A_SUBTYPES) {
        fx.push(stat(
            n,
            n,
            10 + n,
            Layer::L4,
            creatures(),
            add_types(&[], &[word]),
        ));
        fx.push(stat(
            n,
            n,
            20 + n,
            Layer::L6,
            of_subtype(word),
            grant("test.flier"),
        ));
        fx.push(stat(
            n,
            n,
            30 + n,
            Layer::L7c,
            of_subtype(word),
            modify_pt(1, 1),
        ));
    }
    // Layer 4 "artifacts are creatures" (later timestamp than the subtype grants).
    fx.push(stat(
        10,
        2,
        15,
        Layer::L4,
        of_type("Artifact"),
        add_types(&["Creature"], &[]),
    ));
    fx.push(stat(
        20,
        4,
        40,
        Layer::L5,
        creatures(),
        LayerModification::AddColors(ColorBitmask::WHITE),
    ));
    fx.push(stat(
        30,
        2,
        41,
        Layer::L7b,
        of_type("Artifact"),
        set_pt(2, 2),
    ));
    fx.push(stat(
        40,
        4,
        42,
        Layer::L6,
        with_keyword("flying"),
        grant("test.hexproofish"),
    ));
    // Resolved effects locked to fixed sets in layers 6, 7b and 7c.
    let layers = [Layer::L6, Layer::L7b, Layer::L7c];
    let mods = [
        LayerModification::RemoveAllAbilities,
        set_pt(0, 1),
        modify_pt(3, -1),
    ];
    for k in 0..6_u64 {
        let ids: Vec<u64> = (0..10)
            .map(|j| 1 + (k * 31 + j * 7) % A_PERMANENTS)
            .collect();
        let which = usize::try_from(k % 3).expect("small");
        fx.push(resolved(
            50 + k,
            layers[which],
            locked(&ids),
            mods[which].clone(),
        ));
    }
    Board {
        golden_digest: GOLDEN_DIGEST_A,
        golden_order: GOLDEN_ORDER_A,
        // A fixed spread (CDA, plain creatures, artifacts, lands, counters, locked sets): one
        // board pass per object is too slow on the base to check all 200 (docs/DECISIONS.md).
        sample: [1, 4, 5, 7, 10, 11, 14, 21, 32, 50, 77, 94, 154, 200]
            .iter()
            .map(|n| o(*n))
            .collect(),
        changed: 6,
        large: true,
        ..Board::new("a", env, fx)
    }
}

// =============================================================================================
// Board b: "foreign-source" (16 permanents, existence from a foreign source's view)
// =============================================================================================

/// Board b. `swapped` reverses every effect timestamp (`t -> 100 - t`), so both timestamp orders
/// of each pair of effects are covered.
///
/// Existence (CR 613.8a style, as documented: a static effect generated by another object exists
/// iff the environment says so from that source evaluated through the end of the effect's own
/// layer without that effect):
/// - E1 (layer 6, source 1) needs `test.src_one` on object 1; E2 (layer 6, source 2) removes all
///   abilities from `TestMarked` objects and E3 (layer 4) marks object 1, so E1 does not exist.
/// - E4 (layer 6, source 4) grants `test.src_five` to `TestHost` objects (object 5); E5 (layer
///   7c, source 5) needs it: existence depends on another layer's output.
/// - E6 (layer 6, source 6) grants `test.src_seven` to `TestKindB` objects (object 7); E7 (layer
///   6, source 7) needs `test.src_seven`: existence depends on another effect's output in the
///   same layer. E6 itself needs `test.src_six` on object 6, which E8 (layer 6) removes; E9
///   (layer 6, resolved) removes all abilities of object 7 (order against E6 matters).
/// - E10 (layer 4, source 10) animates artifacts and needs `test.src_ten`; E11 (layer 4) makes
///   object 10 a land (its ability survives layer 4).
/// - E12 (layer 7b, source 11) sets artifacts to 4/4 and needs `test.src_eleven`, which E13
///   (layer 6) removes from object 11.
/// - E14 (layer 7c, source 12) +0/+2 to flying creatures, needs `test.src_twelve`, granted to
///   object 12 by E15 (layer 6, source 13) only while object 13 still has `test.src_thirteen`.
fn board_b(swapped: bool) -> Board {
    let t = |n: u64| if swapped { 100 - n } else { n };
    let mut env = Env::default();
    env.put(
        1,
        1,
        creature("test.fs_one", &[], &["test.src_one"], (2, 2)),
    );
    env.put(2, 2, creature("test.fs_two", &[], &[], (1, 1)));
    env.put(
        3,
        1,
        creature("test.fs_three", &["TestMarked"], &["test.flier"], (1, 3)),
    );
    env.put(4, 2, creature("test.fs_four", &[], &[], (2, 1)));
    env.put(5, 1, creature("test.fs_five", &["TestHost"], &[], (1, 1)));
    env.put(
        6,
        2,
        creature("test.fs_six", &["TestKindC"], &["test.src_six"], (3, 3)),
    );
    env.put(7, 1, creature("test.fs_seven", &["TestKindB"], &[], (2, 2)));
    env.put(8, 2, card("test.fs_eight", &["Artifact"], &[], &[], None));
    env.put(
        9,
        1,
        card("test.fs_nine", &["Artifact"], &["TestKindB"], &[], None),
    );
    env.put(
        10,
        2,
        card("test.fs_ten", &["Artifact"], &[], &["test.src_ten"], None),
    );
    env.put(
        11,
        1,
        creature("test.fs_eleven", &[], &["test.src_eleven"], (1, 2)),
    );
    env.put(12, 2, creature("test.fs_twelve", &[], &["flying"], (2, 3)));
    env.put(
        13,
        1,
        creature("test.fs_thirteen", &[], &["test.src_thirteen"], (0, 4)),
    );
    env.put(
        14,
        2,
        creature("test.fs_fourteen", &["TestHost"], &[], (4, 4)),
    );
    env.put(15, 1, card("test.fs_fifteen", &["Land"], &[], &[], None));
    env.put(
        16,
        2,
        creature("test.fs_sixteen", &["TestKindC"], &["flying"], (5, 1)),
    );
    env.counter(3, "+1/+1", 1, 30);
    env.counter(7, "flying", 1, 31);
    env.counter(14, "-1/-1", 2, 32);
    let fx = vec![
        /* E1 */
        stat(1, 1, t(20), Layer::L6, creatures(), grant("test.flier")),
        /* E2 */
        stat(
            2,
            2,
            t(21),
            Layer::L6,
            of_subtype("TestMarked"),
            LayerModification::RemoveAllAbilities,
        ),
        /* E3 */
        resolved(
            t(10),
            Layer::L4,
            locked(&[1]),
            add_types(&[], &["TestMarked"]),
        ),
        /* E4 */
        stat(
            4,
            2,
            t(22),
            Layer::L6,
            of_subtype("TestHost"),
            grant("test.src_five"),
        ),
        /* E5 */
        stat(5, 1, t(40), Layer::L7c, creatures(), modify_pt(1, 0)),
        /* E6 */
        stat(
            6,
            2,
            t(23),
            Layer::L6,
            of_subtype("TestKindB"),
            grant("test.src_seven"),
        ),
        /* E7 */
        stat(
            7,
            1,
            t(24),
            Layer::L6,
            with_keyword("flying"),
            grant("test.warded"),
        ),
        /* E8 */
        stat(
            16,
            2,
            t(25),
            Layer::L6,
            of_subtype("TestKindC"),
            LayerModification::RemoveAllAbilities,
        ),
        /* E9 */
        resolved(
            t(26),
            Layer::L6,
            locked(&[7]),
            LayerModification::RemoveAllAbilities,
        ),
        /* E10 */
        stat(
            10,
            2,
            t(11),
            Layer::L4,
            of_type("Artifact"),
            add_types(&["Creature"], &[]),
        ),
        /* E11 */
        resolved(t(12), Layer::L4, locked(&[10]), set_card_types(&["Land"])),
        /* E12 */
        stat(11, 1, t(30), Layer::L7b, of_type("Artifact"), set_pt(4, 4)),
        /* E13 */
        resolved(
            t(27),
            Layer::L6,
            locked(&[11]),
            LayerModification::RemoveAllAbilities,
        ),
        /* E14 */
        stat(
            12,
            2,
            t(41),
            Layer::L7c,
            with_keyword("flying"),
            modify_pt(0, 2),
        ),
        /* E15 */
        stat(
            13,
            1,
            t(28),
            Layer::L6,
            locked(&[12]),
            grant("test.src_twelve"),
        ),
        /* E16 */
        resolved(t(29), Layer::L7b, locked(&[5, 9]), set_pt(0, 6)),
    ];
    for (effect, ability) in [
        (1, "test.src_one"),
        (5, "test.src_five"),
        (6, "test.src_six"),
        (7, "test.src_seven"),
        (10, "test.src_ten"),
        (12, "test.src_eleven"),
        (14, "test.src_twelve"),
        (15, "test.src_thirteen"),
    ] {
        env.generated_by
            .insert(EffectId::new(effect), ability.to_owned());
    }
    let (label, golden_digest, golden_order) = if swapped {
        ("b2", GOLDEN_DIGEST_B2, GOLDEN_ORDER_B2)
    } else {
        ("b1", GOLDEN_DIGEST_B1, GOLDEN_ORDER_B1)
    };
    Board {
        golden_digest,
        golden_order,
        changed: 4,
        ..Board::new(label, env, fx)
    }
}

// =============================================================================================
// Board c: "CDA and view" (12 permanents with counting P/T)
// =============================================================================================

/// Board c: objects 1, 2 (player 1) and 7 (player 2) have [`count_expr`] P/T (read from the
/// post-layer-6 view of all objects). The counted set changes through a layer-2 control change
/// (object 3), a layer-4 animation of artifacts (objects 6, 11) and a layer-4 type set (object
/// 8); layer-7b and layer-7c effects then act on the counting objects and on the animated
/// artifacts in an order that depends on the timestamps. `swapped` reverses every effect
/// timestamp.
fn board_c(swapped: bool) -> Board {
    let t = |n: u64| if swapped { 100 - n } else { n };
    let mut env = Env::default();
    env.put(1, 1, cda_creature("test.cv_one", &[], &[]));
    env.put(2, 1, cda_creature("test.cv_two", &[], &[]));
    env.put(3, 1, creature("test.cv_three", &[], &[], (1, 1)));
    env.put(4, 1, creature("test.cv_four", &[], &[], (2, 2)));
    env.put(5, 1, creature("test.cv_five", &[], &[], (3, 1)));
    env.put(6, 1, card("test.cv_six", &["Artifact"], &[], &[], None));
    env.put(7, 2, cda_creature("test.cv_seven", &[], &[]));
    env.put(8, 2, creature("test.cv_eight", &[], &[], (1, 2)));
    env.put(9, 2, creature("test.cv_nine", &[], &[], (2, 3)));
    env.put(10, 2, creature("test.cv_ten", &[], &[], (4, 4)));
    env.put(11, 2, card("test.cv_eleven", &["Artifact"], &[], &[], None));
    env.put(12, 2, card("test.cv_twelve", &["Land"], &[], &[], None));
    env.counter(2, "+1/+1", 1, 50);
    env.counter(7, "-1/-1", 1, 51);
    let fx = vec![
        resolved(
            t(10),
            Layer::L2,
            locked(&[3]),
            LayerModification::SetController(p(2)),
        ),
        stat(
            6,
            1,
            t(11),
            Layer::L4,
            of_type("Artifact"),
            add_types(&["Creature"], &[]),
        ),
        resolved(t(12), Layer::L4, locked(&[8]), set_card_types(&["Land"])),
        resolved(t(20), Layer::L7b, locked(&[1]), set_pt(5, 5)),
        stat(7, 2, t(21), Layer::L7c, creatures(), modify_pt(1, 1)),
        resolved(t(22), Layer::L7c, locked(&[2, 7]), modify_pt(-2, 0)),
        stat(12, 2, t(23), Layer::L7b, of_type("Artifact"), set_pt(1, 1)),
        resolved(t(24), Layer::L7b, locked(&[1, 6]), set_pt(0, 4)),
        resolved(
            t(13),
            Layer::L4,
            locked(&[12]),
            add_types(&["Creature"], &[]),
        ),
    ];
    let (label, golden_digest, golden_order) = if swapped {
        ("c2", GOLDEN_DIGEST_C2, GOLDEN_ORDER_C2)
    } else {
        ("c1", GOLDEN_DIGEST_C1, GOLDEN_ORDER_C1)
    };
    Board {
        golden_digest,
        golden_order,
        changed: 9,
        ..Board::new(label, env, fx)
    }
}

fn boards() -> Vec<Board> {
    vec![
        board_a(),
        board_b(false),
        board_b(true),
        board_c(false),
        board_c(true),
    ]
}

// =============================================================================================
// Board d: "off-battlefield and baseless"
// =============================================================================================

/// Board d: objects 1, 3, 5 have bases (5 has a CDA P/T); ids 7 and 9 are listed without a base.
fn board_d(listing: &[u64]) -> Board {
    let mut env = Env::default();
    env.put(1, 1, creature("test.bl_one", &[], &["test.src_bl"], (1, 1)));
    env.put(3, 1, creature("test.bl_three", &[], &[], (2, 2)));
    env.put(5, 1, cda_creature("test.bl_five", &[], &[]));
    env.counter(3, "+1/+1", 1, 9);
    env.listed = Some(listing.iter().map(|n| o(*n)).collect());
    let fx = vec![
        stat(1, 1, 5, Layer::L7c, creatures(), modify_pt(1, 1)),
        resolved(6, Layer::L6, locked(&[3]), grant("test.flier")),
    ];
    env.generated_by
        .insert(EffectId::new(1), "test.src_bl".to_owned());
    Board::new("d", env, fx)
}

// =============================================================================================
// Helpers
// =============================================================================================

fn digest_hex(map: &BTreeMap<ObjectId, Evaluated>) -> String {
    evaluated_digest(map).to_hex()
}

fn n_of(board: &Board) -> u64 {
    u64::try_from(board.ids().len()).expect("fits")
}

// =============================================================================================
// Requirement 3: goldens
// =============================================================================================

/// REQ-LAYER-005, REQ-CORE-001, REQ-LAYER-001: the `evaluated_digest` of `evaluate_all` on boards
/// a, b (both timestamp orders) and c (both orders) equals the golden recorded at 1360900, in
/// both cache modes. (The cache's agreement with these maps is checked by the equivalence
/// tests.)
#[test]
fn req_layer_005_golden_digests() {
    let mut mismatches = Vec::new();
    for board in boards() {
        let reg = board.registry();
        let all = evaluate_all(&reg, &board.env)
            .unwrap_or_else(|err| panic!("board {}: {err:?}", board.label));
        let got = digest_hex(&all);
        if got != board.golden_digest {
            mismatches.push(format!("digest {} = {got}", board.label));
        }
    }
    assert!(
        mismatches.is_empty(),
        "golden mismatches:\n{}",
        mismatches.join("\n")
    );
}

/// REQ-LAYER-005, REQ-LAYER-001, REQ-CORE-001: for every object of boards b and c and the sample
/// objects of board a ([`Board::sample`]), `evaluate_with_order(..).0` equals the object's entry of
/// `evaluate_all` and the record names the object; the `OrderRecord` steps of the sample objects
/// hash to the golden recorded at 1360900 (the order within a layer, REQ-LAYER-002, is unchanged
/// by the optimisation).
#[test]
fn req_layer_005_golden_order_records() {
    let mut mismatches = Vec::new();
    for board in boards() {
        let reg = board.registry();
        let all = evaluate_all(&reg, &board.env).expect("evaluate_all");
        let mut text = String::new();
        for id in board.sample.clone() {
            let (ev, record) = evaluate_with_order(&reg, &board.env, id)
                .unwrap_or_else(|err| panic!("board {}: {id:?}: {err:?}", board.label));
            assert_eq!(record.object, id, "board {}", board.label);
            assert_eq!(Some(&ev), all.get(&id), "board {}: {id:?}", board.label);
            text.push_str(&format!("{}:{:?}\n", id.get(), record.steps));
        }
        let got = sha256(text.as_bytes()).to_hex();
        if got != board.golden_order {
            mismatches.push(format!("order {} = {got}", board.label));
        }
    }
    assert!(
        mismatches.is_empty(),
        "golden mismatches:\n{}",
        mismatches.join("\n")
    );
}

// =============================================================================================
// Requirements 4 and 5: equivalence and cache bookkeeping
// =============================================================================================

/// Requirements 4 and 5 on one board, with one cache (so board a pays as few cold fills as
/// possible): (i) uncached `evaluate_all` equals `evaluate(id)` for the sample objects; (ii) a
/// cold `LayerCache::evaluate_all` equals it, counting n misses and no hit and storing n entries
/// (cached) or none (full recompute); (iii) a warm second call equals it, counting n hits (cached)
/// or n more misses (full recompute); (iv) `invalidate(CounterChange)` bumps the version by one,
/// counts one invalidation and drops every entry, and with the counters changed the cache agrees
/// with the uncached result of the new environment (which differs from the old one); (v) a
/// registry with one more effect invalidates the cache by itself (one invalidation, version +1)
/// and the cache agrees with the new uncached result; the same registry again does not.
fn check_equivalence(board: &Board) {
    let label = board.label;
    let n = n_of(board);
    let reg = board.registry();
    let all = evaluate_all(&reg, &board.env).expect("evaluate_all");
    assert_eq!(
        all.keys().copied().collect::<Vec<_>>(),
        board.ids(),
        "board {label}: universe"
    );
    assert_eq!(
        digest_hex(&all),
        board.golden_digest,
        "board {label}: golden"
    );
    // (i)
    for id in board.sample.clone() {
        let one = evaluate(&reg, &board.env, id).expect("evaluate");
        assert_eq!(Some(&one), all.get(&id), "board {label}: evaluate({id:?})");
    }
    // (ii)
    let mut cache = LayerCache::new();
    assert_eq!(
        cache.evaluate_all(&reg, &board.env).expect("cold"),
        all,
        "{label}: cold"
    );
    let s = cache.stats();
    assert_eq!(
        (s.hits, s.misses, s.invalidations),
        (0, n, 0),
        "board {label}: cold"
    );
    assert_eq!(cache.version().get(), 0, "board {label}: first registry");
    if FULL_RECOMPUTE {
        assert_eq!(cache.len(), 0, "board {label}");
        assert!(cache.is_empty(), "board {label}");
    } else {
        assert_eq!(
            u64::try_from(cache.len()).expect("fits"),
            n,
            "board {label}"
        );
    }
    // (iii)
    assert_eq!(
        cache.evaluate_all(&reg, &board.env).expect("warm"),
        all,
        "{label}: warm"
    );
    let s = cache.stats();
    if FULL_RECOMPUTE {
        assert_eq!((s.hits, s.misses), (0, 2 * n), "board {label}: warm");
        assert_eq!(cache.len(), 0, "board {label}");
    } else {
        assert_eq!((s.hits, s.misses), (n, n), "board {label}: warm");
    }
    assert_eq!(s.invalidations, 0, "board {label}");
    for id in board.sample.clone() {
        assert_eq!(
            cache.evaluate(&reg, &board.env, id).expect("lookup"),
            all[&id],
            "board {label}: cache.evaluate({id:?})"
        );
    }
    // (iv)
    let target = board.changed;
    let mut env2 = board.env.clone();
    env2.counter(target, "+1/+1", 3, 500);
    let expected2 = evaluate_all(&reg, &env2).expect("new env");
    assert_ne!(
        expected2, all,
        "board {label}: the counter change must change a value"
    );
    let v0 = cache.version().get();
    let inv0 = cache.stats().invalidations;
    assert_eq!(cache.invalidate(Invalidation::CounterChange).get(), v0 + 1);
    assert_eq!(cache.version().get(), v0 + 1, "{label}");
    assert_eq!(cache.stats().invalidations, inv0 + 1, "{label}");
    assert_eq!(cache.len(), 0, "board {label}: entries dropped");
    let misses0 = cache.stats().misses;
    assert_eq!(
        cache.evaluate_all(&reg, &env2).expect("after"),
        expected2,
        "{label}: iv"
    );
    assert_eq!(cache.stats().misses, misses0 + n, "board {label}: refilled");
    // (v)
    let mut more = board.effects.clone();
    more.push(resolved(
        600,
        Layer::L7c,
        locked(&[target]),
        modify_pt(2, 0),
    ));
    let reg3 = registry_of(&more);
    let expected3 = evaluate_all(&reg3, &env2).expect("new registry");
    assert_ne!(
        expected3, expected2,
        "board {label}: the new effect must change a value"
    );
    let v1 = cache.version().get();
    let inv1 = cache.stats().invalidations;
    assert_eq!(
        cache.evaluate_all(&reg3, &env2).expect("reg"),
        expected3,
        "{label}: v"
    );
    assert_eq!(cache.version().get(), v1 + 1, "board {label}: version");
    assert_eq!(
        cache.stats().invalidations,
        inv1 + 1,
        "board {label}: invalidations"
    );
    // The same registry again does not invalidate.
    for id in board.sample.clone() {
        assert_eq!(
            cache.evaluate(&reg3, &env2, id).expect("again"),
            expected3[&id],
            "{label}"
        );
    }
    assert_eq!(
        cache.version().get(),
        v1 + 1,
        "board {label}: no new version"
    );
    assert_eq!(
        cache.stats().invalidations,
        inv1 + 1,
        "board {label}: no new invalidation"
    );
}

/// REQ-LAYER-005, REQ-LAYER-001: requirements 4 (i)-(v) and the `evaluate_all` bookkeeping of
/// requirement 5 on board a (200 permanents), in both modes.
#[test]
fn req_layer_005_cache_equivalence_board_a() {
    check_equivalence(&board_a());
}

/// REQ-LAYER-005, REQ-LAYER-001: requirements 4 (i)-(v) and the `evaluate_all` bookkeeping of
/// requirement 5 on board b, both timestamp orders, in both modes.
#[test]
fn req_layer_005_cache_equivalence_board_b() {
    check_equivalence(&board_b(false));
    check_equivalence(&board_b(true));
}

/// REQ-LAYER-005, REQ-LAYER-001: requirements 4 (i)-(v) and the `evaluate_all` bookkeeping of
/// requirement 5 on board c, both timestamp orders, in both modes.
#[test]
fn req_layer_005_cache_equivalence_board_c() {
    check_equivalence(&board_c(false));
    check_equivalence(&board_c(true));
}

/// A single `evaluate(id)` on a cold cache counts one miss and stores only that object (cached
/// mode); a following `evaluate_all` counts exactly one hit and n-1 more misses (cached) or n
/// more misses (full recompute); results agree with the uncached evaluator.
fn check_single_then_all(board: &Board) {
    let label = board.label;
    let n = n_of(board);
    let reg = board.registry();
    let expected = evaluate_all(&reg, &board.env).expect("uncached");
    let pick = board.ids()[board.ids().len() / 2];
    let mut cache = LayerCache::new();
    assert_eq!(
        cache.evaluate(&reg, &board.env, pick).expect("one"),
        expected[&pick]
    );
    let s = cache.stats();
    assert_eq!((s.hits, s.misses), (0, 1), "board {label}: single");
    let want_len = if FULL_RECOMPUTE { 0 } else { 1 };
    assert_eq!(
        cache.len(),
        want_len,
        "board {label}: single stores only itself"
    );
    assert_eq!(
        cache.evaluate_all(&reg, &board.env).expect("all"),
        expected,
        "{label}"
    );
    let s = cache.stats();
    if FULL_RECOMPUTE {
        assert_eq!((s.hits, s.misses), (0, 1 + n), "board {label}");
        assert_eq!(cache.len(), 0, "board {label}");
    } else {
        assert_eq!((s.hits, s.misses), (1, n), "board {label}");
        assert_eq!(
            u64::try_from(cache.len()).expect("fits"),
            n,
            "board {label}"
        );
    }
    assert_eq!(s.invalidations, 0, "board {label}");
}

/// REQ-LAYER-005: single-object lookup then `evaluate_all` bookkeeping (requirement 5) on board a.
#[test]
fn req_layer_005_bookkeeping_single_then_all_board_a() {
    check_single_then_all(&board_a());
}

/// REQ-LAYER-005: single-object lookup then `evaluate_all` bookkeeping (requirement 5) on boards
/// b and c, both timestamp orders.
#[test]
fn req_layer_005_bookkeeping_single_then_all_small_boards() {
    for board in boards().iter().filter(|b| !b.large) {
        check_single_then_all(board);
    }
}

// =============================================================================================
// Requirement 6: board d (unsorted, duplicated, baseless ids)
// =============================================================================================

/// The listings of board d's objects tried: unsorted with a duplicate, reversed, ascending, and
/// another shuffle. Bases: 1, 3, 5; no base: 7, 9; 3 listed twice.
const D_LISTINGS: [[u64; 6]; 4] = [
    [5, 3, 9, 3, 1, 7],
    [9, 7, 5, 3, 3, 1],
    [1, 3, 3, 5, 7, 9],
    [7, 1, 9, 3, 5, 3],
];

/// REQ-LAYER-005, REQ-CORE-001: on board d, `evaluate_all` and `LayerCache::evaluate_all` return
/// the same error, `UnknownObject` of the lowest baseless id (7), for every listing order. The
/// cache looks up 1, 3, 3, 5 and then 7 (ascending, duplicates kept, stopping at the error): in
/// cached mode 4 misses (the error included), 1 hit (the duplicate) and entries 1, 3, 5 stored;
/// in full-recompute mode 5 misses, no hit, nothing stored. The error is never stored.
#[test]
fn req_layer_005_board_d_first_error_any_order() {
    for listing in D_LISTINGS {
        let board = board_d(&listing);
        let reg = board.registry();
        let want = Err(LayerError::UnknownObject(o(7)));
        assert_eq!(evaluate_all(&reg, &board.env), want, "{listing:?}");
        let mut cache = LayerCache::new();
        assert_eq!(
            cache.evaluate_all(&reg, &board.env),
            want,
            "{listing:?}: cache"
        );
        let s = cache.stats();
        if FULL_RECOMPUTE {
            assert_eq!((s.hits, s.misses), (0, 5), "{listing:?}");
            assert_eq!(cache.len(), 0, "{listing:?}");
        } else {
            assert_eq!((s.hits, s.misses), (1, 4), "{listing:?}");
            assert_eq!(
                cache.len(),
                3,
                "{listing:?}: entries below 7 stored, 7 and 9 not"
            );
        }
        // The stored entries answer as hits (cached); the baseless ids are misses again and
        // still not stored.
        for id in [1, 3, 5] {
            assert_eq!(
                cache.evaluate(&reg, &board.env, o(id)),
                evaluate(&reg, &board.env, o(id)),
                "{listing:?}: {id}"
            );
        }
        for id in [7, 9] {
            assert_eq!(
                cache.evaluate(&reg, &board.env, o(id)),
                Err(LayerError::UnknownObject(o(id))),
                "{listing:?}"
            );
        }
        let s = cache.stats();
        if FULL_RECOMPUTE {
            assert_eq!((s.hits, s.misses), (0, 10), "{listing:?}");
            assert_eq!(cache.len(), 0, "{listing:?}");
        } else {
            assert_eq!((s.hits, s.misses), (4, 6), "{listing:?}");
            assert_eq!(cache.len(), 3, "{listing:?}");
        }
        assert_eq!(s.invalidations, 0, "{listing:?}");
    }
}

/// REQ-LAYER-005, REQ-CORE-001: on board d, `evaluate(id)` succeeds for every listed id with a
/// base although other listed ids have none, and is the same for every listing; its universe
/// ({1, 3, 5}) equals the universe of `evaluate_all` over the same listing without the baseless
/// ids, so it equals that successful result (and the cache's).
#[test]
fn req_layer_005_board_d_evaluate_skips_baseless() {
    let canonical = board_d(&[1, 3, 5]);
    let reg = canonical.registry();
    let full = evaluate_all(&reg, &canonical.env).expect("no baseless ids");
    assert_eq!(full.len(), 3);
    assert_eq!(
        LayerCache::new().evaluate_all(&reg, &canonical.env),
        Ok(full.clone())
    );
    for listing in D_LISTINGS {
        let board = board_d(&listing);
        let without: Vec<u64> = listing.iter().copied().filter(|n| *n < 7).collect();
        let clean = board_d(&without);
        let clean_all = evaluate_all(&reg, &clean.env).expect("clean");
        assert_eq!(
            clean_all, full,
            "{listing:?}: listing order and duplicates do not matter"
        );
        let mut cache = LayerCache::new();
        for id in [1, 3, 5] {
            let one = evaluate(&reg, &board.env, o(id)).expect("has a base");
            assert_eq!(one, clean_all[&o(id)], "{listing:?}: {id}");
            assert_eq!(
                cache.evaluate(&reg, &board.env, o(id)),
                Ok(one),
                "{listing:?}"
            );
        }
        assert_eq!(
            evaluate(&reg, &board.env, o(7)),
            Err(LayerError::UnknownObject(o(7)))
        );
    }
}

/// REQ-LAYER-005: a listed duplicate id is looked up once per listing (hit/miss per lookup):
/// with every object listed twice, a cold `LayerCache::evaluate_all` counts n misses and n hits
/// (cached) or 2n misses (full recompute), and equals the uncached result.
#[test]
fn req_layer_005_duplicates_counted_per_lookup() {
    let board = board_b(false);
    let reg = board.registry();
    let expected = evaluate_all(&reg, &board.env).expect("uncached");
    let n = n_of(&board);
    let mut list = board.ids();
    list.extend(board.ids().into_iter().rev());
    let env = board.env.relisted(list);
    assert_eq!(evaluate_all(&reg, &env).expect("dup"), expected);
    let mut cache = LayerCache::new();
    assert_eq!(cache.evaluate_all(&reg, &env).expect("dup cache"), expected);
    let s = cache.stats();
    if FULL_RECOMPUTE {
        assert_eq!((s.hits, s.misses), (0, 2 * n));
    } else {
        assert_eq!((s.hits, s.misses), (n, n));
        assert_eq!(u64::try_from(cache.len()).expect("fits"), n);
    }
}

// =============================================================================================
// Requirement 7: asked object not listed
// =============================================================================================

/// REQ-LAYER-005, REQ-CORE-001: object 4 has a base but is not in `env.objects()` (3, 1, 2;
/// object 5 has a base and is not listed either). `evaluate(4)` succeeds and the cache returns the
/// same; `evaluate_all` does not include 4 or 5; a cached `evaluate_all` after the cache evaluated
/// 4 equals the uncached `evaluate_all` (its universe excludes 4, whose static effect needs its
/// source and whose creature is counted by the CDA of object 1 only in a universe containing 4).
#[test]
fn req_layer_005_unlisted_asked_object() {
    let mut env = Env::default();
    env.put(1, 1, cda_creature("test.ul_one", &[], &[]));
    env.put(2, 1, creature("test.ul_two", &[], &[], (2, 2)));
    env.put(3, 2, creature("test.ul_three", &[], &[], (1, 3)));
    env.put(
        4,
        1,
        creature("test.ul_four", &[], &["test.src_four"], (3, 3)),
    );
    env.put(5, 2, creature("test.ul_five", &[], &[], (1, 1)));
    env.listed = Some(vec![o(3), o(1), o(2)]);
    env.generated_by
        .insert(EffectId::new(1), "test.src_four".to_owned());
    let reg = registry_of(&[
        stat(4, 1, 5, Layer::L7c, creatures(), modify_pt(1, 1)),
        resolved(6, Layer::L7b, locked(&[2, 4]), set_pt(4, 1)),
    ]);
    let uncached_all = evaluate_all(&reg, &env).expect("listed objects have bases");
    assert_eq!(
        uncached_all.keys().copied().collect::<Vec<_>>(),
        vec![o(1), o(2), o(3)]
    );
    let four = evaluate(&reg, &env, o(4)).expect("object 4 has a base");
    // The asked object's own expected value, independent of the cache: base 3/3, layer 7b sets
    // 4/1 (locked {2, 4}), layer 7c +1/+1 from source 4's own generating ability (visible).
    assert_eq!(
        (
            four.characteristics.power.as_ref().and_then(literal_of),
            four.characteristics.toughness.as_ref().and_then(literal_of)
        ),
        (Some(5), Some(2)),
        "evaluate(o(4)) is 5/2"
    );
    let five = evaluate(&reg, &env, o(5)).expect("object 5 has a base");
    let mut cache = LayerCache::new();
    assert_eq!(cache.evaluate(&reg, &env, o(4)), Ok(four.clone()));
    assert_eq!(cache.evaluate(&reg, &env, o(5)), Ok(five));
    assert_eq!(
        cache.evaluate(&reg, &env, o(4)),
        Ok(four),
        "second lookup of 4"
    );
    assert_eq!(cache.evaluate_all(&reg, &env), Ok(uncached_all.clone()));
    assert_eq!(
        cache.evaluate_all(&reg, &env),
        Ok(uncached_all.clone()),
        "warm"
    );
    for id in [1, 2, 3] {
        assert_eq!(
            evaluate(&reg, &env, o(id)).expect("listed"),
            uncached_all[&o(id)],
            "evaluate({id}) has the listed universe"
        );
        assert_eq!(
            cache.evaluate(&reg, &env, o(id)),
            Ok(uncached_all[&o(id)].clone())
        );
    }
    assert_eq!(cache.stats().invalidations, 0);
}

// =============================================================================================
// Requirement 8: order independence
// =============================================================================================

/// A fixed permutation of `1..=200`: `i -> 1 + (77 * i) mod 200` (77 is coprime to 200).
fn permuted_200() -> Vec<ObjectId> {
    (0..A_PERMANENTS)
        .map(|i| o(1 + (77 * i) % A_PERMANENTS))
        .collect()
}

/// REQ-CORE-001, REQ-LAYER-005: the digest of `evaluate_all` on boards a and b (and of
/// `LayerCache::evaluate_all` on board b) is the golden one whichever order `env.objects()` lists the ids: ascending, descending
/// and a fixed shuffle.
#[test]
fn req_core_001_order_independence() {
    let a = board_a();
    let b = board_b(false);
    let mut a_desc = a.ids();
    a_desc.reverse();
    let mut b_desc = b.ids();
    b_desc.reverse();
    let b_shuffle: Vec<ObjectId> = [9, 2, 14, 5, 16, 1, 11, 7, 3, 13, 6, 10, 15, 4, 12, 8]
        .iter()
        .map(|n| o(*n))
        .collect();
    let cases = [
        (&a, a.ids()),
        (&a, a_desc),
        (&a, permuted_200()),
        (&b, b.ids()),
        (&b, b_desc),
        (&b, b_shuffle),
    ];
    for (board, list) in cases {
        let reg = board.registry();
        let env = board.env.relisted(list.clone());
        let want = board.golden_digest;
        let all = evaluate_all(&reg, &env).expect("evaluate_all");
        assert_eq!(
            digest_hex(&all),
            want,
            "board {} listing {list:?}",
            board.label
        );
        // A cold cache fill costs one board pass per object on the base; board a's cache is
        // covered by the equivalence tests, so only board b's is filled per listing here.
        if !board.large {
            let cached = LayerCache::new().evaluate_all(&reg, &env).expect("cache");
            assert_eq!(digest_hex(&cached), want, "board {} cache", board.label);
        }
    }
}

// =============================================================================================
// Property: random small boards
// =============================================================================================

/// A tiny fixed-seed integer LCG (test-only; not engine randomness; R3).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn random_board(seed: u64) -> Board {
    let mut rng = Lcg(seed);
    let n = 4 + rng.below(6);
    let mut env = Env::default();
    for i in 1..=n {
        let controller = 1 + rng.below(2);
        let name = format!("test.prop_{i}");
        let sub = ["TestKindA", "TestKindB"][usize::try_from(rng.below(2)).expect("small")];
        let c = match rng.below(5) {
            0 => card(&name, &["Artifact"], &[sub], &["test.src_prop"], None),
            1 => cda_creature(&name, &[sub], &["flying"]),
            2 => creature(&name, &[], &["test.src_prop", "flying"], (1, 1)),
            _ => creature(
                &name,
                &[sub],
                &[],
                (i64::try_from(rng.below(4)).expect("small"), 2),
            ),
        };
        env.put(i, controller, c);
        if rng.below(3) == 0 {
            let kind = ["+1/+1", "-1/-1", "flying"][usize::try_from(rng.below(3)).expect("small")];
            env.counter(i, kind, 1, rng.below(20));
        }
    }
    let effects = 3 + rng.below(6);
    let mut fx = Vec::new();
    for e in 1..=effects {
        let layer_pick = rng.below(7);
        let (layer, m) = match layer_pick {
            0 => (
                Layer::L2,
                LayerModification::SetController(p(1 + rng.below(2))),
            ),
            1 => (Layer::L4, add_types(&["Creature"], &[])),
            2 => (Layer::L4, add_types(&[], &["TestKindB"])),
            3 => (Layer::L6, grant("flying")),
            4 => (Layer::L6, LayerModification::RemoveAllAbilities),
            5 => (Layer::L7b, set_pt(2, 3)),
            _ => (Layer::L7c, modify_pt(1, -1)),
        };
        let affected = match rng.below(5) {
            0 => creatures(),
            1 => of_type("Artifact"),
            2 => of_subtype("TestKindB"),
            3 => with_keyword("flying"),
            _ => locked(&[1 + rng.below(n), 1 + rng.below(n)]),
        };
        let t = rng.below(12);
        let effect = if rng.below(2) == 0 {
            let source = 1 + rng.below(n);
            if rng.below(2) == 0 {
                env.generated_by
                    .insert(EffectId::new(e), "test.src_prop".to_owned());
            }
            stat(source, 1, t, layer, affected, m)
        } else {
            resolved(t, layer, affected, m)
        };
        fx.push(effect);
    }
    Board::new("prop", env, fx)
}

/// REQ-LAYER-005, REQ-LAYER-001, REQ-CORE-001: over 30 fixed-seed small boards with random
/// effect layers, timestamps, selectors, sources and existence conditions, the cold-cache
/// `evaluate_all`, the uncached `evaluate_all` (listed ascending and reversed) and per-object
/// `evaluate` agree.
#[test]
fn req_layer_005_property_small_boards() {
    for seed in 1..=30_u64 {
        let board = random_board(seed * 0x9E37_79B9);
        let reg = board.registry();
        let all = evaluate_all(&reg, &board.env);
        let cached = LayerCache::new().evaluate_all(&reg, &board.env);
        assert_eq!(cached, all, "seed {seed}");
        let mut rev = board.ids();
        rev.reverse();
        let env_rev = board.env.relisted(rev);
        assert_eq!(
            evaluate_all(&reg, &env_rev),
            all,
            "seed {seed}: reversed listing"
        );
        let all = all.unwrap_or_else(|err| panic!("seed {seed}: {err:?}"));
        for id in board.ids() {
            assert_eq!(
                evaluate(&reg, &board.env, id).as_ref(),
                Ok(&all[&id]),
                "seed {seed}: {id:?}"
            );
        }
    }
}
