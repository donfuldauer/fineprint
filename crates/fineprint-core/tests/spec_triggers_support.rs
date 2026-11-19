// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::triggers` spec tests (REQ-EVENT-002; CR 603.2c,
//! 603.4, 603.10). Black-box: written from the work-item brief only.
//!
//! Each `spec_triggers_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. Nothing here is card data (R5): abilities are synthetic, ids are numbers
//! and predicates are opaque tags the fake environment answers.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::memory::MemoryTable;
use fineprint_core::triggers::{
    EventField, StateView, TriggerEnv, TriggerEvent, TriggerRegistration, WaitingTrigger, detect,
};
use fineprint_dsl::ability::{
    AbilityBase, ActivationLimit, EventBatch as DslBatch, EventKind, EventMatcher, EventSubject,
    FilterValue, Trigger, TriggeredAbilityAST,
};
use fineprint_dsl::expr::{ObjectSelectorAST, PlayerGroup, PredicateAST, SelectorFilter, Zone};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

// ---------------------------------------------------------------------------------------------
// opaque DSL values
// ---------------------------------------------------------------------------------------------

/// A fixed object selector (filter form). The fake environment never looks at it: it answers
/// on the (view, object) pair only.
pub fn sel() -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(PredicateAST::IsCreature)),
    })
}

/// An opaque predicate carrying a test-chosen tag (the fake keys its answers on the tag).
pub fn tag_pred(tag: &str) -> PredicateAST {
    PredicateAST::HasKeyword {
        keyword: tag.to_string(),
    }
}

/// The tag of a predicate built by [`tag_pred`]; `"?"` for anything else.
pub fn tag_of(pred: &PredicateAST) -> String {
    match pred {
        PredicateAST::HasKeyword { keyword } => keyword.clone(),
        _ => "?".to_string(),
    }
}

pub fn zone(z: Zone) -> FilterValue {
    FilterValue::Zone(z)
}

pub fn vzone(z: Zone) -> EventField {
    EventField::Value(FilterValue::Zone(z))
}

pub fn vint(n: i64) -> EventField {
    EventField::Value(FilterValue::Int(n))
}

pub fn vbool(b: bool) -> EventField {
    EventField::Value(FilterValue::Bool(b))
}

pub fn name(s: &str) -> FilterValue {
    FilterValue::name(s).expect("valid name")
}

// ---------------------------------------------------------------------------------------------
// matchers and abilities
// ---------------------------------------------------------------------------------------------

/// A matcher with only `kind`.
pub fn m(kind: EventKind) -> EventMatcher {
    EventMatcher::new(kind)
}

/// `m` with one more filter.
pub fn filt(mut mm: EventMatcher, key: &str, value: FilterValue) -> EventMatcher {
    mm.filters
        .get_or_insert_with(BTreeMap::new)
        .insert(key.to_string(), value);
    mm
}

/// `m` with one more binding (`field` -> `var`).
pub fn bind(mut mm: EventMatcher, field: &str, var: &str) -> EventMatcher {
    mm.bind
        .get_or_insert_with(BTreeMap::new)
        .insert(field.to_string(), var.to_string());
    mm
}

/// `m` with an object subject ([`sel`]).
pub fn subj_obj(mut mm: EventMatcher) -> EventMatcher {
    mm.subject = Some(EventSubject::Object(sel()));
    mm
}

/// `m` with a player subject (`ALL_PLAYERS`).
pub fn subj_players(mut mm: EventMatcher) -> EventMatcher {
    mm.subject = Some(EventSubject::Players(PlayerGroup::AllPlayers));
    mm
}

/// `m` with a batch mode.
pub fn batch(mut mm: EventMatcher, b: Option<DslBatch>) -> EventMatcher {
    mm.batch = b;
    mm
}

/// A triggered ability with id `id` that triggers on `matcher`.
pub fn ability(id: &str, matcher: EventMatcher) -> TriggeredAbilityAST {
    ability_with(id, Trigger::Event(matcher))
}

/// A triggered ability with id `id` and trigger `trigger`.
pub fn ability_with(id: &str, trigger: Trigger) -> TriggeredAbilityAST {
    TriggeredAbilityAST {
        base: AbilityBase::new(id),
        trigger,
        intervening_if: None,
        optional: None,
        limit: None,
        targets: None,
        modes: None,
        effect: Vec::new(),
    }
}

