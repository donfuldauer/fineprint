// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the events module: REQ-EVENT-001 (game events, execution primitives, batches,
// LKI wiring, executed-event log), REQ-DSL-003 (history ledger), REQ-CORE-006 / REQ-NFR-006
// (canonical encoding, determinism). Synthetic ids and synthetic characteristics only.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::encode_to_vec;
use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::card::{ColorBitmask, TextTokenStream};

use super::*;
use crate::knowledge::{Knowledge, ObjectView};
use crate::memory::{
    CharacteristicsProvider, LkiStore, MemoryError, MemoryTable, capture_lki_on_move,
};
use crate::objects::Characteristics;
use crate::permanent::{AttachTarget, PermanentError, Permanents};
use crate::state::{GameState, StateError};
use crate::zones::{MoveRequest, Moved, Position, ZoneError, ZoneId, ZoneKind, Zones, ZonesConfig};

// Lost attachment links in executed outcomes (#180 FP-143, CR 701.3d).
mod unattach;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn c(n: u64) -> CardId {
    CardId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

fn chars(name: &str) -> Characteristics {
    Characteristics {
        names: BTreeSet::from([name.to_owned()]),
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
    }
}

/// Synthetic characteristics for every object except the ones in `missing`; provider counters
/// for the ones in `counters`; provider attachments for the ones in `attached`.
#[derive(Clone, Default)]
struct TestProvider {
    missing: BTreeSet<ObjectId>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    attached: BTreeMap<ObjectId, Vec<ObjectId>>,
}

impl CharacteristicsProvider for TestProvider {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        if self.missing.contains(&id) {
            None
        } else {
            Some(chars("test.vanilla_2_2"))
        }
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.attached.get(&id).cloned().unwrap_or_default()
    }
}

#[derive(Clone)]
struct World {
    game: GameState,
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    events: EventState,
    provider: TestProvider,
}

/// Every component except the provider, for whole-world equality.
type Parts = (
    GameState,
    Zones,
    IdGenerator<ObjectId>,
    Permanents,
    MemoryTable,
    LkiStore,
    EventState,
);

impl World {
    /// Two players at 20 life, turn 1, active player 1.
    fn new() -> World {
        let mut game = GameState::new(7, 2, 20).unwrap();
        game.set_turn_number(1);
        game.set_active_player(Some(p(1))).unwrap();
        World {
            game,
            zones: Zones::new(&[p(1), p(2)], ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            provider: TestProvider::default(),
        }
    }

    fn ctx(&mut self) -> EventContext<'_> {
        EventContext {
            game: &mut self.game,
            zones: &mut self.zones,
            ids: &mut self.ids,
            permanents: &mut self.permanents,
            memories: &mut self.memories,
            lki: &mut self.lki,
            events: &mut self.events,
            provider: &self.provider,
        }
    }

    fn parts(&self) -> Parts {
        (
            self.game.clone(),
            self.zones.clone(),
            self.ids.clone(),
            self.permanents.clone(),
            self.memories.clone(),
            self.lki.clone(),
            self.events.clone(),
        )
    }

    /// Creates card `card` owned by `owner` in a zone of kind `kind` (bottom of an ordered
    /// zone); a battlefield object also becomes a permanent under its owner's control.
    fn create(&mut self, card: u64, owner: u64, kind: ZoneKind) -> ObjectId {
        let mut req = MoveRequest::new(kind);
        req.position = Position::Bottom;
        let id = self
            .zones
            .create_object(&mut self.ids, c(card), p(owner), req)
            .unwrap();
        if kind == ZoneKind::Battlefield {
            self.permanents.enter(id, p(owner), 1).unwrap();
        }
        id
    }

    fn exec(&mut self, event: GameEvent) -> Result<Option<ExecutedEvent>, EventError> {
        self.ctx().execute(event)
    }

    fn batch(&mut self, events: Vec<GameEvent>) -> Result<ExecutedBatch, EventError> {
        self.ctx().execute_batch(&EventBatch::from_events(events))
    }

    fn life(&self, player: u64) -> i64 {
        self.game.player(p(player)).unwrap().life()
    }

    fn hist(&self, stat: HistoryStat, scope: HistoryScope, player: Option<u64>) -> i64 {
        self.events.history().query(&HistoryQuery {
            stat,
            scope,
            player: player.map(p),
        })
    }
}

fn zc(object: ObjectId, to: ZoneKind) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause: ZoneChangeCause::Other,
    }
}

fn destroy(object: ObjectId) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(ZoneKind::Graveyard),
        cause: ZoneChangeCause::Destroy,
    }
}

fn moved_of(executed: &ExecutedEvent) -> Moved {
    match executed.outcome {
        Outcome::ZoneChange(m) => m,
        other => panic!("not a zone change: {other:?}"),
    }
}

/// Asserts that `event` is a no-op on `w`: `Ok(None)`, and every component unchanged.
fn assert_noop(w: &mut World, event: GameEvent) {
    let before = w.parts();
    assert_eq!(w.exec(event), Ok(None));
    assert!(w.parts() == before, "a no-op changed state");
}

/// Asserts that `event` fails with `err` on `w` and changes nothing.
fn assert_fails(w: &mut World, event: GameEvent, err: EventError) {
    let before = w.parts();
    assert_eq!(w.exec(event), Err(err));
    assert!(w.parts() == before, "a failed event changed state");
}

// ---------------------------------------------------------------------------------------------
// Model types
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: AppliedEffects (CR 614.5) set operations, ascending iteration.
#[test]
fn req_event_001_applied_effects_set() {
    let mut a = AppliedEffects::new();
    assert!(a.is_empty());
    assert_eq!(a.len(), 0);
    assert!(a.insert(e(5)));
    assert!(a.insert(e(2)));
    assert!(!a.insert(e(5)));
    assert!(a.contains(e(2)));
    assert!(!a.contains(e(3)));
    assert_eq!(a.len(), 2);
    assert!(!a.is_empty());
    assert_eq!(a.iter().collect::<Vec<_>>(), vec![e(2), e(5)]);
    assert_eq!(AppliedEffects::default(), AppliedEffects::new());
}

// REQ-EVENT-001: CR 500.1, each step belongs to its phase.
#[test]
fn req_event_001_cr_500_1_step_phase() {
    use Phase::*;
    let expected = [
        (Step::Untap, Beginning),
        (Step::Upkeep, Beginning),
        (Step::Draw, Beginning),
        (Step::BeginningOfCombat, Combat),
        (Step::DeclareAttackers, Combat),
        (Step::DeclareBlockers, Combat),
        (Step::FirstStrikeDamage, Combat),
        (Step::CombatDamage, Combat),
        (Step::EndOfCombat, Combat),
        (Step::End, Ending),
        (Step::Cleanup, Ending),
    ];
    for (step, phase) in expected {
        assert_eq!(step.phase(), phase);
    }
}

// REQ-EVENT-001: EventInstance and EventBatch constructors and accessors.
#[test]
fn req_event_001_instance_and_batch_api() {
    let ev = GameEvent::Draw { player: p(1) };
    let i = EventInstance::new(ev.clone());
    assert_eq!(i.event(), &ev);
    assert!(i.applied_effects().is_empty());
    let mut applied = AppliedEffects::new();
    applied.insert(e(9));
    let j = i.clone().with_applied(applied.clone());
    assert_eq!(j.applied_effects(), &applied);
    assert_eq!(j.event(), &ev);
    assert_eq!(EventInstance::from(ev.clone()), i);

    let b = EventBatch::new(vec![i.clone(), j.clone()]);
    assert_eq!(b.len(), 2);
    assert!(!b.is_empty());
    assert_eq!(b.events(), &[i.clone(), j][..]);
    let f = EventBatch::from_events(vec![ev]);
    assert_eq!(f.events(), &[i][..]);
    assert!(EventBatch::from_events(Vec::new()).is_empty());
}

// REQ-EVENT-001: EventError Display, source and From conversions.
#[test]
fn req_event_001_error_display_source_from() {
    use std::error::Error;
    let cases: Vec<EventError> = vec![
        EventError::UnknownObject(ObjectId::new(1)),
        EventError::UnknownPlayer(p(9)),
        EventError::NotOnBattlefield(ObjectId::new(2)),
        EventError::InvalidAmount(-3),
        EventError::Overflow,
        ZoneError::IdExhausted.into(),
        PermanentError::SelfAttach(ObjectId::new(3)).into(),
        MemoryError::UnknownObject(ObjectId::new(4)).into(),
        StateError::IdExhausted.into(),
    ];
    let mut texts = BTreeSet::new();
    for err in &cases {
        let text = err.to_string();
        assert!(!text.is_empty());
        texts.insert(text);
    }
    assert_eq!(texts.len(), cases.len());
    assert!(cases[0].source().is_none());
    for wrapped in &cases[5..] {
        assert!(wrapped.source().is_some());
    }
    assert_eq!(
        EventError::from(ZoneError::IdExhausted),
        EventError::Zone(ZoneError::IdExhausted)
    );
}

// ---------------------------------------------------------------------------------------------
// Zone changes (CR 400.7, CR 110.1, CR 122.2, CR 111.8)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: CR 400.7, CR 110.1, CR 603.10a: a permanent destroyed becomes a new object in
// the graveyard, leaves the permanent container, LKI captured with its permanent state.
#[test]
fn req_event_001_cr_400_7_zone_change_from_battlefield() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    w.permanents.set_tapped(a, true).unwrap();
    w.exec(GameEvent::AddCounters {
        target: CounterTarget::Object(a),
        kind: "+1/+1".into(),
        amount: 2,
    })
    .unwrap();
    w.exec(GameEvent::Damage {
        source: None,
        source_controller: None,
        target: DamageTarget::Object(a),
        amount: 1,
        combat: false,
    })
    .unwrap();

    let ex = w.exec(destroy(a)).unwrap().unwrap();
    let m = moved_of(&ex);
    assert_eq!(m.old, a);
    assert_ne!(m.new, a);
    assert_eq!(m.from, ZoneId::Battlefield);
    assert_eq!(m.to, ZoneId::Graveyard(p(1)));
    assert!(ex.lki_captured);
    assert_eq!(ex.instance.event(), &destroy(a));
    // CR 110.1
    assert!(w.permanents.get(a).is_none());
    assert!(w.permanents.get(m.new).is_none());
    // CR 122.2: counters and damage cease to exist.
    assert!(w.events.markers().counters(a).is_empty());
    assert_eq!(w.events.markers().damage(a), 0);
    // LKI as it last existed, CR 603.10a, CR 608.2h.
    let rec = w.lki.get(a).unwrap();
    assert_eq!(rec.zone(), ZoneId::Battlefield);
    assert!(rec.permanent().unwrap().status().tapped);
    assert_eq!(rec.counters().get("+1/+1"), Some(&2));
    // Logged.
    let last = w.events.log().batches().last().unwrap();
    assert_eq!(last.events, vec![ex]);
    assert_eq!(last.turn, 1);
}

