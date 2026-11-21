// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the interpreter's expression layer: REQ-DSL-003 (references, selectors,
// predicates, value expressions, evaluation context, LKI per CR 608.2h / CR 113.7a /
// CR 608.2b, APNAP per CR 101.4) and REQ-ARCH-002 (impossible instructions, CR 101.3).
// Synthetic objects and players only.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::card::{ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::{
    Cmp, Direction, DistinctAttr, HistoryQuery as HistoryAST, MathOp, ObjectAttr,
    ObjectSelectorAST, OrderKey, Ordering, PlayerAttr, PlayerGroup, PlayerSelectorAST,
    PredicateAST, ReferenceAST, SelectorFilter, ValueExpressionAST, WordRef, Zone,
};

use super::*;
use crate::events::{EventContext, EventState, GameEvent, History};
use crate::memory::{CharacteristicsProvider, ChoiceValue, LkiStore, Memory, MemoryTable};
use crate::objects::Characteristics;
use crate::permanent::Permanents;
use crate::state::GameState;
use crate::zones::{MoveRequest, Position, ZoneKind, Zones, ZonesConfig};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

/// A colour bitmask from bits (W=1, U=2, B=4, R=8, G=16).
fn cols(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).unwrap()
}

fn chars(name: &str, types: &[&str], colors: ColorBitmask) -> Characteristics {
    Characteristics {
        names: BTreeSet::from([name.to_owned()]),
        mana_cost: None,
        color_indicator: None,
        colors,
        supertypes: BTreeSet::new(),
        card_types: types.iter().map(|s| (*s).to_owned()).collect(),
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

/// A synthetic creature `test.flier_<p>_<t>` on the battlefield, controlled by its owner.
fn creature(id: u64, owner: u64, power: i64, toughness: i64) -> ObjectView {
    let mut v = ObjectView::new(
        o(id),
        Zone::Battlefield,
        p(owner),
        chars(
            &format!("test.flier_{power}_{toughness}"),
            &["Creature"],
            ColorBitmask::NONE,
        ),
    );
    v.controller = Some(p(owner));
    v.power = Some(power);
    v.toughness = Some(toughness);
    v
}

/// A synthetic card in a zone with the given types.
fn card(id: u64, zone: Zone, owner: u64, types: &[&str]) -> ObjectView {
    ObjectView::new(
        o(id),
        zone,
        p(owner),
        chars(&format!("test.card_{id}"), types, ColorBitmask::NONE),
    )
}

/// The synthetic game the interpreter reads.
#[derive(Default)]
struct TestEnv {
    players: Vec<PlayerId>,
    active: Option<PlayerId>,
    turn: u64,
    objects: BTreeMap<ObjectId, ObjectView>,
    /// Extra ids reported by `objects_in` (duplicates or ids without information).
    extra_in: Vec<(Zone, ObjectId)>,
    /// 0: ascending ids; otherwise a deterministic permutation seeded by this value.
    shuffle: u64,
    lki: BTreeMap<ObjectId, ObjectView>,
    stats: Vec<(PlayerId, PlayerStat, i64)>,
    devotion: BTreeMap<(PlayerId, String), i64>,
    max_speed: BTreeSet<PlayerId>,
    history: History,
    attached: BTreeMap<ObjectId, ObjectId>,
    defending: BTreeMap<ObjectId, PlayerId>,
    memory: BTreeMap<ObjectId, Memory>,
    spells: Vec<(PlayerId, ObjectView)>,
    died: Vec<ObjectView>,
    mana: BTreeMap<(ObjectId, Option<String>), i64>,
    tokens: BTreeMap<(ObjectId, String), String>,
}

/// A local integer LCG (no `rand`).
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
        if n == 0 { 0 } else { self.next() % n }
    }
}

impl TestEnv {
    /// Players 1..=n in seat order, player 1 active, turn 2.
    fn with_players(n: u64) -> TestEnv {
        TestEnv {
            players: (1..=n).map(p).collect(),
            active: Some(p(1)),
            turn: 2,
            ..TestEnv::default()
        }
    }

    fn add(&mut self, v: ObjectView) {
        self.objects.insert(v.id, v);
    }

    /// Moves an object out of every zone, keeping its last known information.
    fn leave(&mut self, id: ObjectId) {
        if let Some(v) = self.objects.remove(&id) {
            self.lki.insert(id, v);
        }
    }
}

impl InterpEnv for TestEnv {
    fn players(&self) -> Vec<PlayerId> {
        self.players.clone()
    }
    fn active_player(&self) -> Option<PlayerId> {
        self.active
    }
    fn turn_number(&self) -> u64 {
        self.turn
    }
    fn objects_in(&self, zone: Zone) -> Vec<ObjectId> {
        let mut ids: Vec<ObjectId> = self
            .objects
            .values()
            .filter(|v| v.zone == zone)
            .map(|v| v.id)
            .chain(
                self.extra_in
                    .iter()
                    .filter(|(z, _)| *z == zone)
                    .map(|(_, id)| *id),
            )
            .collect();
        if self.shuffle != 0 {
            let mut rng = Lcg(self.shuffle);
            for i in (1..ids.len()).rev() {
                let j = usize::try_from(rng.below(u64::try_from(i + 1).unwrap())).unwrap();
                ids.swap(i, j);
            }
        }
        ids
    }
    fn object(&self, id: ObjectId) -> Option<ObjectView> {
        self.objects.get(&id).cloned()
    }
    fn lki(&self, id: ObjectId) -> Option<ObjectView> {
        self.lki.get(&id).cloned()
    }
    fn player_stat(&self, player: PlayerId, stat: &PlayerStat) -> Option<i64> {
        if !self.players.contains(&player) {
            return None;
        }
        Some(
            self.stats
                .iter()
                .find(|(pl, s, _)| *pl == player && s == stat)
                .map_or(0, |(_, _, n)| *n),
        )
    }
    fn devotion(&self, player: PlayerId, colors: &[String]) -> i64 {
        colors
            .iter()
            .map(|c| {
                self.devotion
                    .get(&(player, c.clone()))
                    .copied()
                    .unwrap_or(0)
            })
            .sum()
    }
    fn has_max_speed(&self, player: PlayerId) -> bool {
        self.max_speed.contains(&player)
    }
    fn history(&self) -> &History {
        &self.history
    }
    fn attached_to(&self, object: ObjectId) -> Option<ObjectId> {
        self.attached.get(&object).copied()
    }
    fn defending_player(&self, attacker: ObjectId) -> Option<PlayerId> {
        self.defending.get(&attacker).copied()
    }
    fn memory(&self, object: ObjectId) -> Option<&Memory> {
        self.memory.get(&object)
    }
    fn spells_cast_this_turn(&self) -> Vec<(PlayerId, ObjectView)> {
        self.spells.clone()
    }
    fn creatures_died_this_turn(&self) -> Vec<ObjectView> {
        self.died.clone()
    }
    fn mana_spent_to_cast(&self, spell: ObjectId, color: Option<&str>) -> i64 {
        self.mana
            .get(&(spell, color.map(str::to_owned)))
            .copied()
            .unwrap_or(0)
    }
    fn text_token_value(&self, source: ObjectId, token_id: &str) -> Option<String> {
        self.tokens.get(&(source, token_id.to_owned())).cloned()
    }
}

fn ctx(controller: u64) -> EvalContext {
    EvalContext::new(p(controller))
}

fn ctx_src(controller: u64, source: u64) -> EvalContext {
    let mut c = EvalContext::new(p(controller));
    c.source = Some(o(source));
    c
}

fn lit(n: i64) -> Box<ValueExpressionAST> {
    Box::new(ValueExpressionAST::Literal(n))
}

fn filter(zones: Option<Vec<Zone>>, predicate: Option<PredicateAST>) -> SelectorFilter {
    SelectorFilter {
        zones,
        count: None,
        ordering: None,
        predicate: predicate.map(Box::new),
    }
}

fn sel(f: SelectorFilter) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(f)
}

fn ids(list: &[u64]) -> Vec<ObjectId> {
    list.iter().map(|n| o(*n)).collect()
}

fn pref(r: ReferenceAST) -> PlayerSelectorAST {
    PlayerSelectorAST::Ref(r)
}

