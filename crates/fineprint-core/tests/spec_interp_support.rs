// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared synthetic environment and AST helpers for the `fineprint_core::interp` spec tests
//! (REQ-DSL-003, REQ-ARCH-002). Black-box: written from the work-item brief only.
//!
//! Each `spec_interp_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. Every card here is synthetic and neutrally named (R5).
//!
//! [`Env::objects_in`] deliberately returns ids in a non-sorted order (reverse insertion order
//! by default, or an LCG permutation), so the tests prove that the interpreter's own results
//! never depend on the order the environment lists objects in (R3).

#![allow(dead_code, unused_imports)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_core::events::{EventContext, EventState, GameEvent, History};
use fineprint_core::interp::{
    ChosenValue, EvalContext, InterpEnv, InterpError, ObjectView, PlayerStat, Referent, Resolution,
};
use fineprint_core::memory::{CharacteristicsProvider, LkiStore, Memory, MemoryTable};
use fineprint_core::objects::Characteristics;
use fineprint_core::permanent::Permanents;
use fineprint_core::state::GameState;
use fineprint_core::zones::{MoveRequest, ZoneKind, Zones, ZonesConfig};
pub use fineprint_dsl::card::ColorBitmask;
use fineprint_dsl::card::TextTokenStream;
pub use fineprint_dsl::expr::{
    Cmp, Direction, DistinctAttr, HistoryQuery, MathOp, ObjectAttr, ObjectSelectorAST, OrderKey,
    Ordering, PlayerAttr, PlayerGroup, PlayerSelectorAST, PredicateAST, ReferenceAST,
    SelectorFilter, ValueExpressionAST, WordRef, Zone,
};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);
/// A player id no environment knows.
pub const NOBODY: PlayerId = PlayerId::new(9);

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

pub fn obj(n: u64) -> Referent {
    Referent::Object(oid(n))
}

pub fn pl(p: PlayerId) -> Referent {
    Referent::Player(p)
}

// ---------------------------------------------------------------------------------------------
// characteristics and views
// ---------------------------------------------------------------------------------------------

pub fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// Synthetic characteristics: one name, the given card types, no subtypes, colorless.
pub fn chars(name: &str, types: &[&str]) -> Characteristics {
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

pub fn colors(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("valid colour mask")
}

/// A plain view of `id` in `zone`, owned by `owner` (controlled by `owner` on the battlefield),
/// with the given card types and the name `test.object_<id>`.
pub fn view(id: u64, zone: Zone, owner: PlayerId, types: &[&str]) -> ObjectView {
    let mut v = ObjectView::new(
        oid(id),
        zone,
        owner,
        chars(&format!("test.object_{id}"), types),
    );
    if zone == Zone::Battlefield {
        v.controller = Some(owner);
    }
    v
}

/// A creature view with numeric power and toughness.
pub fn creature(id: u64, zone: Zone, owner: PlayerId, p: i64, t: i64) -> ObjectView {
    let mut v = view(id, zone, owner, &["Creature"]);
    v.power = Some(p);
    v.toughness = Some(t);
    v
}

/// A land view.
pub fn land(id: u64, zone: Zone, owner: PlayerId) -> ObjectView {
    view(id, zone, owner, &["Land"])
}

/// An artifact view.
pub fn artifact(id: u64, zone: Zone, owner: PlayerId) -> ObjectView {
    view(id, zone, owner, &["Artifact"])
}

/// An instant view.
pub fn instant(id: u64, zone: Zone, owner: PlayerId) -> ObjectView {
    view(id, zone, owner, &["Instant"])
}

// ---------------------------------------------------------------------------------------------
// the synthetic environment
// ---------------------------------------------------------------------------------------------

/// How [`Env::objects_in`] orders its result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListOrder {
    /// Reverse insertion order (the default; deliberately not sorted).
    Reverse,
    /// Insertion order.
    Insertion,
    /// A permutation of insertion order drawn from an LCG with this seed.
    Shuffled(u64),
}

