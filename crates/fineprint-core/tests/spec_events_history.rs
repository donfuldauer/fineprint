// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the history ledger behind `HISTORY(query)` (REQ-DSL-003, REQ-EVENT-001;
//! CR 119.2, 119.9, 120.3a, 120.8, 121.4). Black-box: written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::PlayerId;
use fineprint_core::events::{
    DamageTarget, EventError, GameEvent, History, HistoryQuery, HistoryScope, HistoryStat,
    ZoneChangeCause,
};
use fineprint_core::zones::{MoveRequest, ZoneKind};
use support::*;

const STATS: [HistoryStat; 7] = [
    HistoryStat::SpellsCast,
    HistoryStat::LifeGained,
    HistoryStat::LifeLost,
    HistoryStat::CardsDrawn,
    HistoryStat::DamageDealtBy,
    HistoryStat::DamageDealtToPlayer,
    HistoryStat::TokensCreated,
];

fn query(h: &History, stat: HistoryStat, scope: HistoryScope, player: Option<PlayerId>) -> i64 {
    h.query(&HistoryQuery {
        stat,
        scope,
        player,
    })
}

fn damage(sc: Option<PlayerId>, target: DamageTarget, amount: i64, combat: bool) -> GameEvent {
    GameEvent::Damage {
        source: None,
        source_controller: sc,
        target,
        amount,
        combat,
    }
}

// REQ-DSL-003: an empty ledger answers 0 to everything.
#[test]
fn empty_ledger_is_zero() {
    let h = History::default();
    assert!(h.is_empty());
    assert!(World::new().events.history().is_empty());
    for stat in STATS {
        for scope in [
            HistoryScope::Turn(0),
            HistoryScope::Turn(1),
            HistoryScope::Game,
        ] {
            for p in [Some(P1), Some(P2), None] {
                assert_eq!(query(&h, stat, scope, p), 0);
            }
        }
    }
}

// REQ-DSL-003, CR 601.2i: record_spell_cast adds 1 to SpellsCast for that turn and player.
#[test]
fn record_spell_cast() {
    let mut h = History::default();
    h.record_spell_cast(4, P2);
    h.record_spell_cast(4, P2);
    h.record_spell_cast(5, P2);
    assert!(!h.is_empty());
    assert_eq!(
        query(&h, HistoryStat::SpellsCast, HistoryScope::Turn(4), Some(P2)),
        2
    );
    assert_eq!(
        query(&h, HistoryStat::SpellsCast, HistoryScope::Turn(5), Some(P2)),
        1
    );
    assert_eq!(
        query(&h, HistoryStat::SpellsCast, HistoryScope::Game, Some(P2)),
        3
    );
    assert_eq!(
        query(&h, HistoryStat::SpellsCast, HistoryScope::Game, Some(P1)),
        0
    );
    assert_eq!(
        query(&h, HistoryStat::SpellsCast, HistoryScope::Game, None),
        3
    );
    assert_eq!(
        query(&h, HistoryStat::LifeGained, HistoryScope::Game, None),
        0
    );
}

/// The hand-computed ledger of the scripted sequence: (stat, turn, player) -> value.
fn expected_table() -> BTreeMap<(HistoryStat, u64, PlayerId), i64> {
    use HistoryStat::*;
    [
        // turn 1
        ((SpellsCast, 1, P1), 1),
        ((LifeGained, 1, P1), 3),
        ((LifeLost, 1, P2), 6),
        ((CardsDrawn, 1, P1), 2),
        ((DamageDealtBy, 1, P1), 4),
        ((DamageDealtBy, 1, P2), 3),
        ((DamageDealtToPlayer, 1, P2), 4),
        ((TokensCreated, 1, P1), 1),
        // turn 2
        ((SpellsCast, 2, P1), 1),
        ((SpellsCast, 2, P2), 2),
        ((LifeGained, 2, P2), 7),
        ((LifeLost, 2, P1), 6),
        ((LifeLost, 2, P2), 1),
        ((CardsDrawn, 2, P2), 1),
        ((DamageDealtBy, 2, P2), 6),
        ((DamageDealtToPlayer, 2, P1), 5),
        ((DamageDealtToPlayer, 2, P2), 1),
        ((TokensCreated, 2, P2), 1),
    ]
    .into()
}

