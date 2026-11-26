// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Full-board layer recompute benchmark (REQ-LAYER-005, REQ-NFR-009): 200 synthetic permanents
//! controlled by 4 players, with static selector effects in layers 4, 5, 6, 7b and 7c, resolved
//! effects on locked sets, counters, one characteristic-defining P/T per player and layer-4
//! effects that depend on each other. The SRS target (at most 2 ms per full-board recompute on
//! the reference runner) is measured here and never asserted (R7). Synthetic objects only (R5);
//! the board comes from a fixed generator (no randomness, clock or environment).

use std::collections::{BTreeMap, BTreeSet};
use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, Evaluated, Invalidation,
    LayerCache, LayerEnvironment, LayerModification, MatchContext, ObjectBase, evaluate_all,
    keyword_id, literal, literal_of,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::ability::{AbilityBase, KeywordInstanceAST, Layer};
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, SelectorFilter, WordRef};

/// Permanents on the board.
const PERMANENTS: u64 = 200;
/// Players; each controls `PERMANENTS / PLAYERS` permanents.
const PLAYERS: u64 = 4;
/// One creature subtype per player.
const SUBTYPES: [&str; 4] = ["Elf", "Goblin", "Soldier", "Zombie"];

fn words(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A synthetic keyword ability with the keyword name of `word` (for example `VIGILANCE`).
fn keyword(word: &str) -> AbilityAST {
    AbilityAST::Keyword(KeywordInstanceAST::new(
        AbilityBase::new(format!("test.{word}")),
        keyword_id(word),
    ))
}

fn filter(pred: PredicateAST) -> Affected {
    Affected::Selector(ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    }))
}

fn subtype(word: &str) -> Affected {
    filter(PredicateAST::HasSubtype {
        value: WordRef::Name(word.to_owned()),
    })
}

fn artifacts() -> Affected {
    filter(PredicateAST::HasCardType {
        value: WordRef::Name("Artifact".to_owned()),
    })
}

/// The synthetic board: the bases, the counters and the per-player CDA value.
struct Board {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    cda_value: BTreeMap<PlayerId, i64>,
}

/// Permanent `i` (1-based): the first of each player is a creature with a
/// characteristic-defining P/T; then every tenth is an artifact and every tenth plus five a
/// land; the rest are creatures of their controller's subtype.
fn permanent(i: u64) -> ObjectBase {
    let player = (i - 1) % PLAYERS;
    let size = i64::try_from(i % 5).unwrap_or(0) + 1;
    let mut c = Characteristics {
        names: words(&[&format!("test.creature_{i}")]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::new(1 << player).unwrap_or(ColorBitmask::NONE),
        supertypes: BTreeSet::new(),
        card_types: words(&["Creature"]),
        subtypes: words(&[SUBTYPES[usize::try_from(player).unwrap_or(0)]]),
        rules_text: TextTokenStream::default(),
        abilities: vec![keyword("vigilance")],
        power: Some(literal(size)),
        toughness: Some(literal(size + 1)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    };
    if i <= PLAYERS {
        // A characteristic-defining expression the board evaluates per controller.
        let cda = ValueExpressionAST::Memory("test.count_controlled".to_owned());
        c.power = Some(cda.clone());
        c.toughness = Some(cda);
    } else if i.is_multiple_of(5) {
        c.card_types = words(&[if i.is_multiple_of(10) {
            "Artifact"
        } else {
            "Land"
        }]);
        c.subtypes = BTreeSet::new();
        c.power = None;
        c.toughness = None;
        c.abilities = Vec::new();
    }
    ObjectBase {
        characteristics: c,
        controller: Some(PlayerId::new(player + 1)),
        on_battlefield: true,
    }
}

impl Board {
    fn new() -> Board {
        let bases = (1..=PERMANENTS)
            .map(|i| (ObjectId::new(i), permanent(i)))
            .collect();
        let counters = (7..=PERMANENTS)
            .step_by(7)
            .map(|i| {
                let kind = if i.is_multiple_of(3) {
                    "flying"
                } else {
                    "+1/+1"
                };
                (ObjectId::new(i), BTreeMap::from([(kind.to_owned(), 2)]))
            })
            .collect();
        let per_player = i64::try_from(PERMANENTS / PLAYERS).unwrap_or(0);
        let cda_value = (1..=PLAYERS)
            .map(|p| (PlayerId::new(p), per_player))
            .collect();
        Board {
            bases,
            counters,
            cda_value,
        }
    }
}

fn holds(pred: &PredicateAST, view: &Evaluated) -> bool {
    let c = &view.characteristics;
    match pred {
        PredicateAST::HasCardType {
            value: WordRef::Name(w),
        } => c.card_types.contains(w),
        PredicateAST::HasSubtype {
            value: WordRef::Name(w),
        } => c.subtypes.contains(w),
        // `IS_CREATURE`, the only other predicate this board uses.
        _ => c.card_types.contains("Creature"),
    }
}

impl LayerEnvironment for Board {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn counter_timestamp(&self, id: ObjectId, _kind: &str) -> Option<Timestamp> {
        Some(Timestamp::new(1_000 + id.get()))
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        matches!(selector, ObjectSelectorAST::Filter(f) if f.predicate.as_ref().is_some_and(|p| holds(p, ctx.candidate_view)))
    }

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        literal_of(expr).or_else(|| {
            let controller = self.bases.get(&object)?.controller?;
            self.cda_value.get(&controller).copied()
        })
    }
}

/// A static effect of permanent `source`'s own ability.
fn stat(
    source: u64,
    t: u64,
    layer: Layer,
    affected: Affected,
    m: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(ObjectId::new(source)),
        controller: PlayerId::new((source - 1) % PLAYERS + 1),
        timestamp: Timestamp::new(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Static { granted_at: None },
        affected,
        modification: m,
    }
}

fn add_types(card: &[&str], sub: &[&str]) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: words(card),
        subtypes: words(sub),
    }
}