/// A synthetic [`InterpEnv`]; every map is public so a test can shape any situation.
#[derive(Clone, Debug)]
pub struct Env {
    /// Objects currently in a zone, with their (already evaluated) views.
    pub objects: BTreeMap<ObjectId, ObjectView>,
    /// Insertion order of `objects` (the basis of `objects_in`).
    pub inserted: Vec<ObjectId>,
    /// Extra ids `objects_in(zone)` also lists (stale or duplicate ids).
    pub extra_listed: BTreeMap<Zone, Vec<ObjectId>>,
    pub list_order: ListOrder,
    /// Every player, in seat (turn) order.
    pub players: Vec<PlayerId>,
    pub active: Option<PlayerId>,
    pub turn: u64,
    pub history: History,
    pub memories: BTreeMap<ObjectId, Memory>,
    pub lki: BTreeMap<ObjectId, ObjectView>,
    pub attached: BTreeMap<ObjectId, ObjectId>,
    pub defending: BTreeMap<ObjectId, PlayerId>,
    /// `(player, stat key)` -> value; see [`stat_key`]. Known players without an entry read 0.
    pub stats: BTreeMap<(PlayerId, String), i64>,
    /// `(player, colour word)` -> devotion to that colour.
    pub devotion_table: BTreeMap<(PlayerId, String), i64>,
    /// Every `devotion` call, in order.
    pub devotion_calls: RefCell<Vec<(PlayerId, Vec<String>)>>,
    pub max_speed: BTreeSet<PlayerId>,
    pub spells: Vec<(PlayerId, ObjectView)>,
    pub died: Vec<ObjectView>,
    /// `(spell, colour)` -> mana spent; `None` colour is the total.
    pub mana_spent: BTreeMap<(ObjectId, Option<String>), i64>,
    /// Every `mana_spent_to_cast` call, in order.
    pub mana_calls: RefCell<Vec<(ObjectId, Option<String>)>>,
    /// `(source, token id)` -> current word.
    pub text_tokens: BTreeMap<(ObjectId, String), String>,
}

/// The key of a [`PlayerStat`] in [`Env::stats`].
pub fn stat_key(stat: &PlayerStat) -> String {
    match stat {
        PlayerStat::Life => "LIFE".to_string(),
        PlayerStat::Speed => "SPEED".to_string(),
        PlayerStat::HandSize => "HAND_SIZE".to_string(),
        PlayerStat::LibrarySize => "LIBRARY_SIZE".to_string(),
        PlayerStat::GraveyardSize => "GRAVEYARD_SIZE".to_string(),
        PlayerStat::Counters(k) => format!("COUNTERS:{k}"),
    }
}

impl Env {
    /// Two players (P1 active), turn 1, no objects.
    pub fn new() -> Env {
        Env::with_players(&[P1, P2], Some(P1))
    }

    /// Three players (P1, P2, P3 in seat order) with the given active player, turn 1.
    pub fn three(active: Option<PlayerId>) -> Env {
        Env::with_players(&[P1, P2, P3], active)
    }

    pub fn with_players(players: &[PlayerId], active: Option<PlayerId>) -> Env {
        Env {
            objects: BTreeMap::new(),
            inserted: Vec::new(),
            extra_listed: BTreeMap::new(),
            list_order: ListOrder::Reverse,
            players: players.to_vec(),
            active,
            turn: 1,
            history: History::default(),
            memories: BTreeMap::new(),
            lki: BTreeMap::new(),
            attached: BTreeMap::new(),
            defending: BTreeMap::new(),
            stats: BTreeMap::new(),
            devotion_table: BTreeMap::new(),
            devotion_calls: RefCell::new(Vec::new()),
            max_speed: BTreeSet::new(),
            spells: Vec::new(),
            died: Vec::new(),
            mana_spent: BTreeMap::new(),
            mana_calls: RefCell::new(Vec::new()),
            text_tokens: BTreeMap::new(),
        }
    }

    /// Puts `v` into its zone (replacing an earlier view of the same id).
    pub fn add(&mut self, v: ObjectView) -> ObjectId {
        let id = v.id;
        if !self.inserted.contains(&id) {
            self.inserted.push(id);
        }
        self.objects.insert(id, v);
        id
    }

    pub fn add_all(&mut self, views: impl IntoIterator<Item = ObjectView>) {
        for v in views {
            self.add(v);
        }
    }

    /// The object leaves every zone; its last known information is the view it had.
    pub fn vanish_with_lki(&mut self, id: ObjectId) {
        let v = self.objects.remove(&id).expect("object present");
        self.inserted.retain(|x| *x != id);
        self.lki.insert(id, v);
    }

    /// The object leaves every zone and no information about it remains.
    pub fn vanish(&mut self, id: ObjectId) {
        self.objects.remove(&id);
        self.inserted.retain(|x| *x != id);
        self.lki.remove(&id);
    }

    pub fn set_stat(&mut self, p: PlayerId, stat: &PlayerStat, n: i64) {
        self.stats.insert((p, stat_key(stat)), n);
    }

    pub fn set_token(&mut self, source: ObjectId, token: &str, word: &str) {
        self.text_tokens
            .insert((source, token.to_string()), word.to_string());
    }

    pub fn memory_mut(&mut self, id: ObjectId) -> &mut Memory {
        self.memories.entry(id).or_default()
    }
}