// ---------------------------------------------------------------------------------------------
// Context and constructors
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-003: `EvalContext::new` and `ObjectView::new` defaults.
#[test]
fn constructors_have_documented_defaults() {
    let c = EvalContext::new(p(3));
    assert_eq!(c.controller, p(3));
    assert_eq!(c.source, None);
    assert_eq!(c.x, None);
    assert_eq!(c.that_player, None);
    assert!(c.targets.is_empty() && c.chosen.is_empty());
    assert!(c.bindings.is_empty() && c.linked.is_empty());

    let v = ObjectView::new(
        o(1),
        Zone::Hand,
        p(2),
        chars("test.x", &[], ColorBitmask::NONE),
    );
    assert_eq!(
        (v.id, v.zone, v.owner, v.controller),
        (o(1), Zone::Hand, p(2), None)
    );
    assert_eq!(v.mana_value, 0);
    assert_eq!(
        (v.power, v.toughness, v.loyalty, v.defense),
        (None, None, None, None)
    );
    assert_eq!(v.timestamp, Timestamp::new(0));
    assert_eq!(v.zone_position, 0);
    assert!(
        !(v.is_token
            || v.face_down
            || v.tapped
            || v.attacking
            || v.blocking
            || v.entered_this_turn
            || v.was_cast)
    );
    assert!(v.keywords.is_empty() && v.designations.is_empty() && v.counters.is_empty());

    let r: Resolution<u8> = Resolution::default();
    assert!(r.items.is_empty() && r.ignored.is_empty());
    assert_eq!(InterpError::MissingX.to_string(), "X has no value");
    for e in [
        InterpError::UnboundVariable("$a".into()),
        InterpError::DivisionByZero,
        InterpError::UnknownTextToken("t".into()),
    ] {
        assert!(!e.to_string().is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------------------------

fn resolved(env: &TestEnv, c: &EvalContext, r: ReferenceAST) -> Resolution<Referent> {
    resolve_reference(env, c, &r).unwrap()
}

/// REQ-DSL-003, CR 608.2h, CR 113.7a: SELF uses current information, then LKI after it left.
#[test]
fn self_ref_current_then_lki_then_unresolved() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(10, 1, 2, 2));
    let c = ctx_src(1, 10);
    let me = vec![Referent::Object(o(10))];
    assert_eq!(resolved(&env, &c, ReferenceAST::SelfRef).items, me);
    env.leave(o(10));
    assert_eq!(resolved(&env, &c, ReferenceAST::SelfRef).items, me);
    env.lki.clear();
    let r = resolved(&env, &c, ReferenceAST::SelfRef);
    assert!(r.items.is_empty());
    assert_eq!(r.ignored, vec![Ignored::Unresolved(ReferenceAST::SelfRef)]);
    let r = resolved(&env, &ctx(1), ReferenceAST::SelfRef);
    assert_eq!(r.ignored, vec![Ignored::Unresolved(ReferenceAST::SelfRef)]);
}

/// REQ-DSL-003, CR 608.2h, CR 608.2b: SOURCE_LKI prefers LKI, falls back to current info.
#[test]
fn source_lki_variants() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(10, 1, 2, 2));
    let c = ctx_src(1, 10);
    let me = vec![Referent::Object(o(10))];
    assert_eq!(resolved(&env, &c, ReferenceAST::SourceLki).items, me);
    env.leave(o(10));
    assert_eq!(resolved(&env, &c, ReferenceAST::SourceLki).items, me);
    env.lki.clear();
    let r = resolved(&env, &c, ReferenceAST::SourceLki);
    assert_eq!(
        r.ignored,
        vec![Ignored::Unresolved(ReferenceAST::SourceLki)]
    );
    let r = resolved(&env, &ctx(1), ReferenceAST::SourceLki);
    assert_eq!(
        r.ignored,
        vec![Ignored::Unresolved(ReferenceAST::SourceLki)]
    );
}

/// REQ-DSL-003, REQ-ARCH-002: CONTROLLER, and a controller no longer in the game is gone.
#[test]
fn controller_reference() {
    let env = TestEnv::with_players(2);
    assert_eq!(
        resolved(&env, &ctx(2), ReferenceAST::Controller).items,
        vec![Referent::Player(p(2))]
    );
    let r = resolved(&env, &ctx(9), ReferenceAST::Controller);
    assert!(r.items.is_empty());
    assert_eq!(r.ignored, vec![Ignored::Gone(Referent::Player(p(9)))]);
}

/// REQ-DSL-003, CR 608.2h: OWNER of the source, read from LKI once it left.
#[test]
fn owner_reference() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(10, 2, 1, 1));
    let c = ctx_src(1, 10);
    let two = vec![Referent::Player(p(2))];
    assert_eq!(resolved(&env, &c, ReferenceAST::Owner).items, two);
    env.leave(o(10));
    assert_eq!(resolved(&env, &c, ReferenceAST::Owner).items, two);
    env.lki.clear();
    assert_eq!(
        resolved(&env, &c, ReferenceAST::Owner).ignored,
        vec![Ignored::Unresolved(ReferenceAST::Owner)]
    );
}

/// REQ-DSL-003: ACTIVE_PLAYER, DEFENDING_PLAYER and THAT_PLAYER, present and absent.
#[test]
fn player_references_present_and_absent() {
    let mut env = TestEnv::with_players(3);
    env.active = Some(p(2));
    env.add(creature(10, 1, 1, 1));
    env.defending.insert(o(10), p(3));
    let mut c = ctx_src(1, 10);
    c.that_player = Some(p(3));
    let pl = |n| vec![Referent::Player(p(n))];
    assert_eq!(resolved(&env, &c, ReferenceAST::ActivePlayer).items, pl(2));
    assert_eq!(
        resolved(&env, &c, ReferenceAST::DefendingPlayer).items,
        pl(3)
    );
    assert_eq!(resolved(&env, &c, ReferenceAST::ThatPlayer).items, pl(3));

    env.active = None;
    env.defending.clear();
    let c2 = ctx(1);
    for r in [ReferenceAST::ActivePlayer, ReferenceAST::DefendingPlayer] {
        assert_eq!(
            resolved(&env, &c, r.clone()).ignored,
            vec![Ignored::Unresolved(r.clone())]
        );
    }
    for r in [
        ReferenceAST::ActivePlayer,
        ReferenceAST::DefendingPlayer,
        ReferenceAST::ThatPlayer,
    ] {
        assert_eq!(
            resolved(&env, &c2, r.clone()).ignored,
            vec![Ignored::Unresolved(r)]
        );
    }
}

/// REQ-DSL-003, REQ-ARCH-002, CR 101.3: TARGET slots: chosen, None, out of range, gone; the
/// other targets are still returned.
#[test]
fn target_slots_and_gone_targets() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(10, 1, 1, 1));
    env.add(creature(11, 2, 1, 1));
    let mut c = ctx(1);
    c.targets = vec![
        Some(Referent::Object(o(10))),
        None,
        Some(Referent::Object(o(11))),
        Some(Referent::Player(p(2))),
        Some(Referent::Player(p(7))),
    ];
    let t = |slot| ReferenceAST::Target { slot };
    assert_eq!(
        resolved(&env, &c, t(0)).items,
        vec![Referent::Object(o(10))]
    );
    assert_eq!(
        resolved(&env, &c, t(1)).ignored,
        vec![Ignored::Unresolved(t(1))]
    );
    assert_eq!(
        resolved(&env, &c, t(9)).ignored,
        vec![Ignored::Unresolved(t(9))]
    );
    assert_eq!(resolved(&env, &c, t(3)).items, vec![Referent::Player(p(2))]);
    assert_eq!(
        resolved(&env, &c, t(4)).ignored,
        vec![Ignored::Gone(Referent::Player(p(7)))]
    );
    // The first target left (its LKI does not make it a legal target again); the second is
    // still returned.
    env.leave(o(10));
    let r0 = resolved(&env, &c, t(0));
    assert!(r0.items.is_empty());
    assert_eq!(r0.ignored, vec![Ignored::Gone(Referent::Object(o(10)))]);
    assert_eq!(
        resolved(&env, &c, t(2)).items,
        vec![Referent::Object(o(11))]
    );
    let parts = partition_possible([t(0), t(2)], |r| {
        !resolve_reference(&env, &c, r).unwrap().items.is_empty()
    });
    assert_eq!(parts.performed, vec![t(2)]);
    assert_eq!(parts.ignored, vec![t(0)]);
}

/// REQ-DSL-003: CHOSEN of an object, a player, a number, an absent key and a gone object.
#[test]
fn chosen_values() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(10, 1, 1, 1));
    let mut c = ctx(1);
    c.chosen.insert("obj".into(), ChosenValue::Object(o(10)));
    c.chosen.insert("pl".into(), ChosenValue::Player(p(2)));
    c.chosen.insert("num".into(), ChosenValue::Number(4));
    c.chosen.insert("gone".into(), ChosenValue::Object(o(99)));
    let ch = |k: &str| ReferenceAST::Chosen { key: k.into() };
    assert_eq!(
        resolved(&env, &c, ch("obj")).items,
        vec![Referent::Object(o(10))]
    );
    assert_eq!(
        resolved(&env, &c, ch("pl")).items,
        vec![Referent::Player(p(2))]
    );
    assert_eq!(
        resolved(&env, &c, ch("num")).ignored,
        vec![Ignored::Unresolved(ch("num"))]
    );
    assert_eq!(
        resolved(&env, &c, ch("none")).ignored,
        vec![Ignored::Unresolved(ch("none"))]
    );
    assert_eq!(
        resolved(&env, &c, ch("gone")).ignored,
        vec![Ignored::Gone(Referent::Object(o(99)))]
    );
}

/// REQ-DSL-003, REQ-ARCH-002: BOUND keeps order and drops gone referents; unbound is an error.
#[test]
fn bound_variables() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(10, 1, 1, 1));
    env.add(creature(11, 1, 1, 1));
    let mut c = ctx(1);
    c.bindings.insert(
        "$it".into(),
        vec![
            Referent::Object(o(11)),
            Referent::Object(o(50)),
            Referent::Object(o(10)),
            Referent::Player(p(2)),
        ],
    );
    let r = resolved(&env, &c, ReferenceAST::Bound { var: "$it".into() });
    assert_eq!(
        r.items,
        vec![
            Referent::Object(o(11)),
            Referent::Object(o(10)),
            Referent::Player(p(2))
        ]
    );
    assert_eq!(r.ignored, vec![Ignored::Gone(Referent::Object(o(50)))]);
    assert_eq!(
        resolve_reference(&env, &c, &ReferenceAST::Bound { var: "$x".into() }),
        Err(InterpError::UnboundVariable("$x".into()))
    );
}