// REQ-DSL-003, REQ-EVENT-001, CR 119.2, 120.3a, 120.8, 119.9, 121.4: a scripted sequence over
// two turns and two players gives the hand-computed values for every stat, for Turn(1),
// Turn(2), Game, per player and summed; no-op, failed and rolled-back events are not counted.
#[test]
fn scripted_sequence_matches_hand_computed_ledger() {
    let mut w = World::new();
    w.create(ZoneKind::Library, P1);
    w.create(ZoneKind::Library, P1);
    w.create(ZoneKind::Library, P2);
    let s1 = w.create(ZoneKind::Battlefield, P1);
    let target = w.create(ZoneKind::Battlefield, P2);
    let gy = w.create(ZoneKind::Graveyard, P2);

    // ----- turn 1 -----
    assert_eq!(w.turn(), 1);
    w.ok(GameEvent::Draw { player: P1 });
    w.ok(GameEvent::Draw { player: P1 });
    w.ok(GameEvent::LifeChange {
        player: P1,
        delta: 3,
    });
    w.ok(GameEvent::LifeChange {
        player: P2,
        delta: -2,
    });
    // CR 119.2, 120.3a: damage to a player is also life loss.
    w.ok(GameEvent::Damage {
        source: Some(s1),
        source_controller: Some(P1),
        target: DamageTarget::Player(P2),
        amount: 4,
        combat: true,
    });
    // Damage to an object: DamageDealtBy only when source_controller is Some.
    w.ok(damage(None, DamageTarget::Object(target), 2, false));
    w.ok(damage(Some(P2), DamageTarget::Object(s1), 3, false));
    let card = w.fresh_card();
    w.ok(GameEvent::CreateToken {
        owner: P1,
        card,
        controller: None,
    });
    w.events.history_mut().record_spell_cast(1, P1);
    // Not counted: no-ops (CR 119.9, 120.8, 121.4), a failed event, a rolled-back batch.
    assert_eq!(
        w.exec(GameEvent::LifeChange {
            player: P1,
            delta: 0
        }),
        Ok(None)
    );
    assert_eq!(
        w.exec(damage(Some(P1), DamageTarget::Player(P2), 0, false)),
        Ok(None)
    );
    assert_eq!(w.exec(GameEvent::Draw { player: P1 }), Ok(None));
    assert_eq!(
        w.exec(damage(Some(P1), DamageTarget::Object(gy), 2, false)),
        Err(EventError::NotOnBattlefield(gy))
    );
    let ledger_before = bytes(w.events.history());
    let spare = w.fresh_card();
    assert_eq!(
        w.batch(vec![
            GameEvent::LifeChange {
                player: P1,
                delta: 5,
            },
            damage(Some(P2), DamageTarget::Player(P1), 9, false),
            GameEvent::CreateToken {
                owner: P2,
                card: spare,
                controller: None,
            },
            GameEvent::ZoneChange {
                object: GHOST,
                request: MoveRequest::new(ZoneKind::Exile),
                cause: ZoneChangeCause::Other,
            },
        ]),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(bytes(w.events.history()), ledger_before);

    // ----- turn 2 -----
    w.ok(GameEvent::TurnBegins { player: P2 });
    assert_eq!(w.turn(), 2);
    w.ok(GameEvent::Draw { player: P2 });
    assert_eq!(w.exec(GameEvent::Draw { player: P1 }), Ok(None));
    w.ok(GameEvent::LifeChange {
        player: P1,
        delta: -1,
    });
    w.ok(damage(Some(P2), DamageTarget::Player(P1), 5, true));
    w.ok(damage(Some(P2), DamageTarget::Player(P2), 1, false));
    w.ok(GameEvent::LifeChange {
        player: P2,
        delta: 7,
    });
    let card = w.fresh_card();
    w.ok(GameEvent::CreateToken {
        owner: P2,
        card,
        controller: Some(P1),
    });
    w.events.history_mut().record_spell_cast(2, P2);
    w.events.history_mut().record_spell_cast(2, P2);
    w.events.history_mut().record_spell_cast(2, P1);

    // Life totals agree with the script.
    assert_eq!(w.life(P1), STARTING_LIFE + 3 - 1 - 5);
    assert_eq!(w.life(P2), STARTING_LIFE - 2 - 4 - 1 + 7);

    let h = w.events.history();
    // A few literal spot checks.
    assert_eq!(
        query(h, HistoryStat::LifeLost, HistoryScope::Turn(1), Some(P2)),
        6
    );
    assert_eq!(
        query(h, HistoryStat::LifeLost, HistoryScope::Game, None),
        13
    );
    assert_eq!(
        query(h, HistoryStat::DamageDealtBy, HistoryScope::Game, Some(P2)),
        9
    );
    assert_eq!(
        query(h, HistoryStat::CardsDrawn, HistoryScope::Game, None),
        3
    );
    assert_eq!(
        query(h, HistoryStat::SpellsCast, HistoryScope::Turn(2), None),
        3
    );
    assert_eq!(
        query(h, HistoryStat::TokensCreated, HistoryScope::Game, Some(P1)),
        1
    );

    // Every stat, scope and player against the table.
    let table = expected_table();
    let at = |stat, turn, p| table.get(&(stat, turn, p)).copied().unwrap_or(0);
    for stat in STATS {
        for turn in [1u64, 2, 3] {
            for p in [P1, P2] {
                assert_eq!(
                    query(h, stat, HistoryScope::Turn(turn), Some(p)),
                    at(stat, turn, p),
                    "{stat:?} Turn({turn}) {p:?}"
                );
            }
            assert_eq!(
                query(h, stat, HistoryScope::Turn(turn), None),
                at(stat, turn, P1) + at(stat, turn, P2),
                "{stat:?} Turn({turn}) all"
            );
        }
        for p in [P1, P2] {
            assert_eq!(
                query(h, stat, HistoryScope::Game, Some(p)),
                at(stat, 1, p) + at(stat, 2, p),
                "{stat:?} Game {p:?}"
            );
        }
        let all: i64 = [1u64, 2]
            .iter()
            .flat_map(|&t| [P1, P2].map(|p| at(stat, t, p)))
            .sum();
        assert_eq!(
            query(h, stat, HistoryScope::Game, None),
            all,
            "{stat:?} Game all"
        );
        assert_eq!(query(h, stat, HistoryScope::Turn(0), None), 0);
        assert_eq!(query(h, stat, HistoryScope::Game, Some(NOBODY)), 0);
    }
}

// REQ-DSL-003: the ledger is keyed by the game turn number at execution time.
#[test]
fn ledger_uses_turn_number_at_execution() {
    let mut w = World::new();
    w.game.set_turn_number(7);
    w.ok(GameEvent::LifeChange {
        player: P2,
        delta: 2,
    });
    let h = w.events.history();
    assert_eq!(
        query(h, HistoryStat::LifeGained, HistoryScope::Turn(7), Some(P2)),
        2
    );
    assert_eq!(
        query(h, HistoryStat::LifeGained, HistoryScope::Turn(1), Some(P2)),
        0
    );
}

// REQ-DSL-003: nothing else is recorded (counters, tap, attach, phases, zone changes other than
// draws).
#[test]
fn other_events_are_not_ledgered() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let l = w.create(ZoneKind::Library, P1);
    w.ok(GameEvent::Tap { object: a });
    w.ok(GameEvent::AddCounters {
        target: fineprint_core::events::CounterTarget::Player(P1),
        kind: "test.poisonish".into(),
        amount: 2,
    });
    w.ok(GameEvent::PhaseBegins {
        phase: fineprint_core::events::Phase::Combat,
    });
    // CR 121.5: a library-to-hand move that is not a draw is not a draw.
    w.ok(GameEvent::ZoneChange {
        object: l,
        request: MoveRequest::new(ZoneKind::Hand),
        cause: ZoneChangeCause::Other,
    });
    assert!(w.events.history().is_empty());
}

// REQ-DSL-003: sums saturate instead of overflowing.
#[test]
fn ledger_sums_saturate() {
    let mut w = World::new();
    w.game.player_mut(P1).unwrap().set_life(0);
    w.ok(GameEvent::LifeChange {
        player: P1,
        delta: i64::MAX,
    });
    w.ok(GameEvent::LifeChange {
        player: P1,
        delta: -i64::MAX,
    });
    w.ok(GameEvent::LifeChange {
        player: P1,
        delta: i64::MAX,
    });
    assert_eq!(
        query(
            w.events.history(),
            HistoryStat::LifeGained,
            HistoryScope::Game,
            Some(P1)
        ),
        i64::MAX
    );
    let mut h = History::default();
    for _ in 0..3 {
        h.record_spell_cast(1, P1);
    }
    assert_eq!(
        query(&h, HistoryStat::SpellsCast, HistoryScope::Game, None),
        3
    );
}