// REQ-EVENT-001: CR 110.1, CR 400.7: entering the battlefield makes a new permanent under the
// requested controller; the hand departure captures LKI.
#[test]
fn req_event_001_cr_110_1_zone_change_to_battlefield() {
    let mut w = World::new();
    w.game.set_turn_number(4);
    let h = w.create(1, 1, ZoneKind::Hand);
    let mut req = MoveRequest::new(ZoneKind::Battlefield);
    req.controller = Some(p(2));
    let ex = w
        .exec(GameEvent::ZoneChange {
            object: h,
            request: req,
            cause: ZoneChangeCause::Other,
        })
        .unwrap()
        .unwrap();
    let m = moved_of(&ex);
    assert!(ex.lki_captured);
    let perm = w.permanents.get(m.new).unwrap();
    assert_eq!(perm.controller(), p(2));
    assert_eq!(perm.controlled_since_turn(), 4);
    assert!(!perm.status().tapped);
    assert_eq!(w.lki.get(h).unwrap().zone(), ZoneId::Hand(p(1)));
}

// REQ-EVENT-001: CR 400.7: a same-zone reposition is an executed event but not a zone change:
// no LKI, same id, markers kept.
#[test]
fn req_event_001_cr_400_7_reposition_keeps_identity() {
    let mut w = World::new();
    let top = w.create(1, 1, ZoneKind::Library);
    let second = w.create(2, 1, ZoneKind::Library);
    w.exec(GameEvent::AddCounters {
        target: CounterTarget::Object(top),
        kind: "study".into(),
        amount: 1,
    })
    .unwrap();
    let mut req = MoveRequest::new(ZoneKind::Library);
    req.position = Position::Bottom;
    let ex = w
        .exec(GameEvent::ZoneChange {
            object: top,
            request: req,
            cause: ZoneChangeCause::Other,
        })
        .unwrap()
        .unwrap();
    let m = moved_of(&ex);
    assert_eq!(m.old, m.new);
    assert!(!ex.lki_captured);
    assert!(w.lki.is_empty());
    assert_eq!(w.zones.contents(ZoneId::Library(p(1))), vec![second, top]);
    assert_eq!(w.events.markers().counter(top, "study"), 1);
    assert_eq!(w.events.log().len(), 2);
}

// REQ-EVENT-001: battlefield reposition does not touch the permanent.
#[test]
fn req_event_001_cr_400_7_battlefield_reposition() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    w.permanents.set_tapped(a, true).unwrap();
    let ex = w.exec(zc(a, ZoneKind::Battlefield)).unwrap().unwrap();
    assert_eq!(moved_of(&ex).new, a);
    assert!(w.permanents.get(a).unwrap().status().tapped);
}

// REQ-EVENT-001: zone-change errors: unknown object; a zone error rolls back.
#[test]
fn req_event_001_zone_change_errors() {
    let mut w = World::new();
    let g = w.create(1, 1, ZoneKind::Graveyard);
    assert_fails(
        &mut w,
        zc(ObjectId::new(99), ZoneKind::Exile),
        EventError::UnknownObject(ObjectId::new(99)),
    );
    let mut req = MoveRequest::new(ZoneKind::Hand);
    req.controller = Some(p(1));
    assert_fails(
        &mut w,
        GameEvent::ZoneChange {
            object: g,
            request: req,
            cause: ZoneChangeCause::Other,
        },
        EventError::Zone(ZoneError::ControllerNotApplicable),
    );
}

// REQ-EVENT-001: CR 111.8: a token that left the battlefield can't move again (no-op); CR
// 111.6/400.7: the token mark is per card and survives the move.
#[test]
fn req_event_001_cr_111_8_token_off_battlefield_does_not_move() {
    let mut w = World::new();
    let ex = w
        .exec(GameEvent::CreateToken {
            owner: p(1),
            card: c(50),
            controller: None,
        })
        .unwrap()
        .unwrap();
    let Outcome::TokenCreated { object } = ex.outcome else {
        panic!("expected a token");
    };
    let gone = moved_of(&w.exec(destroy(object)).unwrap().unwrap()).new;
    assert!(w.events.markers().is_token(c(50)));
    assert_noop(&mut w, zc(gone, ZoneKind::Battlefield));
    assert_noop(&mut w, zc(gone, ZoneKind::Hand));
}

// REQ-EVENT-001: CR 704.8, CR 603.10a: two creatures destroyed simultaneously; both LKI
// records reflect the state before either moved (the aura-like attachment still attached).
#[test]
fn req_event_001_cr_704_8_simultaneous_destroy_lki() {
    let mut w = World::new();
    let host = w.create(1, 1, ZoneKind::Battlefield);
    let att = w.create(2, 1, ZoneKind::Battlefield);
    w.exec(GameEvent::Attach {
        attachment: att,
        target: AttachTarget::Permanent(host),
    })
    .unwrap()
    .unwrap();
    let out = w.batch(vec![destroy(host), destroy(att)]).unwrap();
    assert_eq!(out.events.len(), 2);
    assert!(out.events.iter().all(|x| x.lki_captured));
    let host_rec = w.lki.get(host).unwrap();
    let att_rec = w.lki.get(att).unwrap();
    // Before any move: the attachment was still on the host.
    assert_eq!(host_rec.attached_here(), &[att][..]);
    assert_eq!(
        att_rec.permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(host))
    );
    assert!(w.permanents.is_empty());

    // Contrast: two separate batches see the attachment gone for the second.
    let mut v = World::new();
    let host = v.create(1, 1, ZoneKind::Battlefield);
    let att = v.create(2, 1, ZoneKind::Battlefield);
    v.exec(GameEvent::Attach {
        attachment: att,
        target: AttachTarget::Permanent(host),
    })
    .unwrap();
    v.exec(destroy(host)).unwrap();
    v.exec(destroy(att)).unwrap();
    assert_eq!(
        v.lki.get(att).unwrap().permanent().unwrap().attached_to(),
        None
    );
}

// REQ-EVENT-001: CR 704.8: LKI is captured before any event of the batch; a later event of the
// same batch naming the replaced id fails with UnknownObject and rolls back.
#[test]
fn req_event_001_batch_replaced_id_fails() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let before = w.parts();
    assert_eq!(
        w.batch(vec![destroy(a), GameEvent::Tap { object: a }]),
        Err(EventError::UnknownObject(a))
    );
    assert!(w.parts() == before);
    assert_eq!(
        w.batch(vec![destroy(a), destroy(a)]),
        Err(EventError::UnknownObject(a))
    );
    assert!(w.parts() == before);
}

// REQ-EVENT-001: an object created earlier in the same batch is captured just before its own
// move (CR 603.10).
#[test]
fn req_event_001_batch_created_then_moved_captures_lki() {
    let mut w = World::new();
    let predicted = ObjectId::new(w.ids.last_issued() + 1);
    let out = w
        .batch(vec![
            GameEvent::CreateToken {
                owner: p(2),
                card: c(70),
                controller: None,
            },
            GameEvent::ZoneChange {
                object: predicted,
                request: MoveRequest::new(ZoneKind::Exile),
                cause: ZoneChangeCause::Exile,
            },
        ])
        .unwrap();
    assert_eq!(out.events.len(), 2);
    assert_eq!(
        out.events[0].outcome,
        Outcome::TokenCreated { object: predicted }
    );
    assert!(!out.events[0].lki_captured);
    assert!(out.events[1].lki_captured);
    assert_eq!(w.lki.get(predicted).unwrap().zone(), ZoneId::Battlefield);
}

// REQ-EVENT-001: CR 400.7, CR 603.10: LKI capture failure (no characteristics) is an error
// that rolls back, and a retry with characteristics succeeds.
#[test]
fn req_event_001_lki_failure_rolls_back() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    w.provider.missing.insert(a);
    assert_fails(
        &mut w,
        destroy(a),
        EventError::Memory(MemoryError::NoCharacteristics(a)),
    );
    w.provider.missing.clear();
    assert!(w.exec(destroy(a)).unwrap().unwrap().lki_captured);
}

// REQ-EVENT-001: an already-captured LKI record is an error (records are never overwritten).
#[test]
fn req_event_001_lki_already_captured_error() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    capture_lki_on_move(
        &w.zones,
        &w.permanents,
        &w.memories,
        &w.provider,
        &mut w.lki,
        a,
        &MoveRequest::new(ZoneKind::Graveyard),
    )
    .unwrap();
    assert_fails(
        &mut w,
        destroy(a),
        EventError::Memory(MemoryError::AlreadyCaptured(a)),
    );
}

// REQ-EVENT-001: CR 122.1, CR 122.2: ObjectMarkers is the single source of counters on
// objects (DECISIONS "LKI sources"): AddCounters, RemoveCounters and the LKI record all read
// the markers; the provider's counters are not consulted.
#[test]
fn req_event_001_lki_counters_single_source() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    w.provider.counters.insert(
        a,
        BTreeMap::from([("charge".to_owned(), 4), ("x".to_owned(), 1)]),
    );
    w.exec(GameEvent::AddCounters {
        target: CounterTarget::Object(a),
        kind: "charge".into(),
        amount: 2,
    })
    .unwrap()
    .unwrap();
    assert_eq!(w.events.markers().counter(a, "charge"), 2);
    // A kind only the provider reports is not held: removing it is no event.
    assert_noop(
        &mut w,
        GameEvent::RemoveCounters {
            target: CounterTarget::Object(a),
            kind: "x".into(),
            amount: 1,
        },
    );
    let ex = w
        .exec(GameEvent::RemoveCounters {
            target: CounterTarget::Object(a),
            kind: "charge".into(),
            amount: 1,
        })
        .unwrap()
        .unwrap();
    assert_eq!(ex.outcome, Outcome::CountersRemoved { removed: 1 });
    w.exec(destroy(a)).unwrap();
    let rec = w.lki.get(a).unwrap();
    assert_eq!(rec.counters(), &BTreeMap::from([("charge".to_owned(), 1)]));

    // Without the removal the record shows exactly the 2 the events put (not 6, no `x`).
    let mut v = World::new();
    let b = v.create(1, 1, ZoneKind::Battlefield);
    v.provider.counters.insert(
        b,
        BTreeMap::from([("charge".to_owned(), 4), ("x".to_owned(), 1)]),
    );
    v.exec(GameEvent::AddCounters {
        target: CounterTarget::Object(b),
        kind: "charge".into(),
        amount: 2,
    })
    .unwrap();
    v.exec(destroy(b)).unwrap();
    assert_eq!(
        v.lki.get(b).unwrap().counters(),
        &BTreeMap::from([("charge".to_owned(), 2)])
    );
}

