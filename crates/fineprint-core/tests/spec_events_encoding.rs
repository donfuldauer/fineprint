// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the canonical encoding of the events types (REQ-CORE-006, REQ-EVENT-001,
//! REQ-DSL-003): equal values give equal bytes; a one-field difference gives different bytes.
//! Black-box: written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::ids::Timestamp;
use fineprint_core::events::{
    CounterTarget, DamageTarget, EventInstance, EventState, ExecutedBatch, ExecutedEvent,
    GameEvent, History, Outcome, Phase, Step, ZoneChangeCause,
};
use fineprint_core::permanent::AttachTarget;
use fineprint_core::zones::{MoveRequest, Moved, Position, ZoneId, ZoneKind};
use support::*;

/// Asserts that every value encodes deterministically and that all values differ pairwise in
/// bytes. Also checks that encoding appends to a non-empty buffer.
fn assert_distinct<T: CanonicalEncode + Clone + std::fmt::Debug>(values: &[T]) {
    let encoded: Vec<Vec<u8>> = values.iter().map(bytes).collect();
    for (i, v) in values.iter().enumerate() {
        assert_eq!(
            bytes(&v.clone()),
            encoded[i],
            "clone of {v:?} encodes equally"
        );
        let mut out = vec![0xA5u8, 0x5A];
        v.encode_canonical(&mut out);
        assert_eq!(&out[..2], &[0xA5, 0x5A], "prefix kept");
        assert_eq!(&out[2..], encoded[i].as_slice());
        for (j, w) in values.iter().enumerate().skip(i + 1) {
            assert_ne!(encoded[i], encoded[j], "{v:?} vs {w:?}");
        }
    }
}

fn mv(to: ZoneKind) -> MoveRequest {
    MoveRequest::new(to)
}

