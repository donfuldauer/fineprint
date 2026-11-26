// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests: look-back trigger detection sees effect-granted characteristics (#184 FP-145;
//! REQ-LAYER-001, REQ-EVENT-002, REQ-CORE-003; CR 603.10a, 608.2h, 613.1d). A
//! leaves-the-battlefield trigger whose subject selector is satisfied only by an
//! effect-granted card type triggers when the zone change was executed with the snapshot
//! `LayeredProvider` (the LKI holds the evaluated characteristics) and does not with the
//! base-only control. Black-box: written from the work-item brief only.

mod spec_layers_consumers_support;

use std::collections::BTreeMap;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::layers::LayeredProvider;
use fineprint_core::memory::{CharacteristicsProvider, LkiStore, MemoryTable};
use fineprint_core::objects::Characteristics;
use fineprint_core::state::GameEnvironment;
use fineprint_core::triggers::{
    EventField, LookBackKind, StateView, TriggerEnv, TriggerRegistration, WaitingTrigger, detect,
    events_from_batch_with_lki,
};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::ability::{
    AbilityBase, EventKind, EventMatcher, EventSubject, FilterValue, Trigger, TriggeredAbilityAST,
};
use fineprint_dsl::expr::{ObjectSelectorAST, PlayerGroup, PredicateAST, SelectorFilter, Zone};
use spec_layers_consumers_support::*;

/// An opaque object selector; the environment below answers on the object's characteristics
/// only ("has the granted card type").
fn subject_selector() -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(PredicateAST::IsCreature)),
    })
}

/// A triggered ability `id` on `matcher`.
fn triggered(id: &str, matcher: EventMatcher) -> TriggeredAbilityAST {
    TriggeredAbilityAST {
        base: AbilityBase::new(id),
        trigger: Trigger::Event(matcher),
        intervening_if: None,
        optional: None,
        limit: None,
        targets: None,
        modes: None,
        effect: Vec::new(),
    }
}

/// The look-back matchers used: LEAVES_BATTLEFIELD, and ZONE_CHANGE from the battlefield, each
/// with the object subject.
fn look_back_matchers() -> Vec<(&'static str, EventMatcher)> {
    let mut ltb = EventMatcher::new(EventKind::LeavesBattlefield);
    ltb.subject = Some(EventSubject::Object(subject_selector()));
    let mut zc = EventMatcher::new(EventKind::ZoneChange);
    zc.filters = Some(BTreeMap::from([(
        "from".to_string(),
        FilterValue::Zone(Zone::Battlefield),
    )]));
    zc.subject = Some(EventSubject::Object(subject_selector()));
    vec![("test.ltb_granted", ltb), ("test.zc_from_bf_granted", zc)]
}

/// A trigger environment answering `object_matches` from the LKI (pre-event view, the object
/// has left) or else from `current` (the evaluated view of the objects that are still there):
/// the object matches iff it has the [`GRANTED`] card type.
struct LkiEnv<'a> {
    regs: Vec<TriggerRegistration>,
    lki: &'a LkiStore,
    current: &'a dyn CharacteristicsProvider,
}

impl LkiEnv<'_> {
    fn chars(&self, view: StateView, object: ObjectId) -> Option<Characteristics> {
        match view {
            // CR 603.10a: look back in time, at how the object looked immediately before.
            StateView::PreEvent => self
                .lki
                .get(object)
                .map(|r| r.characteristics().clone())
                .or_else(|| self.current.characteristics(object)),
            StateView::PostEvent => self.current.characteristics(object),
        }
    }
}

impl TriggerEnv for LkiEnv<'_> {
    fn registrations(&self, _view: StateView) -> Vec<TriggerRegistration> {
        self.regs.clone()
    }

    fn object_matches(
        &self,
        view: StateView,
        _reg: &TriggerRegistration,
        object: ObjectId,
        _selector: &ObjectSelectorAST,
    ) -> bool {
        self.chars(view, object)
            .is_some_and(|c| c.card_types.contains(GRANTED))
    }

    fn player_matches(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _player: PlayerId,
        _group: &PlayerGroup,
    ) -> bool {
        false
    }

    fn predicate_holds(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _pred: &PredicateAST,
        _field: Option<&EventField>,
    ) -> bool {
        false
    }

    fn condition_holds(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _pred: &PredicateAST,
        _bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        false
    }

    fn is_visible_to_all(&self, _object: ObjectId) -> bool {
        true
    }
}

/// The scenario after the move: the table, the watcher's id, and the fixture.
struct Moved {
    f: Fixture,
    watcher: ObjectId,
    events: Vec<fineprint_core::triggers::TriggerEvent>,
}

/// The fixture creature `a` (base: no granted type; evaluated: artifact creature 3/4) and a
/// watcher `w` that stays on the battlefield; `a` leaves the battlefield in one batch executed
/// with the evaluated provider (`evaluated`) or the base-only control.
fn move_subject(evaluated: bool) -> Moved {
    let mut f = fixture();
    let watcher = f.table.create(ZoneKind::Battlefield, P2);
    f.objects
        .put(watcher, creature("test.ltb_watcher_1_1", 1, 1), P2);
    let snap = f.table.game.clone();
    let env = GameEnvironment::new(&snap, &f.objects);
    let layered = LayeredProvider::new(snap.effects(), &env);
    let base = BaseProvider(&f.objects);
    let provider: &dyn CharacteristicsProvider = if evaluated { &layered } else { &base };
    let batch = f
        .table
        .batch(provider, vec![zone_change(f.a, ZoneKind::Graveyard)]);
    let events = events_from_batch_with_lki(&batch, &f.table.lki);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].subject_pre, Some(f.a));
    Moved { f, watcher, events }
}