/// REQ-DSL-003: LINKED in order; absent or empty is unresolved.
#[test]
fn linked_objects() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(10, 1, 1, 1));
    env.add(creature(12, 1, 1, 1));
    let mut c = ctx(1);
    c.linked.insert(
        "L".into(),
        vec![Referent::Object(o(12)), Referent::Object(o(10))],
    );
    c.linked.insert("E".into(), vec![]);
    let l = |k: &str| ReferenceAST::Linked { link_id: k.into() };
    assert_eq!(
        resolved(&env, &c, l("L")).items,
        vec![Referent::Object(o(12)), Referent::Object(o(10))]
    );
    assert_eq!(
        resolved(&env, &c, l("E")).ignored,
        vec![Ignored::Unresolved(l("E"))]
    );
    assert_eq!(
        resolved(&env, &c, l("Z")).ignored,
        vec![Ignored::Unresolved(l("Z"))]
    );
}

/// REQ-DSL-003: ENCHANTED, EQUIPPED and ATTACHED_TO are the source's host.
#[test]
fn attachment_references() {
    let mut env = TestEnv::with_players(2);
    env.add(card(5, Zone::Battlefield, 1, &["Artifact"]));
    env.add(creature(10, 1, 1, 1));
    env.attached.insert(o(5), o(10));
    for r in [
        ReferenceAST::Enchanted,
        ReferenceAST::Equipped,
        ReferenceAST::AttachedTo,
    ] {
        assert_eq!(
            resolved(&env, &ctx_src(1, 5), r.clone()).items,
            vec![Referent::Object(o(10))]
        );
        assert_eq!(
            resolved(&env, &ctx(1), r.clone()).ignored,
            vec![Ignored::Unresolved(r.clone())]
        );
        assert_eq!(
            resolved(&env, &ctx_src(1, 10), r.clone()).ignored,
            vec![Ignored::Unresolved(r)]
        );
    }
    env.leave(o(10));
    assert_eq!(
        resolved(&env, &ctx_src(1, 5), ReferenceAST::Enchanted).ignored,
        vec![Ignored::Gone(Referent::Object(o(10)))]
    );
}

/// REQ-DSL-003: REMEMBERED reads a remembered player from the source's Memory.
#[test]
fn remembered_player() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(10, 1, 1, 1));
    let mut m = Memory::new();
    m.set_choice("who", ChoiceValue::Player(p(2)));
    m.set_choice("num", ChoiceValue::Number(3));
    env.memory.insert(o(10), m);
    let rm = |k: &str| ReferenceAST::Remembered { key: k.into() };
    let c = ctx_src(1, 10);
    assert_eq!(
        resolved(&env, &c, rm("who")).items,
        vec![Referent::Player(p(2))]
    );
    assert_eq!(
        resolved(&env, &c, rm("num")).ignored,
        vec![Ignored::Unresolved(rm("num"))]
    );
    assert_eq!(
        resolved(&env, &c, rm("zzz")).ignored,
        vec![Ignored::Unresolved(rm("zzz"))]
    );
    assert_eq!(
        resolved(&env, &ctx(1), rm("who")).ignored,
        vec![Ignored::Unresolved(rm("who"))]
    );
}

/// REQ-DSL-003, CR 101.4: EACH_OPPONENT and ALL_PLAYERS in APNAP order, with two and three
/// players; no rotation without an active player.
#[test]
fn apnap_order_each_opponent_and_all_players() {
    let all = PlayerSelectorAST::Players(PlayerGroup::AllPlayers);
    let pls = |list: &[u64]| {
        list.iter()
            .map(|n| Referent::Player(p(*n)))
            .collect::<Vec<_>>()
    };
    let mut env = TestEnv::with_players(3);
    env.active = Some(p(2));
    assert_eq!(
        resolve_players(&env, &ctx(1), &all).unwrap().items,
        vec![p(2), p(3), p(1)]
    );
    assert_eq!(
        resolved(&env, &ctx(1), ReferenceAST::EachOpponent).items,
        pls(&[2, 3])
    );
    env.active = Some(p(3));
    assert_eq!(
        resolved(&env, &ctx(2), ReferenceAST::EachOpponent).items,
        pls(&[3, 1])
    );
    env.active = None;
    assert_eq!(
        resolve_players(&env, &ctx(1), &all).unwrap().items,
        vec![p(1), p(2), p(3)]
    );

    let mut env2 = TestEnv::with_players(2);
    env2.active = Some(p(2));
    assert_eq!(
        resolve_players(&env2, &ctx(1), &all).unwrap().items,
        vec![p(2), p(1)]
    );
    let r = resolved(&env2, &ctx(2), ReferenceAST::EachOpponent);
    assert_eq!(r.items, pls(&[1]));
    assert!(r.ignored.is_empty());
    let solo = TestEnv::with_players(1);
    let r = resolved(&solo, &ctx(1), ReferenceAST::EachOpponent);
    assert!(r.items.is_empty() && r.ignored.is_empty());
}

/// REQ-DSL-003, REQ-ARCH-002: a player selector drops object referents as unresolved;
/// TEAMMATES is empty.
#[test]
fn resolve_players_ref_and_groups() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(10, 1, 1, 1));
    let s = pref(ReferenceAST::SelfRef);
    let r = resolve_players(&env, &ctx_src(1, 10), &s).unwrap();
    assert!(r.items.is_empty());
    assert_eq!(r.ignored, vec![Ignored::Unresolved(ReferenceAST::SelfRef)]);
    let r = resolve_players(&env, &ctx(2), &pref(ReferenceAST::Controller)).unwrap();
    assert_eq!(r.items, vec![p(2)]);
    let teammates = PlayerSelectorAST::Players(PlayerGroup::Teammates);
    let r = resolve_players(&env, &ctx(1), &teammates).unwrap();
    assert!(r.items.is_empty() && r.ignored.is_empty());
    let unbound = pref(ReferenceAST::Bound { var: "$q".into() });
    assert_eq!(
        resolve_players(&env, &ctx(1), &unbound),
        Err(InterpError::UnboundVariable("$q".into()))
    );
}

// ---------------------------------------------------------------------------------------------
// Selectors
// ---------------------------------------------------------------------------------------------

fn select(env: &TestEnv, c: &EvalContext, s: &ObjectSelectorAST) -> Vec<ObjectId> {
    select_objects(env, c, s).unwrap().items
}

fn zoo() -> TestEnv {
    let mut env = TestEnv::with_players(2);
    env.add(creature(1, 1, 2, 2));
    env.add(creature(2, 2, 3, 1));
    let mut land = card(3, Zone::Battlefield, 1, &["Land"]);
    land.controller = Some(p(1));
    env.add(land);
    env.add(card(4, Zone::Graveyard, 1, &["Creature"]));
    env.add(card(5, Zone::Hand, 2, &["Instant"]));
    env
}

/// REQ-DSL-003: default zone is the battlefield; multiple zones; duplicates and ids without
/// information are skipped.
#[test]
fn selector_zones() {
    let mut env = zoo();
    let c = ctx(1);
    assert_eq!(select(&env, &c, &sel(filter(None, None))), ids(&[1, 2, 3]));
    let two = || sel(filter(Some(vec![Zone::Hand, Zone::Graveyard]), None));
    assert_eq!(select(&env, &c, &two()), ids(&[4, 5]));
    env.extra_in.push((Zone::Graveyard, o(4)));
    env.extra_in.push((Zone::Hand, o(4)));
    env.extra_in.push((Zone::Hand, o(77)));
    assert_eq!(select(&env, &c, &two()), ids(&[4, 5]));
}

/// REQ-DSL-003: predicate trees AND / OR / NOT in a selector.
#[test]
fn selector_predicate_trees() {
    let env = zoo();
    let c = ctx(1);
    let mine = PredicateAST::ControlledBy {
        player: pref(ReferenceAST::Controller),
    };
    let and = PredicateAST::And(vec![PredicateAST::IsCreature, mine]);
    assert_eq!(select(&env, &c, &sel(filter(None, Some(and)))), ids(&[1]));
    let or = PredicateAST::Or(vec![
        PredicateAST::IsLand,
        PredicateAST::Power {
            cmp: Cmp::Ge,
            value: lit(3),
        },
    ]);
    assert_eq!(select(&env, &c, &sel(filter(None, Some(or)))), ids(&[2, 3]));
    let not = PredicateAST::Not(Box::new(PredicateAST::IsCreature));
    assert_eq!(select(&env, &c, &sel(filter(None, Some(not)))), ids(&[3]));
    // An error inside the predicate propagates.
    let bad = PredicateAST::Power {
        cmp: Cmp::Eq,
        value: Box::new(ValueExpressionAST::X),
    };
    assert_eq!(
        select_objects(&env, &c, &sel(filter(None, Some(bad)))),
        Err(InterpError::MissingX)
    );
}

/// id, timestamp, mana value, power, toughness, zone position.
type OrderRow = (u64, u64, i64, Option<i64>, Option<i64>, u32);