/// Every GameEvent variant, plus a one-field variation of each field.
fn events() -> Vec<GameEvent> {
    let mut face_down = mv(ZoneKind::Exile);
    face_down.face_down = true;
    let mut bottom = mv(ZoneKind::Library);
    bottom.position = Position::Bottom;
    let mut controlled = mv(ZoneKind::Battlefield);
    controlled.controller = Some(P2);
    let dmg = |source, sc, target, amount, combat| GameEvent::Damage {
        source,
        source_controller: sc,
        target,
        amount,
        combat,
    };
    vec![
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Graveyard),
            cause: ZoneChangeCause::Other,
        },
        GameEvent::ZoneChange {
            object: oid(2),
            request: mv(ZoneKind::Graveyard),
            cause: ZoneChangeCause::Other,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Exile),
            cause: ZoneChangeCause::Other,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: face_down,
            cause: ZoneChangeCause::Other,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Library),
            cause: ZoneChangeCause::Other,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: bottom,
            cause: ZoneChangeCause::Other,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Battlefield),
            cause: ZoneChangeCause::Other,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: controlled,
            cause: ZoneChangeCause::Other,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Graveyard),
            cause: ZoneChangeCause::Destroy,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Graveyard),
            cause: ZoneChangeCause::Sacrifice,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Graveyard),
            cause: ZoneChangeCause::Discard,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Graveyard),
            cause: ZoneChangeCause::Mill,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Graveyard),
            cause: ZoneChangeCause::Exile,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Graveyard),
            cause: ZoneChangeCause::CounterSpell,
        },
        GameEvent::ZoneChange {
            object: oid(1),
            request: mv(ZoneKind::Graveyard),
            cause: ZoneChangeCause::StateBasedAction,
        },
        GameEvent::Draw { player: P1 },
        GameEvent::Draw { player: P2 },
        dmg(None, None, DamageTarget::Player(P1), 1, false),
        dmg(Some(oid(1)), None, DamageTarget::Player(P1), 1, false),
        dmg(Some(oid(2)), None, DamageTarget::Player(P1), 1, false),
        dmg(None, Some(P1), DamageTarget::Player(P1), 1, false),
        dmg(None, Some(P2), DamageTarget::Player(P1), 1, false),
        dmg(None, None, DamageTarget::Player(P2), 1, false),
        dmg(None, None, DamageTarget::Object(oid(1)), 1, false),
        dmg(None, None, DamageTarget::Player(P1), 2, false),
        dmg(None, None, DamageTarget::Player(P1), 1, true),
        GameEvent::LifeChange {
            player: P1,
            delta: 1,
        },
        GameEvent::LifeChange {
            player: P1,
            delta: -1,
        },
        GameEvent::LifeChange {
            player: P2,
            delta: 1,
        },
        GameEvent::AddCounters {
            target: CounterTarget::Object(oid(1)),
            kind: "test.a".into(),
            amount: 1,
        },
        GameEvent::AddCounters {
            target: CounterTarget::Player(P1),
            kind: "test.a".into(),
            amount: 1,
        },
        GameEvent::AddCounters {
            target: CounterTarget::Object(oid(1)),
            kind: "test.b".into(),
            amount: 1,
        },
        GameEvent::AddCounters {
            target: CounterTarget::Object(oid(1)),
            kind: "test.a".into(),
            amount: 2,
        },
        GameEvent::RemoveCounters {
            target: CounterTarget::Object(oid(1)),
            kind: "test.a".into(),
            amount: 1,
        },
        GameEvent::RemoveCounters {
            target: CounterTarget::Object(oid(1)),
            kind: "test.a".into(),
            amount: 2,
        },
        GameEvent::Tap { object: oid(1) },
        GameEvent::Tap { object: oid(2) },
        GameEvent::Untap { object: oid(1) },
        GameEvent::CreateToken {
            owner: P1,
            card: cid(1),
            controller: None,
        },
        GameEvent::CreateToken {
            owner: P2,
            card: cid(1),
            controller: None,
        },
        GameEvent::CreateToken {
            owner: P1,
            card: cid(2),
            controller: None,
        },
        GameEvent::CreateToken {
            owner: P1,
            card: cid(1),
            controller: Some(P1),
        },
        GameEvent::TurnBegins { player: P1 },
        GameEvent::TurnBegins { player: P2 },
        GameEvent::PhaseBegins {
            phase: Phase::Beginning,
        },
        GameEvent::PhaseBegins {
            phase: Phase::PrecombatMain,
        },
        GameEvent::PhaseBegins {
            phase: Phase::Combat,
        },
        GameEvent::PhaseBegins {
            phase: Phase::PostcombatMain,
        },
        GameEvent::PhaseBegins {
            phase: Phase::Ending,
        },
        GameEvent::StepBegins { step: Step::Untap },
        GameEvent::StepBegins { step: Step::Upkeep },
        GameEvent::StepBegins { step: Step::Draw },
        GameEvent::StepBegins {
            step: Step::BeginningOfCombat,
        },
        GameEvent::StepBegins {
            step: Step::DeclareAttackers,
        },
        GameEvent::StepBegins {
            step: Step::DeclareBlockers,
        },
        GameEvent::StepBegins {
            step: Step::FirstStrikeDamage,
        },
        GameEvent::StepBegins {
            step: Step::CombatDamage,
        },
        GameEvent::StepBegins {
            step: Step::EndOfCombat,
        },
        GameEvent::StepBegins { step: Step::End },
        GameEvent::StepBegins {
            step: Step::Cleanup,
        },
        GameEvent::Attach {
            attachment: oid(1),
            target: AttachTarget::Permanent(oid(2)),
        },
        GameEvent::Attach {
            attachment: oid(1),
            target: AttachTarget::Permanent(oid(3)),
        },
        GameEvent::Attach {
            attachment: oid(1),
            target: AttachTarget::Player(P1),
        },
        GameEvent::Attach {
            attachment: oid(2),
            target: AttachTarget::Player(P1),
        },
        GameEvent::Unattach { attachment: oid(1) },
        GameEvent::Unattach { attachment: oid(2) },
        GameEvent::BecomeCopy {
            object: oid(1),
            copied: oid(2),
        },
        GameEvent::BecomeCopy {
            object: oid(1),
            copied: oid(3),
        },
        GameEvent::BecomeCopy {
            object: oid(3),
            copied: oid(2),
        },
        GameEvent::TurnFaceUp { object: oid(1) },
        GameEvent::TurnFaceUp { object: oid(2) },
        GameEvent::TurnFaceDown { object: oid(1) },
    ]
}

// REQ-CORE-006, REQ-EVENT-001: every GameEvent variant encodes deterministically and any
// one-field difference changes the bytes.
#[test]
fn game_event_encoding() {
    assert_distinct(&events());
}

// REQ-CORE-006, CR 614.5: EventInstance encodes its AppliedEffects.
#[test]
fn event_instance_encoding() {
    let ev = GameEvent::Draw { player: P1 };
    let other = GameEvent::Draw { player: P2 };
    assert_distinct(&[
        EventInstance::new(ev.clone()),
        EventInstance::new(other.clone()),
        EventInstance::new(ev.clone()).with_applied(applied(&[1])),
        EventInstance::new(ev.clone()).with_applied(applied(&[2])),
        EventInstance::new(ev.clone()).with_applied(applied(&[1, 2])),
        EventInstance::new(other).with_applied(applied(&[1])),
    ]);
    // Insertion order does not matter (a set).
    assert_eq!(
        bytes(&EventInstance::new(ev.clone()).with_applied(applied(&[3, 1]))),
        bytes(&EventInstance::new(ev).with_applied(applied(&[1, 3])))
    );
}