/// `a` with an intervening-if clause tagged `tag`.
pub fn with_if(mut a: TriggeredAbilityAST, tag: &str) -> TriggeredAbilityAST {
    a.intervening_if = Some(tag_pred(tag));
    a
}

/// `a` with a trigger limit.
pub fn with_limit(
    mut a: TriggeredAbilityAST,
    per_turn: Option<u32>,
    per_game: Option<u32>,
) -> TriggeredAbilityAST {
    a.limit = Some(ActivationLimit { per_turn, per_game });
    a
}

pub fn reg(source: u64, controller: PlayerId, ability: TriggeredAbilityAST) -> TriggerRegistration {
    TriggerRegistration {
        source: oid(source),
        controller,
        ability,
    }
}

// ---------------------------------------------------------------------------------------------
// events
// ---------------------------------------------------------------------------------------------

/// An event of `kinds` about one object, `pre` before and `post` after.
pub fn ev_obj(kinds: &[EventKind], pre: Option<u64>, post: Option<u64>) -> TriggerEvent {
    TriggerEvent::new(kinds.to_vec()).with_object(pre.map(oid), post.map(oid))
}

/// An event of `kinds` about a player.
pub fn ev_player(kinds: &[EventKind], p: PlayerId) -> TriggerEvent {
    TriggerEvent::new(kinds.to_vec()).with_player(p)
}

/// A battlefield-to-graveyard move (`pre` -> `post`) in its full kind set.
pub fn dies_event(pre: u64, post: u64) -> TriggerEvent {
    ev_obj(
        &[
            EventKind::ZoneChange,
            EventKind::LeavesBattlefield,
            EventKind::Dies,
            EventKind::PutIntoGraveyard,
        ],
        Some(pre),
        Some(post),
    )
    .with_field("from", vzone(Zone::Battlefield))
    .with_field("to", vzone(Zone::Graveyard))
}

// ---------------------------------------------------------------------------------------------
// the fake environment
// ---------------------------------------------------------------------------------------------

/// One recorded `condition_holds` call: view, tag, bindings.
pub type CondCall = (StateView, String, BTreeMap<String, EventField>);

/// A scripted [`TriggerEnv`]. Every answer is keyed on the view and the object/player/tag,
/// never on the selector. Calls are recorded for inspection.
#[derive(Debug, Default)]
pub struct FakeEnv {
    pub pre: Vec<TriggerRegistration>,
    pub post: Vec<TriggerRegistration>,
    /// `(view, object)` pairs `object_matches` accepts.
    pub objects: BTreeSet<(StateView, ObjectId)>,
    /// `(view, player)` pairs `player_matches` accepts.
    pub players: BTreeSet<(StateView, PlayerId)>,
    /// `(view, tag)` pairs `predicate_holds` answers true for.
    pub preds: BTreeSet<(StateView, String)>,
    /// `(view, tag)` pairs `condition_holds` answers true for.
    pub conds: BTreeSet<(StateView, String)>,
    /// When set, `condition_holds` with tag `"bound"` is true iff some binding is an object in
    /// this set.
    pub cond_objects: BTreeSet<ObjectId>,
    /// Objects every player can see (by PRE-event id).
    pub visible: BTreeSet<ObjectId>,
    pub object_calls: RefCell<Vec<(StateView, ObjectId)>>,
    pub player_calls: RefCell<Vec<(StateView, PlayerId)>>,
    pub pred_calls: RefCell<Vec<(StateView, String, Option<EventField>)>>,
    pub cond_calls: RefCell<Vec<CondCall>>,
    pub visible_calls: RefCell<Vec<ObjectId>>,
}

impl FakeEnv {
    pub fn new() -> FakeEnv {
        FakeEnv::default()
    }

    pub fn accept(mut self, view: StateView, ids: &[u64]) -> FakeEnv {
        for &n in ids {
            self.objects.insert((view, oid(n)));
        }
        self
    }

    pub fn accept_player(mut self, view: StateView, p: PlayerId) -> FakeEnv {
        self.players.insert((view, p));
        self
    }

    pub fn pred_true(mut self, view: StateView, tag: &str) -> FakeEnv {
        self.preds.insert((view, tag.to_string()));
        self
    }

    pub fn cond_true(mut self, view: StateView, tag: &str) -> FakeEnv {
        self.conds.insert((view, tag.to_string()));
        self
    }

