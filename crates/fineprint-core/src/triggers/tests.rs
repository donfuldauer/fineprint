// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the triggers module: REQ-EVENT-002 (trigger detection: look-back
// classification CR 603.10a-g, event matching CR 603.2 / 603.2c, intervening-if CR 603.4,
// limits, waiting-trigger queue). Synthetic object and player ids and neutral ability ids only.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::{
    AbilityBase, ActivationLimit, EventBatch as DslEventBatch, EventKind, EventMatcher,
    EventSubject, FilterValue, StepId, Trigger, TriggeredAbilityAST,
};
use fineprint_dsl::card::{ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::{
    ObjectSelectorAST, PlayerGroup, PlayerSelectorAST, PredicateAST, ReferenceAST, SelectorFilter,
    Zone,
};

use super::*;
use crate::events::{
    CounterTarget, DamageTarget, EventBatch, EventContext, EventInstance, EventState,
    ExecutedBatch, ExecutedEvent, GameEvent, Outcome, Phase, Step, ZoneChangeCause,
};
use crate::memory::{CharacteristicsProvider, ChoiceValue, LkiStore, MemoryTable};
use crate::objects::Characteristics;
use crate::permanent::{AttachTarget, Permanents};
use crate::state::GameState;
use crate::zones::{MoveRequest, Moved, Position, ZoneId, ZoneKind, Zones, ZonesConfig};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn zone_val(z: Zone) -> EventField {
    EventField::Value(FilterValue::Zone(z))
}

fn int_val(n: i64) -> EventField {
    EventField::Value(FilterValue::Int(n))
}

/// A selector the fake env treats as "is a creature in this view".
fn creature_sel() -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(PredicateAST::IsCreature)),
    })
}

/// A selector the fake env treats as "is the ability's source".
fn self_sel() -> ObjectSelectorAST {
    ObjectSelectorAST::Ref(ReferenceAST::SelfRef)
}

fn matcher(kind: EventKind) -> EventMatcher {
    EventMatcher::new(kind)
}

fn with_filter(mut m: EventMatcher, key: &str, v: FilterValue) -> EventMatcher {
    m.filters
        .get_or_insert_with(BTreeMap::new)
        .insert(key.to_owned(), v);
    m
}

fn with_bind(mut m: EventMatcher, field: &str, var: &str) -> EventMatcher {
    m.bind
        .get_or_insert_with(BTreeMap::new)
        .insert(field.to_owned(), var.to_owned());
    m
}

fn with_subject(mut m: EventMatcher, s: EventSubject) -> EventMatcher {
    m.subject = Some(s);
    m
}

fn with_batch(mut m: EventMatcher, b: DslEventBatch) -> EventMatcher {
    m.batch = Some(b);
    m
}

fn ability(id: &str, trigger: Trigger) -> TriggeredAbilityAST {
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

fn reg(source: u64, controller: u64, id: &str, m: EventMatcher) -> TriggerRegistration {
    TriggerRegistration {
        source: o(source),
        controller: p(controller),
        ability: ability(id, Trigger::Event(m)),
    }
}

fn limited(
    mut r: TriggerRegistration,
    per_turn: Option<u32>,
    per_game: Option<u32>,
) -> TriggerRegistration {
    r.ability.limit = Some(ActivationLimit { per_turn, per_game });
    r
}

fn conditional(mut r: TriggerRegistration) -> TriggerRegistration {
    r.ability.intervening_if = Some(PredicateAST::IsPermanent);
    r
}

/// A fake game state. Objects in `creatures` (per view) match `creature_sel`; `self_sel`
/// matches the registration's source; players in `players` (per view) match any group; a
/// `Predicate` filter holds iff the field is a player; the intervening-if holds iff `condition`.
#[derive(Default)]
struct FakeEnv {
    pre: Vec<TriggerRegistration>,
    post: Vec<TriggerRegistration>,
    creatures: BTreeSet<(StateView, ObjectId)>,
    players: BTreeSet<(StateView, PlayerId)>,
    visible: BTreeSet<ObjectId>,
    condition: bool,
    queries: RefCell<Vec<StateView>>,
    conditions_seen: RefCell<Vec<(StateView, BTreeMap<String, EventField>)>>,
}

impl FakeEnv {
    fn new() -> FakeEnv {
        FakeEnv {
            condition: true,
            ..FakeEnv::default()
        }
    }

    fn pre(mut self, r: TriggerRegistration) -> FakeEnv {
        self.pre.push(r);
        self
    }

    fn post(mut self, r: TriggerRegistration) -> FakeEnv {
        self.post.push(r);
        self
    }

    fn both(self, r: TriggerRegistration) -> FakeEnv {
        self.pre(r.clone()).post(r)
    }

    fn creature(mut self, view: StateView, id: u64) -> FakeEnv {
        self.creatures.insert((view, o(id)));
        self
    }
}

impl TriggerEnv for FakeEnv {
    fn registrations(&self, view: StateView) -> Vec<TriggerRegistration> {
        self.queries.borrow_mut().push(view);
        match view {
            StateView::PreEvent => self.pre.clone(),
            StateView::PostEvent => self.post.clone(),
        }
    }

    fn object_matches(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        object: ObjectId,
        selector: &ObjectSelectorAST,
    ) -> bool {
        match selector {
            ObjectSelectorAST::Ref(ReferenceAST::SelfRef) => object == reg.source,
            _ => self.creatures.contains(&(view, object)),
        }
    }

    fn player_matches(
        &self,
        view: StateView,
        _reg: &TriggerRegistration,
        player: PlayerId,
        _group: &PlayerGroup,
    ) -> bool {
        self.players.contains(&(view, player))
    }

    fn predicate_holds(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _pred: &PredicateAST,
        field: Option<&EventField>,
    ) -> bool {
        matches!(field, Some(EventField::Player(_)))
    }

    fn condition_holds(
        &self,
        view: StateView,
        _reg: &TriggerRegistration,
        _pred: &PredicateAST,
        bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        self.conditions_seen
            .borrow_mut()
            .push((view, bindings.clone()));
        self.condition
    }

    fn is_visible_to_all(&self, object: ObjectId) -> bool {
        self.visible.contains(&object)
    }
}

fn run(env: &FakeEnv, events: &[TriggerEvent]) -> Vec<WaitingTrigger> {
    detect(env, &mut MemoryTable::new(), 1, events)
}

fn zone_event(kinds: Vec<EventKind>, pre: u64, post: u64, from: Zone, to: Zone) -> TriggerEvent {
    TriggerEvent::new(kinds)
        .with_object(Some(o(pre)), Some(o(post)))
        .with_field("from", zone_val(from))
        .with_field("to", zone_val(to))
}

fn executed(event: GameEvent, outcome: Outcome) -> ExecutedEvent {
    ExecutedEvent {
        instance: EventInstance::new(event),
        outcome,
        lki_captured: false,
    }
}

fn batch_of(events: Vec<ExecutedEvent>) -> ExecutedBatch {
    ExecutedBatch {
        index: 0,
        turn: 1,
        events,
    }
}

fn zone_change_exec(
    old: u64,
    new: u64,
    from: ZoneId,
    to: ZoneId,
    cause: ZoneChangeCause,
) -> ExecutedEvent {
    executed(
        GameEvent::ZoneChange {
            object: o(old),
            request: MoveRequest::new(to.kind()),
            cause,
        },
        Outcome::ZoneChange(Moved {
            old: o(old),
            new: o(new),
            from,
            to,
        }),
    )
}

fn kinds_set(e: &TriggerEvent) -> BTreeSet<EventKind> {
    e.kinds.iter().copied().collect()
}

/// Asserts that `m` (category `cat`) evaluates `event` in the pre-event view: a registration
/// listed only pre-event triggers with `look_back == Some(cat)`, one listed only post-event
/// does not.
fn assert_look_back(m: EventMatcher, event: TriggerEvent, cat: LookBackKind) {
    assert_eq!(look_back_kind(&m), Some(cat));
    let pre_only = FakeEnv::new().pre(reg(50, 1, "a1", m.clone()));
    let got = run(&pre_only, std::slice::from_ref(&event));
    assert_eq!(got.len(), 1, "{cat:?} must trigger from the pre-event view");
    assert_eq!(got[0].look_back, Some(cat));
    assert_eq!(got[0].source, o(50));
    let post_only = FakeEnv::new().post(reg(50, 1, "a1", m));
    assert!(
        run(&post_only, &[event]).is_empty(),
        "{cat:?} must not use post-event registrations"
    );
}

/// Asserts that `m` is not look-back: it evaluates `event` in the post-event view.
fn assert_post_event(m: EventMatcher, event: TriggerEvent) {
    assert_eq!(look_back_kind(&m), None, "{m:?}");
    let pre_only = FakeEnv::new().pre(reg(50, 1, "a1", m.clone()));
    assert!(run(&pre_only, std::slice::from_ref(&event)).is_empty());
    let post_only = FakeEnv::new().post(reg(50, 1, "a1", m));
    let got = run(&post_only, &[event]);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].look_back, None);
}

// ---------------------------------------------------------------------------------------------
// Look-back categories: one test per category (REQ-EVENT-002, SRS 8.2, CR 603.10a-g)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: CR 603.10a leaves-the-battlefield abilities look back (LEAVES_BATTLEFIELD,
// DIES, and zone-change matchers with from=BATTLEFIELD).
#[test]
fn req_event_002_cr_603_10a_leaves_battlefield_looks_back() {
    let ev = zone_event(
        vec![
            EventKind::ZoneChange,
            EventKind::LeavesBattlefield,
            EventKind::Dies,
            EventKind::PutIntoGraveyard,
        ],
        1,
        11,
        Zone::Battlefield,
        Zone::Graveyard,
    );
    let cat = LookBackKind::LeavesBattlefield;
    assert_eq!(cat.rule(), "603.10a");
    assert_look_back(matcher(EventKind::LeavesBattlefield), ev.clone(), cat);
    assert_look_back(matcher(EventKind::Dies), ev.clone(), cat);
    for kind in [EventKind::ZoneChange, EventKind::PutIntoGraveyard] {
        let m = with_filter(matcher(kind), "from", FilterValue::Zone(Zone::Battlefield));
        assert_look_back(m, ev.clone(), cat);
    }
}

// REQ-EVENT-002: CR 603.10a abilities that trigger when a player sacrifices a permanent look
// back.
#[test]
fn req_event_002_cr_603_10a_sacrificed_looks_back() {
    let batch = batch_of(vec![zone_change_exec(
        1,
        11,
        ZoneId::Battlefield,
        ZoneId::Graveyard(p(1)),
        ZoneChangeCause::Sacrifice,
    )]);
    let ev = events_from_batch(&batch).remove(0);
    assert!(ev.kinds.contains(&EventKind::Sacrificed));
    assert_eq!(LookBackKind::Sacrificed.rule(), "603.10a");
    assert_look_back(matcher(EventKind::Sacrificed), ev, LookBackKind::Sacrificed);
}

// REQ-EVENT-002: CR 603.10a abilities that trigger when a card leaves a graveyard look back.
#[test]
fn req_event_002_cr_603_10a_leaves_graveyard_looks_back() {
    let ev = zone_event(
        vec![
            EventKind::ZoneChange,
            EventKind::LeavesGraveyard,
            EventKind::Exiled,
        ],
        3,
        13,
        Zone::Graveyard,
        Zone::Exile,
    );
    let cat = LookBackKind::LeavesGraveyard;
    assert_eq!(cat.rule(), "603.10a");
    assert_look_back(matcher(EventKind::LeavesGraveyard), ev.clone(), cat);
    for kind in [EventKind::ZoneChange, EventKind::Exiled] {
        let m = with_filter(matcher(kind), "from", FilterValue::Zone(Zone::Graveyard));
        assert_look_back(m, ev.clone(), cat);
    }
}