/// REQ-DSL-003: ordering by every key, ascending and descending, ties broken by ascending
/// ObjectId under both directions, independent of the `objects_in` order.
#[test]
fn selector_ordering_every_key() {
    let mut env = TestEnv::with_players(2);
    let rows: [OrderRow; 4] = [
        (1, 30, 2, Some(1), None, 2),
        (2, 10, 5, None, Some(3), 0),
        (3, 30, 2, Some(4), Some(3), 1),
        (4, 20, 1, Some(1), Some(0), 2),
    ];
    for (id, ts, mv, pw, tg, pos) in rows {
        let mut v = card(id, Zone::Battlefield, 1, &["Creature"]);
        v.timestamp = Timestamp::new(ts);
        v.mana_value = mv;
        v.power = pw;
        v.toughness = tg;
        v.zone_position = pos;
        env.add(v);
    }
    let cases: &[(OrderKey, [u64; 4], [u64; 4])] = &[
        (OrderKey::Timestamp, [2, 4, 1, 3], [1, 3, 4, 2]),
        (OrderKey::ManaValue, [4, 1, 3, 2], [2, 1, 3, 4]),
        (OrderKey::Power, [2, 1, 4, 3], [3, 1, 4, 2]),
        (OrderKey::Toughness, [1, 4, 2, 3], [2, 3, 4, 1]),
        (OrderKey::ZonePosition, [2, 3, 1, 4], [1, 4, 3, 2]),
    ];
    for (by, asc, desc) in cases {
        for (direction, want) in [(Direction::Asc, asc), (Direction::Desc, desc)] {
            for shuffle in [0, 1, 7, 42] {
                env.shuffle = shuffle;
                let mut f = filter(None, None);
                f.ordering = Some(Ordering { by: *by, direction });
                assert_eq!(
                    select(&env, &ctx(1), &sel(f)),
                    ids(want),
                    "{by:?} {direction:?}"
                );
            }
        }
    }
}

/// REQ-DSL-003: count 0, negative, equal to and larger than the candidates, and X.
#[test]
fn selector_count() {
    let mut env = TestEnv::with_players(2);
    for id in 1..=4 {
        env.add(creature(id, 1, 1, 1));
    }
    let with_count = |v: ValueExpressionAST| {
        let mut f = filter(None, None);
        f.count = Some(Box::new(v));
        sel(f)
    };
    let c = ctx(1);
    let n = ValueExpressionAST::Literal;
    assert!(select(&env, &c, &with_count(n(0))).is_empty());
    assert!(select(&env, &c, &with_count(n(-3))).is_empty());
    assert_eq!(select(&env, &c, &with_count(n(2))), ids(&[1, 2]));
    assert_eq!(select(&env, &c, &with_count(n(4))), ids(&[1, 2, 3, 4]));
    assert_eq!(
        select(&env, &c, &with_count(n(i64::MAX))),
        ids(&[1, 2, 3, 4])
    );
    let mut cx = ctx(1);
    cx.x = Some(3);
    assert_eq!(
        select(&env, &cx, &with_count(ValueExpressionAST::X)),
        ids(&[1, 2, 3])
    );
    assert_eq!(
        select_objects(&env, &c, &with_count(ValueExpressionAST::X)),
        Err(InterpError::MissingX)
    );
}

/// REQ-DSL-003, REQ-ARCH-002: reference-form selectors: objects kept, player referents
/// unresolved, gone objects ignored.
#[test]
fn selector_reference_form() {
    let mut env = zoo();
    let s = ObjectSelectorAST::Ref(ReferenceAST::Controller);
    let r = select_objects(&env, &ctx(1), &s).unwrap();
    assert!(r.items.is_empty());
    assert_eq!(
        r.ignored,
        vec![Ignored::Unresolved(ReferenceAST::Controller)]
    );
    let mut c = ctx(1);
    c.targets = vec![Some(Referent::Object(o(2))), Some(Referent::Object(o(1)))];
    let t0 = ObjectSelectorAST::Ref(ReferenceAST::Target { slot: 0 });
    assert_eq!(select(&env, &c, &t0), ids(&[2]));
    env.leave(o(2));
    let r = select_objects(&env, &c, &t0).unwrap();
    assert!(r.items.is_empty());
    assert_eq!(r.ignored, vec![Ignored::Gone(Referent::Object(o(2)))]);
}

// ---------------------------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------------------------

fn pred_obj(env: &TestEnv, c: &EvalContext, pr: &PredicateAST, s: &ObjectView) -> bool {
    eval_predicate_object(env, c, pr, s).unwrap()
}

fn pred_pl(env: &TestEnv, c: &EvalContext, pr: &PredicateAST, s: u64) -> bool {
    eval_predicate_player(env, c, pr, p(s)).unwrap()
}

/// REQ-DSL-003: every object-only predicate, true and false on an object, false on a player.
#[test]
fn object_flag_predicates_table() {
    let env = TestEnv::with_players(2);
    let c = ctx(1);
    let base = card(1, Zone::Graveyard, 1, &["Instant"]);
    let mut yes = card(2, Zone::Battlefield, 1, &["Creature", "Land"]);
    yes.characteristics.colors = cols(1 | 2);
    yes.characteristics.subtypes.insert("Elf".into());
    yes.characteristics.supertypes.insert("Basic".into());
    yes.is_token = true;
    yes.face_down = true;
    yes.tapped = true;
    yes.attacking = true;
    yes.blocking = true;
    yes.entered_this_turn = true;
    yes.was_cast = true;
    yes.keywords.insert("Flying".into());
    yes.designations.insert("monarch_marker".into());
    yes.counters.insert("+1/+1".into(), 2);
    let name = |s: &str| WordRef::Name(s.into());
    let counter = |compare| PredicateAST::HasCounter {
        kind: "+1/+1".into(),
        compare,
    };
    // (predicate, holds on `yes`, holds on `base`)
    let table: Vec<(PredicateAST, bool, bool)> = vec![
        (PredicateAST::IsCreature, true, false),
        (PredicateAST::IsLand, true, false),
        (PredicateAST::IsPermanent, true, false),
        (PredicateAST::IsToken, true, false),
        (PredicateAST::IsColorless, false, true),
        (PredicateAST::IsMulticolored, true, false),
        (PredicateAST::FaceUp, false, true),
        (PredicateAST::FaceDown, true, false),
        (PredicateAST::Tapped, true, false),
        (PredicateAST::Untapped, false, true),
        (PredicateAST::Attacking, true, false),
        (PredicateAST::Blocking, true, false),
        (PredicateAST::EnteredThisTurn, true, false),
        (PredicateAST::WasCast, true, false),
        (
            PredicateAST::HasCardType {
                value: name("Creature"),
            },
            true,
            false,
        ),
        (
            PredicateAST::HasCardType {
                value: name("creature"),
            },
            false,
            false,
        ),
        (PredicateAST::HasSubtype { value: name("Elf") }, true, false),
        (
            PredicateAST::HasSupertype {
                value: name("Basic"),
            },
            true,
            false,
        ),
        (PredicateAST::HasColor { value: name("W") }, true, false),
        (
            PredicateAST::HasColor {
                value: name("blue"),
            },
            true,
            false,
        ),
        (
            PredicateAST::HasColor {
                value: name("Blue"),
            },
            true,
            false,
        ),
        (PredicateAST::HasColor { value: name("u") }, true, false),
        (PredicateAST::HasColor { value: name("G") }, false, false),
        (
            PredicateAST::HasColor {
                value: name("purple"),
            },
            false,
            false,
        ),
        (
            PredicateAST::HasKeyword {
                keyword: "Flying".into(),
            },
            true,
            false,
        ),
        (
            PredicateAST::HasDesignation {
                kind: "monarch_marker".into(),
            },
            true,
            false,
        ),
        (counter(None), true, false),
        (counter(Some((Cmp::Ge, lit(2)))), true, false),
        (counter(Some((Cmp::Eq, lit(0)))), false, true),
    ];
    for (pr, on_yes, on_base) in &table {
        assert_eq!(pred_obj(&env, &c, pr, &yes), *on_yes, "{pr:?} on yes");
        assert_eq!(pred_obj(&env, &c, pr, &base), *on_base, "{pr:?} on base");
        assert!(!pred_pl(&env, &c, pr, 1), "{pr:?} on a player");
    }
    let mut red = base.clone();
    red.characteristics.colors = ColorBitmask::RED;
    for (word, holds) in [("r", true), ("RED", true), ("black", false), ("B", false)] {
        let pr = PredicateAST::HasColor { value: name(word) };
        assert_eq!(pred_obj(&env, &c, &pr, &red), holds, "{word}");
    }
    assert!(!pred_obj(&env, &c, &PredicateAST::IsMulticolored, &red));
    assert!(!pred_obj(&env, &c, &PredicateAST::IsColorless, &red));
}

/// REQ-DSL-003: IS_PLAYER and IS_CARD_OR_TOKEN.
#[test]
fn subject_kind_predicates() {
    let env = TestEnv::with_players(2);
    let c = ctx(1);
    let v = creature(1, 1, 1, 1);
    assert!(!pred_obj(&env, &c, &PredicateAST::IsPlayer, &v));
    assert!(pred_pl(&env, &c, &PredicateAST::IsPlayer, 1));
    assert!(pred_obj(&env, &c, &PredicateAST::IsCardOrToken, &v));
    assert!(!pred_pl(&env, &c, &PredicateAST::IsCardOrToken, 1));
}

