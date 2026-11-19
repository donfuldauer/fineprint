// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the Damage and LifeChange events (REQ-EVENT-001, REQ-DSL-003; CR 119.2,
//! 119.3, 119.6, 119.9, 120.3a, 120.3e, 120.8, 514.2). Black-box: written from the work-item
//! brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::events::{
    DamageTarget, EventError, GameEvent, HistoryQuery, HistoryScope, HistoryStat, Outcome,
};
use fineprint_core::zones::ZoneKind;
use support::*;

fn dmg(target: DamageTarget, amount: i64) -> GameEvent {
    GameEvent::Damage {
        source: None,
        source_controller: None,
        target,
        amount,
        combat: false,
    }
}

fn life(player: PlayerId, delta: i64) -> GameEvent {
    GameEvent::LifeChange { player, delta }
}

fn q(w: &World, stat: HistoryStat, p: PlayerId) -> i64 {
    w.events.history().query(&HistoryQuery {
        stat,
        scope: HistoryScope::Game,
        player: Some(p),
    })
}

fn creature(w: &mut World) -> ObjectId {
    w.create(ZoneKind::Battlefield, P1)
}

// REQ-EVENT-001, CR 120.8: 0 damage is not dealt at all: no event, nothing changes.
#[test]
fn zero_damage_is_no_event() {
    let mut w = World::new();
    let a = creature(&mut w);
    let before = w.snap();
    assert_eq!(w.exec(dmg(DamageTarget::Player(P2), 0)), Ok(None));
    assert_eq!(w.exec(dmg(DamageTarget::Object(a), 0)), Ok(None));
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 119.9, 119.10: a 0 life change is no event (loss by analogy).
#[test]
fn zero_life_change_is_no_event() {
    let mut w = World::new();
    let before = w.snap();
    assert_eq!(w.exec(life(P1, 0)), Ok(None));
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001: negative damage is InvalidAmount and changes nothing.
#[test]
fn negative_damage_is_invalid() {
    let mut w = World::new();
    let a = creature(&mut w);
    let before = w.snap();
    assert_eq!(
        w.exec(dmg(DamageTarget::Player(P1), -1)),
        Err(EventError::InvalidAmount(-1))
    );
    assert_eq!(
        w.exec(dmg(DamageTarget::Object(a), -5)),
        Err(EventError::InvalidAmount(-5))
    );
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, REQ-DSL-003, CR 119.2, 120.3a: damage to a player lowers life and records
// LifeLost and DamageDealtToPlayer (and DamageDealtBy for the source's controller).
#[test]
fn player_damage_lowers_life_and_ledgers() {
    let mut w = World::new();
    let src = creature(&mut w);
    let done = w.ok(GameEvent::Damage {
        source: Some(src),
        source_controller: Some(P1),
        target: DamageTarget::Player(P2),
        amount: 3,
        combat: true,
    });
    assert_eq!(done.outcome, Outcome::Applied);
    assert_eq!(w.life(P2), STARTING_LIFE - 3);
    assert_eq!(w.life(P1), STARTING_LIFE);
    assert_eq!(q(&w, HistoryStat::LifeLost, P2), 3);
    assert_eq!(q(&w, HistoryStat::DamageDealtToPlayer, P2), 3);
    assert_eq!(q(&w, HistoryStat::DamageDealtBy, P1), 3);
    assert_eq!(q(&w, HistoryStat::LifeGained, P2), 0);
    assert_eq!(q(&w, HistoryStat::LifeLost, P1), 0);
}

// REQ-EVENT-001, CR 120.3e: damage to a permanent is marked on it; Outcome::Applied.
#[test]
fn object_damage_is_marked() {
    let mut w = World::new();
    let a = creature(&mut w);
    let b = creature(&mut w);
    assert_eq!(w.events.markers().damage(a), 0);
    assert_eq!(
        w.ok(dmg(DamageTarget::Object(a), 2)).outcome,
        Outcome::Applied
    );
    w.ok(dmg(DamageTarget::Object(a), 3));
    assert_eq!(w.events.markers().damage(a), 5);
    assert_eq!(w.events.markers().damage(b), 0);
    assert_eq!(w.life(P1), STARTING_LIFE);
}

// REQ-EVENT-001, CR 120.3e: an object target must be a permanent (NotOnBattlefield).
#[test]
fn object_damage_requires_a_permanent() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P1);
    let h = w.create(ZoneKind::Hand, P2);
    let before = w.snap();
    assert_eq!(
        w.exec(dmg(DamageTarget::Object(g), 1)),
        Err(EventError::NotOnBattlefield(g))
    );
    assert_eq!(
        w.exec(dmg(DamageTarget::Object(h), 1)),
        Err(EventError::NotOnBattlefield(h))
    );
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 514.2, 120.6: clear_all_damage removes all marked damage.
#[test]
fn clear_all_damage_removes_marked_damage() {
    let mut w = World::new();
    let a = creature(&mut w);
    let b = creature(&mut w);
    w.ok(dmg(DamageTarget::Object(a), 2));
    w.ok(dmg(DamageTarget::Object(b), 4));
    w.events.markers_mut().clear_all_damage();
    assert_eq!(w.events.markers().damage(a), 0);
    assert_eq!(w.events.markers().damage(b), 0);
}

// REQ-EVENT-001, CR 119.3: life gain and loss adjust the total; ledgered.
#[test]
fn life_change_adjusts_life_and_ledgers() {
    let mut w = World::new();
    assert_eq!(w.ok(life(P1, 4)).outcome, Outcome::Applied);
    w.ok(life(P2, -6));
    assert_eq!(w.life(P1), STARTING_LIFE + 4);
    assert_eq!(w.life(P2), STARTING_LIFE - 6);
    assert_eq!(q(&w, HistoryStat::LifeGained, P1), 4);
    assert_eq!(q(&w, HistoryStat::LifeLost, P2), 6);
    assert_eq!(q(&w, HistoryStat::LifeLost, P1), 0);
    assert_eq!(q(&w, HistoryStat::DamageDealtToPlayer, P2), 0);
}

// REQ-EVENT-001, CR 119.6: a life total below 0 is allowed; losing is a later SBA.
#[test]
fn life_may_go_below_zero() {
    let mut w = World::new();
    w.ok(life(P1, -(STARTING_LIFE + 5)));
    assert_eq!(w.life(P1), -5);
    w.ok(dmg(DamageTarget::Player(P1), 3));
    assert_eq!(w.life(P1), -8);
}

// REQ-EVENT-001: life overflow is Overflow with nothing changed.
#[test]
fn life_overflow_fails_cleanly() {
    let mut w = World::new();
    w.game.player_mut(P1).unwrap().set_life(i64::MAX);
    w.game.player_mut(P2).unwrap().set_life(i64::MIN + 1);
    let before = w.snap();
    assert_eq!(w.exec(life(P1, 1)), Err(EventError::Overflow));
    assert_eq!(w.exec(life(P2, -2)), Err(EventError::Overflow));
    assert_eq!(
        w.exec(dmg(DamageTarget::Player(P2), 2)),
        Err(EventError::Overflow)
    );
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 120.3e: marked damage overflow is Overflow with nothing changed.
#[test]
fn marked_damage_overflow_fails_cleanly() {
    let mut w = World::new();
    let a = creature(&mut w);
    w.ok(dmg(DamageTarget::Object(a), i64::MAX));
    let before = w.snap();
    assert_eq!(
        w.exec(dmg(DamageTarget::Object(a), 1)),
        Err(EventError::Overflow)
    );
    assert_eq!(w.snap(), before);
    assert_eq!(w.events.markers().damage(a), i64::MAX);
}

// REQ-EVENT-001: unknown players are UnknownPlayer and nothing changes.
#[test]
fn unknown_player_fails_cleanly() {
    let mut w = World::new();
    let before = w.snap();
    assert_eq!(
        w.exec(life(NOBODY, 2)),
        Err(EventError::UnknownPlayer(NOBODY))
    );
    assert_eq!(
        w.exec(dmg(DamageTarget::Player(NOBODY), 2)),
        Err(EventError::UnknownPlayer(NOBODY))
    );
    assert_eq!(w.snap(), before);
}