// REQ-EVENT-002: CR 603.10a an object all players can see put into a hand or library looks
// back; an object not visible to all does not (it is matched post-event).
#[test]
fn req_event_002_cr_603_10a_to_hand_or_library_looks_back_only_if_visible() {
    let cat = LookBackKind::ToHandOrLibrary;
    assert_eq!(cat.rule(), "603.10a");
    let to_hand = zone_event(
        vec![EventKind::ZoneChange, EventKind::ReturnedToHand],
        4,
        14,
        Zone::Exile,
        Zone::Hand,
    );
    let to_library = zone_event(
        vec![EventKind::ZoneChange],
        4,
        14,
        Zone::Exile,
        Zone::Library,
    );
    let to_hand_m = with_filter(
        matcher(EventKind::ZoneChange),
        "to",
        FilterValue::Zone(Zone::Hand),
    );
    let to_lib_m = with_filter(
        matcher(EventKind::ZoneChange),
        "to",
        FilterValue::Zone(Zone::Library),
    );
    let returned = matcher(EventKind::ReturnedToHand);
    for (m, ev) in [
        (to_hand_m, to_hand.clone()),
        (to_lib_m, to_library),
        (returned, to_hand),
    ] {
        assert_eq!(look_back_kind(&m), Some(cat));
        // Visible to all: pre-event view.
        let mut env = FakeEnv::new().pre(reg(50, 1, "a1", m.clone()));
        env.visible.insert(o(4));
        let got = run(&env, std::slice::from_ref(&ev));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].look_back, Some(cat));
        // Not visible to all: no look-back, so a pre-only registration does not trigger...
        let env = FakeEnv::new().pre(reg(50, 1, "a1", m.clone()));
        assert!(run(&env, std::slice::from_ref(&ev)).is_empty());
        // ...and a post-event registration triggers in the post-event view.
        let env = FakeEnv::new().post(reg(50, 1, "a1", m));
        let got = run(&env, std::slice::from_ref(&ev));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].look_back, None);
    }
    // No pre-event subject: never look-back.
    let ev = TriggerEvent::new(vec![EventKind::ReturnedToHand]);
    let mut env = FakeEnv::new().post(reg(50, 1, "a1", matcher(EventKind::ReturnedToHand)));
    env.visible.insert(o(4));
    assert_eq!(run(&env, &[ev])[0].look_back, None);
}

// REQ-EVENT-002: CR 603.10b abilities that trigger when a permanent phases out look back.
#[test]
fn req_event_002_cr_603_10b_phased_out_looks_back() {
    let cat = LookBackKind::PhasedOut;
    assert_eq!(cat.rule(), "603.10b");
    let ev = TriggerEvent::new(vec![EventKind::PhasedOut]).with_object(Some(o(5)), Some(o(5)));
    assert_look_back(matcher(EventKind::PhasedOut), ev, cat);
}

// REQ-EVENT-002: CR 603.10c abilities that trigger specifically when an object becomes
// unattached look back.
#[test]
fn req_event_002_cr_603_10c_unattached_looks_back() {
    let cat = LookBackKind::Unattached;
    assert_eq!(cat.rule(), "603.10c");
    let batch = batch_of(vec![executed(
        GameEvent::Unattach { attachment: o(6) },
        Outcome::Applied,
    )]);
    let ev = events_from_batch(&batch).remove(0);
    assert_look_back(matcher(EventKind::Unattached), ev, cat);
}

// REQ-EVENT-002: CR 603.10d abilities that trigger when a player loses control of an object
// look back.
#[test]
fn req_event_002_cr_603_10d_control_lost_looks_back() {
    let cat = LookBackKind::ControlLost;
    assert_eq!(cat.rule(), "603.10d");
    let ev = TriggerEvent::new(vec![EventKind::ControlChanged])
        .with_object(Some(o(7)), Some(o(7)))
        .with_player(p(2));
    assert_look_back(matcher(EventKind::ControlChanged), ev, cat);
}

// REQ-EVENT-002: CR 603.10e abilities that trigger when a spell is countered look back
// (CR 701.6a: a countered spell is put into its owner's graveyard).
#[test]
fn req_event_002_cr_603_10e_countered_looks_back() {
    let cat = LookBackKind::Countered;
    assert_eq!(cat.rule(), "603.10e");
    let batch = batch_of(vec![zone_change_exec(
        8,
        18,
        ZoneId::Stack,
        ZoneId::Graveyard(p(2)),
        ZoneChangeCause::CounterSpell,
    )]);
    let ev = events_from_batch(&batch).remove(0);
    assert!(ev.kinds.contains(&EventKind::Countered));
    assert_look_back(matcher(EventKind::Countered), ev.clone(), cat);
    // A plain stack-to-graveyard "put into a graveyard" trigger is not look-back.
    assert_post_event(matcher(EventKind::PutIntoGraveyard), ev);
}

// REQ-EVENT-002: CR 603.10f abilities that trigger when a player loses the game look back.
#[test]
fn req_event_002_cr_603_10f_player_lost_looks_back() {
    let cat = LookBackKind::PlayerLost;
    assert_eq!(cat.rule(), "603.10f");
    let ev = TriggerEvent::new(vec![EventKind::PlayerLost]).with_player(p(2));
    assert_look_back(matcher(EventKind::PlayerLost), ev, cat);
}

// REQ-EVENT-002: CR 603.10g abilities that trigger when a player planeswalks away from a plane
// look back.
#[test]
fn req_event_002_cr_603_10g_planeswalked_looks_back() {
    let cat = LookBackKind::Planeswalked;
    assert_eq!(cat.rule(), "603.10g");
    let ev = TriggerEvent::new(vec![EventKind::Planeswalked]).with_player(p(1));
    assert_look_back(matcher(EventKind::Planeswalked), ev, cat);
}

// ---------------------------------------------------------------------------------------------
// look_back_kind table and negatives (REQ-EVENT-002, CR 603.10, CR 603.6a, CR 603.6c)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: CR 603.6a enters, CR 603.6c "from anywhere" put into a graveyard, tapped,
// activated (crew/saddle/station, CR 702.122e, 702.171c, 702.184a) and drawn are not look-back.
#[test]
fn req_event_002_negatives_are_post_event() {
    let enters = zone_event(
        vec![EventKind::ZoneChange, EventKind::Enters],
        1,
        11,
        Zone::Stack,
        Zone::Battlefield,
    );
    assert_post_event(matcher(EventKind::Enters), enters);
    let to_gy = zone_event(
        vec![
            EventKind::ZoneChange,
            EventKind::PutIntoGraveyard,
            EventKind::LeavesBattlefield,
            EventKind::Dies,
        ],
        1,
        11,
        Zone::Battlefield,
        Zone::Graveyard,
    );
    // "From anywhere": no from filter, even though this object came from the battlefield.
    assert_post_event(matcher(EventKind::PutIntoGraveyard), to_gy.clone());
    assert_post_event(
        with_filter(
            matcher(EventKind::PutIntoGraveyard),
            "from",
            FilterValue::Any,
        ),
        to_gy,
    );
    let tapped = TriggerEvent::new(vec![EventKind::Tapped]).with_object(Some(o(2)), Some(o(2)));
    assert_post_event(matcher(EventKind::Tapped), tapped);
    let activated =
        TriggerEvent::new(vec![EventKind::Activated]).with_object(Some(o(2)), Some(o(2)));
    assert_post_event(matcher(EventKind::Activated), activated);
    let drawn = TriggerEvent::new(vec![EventKind::Drawn]).with_player(p(1));
    assert_post_event(matcher(EventKind::Drawn), drawn);
}

/// The expected category of `kind` with optional from/to zone filters (the brief's table).
fn expected(kind: EventKind, from: Option<Zone>, to: Option<Zone>) -> Option<LookBackKind> {
    use LookBackKind as L;
    match kind {
        // CR 701.8a, CR 603.6c, CR 603.10a: destroyed is a battlefield-to-graveyard move.
        EventKind::LeavesBattlefield | EventKind::Dies | EventKind::Destroyed => {
            Some(L::LeavesBattlefield)
        }
        EventKind::Sacrificed => Some(L::Sacrificed),
        EventKind::LeavesGraveyard => Some(L::LeavesGraveyard),
        EventKind::ZoneChange => {
            if from == Some(Zone::Battlefield) {
                Some(L::LeavesBattlefield)
            } else if from == Some(Zone::Graveyard) {
                Some(L::LeavesGraveyard)
            } else if to == Some(Zone::Hand) || to == Some(Zone::Library) {
                Some(L::ToHandOrLibrary)
            } else {
                None
            }
        }
        EventKind::PutIntoGraveyard => {
            (from == Some(Zone::Battlefield)).then_some(L::LeavesBattlefield)
        }
        EventKind::Exiled => match from {
            Some(Zone::Battlefield) => Some(L::LeavesBattlefield),
            Some(Zone::Graveyard) => Some(L::LeavesGraveyard),
            _ => None,
        },
        EventKind::ReturnedToHand => match from {
            Some(Zone::Battlefield) => Some(L::LeavesBattlefield),
            Some(Zone::Graveyard) => Some(L::LeavesGraveyard),
            _ => Some(L::ToHandOrLibrary),
        },
        EventKind::PhasedOut => Some(L::PhasedOut),
        EventKind::Unattached => Some(L::Unattached),
        EventKind::ControlChanged => Some(L::ControlLost),
        EventKind::Countered => Some(L::Countered),
        EventKind::PlayerLost => Some(L::PlayerLost),
        EventKind::Planeswalked => Some(L::Planeswalked),
        _ => None,
    }
}

// REQ-EVENT-002: CR 603.10a-g, the whole derivation table over every kind and every from/to
// zone filter (and their absence); a non-zone filter value is not a zone.
#[test]
fn req_event_002_cr_603_10_look_back_table_exhaustive() {
    let zones: Vec<Option<Zone>> = std::iter::once(None)
        .chain(Zone::ALL.iter().copied().map(Some))
        .collect();
    for &kind in EventKind::ALL {
        for &from in &zones {
            for &to in &zones {
                let mut m = matcher(kind);
                if let Some(z) = from {
                    m = with_filter(m, "from", FilterValue::Zone(z));
                }
                if let Some(z) = to {
                    m = with_filter(m, "to", FilterValue::Zone(z));
                }
                assert_eq!(
                    look_back_kind(&m),
                    expected(kind, from, to),
                    "{kind:?} {from:?} {to:?}"
                );
            }
        }
        // A from filter that is not a Zone value counts as absent.
        let m = with_filter(matcher(kind), "from", FilterValue::name("x").unwrap());
        assert_eq!(look_back_kind(&m), expected(kind, None, None));
    }
    // CR 701.8a, CR 603.10a: DESTROYED looks back as a leaves-the-battlefield ability.
    assert_eq!(
        look_back_kind(&matcher(EventKind::Destroyed)),
        Some(LookBackKind::LeavesBattlefield)
    );
    // Explicit negatives named by SRS 8.2 / the brief.
    for kind in [
        EventKind::Enters,
        EventKind::Drawn,
        EventKind::Discarded,
        EventKind::Milled,
        EventKind::Cycled,
        EventKind::Cast,
        EventKind::Tapped,
        EventKind::Untapped,
        EventKind::Attached,
        EventKind::Activated,
        EventKind::DamageDealt,
        EventKind::LifeGained,
        EventKind::LifeLost,
        EventKind::CountersPut,
        EventKind::CountersRemoved,
    ] {
        assert_eq!(look_back_kind(&matcher(kind)), None, "{kind:?}");
    }
}

// REQ-EVENT-002: SRS 8.2 the look-back category does not depend on subject, bind or batch.
#[test]
fn req_event_002_look_back_ignores_subject_bind_batch() {
    for &kind in EventKind::ALL {
        let plain = matcher(kind);
        let mut decorated = with_subject(
            with_batch(
                with_bind(matcher(kind), "object", "x"),
                DslEventBatch::OneOrMore,
            ),
            EventSubject::Object(creature_sel()),
        );
        assert_eq!(look_back_kind(&plain), look_back_kind(&decorated));
        decorated.subject = Some(EventSubject::Players(PlayerGroup::AllPlayers));
        decorated.batch = Some(DslEventBatch::Each);
        assert_eq!(look_back_kind(&plain), look_back_kind(&decorated));
    }
}

// REQ-EVENT-002: LookBackKind::rule names CR 603.10a for the four zone-change categories and
// 603.10b-g for the rest.
#[test]
fn req_event_002_look_back_rule_numbers() {
    use LookBackKind as L;
    let table = [
        (L::LeavesBattlefield, "603.10a"),
        (L::Sacrificed, "603.10a"),
        (L::LeavesGraveyard, "603.10a"),
        (L::ToHandOrLibrary, "603.10a"),
        (L::PhasedOut, "603.10b"),
        (L::Unattached, "603.10c"),
        (L::ControlLost, "603.10d"),
        (L::Countered, "603.10e"),
        (L::PlayerLost, "603.10f"),
        (L::Planeswalked, "603.10g"),
    ];
    for (k, r) in table {
        assert_eq!(k.rule(), r);
    }
}

// ---------------------------------------------------------------------------------------------
// Pre-event-only triggers (REQ-EVENT-002, CR 603.10a example)
// ---------------------------------------------------------------------------------------------

/// Three objects destroyed simultaneously: creatures 1 and 2, and the source 3.
fn destroy_all_batch() -> ExecutedBatch {
    batch_of(vec![
        zone_change_exec(
            1,
            11,
            ZoneId::Battlefield,
            ZoneId::Graveyard(p(1)),
            ZoneChangeCause::Destroy,
        ),
        zone_change_exec(
            2,
            12,
            ZoneId::Battlefield,
            ZoneId::Graveyard(p(2)),
            ZoneChangeCause::Destroy,
        ),
        zone_change_exec(
            3,
            13,
            ZoneId::Battlefield,
            ZoneId::Graveyard(p(1)),
            ZoneChangeCause::Destroy,
        ),
    ])
}