/// REQ-DSL-003: AND / OR / NOT on objects and players, short-circuit and error propagation.
#[test]
fn boolean_predicates() {
    let env = TestEnv::with_players(2);
    let c = ctx(1);
    let v = creature(1, 1, 1, 1);
    let err = PredicateAST::Compare {
        cmp: Cmp::Eq,
        lhs: Box::new(ValueExpressionAST::X),
        rhs: lit(0),
    };
    let t = PredicateAST::IsCardOrToken;
    let f = PredicateAST::IsLand;
    let and = |v: Vec<PredicateAST>| PredicateAST::And(v);
    let or = |v: Vec<PredicateAST>| PredicateAST::Or(v);
    assert!(pred_obj(
        &env,
        &c,
        &and(vec![t.clone(), PredicateAST::IsCreature]),
        &v
    ));
    assert!(!pred_obj(&env, &c, &and(vec![f.clone(), err.clone()]), &v));
    assert!(pred_obj(&env, &c, &or(vec![t.clone(), err.clone()]), &v));
    assert!(!pred_obj(
        &env,
        &c,
        &or(vec![f.clone(), PredicateAST::IsToken]),
        &v
    ));
    assert!(pred_obj(
        &env,
        &c,
        &PredicateAST::Not(Box::new(f.clone())),
        &v
    ));
    assert_eq!(
        eval_predicate_object(&env, &c, &and(vec![t.clone(), err.clone()]), &v),
        Err(InterpError::MissingX)
    );
    assert_eq!(
        eval_predicate_player(&env, &c, &or(vec![f.clone(), err.clone()]), p(1)),
        Err(InterpError::MissingX)
    );
    assert_eq!(
        eval_predicate_player(&env, &c, &PredicateAST::Not(Box::new(err)), p(1)),
        Err(InterpError::MissingX)
    );
    assert!(pred_pl(
        &env,
        &c,
        &PredicateAST::Not(Box::new(t.clone())),
        1
    ));
    assert!(pred_pl(&env, &c, &and(vec![PredicateAST::IsPlayer]), 1));
    assert!(!pred_pl(&env, &c, &or(vec![t, f]), 1));
}

/// REQ-DSL-003: CONTROLLED_BY / OWNED_BY, incl. a controller-less object.
#[test]
fn controlled_and_owned_by() {
    let env = TestEnv::with_players(2);
    let c = ctx(1);
    let mut v = creature(1, 1, 1, 1);
    v.controller = Some(p(2));
    let ctl = |r| PredicateAST::ControlledBy { player: pref(r) };
    let own = |r| PredicateAST::OwnedBy { player: pref(r) };
    assert!(!pred_obj(&env, &c, &ctl(ReferenceAST::Controller), &v));
    assert!(pred_obj(&env, &c, &ctl(ReferenceAST::EachOpponent), &v));
    assert!(pred_obj(&env, &c, &own(ReferenceAST::Controller), &v));
    assert!(!pred_obj(&env, &c, &own(ReferenceAST::EachOpponent), &v));
    let gy = card(3, Zone::Graveyard, 1, &["Creature"]);
    let all = PredicateAST::ControlledBy {
        player: PlayerSelectorAST::Players(PlayerGroup::AllPlayers),
    };
    assert!(!pred_obj(&env, &c, &all, &gy));
    assert!(!pred_pl(&env, &c, &all, 1));
    assert!(!pred_pl(&env, &c, &own(ReferenceAST::Controller), 1));
}

/// REQ-DSL-003: MANA_VALUE / POWER / TOUGHNESS comparisons; missing P/T is false.
#[test]
fn numeric_characteristic_predicates() {
    let env = TestEnv::with_players(2);
    let c = ctx(1);
    let mut v = creature(1, 1, 3, 4);
    v.mana_value = 5;
    let gy = card(2, Zone::Graveyard, 1, &["Sorcery"]);
    let mv = |cmp, n| PredicateAST::ManaValue { cmp, value: lit(n) };
    let pw = |cmp, n| PredicateAST::Power { cmp, value: lit(n) };
    let tg = |cmp, n| PredicateAST::Toughness { cmp, value: lit(n) };
    assert!(pred_obj(&env, &c, &mv(Cmp::Eq, 5), &v));
    assert!(!pred_obj(&env, &c, &mv(Cmp::Lt, 5), &v));
    assert!(pred_obj(&env, &c, &pw(Cmp::Le, 3), &v));
    assert!(!pred_obj(&env, &c, &pw(Cmp::Gt, 3), &v));
    assert!(pred_obj(&env, &c, &tg(Cmp::Ne, 3), &v));
    assert!(!pred_obj(&env, &c, &tg(Cmp::Lt, 4), &v));
    assert!(!pred_obj(&env, &c, &pw(Cmp::Ge, -100), &gy));
    assert!(!pred_obj(&env, &c, &tg(Cmp::Ge, -100), &gy));
    assert!(pred_obj(&env, &c, &mv(Cmp::Eq, 0), &gy));
    for pr in [mv(Cmp::Ge, 0), pw(Cmp::Ge, 0), tg(Cmp::Ge, 0)] {
        assert!(!pred_pl(&env, &c, &pr, 1));
    }
}

/// REQ-DSL-003: SAME_AS for objects and players.
#[test]
fn same_as_predicate() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(1, 1, 1, 1));
    env.add(creature(2, 1, 1, 1));
    let mut c = ctx(1);
    c.targets = vec![Some(Referent::Object(o(1))), Some(Referent::Player(p(2)))];
    let same = |slot| PredicateAST::SameAs {
        reference: ReferenceAST::Target { slot },
    };
    assert!(pred_obj(&env, &c, &same(0), &env.objects[&o(1)]));
    assert!(!pred_obj(&env, &c, &same(0), &env.objects[&o(2)]));
    assert!(pred_pl(&env, &c, &same(1), 2));
    assert!(!pred_pl(&env, &c, &same(1), 1));
    assert!(!pred_pl(&env, &c, &same(5), 1));
}

/// REQ-DSL-003: CONTROLS with and without a player, on an object and on a player.
#[test]
fn controls_predicate() {
    let env = zoo();
    let c = ctx(1);
    let lands = Box::new(sel(filter(None, Some(PredicateAST::IsLand))));
    let opp_controls = PredicateAST::Controls {
        selector: lands.clone(),
        player: Some(pref(ReferenceAST::EachOpponent)),
    };
    let controls = PredicateAST::Controls {
        selector: lands,
        player: None,
    };
    let v = creature(9, 2, 1, 1);
    assert!(!pred_obj(&env, &c, &opp_controls, &v));
    assert!(pred_obj(&env, &c, &controls, &v));
    assert!(!pred_obj(&env, &ctx(2), &controls, &v));
    assert!(pred_pl(&env, &c, &controls, 1));
    assert!(!pred_pl(&env, &c, &controls, 2));
    assert!(!pred_pl(&env, &c, &opp_controls, 1));
}

/// REQ-DSL-003: PLAYER_HAS_MAX_SPEED and COMPARE are independent of the subject.
#[test]
fn speed_and_compare_predicates() {
    let mut env = TestEnv::with_players(2);
    env.max_speed.insert(p(1));
    let c = ctx(1);
    let v = creature(1, 1, 1, 1);
    let me = PredicateAST::PlayerHasMaxSpeed {
        player: pref(ReferenceAST::Controller),
    };
    let all = PredicateAST::PlayerHasMaxSpeed {
        player: PlayerSelectorAST::Players(PlayerGroup::AllPlayers),
    };
    let none = PredicateAST::PlayerHasMaxSpeed {
        player: PlayerSelectorAST::Players(PlayerGroup::Teammates),
    };
    assert!(pred_obj(&env, &c, &me, &v) && pred_pl(&env, &c, &me, 2));
    assert!(!pred_obj(&env, &c, &all, &v) && !pred_pl(&env, &c, &all, 1));
    assert!(!pred_pl(&env, &c, &none, 1));
    env.max_speed.insert(p(2));
    assert!(pred_pl(&env, &c, &all, 1));
    let cmp = |cmp, l, r| PredicateAST::Compare {
        cmp,
        lhs: lit(l),
        rhs: lit(r),
    };
    assert!(pred_obj(&env, &c, &cmp(Cmp::Lt, 1, 2), &v));
    assert!(pred_pl(&env, &c, &cmp(Cmp::Lt, 1, 2), 2));
    assert!(!pred_pl(&env, &c, &cmp(Cmp::Gt, 1, 2), 1));
    assert!(!pred_obj(&env, &c, &cmp(Cmp::Gt, 1, 2), &v));
}