/// The effects: per player a layer-4 subtype grant to every creature, a layer-6 keyword grant
/// and a layer-7c boost to that subtype (static selectors); a layer-4 "artifacts are creatures"
/// effect the subtype grants depend on, a layer-5 color addition, a layer-7b set on artifacts;
/// and resolved effects locked to fixed sets in layers 6, 7b and 7c.
fn effects() -> ContinuousEffects {
    let mut out: Vec<ContinuousEffect> = Vec::new();
    for (n, word) in (1_u64..).zip(SUBTYPES) {
        let creatures = filter(PredicateAST::IsCreature);
        out.push(stat(
            n,
            10 + n,
            Layer::L4,
            creatures,
            add_types(&[], &[word]),
        ));
        let flying = LayerModification::AddAbility(keyword("flying"));
        out.push(stat(n, 20 + n, Layer::L6, subtype(word), flying));
        let boost = LayerModification::ModifyPt {
            power: 1,
            toughness: 1,
        };
        out.push(stat(n, 30 + n, Layer::L7c, subtype(word), boost));
    }
    out.push(stat(
        10,
        15,
        Layer::L4,
        artifacts(),
        add_types(&["Creature"], &[]),
    ));
    let white = LayerModification::AddColors(ColorBitmask::WHITE);
    out.push(stat(
        20,
        40,
        Layer::L5,
        filter(PredicateAST::IsCreature),
        white,
    ));
    let two = LayerModification::SetPt {
        power: Some(2),
        toughness: Some(2),
    };
    out.push(stat(30, 41, Layer::L7b, artifacts(), two));
    let layers = [Layer::L6, Layer::L7b, Layer::L7c];
    let mods = [
        LayerModification::RemoveAllAbilities,
        LayerModification::SetPt {
            power: Some(0),
            toughness: Some(1),
        },
        LayerModification::ModifyPt {
            power: 3,
            toughness: -1,
        },
    ];
    for k in 0..6_u64 {
        let ids = (0..10)
            .map(|j| ObjectId::new(1 + (k * 31 + j * 7) % PERMANENTS))
            .collect();
        let which = usize::try_from(k % 3).unwrap_or(0);
        out.push(ContinuousEffect {
            source: None,
            origin: EffectOrigin::Resolved,
            ..stat(
                1 + k,
                50 + k,
                layers[which],
                Affected::Objects(ids),
                mods[which].clone(),
            )
        });
    }
    let mut registry = ContinuousEffects::new();
    for (id, effect) in (1_u64..).zip(out) {
        registry
            .register(EffectId::new(id), effect)
            .expect("valid synthetic effect");
    }
    registry
}

fn bench(c: &mut Criterion) {
    let board = Board::new();
    let registry = effects();
    let mut warm = LayerCache::new();
    assert_eq!(
        warm.evaluate_all(&registry, &board),
        evaluate_all(&registry, &board),
        "the cache must agree with a full recompute"
    );
    c.bench_function("full_recompute_200", |b| {
        b.iter(|| black_box(evaluate_all(black_box(&registry), &board)));
    });
    c.bench_function("cached_cold_200", |b| {
        b.iter(|| black_box(LayerCache::new().evaluate_all(black_box(&registry), &board)));
    });
    c.bench_function("cached_warm_200", |b| {
        b.iter(|| black_box(warm.evaluate_all(black_box(&registry), &board)));
    });
    c.bench_function("cached_after_invalidation_200", |b| {
        b.iter_batched(
            || warm.clone(),
            |mut cache| {
                cache.invalidate(Invalidation::CounterChange);
                black_box(cache.evaluate_all(black_box(&registry), &board))
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