// REQ-EVENT-001: CR 701.3a, CR 701.3d, CR 603.10a: the permanent container is the only source
// of the objects attached here; a stale provider attachment after Unattach is not in the LKI.
#[test]
fn req_event_001_lki_attachments_follow_permanents() {
    let mut w = World::new();
    let host = w.create(1, 1, ZoneKind::Battlefield);
    let att = w.create(2, 1, ZoneKind::Battlefield);
    w.exec(GameEvent::Attach {
        attachment: att,
        target: AttachTarget::Permanent(host),
    })
    .unwrap()
    .unwrap();
    w.exec(GameEvent::Unattach { attachment: att })
        .unwrap()
        .unwrap();
    // The provider still (stale) reports the attachment.
    w.provider.attached.insert(host, vec![att]);
    w.exec(destroy(host)).unwrap().unwrap();
    assert!(w.lki.get(host).unwrap().attached_here().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Draw (CR 121)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, REQ-DSL-003: CR 121.1: a draw moves the top card into the hand, captures LKI
// and counts one card drawn.
#[test]
fn req_event_001_cr_121_1_draw_top_card() {
    let mut w = World::new();
    let top = w.create(1, 1, ZoneKind::Library);
    let second = w.create(2, 1, ZoneKind::Library);
    let ex = w.exec(GameEvent::Draw { player: p(1) }).unwrap().unwrap();
    let m = moved_of(&ex);
    assert_eq!(m.old, top);
    assert_eq!(m.from, ZoneId::Library(p(1)));
    assert_eq!(m.to, ZoneId::Hand(p(1)));
    assert!(ex.lki_captured);
    assert_eq!(w.zones.contents(ZoneId::Hand(p(1))), vec![m.new]);
    assert_eq!(w.zones.contents(ZoneId::Library(p(1))), vec![second]);
    assert_eq!(
        w.hist(HistoryStat::CardsDrawn, HistoryScope::Turn(1), Some(1)),
        1
    );
}

// REQ-EVENT-001: CR 121.2: two draws in one batch draw the top two cards, both LKI captured
// before either move; a reposition earlier in the batch changes which card is drawn.
#[test]
fn req_event_001_cr_121_2_draws_in_batch() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Library);
    let b = w.create(2, 1, ZoneKind::Library);
    let d = w.create(3, 1, ZoneKind::Library);
    let mut bottom = MoveRequest::new(ZoneKind::Library);
    bottom.position = Position::Bottom;
    let out = w
        .batch(vec![
            GameEvent::ZoneChange {
                object: a,
                request: bottom,
                cause: ZoneChangeCause::Other,
            },
            GameEvent::Draw { player: p(1) },
            GameEvent::Draw { player: p(1) },
        ])
        .unwrap();
    assert_eq!(out.events.len(), 3);
    assert!(!out.events[0].lki_captured);
    assert_eq!(moved_of(&out.events[1]).old, b);
    assert_eq!(moved_of(&out.events[2]).old, d);
    assert!(out.events[1].lki_captured && out.events[2].lki_captured);
    assert_eq!(w.lki.ids(), vec![b, d]);
    assert_eq!(w.zones.contents(ZoneId::Library(p(1))), vec![a]);
}

// REQ-EVENT-001: CR 121.4, CR 704.5b: drawing from an empty library is no event and flags the
// player; take returns and clears.
#[test]
fn req_event_001_cr_121_4_empty_library_flag() {
    let mut w = World::new();
    assert_eq!(w.exec(GameEvent::Draw { player: p(2) }), Ok(None));
    assert_eq!(w.exec(GameEvent::Draw { player: p(2) }), Ok(None));
    assert_eq!(w.exec(GameEvent::Draw { player: p(1) }), Ok(None));
    assert!(w.events.log().is_empty());
    assert!(w.events.history().is_empty());
    assert_eq!(w.events.empty_library_draws(), vec![p(1), p(2)]);
    assert_eq!(w.events.take_empty_library_draws(), vec![p(1), p(2)]);
    assert!(w.events.empty_library_draws().is_empty());
    assert!(w.events.take_empty_library_draws().is_empty());
}

// REQ-EVENT-001: draw errors.
#[test]
fn req_event_001_draw_unknown_player() {
    let mut w = World::new();
    assert_fails(
        &mut w,
        GameEvent::Draw { player: p(9) },
        EventError::UnknownPlayer(p(9)),
    );
}

// ---------------------------------------------------------------------------------------------
// Damage and life (CR 119, CR 120)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, REQ-DSL-003: CR 119.2, CR 120.3a: damage to a player is life loss, recorded as
// life lost, damage to the player and damage dealt by the source's controller.
#[test]
fn req_event_001_cr_120_3a_damage_to_player() {
    let mut w = World::new();
    let ex = w
        .exec(GameEvent::Damage {
            source: Some(ObjectId::new(77)),
            source_controller: Some(p(2)),
            target: DamageTarget::Player(p(1)),
            amount: 4,
            combat: true,
        })
        .unwrap()
        .unwrap();
    assert_eq!(ex.outcome, Outcome::Applied);
    assert!(!ex.lki_captured);
    assert_eq!(w.life(1), 16);
    let t = HistoryScope::Turn(1);
    assert_eq!(w.hist(HistoryStat::LifeLost, t, Some(1)), 4);
    assert_eq!(w.hist(HistoryStat::DamageDealtToPlayer, t, Some(1)), 4);
    assert_eq!(w.hist(HistoryStat::DamageDealtBy, t, Some(2)), 4);
    assert_eq!(w.hist(HistoryStat::DamageDealtBy, t, Some(1)), 0);
}

// REQ-EVENT-001: CR 120.3e: damage to a permanent is marked on it; CR 514.2 clears it.
#[test]
fn req_event_001_cr_120_3e_damage_marked() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    for _ in 0..2 {
        w.exec(GameEvent::Damage {
            source: None,
            source_controller: Some(p(1)),
            target: DamageTarget::Object(a),
            amount: 3,
            combat: false,
        })
        .unwrap()
        .unwrap();
    }
    assert_eq!(w.events.markers().damage(a), 6);
    assert_eq!(
        w.hist(HistoryStat::DamageDealtBy, HistoryScope::Game, Some(1)),
        6
    );
    assert_eq!(w.hist(HistoryStat::LifeLost, HistoryScope::Game, None), 0);
    // CR 514.2
    w.events.markers_mut().clear_all_damage();
    assert_eq!(w.events.markers().damage(a), 0);
    w.events.markers_mut().clear_all_damage();
}

// REQ-EVENT-001: CR 120.8: 0 damage is no event; negative is an error; target errors;
// overflow.
#[test]
fn req_event_001_cr_120_8_damage_noop_and_errors() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let g = w.create(2, 1, ZoneKind::Graveyard);
    let dmg = |target, amount| GameEvent::Damage {
        source: None,
        source_controller: Some(p(1)),
        target,
        amount,
        combat: false,
    };
    assert_noop(&mut w, dmg(DamageTarget::Player(p(1)), 0));
    assert_noop(&mut w, dmg(DamageTarget::Object(a), 0));
    assert_fails(
        &mut w,
        dmg(DamageTarget::Player(p(1)), -1),
        EventError::InvalidAmount(-1),
    );
    assert_fails(
        &mut w,
        dmg(DamageTarget::Player(p(5)), 1),
        EventError::UnknownPlayer(p(5)),
    );
    assert_fails(
        &mut w,
        dmg(DamageTarget::Object(g), 1),
        EventError::NotOnBattlefield(g),
    );
    assert_fails(
        &mut w,
        dmg(DamageTarget::Object(ObjectId::new(99)), 1),
        EventError::UnknownObject(ObjectId::new(99)),
    );
    w.exec(dmg(DamageTarget::Object(a), i64::MAX)).unwrap();
    assert_fails(
        &mut w,
        dmg(DamageTarget::Object(a), 1),
        EventError::Overflow,
    );
    w.game.player_mut(p(2)).unwrap().set_life(i64::MIN + 1);
    assert_fails(
        &mut w,
        dmg(DamageTarget::Player(p(2)), 2),
        EventError::Overflow,
    );
}

// REQ-EVENT-001, REQ-DSL-003: CR 119.3: life gain and loss; CR 119.6: below 0 is allowed.
#[test]
fn req_event_001_cr_119_3_life_change() {
    let mut w = World::new();
    w.exec(GameEvent::LifeChange {
        player: p(1),
        delta: 3,
    })
    .unwrap()
    .unwrap();
    assert_eq!(w.life(1), 23);
    w.exec(GameEvent::LifeChange {
        player: p(1),
        delta: -30,
    })
    .unwrap()
    .unwrap();
    assert_eq!(w.life(1), -7);
    let t = HistoryScope::Turn(1);
    assert_eq!(w.hist(HistoryStat::LifeGained, t, Some(1)), 3);
    assert_eq!(w.hist(HistoryStat::LifeLost, t, Some(1)), 30);
}

// REQ-EVENT-001: CR 119.9, CR 119.10: 0 life is no event; unknown player; overflow.
#[test]
fn req_event_001_cr_119_9_life_noop_and_errors() {
    let mut w = World::new();
    assert_noop(
        &mut w,
        GameEvent::LifeChange {
            player: p(1),
            delta: 0,
        },
    );
    assert_fails(
        &mut w,
        GameEvent::LifeChange {
            player: p(3),
            delta: 1,
        },
        EventError::UnknownPlayer(p(3)),
    );
    w.game.player_mut(p(1)).unwrap().set_life(i64::MAX);
    assert_fails(
        &mut w,
        GameEvent::LifeChange {
            player: p(1),
            delta: 1,
        },
        EventError::Overflow,
    );
    w.game.player_mut(p(1)).unwrap().set_life(0);
    // `i64::MIN` life loss: life fits, the ledger saturates.
    w.exec(GameEvent::LifeChange {
        player: p(1),
        delta: i64::MIN,
    })
    .unwrap();
    assert_eq!(w.life(1), i64::MIN);
    assert_eq!(
        w.hist(HistoryStat::LifeLost, HistoryScope::Game, Some(1)),
        i64::MAX
    );
}

// ---------------------------------------------------------------------------------------------
// Counters (CR 122)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: CR 122.1, CR 122.3: counters on objects and players; +1/+1 and -1/-1 are not
// annihilated by the event.
#[test]
fn req_event_001_cr_122_1_add_counters() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let add = |target, kind: &str, amount| GameEvent::AddCounters {
        target,
        kind: kind.to_owned(),
        amount,
    };
    w.exec(add(CounterTarget::Object(a), "+1/+1", 2))
        .unwrap()
        .unwrap();
    w.exec(add(CounterTarget::Object(a), "-1/-1", 1))
        .unwrap()
        .unwrap();
    w.exec(add(CounterTarget::Object(a), "+1/+1", 1))
        .unwrap()
        .unwrap();
    assert_eq!(
        w.events.markers().counters(a),
        BTreeMap::from([("+1/+1".to_owned(), 3), ("-1/-1".to_owned(), 1)])
    );
    w.exec(add(CounterTarget::Player(p(2)), "poison", 3))
        .unwrap()
        .unwrap();
    assert_eq!(w.game.player(p(2)).unwrap().counter("poison"), 3);
    // A card in a non-battlefield zone can hold counters.
    let x = w.create(2, 1, ZoneKind::Exile);
    w.exec(add(CounterTarget::Object(x), "time", 4))
        .unwrap()
        .unwrap();
    assert_eq!(w.events.markers().counter(x, "time"), 4);
}

// REQ-EVENT-001: add-counter no-op and errors.
#[test]
fn req_event_001_add_counters_noop_and_errors() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let add = |target, amount| GameEvent::AddCounters {
        target,
        kind: "charge".to_owned(),
        amount,
    };
    assert_noop(&mut w, add(CounterTarget::Object(a), 0));
    assert_fails(
        &mut w,
        add(CounterTarget::Object(a), -2),
        EventError::InvalidAmount(-2),
    );
    assert_fails(
        &mut w,
        add(CounterTarget::Object(ObjectId::new(50)), 1),
        EventError::UnknownObject(ObjectId::new(50)),
    );
    assert_fails(
        &mut w,
        add(CounterTarget::Player(p(4)), 1),
        EventError::UnknownPlayer(p(4)),
    );
    w.exec(add(CounterTarget::Object(a), i64::MAX)).unwrap();
    assert_fails(
        &mut w,
        add(CounterTarget::Object(a), 1),
        EventError::Overflow,
    );
}