/// REQ-DSL-003, SRS §14.5: TEXT_TOKEN_MATCH and text-token words read the CURRENT value.
#[test]
fn text_token_words() {
    let mut env = TestEnv::with_players(2);
    let mut v = creature(1, 1, 1, 1);
    v.characteristics.subtypes.insert("Elf".into());
    v.characteristics.colors = ColorBitmask::GREEN;
    env.add(v.clone());
    env.tokens.insert((o(1), "t1".into()), "Elf".into());
    env.tokens.insert((o(1), "col".into()), "green".into());
    let c = ctx_src(1, 1);
    let m = |v: &str| PredicateAST::TextTokenMatch {
        text_token: "t1".into(),
        value: v.into(),
    };
    let sub = PredicateAST::HasSubtype {
        value: WordRef::TextToken("t1".into()),
    };
    let col = PredicateAST::HasColor {
        value: WordRef::TextToken("col".into()),
    };
    assert!(pred_obj(&env, &c, &m("Elf"), &v) && pred_pl(&env, &c, &m("Elf"), 1));
    assert!(pred_obj(&env, &c, &sub, &v));
    assert!(pred_obj(&env, &c, &col, &v));
    // A text-changing effect rewrote the tokens.
    env.tokens.insert((o(1), "t1".into()), "Goblin".into());
    env.tokens.insert((o(1), "col".into()), "red".into());
    assert!(!pred_obj(&env, &c, &m("Elf"), &v) && pred_obj(&env, &c, &m("Goblin"), &v));
    assert!(!pred_pl(&env, &c, &m("Elf"), 1));
    assert!(!pred_obj(&env, &c, &sub, &v));
    assert!(!pred_obj(&env, &c, &col, &v));
    // No source: TEXT_TOKEN_MATCH is false, a token word is an error.
    assert!(!pred_obj(&env, &ctx(1), &m("Elf"), &v));
    assert_eq!(
        eval_predicate_object(&env, &ctx(1), &sub, &v),
        Err(InterpError::UnknownTextToken("t1".into()))
    );
    let missing = PredicateAST::TextTokenMatch {
        text_token: "nope".into(),
        value: "x".into(),
    };
    assert_eq!(
        eval_predicate_player(&env, &c, &missing, p(1)),
        Err(InterpError::UnknownTextToken("nope".into()))
    );
    let ty = PredicateAST::HasCardType {
        value: WordRef::TextToken("nope".into()),
    };
    assert_eq!(
        eval_predicate_object(&env, &c, &ty, &v),
        Err(InterpError::UnknownTextToken("nope".into()))
    );
}