// REQ-EVENT-002: CR 603.10a example. A "whenever a creature dies" ability on a non-creature
// source triggers twice when it leaves at the same time as two creatures; the pre-event
// registration and pre-event characteristics are used. Bindings carry pre-event ids.
#[test]
fn req_event_002_cr_603_10a_example_triggers_once_per_dying_creature() {
    let events = events_from_batch(&destroy_all_batch());
    let m = with_bind(
        with_subject(
            matcher(EventKind::Dies),
            EventSubject::Object(creature_sel()),
        ),
        "object",
        "it",
    );
    // Pre-event: 1 and 2 are creatures; the source 3 has the ability. Post-event: the
    // ability no longer exists and nothing is a creature.
    let env = FakeEnv::new()
        .pre(reg(3, 1, "a1", m))
        .creature(StateView::PreEvent, 1)
        .creature(StateView::PreEvent, 2);
    let got = run(&env, &events);
    assert_eq!(got.len(), 2, "CR 603.10a: the ability triggers twice");
    assert_eq!(got[0].event_indices, vec![0]);
    assert_eq!(got[1].event_indices, vec![1]);
    for (t, id) in got.iter().zip([1, 2]) {
        assert_eq!(t.source, o(3));
        assert_eq!(t.look_back, Some(LookBackKind::LeavesBattlefield));
        assert_eq!(t.bindings.get("it"), Some(&EventField::Object(o(id))));
    }
}

// REQ-EVENT-002: CR 603.10a, the source sees itself die when it is a creature in the batch.
#[test]
fn req_event_002_cr_603_10a_source_sees_itself_die() {
    let events = events_from_batch(&destroy_all_batch());
    let m = with_subject(
        matcher(EventKind::Dies),
        EventSubject::Object(creature_sel()),
    );
    let env = FakeEnv::new()
        .pre(reg(3, 1, "a1", m))
        .creature(StateView::PreEvent, 1)
        .creature(StateView::PreEvent, 2)
        .creature(StateView::PreEvent, 3);
    let got = run(&env, &events);
    assert_eq!(got.len(), 3);
    assert_eq!(
        got.iter().map(|t| t.event_indices[0]).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    // "When this dies" with the SELF reference matches only the source's own event.
    let env = FakeEnv::new().pre(reg(
        3,
        1,
        "a2",
        with_subject(matcher(EventKind::Dies), EventSubject::Object(self_sel())),
    ));
    let got = run(&env, &events);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].event_indices, vec![2]);
}

// REQ-EVENT-002: a trigger that would only see the object pre-event triggers only under
// look-back (CR 603.10, CR 603.10a). The same selector under ENTERS or a non-look-back
// matcher does not trigger, because the object matches only in the pre-event view.
#[test]
fn req_event_002_pre_event_only_object_triggers_only_under_look_back() {
    let events = events_from_batch(&destroy_all_batch());
    let sel = EventSubject::Object(creature_sel());
    let env_for = |m: EventMatcher| {
        FakeEnv::new()
            .both(reg(9, 1, "a1", m))
            .creature(StateView::PreEvent, 1)
            .creature(StateView::PreEvent, 2)
    };
    let dies = with_subject(matcher(EventKind::Dies), sel.clone());
    assert_eq!(run(&env_for(dies), &events).len(), 2);
    // Non-look-back zone change to the graveyard ("from anywhere"): post-event, no creature.
    let to_gy = with_filter(
        with_subject(matcher(EventKind::ZoneChange), sel.clone()),
        "to",
        FilterValue::Zone(Zone::Graveyard),
    );
    assert_eq!(look_back_kind(&to_gy), None);
    assert!(run(&env_for(to_gy), &events).is_empty());
    let put = with_subject(matcher(EventKind::PutIntoGraveyard), sel.clone());
    assert!(run(&env_for(put), &events).is_empty());
    // ENTERS: post-event; the same objects entering would need to be creatures post-event.
    let enters_events = vec![zone_event(
        vec![EventKind::ZoneChange, EventKind::Enters],
        1,
        21,
        Zone::Graveyard,
        Zone::Battlefield,
    )];
    let enters = with_subject(matcher(EventKind::Enters), sel);
    assert!(run(&env_for(enters.clone()), &enters_events).is_empty());
    // CR 603.6a: post-event characteristics: the newcomer is a creature post-event.
    let env = env_for(enters).creature(StateView::PostEvent, 21);
    let got = run(&env, &enters_events);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].look_back, None);
}

// ---------------------------------------------------------------------------------------------
// Registrations and views (REQ-EVENT-002, CR 603.10)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: CR 603.10, look-back matchers use only pre-event registrations, normal
// matchers only post-event registrations; each view is queried at most once per call.
#[test]
fn req_event_002_registrations_per_view() {
    let ev = zone_event(
        vec![
            EventKind::ZoneChange,
            EventKind::LeavesBattlefield,
            EventKind::PutIntoGraveyard,
        ],
        1,
        11,
        Zone::Battlefield,
        Zone::Graveyard,
    );
    let ltb = matcher(EventKind::LeavesBattlefield);
    let gy = matcher(EventKind::PutIntoGraveyard);
    // An ability that exists only pre-event: only its look-back matcher triggers.
    let env = FakeEnv::new()
        .pre(reg(5, 1, "ltb", ltb.clone()))
        .pre(reg(5, 1, "gy", gy.clone()));
    let got = run(&env, std::slice::from_ref(&ev));
    assert_eq!(
        got.iter()
            .map(|t| t.ability_id.as_str())
            .collect::<Vec<_>>(),
        vec!["ltb"]
    );
    // An ability that exists only post-event: only its normal matcher triggers.
    let env = FakeEnv::new()
        .post(reg(6, 1, "ltb", ltb))
        .post(reg(6, 1, "gy", gy));
    let got = run(&env, std::slice::from_ref(&ev));
    assert_eq!(
        got.iter()
            .map(|t| t.ability_id.as_str())
            .collect::<Vec<_>>(),
        vec!["gy"]
    );
    let queries = env.queries.borrow().clone();
    assert_eq!(
        queries
            .iter()
            .filter(|v| **v == StateView::PreEvent)
            .count(),
        1
    );
    assert_eq!(
        queries
            .iter()
            .filter(|v| **v == StateView::PostEvent)
            .count(),
        1
    );
}