fn moved(old: u64, new: u64, from: ZoneId, to: ZoneId) -> Moved {
    Moved {
        old: oid(old),
        new: oid(new),
        from,
        to,
    }
}

fn outcomes() -> Vec<Outcome> {
    vec![
        Outcome::ZoneChange(moved(1, 2, ZoneId::Battlefield, ZoneId::Graveyard(P1))),
        Outcome::ZoneChange(moved(3, 2, ZoneId::Battlefield, ZoneId::Graveyard(P1))),
        Outcome::ZoneChange(moved(1, 4, ZoneId::Battlefield, ZoneId::Graveyard(P1))),
        Outcome::ZoneChange(moved(1, 2, ZoneId::Exile, ZoneId::Graveyard(P1))),
        Outcome::ZoneChange(moved(1, 2, ZoneId::Battlefield, ZoneId::Graveyard(P2))),
        Outcome::TokenCreated { object: oid(1) },
        Outcome::TokenCreated { object: oid(2) },
        Outcome::CountersRemoved { removed: 1 },
        Outcome::CountersRemoved { removed: 2 },
        Outcome::Attached {
            timestamp: Timestamp::new(1),
        },
        Outcome::Attached {
            timestamp: Timestamp::new(2),
        },
        Outcome::TurnBegan { turn: 1 },
        Outcome::TurnBegan { turn: 2 },
        Outcome::Applied,
    ]
}

// REQ-CORE-006: every Outcome variant and field is encoded.
#[test]
fn outcome_encoding() {
    assert_distinct(&outcomes());
}

fn executed(ev: GameEvent, outcome: Outcome, lki: bool) -> ExecutedEvent {
    ExecutedEvent {
        instance: EventInstance::new(ev),
        outcome,
        lki_captured: lki,
    }
}

// REQ-CORE-006: ExecutedEvent and ExecutedBatch encode every field.
#[test]
fn executed_event_and_batch_encoding() {
    let a = executed(GameEvent::Draw { player: P1 }, Outcome::Applied, false);
    let execs = vec![
        a.clone(),
        executed(GameEvent::Draw { player: P2 }, Outcome::Applied, false),
        executed(
            GameEvent::Draw { player: P1 },
            Outcome::TurnBegan { turn: 1 },
            false,
        ),
        executed(GameEvent::Draw { player: P1 }, Outcome::Applied, true),
        ExecutedEvent {
            instance: EventInstance::new(GameEvent::Draw { player: P1 })
                .with_applied(applied(&[1])),
            outcome: Outcome::Applied,
            lki_captured: false,
        },
    ];
    assert_distinct(&execs);

    let b = executed(GameEvent::Tap { object: oid(1) }, Outcome::Applied, false);
    assert_distinct(&[
        ExecutedBatch {
            index: 0,
            turn: 1,
            events: vec![a.clone()],
        },
        ExecutedBatch {
            index: 1,
            turn: 1,
            events: vec![a.clone()],
        },
        ExecutedBatch {
            index: 0,
            turn: 2,
            events: vec![a.clone()],
        },
        ExecutedBatch {
            index: 0,
            turn: 1,
            events: vec![b.clone()],
        },
        ExecutedBatch {
            index: 0,
            turn: 1,
            events: vec![a.clone(), b.clone()],
        },
        ExecutedBatch {
            index: 0,
            turn: 1,
            events: vec![b, a],
        },
        ExecutedBatch {
            index: 0,
            turn: 1,
            events: vec![],
        },
    ]);
}

/// Runs `script` on a fresh world with a battlefield creature `oid(1)` and returns the world.
fn world_after(script: &[GameEvent]) -> World {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    assert_eq!(a, oid(1));
    for ev in script {
        w.exec(ev.clone()).expect("script event succeeds");
    }
    w
}