/// REQ-DSL-003: `compare` for every operator.
#[test]
fn compare_every_operator() {
    let rows = [
        (Cmp::Eq, [false, true, false]),
        (Cmp::Ne, [true, false, true]),
        (Cmp::Lt, [true, false, false]),
        (Cmp::Le, [true, true, false]),
        (Cmp::Gt, [false, false, true]),
        (Cmp::Ge, [false, true, true]),
    ];
    for (cmp, want) in rows {
        assert_eq!(
            [compare(cmp, 1, 2), compare(cmp, 2, 2), compare(cmp, 3, 2)],
            want
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------------------

fn val(env: &TestEnv, c: &EvalContext, v: &ValueExpressionAST) -> i64 {
    eval_value(env, c, v).unwrap()
}

/// REQ-DSL-003, REQ-CORE-006: LITERAL, X and COUNT.
#[test]
fn literal_x_count() {
    let env = zoo();
    let mut c = ctx(1);
    assert_eq!(val(&env, &c, &ValueExpressionAST::Literal(-7)), -7);
    assert_eq!(
        eval_value(&env, &c, &ValueExpressionAST::X),
        Err(InterpError::MissingX)
    );
    c.x = Some(4);
    assert_eq!(val(&env, &c, &ValueExpressionAST::X), 4);
    let creatures = ValueExpressionAST::Count(Box::new(sel(filter(
        Some(vec![Zone::Battlefield, Zone::Graveyard]),
        Some(PredicateAST::IsCreature),
    ))));
    assert_eq!(val(&env, &c, &creatures), 3);
}

/// REQ-DSL-003, CR 208.5, CR 608.2h: ATTR for every ObjectAttr, from LKI once gone; nothing
/// resolved is 0.
#[test]
fn attr_every_object_attr() {
    let mut env = TestEnv::with_players(2);
    let mut v = creature(1, 1, 3, 4);
    v.mana_value = 6;
    v.loyalty = Some(5);
    v.defense = Some(2);
    v.characteristics.colors = cols(4 | 8 | 16);
    v.counters.insert("charge".into(), 7);
    env.add(v);
    env.add(card(2, Zone::Battlefield, 1, &["Artifact"]));
    let c = ctx_src(1, 1);
    let at = |r: ReferenceAST, a: ObjectAttr| ValueExpressionAST::Attr {
        reference: r,
        attr: a,
    };
    let cases = [
        (ObjectAttr::Power, 3),
        (ObjectAttr::Toughness, 4),
        (ObjectAttr::ManaValue, 6),
        (ObjectAttr::Loyalty, 5),
        (ObjectAttr::Defense, 2),
        (ObjectAttr::ColorCount, 3),
        (ObjectAttr::Counters("charge".into()), 7),
        (ObjectAttr::Counters("other".into()), 0),
    ];
    for (attr, on_creature) in &cases {
        let me = at(ReferenceAST::SelfRef, attr.clone());
        assert_eq!(val(&env, &c, &me), *on_creature);
        // CR 208.5: missing values are 0 on the artifact.
        assert_eq!(val(&env, &ctx_src(1, 2), &me), 0);
        assert_eq!(val(&env, &ctx(1), &me), 0);
        assert_eq!(
            val(&env, &c, &at(ReferenceAST::Controller, attr.clone())),
            0
        );
    }
    env.leave(o(1));
    assert_eq!(
        val(&env, &c, &at(ReferenceAST::SelfRef, ObjectAttr::Power)),
        3
    );
    // A bound list sums; a gone member contributes 0.
    let mut cb = ctx(1);
    cb.bindings.insert(
        "$all".into(),
        vec![Referent::Object(o(2)), Referent::Object(o(1))],
    );
    env.objects.get_mut(&o(2)).unwrap().power = Some(10);
    let bound = at(
        ReferenceAST::Bound { var: "$all".into() },
        ObjectAttr::Power,
    );
    assert_eq!(val(&env, &cb, &bound), 10);
}

/// REQ-DSL-003: PLAYER_ATTR for every PlayerAttr, incl. devotion with a text-token colour.
#[test]
fn player_attr_every_attr() {
    let mut env = TestEnv::with_players(3);
    env.add(creature(1, 1, 1, 1));
    let stats = [
        (PlayerStat::Life, 20, 17),
        (PlayerStat::Speed, 2, 4),
        (PlayerStat::HandSize, 5, 1),
        (PlayerStat::LibrarySize, 40, 30),
        (PlayerStat::GraveyardSize, 3, 6),
        (PlayerStat::Counters("poison".into()), 1, 2),
    ];
    for (s, a, b) in &stats {
        env.stats.push((p(1), s.clone(), *a));
        env.stats.push((p(2), s.clone(), *b));
    }
    env.devotion.insert((p(1), "green".into()), 3);
    env.devotion.insert((p(1), "W".into()), 2);
    env.devotion.insert((p(2), "green".into()), 1);
    env.tokens.insert((o(1), "c".into()), "green".into());
    let c = ctx_src(1, 1);
    let pa = |r: ReferenceAST, a: PlayerAttr| ValueExpressionAST::PlayerAttr {
        reference: r,
        attr: a,
    };
    let attrs = [
        PlayerAttr::Life,
        PlayerAttr::Speed,
        PlayerAttr::HandSize,
        PlayerAttr::LibrarySize,
        PlayerAttr::GraveyardSize,
        PlayerAttr::Counters("poison".into()),
    ];
    for (attr, (_, a, b)) in attrs.iter().zip(stats.iter()) {
        assert_eq!(
            val(&env, &c, &pa(ReferenceAST::Controller, attr.clone())),
            *a
        );
        // Opponents 2 and 3 (3 has nothing recorded: 0).
        assert_eq!(
            val(&env, &c, &pa(ReferenceAST::EachOpponent, attr.clone())),
            *b
        );
        assert_eq!(val(&env, &c, &pa(ReferenceAST::SelfRef, attr.clone())), 0);
    }
    let dev = PlayerAttr::Devotion(vec![
        WordRef::TextToken("c".into()),
        WordRef::Name("W".into()),
    ]);
    assert_eq!(val(&env, &c, &pa(ReferenceAST::Controller, dev.clone())), 5);
    assert_eq!(
        val(&env, &c, &pa(ReferenceAST::EachOpponent, dev.clone())),
        1
    );
    env.tokens.insert((o(1), "c".into()), "blue".into());
    assert_eq!(val(&env, &c, &pa(ReferenceAST::Controller, dev.clone())), 2);
    assert_eq!(
        eval_value(&env, &ctx(1), &pa(ReferenceAST::Controller, dev)),
        Err(InterpError::UnknownTextToken("c".into()))
    );
    // A player not in the game is gone: 0.
    assert_eq!(
        val(
            &env,
            &ctx(9),
            &pa(ReferenceAST::Controller, PlayerAttr::Life)
        ),
        0
    );
}

fn math(op: MathOp, l: i64, r: i64) -> Result<i64, InterpError> {
    let env = TestEnv::with_players(1);
    eval_value(
        &env,
        &ctx(1),
        &ValueExpressionAST::Math {
            op,
            lhs: lit(l),
            rhs: lit(r),
        },
    )
}

/// REQ-DSL-003, REQ-CORE-006: MATH for every operator, floor/ceil with signs, division by
/// zero, saturation and `i64::MIN / -1`.
#[test]
fn math_every_operator() {
    use MathOp::*;
    let ok: &[(MathOp, i64, i64, i64)] = &[
        (Add, 2, 3, 5),
        (Sub, 2, 3, -1),
        (Mul, -2, 3, -6),
        (Min, 2, -3, -3),
        (Max, 2, -3, 2),
        (DivFloor, 7, 2, 3),
        (DivFloor, -7, 2, -4),
        (DivFloor, 7, -2, -4),
        (DivFloor, -7, -2, 3),
        (DivFloor, 6, 3, 2),
        (DivFloor, -6, 3, -2),
        (DivCeil, 7, 2, 4),
        (DivCeil, -7, 2, -3),
        (DivCeil, 7, -2, -3),
        (DivCeil, -7, -2, 4),
        (DivCeil, 6, -3, -2),
        (DivCeil, 0, 5, 0),
        (Add, i64::MAX, 1, i64::MAX),
        (Add, i64::MIN, -1, i64::MIN),
        (Sub, i64::MIN, 1, i64::MIN),
        (Sub, i64::MAX, -1, i64::MAX),
        (Mul, i64::MAX, 2, i64::MAX),
        (Mul, i64::MIN, 2, i64::MIN),
        (Mul, i64::MIN, -1, i64::MAX),
        (DivFloor, i64::MIN, -1, i64::MAX),
        (DivCeil, i64::MIN, -1, i64::MAX),
        (DivFloor, i64::MIN, 1, i64::MIN),
        (DivCeil, i64::MAX, 2, i64::MAX / 2 + 1),
        (DivFloor, i64::MIN, 2, i64::MIN / 2),
    ];
    for (op, l, r, want) in ok {
        assert_eq!(math(*op, *l, *r), Ok(*want), "{op:?} {l} {r}");
    }
    assert_eq!(math(DivFloor, 5, 0), Err(InterpError::DivisionByZero));
    assert_eq!(math(DivCeil, 0, 0), Err(InterpError::DivisionByZero));
    // The lhs error wins over the rhs error.
    let env = TestEnv::with_players(1);
    let v = ValueExpressionAST::Math {
        op: Add,
        lhs: Box::new(ValueExpressionAST::X),
        rhs: Box::new(ValueExpressionAST::Math {
            op: DivFloor,
            lhs: lit(1),
            rhs: lit(0),
        }),
    };
    assert_eq!(eval_value(&env, &ctx(1), &v), Err(InterpError::MissingX));
}

/// REQ-DSL-003: DISTINCT for every DistinctAttr, and a delirium-style "four or more card types
/// among cards in graveyards" check.
#[test]
fn distinct_every_attr_and_card_type_check() {
    let mut env = TestEnv::with_players(2);
    let mut a = card(1, Zone::Graveyard, 1, &["Creature", "Artifact"]);
    a.characteristics.subtypes = BTreeSet::from(["Elf".into(), "Golem".into()]);
    a.characteristics.supertypes = BTreeSet::from(["Legendary".into()]);
    a.characteristics.colors = cols(1 | 2);
    a.mana_value = 3;
    a.power = Some(2);
    a.toughness = Some(2);
    let mut b = card(2, Zone::Graveyard, 2, &["Land"]);
    b.characteristics.subtypes = BTreeSet::from(["Elf".into()]);
    b.characteristics.colors = cols(2 | 8);
    b.mana_value = 3;
    let mut d = card(3, Zone::Graveyard, 1, &["Creature"]);
    d.characteristics.names = BTreeSet::from(["test.card_1".into(), "test.other".into()]);
    d.mana_value = 1;
    d.power = Some(2);
    d.toughness = Some(5);
    env.add(a);
    env.add(b);
    env.add(d);
    env.add(card(4, Zone::Battlefield, 1, &["Enchantment"]));
    let gy = || Box::new(sel(filter(Some(vec![Zone::Graveyard]), None)));
    let dis = |attr| ValueExpressionAST::Distinct {
        attr,
        selector: gy(),
    };
    let c = ctx(1);
    let want = [
        (DistinctAttr::CardType, 3),
        (DistinctAttr::Subtype, 2),
        (DistinctAttr::Supertype, 1),
        (DistinctAttr::Color, 3),
        (DistinctAttr::Name, 3),
        (DistinctAttr::ManaValue, 2),
        (DistinctAttr::Power, 1),
        (DistinctAttr::Toughness, 2),
    ];
    for (attr, n) in want {
        assert_eq!(val(&env, &c, &dis(attr)), n, "{attr:?}");
    }
    let four_or_more = PredicateAST::Compare {
        cmp: Cmp::Ge,
        lhs: Box::new(dis(DistinctAttr::CardType)),
        rhs: lit(4),
    };
    assert!(!pred_pl(&env, &c, &four_or_more, 1));
    env.add(card(5, Zone::Graveyard, 2, &["Instant"]));
    assert!(pred_pl(&env, &c, &four_or_more, 1));
    let mut all = card(6, Zone::Graveyard, 2, &[]);
    all.characteristics.colors = cols(31);
    env.add(all);
    assert_eq!(val(&env, &c, &dis(DistinctAttr::Color)), 5);
    let empty = ValueExpressionAST::Distinct {
        attr: DistinctAttr::Color,
        selector: Box::new(sel(filter(Some(vec![Zone::Exile]), None))),
    };
    assert_eq!(val(&env, &c, &empty), 0);
}

/// Characteristics for the LKI capture of the ledger fixture.
struct Provider;

impl CharacteristicsProvider for Provider {
    fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
        Some(chars("test.vanilla_1_1", &["Creature"], ColorBitmask::NONE))
    }
}

/// A real `events::History` driven through the events executor and `record_spell_cast`.
/// Turn 1: player 1 draws 1, gains 9, loses 9; player 2 casts 1 spell.
/// Turn 2: player 1 draws 2, gains 3, casts 2 spells; player 2 loses 4 and 1, casts 1 spell;
/// player 3 loses 2.
fn ledger() -> History {
    let mut game = GameState::new(7, 3, 20).unwrap();
    let mut zones = Zones::new(&[p(1), p(2), p(3)], ZonesConfig::default());
    let mut ids = IdGenerator::new();
    for card_id in 1..=5 {
        let mut req = MoveRequest::new(ZoneKind::Library);
        req.position = Position::Bottom;
        zones
            .create_object(&mut ids, CardId::new(card_id), p(1), req)
            .unwrap();
    }
    let mut permanents = Permanents::new();
    let mut memories = MemoryTable::new();
    let mut lki = LkiStore::new();
    let mut events = EventState::new();
    let life = |player, delta| GameEvent::LifeChange {
        player: p(player),
        delta,
    };
    let draw = || GameEvent::Draw { player: p(1) };
    let script: [(u64, Vec<GameEvent>); 2] = [
        (1, vec![draw(), life(1, 9), life(1, -9)]),
        (
            2,
            vec![
                draw(),
                draw(),
                life(1, 3),
                life(2, -4),
                life(2, -1),
                life(3, -2),
            ],
        ),
    ];
    for (turn, list) in script {
        game.set_turn_number(turn);
        for event in list {
            let mut cx = EventContext {
                game: &mut game,
                zones: &mut zones,
                ids: &mut ids,
                permanents: &mut permanents,
                memories: &mut memories,
                lki: &mut lki,
                events: &mut events,
                provider: &Provider,
            };
            cx.execute(event).unwrap();
        }
    }
    let h = events.history_mut();
    h.record_spell_cast(1, p(2));
    h.record_spell_cast(2, p(1));
    h.record_spell_cast(2, p(1));
    h.record_spell_cast(2, p(2));
    events.history().clone()
}

/// REQ-DSL-003: HISTORY queries answered from the ledger for the current turn only.
#[test]
fn history_ledger_queries() {
    let mut env = TestEnv::with_players(3);
    env.history = ledger();
    env.turn = 2;
    let c = ctx(1);
    let h = |q: HistoryAST| ValueExpressionAST::History(Box::new(q));
    let me = || pref(ReferenceAST::Controller);
    let opps = || pref(ReferenceAST::EachOpponent);
    let spells = |player| {
        h(HistoryAST::SpellsCastThisTurn {
            player,
            condition: None,
        })
    };
    let teammates = PlayerSelectorAST::Players(PlayerGroup::Teammates);
    assert_eq!(val(&env, &c, &spells(None)), 3);
    assert_eq!(val(&env, &c, &spells(Some(me()))), 2);
    assert_eq!(val(&env, &c, &spells(Some(opps()))), 1);
    assert_eq!(val(&env, &c, &spells(Some(teammates))), 0);
    let lost = |player| h(HistoryAST::LifeLostThisTurn { player });
    let gained = |player| h(HistoryAST::LifeGainedThisTurn { player });
    let drawn = |player| h(HistoryAST::CardsDrawnThisTurn { player });
    assert_eq!(val(&env, &c, &lost(me())), 0);
    assert_eq!(val(&env, &c, &lost(opps())), 7);
    assert_eq!(val(&env, &c, &gained(me())), 3);
    assert_eq!(val(&env, &c, &drawn(me())), 2);
    assert_eq!(val(&env, &c, &drawn(opps())), 0);
    // Turn 1 counts only turn 1's events.
    env.turn = 1;
    assert_eq!(val(&env, &c, &spells(None)), 1);
    assert_eq!(val(&env, &c, &lost(me())), 9);
    assert_eq!(val(&env, &c, &lost(opps())), 0);
    assert_eq!(val(&env, &c, &gained(me())), 9);
    assert_eq!(val(&env, &c, &drawn(me())), 1);
    env.turn = 3;
    assert_eq!(val(&env, &c, &spells(None)), 0);
    assert_eq!(val(&env, &c, &drawn(me())), 0);
}

/// REQ-DSL-003: conditioned spells-cast, creatures-died and mana-spent queries.
#[test]
fn history_list_queries() {
    let mut env = TestEnv::with_players(2);
    let mut red = card(1, Zone::Stack, 1, &["Instant"]);
    red.characteristics.colors = ColorBitmask::RED;
    let mut blue = card(2, Zone::Stack, 2, &["Creature"]);
    blue.characteristics.colors = ColorBitmask::BLUE;
    env.spells = vec![
        (p(1), red.clone()),
        (p(2), blue),
        (p(1), red),
        // A caster not in the game: not among "all players".
        (p(7), card(3, Zone::Stack, 7, &["Instant"])),
    ];
    env.died = vec![creature(4, 1, 2, 2), creature(5, 2, 5, 5)];
    env.add(creature(6, 1, 1, 1));
    env.mana.insert((o(6), None), 4);
    env.mana.insert((o(6), Some("R".into())), 1);
    env.tokens.insert((o(6), "c".into()), "R".into());
    let c = ctx_src(1, 6);
    let h = |q: HistoryAST| ValueExpressionAST::History(Box::new(q));
    let instants = |player| {
        h(HistoryAST::SpellsCastThisTurn {
            player,
            condition: Some(PredicateAST::HasCardType {
                value: WordRef::Name("Instant".into()),
            }),
        })
    };
    assert_eq!(val(&env, &c, &instants(None)), 2);
    assert_eq!(
        val(&env, &c, &instants(Some(pref(ReferenceAST::Controller)))),
        2
    );
    assert_eq!(
        val(&env, &c, &instants(Some(pref(ReferenceAST::EachOpponent)))),
        0
    );
    let died = |condition| h(HistoryAST::CreaturesDiedThisTurn { condition });
    let power_ge = |v| PredicateAST::Power {
        cmp: Cmp::Ge,
        value: Box::new(v),
    };
    assert_eq!(val(&env, &c, &died(None)), 2);
    assert_eq!(
        val(
            &env,
            &c,
            &died(Some(power_ge(ValueExpressionAST::Literal(3))))
        ),
        1
    );
    assert_eq!(
        eval_value(&env, &c, &died(Some(power_ge(ValueExpressionAST::X)))),
        Err(InterpError::MissingX)
    );
    assert_eq!(
        eval_value(&env, &c, &instants(None).clone()).map(|n| n > 0),
        Ok(true)
    );
    let spent = |reference, color| h(HistoryAST::ManaSpentToCast { reference, color });
    let red_word = Some(WordRef::Name("R".into()));
    let token_word = Some(WordRef::TextToken("c".into()));
    assert_eq!(val(&env, &c, &spent(ReferenceAST::SelfRef, None)), 4);
    assert_eq!(val(&env, &c, &spent(ReferenceAST::SelfRef, red_word)), 1);
    assert_eq!(val(&env, &c, &spent(ReferenceAST::SelfRef, token_word)), 1);
    assert_eq!(val(&env, &c, &spent(ReferenceAST::Controller, None)), 0);
    assert_eq!(val(&env, &ctx(1), &spent(ReferenceAST::SelfRef, None)), 0);
}

/// REQ-DSL-003: MEMORY reads a number or mode; anything else is 0.
#[test]
fn memory_values() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(1, 1, 1, 1));
    env.add(creature(2, 1, 1, 1));
    let mut m = Memory::new();
    m.set_choice("n", ChoiceValue::Number(-6));
    m.set_choice("m", ChoiceValue::Mode(2));
    m.set_choice("c", ChoiceValue::Color("red".into()));
    env.memory.insert(o(1), m);
    let c = ctx_src(1, 1);
    let mem = |k: &str| ValueExpressionAST::Memory(k.into());
    assert_eq!(val(&env, &c, &mem("n")), -6);
    assert_eq!(val(&env, &c, &mem("m")), 2);
    assert_eq!(val(&env, &c, &mem("c")), 0);
    assert_eq!(val(&env, &c, &mem("absent")), 0);
    assert_eq!(val(&env, &ctx(1), &mem("n")), 0);
    assert_eq!(val(&env, &ctx_src(1, 2), &mem("n")), 0);
}