// REQ-EVENT-001: CR 122.1: removing removes up to the amount held and reports the number.
#[test]
fn req_event_001_cr_122_1_remove_counters() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let add = |target, amount| GameEvent::AddCounters {
        target,
        kind: "charge".to_owned(),
        amount,
    };
    let rem = |target, amount| GameEvent::RemoveCounters {
        target,
        kind: "charge".to_owned(),
        amount,
    };
    w.exec(add(CounterTarget::Object(a), 3)).unwrap();
    let ex = w.exec(rem(CounterTarget::Object(a), 2)).unwrap().unwrap();
    assert_eq!(ex.outcome, Outcome::CountersRemoved { removed: 2 });
    let ex = w.exec(rem(CounterTarget::Object(a), 5)).unwrap().unwrap();
    assert_eq!(ex.outcome, Outcome::CountersRemoved { removed: 1 });
    assert!(w.events.markers().counters(a).is_empty());
    assert_noop(&mut w, rem(CounterTarget::Object(a), 1));
    assert_noop(&mut w, rem(CounterTarget::Object(a), 0));
    assert_fails(
        &mut w,
        rem(CounterTarget::Object(a), -1),
        EventError::InvalidAmount(-1),
    );
    assert_fails(
        &mut w,
        rem(CounterTarget::Object(ObjectId::new(42)), 1),
        EventError::UnknownObject(ObjectId::new(42)),
    );
    assert_fails(
        &mut w,
        rem(CounterTarget::Player(p(8)), 1),
        EventError::UnknownPlayer(p(8)),
    );
    w.exec(add(CounterTarget::Player(p(1)), 2)).unwrap();
    let ex = w
        .exec(rem(CounterTarget::Player(p(1)), 2))
        .unwrap()
        .unwrap();
    assert_eq!(ex.outcome, Outcome::CountersRemoved { removed: 2 });
    assert_eq!(w.game.player(p(1)).unwrap().counter("charge"), 0);
}

// ---------------------------------------------------------------------------------------------
// Tap / untap (CR 701.26)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: CR 701.26a, CR 701.26b: tap and untap; repeating is no event; errors.
#[test]
fn req_event_001_cr_701_26_tap_untap() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let h = w.create(2, 1, ZoneKind::Hand);
    assert_noop(&mut w, GameEvent::Untap { object: a });
    let ex = w.exec(GameEvent::Tap { object: a }).unwrap().unwrap();
    assert_eq!(ex.outcome, Outcome::Applied);
    assert!(w.permanents.get(a).unwrap().status().tapped);
    assert_noop(&mut w, GameEvent::Tap { object: a });
    w.exec(GameEvent::Untap { object: a }).unwrap().unwrap();
    assert!(!w.permanents.get(a).unwrap().status().tapped);
    assert_fails(
        &mut w,
        GameEvent::Tap { object: h },
        EventError::NotOnBattlefield(h),
    );
    assert_fails(
        &mut w,
        GameEvent::Untap {
            object: ObjectId::new(66),
        },
        EventError::UnknownObject(ObjectId::new(66)),
    );
}

// REQ-EVENT-001: CR 502.3: a simultaneous untap is one batch and one log entry; untapped
// permanents are no event.
#[test]
fn req_event_001_untap_batch_one_entry() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let b = w.create(2, 1, ZoneKind::Battlefield);
    let d = w.create(3, 1, ZoneKind::Battlefield);
    w.permanents.set_tapped(a, true).unwrap();
    w.permanents.set_tapped(d, true).unwrap();
    let out = w
        .batch(vec![
            GameEvent::Untap { object: a },
            GameEvent::Untap { object: b },
            GameEvent::Untap { object: d },
        ])
        .unwrap();
    assert_eq!(out.events.len(), 2);
    assert_eq!(out.index, 0);
    assert_eq!(w.events.log().len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Tokens (CR 111, CR 701.7)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, REQ-DSL-003: CR 111.2, CR 701.7a: token creation.
#[test]
fn req_event_001_cr_111_2_create_token() {
    let mut w = World::new();
    w.game.set_turn_number(3);
    let ex = w
        .exec(GameEvent::CreateToken {
            owner: p(1),
            card: c(90),
            controller: Some(p(2)),
        })
        .unwrap()
        .unwrap();
    let Outcome::TokenCreated { object } = ex.outcome else {
        panic!("expected a token");
    };
    assert!(!ex.lki_captured);
    let rec = w.zones.object(object).unwrap();
    assert_eq!(rec.zone, ZoneId::Battlefield);
    assert_eq!(rec.owner, p(1));
    let perm = w.permanents.get(object).unwrap();
    assert_eq!(perm.controller(), p(2));
    assert_eq!(perm.controlled_since_turn(), 3);
    assert!(w.events.markers().is_token(c(90)));
    assert!(!w.events.markers().is_token(c(91)));
    assert!(w.lki.is_empty());
    assert_eq!(
        w.hist(HistoryStat::TokensCreated, HistoryScope::Turn(3), Some(1)),
        1
    );
    assert_eq!(
        w.hist(HistoryStat::TokensCreated, HistoryScope::Turn(3), Some(2)),
        0
    );
}

// REQ-EVENT-001: token-creation errors.
#[test]
fn req_event_001_create_token_errors() {
    let mut w = World::new();
    w.create(5, 1, ZoneKind::Hand);
    let tok = |owner, card, controller: Option<u64>| GameEvent::CreateToken {
        owner: p(owner),
        card: c(card),
        controller: controller.map(p),
    };
    assert_fails(&mut w, tok(9, 1, None), EventError::UnknownPlayer(p(9)));
    assert_fails(&mut w, tok(1, 1, Some(9)), EventError::UnknownPlayer(p(9)));
    assert_fails(
        &mut w,
        tok(1, 5, None),
        EventError::Zone(ZoneError::CardAlreadyPresent(c(5))),
    );
}

// ---------------------------------------------------------------------------------------------
// Turn structure (CR 500)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: CR 500.1, CR 500.6, CR 603.2b: turn, phase and step beginnings.
#[test]
fn req_event_001_cr_500_6_turn_phase_step() {
    let mut w = World::new();
    w.exec(GameEvent::StepBegins { step: Step::Upkeep })
        .unwrap()
        .unwrap();
    assert_eq!(w.events.phase(), Some(Phase::Beginning));
    assert_eq!(w.events.step(), Some(Step::Upkeep));
    let ex = w
        .exec(GameEvent::TurnBegins { player: p(2) })
        .unwrap()
        .unwrap();
    assert_eq!(ex.outcome, Outcome::TurnBegan { turn: 2 });
    assert_eq!(w.game.turn().turn_number(), 2);
    assert_eq!(w.game.turn().active_player(), Some(p(2)));
    assert_eq!(w.events.phase(), None);
    assert_eq!(w.events.step(), None);
    assert_eq!(w.events.log().batches().last().unwrap().turn, 2);
    w.exec(GameEvent::PhaseBegins {
        phase: Phase::PrecombatMain,
    })
    .unwrap()
    .unwrap();
    assert_eq!(w.events.phase(), Some(Phase::PrecombatMain));
    assert_eq!(w.events.step(), None);
    w.exec(GameEvent::StepBegins {
        step: Step::CombatDamage,
    })
    .unwrap();
    assert_eq!(w.events.phase(), Some(Phase::Combat));
    w.exec(GameEvent::PhaseBegins {
        phase: Phase::Ending,
    })
    .unwrap();
    assert_eq!(w.events.step(), None);
}

// REQ-EVENT-001: turn-begin errors.
#[test]
fn req_event_001_turn_begins_errors() {
    let mut w = World::new();
    assert_fails(
        &mut w,
        GameEvent::TurnBegins { player: p(3) },
        EventError::UnknownPlayer(p(3)),
    );
    w.game.set_turn_number(u64::MAX);
    assert_fails(
        &mut w,
        GameEvent::TurnBegins { player: p(1) },
        EventError::Overflow,
    );
}

// ---------------------------------------------------------------------------------------------
// Attach (CR 701.3)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: CR 701.3a, CR 701.3b, CR 701.3c, CR 701.3d.
#[test]
fn req_event_001_cr_701_3_attach_unattach() {
    let mut w = World::new();
    let host = w.create(1, 1, ZoneKind::Battlefield);
    let other = w.create(2, 1, ZoneKind::Battlefield);
    let att = w.create(3, 1, ZoneKind::Battlefield);
    let attach = |target| GameEvent::Attach {
        attachment: att,
        target,
    };
    let ex = w
        .exec(attach(AttachTarget::Permanent(host)))
        .unwrap()
        .unwrap();
    let Outcome::Attached { timestamp: t1 } = ex.outcome else {
        panic!("expected attached");
    };
    assert_eq!(
        w.permanents.attached_to(att),
        Some(AttachTarget::Permanent(host))
    );
    // CR 701.3b
    assert_noop(&mut w, attach(AttachTarget::Permanent(host)));
    // CR 701.3c: a new timestamp per real attach.
    let Outcome::Attached { timestamp: t2 } = w
        .exec(attach(AttachTarget::Permanent(other)))
        .unwrap()
        .unwrap()
        .outcome
    else {
        panic!("expected attached");
    };
    assert!(t2 > t1);
    let Outcome::Attached { timestamp: t3 } = w
        .exec(attach(AttachTarget::Player(p(2))))
        .unwrap()
        .unwrap()
        .outcome
    else {
        panic!("expected attached");
    };
    assert!(t3 > t2);
    // CR 701.3d
    let ex = w
        .exec(GameEvent::Unattach { attachment: att })
        .unwrap()
        .unwrap();
    assert_eq!(ex.outcome, Outcome::Applied);
    assert_eq!(w.permanents.attached_to(att), None);
    assert_noop(&mut w, GameEvent::Unattach { attachment: att });
    let h = w.create(4, 1, ZoneKind::Hand);
    assert_noop(&mut w, GameEvent::Unattach { attachment: h });
    assert_fails(
        &mut w,
        GameEvent::Unattach {
            attachment: ObjectId::new(77),
        },
        EventError::UnknownObject(ObjectId::new(77)),
    );
}

// REQ-EVENT-001: attach errors: unknown/off-battlefield objects, unknown player, cycle.
#[test]
fn req_event_001_attach_errors() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let b = w.create(2, 1, ZoneKind::Battlefield);
    let g = w.create(3, 1, ZoneKind::Graveyard);
    w.exec(GameEvent::Attach {
        attachment: a,
        target: AttachTarget::Permanent(b),
    })
    .unwrap();
    let attach = |attachment, target| GameEvent::Attach { attachment, target };
    assert_fails(
        &mut w,
        attach(b, AttachTarget::Permanent(a)),
        EventError::Permanent(PermanentError::AttachmentCycle {
            object: b,
            target: a,
        }),
    );
    assert_fails(
        &mut w,
        attach(b, AttachTarget::Permanent(b)),
        EventError::Permanent(PermanentError::SelfAttach(b)),
    );
    assert_fails(
        &mut w,
        attach(g, AttachTarget::Permanent(a)),
        EventError::NotOnBattlefield(g),
    );
    assert_fails(
        &mut w,
        attach(a, AttachTarget::Permanent(g)),
        EventError::NotOnBattlefield(g),
    );
    assert_fails(
        &mut w,
        attach(ObjectId::new(80), AttachTarget::Player(p(1))),
        EventError::UnknownObject(ObjectId::new(80)),
    );
    assert_fails(
        &mut w,
        attach(a, AttachTarget::Player(p(7))),
        EventError::UnknownPlayer(p(7)),
    );
}