// REQ-EVENT-002: non-event triggers (PHASE_STEP, STATE) are ignored; an empty event list gives
// no trigger; the controller comes from the registration.
#[test]
fn req_event_002_non_event_triggers_and_empty_input() {
    let phase = TriggerRegistration {
        source: o(1),
        controller: p(1),
        ability: ability(
            "ps",
            Trigger::PhaseStep {
                step: StepId::Upkeep,
                whose: PlayerSelectorAST::Ref(ReferenceAST::Controller),
            },
        ),
    };
    let state = TriggerRegistration {
        source: o(1),
        controller: p(1),
        ability: ability("st", Trigger::State(PredicateAST::IsCreature)),
    };
    let ev = TriggerEvent::new(EventKind::ALL.to_vec())
        .with_object(Some(o(1)), Some(o(1)))
        .with_player(p(1));
    let env =
        FakeEnv::new()
            .both(phase)
            .both(state)
            .both(reg(2, 2, "ev", matcher(EventKind::Tapped)));
    let got = run(&env, &[ev]);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].ability_id, "ev");
    assert_eq!(got[0].controller, p(2));
    assert!(run(&env, &[]).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Subject and filters (REQ-EVENT-002, CR 603.2)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: CR 603.2, object subjects need an object in the view; player subjects need a
// subject player in the group.
#[test]
fn req_event_002_subject_matching() {
    let obj_m = with_subject(
        matcher(EventKind::Drawn),
        EventSubject::Object(creature_sel()),
    );
    let ply_m = with_subject(
        matcher(EventKind::Drawn),
        EventSubject::Players(PlayerGroup::AllPlayers),
    );
    let drawn = TriggerEvent::new(vec![EventKind::Drawn]).with_player(p(1));
    // An object subject on an event with no object: no match.
    let env = FakeEnv::new()
        .post(reg(1, 1, "a1", obj_m))
        .creature(StateView::PostEvent, 1);
    assert!(run(&env, std::slice::from_ref(&drawn)).is_empty());
    // A player subject: the player must match.
    let mut env = FakeEnv::new().post(reg(1, 1, "a1", ply_m.clone()));
    assert!(run(&env, std::slice::from_ref(&drawn)).is_empty());
    env.players.insert((StateView::PostEvent, p(1)));
    assert_eq!(run(&env, std::slice::from_ref(&drawn)).len(), 1);
    // A player subject on an event without a subject player: no match.
    let no_player = TriggerEvent::new(vec![EventKind::Drawn]);
    assert!(run(&env, &[no_player]).is_empty());
    // Players are checked in the view of the matcher: a pre-event player set does not count.
    let mut env = FakeEnv::new().post(reg(1, 1, "a1", ply_m));
    env.players.insert((StateView::PreEvent, p(1)));
    assert!(run(&env, &[drawn]).is_empty());
    // An object subject uses the post-event id in the post-event view.
    let tapped = TriggerEvent::new(vec![EventKind::Tapped]).with_object(Some(o(4)), Some(o(5)));
    let m = with_subject(
        matcher(EventKind::Tapped),
        EventSubject::Object(creature_sel()),
    );
    let env = FakeEnv::new()
        .post(reg(1, 1, "a1", m.clone()))
        .creature(StateView::PostEvent, 4);
    assert!(run(&env, std::slice::from_ref(&tapped)).is_empty());
    let env = FakeEnv::new()
        .post(reg(1, 1, "a1", m))
        .creature(StateView::PostEvent, 5);
    assert_eq!(run(&env, &[tapped]).len(), 1);
}

// REQ-EVENT-002: CR 603.2, filters: ANY always matches (even a missing field); Bool, Int, Zone
// and Name need an equal plain value; a predicate goes to the env with the field.
#[test]
fn req_event_002_filters() {
    let dmg = TriggerEvent::new(vec![EventKind::DamageDealt])
        .with_object(Some(o(1)), Some(o(1)))
        .with_field("combat", EventField::Value(FilterValue::Bool(true)))
        .with_field("amount", int_val(3))
        .with_field("recipient", EventField::Player(p(2)))
        .with_field("zone", zone_val(Zone::Battlefield))
        .with_field(
            "kind",
            EventField::Value(FilterValue::name("charge").unwrap()),
        );
    let check = |key: &str, v: FilterValue| -> bool {
        let m = with_filter(matcher(EventKind::DamageDealt), key, v);
        !run(
            &FakeEnv::new().post(reg(9, 1, "a1", m)),
            std::slice::from_ref(&dmg),
        )
        .is_empty()
    };
    assert!(check("combat", FilterValue::Bool(true)));
    assert!(!check("combat", FilterValue::Bool(false)));
    assert!(check("amount", FilterValue::Int(3)));
    assert!(!check("amount", FilterValue::Int(2)));
    assert!(check("zone", FilterValue::Zone(Zone::Battlefield)));
    assert!(!check("zone", FilterValue::Zone(Zone::Exile)));
    assert!(check("kind", FilterValue::name("charge").unwrap()));
    assert!(!check("kind", FilterValue::name("other").unwrap()));
    assert!(check("missing", FilterValue::Any));
    assert!(check("combat", FilterValue::Any));
    assert!(!check("missing", FilterValue::Bool(true)));
    // A plain filter never equals a non-Value field.
    assert!(!check("recipient", FilterValue::Int(2)));
    // A type mismatch does not match.
    assert!(!check("amount", FilterValue::Bool(true)));
    // Predicate: the fake holds iff the field is a player.
    assert!(check(
        "recipient",
        FilterValue::Predicate(PredicateAST::IsPlayer)
    ));
    assert!(!check(
        "amount",
        FilterValue::Predicate(PredicateAST::IsPlayer)
    ));
    assert!(!check(
        "missing",
        FilterValue::Predicate(PredicateAST::IsPlayer)
    ));
    // All filters must hold.
    let m = with_filter(
        with_filter(
            matcher(EventKind::DamageDealt),
            "combat",
            FilterValue::Bool(true),
        ),
        "amount",
        FilterValue::Int(4),
    );
    assert!(run(&FakeEnv::new().post(reg(9, 1, "a1", m)), &[dmg]).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Batching (REQ-EVENT-002, CR 603.2c)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: CR 603.2c, EACH (and an absent batch) triggers once per matching event;
// ONE_OR_MORE once per batch with every matching index, emitted at its first index.
#[test]
fn req_event_002_cr_603_2c_each_vs_one_or_more() {
    let tap =
        |n: u64| TriggerEvent::new(vec![EventKind::Tapped]).with_object(Some(o(n)), Some(o(n)));
    let draw = TriggerEvent::new(vec![EventKind::Drawn]).with_player(p(1));
    let events = vec![draw, tap(1), tap(2), tap(3)];
    let sel = EventSubject::Object(creature_sel());
    let base = with_subject(matcher(EventKind::Tapped), sel);
    let env_for = |m: EventMatcher, id: &str, src: u64| {
        FakeEnv::new()
            .post(reg(src, 1, id, m))
            .creature(StateView::PostEvent, 2)
            .creature(StateView::PostEvent, 3)
    };
    for m in [base.clone(), with_batch(base.clone(), DslEventBatch::Each)] {
        let got = run(&env_for(m, "a1", 9), &events);
        assert_eq!(
            got.iter()
                .map(|t| t.event_indices.clone())
                .collect::<Vec<_>>(),
            vec![vec![2], vec![3]]
        );
    }
    let oom = with_bind(
        with_batch(base, DslEventBatch::OneOrMore),
        "object",
        "first",
    );
    let got = run(&env_for(oom.clone(), "a1", 9), &events);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].event_indices, vec![2, 3]);
    // Bindings come from the first matching event.
    assert_eq!(
        got[0].bindings.get("first"),
        Some(&EventField::Object(o(2)))
    );
    // Ordering against an EACH trigger of a lower source on the later event.
    let env = env_for(oom, "a1", 9).post(reg(
        1,
        1,
        "b1",
        with_subject(matcher(EventKind::Tapped), EventSubject::Object(self_sel())),
    ));
    let got = run(&env, &events);
    assert_eq!(
        got.iter()
            .map(|t| (t.source, t.event_indices.clone()))
            .collect::<Vec<_>>(),
        vec![(o(1), vec![1]), (o(9), vec![2, 3])]
    );
}

// REQ-EVENT-002: output is ordered by (first event index, source, ability id).
#[test]
fn req_event_002_output_order() {
    let ev =
        |n: u64| TriggerEvent::new(vec![EventKind::Tapped]).with_object(Some(o(n)), Some(o(n)));
    let env = FakeEnv::new()
        .post(reg(7, 1, "b", matcher(EventKind::Tapped)))
        .post(reg(7, 1, "a", matcher(EventKind::Tapped)))
        .post(reg(3, 2, "z", matcher(EventKind::Tapped)));
    let got = run(&env, &[ev(1), ev(2)]);
    let order: Vec<(usize, ObjectId, String)> = got
        .iter()
        .map(|t| (t.event_indices[0], t.source, t.ability_id.clone()))
        .collect();
    assert_eq!(
        order,
        vec![
            (0, o(3), "z".to_owned()),
            (0, o(7), "a".to_owned()),
            (0, o(7), "b".to_owned()),
            (1, o(3), "z".to_owned()),
            (1, o(7), "a".to_owned()),
            (1, o(7), "b".to_owned()),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Bindings (REQ-EVENT-002)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: `object` binds the pre-event id under look-back (CR 603.10a) and the
// post-event id otherwise; `player` the subject player; plain fields by name; absent fields are
// not bound.
#[test]
fn req_event_002_bindings() {
    let ev = zone_event(
        vec![
            EventKind::ZoneChange,
            EventKind::LeavesBattlefield,
            EventKind::PutIntoGraveyard,
        ],
        1,
        11,
        Zone::Battlefield,
        Zone::Graveyard,
    )
    .with_player(p(2));
    let bind_all = |m: EventMatcher| {
        let m = with_bind(m, "object", "obj");
        let m = with_bind(m, "player", "who");
        let m = with_bind(m, "to", "dest");
        with_bind(m, "nosuch", "none")
    };
    let env = FakeEnv::new()
        .both(reg(
            5,
            1,
            "lb",
            bind_all(matcher(EventKind::LeavesBattlefield)),
        ))
        .both(reg(
            5,
            1,
            "pg",
            bind_all(matcher(EventKind::PutIntoGraveyard)),
        ));
    let got = run(&env, &[ev]);
    assert_eq!(got.len(), 2);
    let lb = got.iter().find(|t| t.ability_id == "lb").unwrap();
    let pg = got.iter().find(|t| t.ability_id == "pg").unwrap();
    assert_eq!(lb.bindings.get("obj"), Some(&EventField::Object(o(1))));
    assert_eq!(pg.bindings.get("obj"), Some(&EventField::Object(o(11))));
    for t in [lb, pg] {
        assert_eq!(t.bindings.get("who"), Some(&EventField::Player(p(2))));
        assert_eq!(t.bindings.get("dest"), Some(&zone_val(Zone::Graveyard)));
        assert!(!t.bindings.contains_key("none"));
        assert_eq!(t.bindings.len(), 3);
    }
    // No subject player and no object: `player` and `object` stay unbound.
    let bare = TriggerEvent::new(vec![EventKind::Tapped]);
    let env = FakeEnv::new().post(reg(5, 1, "t", bind_all(matcher(EventKind::Tapped))));
    assert!(run(&env, &[bare])[0].bindings.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Intervening-if (REQ-EVENT-002, CR 603.4)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: CR 603.4, a false intervening-if condition at trigger time means no trigger
// and no limit consumed; a true one triggers, and the condition sees the bindings and the
// matcher's view.
#[test]
fn req_event_002_cr_603_4_intervening_if() {
    let ev = zone_event(
        vec![EventKind::ZoneChange, EventKind::LeavesBattlefield],
        1,
        11,
        Zone::Battlefield,
        Zone::Exile,
    );
    let m = with_bind(matcher(EventKind::LeavesBattlefield), "object", "it");
    let r = limited(conditional(reg(5, 1, "a1", m)), Some(1), None);
    let mut env = FakeEnv::new().pre(r);
    env.condition = false;
    let mut memory = MemoryTable::new();
    assert!(detect(&env, &mut memory, 1, std::slice::from_ref(&ev)).is_empty());
    assert!(memory.get(o(5)).is_none(), "no limit consumed");
    {
        let seen = env.conditions_seen.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, StateView::PreEvent);
        assert_eq!(seen[0].1.get("it"), Some(&EventField::Object(o(1))));
    }
    env.condition = true;
    let got = detect(&env, &mut memory, 1, std::slice::from_ref(&ev));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].bindings.get("it"), Some(&EventField::Object(o(1))));
    // A matcher without intervening-if never asks.
    let env = FakeEnv::new().pre(reg(5, 1, "a1", matcher(EventKind::LeavesBattlefield)));
    assert_eq!(run(&env, &[ev]).len(), 1);
    assert!(env.conditions_seen.borrow().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Limits (REQ-EVENT-002, SRS 8.2 item 8, CR 603.2h)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: SRS 8.2 item 8 / CR 603.2h, perTurn=1: only the first event of a batch
// triggers, a later batch in the same turn does not, the next turn triggers again.
#[test]
fn req_event_002_per_turn_limit() {
    let tap =
        |n: u64| TriggerEvent::new(vec![EventKind::Tapped]).with_object(Some(o(n)), Some(o(n)));
    let env = FakeEnv::new().post(limited(
        reg(5, 1, "a1", matcher(EventKind::Tapped)),
        Some(1),
        None,
    ));
    let mut memory = MemoryTable::new();
    let got = detect(&env, &mut memory, 3, &[tap(1), tap(2)]);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].event_indices, vec![0]);
    assert!(detect(&env, &mut memory, 3, &[tap(1)]).is_empty());
    assert_eq!(detect(&env, &mut memory, 4, &[tap(1), tap(2)]).len(), 1);
    // The key scheme recorded in DECISIONS.md.
    let mem = memory.get(o(5)).unwrap();
    assert_eq!(
        mem.choice("trigger.limit.turn:a1"),
        Some(&ChoiceValue::Number(4))
    );
    assert_eq!(
        mem.choice("trigger.limit.turn_count:a1"),
        Some(&ChoiceValue::Number(1))
    );
    assert_eq!(
        mem.choice("trigger.limit.game_count:a1"),
        Some(&ChoiceValue::Number(2))
    );
    // Per ability instance: another ability on the same source and the same ability id on
    // another source are independent.
    let env = FakeEnv::new()
        .post(limited(
            reg(5, 1, "a2", matcher(EventKind::Tapped)),
            Some(1),
            None,
        ))
        .post(limited(
            reg(6, 1, "a1", matcher(EventKind::Tapped)),
            Some(1),
            None,
        ));
    assert_eq!(detect(&env, &mut memory, 4, &[tap(1)]).len(), 2);
}

// REQ-EVENT-002: SRS 8.2 item 8, perGame counts every triggering; perTurn and perGame together
// both have to allow it.
#[test]
fn req_event_002_per_game_limit() {
    let tap = TriggerEvent::new(vec![EventKind::Tapped]);
    let env = FakeEnv::new().post(limited(
        reg(5, 1, "a1", matcher(EventKind::Tapped)),
        None,
        Some(2),
    ));
    let mut memory = MemoryTable::new();
    let got = detect(
        &env,
        &mut memory,
        1,
        &[tap.clone(), tap.clone(), tap.clone()],
    );
    assert_eq!(got.len(), 2);
    assert!(detect(&env, &mut memory, 2, std::slice::from_ref(&tap)).is_empty());
    let env = FakeEnv::new().post(limited(
        reg(6, 1, "a1", matcher(EventKind::Tapped)),
        Some(1),
        Some(2),
    ));
    let mut memory = MemoryTable::new();
    assert_eq!(
        detect(&env, &mut memory, 1, &[tap.clone(), tap.clone()]).len(),
        1
    );
    assert_eq!(
        detect(&env, &mut memory, 2, std::slice::from_ref(&tap)).len(),
        1
    );
    assert!(detect(&env, &mut memory, 3, std::slice::from_ref(&tap)).is_empty());
    // A non-number value under a key counts as zero (defensive).
    let mut memory = MemoryTable::new();
    memory
        .get_mut_or_default(o(6))
        .set_choice("trigger.limit.game_count:a1", ChoiceValue::Mode(9));
    assert_eq!(
        detect(&env, &mut memory, 1, std::slice::from_ref(&tap)).len(),
        1
    );
    // ONE_OR_MORE consumes one use per batch.
    let oom = with_batch(matcher(EventKind::Tapped), DslEventBatch::OneOrMore);
    let env = FakeEnv::new().post(limited(reg(7, 1, "a1", oom), Some(1), None));
    let mut memory = MemoryTable::new();
    assert_eq!(detect(&env, &mut memory, 1, &[tap.clone(), tap]).len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Determinism (REQ-EVENT-002, REQ-CORE-001)
// ---------------------------------------------------------------------------------------------

/// Every permutation of `items`.
fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, head.clone());
            out.push(tail);
        }
    }
    out
}

// REQ-EVENT-002, REQ-CORE-001: the same input gives identical output and Memory, and the order
// in which the env lists registrations does not matter.
#[test]
fn req_event_002_determinism_and_registration_order_independence() {
    let events = events_from_batch(&destroy_all_batch());
    let regs = vec![
        limited(
            reg(
                3,
                1,
                "a1",
                with_subject(
                    matcher(EventKind::Dies),
                    EventSubject::Object(creature_sel()),
                ),
            ),
            Some(1),
            None,
        ),
        reg(
            9,
            2,
            "a1",
            with_batch(
                matcher(EventKind::PutIntoGraveyard),
                DslEventBatch::OneOrMore,
            ),
        ),
        reg(
            2,
            2,
            "b1",
            with_bind(matcher(EventKind::LeavesBattlefield), "object", "x"),
        ),
        reg(2, 2, "a9", matcher(EventKind::Destroyed)),
    ];
    let reference = {
        let mut env = FakeEnv::new()
            .creature(StateView::PreEvent, 1)
            .creature(StateView::PreEvent, 2);
        env.pre = regs.clone();
        env.post = regs.clone();
        let mut memory = MemoryTable::new();
        (detect(&env, &mut memory, 1, &events), memory)
    };
    assert!(reference.0.len() > 4);
    for perm in permutations(&regs) {
        let mut env = FakeEnv::new()
            .creature(StateView::PreEvent, 1)
            .creature(StateView::PreEvent, 2);
        env.pre = perm.clone();
        let mut rev = perm;
        rev.reverse();
        env.post = rev;
        let mut memory = MemoryTable::new();
        let got = detect(&env, &mut memory, 1, &events);
        assert_eq!(got, reference.0);
        assert_eq!(memory, reference.1);
    }
}

// REQ-EVENT-002, R12: detect and events_from_batch do not panic on arbitrary bounded
// pseudo-random inputs (deterministic LCG), and detect stays deterministic on them.
#[test]
fn req_event_002_no_panic_on_bounded_random_inputs() {
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        seed >> 33
    };
    let kinds = EventKind::ALL;
    for _ in 0..200 {
        let mut events = Vec::new();
        for _ in 0..(next() % 5) {
            let mut ev = TriggerEvent::new(
                (0..next() % 4)
                    .map(|_| kinds[(next() as usize) % kinds.len()])
                    .collect(),
            );
            if next() % 2 == 0 {
                ev = ev.with_object(Some(o(next() % 4)), Some(o(next() % 4)));
            }
            if next() % 2 == 0 {
                ev = ev.with_player(p(next() % 3));
            }
            if next() % 2 == 0 {
                ev = ev.with_field(
                    "from",
                    zone_val(Zone::ALL[(next() as usize) % Zone::ALL.len()]),
                );
            }
            events.push(ev);
        }
        let mut env = FakeEnv::new();
        for i in 0..(next() % 4) {
            let mut m = matcher(kinds[(next() as usize) % kinds.len()]);
            if next() % 2 == 0 {
                m = with_filter(
                    m,
                    "from",
                    FilterValue::Zone(Zone::ALL[(next() as usize) % Zone::ALL.len()]),
                );
            }
            if next() % 2 == 0 {
                m = with_batch(m, DslEventBatch::OneOrMore);
            }
            if next() % 2 == 0 {
                m = with_subject(m, EventSubject::Object(creature_sel()));
            }
            let r = limited(
                reg(next() % 4, 1 + next() % 2, &format!("a{i}"), m),
                Some(1),
                None,
            );
            if next() % 2 == 0 {
                env.pre.push(r.clone());
            }
            env.post.push(r);
        }
        env.creatures.insert((StateView::PreEvent, o(next() % 4)));
        env.visible.insert(o(next() % 4));
        let a = detect(&env, &mut MemoryTable::new(), next(), &events);
        let again = detect(&env, &mut MemoryTable::new(), 0, &events);
        let again2 = detect(&env, &mut MemoryTable::new(), 0, &events);
        assert_eq!(again, again2);
        for t in a {
            assert!(!t.event_indices.is_empty());
            assert!(t.event_indices.windows(2).all(|w| w[0] < w[1]));
            assert!(t.event_indices.iter().all(|&i| i < events.len()));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// TriggerEvent builders (REQ-EVENT-002)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: TriggerEvent::new leaves every other member empty; the builders set them.
#[test]
fn req_event_002_trigger_event_builders() {
    let e = TriggerEvent::new(vec![EventKind::Tapped]);
    assert_eq!(e.kinds, vec![EventKind::Tapped]);
    assert_eq!(
        (e.subject_pre, e.subject_post, e.subject_player),
        (None, None, None)
    );
    assert!(e.fields.is_empty());
    let e = e
        .with_object(Some(o(1)), Some(o(2)))
        .with_player(p(3))
        .with_field("amount", int_val(1))
        .with_field("amount", int_val(2));
    assert_eq!(
        (e.subject_pre, e.subject_post, e.subject_player),
        (Some(o(1)), Some(o(2)), Some(p(3)))
    );
    assert_eq!(e.fields.get("amount"), Some(&int_val(2)));
    assert_eq!(e.fields.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// events_from_batch mapping, row by row (REQ-EVENT-002)
// ---------------------------------------------------------------------------------------------

fn one(e: ExecutedEvent) -> Vec<TriggerEvent> {
    events_from_batch(&batch_of(vec![e]))
}

// REQ-EVENT-002: zone changes map to ZONE_CHANGE plus every derived kind (CR 603.6a, 603.6c,
// 603.10a), with pre/post ids and from/to fields.
#[test]
fn req_event_002_mapping_zone_change_kinds() {
    use EventKind as K;
    let zid = |k: ZoneKind| ZoneId::of(k, p(1));
    let derived = |from: ZoneKind, to: ZoneKind| -> BTreeSet<EventKind> {
        use ZoneKind::*;
        let mut s = BTreeSet::from([K::ZoneChange]);
        if to == Battlefield && from != Battlefield {
            s.insert(K::Enters);
        }
        if from == Battlefield && to != Battlefield {
            s.insert(K::LeavesBattlefield);
        }
        if from == Battlefield && to == Graveyard {
            s.insert(K::Dies);
        }
        if to == Graveyard && from != Graveyard {
            s.insert(K::PutIntoGraveyard);
        }
        if from == Graveyard && to != Graveyard {
            s.insert(K::LeavesGraveyard);
        }
        if to == Exile && from != Exile {
            s.insert(K::Exiled);
        }
        if to == Hand && matches!(from, Battlefield | Graveyard | Exile | Stack | Command) {
            s.insert(K::ReturnedToHand);
        }
        // CR 400.8, CR 406.7: an exiled object exiled again is only EXILED (no zone change).
        if from == Exile && to == Exile {
            s = BTreeSet::from([K::Exiled]);
        }
        s
    };
    let dsl = |k: ZoneKind| match k {
        ZoneKind::Library => Zone::Library,
        ZoneKind::Hand => Zone::Hand,
        ZoneKind::Battlefield => Zone::Battlefield,
        ZoneKind::Graveyard => Zone::Graveyard,
        ZoneKind::Stack => Zone::Stack,
        ZoneKind::Exile => Zone::Exile,
        ZoneKind::Command => Zone::Command,
    };
    for from in ZoneKind::ALL {
        for to in ZoneKind::ALL {
            let got = one(zone_change_exec(
                1,
                2,
                zid(from),
                zid(to),
                ZoneChangeCause::Other,
            ));
            assert_eq!(got.len(), 1);
            let e = &got[0];
            assert_eq!(kinds_set(e), derived(from, to), "{from:?} -> {to:?}");
            assert_eq!((e.subject_pre, e.subject_post), (Some(o(1)), Some(o(2))));
            assert_eq!(e.subject_player, None);
            assert_eq!(e.fields.get("from"), Some(&zone_val(dsl(from))));
            assert_eq!(e.fields.get("to"), Some(&zone_val(dsl(to))));
            assert_eq!(e.fields.len(), 2);
            // CR 400.7: the same object repositioned in its zone (same id) is no event; exile
            // and the command zone always give a new id (CR 400.8, CR 400.10).
            if from == to && !matches!(from, ZoneKind::Exile | ZoneKind::Command) {
                for cause in [
                    ZoneChangeCause::Other,
                    ZoneChangeCause::Sacrifice,
                    ZoneChangeCause::Destroy,
                    ZoneChangeCause::Mill,
                ] {
                    let same = one(zone_change_exec(1, 1, zid(from), zid(to), cause));
                    assert!(same.is_empty(), "{from:?} reposition {cause:?}");
                }
            }
        }
    }
    // By cause.
    for (cause, kind) in [
        (ZoneChangeCause::Sacrifice, Some(K::Sacrificed)),
        (ZoneChangeCause::Destroy, Some(K::Destroyed)),
        (ZoneChangeCause::Discard, Some(K::Discarded)),
        (ZoneChangeCause::Mill, Some(K::Milled)),
        (ZoneChangeCause::CounterSpell, Some(K::Countered)),
        (ZoneChangeCause::Exile, None),
        (ZoneChangeCause::StateBasedAction, None),
        (ZoneChangeCause::Other, None),
    ] {
        let e = one(zone_change_exec(
            1,
            2,
            ZoneId::Battlefield,
            ZoneId::Graveyard(p(1)),
            cause,
        ))
        .remove(0);
        let mut want = derived(ZoneKind::Battlefield, ZoneKind::Graveyard);
        want.extend(kind);
        assert_eq!(kinds_set(&e), want, "{cause:?}");
    }
    // A zone-change event whose outcome is not a zone change produces nothing.
    assert!(
        one(executed(
            GameEvent::ZoneChange {
                object: o(1),
                request: MoveRequest::new(ZoneKind::Graveyard),
                cause: ZoneChangeCause::Other,
            },
            Outcome::Applied,
        ))
        .is_empty()
    );
}

// REQ-EVENT-002: Draw, Damage and LifeChange rows.
#[test]
fn req_event_002_mapping_draw_damage_life() {
    let draw = one(executed(GameEvent::Draw { player: p(2) }, Outcome::Applied)).remove(0);
    assert_eq!(
        draw,
        TriggerEvent::new(vec![EventKind::Drawn]).with_player(p(2))
    );
    // CR 120.3a: damage to a player is two occurrences (damage, then life loss); damage to an
    // object only one (CR 120.3e is no life loss).
    for (target, recipient, count) in [
        (DamageTarget::Player(p(2)), EventField::Player(p(2)), 2),
        (DamageTarget::Object(o(4)), EventField::Object(o(4)), 1),
    ] {
        let mut all = one(executed(
            GameEvent::Damage {
                source: Some(o(3)),
                source_controller: Some(p(1)),
                target,
                amount: 5,
                combat: true,
            },
            Outcome::Applied,
        ));
        assert_eq!(all.len(), count);
        let e = all.remove(0);
        let want = TriggerEvent::new(vec![EventKind::DamageDealt])
            .with_object(Some(o(3)), Some(o(3)))
            .with_field("combat", EventField::Value(FilterValue::Bool(true)))
            .with_field("amount", int_val(5))
            .with_field("recipient", recipient);
        assert_eq!(e, want);
    }
    let sourceless = one(executed(
        GameEvent::Damage {
            source: None,
            source_controller: None,
            target: DamageTarget::Player(p(1)),
            amount: 1,
            combat: false,
        },
        Outcome::Applied,
    ))
    .remove(0);
    assert_eq!(
        (sourceless.subject_pre, sourceless.subject_post),
        (None, None)
    );
    // CR 120.8: a zero-damage event (never logged by the executor) is no life loss.
    let zero = one(executed(
        GameEvent::Damage {
            source: Some(o(3)),
            source_controller: None,
            target: DamageTarget::Player(p(1)),
            amount: 0,
            combat: false,
        },
        Outcome::Applied,
    ));
    assert_eq!(zero.len(), 1);
    assert_eq!(zero[0].kinds, vec![EventKind::DamageDealt]);
    let life = |delta: i64| {
        one(executed(
            GameEvent::LifeChange {
                player: p(1),
                delta,
            },
            Outcome::Applied,
        ))
    };
    assert_eq!(
        life(3),
        vec![
            TriggerEvent::new(vec![EventKind::LifeGained])
                .with_player(p(1))
                .with_field("amount", int_val(3))
        ]
    );
    assert_eq!(
        life(-4),
        vec![
            TriggerEvent::new(vec![EventKind::LifeLost])
                .with_player(p(1))
                .with_field("amount", int_val(4))
        ]
    );
    assert!(life(0).is_empty());
    assert_eq!(
        life(i64::MIN)[0].fields.get("amount"),
        Some(&int_val(i64::MAX))
    );
}

// REQ-EVENT-002: counter rows: object and player targets, `kind` only when a valid filter name,
// removed amount from the outcome.
#[test]
fn req_event_002_mapping_counters() {
    let add = one(executed(
        GameEvent::AddCounters {
            target: CounterTarget::Object(o(4)),
            kind: "charge".to_owned(),
            amount: 2,
        },
        Outcome::Applied,
    ))
    .remove(0);
    assert_eq!(
        add,
        TriggerEvent::new(vec![EventKind::CountersPut])
            .with_object(Some(o(4)), Some(o(4)))
            .with_field("amount", int_val(2))
            .with_field(
                "kind",
                EventField::Value(FilterValue::name("charge").unwrap())
            )
    );
    let remove = one(executed(
        GameEvent::RemoveCounters {
            target: CounterTarget::Player(p(2)),
            kind: "poison".to_owned(),
            amount: 5,
        },
        Outcome::CountersRemoved { removed: 3 },
    ))
    .remove(0);
    assert_eq!(
        remove,
        TriggerEvent::new(vec![EventKind::CountersRemoved])
            .with_player(p(2))
            .with_field("amount", int_val(3))
            .with_field(
                "kind",
                EventField::Value(FilterValue::name("poison").unwrap())
            )
    );
    // A zone-named kind is kept as the zone the DSL decodes it to.
    for (name, zone) in [("GRAVEYARD", Zone::Graveyard), ("HAND", Zone::Hand)] {
        let e = one(executed(
            GameEvent::RemoveCounters {
                target: CounterTarget::Object(o(1)),
                kind: name.to_owned(),
                amount: 2,
            },
            Outcome::Applied,
        ))
        .remove(0);
        assert_eq!(e.fields.get("kind"), Some(&zone_val(zone)), "{name:?}");
    }
    // A kind that is neither a valid filter name nor a zone name ("ANY", empty) is not a
    // field; a mismatched outcome falls back to the event amount.
    for bad in ["ANY", ""] {
        let e = one(executed(
            GameEvent::RemoveCounters {
                target: CounterTarget::Object(o(1)),
                kind: bad.to_owned(),
                amount: 2,
            },
            Outcome::Applied,
        ))
        .remove(0);
        assert!(!e.fields.contains_key("kind"), "{bad:?}");
        assert_eq!(e.fields.get("amount"), Some(&int_val(2)));
    }
}

// REQ-EVENT-002: tap, untap, turn face up, token, attach and unattach rows; unmapped events
// produce nothing; output keeps batch order.
#[test]
fn req_event_002_mapping_remaining_rows() {
    let same =
        |kind: EventKind, n: u64| TriggerEvent::new(vec![kind]).with_object(Some(o(n)), Some(o(n)));
    let batch = batch_of(vec![
        executed(GameEvent::Tap { object: o(1) }, Outcome::Applied),
        executed(
            GameEvent::TurnBegins { player: p(1) },
            Outcome::TurnBegan { turn: 2 },
        ),
        executed(GameEvent::Untap { object: o(2) }, Outcome::Applied),
        executed(
            GameEvent::PhaseBegins {
                phase: Phase::Combat,
            },
            Outcome::Applied,
        ),
        executed(
            GameEvent::StepBegins { step: Step::Upkeep },
            Outcome::Applied,
        ),
        executed(GameEvent::TurnFaceUp { object: o(3) }, Outcome::Applied),
        executed(GameEvent::TurnFaceDown { object: o(3) }, Outcome::Applied),
        executed(
            GameEvent::BecomeCopy {
                object: o(3),
                copied: o(1),
            },
            Outcome::Applied,
        ),
        executed(
            GameEvent::CreateToken {
                owner: p(1),
                card: CardId::new(9),
                controller: Some(p(2)),
            },
            Outcome::TokenCreated { object: o(20) },
        ),
        executed(
            GameEvent::CreateToken {
                owner: p(1),
                card: CardId::new(10),
                controller: None,
            },
            Outcome::TokenCreated { object: o(21) },
        ),
        executed(
            GameEvent::Attach {
                attachment: o(5),
                target: AttachTarget::Player(p(2)),
            },
            Outcome::Attached {
                timestamp: Timestamp::new(4),
            },
        ),
        executed(GameEvent::Unattach { attachment: o(5) }, Outcome::Applied),
    ]);
    let got = events_from_batch(&batch);
    assert_eq!(
        got,
        vec![
            same(EventKind::Tapped, 1),
            same(EventKind::Untapped, 2),
            same(EventKind::TurnedFaceUp, 3),
            // CR 111.2: the creator is the owner; the controller is its own field.
            // CR 701.7a, CR 603.6a: `to` is the battlefield; no `from`.
            TriggerEvent::new(vec![EventKind::TokenCreated, EventKind::Enters])
                .with_object(None, Some(o(20)))
                .with_player(p(1))
                .with_field("to", zone_val(Zone::Battlefield))
                .with_field("controller", EventField::Player(p(2))),
            TriggerEvent::new(vec![EventKind::TokenCreated, EventKind::Enters])
                .with_object(None, Some(o(21)))
                .with_player(p(1))
                .with_field("to", zone_val(Zone::Battlefield))
                .with_field("controller", EventField::Player(p(1))),
            same(EventKind::Attached, 5),
            same(EventKind::Unattached, 5),
        ]
    );
    // A token event with an unexpected outcome has no post-event object.
    let e = one(executed(
        GameEvent::CreateToken {
            owner: p(1),
            card: CardId::new(9),
            controller: None,
        },
        Outcome::Applied,
    ))
    .remove(0);
    assert_eq!(e.subject_post, None);
    assert!(events_from_batch(&batch_of(Vec::new())).is_empty());
    // CR 603.6a, CR 701.7a: an ENTERS matcher (post-event) matches each token's new id; no
    // ZONE_CHANGE (a token has no origin zone).
    assert!(
        got.iter()
            .all(|e| !e.kinds.contains(&EventKind::ZoneChange))
    );
    let env = FakeEnv::new()
        .post(reg(
            9,
            1,
            "enters",
            with_bind(matcher(EventKind::Enters), "object", "it"),
        ))
        .post(reg(
            9,
            1,
            "made",
            with_subject(
                matcher(EventKind::TokenCreated),
                EventSubject::Players(PlayerGroup::AllPlayers),
            ),
        ));
    let mut env = env;
    env.players.insert((StateView::PostEvent, p(1)));
    let fired = run(&env, &got);
    let enters: Vec<_> = fired.iter().filter(|t| t.ability_id == "enters").collect();
    assert_eq!(enters.len(), 2);
    assert_eq!(
        enters[0].bindings.get("it"),
        Some(&EventField::Object(o(20)))
    );
    assert_eq!(
        enters[1].bindings.get("it"),
        Some(&EventField::Object(o(21)))
    );
    assert!(enters.iter().all(|t| t.look_back.is_none()));
    // The TOKEN_CREATED player subject is the owner (P1) for both tokens.
    assert_eq!(fired.iter().filter(|t| t.ability_id == "made").count(), 2);
    env.players.clear();
    env.players.insert((StateView::PostEvent, p(2)));
    assert!(run(&env, &got).iter().all(|t| t.ability_id != "made"));
}

// ---------------------------------------------------------------------------------------------
// Real executed batches (REQ-EVENT-002 with REQ-EVENT-001's EventContext)
// ---------------------------------------------------------------------------------------------

struct Provider;

impl CharacteristicsProvider for Provider {
    fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
        Some(Characteristics {
            names: BTreeSet::from(["test.vanilla_2_2".to_owned()]),
            mana_cost: None,
            color_indicator: None,
            colors: ColorBitmask::NONE,
            supertypes: BTreeSet::new(),
            card_types: BTreeSet::from(["Creature".to_owned()]),
            subtypes: BTreeSet::new(),
            rules_text: TextTokenStream(Vec::new()),
            abilities: Vec::new(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            hand_modifier: None,
            life_modifier: None,
        })
    }

    fn counters(&self, _id: ObjectId) -> BTreeMap<String, i64> {
        BTreeMap::new()
    }

    fn attached_here(&self, _id: ObjectId) -> Vec<ObjectId> {
        Vec::new()
    }
}

// REQ-EVENT-002: events executed through EventContext::execute_batch and read back from the
// executed-event log normalise as the mapping says; the CR 603.10a example then triggers once
// per creature from the pre-event view.
#[test]
fn req_event_002_mapping_from_real_execution() {
    let mut game = GameState::new(7, 2, 20).unwrap();
    game.set_turn_number(1);
    game.set_active_player(Some(p(1))).unwrap();
    let mut zones = Zones::new(&[p(1), p(2)], ZonesConfig::default());
    let mut ids = IdGenerator::new();
    let mut permanents = Permanents::new();
    let mut memories = MemoryTable::new();
    let mut lki = LkiStore::new();
    let mut events = EventState::new();
    let provider = Provider;
    let mut create = |card: u64, owner: u64, kind: ZoneKind| {
        let mut req = MoveRequest::new(kind);
        req.position = Position::Bottom;
        let id = zones
            .create_object(&mut ids, CardId::new(card), p(owner), req)
            .unwrap();
        if kind == ZoneKind::Battlefield {
            permanents.enter(id, p(owner), 1).unwrap();
        }
        id
    };
    let a = create(1, 1, ZoneKind::Battlefield);
    let b = create(2, 2, ZoneKind::Battlefield);
    let lib = create(3, 1, ZoneKind::Library);
    let mut ctx = EventContext {
        game: &mut game,
        zones: &mut zones,
        ids: &mut ids,
        permanents: &mut permanents,
        memories: &mut memories,
        lki: &mut lki,
        events: &mut events,
        provider: &provider,
    };
    let destroy = |object| GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(ZoneKind::Graveyard),
        cause: ZoneChangeCause::Destroy,
    };
    ctx.execute_batch(&EventBatch::from_events(vec![
        destroy(a),
        destroy(b),
        GameEvent::Draw { player: p(1) },
        GameEvent::LifeChange {
            player: p(2),
            delta: 2,
        },
    ]))
    .unwrap();
    let logged = &events.log().batches_since(0)[0];
    let tes = events_from_batch(logged);
    assert_eq!(tes.len(), 4);
    for (e, old) in tes.iter().zip([a, b]) {
        assert_eq!(e.subject_pre, Some(old));
        assert_ne!(e.subject_post, Some(old));
        assert!(e.kinds.contains(&EventKind::Dies));
        assert!(e.kinds.contains(&EventKind::Destroyed));
        assert_eq!(e.fields.get("from"), Some(&zone_val(Zone::Battlefield)));
        assert_eq!(e.fields.get("to"), Some(&zone_val(Zone::Graveyard)));
    }
    // CR 121.1: the draw is also a library-to-hand zone change, with a new id.
    let draw = &tes[2];
    assert_eq!(
        kinds_set(draw),
        BTreeSet::from([EventKind::Drawn, EventKind::ZoneChange])
    );
    assert_eq!(draw.subject_player, Some(p(1)));
    assert_eq!(draw.subject_pre, Some(lib));
    assert!(draw.subject_post.is_some() && draw.subject_post != Some(lib));
    assert_eq!(draw.fields.get("from"), Some(&zone_val(Zone::Library)));
    assert_eq!(draw.fields.get("to"), Some(&zone_val(Zone::Hand)));
    assert_eq!(
        tes[3],
        TriggerEvent::new(vec![EventKind::LifeGained])
            .with_player(p(2))
            .with_field("amount", int_val(2))
    );
    let env = FakeEnv::new()
        .pre(reg(
            99,
            1,
            "a1",
            with_subject(
                matcher(EventKind::Dies),
                EventSubject::Object(creature_sel()),
            ),
        ))
        .creature(StateView::PreEvent, a.get())
        .creature(StateView::PreEvent, b.get());
    let got = detect(&env, &mut memories, logged.turn, &tes);
    assert_eq!(got.len(), 2);
}

// ---------------------------------------------------------------------------------------------
// CodeRabbit round 1 regressions (REQ-EVENT-002)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002: CR 603.2c, CR 603.10a. A ONE_OR_MORE matcher yields one trigger per ability
// instance even when its events match in different views (a visible card put into a hand looks
// back, a hidden one does not); view, look-back and bindings come from the first match.
#[test]
fn req_event_002_cr_603_2c_one_or_more_groups_across_views() {
    let m = with_bind(
        with_batch(
            with_filter(
                matcher(EventKind::ZoneChange),
                "to",
                FilterValue::Zone(Zone::Hand),
            ),
            DslEventBatch::OneOrMore,
        ),
        "object",
        "it",
    );
    let visible = zone_event(
        vec![EventKind::ZoneChange, EventKind::ReturnedToHand],
        1,
        11,
        Zone::Exile,
        Zone::Hand,
    );
    let hidden = zone_event(
        vec![EventKind::ZoneChange],
        2,
        12,
        Zone::Library,
        Zone::Hand,
    );
    let mut env = FakeEnv::new().both(reg(9, 1, "a1", m));
    env.visible.insert(o(1));
    let got = run(&env, &[visible.clone(), hidden.clone()]);
    assert_eq!(got.len(), 1, "CR 603.2c: one trigger per ability instance");
    assert_eq!(got[0].event_indices, vec![0, 1]);
    assert_eq!(got[0].look_back, Some(LookBackKind::ToHandOrLibrary));
    assert_eq!(got[0].bindings.get("it"), Some(&EventField::Object(o(1))));
    // Reversed: the first match is post-event, so are its look-back and bindings.
    let got = run(&env, &[hidden, visible]);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].event_indices, vec![0, 1]);
    assert_eq!(got[0].look_back, None);
    assert_eq!(got[0].bindings.get("it"), Some(&EventField::Object(o(12))));
}

// REQ-EVENT-002: SRS 8.2 item 8, CR 603.2h. A departing source's used limit is read from the
// pre-event Memory; no Memory entry is recreated for its retired id; a surviving source's
// counters are written; accepted candidates within one call count against the limit.
#[test]
fn req_event_002_cr_603_2h_limit_of_departing_source() {
    let dies = zone_event(
        vec![
            EventKind::ZoneChange,
            EventKind::LeavesBattlefield,
            EventKind::Dies,
            EventKind::PutIntoGraveyard,
        ],
        1,
        11,
        Zone::Battlefield,
        Zone::Graveyard,
    );
    let r = limited(reg(5, 1, "a1", matcher(EventKind::Dies)), Some(1), None);
    let surviving = FakeEnv::new().both(r.clone());
    let departed = FakeEnv::new().pre(r);
    // An earlier batch this turn used the limit while the source survived.
    let mut memory = MemoryTable::new();
    assert_eq!(
        detect(&surviving, &mut memory, 3, std::slice::from_ref(&dies)).len(),
        1
    );
    let pre_memory = memory.clone();
    // (a) The source dies in this batch: its live Memory entry is gone.
    let mut post = MemoryTable::new();
    assert!(
        detect_with_pre_event_memory(
            &departed,
            &pre_memory,
            &mut post,
            3,
            std::slice::from_ref(&dies)
        )
        .is_empty()
    );
    assert!(post.get(o(5)).is_none(), "no entry for a departed source");
    // (b) Next turn it triggers again, still without an entry.
    assert_eq!(
        detect_with_pre_event_memory(
            &departed,
            &pre_memory,
            &mut post,
            4,
            std::slice::from_ref(&dies)
        )
        .len(),
        1
    );
    assert!(post.get(o(5)).is_none());
    // (c) A surviving source's counters are written to `memory`.
    let mut post = MemoryTable::new();
    assert_eq!(
        detect_with_pre_event_memory(
            &surviving,
            &pre_memory,
            &mut post,
            4,
            std::slice::from_ref(&dies)
        )
        .len(),
        1
    );
    let mem = post.get(o(5)).unwrap();
    assert_eq!(
        mem.choice("trigger.limit.turn:a1"),
        Some(&ChoiceValue::Number(4))
    );
    assert_eq!(
        mem.choice("trigger.limit.turn_count:a1"),
        Some(&ChoiceValue::Number(1))
    );
    assert_eq!(
        mem.choice("trigger.limit.game_count:a1"),
        Some(&ChoiceValue::Number(2))
    );
    // (d) perTurn 2 within one call: pre-event count plus the in-call tally.
    let three = [dies.clone(), dies.clone(), dies];
    let r2 = limited(reg(6, 1, "b1", matcher(EventKind::Dies)), Some(2), None);
    let departed = FakeEnv::new().pre(r2.clone());
    let mut post = MemoryTable::new();
    let got = detect_with_pre_event_memory(&departed, &MemoryTable::new(), &mut post, 3, &three);
    assert_eq!(
        got.iter().map(|t| t.event_indices[0]).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(post.get(o(6)).is_none());
    let mut pre_used = MemoryTable::new();
    let m = pre_used.get_mut_or_default(o(6));
    m.set_choice("trigger.limit.turn:b1", ChoiceValue::Number(3));
    m.set_choice("trigger.limit.turn_count:b1", ChoiceValue::Number(1));
    assert_eq!(
        detect_with_pre_event_memory(&departed, &pre_used, &mut post, 3, &three).len(),
        1
    );
    let surviving = FakeEnv::new().both(r2);
    let mut post = MemoryTable::new();
    assert_eq!(
        detect_with_pre_event_memory(&surviving, &pre_used, &mut post, 3, &three).len(),
        1
    );
    assert_eq!(
        post.get(o(6))
            .unwrap()
            .choice("trigger.limit.turn_count:b1"),
        Some(&ChoiceValue::Number(2))
    );
}

// REQ-EVENT-002: a counter kind spelled as a zone is kept as the zone the DSL decodes; an
// ordinary name stays a name; ANY is no field and a wildcard filter still passes.
#[test]
fn req_event_002_counter_kind_zone_spelling() {
    let put = |kind: &str| {
        one(executed(
            GameEvent::AddCounters {
                target: CounterTarget::Object(o(4)),
                kind: kind.to_owned(),
                amount: 1,
            },
            Outcome::Applied,
        ))
        .remove(0)
    };
    let hand = put("HAND");
    assert_eq!(hand.fields.get("kind"), Some(&zone_val(Zone::Hand)));
    assert_eq!(
        put("poison").fields.get("kind"),
        Some(&EventField::Value(FilterValue::name("poison").unwrap()))
    );
    let any = put("ANY");
    assert!(!any.fields.contains_key("kind"));
    let exact = with_filter(
        matcher(EventKind::CountersPut),
        "kind",
        FilterValue::Zone(Zone::Hand),
    );
    let env = FakeEnv::new().post(reg(9, 1, "a1", exact));
    assert_eq!(run(&env, std::slice::from_ref(&hand)).len(), 1);
    assert!(run(&env, std::slice::from_ref(&any)).is_empty());
    let wildcard = with_filter(matcher(EventKind::CountersPut), "kind", FilterValue::Any);
    let env = FakeEnv::new().post(reg(9, 1, "a1", wildcard));
    assert_eq!(run(&env, &[any]).len(), 1);
}

// REQ-EVENT-002: CR 400.8, CR 406.7. An exiled object exiled again gets a new id and is
// EXILED, without ZONE_CHANGE; the cause kind still applies; ordinary exiling is unchanged.
#[test]
fn req_event_002_cr_400_8_re_exile() {
    let again = one(zone_change_exec(
        1,
        2,
        ZoneId::Exile,
        ZoneId::Exile,
        ZoneChangeCause::Exile,
    ))
    .remove(0);
    assert_eq!(again.kinds, vec![EventKind::Exiled]);
    assert_eq!(
        (again.subject_pre, again.subject_post),
        (Some(o(1)), Some(o(2)))
    );
    assert_eq!(again.fields.get("from"), Some(&zone_val(Zone::Exile)));
    assert_eq!(again.fields.get("to"), Some(&zone_val(Zone::Exile)));
    let destroyed = one(zone_change_exec(
        1,
        2,
        ZoneId::Exile,
        ZoneId::Exile,
        ZoneChangeCause::Destroy,
    ))
    .remove(0);
    assert_eq!(
        kinds_set(&destroyed),
        BTreeSet::from([EventKind::Exiled, EventKind::Destroyed])
    );
    let env = FakeEnv::new().post(reg(9, 1, "a1", matcher(EventKind::Exiled)));
    assert_eq!(run(&env, std::slice::from_ref(&again)).len(), 1);
    let zc = FakeEnv::new().both(reg(9, 1, "a1", matcher(EventKind::ZoneChange)));
    assert!(run(&zc, &[again]).is_empty());
    let first = one(zone_change_exec(
        1,
        2,
        ZoneId::Battlefield,
        ZoneId::Exile,
        ZoneChangeCause::Exile,
    ))
    .remove(0);
    assert!(first.kinds.contains(&EventKind::Exiled));
    assert!(first.kinds.contains(&EventKind::ZoneChange));
}

// REQ-EVENT-002: CR 701.21a, CR 603.10a. P2 sacrifices a permanent P1 owns: the subject player
// is P2, read from the LKI the executed batch captured; without LKI it is absent.
#[test]
fn req_event_002_cr_701_21a_sacrifice_subject_is_controller() {
    let mut game = GameState::new(7, 2, 20).unwrap();
    game.set_turn_number(1);
    game.set_active_player(Some(p(1))).unwrap();
    let mut zones = Zones::new(&[p(1), p(2)], ZonesConfig::default());
    let mut ids = IdGenerator::new();
    let mut permanents = Permanents::new();
    let mut memories = MemoryTable::new();
    let mut lki = LkiStore::new();
    let mut events = EventState::new();
    let provider = Provider;
    let mut req = MoveRequest::new(ZoneKind::Battlefield);
    req.controller = Some(p(2));
    let x = zones
        .create_object(&mut ids, CardId::new(1), p(1), req)
        .unwrap();
    permanents.enter(x, p(2), 1).unwrap();
    let mut ctx = EventContext {
        game: &mut game,
        zones: &mut zones,
        ids: &mut ids,
        permanents: &mut permanents,
        memories: &mut memories,
        lki: &mut lki,
        events: &mut events,
        provider: &provider,
    };
    ctx.execute_batch(&EventBatch::from_events(vec![GameEvent::ZoneChange {
        object: x,
        request: MoveRequest::new(ZoneKind::Graveyard),
        cause: ZoneChangeCause::Sacrifice,
    }]))
    .unwrap();
    let logged = &events.log().batches_since(0)[0];
    let tes = events_from_batch_with_lki(logged, &lki);
    assert_eq!(tes.len(), 1);
    let e = &tes[0];
    assert!(e.kinds.contains(&EventKind::Sacrificed));
    assert_eq!(e.subject_player, Some(p(2)));
    assert_eq!(e.subject_pre, Some(x));
    assert!(e.subject_post.is_some() && e.subject_post != Some(x));
    assert_eq!(events_from_batch(logged)[0].subject_player, None);
    let m = with_subject(
        matcher(EventKind::Sacrificed),
        EventSubject::Players(PlayerGroup::AllPlayers),
    );
    let mut env = FakeEnv::new().pre(reg(9, 1, "a1", m));
    env.players.insert((StateView::PreEvent, p(2)));
    let got = run(&env, &tes);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].look_back, Some(LookBackKind::Sacrificed));
    env.players.clear();
    env.players.insert((StateView::PreEvent, p(1)));
    assert!(run(&env, &tes).is_empty());
}

// REQ-EVENT-002: CR 121.1, CR 400.2, CR 603.10a. A draw is DRAWN and a library-to-hand
// ZONE_CHANGE (not RETURNED_TO_HAND) with old and new ids; a to-hand zone-change matcher looks
// back only when the env says the card was visible to all.
#[test]
fn req_event_002_cr_121_1_draw_is_a_zone_change() {
    let drawn = |old: u64, new: u64| {
        one(executed(
            GameEvent::Draw { player: p(2) },
            Outcome::ZoneChange(Moved {
                old: o(old),
                new: o(new),
                from: ZoneId::Library(p(2)),
                to: ZoneId::Hand(p(2)),
            }),
        ))
    };
    let got = drawn(1, 2);
    assert_eq!(
        got,
        vec![
            TriggerEvent::new(vec![EventKind::Drawn, EventKind::ZoneChange])
                .with_object(Some(o(1)), Some(o(2)))
                .with_field("from", zone_val(Zone::Library))
                .with_field("to", zone_val(Zone::Hand))
                .with_player(p(2))
        ]
    );
    // A malformed same-id outcome keeps the plain draw shape.
    assert_eq!(
        drawn(1, 1),
        vec![TriggerEvent::new(vec![EventKind::Drawn]).with_player(p(2))]
    );
    let to_hand = with_filter(
        matcher(EventKind::ZoneChange),
        "to",
        FilterValue::Zone(Zone::Hand),
    );
    let mut env = FakeEnv::new()
        .both(reg(9, 1, "zc", to_hand))
        .both(reg(9, 1, "dr", matcher(EventKind::Drawn)))
        .both(reg(9, 1, "rh", matcher(EventKind::ReturnedToHand)));
    // Hidden (the usual case for a library): post-event view.
    let fired = run(&env, &got);
    assert_eq!(
        fired
            .iter()
            .map(|t| (t.ability_id.as_str(), t.look_back))
            .collect::<Vec<_>>(),
        vec![("dr", None), ("zc", None)]
    );
    // Visible to all: the zone-change matcher looks back.
    env.visible.insert(o(1));
    let fired = run(&env, &got);
    assert_eq!(
        fired
            .iter()
            .map(|t| (t.ability_id.as_str(), t.look_back))
            .collect::<Vec<_>>(),
        vec![("dr", None), ("zc", Some(LookBackKind::ToHandOrLibrary))]
    );
}

// REQ-EVENT-002: CR 120.3a, CR 120.4b/c. Damage to a player is a DAMAGE_DEALT occurrence then a
// LIFE_LOST occurrence for that player (source and recipient kept); damage to an object is no
// life loss (CR 120.3e).
#[test]
fn req_event_002_cr_120_3a_damage_to_player_is_life_loss() {
    let damage = |target: DamageTarget| {
        executed(
            GameEvent::Damage {
                source: Some(o(3)),
                source_controller: Some(p(1)),
                target,
                amount: 3,
                combat: false,
            },
            Outcome::Applied,
        )
    };
    let tes = events_from_batch(&batch_of(vec![
        damage(DamageTarget::Player(p(2))),
        damage(DamageTarget::Object(o(4))),
        executed(GameEvent::Tap { object: o(4) }, Outcome::Applied),
    ]));
    assert_eq!(
        tes.iter().map(|e| e.kinds.clone()).collect::<Vec<_>>(),
        vec![
            vec![EventKind::DamageDealt],
            vec![EventKind::LifeLost],
            vec![EventKind::DamageDealt],
            vec![EventKind::Tapped],
        ]
    );
    assert_eq!(
        tes[1],
        TriggerEvent::new(vec![EventKind::LifeLost])
            .with_player(p(2))
            .with_field("amount", int_val(3))
            .with_object(Some(o(3)), Some(o(3)))
            .with_field("recipient", EventField::Player(p(2)))
    );
    let m = with_bind(
        with_subject(
            matcher(EventKind::LifeLost),
            EventSubject::Players(PlayerGroup::AllPlayers),
        ),
        "amount",
        "n",
    );
    let mut env = FakeEnv::new().post(reg(9, 1, "a1", m));
    env.players.insert((StateView::PostEvent, p(2)));
    let got = run(&env, &tes);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].event_indices, vec![1]);
    assert_eq!(got[0].bindings.get("n"), Some(&int_val(3)));
}