// ---------------------------------------------------------------------------------------------
// Impossible instructions (REQ-ARCH-002)
// ---------------------------------------------------------------------------------------------

/// REQ-ARCH-002, CR 101.3: `partition_possible` calls once per part, in order, and keeps order.
#[test]
fn partition_possible_order_and_split() {
    let mut calls = Vec::new();
    let parts = partition_possible(vec![5, 2, 8, 1, 4, 6], |n| {
        calls.push(*n);
        n % 2 == 0
    });
    assert_eq!(calls, vec![5, 2, 8, 1, 4, 6]);
    assert_eq!(parts.performed, vec![2, 8, 4, 6]);
    assert_eq!(parts.ignored, vec![5, 1]);
    let empty: Partition<u8> = partition_possible(Vec::new(), |_| true);
    assert!(empty.performed.is_empty() && empty.ignored.is_empty());
}

/// REQ-ARCH-002, CR 101.3: an ignored reference inside COUNT / ATTR / PLAYER_ATTR contributes 0
/// and never errors.
#[test]
fn ignored_references_in_values_are_zero() {
    let mut env = TestEnv::with_players(2);
    env.add(creature(1, 1, 3, 3));
    let mut c = ctx(1);
    c.targets = vec![Some(Referent::Object(o(1))), None];
    env.leave(o(1));
    env.lki.clear();
    for r in [
        ReferenceAST::Target { slot: 0 },
        ReferenceAST::Target { slot: 1 },
        ReferenceAST::SelfRef,
        ReferenceAST::ThatPlayer,
    ] {
        let count = ValueExpressionAST::Count(Box::new(ObjectSelectorAST::Ref(r.clone())));
        let attr = ValueExpressionAST::Attr {
            reference: r.clone(),
            attr: ObjectAttr::Power,
        };
        let pattr = ValueExpressionAST::PlayerAttr {
            reference: r,
            attr: PlayerAttr::Life,
        };
        for v in [count, attr, pattr] {
            assert_eq!(eval_value(&env, &c, &v), Ok(0), "{v:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Property sweep (REQ-CORE-006 determinism)
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-003, REQ-CORE-006: over a deterministic pseudo-random sweep, evaluation is
/// repeatable, selector results do not depend on `objects_in` order, and MATH never panics.
#[test]
fn property_sweep_determinism_and_no_panics() {
    let extremes = [
        i64::MIN,
        i64::MIN + 1,
        -2,
        -1,
        0,
        1,
        2,
        i64::MAX - 1,
        i64::MAX,
    ];
    let mut rng = Lcg(0x5eed);
    let small = |r: &mut Lcg, n: u64| i64::try_from(r.below(n)).unwrap();
    for round in 0..200_u64 {
        let mut env = TestEnv::with_players(1 + rng.below(4));
        let n = rng.below(8);
        for id in 1..=n {
            let mut v = creature(id, 1 + rng.below(2), small(&mut rng, 5), 1);
            v.mana_value = small(&mut rng, 4);
            v.timestamp = Timestamp::new(rng.below(3));
            v.zone_position = u32::try_from(rng.below(3)).unwrap();
            if rng.below(2) == 0 {
                v.zone = Zone::Graveyard;
            }
            if rng.below(3) == 0 {
                v.power = None;
            }
            env.add(v);
        }
        let by = OrderKey::ALL[usize::try_from(rng.below(5)).unwrap()];
        let direction = if rng.below(2) == 0 {
            Direction::Asc
        } else {
            Direction::Desc
        };
        let mut f = filter(
            Some(vec![Zone::Graveyard, Zone::Battlefield]),
            Some(PredicateAST::Or(vec![
                PredicateAST::Power {
                    cmp: Cmp::Ge,
                    value: lit(small(&mut rng, 3)),
                },
                PredicateAST::IsPermanent,
            ])),
        );
        f.ordering = Some(Ordering { by, direction });
        f.count = Some(lit(small(&mut rng, 10) - 2));
        let s = sel(f);
        let c = ctx(1);
        env.shuffle = 0;
        let base = select_objects(&env, &c, &s);
        assert_eq!(base, select_objects(&env, &c, &s));
        for seed in 1..4 {
            env.shuffle = round * 31 + seed;
            assert_eq!(select_objects(&env, &c, &s), base, "round {round}");
        }
        let dv = ValueExpressionAST::Distinct {
            attr: DistinctAttr::Power,
            selector: Box::new(s),
        };
        assert_eq!(eval_value(&env, &c, &dv), eval_value(&env, &c, &dv));

        let pick = |r: &mut Lcg| {
            if r.below(2) == 0 {
                extremes[usize::try_from(r.below(9)).unwrap()]
            } else {
                i64::try_from(r.next()).unwrap() - (1 << 30)
            }
        };
        let (l, r) = (pick(&mut rng), pick(&mut rng));
        for op in MathOp::ALL {
            let got = math(*op, l, r);
            assert_eq!(got, math(*op, l, r));
            let divides = matches!(op, MathOp::DivFloor | MathOp::DivCeil);
            if divides && r != 0 && !(l == i64::MIN && r == -1) {
                let q = got.unwrap();
                // Floor and ceiling are within one step of the exact quotient, on the
                // correct side.
                let rem = i128::from(l) - i128::from(q) * i128::from(r);
                assert!(rem.abs() < i128::from(r).abs(), "{op:?} {l} {r} -> {q}");
                let exact_ge_q = (rem == 0) || ((rem > 0) == (r > 0));
                if *op == MathOp::DivFloor {
                    assert!(exact_ge_q, "floor {l} {r} -> {q}");
                } else {
                    assert!(rem == 0 || !exact_ge_q, "ceil {l} {r} -> {q}");
                }
            }
        }
    }
}