impl Default for Env {
    fn default() -> Env {
        Env::new()
    }
}

impl InterpEnv for Env {
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
            .inserted
            .iter()
            .copied()
            .filter(|id| self.objects.get(id).is_some_and(|v| v.zone == zone))
            .collect();
        match self.list_order {
            ListOrder::Insertion => {}
            ListOrder::Reverse => ids.reverse(),
            ListOrder::Shuffled(seed) => {
                let mut rng = Lcg::new(seed ^ zone_salt(zone));
                rng.shuffle(&mut ids);
            }
        }
        if let Some(extra) = self.extra_listed.get(&zone) {
            ids.extend(extra.iter().copied());
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
                .get(&(player, stat_key(stat)))
                .copied()
                .unwrap_or(0),
        )
    }

    fn devotion(&self, player: PlayerId, colors: &[String]) -> i64 {
        self.devotion_calls
            .borrow_mut()
            .push((player, colors.to_vec()));
        let distinct: BTreeSet<&String> = colors.iter().collect();
        distinct
            .into_iter()
            .map(|c| {
                self.devotion_table
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
        self.memories.get(&object)
    }

    fn spells_cast_this_turn(&self) -> Vec<(PlayerId, ObjectView)> {
        self.spells.clone()
    }

    fn creatures_died_this_turn(&self) -> Vec<ObjectView> {
        self.died.clone()
    }

    fn mana_spent_to_cast(&self, spell: ObjectId, color: Option<&str>) -> i64 {
        let key = (spell, color.map(str::to_string));
        self.mana_calls.borrow_mut().push(key.clone());
        self.mana_spent.get(&key).copied().unwrap_or(0)
    }

    fn text_token_value(&self, source: ObjectId, token_id: &str) -> Option<String> {
        self.text_tokens
            .get(&(source, token_id.to_string()))
            .cloned()
    }
}

fn zone_salt(zone: Zone) -> u64 {
    // Stable per-zone salt from the zone's position in `Zone::ALL`.
    Zone::ALL
        .iter()
        .position(|z| *z == zone)
        .map_or(0, |i| (i as u64).wrapping_mul(0x9E37_79B9))
}

// ---------------------------------------------------------------------------------------------
// contexts
// ---------------------------------------------------------------------------------------------

/// `EvalContext::new(controller)`.
pub fn ctx(controller: PlayerId) -> EvalContext {
    EvalContext::new(controller)
}

/// `EvalContext::new(controller)` with `source` set.
pub fn ctx_src(controller: PlayerId, source: ObjectId) -> EvalContext {
    let mut c = EvalContext::new(controller);
    c.source = Some(source);
    c
}

pub fn chosen_obj(id: ObjectId) -> ChosenValue {
    ChosenValue::Object(id)
}

// ---------------------------------------------------------------------------------------------
// AST builders
// ---------------------------------------------------------------------------------------------

pub fn lit(n: i64) -> ValueExpressionAST {
    ValueExpressionAST::Literal(n)
}

pub fn blit(n: i64) -> Box<ValueExpressionAST> {
    Box::new(lit(n))
}

pub fn math(op: MathOp, lhs: ValueExpressionAST, rhs: ValueExpressionAST) -> ValueExpressionAST {
    ValueExpressionAST::Math {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

pub fn name(s: &str) -> WordRef {
    WordRef::Name(s.to_string())
}

pub fn token(id: &str) -> WordRef {
    WordRef::TextToken(id.to_string())
}

pub fn pref(r: ReferenceAST) -> PlayerSelectorAST {
    PlayerSelectorAST::Ref(r)
}

/// A filter selector.
pub fn filter(
    zones: Option<Vec<Zone>>,
    predicate: Option<PredicateAST>,
    ordering: Option<Ordering>,
    count: Option<ValueExpressionAST>,
) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones,
        count: count.map(Box::new),
        ordering,
        predicate: predicate.map(Box::new),
    })
}

/// A filter selector over the given zones with a predicate.
pub fn in_zones(zones: &[Zone], predicate: Option<PredicateAST>) -> ObjectSelectorAST {
    filter(Some(zones.to_vec()), predicate, None, None)
}

/// A filter selector with no zones (the battlefield default) and the given predicate.
pub fn bf(predicate: PredicateAST) -> ObjectSelectorAST {
    filter(None, Some(predicate), None, None)
}

pub fn count(sel: ObjectSelectorAST) -> ValueExpressionAST {
    ValueExpressionAST::Count(Box::new(sel))
}

pub fn attr(r: ReferenceAST, a: ObjectAttr) -> ValueExpressionAST {
    ValueExpressionAST::Attr {
        reference: r,
        attr: a,
    }
}