// ---------------------------------------------------------------------------------------------
// Copy and face (CR 707, CR 701.27b, CR 708.8)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: CR 707.2, CR 707.4: becoming a copy records a link, no zone change.
#[test]
fn req_event_001_cr_707_4_become_copy() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let b = w.create(2, 2, ZoneKind::Battlefield);
    let g = w.create(3, 1, ZoneKind::Graveyard);
    let copy = |object, copied| GameEvent::BecomeCopy { object, copied };
    let ex = w.exec(copy(a, b)).unwrap().unwrap();
    assert_eq!(ex.outcome, Outcome::Applied);
    assert_eq!(w.events.markers().copy_of(a), Some(b));
    assert!(w.permanents.get(a).is_some());
    assert_noop(&mut w, copy(a, b));
    // A copy of an object in another zone.
    w.exec(copy(a, g)).unwrap().unwrap();
    assert_eq!(w.events.markers().copy_of(a), Some(g));
    assert_fails(&mut w, copy(g, a), EventError::NotOnBattlefield(g));
    assert_fails(
        &mut w,
        copy(a, ObjectId::new(90)),
        EventError::UnknownObject(ObjectId::new(90)),
    );
    // The link ceases with a zone change (CR 400.7).
    w.exec(destroy(a)).unwrap();
    assert_eq!(w.events.markers().copy_of(a), None);
}

// REQ-EVENT-001: CR 701.27b, CR 708.8: turning face up and down.
#[test]
fn req_event_001_cr_701_27b_face_up_down() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let h = w.create(2, 1, ZoneKind::Hand);
    assert_noop(&mut w, GameEvent::TurnFaceUp { object: a });
    w.exec(GameEvent::TurnFaceDown { object: a })
        .unwrap()
        .unwrap();
    assert!(w.permanents.get(a).unwrap().status().face_down);
    assert_noop(&mut w, GameEvent::TurnFaceDown { object: a });
    let ex = w
        .exec(GameEvent::TurnFaceUp { object: a })
        .unwrap()
        .unwrap();
    assert_eq!(ex.outcome, Outcome::Applied);
    assert!(!w.permanents.get(a).unwrap().status().face_down);
    assert_fails(
        &mut w,
        GameEvent::TurnFaceDown { object: h },
        EventError::NotOnBattlefield(h),
    );
}

/// The face status of `id` in the zone record and in the permanent container.
fn face_states(w: &World, id: ObjectId) -> (bool, bool) {
    (
        w.zones.object(id).unwrap().face_down,
        w.permanents.get(id).unwrap().status().face_down,
    )
}

/// The battlefield view of `id` for `viewer` (REQ-CORE-004).
fn battlefield_view(w: &World, viewer: u64, id: ObjectId) -> ObjectView {
    Knowledge::new()
        .project(&w.zones, p(viewer))
        .unwrap()
        .battlefield
        .into_iter()
        .find(|v| v.id == id)
        .unwrap()
}

// REQ-EVENT-001: CR 708.8, CR 110.1: a permanent put onto the battlefield face down is face
// down in the zone record and in the permanent container; TurnFaceUp then occurs and clears
// both (CR 701.27b).
#[test]
fn req_event_001_cr_708_8_enter_face_down_synced() {
    let mut w = World::new();
    let h = w.create(1, 1, ZoneKind::Hand);
    let mut req = MoveRequest::new(ZoneKind::Battlefield);
    req.face_down = true;
    let ex = w
        .exec(GameEvent::ZoneChange {
            object: h,
            request: req,
            cause: ZoneChangeCause::Other,
        })
        .unwrap()
        .unwrap();
    let a = moved_of(&ex).new;
    assert_eq!(face_states(&w, a), (true, true));
    let up = w
        .exec(GameEvent::TurnFaceUp { object: a })
        .unwrap()
        .unwrap();
    assert_eq!(up.outcome, Outcome::Applied);
    assert_eq!(face_states(&w, a), (false, false));
}

// REQ-EVENT-001: CR 400.7, CR 708.8: a same-zone battlefield reposition that asks for face
// down sets the zone record and the permanent face down together (same object).
#[test]
fn req_event_001_cr_400_7_battlefield_reposition_face_down_synced() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let mut req = MoveRequest::new(ZoneKind::Battlefield);
    req.face_down = true;
    req.controller = Some(p(1));
    let ex = w
        .exec(GameEvent::ZoneChange {
            object: a,
            request: req,
            cause: ZoneChangeCause::Other,
        })
        .unwrap()
        .unwrap();
    assert_eq!(moved_of(&ex).new, a);
    assert_eq!(face_states(&w, a), (true, true));
}

// REQ-EVENT-001, REQ-CORE-004: CR 701.27b, CR 708.8, CR 400.7: TurnFaceDown turns the zone
// record face down too (same id, controller, owner and card), so another player's redacted
// view no longer identifies it; TurnFaceUp restores both.
#[test]
fn req_event_001_req_core_004_cr_701_27b_face_down_hides_identity() {
    let mut w = World::new();
    // Owned by P2, controlled by P1: the controller must survive the record update.
    let h = w.create(1, 2, ZoneKind::Hand);
    let mut req = MoveRequest::new(ZoneKind::Battlefield);
    req.controller = Some(p(1));
    let a = moved_of(
        &w.exec(GameEvent::ZoneChange {
            object: h,
            request: req,
            cause: ZoneChangeCause::Other,
        })
        .unwrap()
        .unwrap(),
    )
    .new;
    let other = w.create(2, 1, ZoneKind::Battlefield);
    let record_before = *w.zones.object(a).unwrap();
    let battlefield_before = w.zones.contents(ZoneId::Battlefield);
    let ids_before = w.ids.clone();
    let lki_before = w.lki.clone();
    assert_eq!(battlefield_view(&w, 2, a).card, Some(c(1)));

    let ex = w
        .exec(GameEvent::TurnFaceDown { object: a })
        .unwrap()
        .unwrap();
    assert_eq!(ex.outcome, Outcome::Applied);
    assert!(!ex.lki_captured);
    assert_eq!(face_states(&w, a), (true, true));
    assert_eq!(w.zones.is_public_info(a), Some(false));
    let record = *w.zones.object(a).unwrap();
    assert_eq!(record.id, record_before.id);
    assert_eq!(record.controller, p(1));
    assert_eq!(record.owner, p(2));
    assert_eq!(record.card, record_before.card);
    assert_eq!(record.zone, ZoneId::Battlefield);
    assert_eq!(w.zones.contents(ZoneId::Battlefield), battlefield_before);
    assert!(w.ids == ids_before);
    assert!(w.lki == lki_before);
    assert_eq!(w.permanents.get(a).unwrap().controller(), p(1));
    assert!(w.permanents.get(other).is_some());

    // The opponent (here also the owner) sees a face-down object it cannot identify.
    let theirs = battlefield_view(&w, 2, a);
    assert!(theirs.face_down);
    assert_eq!(theirs.card, None);
    // CR 708.5 (knowledge unit): the controller may look at it.
    let mine = battlefield_view(&w, 1, a);
    assert!(mine.face_down);
    assert_eq!(mine.card, Some(c(1)));

    let up = w
        .exec(GameEvent::TurnFaceUp { object: a })
        .unwrap()
        .unwrap();
    assert_eq!(up.outcome, Outcome::Applied);
    assert_eq!(face_states(&w, a), (false, false));
    assert_eq!(w.zones.is_public_info(a), Some(true));
    assert_eq!(w.zones.object(a).unwrap().controller, p(1));
    assert_eq!(battlefield_view(&w, 2, a).card, Some(c(1)));
}

// REQ-EVENT-001: CR 701.27b, CR 603.10a, CR 400.7: a permanent turned face down and then
// destroyed has a face-down LKI record and a face-down permanent snapshot.
#[test]
fn req_event_001_cr_701_27b_face_down_then_departure_lki() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    w.exec(GameEvent::TurnFaceDown { object: a })
        .unwrap()
        .unwrap();
    assert!(w.exec(destroy(a)).unwrap().unwrap().lki_captured);
    let rec = w.lki.get(a).unwrap();
    assert!(rec.face_down());
    assert!(rec.permanent().unwrap().status().face_down);
}

// REQ-EVENT-001, REQ-CORE-004: CR 701.27b: a batch [TurnFaceDown, failing event] rolls back
// both the zone record and the permanent; errors are unchanged; a record/status disagreement
// is repaired by the event (which then occurs).
#[test]
fn req_event_001_cr_701_27b_face_down_rollback_errors_repair() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let h = w.create(2, 1, ZoneKind::Hand);
    let before = w.parts();
    assert_eq!(
        w.batch(vec![
            GameEvent::TurnFaceDown { object: a },
            GameEvent::Tap {
                object: ObjectId::new(99)
            },
        ]),
        Err(EventError::UnknownObject(ObjectId::new(99)))
    );
    assert!(w.parts() == before);
    assert_eq!(face_states(&w, a), (false, false));

    assert_fails(
        &mut w,
        GameEvent::TurnFaceDown {
            object: ObjectId::new(99),
        },
        EventError::UnknownObject(ObjectId::new(99)),
    );
    assert_fails(
        &mut w,
        GameEvent::TurnFaceDown { object: h },
        EventError::NotOnBattlefield(h),
    );
    assert_fails(
        &mut w,
        GameEvent::TurnFaceUp { object: h },
        EventError::NotOnBattlefield(h),
    );

    // Repair: the permanent says face down, the record says face up.
    w.permanents.set_face_down(a, true).unwrap();
    assert_eq!(face_states(&w, a), (false, true));
    let ex = w
        .exec(GameEvent::TurnFaceDown { object: a })
        .unwrap()
        .unwrap();
    assert_eq!(ex.outcome, Outcome::Applied);
    assert_eq!(face_states(&w, a), (true, true));
    assert_noop(&mut w, GameEvent::TurnFaceDown { object: a });
}

// REQ-EVENT-001: CR 400.7, CR 603.10a: a same-zone reposition reports no LKI even when a later
// draw of the same batch takes (and pre-captures) the same card; the draw reports it.
#[test]
fn req_event_001_cr_400_7_reposition_then_draw_lki_flag() {
    let mut w = World::new();
    let only = w.create(1, 1, ZoneKind::Library);
    let mut bottom = MoveRequest::new(ZoneKind::Library);
    bottom.position = Position::Bottom;
    let out = w
        .batch(vec![
            GameEvent::ZoneChange {
                object: only,
                request: bottom,
                cause: ZoneChangeCause::Other,
            },
            GameEvent::Draw { player: p(1) },
        ])
        .unwrap();
    assert_eq!(out.events.len(), 2);
    assert_eq!(moved_of(&out.events[0]).new, only);
    assert!(!out.events[0].lki_captured);
    assert_eq!(moved_of(&out.events[1]).old, only);
    assert!(out.events[1].lki_captured);
    assert_eq!(w.lki.ids(), vec![only]);
}