// REQ-EVENT-002: CR 701.8a, CR 603.6c, CR 603.10a. An observer destroyed together with two
// creatures sees them destroyed through the pre-event view; it does not trigger from the
// post-event list. CR 701.8b: DESTROYED stays distinct from DIES.
#[test]
fn req_event_002_cr_701_8a_destroyed_looks_back() {
    let events = events_from_batch(&destroy_all_batch());
    let m = with_subject(
        matcher(EventKind::Destroyed),
        EventSubject::Object(creature_sel()),
    );
    let env = FakeEnv::new()
        .pre(reg(3, 1, "a1", m.clone()))
        .creature(StateView::PreEvent, 1)
        .creature(StateView::PreEvent, 2);
    let got = run(&env, &events);
    assert_eq!(got.len(), 2);
    assert!(
        got.iter()
            .all(|t| t.look_back == Some(LookBackKind::LeavesBattlefield))
    );
    let env = FakeEnv::new()
        .post(reg(3, 1, "a1", m))
        .creature(StateView::PreEvent, 1)
        .creature(StateView::PreEvent, 2);
    assert!(run(&env, &events).is_empty());
    // A sacrifice to the graveyard dies but is not destroyed.
    let sac = one(zone_change_exec(
        1,
        11,
        ZoneId::Battlefield,
        ZoneId::Graveyard(p(1)),
        ZoneChangeCause::Sacrifice,
    ));
    let env = FakeEnv::new().pre(reg(3, 1, "a1", matcher(EventKind::Destroyed)));
    assert!(run(&env, &sac).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Waiting-trigger queue (REQ-EVENT-002, CR 603.3)
// ---------------------------------------------------------------------------------------------

fn waiting(source: u64, controller: u64) -> WaitingTrigger {
    WaitingTrigger {
        source: o(source),
        controller: p(controller),
        ability_id: "a1".to_owned(),
        look_back: None,
        event_indices: vec![0],
        bindings: BTreeMap::new(),
    }
}

// REQ-EVENT-002: CR 603.3, the queue is FIFO in creation order; take_all drains it;
// take_for_controller removes only that controller's triggers and keeps the others' order (no
// APNAP ordering, CR 603.3b is the stack unit's job).
#[test]
fn req_event_002_waiting_queue() {
    let mut q = WaitingTriggers::new();
    assert!(q.is_empty());
    assert_eq!(q, WaitingTriggers::default());
    q.push_all(vec![waiting(1, 2), waiting(2, 1)]);
    q.push_all(vec![waiting(3, 2), waiting(4, 1), waiting(5, 2)]);
    assert_eq!(q.len(), 5);
    assert!(!q.is_empty());
    assert_eq!(
        q.iter().map(|t| t.source).collect::<Vec<_>>(),
        vec![o(1), o(2), o(3), o(4), o(5)]
    );
    let mine = q.take_for_controller(p(2));
    assert_eq!(
        mine.iter().map(|t| t.source).collect::<Vec<_>>(),
        vec![o(1), o(3), o(5)]
    );
    assert_eq!(
        q.iter().map(|t| t.source).collect::<Vec<_>>(),
        vec![o(2), o(4)]
    );
    assert!(q.take_for_controller(p(3)).is_empty());
    assert_eq!(q.len(), 2);
    let all = q.take_all();
    assert_eq!(
        all.iter().map(|t| t.source).collect::<Vec<_>>(),
        vec![o(2), o(4)]
    );
    assert!(q.is_empty());
    assert!(q.take_all().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Review round 2 (REQ-EVENT-002)
// ---------------------------------------------------------------------------------------------

/// A [`FakeEnv`] whose sources in `alive` survive the batch (e.g. phased out, CR 702.26d) even
/// when no post-event registration lists them. Records every `source_survives` query.
struct SurvivesEnv {
    inner: FakeEnv,
    alive: BTreeSet<ObjectId>,
    asked: RefCell<Vec<ObjectId>>,
}

impl TriggerEnv for SurvivesEnv {
    fn registrations(&self, view: StateView) -> Vec<TriggerRegistration> {
        self.inner.registrations(view)
    }

    fn object_matches(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        object: ObjectId,
        selector: &ObjectSelectorAST,
    ) -> bool {
        self.inner.object_matches(view, reg, object, selector)
    }

    fn player_matches(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        player: PlayerId,
        group: &PlayerGroup,
    ) -> bool {
        self.inner.player_matches(view, reg, player, group)
    }

    fn predicate_holds(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        pred: &PredicateAST,
        field: Option<&EventField>,
    ) -> bool {
        self.inner.predicate_holds(view, reg, pred, field)
    }

    fn condition_holds(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        pred: &PredicateAST,
        bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        self.inner.condition_holds(view, reg, pred, bindings)
    }

    fn is_visible_to_all(&self, object: ObjectId) -> bool {
        self.inner.is_visible_to_all(object)
    }

    fn source_survives(&self, source: ObjectId) -> bool {
        self.asked.borrow_mut().push(source);
        self.alive.contains(&source)
    }
}

// REQ-EVENT-002: CR 603.2h, CR 603.10b, CR 702.26d. A perGame:1 phases-out ability whose source
// phases out (so it is listed only pre-event) still exists: its use is recorded, and the next
// phase-out does not trigger it again.
#[test]
fn req_event_002_cr_702_26d_phased_out_source_keeps_limit() {
    let phased = TriggerEvent::new(vec![EventKind::PhasedOut]).with_object(Some(o(5)), Some(o(5)));
    let r = limited(
        reg(
            5,
            1,
            "a1",
            with_subject(
                matcher(EventKind::PhasedOut),
                EventSubject::Object(self_sel()),
            ),
        ),
        None,
        Some(1),
    );
    let env = SurvivesEnv {
        inner: FakeEnv::new().pre(r),
        alive: BTreeSet::from([o(5)]),
        asked: RefCell::new(Vec::new()),
    };
    let mut memory = MemoryTable::new();
    let first = detect_with_pre_event_memory(
        &env,
        &MemoryTable::new(),
        &mut memory,
        1,
        &[phased.clone(), phased.clone()],
    );
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].look_back, Some(LookBackKind::PhasedOut));
    // Asked once per source per call (cached).
    assert_eq!(*env.asked.borrow(), vec![o(5)]);
    assert_eq!(
        memory
            .get(o(5))
            .unwrap()
            .choice("trigger.limit.game_count:a1"),
        Some(&ChoiceValue::Number(1))
    );
    // It phases in and out again later: the pre-event Memory holds the used limit.
    let pre_memory = memory.clone();
    let second = detect_with_pre_event_memory(
        &env,
        &pre_memory,
        &mut memory,
        2,
        std::slice::from_ref(&phased),
    );
    assert!(second.is_empty());
}

// REQ-EVENT-002: CR 400.7, CR 603.2h. A source retired by a zone change (source_survives false)
// still triggers but gets no Memory entry.
#[test]
fn req_event_002_cr_400_7_retired_source_gets_no_entry() {
    let phased = TriggerEvent::new(vec![EventKind::PhasedOut]).with_object(Some(o(5)), Some(o(5)));
    let r = limited(
        reg(5, 1, "a1", matcher(EventKind::PhasedOut)),
        None,
        Some(1),
    );
    let env = SurvivesEnv {
        inner: FakeEnv::new().pre(r),
        alive: BTreeSet::new(),
        asked: RefCell::new(Vec::new()),
    };
    let mut memory = MemoryTable::new();
    let got = detect_with_pre_event_memory(
        &env,
        &MemoryTable::new(),
        &mut memory,
        1,
        std::slice::from_ref(&phased),
    );
    assert_eq!(got.len(), 1);
    assert!(memory.get(o(5)).is_none());
    assert_eq!(*env.asked.borrow(), vec![o(5)]);
}

// REQ-EVENT-002: CR 603.2h. The default `source_survives` is post-event registration
// membership by source: a source listed post-event (under any ability) survives, one listed
// only pre-event does not.
#[test]
fn req_event_002_default_source_survives_is_post_event_membership() {
    let r = limited(
        reg(5, 1, "a1", matcher(EventKind::PhasedOut)),
        None,
        Some(1),
    );
    let pre_only = FakeEnv::new().pre(r.clone());
    assert!(!pre_only.source_survives(o(5)));
    let other_ability =
        FakeEnv::new()
            .pre(r.clone())
            .post(reg(5, 1, "b1", matcher(EventKind::Tapped)));
    assert!(other_ability.source_survives(o(5)));
    assert!(!other_ability.source_survives(o(6)));
    let phased = TriggerEvent::new(vec![EventKind::PhasedOut]).with_object(Some(o(5)), Some(o(5)));
    let mut memory = MemoryTable::new();
    assert_eq!(
        detect(&pre_only, &mut memory, 1, std::slice::from_ref(&phased)).len(),
        1
    );
    assert!(memory.get(o(5)).is_none());
    let mut memory = MemoryTable::new();
    assert_eq!(
        detect(
            &other_ability,
            &mut memory,
            1,
            std::slice::from_ref(&phased)
        )
        .len(),
        1
    );
    assert_eq!(
        memory
            .get(o(5))
            .unwrap()
            .choice("trigger.limit.game_count:a1"),
        Some(&ChoiceValue::Number(1))
    );
}

// REQ-EVENT-002: CR 701.9a, CR 701.17a. A discard or mill's subject player is the owner of the
// hand or library the card left; other causes (except sacrifice) have none.
#[test]
fn req_event_002_cr_701_9a_701_17a_discard_mill_subject_player() {
    let discard = one(zone_change_exec(
        1,
        2,
        ZoneId::Hand(p(2)),
        ZoneId::Graveyard(p(2)),
        ZoneChangeCause::Discard,
    ))
    .remove(0);
    assert_eq!(discard.subject_player, Some(p(2)));
    assert_eq!(
        (discard.subject_pre, discard.subject_post),
        (Some(o(1)), Some(o(2)))
    );
    assert_eq!(
        kinds_set(&discard),
        BTreeSet::from([
            EventKind::ZoneChange,
            EventKind::PutIntoGraveyard,
            EventKind::Discarded
        ])
    );
    assert_eq!(discard.fields.get("from"), Some(&zone_val(Zone::Hand)));
    assert_eq!(discard.fields.get("to"), Some(&zone_val(Zone::Graveyard)));
    let mill = one(zone_change_exec(
        3,
        4,
        ZoneId::Library(p(2)),
        ZoneId::Graveyard(p(2)),
        ZoneChangeCause::Mill,
    ))
    .remove(0);
    assert_eq!(mill.subject_player, Some(p(2)));
    assert_eq!(
        (mill.subject_pre, mill.subject_post),
        (Some(o(3)), Some(o(4)))
    );
    assert_eq!(
        kinds_set(&mill),
        BTreeSet::from([
            EventKind::ZoneChange,
            EventKind::PutIntoGraveyard,
            EventKind::Milled
        ])
    );
    // A source zone with no owner gives no subject player.
    let odd = one(zone_change_exec(
        5,
        6,
        ZoneId::Battlefield,
        ZoneId::Graveyard(p(1)),
        ZoneChangeCause::Discard,
    ))
    .remove(0);
    assert_eq!(odd.subject_player, None);
    // Destroy keeps no subject player.
    let destroyed = one(zone_change_exec(
        7,
        8,
        ZoneId::Battlefield,
        ZoneId::Graveyard(p(2)),
        ZoneChangeCause::Destroy,
    ))
    .remove(0);
    assert_eq!(destroyed.subject_player, None);
    // A Players-subject DISCARDED matcher triggers for P2 and not for P1.
    let m = with_subject(
        matcher(EventKind::Discarded),
        EventSubject::Players(PlayerGroup::AllPlayers),
    );
    let mut env = FakeEnv::new().both(reg(9, 1, "a1", m));
    env.players.insert((StateView::PostEvent, p(2)));
    env.players.insert((StateView::PreEvent, p(2)));
    assert_eq!(run(&env, std::slice::from_ref(&discard)).len(), 1);
    env.players.clear();
    env.players.insert((StateView::PostEvent, p(1)));
    env.players.insert((StateView::PreEvent, p(1)));
    assert!(run(&env, &[discard]).is_empty());
}

// REQ-EVENT-002: CR 603.6a, CR 701.7a. A created token's occurrence has `to` = battlefield and
// no `from`: an ENTERS matcher filtered on to: BATTLEFIELD matches it, one filtered on
// from: GRAVEYARD does not.
#[test]
fn req_event_002_cr_701_7a_token_enters_to_battlefield() {
    let tes = one(executed(
        GameEvent::CreateToken {
            owner: p(1),
            card: CardId::new(9),
            controller: None,
        },
        Outcome::TokenCreated { object: o(20) },
    ));
    assert_eq!(tes.len(), 1);
    assert_eq!(tes[0].fields.get("to"), Some(&zone_val(Zone::Battlefield)));
    assert!(!tes[0].fields.contains_key("from"));
    assert!(!tes[0].kinds.contains(&EventKind::ZoneChange));
    let to_bf = with_filter(
        matcher(EventKind::Enters),
        "to",
        FilterValue::Zone(Zone::Battlefield),
    );
    let got = run(&FakeEnv::new().post(reg(9, 1, "a1", to_bf)), &tes);
    assert_eq!(got.len(), 1);
    let from_gy = with_filter(
        matcher(EventKind::Enters),
        "from",
        FilterValue::Zone(Zone::Graveyard),
    );
    assert!(run(&FakeEnv::new().post(reg(9, 1, "a1", from_gy)), &tes).is_empty());
}