pub fn pattr(r: ReferenceAST, a: PlayerAttr) -> ValueExpressionAST {
    ValueExpressionAST::PlayerAttr {
        reference: r,
        attr: a,
    }
}

pub fn distinct(a: DistinctAttr, sel: ObjectSelectorAST) -> ValueExpressionAST {
    ValueExpressionAST::Distinct {
        attr: a,
        selector: Box::new(sel),
    }
}

pub fn history(q: HistoryQuery) -> ValueExpressionAST {
    ValueExpressionAST::History(Box::new(q))
}

pub fn order(by: OrderKey, direction: Direction) -> Ordering {
    Ordering { by, direction }
}

pub fn target(slot: u32) -> ReferenceAST {
    ReferenceAST::Target { slot }
}

pub fn bound(var: &str) -> ReferenceAST {
    ReferenceAST::Bound {
        var: var.to_string(),
    }
}

/// The items of a resolution, panicking on an error.
pub fn items<T: Clone + std::fmt::Debug>(r: Result<Resolution<T>, InterpError>) -> Vec<T> {
    r.expect("no error").items
}

// ---------------------------------------------------------------------------------------------
// history ledger, fed through executed events (the only way besides record_spell_cast)
// ---------------------------------------------------------------------------------------------

/// One input of the ledger.
#[derive(Clone, Copy, Debug)]
pub enum Feed {
    /// `History::record_spell_cast`.
    Spell(PlayerId),
    /// A `LifeChange` of `-n`.
    Lose(PlayerId, i64),
    /// A `LifeChange` of `+n`.
    Gain(PlayerId, i64),
    /// A `Draw` (a library card is created first).
    Draw(PlayerId),
}

#[derive(Clone, Debug, Default)]
struct BlankProvider;

impl CharacteristicsProvider for BlankProvider {
    fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
        Some(chars("test.library_card", &["Instant"]))
    }
}

/// A `History` built by executing `script` (each entry at its turn number) in a fresh game with
/// `player_count` players issued `1..=player_count`.
pub fn ledger(player_count: usize, script: &[(u64, Feed)]) -> History {
    let mut game = GameState::new(0x1E7_0057, player_count, 20).expect("valid players");
    game.set_turn_number(1);
    let ids_of: Vec<PlayerId> = (1..=player_count as u64).map(PlayerId::new).collect();
    game.set_active_player(Some(ids_of[0])).expect("known");
    let mut zones = Zones::new(&ids_of, ZonesConfig::default());
    let mut ids: IdGenerator<ObjectId> = IdGenerator::new();
    let mut permanents = Permanents::new();
    let mut memories = MemoryTable::new();
    let mut lki = LkiStore::new();
    let mut events = EventState::new();
    let provider = BlankProvider;
    let mut next_card = 1_u64;
    for (turn, feed) in script {
        game.set_turn_number(*turn);
        match *feed {
            Feed::Spell(p) => events.history_mut().record_spell_cast(*turn, p),
            Feed::Lose(p, n) | Feed::Gain(p, n) => {
                let delta = if matches!(feed, Feed::Lose(..)) {
                    -n
                } else {
                    n
                };
                let mut cx = EventContext {
                    game: &mut game,
                    zones: &mut zones,
                    ids: &mut ids,
                    permanents: &mut permanents,
                    memories: &mut memories,
                    lki: &mut lki,
                    events: &mut events,
                    provider: &provider,
                };
                cx.execute(GameEvent::LifeChange { player: p, delta })
                    .expect("life change executes");
            }
            Feed::Draw(p) => {
                let card = CardId::new(next_card);
                next_card += 1;
                zones
                    .create_object(&mut ids, card, p, MoveRequest::new(ZoneKind::Library))
                    .expect("library card");
                let mut cx = EventContext {
                    game: &mut game,
                    zones: &mut zones,
                    ids: &mut ids,
                    permanents: &mut permanents,
                    memories: &mut memories,
                    lki: &mut lki,
                    events: &mut events,
                    provider: &provider,
                };
                cx.execute(GameEvent::Draw { player: p })
                    .expect("draw executes");
            }
        }
    }
    events.history().clone()
}

// ---------------------------------------------------------------------------------------------
// deterministic pseudo-random numbers for property tests (R3: seeded, no system source)
// ---------------------------------------------------------------------------------------------

/// A 64-bit linear congruential generator (Knuth MMIX constants).
#[derive(Clone, Debug)]
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    /// A value in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let i = usize::try_from(self.below(items.len() as u64)).expect("fits");
        items[i]
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = usize::try_from(self.below(i as u64 + 1)).expect("fits");
            items.swap(i, j);
        }
    }
}