    pub fn visible(mut self, ids: &[u64]) -> FakeEnv {
        for &n in ids {
            self.visible.insert(oid(n));
        }
        self
    }

    pub fn with_pre(mut self, r: TriggerRegistration) -> FakeEnv {
        self.pre.push(r);
        self
    }

    pub fn with_post(mut self, r: TriggerRegistration) -> FakeEnv {
        self.post.push(r);
        self
    }

    /// Registers `r` in both views.
    pub fn with_both(self, r: TriggerRegistration) -> FakeEnv {
        self.with_pre(r.clone()).with_post(r)
    }

    pub fn clear_calls(&self) {
        self.object_calls.borrow_mut().clear();
        self.player_calls.borrow_mut().clear();
        self.pred_calls.borrow_mut().clear();
        self.cond_calls.borrow_mut().clear();
        self.visible_calls.borrow_mut().clear();
    }
}

impl TriggerEnv for FakeEnv {
    fn registrations(&self, view: StateView) -> Vec<TriggerRegistration> {
        match view {
            StateView::PreEvent => self.pre.clone(),
            StateView::PostEvent => self.post.clone(),
        }
    }

    fn object_matches(
        &self,
        view: StateView,
        _reg: &TriggerRegistration,
        object: ObjectId,
        _selector: &ObjectSelectorAST,
    ) -> bool {
        self.object_calls.borrow_mut().push((view, object));
        self.objects.contains(&(view, object))
    }

    fn player_matches(
        &self,
        view: StateView,
        _reg: &TriggerRegistration,
        player: PlayerId,
        _group: &PlayerGroup,
    ) -> bool {
        self.player_calls.borrow_mut().push((view, player));
        self.players.contains(&(view, player))
    }

    fn predicate_holds(
        &self,
        view: StateView,
        _reg: &TriggerRegistration,
        pred: &PredicateAST,
        field: Option<&EventField>,
    ) -> bool {
        let tag = tag_of(pred);
        self.pred_calls
            .borrow_mut()
            .push((view, tag.clone(), field.cloned()));
        self.preds.contains(&(view, tag))
    }

    fn condition_holds(
        &self,
        view: StateView,
        _reg: &TriggerRegistration,
        pred: &PredicateAST,
        bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        let tag = tag_of(pred);
        self.cond_calls
            .borrow_mut()
            .push((view, tag.clone(), bindings.clone()));
        if tag == "bound" {
            return bindings
                .values()
                .any(|f| matches!(f, EventField::Object(o) if self.cond_objects.contains(o)));
        }
        self.conds.contains(&(view, tag))
    }

    fn is_visible_to_all(&self, object: ObjectId) -> bool {
        self.visible_calls.borrow_mut().push(object);
        self.visible.contains(&object)
    }
}

// ---------------------------------------------------------------------------------------------
// running detection
// ---------------------------------------------------------------------------------------------

/// `detect` at turn 1 with a fresh memory table.
pub fn run(env: &FakeEnv, events: &[TriggerEvent]) -> Vec<WaitingTrigger> {
    let mut mem = MemoryTable::new();
    detect(env, &mut mem, 1, events)
}

/// `detect` with a given memory table and turn.
pub fn run_in(
    env: &FakeEnv,
    mem: &mut MemoryTable,
    turn: u64,
    events: &[TriggerEvent],
) -> Vec<WaitingTrigger> {
    detect(env, mem, turn, events)
}

/// `(source, ability_id, event_indices)` of every trigger, in output order.
pub fn summary(ts: &[WaitingTrigger]) -> Vec<(u64, String, Vec<usize>)> {
    ts.iter()
        .map(|t| {
            (
                t.source.get(),
                t.ability_id.clone(),
                t.event_indices.clone(),
            )
        })
        .collect()
}

/// A waiting trigger literal, for queue tests.
pub fn wt(source: u64, controller: PlayerId, id: &str, index: usize) -> WaitingTrigger {
    WaitingTrigger {
        source: oid(source),
        controller,
        ability_id: id.to_string(),
        look_back: None,
        event_indices: vec![index],
        bindings: BTreeMap::new(),
    }
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

    pub fn idx(&mut self, n: usize) -> usize {
        usize::try_from(self.below(n as u64)).expect("fits")
    }

    pub fn coin(&mut self) -> bool {
        self.below(2) == 0
    }

    pub fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.idx(items.len())].clone()
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.idx(i + 1);
            items.swap(i, j);
        }
    }
}