// REQ-EVENT-001, REQ-DSL-003: CR 111.8, CR 121.1: a token that left the battlefield for the
// library remains there when drawn: the draw is no event (no move, no LKI, no card drawn, no
// log entry) and is not an empty-library draw (CR 121.4).
#[test]
fn req_event_001_cr_111_8_token_in_library_not_drawn() {
    let mut w = World::new();
    let ex = w
        .exec(GameEvent::CreateToken {
            owner: p(1),
            card: c(50),
            controller: None,
        })
        .unwrap()
        .unwrap();
    let Outcome::TokenCreated { object } = ex.outcome else {
        panic!("expected a token");
    };
    let in_library = moved_of(&w.exec(zc(object, ZoneKind::Library)).unwrap().unwrap()).new;
    assert_eq!(w.zones.contents(ZoneId::Library(p(1))), vec![in_library]);
    assert_noop(&mut w, GameEvent::Draw { player: p(1) });
    assert_eq!(w.zones.contents(ZoneId::Library(p(1))), vec![in_library]);
    assert_eq!(
        w.hist(HistoryStat::CardsDrawn, HistoryScope::Game, Some(1)),
        0
    );
    assert!(w.events.empty_library_draws().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Batches and the log
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: a batch whose last event fails restores every component; a retry without it
// succeeds.
#[test]
fn req_event_001_batch_atomic_rollback_and_retry() {
    let mut w = World::new();
    let lib = w.create(1, 1, ZoneKind::Library);
    let a = w.create(2, 1, ZoneKind::Battlefield);
    let b = w.create(3, 1, ZoneKind::Battlefield);
    w.exec(GameEvent::LifeChange {
        player: p(2),
        delta: 1,
    })
    .unwrap();
    w.events.history_mut().record_spell_cast(1, p(1));
    let mut good = vec![
        GameEvent::Draw { player: p(1) },
        GameEvent::Draw { player: p(2) },
        GameEvent::LifeChange {
            player: p(1),
            delta: 5,
        },
        GameEvent::AddCounters {
            target: CounterTarget::Object(a),
            kind: "charge".into(),
            amount: 1,
        },
        GameEvent::Damage {
            source: None,
            source_controller: Some(p(2)),
            target: DamageTarget::Object(b),
            amount: 2,
            combat: true,
        },
        GameEvent::Tap { object: a },
        GameEvent::Attach {
            attachment: a,
            target: AttachTarget::Permanent(b),
        },
        GameEvent::CreateToken {
            owner: p(1),
            card: c(40),
            controller: None,
        },
        destroy(b),
        GameEvent::StepBegins { step: Step::Draw },
        GameEvent::TurnBegins { player: p(2) },
    ];
    let mut bad = good.clone();
    bad.push(GameEvent::LifeChange {
        player: p(9),
        delta: 1,
    });
    let before = w.parts();
    let before_bytes = encode_to_vec(&w.events);
    assert_eq!(w.batch(bad), Err(EventError::UnknownPlayer(p(9))));
    let after = w.parts();
    assert!(after.0 == before.0, "game");
    assert!(after.1 == before.1, "zones");
    assert_eq!(after.2, before.2, "ids high-water mark");
    assert_eq!(after.3, before.3, "permanents");
    assert_eq!(after.4, before.4, "memories");
    assert_eq!(after.5, before.5, "lki");
    assert_eq!(after.6, before.6, "event state");
    assert_eq!(encode_to_vec(&w.events), before_bytes);
    assert!(w.events.empty_library_draws().is_empty());

    let out = w.batch(std::mem::take(&mut good)).unwrap();
    assert_eq!(out.index, 1);
    assert_eq!(out.turn, 2);
    // The draw from player 2's empty library is no event.
    assert_eq!(out.events.len(), 10);
    assert_eq!(w.events.empty_library_draws(), vec![p(2)]);
    assert_eq!(w.lki.ids(), vec![lib, b]);
    assert_eq!(w.life(1), 25);
}

// REQ-EVENT-001: a batch where nothing occurs is returned empty and not logged; indices count
// logged batches; batches_since.
#[test]
fn req_event_001_log_indices_and_empty_batches() {
    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    let empty = w.batch(Vec::new()).unwrap();
    assert!(empty.events.is_empty());
    assert_eq!(empty.index, 0);
    assert_eq!(empty.turn, 1);
    let noop = w.batch(vec![GameEvent::Untap { object: a }]).unwrap();
    assert!(noop.events.is_empty());
    assert!(w.events.log().is_empty());
    assert_eq!(w.events.log().len(), 0);
    w.exec(GameEvent::Tap { object: a }).unwrap().unwrap();
    w.exec(GameEvent::Untap { object: a }).unwrap().unwrap();
    w.exec(GameEvent::Tap { object: a }).unwrap().unwrap();
    let log = w.events.log();
    assert_eq!(log.len(), 3);
    for (i, b) in log.batches().iter().enumerate() {
        assert_eq!(b.index, i as u64);
    }
    assert_eq!(log.batches_since(0).len(), 3);
    assert_eq!(log.batches_since(2).len(), 1);
    assert_eq!(log.batches_since(2)[0].index, 2);
    assert!(log.batches_since(3).is_empty());
    assert!(log.batches_since(u64::MAX).is_empty());
}

// REQ-EVENT-001: AppliedEffects travel into the log unchanged (CR 614.5).
#[test]
fn req_event_001_cr_614_5_applied_effects_logged() {
    let mut w = World::new();
    let mut applied = AppliedEffects::new();
    applied.insert(e(3));
    let inst = EventInstance::new(GameEvent::LifeChange {
        player: p(1),
        delta: 2,
    })
    .with_applied(applied.clone());
    let ex = w.ctx().execute(inst.clone()).unwrap().unwrap();
    assert_eq!(ex.instance, inst);
    assert_eq!(
        w.events.log().batches()[0].events[0]
            .instance
            .applied_effects(),
        &applied
    );
}

// ---------------------------------------------------------------------------------------------
// History ledger (REQ-DSL-003)
// ---------------------------------------------------------------------------------------------

// REQ-DSL-003: a scripted two-turn sequence gives the expected per-player, per-turn, game-wide
// and all-player values (CR 119.2, CR 120.3a for damage as life loss).
#[test]
fn req_dsl_003_scripted_history() {
    let mut w = World::new();
    for card in 1..=4 {
        w.create(card, 1, ZoneKind::Library);
    }
    let dmg = |amount| GameEvent::Damage {
        source: None,
        source_controller: Some(p(1)),
        target: DamageTarget::Player(p(2)),
        amount,
        combat: false,
    };
    // Turn 1.
    w.exec(GameEvent::Draw { player: p(1) }).unwrap();
    w.exec(GameEvent::Draw { player: p(1) }).unwrap();
    w.exec(GameEvent::LifeChange {
        player: p(1),
        delta: 3,
    })
    .unwrap();
    w.exec(GameEvent::LifeChange {
        player: p(1),
        delta: -1,
    })
    .unwrap();
    w.exec(dmg(4)).unwrap();
    w.exec(GameEvent::CreateToken {
        owner: p(1),
        card: c(100),
        controller: None,
    })
    .unwrap();
    w.events.history_mut().record_spell_cast(1, p(1));
    // Turn 2.
    w.exec(GameEvent::TurnBegins { player: p(2) }).unwrap();
    w.exec(GameEvent::Draw { player: p(1) }).unwrap();
    w.exec(GameEvent::LifeChange {
        player: p(2),
        delta: 2,
    })
    .unwrap();
    w.exec(dmg(1)).unwrap();
    w.events.history_mut().record_spell_cast(2, p(2));
    w.events.history_mut().record_spell_cast(2, p(2));

    use HistoryScope::{Game, Turn};
    use HistoryStat::*;
    let cases: &[(HistoryStat, HistoryScope, Option<u64>, i64)] = &[
        (CardsDrawn, Turn(1), Some(1), 2),
        (CardsDrawn, Turn(2), Some(1), 1),
        (CardsDrawn, Game, Some(1), 3),
        (CardsDrawn, Game, Some(2), 0),
        (CardsDrawn, Game, None, 3),
        (LifeGained, Turn(1), Some(1), 3),
        (LifeGained, Turn(2), Some(2), 2),
        (LifeGained, Game, None, 5),
        (LifeLost, Turn(1), Some(1), 1),
        (LifeLost, Turn(1), Some(2), 4),
        (LifeLost, Turn(2), Some(2), 1),
        (LifeLost, Game, Some(2), 5),
        (LifeLost, Game, None, 6),
        (DamageDealtToPlayer, Turn(1), Some(2), 4),
        (DamageDealtToPlayer, Game, Some(2), 5),
        (DamageDealtToPlayer, Game, Some(1), 0),
        (DamageDealtBy, Game, Some(1), 5),
        (DamageDealtBy, Turn(2), Some(1), 1),
        (TokensCreated, Turn(1), Some(1), 1),
        (TokensCreated, Turn(2), None, 0),
        (SpellsCast, Turn(1), Some(1), 1),
        (SpellsCast, Turn(2), Some(2), 2),
        (SpellsCast, Game, None, 3),
        (SpellsCast, Turn(3), None, 0),
    ];
    for &(stat, scope, player, expected) in cases {
        assert_eq!(
            w.hist(stat, scope, player),
            expected,
            "{stat:?} {scope:?} {player:?}"
        );
    }
    assert_eq!(w.life(1), 22);
    assert_eq!(w.life(2), 17);
}

// REQ-DSL-003: the ledger is empty at first, counts only executed events (no-ops record
// nothing), and saturates.
#[test]
fn req_dsl_003_history_basics() {
    let mut h = History::default();
    assert!(h.is_empty());
    let q = HistoryQuery {
        stat: HistoryStat::SpellsCast,
        scope: HistoryScope::Game,
        player: None,
    };
    assert_eq!(h.query(&q), 0);
    h.record_spell_cast(1, p(1));
    assert!(!h.is_empty());
    assert_eq!(h.query(&q), 1);
    h.add(1, HistoryStat::LifeGained, p(1), i64::MAX);
    h.add(1, HistoryStat::LifeGained, p(1), 5);
    h.add(2, HistoryStat::LifeGained, p(2), 5);
    h.add(2, HistoryStat::LifeGained, p(2), 0);
    let gained = |player| HistoryQuery {
        stat: HistoryStat::LifeGained,
        scope: HistoryScope::Game,
        player,
    };
    assert_eq!(h.query(&gained(Some(p(1)))), i64::MAX);
    assert_eq!(h.query(&gained(None)), i64::MAX);
    assert_eq!(h.query(&gained(Some(p(2)))), 5);

    let mut w = World::new();
    let a = w.create(1, 1, ZoneKind::Battlefield);
    assert_noop(
        &mut w,
        GameEvent::Damage {
            source: None,
            source_controller: Some(p(1)),
            target: DamageTarget::Object(a),
            amount: 0,
            combat: false,
        },
    );
    assert_noop(
        &mut w,
        GameEvent::LifeChange {
            player: p(1),
            delta: 0,
        },
    );
    assert!(w.events.history().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Canonical encoding and determinism (REQ-CORE-006, REQ-NFR-006)
// ---------------------------------------------------------------------------------------------

/// One event of each variant, in tag order.
fn sample_events() -> Vec<GameEvent> {
    let o = ObjectId::new(1);
    vec![
        zc(o, ZoneKind::Graveyard),
        GameEvent::Draw { player: p(1) },
        GameEvent::Damage {
            source: Some(o),
            source_controller: Some(p(1)),
            target: DamageTarget::Player(p(2)),
            amount: 1,
            combat: false,
        },
        GameEvent::LifeChange {
            player: p(1),
            delta: 1,
        },
        GameEvent::AddCounters {
            target: CounterTarget::Object(o),
            kind: "k".into(),
            amount: 1,
        },
        GameEvent::RemoveCounters {
            target: CounterTarget::Object(o),
            kind: "k".into(),
            amount: 1,
        },
        GameEvent::Tap { object: o },
        GameEvent::Untap { object: o },
        GameEvent::CreateToken {
            owner: p(1),
            card: c(1),
            controller: None,
        },
        GameEvent::TurnBegins { player: p(1) },
        GameEvent::PhaseBegins {
            phase: Phase::Beginning,
        },
        GameEvent::StepBegins { step: Step::Untap },
        GameEvent::Attach {
            attachment: o,
            target: AttachTarget::Player(p(1)),
        },
        GameEvent::Unattach { attachment: o },
        GameEvent::BecomeCopy {
            object: o,
            copied: o,
        },
        GameEvent::TurnFaceUp { object: o },
        GameEvent::TurnFaceDown { object: o },
        GameEvent::UnattachFrom {
            attachment: o,
            host: AttachTarget::Player(p(1)),
        },
    ]
}

/// Events that each differ from a sample event in one field.
fn event_field_variants() -> Vec<GameEvent> {
    let o = ObjectId::new(1);
    let mut req = MoveRequest::new(ZoneKind::Graveyard);
    let mut out = Vec::new();
    for pos in [Position::Bottom, Position::Index(0), Position::Index(3)] {
        req.position = pos;
        out.push(GameEvent::ZoneChange {
            object: o,
            request: req,
            cause: ZoneChangeCause::Other,
        });
    }
    let mut fd = MoveRequest::new(ZoneKind::Graveyard);
    fd.face_down = true;
    out.push(GameEvent::ZoneChange {
        object: o,
        request: fd,
        cause: ZoneChangeCause::Other,
    });
    let mut ctl = MoveRequest::new(ZoneKind::Battlefield);
    out.push(GameEvent::ZoneChange {
        object: o,
        request: ctl,
        cause: ZoneChangeCause::Other,
    });
    ctl.controller = Some(p(2));
    out.push(GameEvent::ZoneChange {
        object: o,
        request: ctl,
        cause: ZoneChangeCause::Other,
    });
    for cause in [
        ZoneChangeCause::Destroy,
        ZoneChangeCause::Sacrifice,
        ZoneChangeCause::Discard,
        ZoneChangeCause::Mill,
        ZoneChangeCause::Exile,
        ZoneChangeCause::CounterSpell,
        ZoneChangeCause::StateBasedAction,
    ] {
        out.push(GameEvent::ZoneChange {
            object: o,
            request: MoveRequest::new(ZoneKind::Graveyard),
            cause,
        });
    }
    for kind in ZoneKind::ALL {
        out.push(zc(ObjectId::new(2), kind));
    }
    let damage = |source, source_controller, target, amount, combat| GameEvent::Damage {
        source,
        source_controller,
        target,
        amount,
        combat,
    };
    out.push(damage(
        Some(o),
        Some(p(1)),
        DamageTarget::Player(p(2)),
        1,
        true,
    ));
    out.push(damage(
        None,
        Some(p(1)),
        DamageTarget::Player(p(2)),
        1,
        false,
    ));
    out.push(damage(Some(o), None, DamageTarget::Player(p(2)), 1, false));
    out.push(damage(
        Some(o),
        Some(p(1)),
        DamageTarget::Object(ObjectId::new(2)),
        1,
        false,
    ));
    out.push(damage(
        Some(o),
        Some(p(1)),
        DamageTarget::Player(p(2)),
        2,
        false,
    ));
    out.push(GameEvent::AddCounters {
        target: CounterTarget::Player(p(1)),
        kind: "k".into(),
        amount: 1,
    });
    out.push(GameEvent::AddCounters {
        target: CounterTarget::Object(o),
        kind: "kk".into(),
        amount: 1,
    });
    out.push(GameEvent::RemoveCounters {
        target: CounterTarget::Object(o),
        kind: "k".into(),
        amount: 2,
    });
    out.push(GameEvent::CreateToken {
        owner: p(1),
        card: c(1),
        controller: Some(p(1)),
    });
    for phase in [
        Phase::PrecombatMain,
        Phase::Combat,
        Phase::PostcombatMain,
        Phase::Ending,
    ] {
        out.push(GameEvent::PhaseBegins { phase });
    }
    for step in [
        Step::Upkeep,
        Step::Draw,
        Step::BeginningOfCombat,
        Step::DeclareAttackers,
        Step::DeclareBlockers,
        Step::FirstStrikeDamage,
        Step::CombatDamage,
        Step::EndOfCombat,
        Step::End,
        Step::Cleanup,
    ] {
        out.push(GameEvent::StepBegins { step });
    }
    out.push(GameEvent::Attach {
        attachment: o,
        target: AttachTarget::Permanent(ObjectId::new(1)),
    });
    out.push(GameEvent::BecomeCopy {
        object: o,
        copied: ObjectId::new(2),
    });
    out.push(GameEvent::UnattachFrom {
        attachment: o,
        host: AttachTarget::Player(p(2)),
    });
    out.push(GameEvent::UnattachFrom {
        attachment: o,
        host: AttachTarget::Permanent(ObjectId::new(1)),
    });
    out.push(GameEvent::UnattachFrom {
        attachment: ObjectId::new(2),
        host: AttachTarget::Player(p(1)),
    });
    out
}

// REQ-CORE-006: every GameEvent encodes distinctly; equal events encode equally; the first
// byte is the documented variant tag.
#[test]
fn req_core_006_game_event_encoding_injective() {
    let mut all = sample_events();
    all.extend(event_field_variants());
    let mut seen = BTreeMap::new();
    for ev in &all {
        assert_eq!(encode_to_vec(ev), encode_to_vec(&ev.clone()));
        if let Some(prev) = seen.insert(encode_to_vec(ev), ev.clone()) {
            assert_eq!(&prev, ev, "two different events share an encoding");
        }
    }
    let distinct: BTreeSet<_> = all.iter().map(|x| format!("{x:?}")).collect();
    assert_eq!(seen.len(), distinct.len());
    for (tag, ev) in sample_events().iter().enumerate() {
        assert_eq!(usize::from(encode_to_vec(ev)[0]), tag);
    }
}

// REQ-CORE-006: Outcome, EventInstance, ExecutedEvent, ExecutedBatch encodings.
#[test]
fn req_core_006_outcome_and_records_encoding() {
    let m = Moved {
        old: ObjectId::new(1),
        new: ObjectId::new(2),
        from: ZoneId::Hand(p(1)),
        to: ZoneId::Graveyard(p(1)),
    };
    let outcomes = vec![
        Outcome::ZoneChange(m),
        Outcome::ZoneChange(Moved {
            new: ObjectId::new(3),
            ..m
        }),
        Outcome::ZoneChange(Moved {
            old: ObjectId::new(5),
            ..m
        }),
        Outcome::ZoneChange(Moved {
            to: ZoneId::Graveyard(p(2)),
            ..m
        }),
        Outcome::ZoneChange(Moved {
            from: ZoneId::Exile,
            ..m
        }),
        Outcome::TokenCreated {
            object: ObjectId::new(1),
        },
        Outcome::CountersRemoved { removed: 1 },
        Outcome::Attached {
            timestamp: Timestamp::new(1),
        },
        Outcome::TurnBegan { turn: 1 },
        Outcome::Applied,
    ];
    let bytes: BTreeSet<_> = outcomes.iter().map(encode_to_vec).collect();
    assert_eq!(bytes.len(), outcomes.len());
    assert_eq!(
        encode_to_vec(&outcomes[0]),
        encode_to_vec(&Outcome::ZoneChange(m))
    );

    let inst = EventInstance::new(GameEvent::Tap {
        object: ObjectId::new(1),
    });
    let mut applied = AppliedEffects::new();
    applied.insert(e(1));
    let inst2 = inst.clone().with_applied(applied);
    assert_eq!(encode_to_vec(&inst), encode_to_vec(&inst.clone()));
    assert_ne!(encode_to_vec(&inst), encode_to_vec(&inst2));

    let ex = ExecutedEvent {
        instance: inst.clone(),
        outcome: Outcome::Applied,
        lki_captured: false,
        unattached: vec![],
    };
    let variants = [
        ExecutedEvent {
            instance: inst2,
            ..ex.clone()
        },
        ExecutedEvent {
            outcome: Outcome::TurnBegan { turn: 0 },
            ..ex.clone()
        },
        ExecutedEvent {
            lki_captured: true,
            ..ex.clone()
        },
    ];
    for v in &variants {
        assert_ne!(encode_to_vec(v), encode_to_vec(&ex));
    }
    assert_eq!(encode_to_vec(&ex), encode_to_vec(&ex.clone()));

    let b = ExecutedBatch {
        index: 0,
        turn: 1,
        events: vec![ex.clone()],
    };
    let bvars = [
        ExecutedBatch {
            index: 1,
            ..b.clone()
        },
        ExecutedBatch {
            turn: 2,
            ..b.clone()
        },
        ExecutedBatch {
            events: vec![ex.clone(), ex],
            ..b.clone()
        },
    ];
    for v in &bvars {
        assert_ne!(encode_to_vec(v), encode_to_vec(&b));
    }
}

// REQ-CORE-006: ObjectMarkers, EventLog, History and EventState encodings: equal values give
// equal bytes, one differing field gives different bytes.
#[test]
fn req_core_006_state_encodings() {
    let base = ObjectMarkers::default();
    let mut m1 = base.clone();
    m1.set_counter(ObjectId::new(1), "k", 1);
    let mut m2 = base.clone();
    m2.set_damage(ObjectId::new(1), 1);
    let mut m3 = base.clone();
    m3.mark_token(c(1));
    let mut m4 = base.clone();
    m4.set_copy(ObjectId::new(1), ObjectId::new(2));
    let mut m5 = m1.clone();
    m5.set_counter(ObjectId::new(1), "k", 2);
    let all = [&base, &m1, &m2, &m3, &m4, &m5];
    let bytes: BTreeSet<_> = all.iter().map(|m| encode_to_vec(*m)).collect();
    assert_eq!(bytes.len(), all.len());
    let mut back = m1.clone();
    back.set_counter(ObjectId::new(1), "k", 0);
    assert_eq!(back, base);
    assert_eq!(encode_to_vec(&back), encode_to_vec(&base));

    let mut h1 = History::default();
    h1.record_spell_cast(1, p(1));
    let mut h2 = History::default();
    h2.record_spell_cast(2, p(1));
    let mut h3 = History::default();
    h3.record_spell_cast(1, p(2));
    let mut h4 = History::default();
    h4.add(1, HistoryStat::CardsDrawn, p(1), 1);
    let hs = [History::default(), h1.clone(), h2, h3, h4];
    let hb: BTreeSet<_> = hs.iter().map(encode_to_vec).collect();
    assert_eq!(hb.len(), hs.len());
    assert_eq!(encode_to_vec(&h1), encode_to_vec(&h1.clone()));

    // EventState and EventLog through execution.
    let mut w = World::new();
    let s0 = w.events.clone();
    let log0 = encode_to_vec(w.events.log());
    w.exec(GameEvent::LifeChange {
        player: p(1),
        delta: 1,
    })
    .unwrap();
    assert_ne!(encode_to_vec(w.events.log()), log0);
    let s1 = w.events.clone();
    let mut s2 = s1.clone();
    s2.set_phase(Some(Phase::Combat));
    let mut s3 = s1.clone();
    s3.set_step(Some(Step::End));
    let mut s4 = s1.clone();
    s4.flag_empty_library_draw(p(1));
    let mut s5 = s1.clone();
    s5.markers_mut().mark_token(c(3));
    let mut s6 = s1.clone();
    s6.history_mut().record_spell_cast(1, p(2));
    let states = [s0, s1.clone(), s2, s3, s4, s5, s6];
    let sb: BTreeSet<_> = states.iter().map(encode_to_vec).collect();
    assert_eq!(sb.len(), states.len());
    assert_eq!(encode_to_vec(&s1), encode_to_vec(&s1.clone()));
}

/// A tiny deterministic generator for test scripts (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.below(len as u64)).unwrap()
    }
}

/// A pseudo-random event over the live objects and players of `w` (some deliberately invalid).
fn random_event(rng: &mut Rng, w: &World) -> GameEvent {
    let ids = w.zones.all_object_ids();
    let obj = if ids.is_empty() || rng.below(10) == 0 {
        ObjectId::new(1_000 + rng.below(5))
    } else {
        ids[rng.index(ids.len())]
    };
    let player = p(1 + rng.below(3));
    let amount = i64::try_from(rng.below(5)).unwrap() - 1;
    let kinds = [
        ZoneKind::Library,
        ZoneKind::Hand,
        ZoneKind::Battlefield,
        ZoneKind::Graveyard,
        ZoneKind::Exile,
    ];
    match rng.below(14) {
        0 => zc(obj, kinds[rng.index(kinds.len())]),
        1 => GameEvent::Draw { player },
        2 => GameEvent::Damage {
            source: None,
            source_controller: Some(player),
            target: if rng.below(2) == 0 {
                DamageTarget::Player(player)
            } else {
                DamageTarget::Object(obj)
            },
            amount,
            combat: rng.below(2) == 0,
        },
        3 => GameEvent::LifeChange {
            player,
            delta: amount * 2 - 2,
        },
        4 => GameEvent::AddCounters {
            target: CounterTarget::Object(obj),
            kind: "charge".into(),
            amount,
        },
        5 => GameEvent::RemoveCounters {
            target: CounterTarget::Object(obj),
            kind: "charge".into(),
            amount,
        },
        6 => GameEvent::Tap { object: obj },
        7 => GameEvent::Untap { object: obj },
        8 => GameEvent::CreateToken {
            owner: player,
            card: c(10_000 + rng.below(40)),
            controller: None,
        },
        9 => GameEvent::TurnBegins { player },
        10 => GameEvent::StepBegins { step: Step::Upkeep },
        11 => GameEvent::Attach {
            attachment: obj,
            target: AttachTarget::Player(player),
        },
        12 => GameEvent::BecomeCopy {
            object: obj,
            copied: obj,
        },
        _ => GameEvent::TurnFaceDown { object: obj },
    }
}

fn seeded_world() -> World {
    let mut w = World::new();
    for card in 1..=6 {
        w.create(card, 1, ZoneKind::Library);
        w.create(card + 10, 2, ZoneKind::Library);
    }
    for card in 21..=24 {
        w.create(card, 1 + card % 2, ZoneKind::Battlefield);
    }
    w
}

// REQ-CORE-001, REQ-CORE-006, REQ-EVENT-001: property: replaying the same random event script
// on two identical worlds gives equal state and equal encodings; every failed batch leaves
// every component unchanged; log indices are contiguous and logged batches are non-empty.
#[test]
fn req_event_001_property_determinism_and_atomicity() {
    for seed in 1..=40_u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut a = seeded_world();
        let mut b = seeded_world();
        for _ in 0..30 {
            let n = 1 + rng.below(3);
            let events: Vec<GameEvent> = (0..n).map(|_| random_event(&mut rng, &a)).collect();
            let before = a.parts();
            let ra = a.batch(events.clone());
            let rb = b.batch(events);
            assert_eq!(ra, rb);
            if ra.is_err() {
                assert!(
                    a.parts() == before,
                    "seed {seed}: failed batch changed state"
                );
            }
            assert!(a.parts() == b.parts());
        }
        assert_eq!(encode_to_vec(&a.events), encode_to_vec(&b.events));
        assert_eq!(encode_to_vec(&a.zones), encode_to_vec(&b.zones));
        assert_eq!(encode_to_vec(&a.lki), encode_to_vec(&b.lki));
        for (i, batch) in a.events.log().batches().iter().enumerate() {
            assert_eq!(batch.index, i as u64);
            assert!(!batch.events.is_empty());
        }
    }
}

// REQ-EVENT-001: EventState accessors on a fresh state.
#[test]
fn req_event_001_event_state_fresh() {
    let s = EventState::new();
    assert_eq!(s, EventState::default());
    assert!(s.log().is_empty());
    assert!(s.history().is_empty());
    assert_eq!(s.phase(), None);
    assert_eq!(s.step(), None);
    assert!(s.empty_library_draws().is_empty());
    assert_eq!(s.markers(), &ObjectMarkers::default());
    assert_eq!(s.markers().damage(ObjectId::new(1)), 0);
    assert_eq!(s.markers().counter(ObjectId::new(1), "x"), 0);
    assert_eq!(s.markers().copy_of(ObjectId::new(1)), None);
}

// REQ-EVENT-001, REQ-CORE-003: CR 707.5, CR 614.1c, CR 608.2h:
// `execute_batch_with_entry_copies` sets the copy link as part of the entering event, in that
// event's own executed batch (token and zone change); a copied object that left earlier in the
// batch is accepted through its LKI; one with neither a zone record nor LKI is `UnknownObject`
// and the call rolls back; a listed event that did not enter sets nothing; `execute_batch`
// keeps standalone `BecomeCopy` strict for a departed copied object.
#[test]
fn req_event_001_cr_707_5_execute_batch_with_entry_copies() {
    // Zone change, source stays.
    let mut w = World::new();
    let original = w.create(1, 2, ZoneKind::Battlefield);
    let h = w.create(2, 1, ZoneKind::Hand);
    let batch = EventBatch::from_events(vec![zc(h, ZoneKind::Battlefield)]);
    let ex = w
        .ctx()
        .execute_batch_with_entry_copies(&batch, &[(0, original)])
        .unwrap();
    assert_eq!(ex.events.len(), 1);
    let new = moved_of(&ex.events[0]).new;
    assert_eq!(w.events.markers().copy_of(new), Some(original));
    assert_eq!(w.events.log().len(), 1);
    // A token.
    let token = GameEvent::CreateToken {
        owner: p(1),
        card: c(9),
        controller: None,
    };
    let ex = w
        .ctx()
        .execute_batch_with_entry_copies(&EventBatch::from_events(vec![token]), &[(0, original)])
        .unwrap();
    let Outcome::TokenCreated { object } = ex.events[0].outcome else {
        panic!("not a token");
    };
    assert_eq!(w.events.markers().copy_of(object), Some(original));
    assert_eq!(w.events.log().len(), 2);

    // The source leaves earlier in the same batch: accepted through LKI captured before it.
    for source_first in [true, false] {
        let mut w = World::new();
        let original = w.create(1, 2, ZoneKind::Battlefield);
        let h = w.create(2, 1, ZoneKind::Hand);
        let (events, index) = if source_first {
            (vec![destroy(original), zc(h, ZoneKind::Battlefield)], 1)
        } else {
            (vec![zc(h, ZoneKind::Battlefield), destroy(original)], 0)
        };
        let ex = w
            .ctx()
            .execute_batch_with_entry_copies(&EventBatch::from_events(events), &[(index, original)])
            .unwrap();
        assert_eq!(ex.events.len(), 2);
        let new = moved_of(&ex.events[index]).new;
        assert_eq!(w.events.markers().copy_of(new), Some(original));
        assert!(w.zones.object(original).is_none());
        assert!(w.lki.get(original).is_some());
        assert_eq!(w.events.log().len(), 1);
        // Standalone BecomeCopy of the departed id stays strict.
        assert_fails(
            &mut w,
            GameEvent::BecomeCopy {
                object: new,
                copied: original,
            },
            EventError::UnknownObject(original),
        );
        // In a later call the departed source is still accepted for an entering copy.
        let h2 = w.create(3, 1, ZoneKind::Hand);
        let ex = w
            .ctx()
            .execute_batch_with_entry_copies(
                &EventBatch::from_events(vec![zc(h2, ZoneKind::Battlefield)]),
                &[(0, original)],
            )
            .unwrap();
        let new2 = moved_of(&ex.events[0]).new;
        assert_eq!(w.events.markers().copy_of(new2), Some(original));
    }

    // Neither a zone record nor LKI: UnknownObject, everything rolls back.
    let mut w = World::new();
    let h = w.create(2, 1, ZoneKind::Hand);
    let before = w.parts();
    let batch = EventBatch::from_events(vec![life(1, 1), zc(h, ZoneKind::Battlefield)]);
    assert_eq!(
        w.ctx()
            .execute_batch_with_entry_copies(&batch, &[(1, ObjectId::new(90))]),
        Err(EventError::UnknownObject(ObjectId::new(90)))
    );
    assert!(w.parts() == before, "a failed batch changed state");

    // A listed event that did not enter: nothing set, nothing fails (even for an unknown id).
    let mut w = World::new();
    let x = w.create(1, 1, ZoneKind::Battlefield);
    let h = w.create(2, 1, ZoneKind::Hand);
    let ex = w
        .ctx()
        .execute_batch_with_entry_copies(
            &EventBatch::from_events(vec![zc(h, ZoneKind::Graveyard), life(1, 0)]),
            &[(0, x), (1, ObjectId::new(90))],
        )
        .unwrap();
    assert_eq!(ex.events.len(), 1);
    let gone = moved_of(&ex.events[0]).new;
    assert_eq!(w.events.markers().copy_of(gone), None);
    assert_eq!(w.events.markers().copy_of(x), None);
    // An empty list behaves as `execute_batch`.
    let mut a = World::new();
    let ha = a.create(2, 1, ZoneKind::Hand);
    let mut b = a.clone();
    let ea = a
        .ctx()
        .execute_batch_with_entry_copies(
            &EventBatch::from_events(vec![zc(ha, ZoneKind::Battlefield)]),
            &[],
        )
        .unwrap();
    let eb = b.batch(vec![zc(ha, ZoneKind::Battlefield)]).unwrap();
    assert_eq!(ea, eb);
    assert!(a.parts() == b.parts());
}

fn life(player: u64, delta: i64) -> GameEvent {
    GameEvent::LifeChange {
        player: p(player),
        delta,
    }
}