/// `detect` for the look-back ability `(id, matcher)` on `m.watcher`, with `current` the
/// evaluated view of the objects still present.
fn detect_with(
    m: &Moved,
    id: &str,
    matcher: &EventMatcher,
    current: &dyn CharacteristicsProvider,
) -> Vec<WaitingTrigger> {
    let env = LkiEnv {
        regs: vec![TriggerRegistration {
            source: m.watcher,
            controller: P2,
            ability: triggered(id, matcher.clone()),
        }],
        lki: &m.f.table.lki,
        current,
    };
    let mut mem = MemoryTable::new();
    detect(&env, &mut mem, 1, &m.events)
}

// REQ-LAYER-001, REQ-EVENT-002, REQ-CORE-003; CR 603.10a, 608.2h, 613.1d: a leaves-the-
// battlefield look-back trigger whose subject must have the effect-granted type triggers
// (in the pre-event view) when the LKI was captured with the evaluated provider, and does not
// with the base-only control.
#[test]
fn look_back_trigger_sees_granted_type_in_lki() {
    for (id, matcher) in look_back_matchers() {
        let m = move_subject(true);
        assert!(has_type(m.f.table.lki_of(m.f.a).characteristics(), GRANTED));
        let snap = m.f.table.game.clone();
        let env = GameEnvironment::new(&snap, &m.f.objects);
        let current = LayeredProvider::new(snap.effects(), &env);
        let ts = detect_with(&m, id, &matcher, &current);
        assert_eq!(ts.len(), 1, "{id}: triggers with evaluated LKI");
        assert_eq!(ts[0].source, m.watcher);
        assert_eq!(ts[0].ability_id, id);
        assert_eq!(ts[0].look_back, Some(LookBackKind::LeavesBattlefield));
        assert_eq!(ts[0].event_indices, vec![0]);

        // Control: the same move executed with the base-only provider.
        let c = move_subject(false);
        assert!(!has_type(
            c.f.table.lki_of(c.f.a).characteristics(),
            GRANTED
        ));
        let snap = c.f.table.game.clone();
        let env = GameEnvironment::new(&snap, &c.f.objects);
        let current = LayeredProvider::new(snap.effects(), &env);
        assert!(
            detect_with(&c, id, &matcher, &current).is_empty(),
            "{id}: control (base LKI) does not trigger"
        );
    }
}

// REQ-LAYER-001, REQ-EVENT-002, REQ-CORE-003; CR 603.10a, 608.2h: detection is on the
// pre-event state, so removing the granting effect after the move does not change the result:
// the trigger still triggers from the LKI, although the current evaluation of the same object
// (over a snapshot taken after the removal) no longer has the type.
#[test]
fn removing_the_granting_effect_after_the_move_changes_nothing() {
    for (id, matcher) in look_back_matchers() {
        let mut m = move_subject(true);
        assert!(m.f.table.game.remove_effect(m.f.grant));
        let snap = m.f.table.game.clone();
        let env = GameEnvironment::new(&snap, &m.f.objects);
        let current = LayeredProvider::new(snap.effects(), &env);
        // Control: the current (post-removal) evaluation of the old object lacks the type.
        let now = current
            .characteristics(m.f.a)
            .expect("the world knows the base");
        assert!(!has_type(&now, GRANTED), "control");
        // The LKI is unchanged and the trigger still triggers.
        assert!(has_type(m.f.table.lki_of(m.f.a).characteristics(), GRANTED));
        let ts = detect_with(&m, id, &matcher, &current);
        assert_eq!(ts.len(), 1, "{id}");
        assert_eq!(ts[0].look_back, Some(LookBackKind::LeavesBattlefield));
    }
}

// REQ-EVENT-002, REQ-LAYER-001; CR 603.10a: a look-back trigger evaluated without any LKI (the
// object answered only from the current state, where it no longer is) does not trigger; with
// the evaluated LKI it does. This pins that the trigger result above comes from the LKI.
#[test]
fn trigger_result_comes_from_the_lki() {
    let (id, matcher) = look_back_matchers().remove(0);
    let m = move_subject(true);
    // An environment whose "current" view knows nothing (the object has left every zone).
    struct Nothing;
    impl CharacteristicsProvider for Nothing {
        fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
            None
        }
    }
    assert_eq!(detect_with(&m, id, &matcher, &Nothing).len(), 1);
    let empty = LkiStore::new();
    let env = LkiEnv {
        regs: vec![TriggerRegistration {
            source: m.watcher,
            controller: P2,
            ability: triggered(id, matcher.clone()),
        }],
        lki: &empty,
        current: &Nothing,
    };
    let mut mem = MemoryTable::new();
    assert!(detect(&env, &mut mem, 1, &m.events).is_empty(), "control");
}