fn scripts() -> Vec<Vec<GameEvent>> {
    let life = |player, delta| GameEvent::LifeChange { player, delta };
    let counters = |kind: &str, amount| GameEvent::AddCounters {
        target: CounterTarget::Object(oid(1)),
        kind: kind.into(),
        amount,
    };
    let damage = |amount| GameEvent::Damage {
        source: None,
        source_controller: None,
        target: DamageTarget::Object(oid(1)),
        amount,
        combat: false,
    };
    vec![
        vec![],
        vec![life(P1, 1)],
        vec![life(P1, 2)],
        vec![life(P2, 1)],
        vec![life(P1, -1)],
        vec![life(P1, 1), life(P1, 1)],
        vec![counters("test.a", 1)],
        vec![counters("test.a", 2)],
        vec![counters("test.b", 1)],
        vec![damage(1)],
        vec![damage(2)],
        vec![GameEvent::Tap { object: oid(1) }],
        vec![GameEvent::PhaseBegins {
            phase: Phase::Combat,
        }],
        vec![GameEvent::StepBegins { step: Step::End }],
        vec![GameEvent::TurnBegins { player: P2 }],
    ]
}

// REQ-CORE-006: EventLog, ObjectMarkers, History and EventState: equal histories give equal
// bytes; different histories give different bytes.
#[test]
fn state_component_encoding() {
    let worlds: Vec<World> = scripts().iter().map(|s| world_after(s)).collect();
    let again: Vec<World> = scripts().iter().map(|s| world_after(s)).collect();
    for (w, v) in worlds.iter().zip(&again) {
        assert_eq!(bytes(&w.events), bytes(&v.events));
        assert_eq!(bytes(w.events.log()), bytes(v.events.log()));
        assert_eq!(bytes(w.events.markers()), bytes(v.events.markers()));
        assert_eq!(bytes(w.events.history()), bytes(v.events.history()));
        assert_eq!(bytes(&w.events.clone()), bytes(&w.events));
    }
    // EventState: every script differs.
    let states: Vec<Vec<u8>> = worlds.iter().map(|w| bytes(&w.events)).collect();
    for i in 0..states.len() {
        for j in i + 1..states.len() {
            assert_ne!(states[i], states[j], "scripts {i} and {j}");
        }
    }
    // EventLog: every script differs (each logs something different).
    let logs: Vec<Vec<u8>> = worlds.iter().map(|w| bytes(w.events.log())).collect();
    for i in 0..logs.len() {
        for j in i + 1..logs.len() {
            assert_ne!(logs[i], logs[j], "logs {i} and {j}");
        }
    }
    // ObjectMarkers: scripts 0, 6, 7, 8, 9, 10 differ in markers.
    let marker_idx = [0usize, 6, 7, 8, 9, 10];
    for (k, &i) in marker_idx.iter().enumerate() {
        for &j in &marker_idx[k + 1..] {
            assert_ne!(
                bytes(worlds[i].events.markers()),
                bytes(worlds[j].events.markers()),
                "markers {i} and {j}"
            );
        }
    }
    // History: scripts 0..=4 differ in the ledger (script 5 ledgers the same as script 2).
    for i in 0..=4 {
        for j in i + 1..=4 {
            assert_ne!(
                bytes(worlds[i].events.history()),
                bytes(worlds[j].events.history()),
                "history {i} and {j}"
            );
        }
    }
}

// REQ-CORE-006: History encodes turn, player and count; the empty-library flag set is part of
// EventState; a token mark is part of ObjectMarkers.
#[test]
fn history_flags_and_token_marks_are_encoded() {
    let mut hs = Vec::new();
    for (turn, p, n) in [(1u64, P1, 1), (2, P1, 1), (1, P2, 1), (1, P1, 2)] {
        let mut h = History::default();
        for _ in 0..n {
            h.record_spell_cast(turn, p);
        }
        hs.push(h);
    }
    hs.push(History::default());
    let enc: Vec<Vec<u8>> = hs.iter().map(bytes).collect();
    for i in 0..enc.len() {
        for j in i + 1..enc.len() {
            assert_ne!(enc[i], enc[j], "history {i} and {j}");
        }
    }

    let mut flagged = World::new();
    assert_eq!(flagged.exec(GameEvent::Draw { player: P1 }), Ok(None));
    assert_ne!(bytes(&flagged.events), bytes(&EventState::new()));
    let mut other = World::new();
    assert_eq!(other.exec(GameEvent::Draw { player: P2 }), Ok(None));
    assert_ne!(bytes(&flagged.events), bytes(&other.events));
    flagged.events.take_empty_library_draws();
    assert_eq!(bytes(&flagged.events), bytes(&EventState::new()));

    // A created token is marked in ObjectMarkers.
    let mut tok = World::new();
    let card = tok.fresh_card();
    tok.ok(GameEvent::CreateToken {
        owner: P1,
        card,
        controller: None,
    });
    assert_ne!(
        bytes(tok.events.markers()),
        bytes(EventState::new().markers())
    );
}
