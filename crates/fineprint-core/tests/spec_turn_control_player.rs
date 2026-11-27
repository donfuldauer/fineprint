// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for controlling another player (REQ-TURN-003, REQ-CORE-004; CR 724.1, 724.1a,
//! 724.1b, 724.3, 724.4, 724.5, 724.5a, 724.6, 724.8, 724.9, 800.4a): pending and active
//! effects, routing of the controlled player's choices to the controller, and the extended
//! redaction view. Black-box: written from the work-item brief only.

#[path = "spec_turn_control_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::ids::PlayerId;
use fineprint_core::turn::control::{ControlError, InfoScope, PlayerControl, RoutedRequest};
use support::*;

const ALL: [PlayerId; 3] = [P1, P2, P3];

fn set(ps: &[PlayerId]) -> BTreeSet<PlayerId> {
    ps.iter().copied().collect()
}

/// P1 controls P2 during P2's turn.
fn p1_controls_p2() -> PlayerControl {
    let mut c = PlayerControl::new();
    c.add_control_effect(P2, P1);
    c.begin_turn(P2);
    c
}

/// Asserts that nobody is controlled.
fn nobody_controlled(c: &PlayerControl) {
    for p in ALL {
        assert_eq!(c.controller_of(p), p);
        assert!(!c.is_controlled(p));
    }
}

/// REQ-TURN-003: a fresh `PlayerControl` controls nobody (CR 724.8).
#[test]
fn fresh_control_is_empty() {
    let c = PlayerControl::new();
    nobody_controlled(&c);
    for p in ALL {
        assert_eq!(c.pending_controller(p), None);
        assert_eq!(c.viewing_players(p), set(&[p]));
    }
}

/// REQ-TURN-003, CR 724.1: the effect applies to the controlled player's next turn; before
/// `begin_turn(P2)` it is only pending.
#[test]
fn effect_is_pending_until_the_turn_begins() {
    let mut c = PlayerControl::new();
    c.add_control_effect(P2, P1);
    assert_eq!(c.pending_controller(P2), Some(P1));
    nobody_controlled(&c);
    // Another player's turn does not start it.
    c.begin_turn(P3);
    nobody_controlled(&c);
    assert_eq!(c.pending_controller(P2), Some(P1));
    c.begin_turn(P2);
    assert_eq!(c.controller_of(P2), P1);
    assert!(c.is_controlled(P2));
    assert_eq!(c.pending_controller(P2), None);
    // CR 724.3: only P2 is controlled; P1 is not.
    assert_eq!(c.controller_of(P1), P1);
    assert!(!c.is_controlled(P1));
}

/// REQ-TURN-003, CR 724.1: the effect lasts the whole turn and ends when the next turn begins
/// (whoever's turn that is).
#[test]
fn effect_ends_when_next_turn_begins() {
    for next_active in [P1, P3] {
        let mut c = p1_controls_p2();
        assert_eq!(c.controller_of(P2), P1);
        c.begin_turn(next_active);
        nobody_controlled(&c);
    }
    // Even if P2 takes the next turn (an extra turn): the effect applied to one turn only.
    let mut c = p1_controls_p2();
    c.begin_turn(P2);
    nobody_controlled(&c);
}

/// REQ-TURN-003, CR 724.1a: two effects on the same player: the later one works. Both
/// creation orders.
#[test]
fn later_effect_overwrites() {
    for (first, second) in [(P1, P3), (P3, P1)] {
        let mut c = PlayerControl::new();
        c.add_control_effect(P2, first);
        c.add_control_effect(P2, second);
        assert_eq!(c.pending_controller(P2), Some(second));
        c.begin_turn(P2);
        assert_eq!(c.controller_of(P2), second, "{first:?} then {second:?}");
    }
}

/// REQ-TURN-003, CR 724.1a, CR 724.9: a later self-control effect overwrites (cancels) the
/// pending one; an earlier one is overwritten by a later effect. Both orders.
#[test]
fn self_control_effect_overwrites() {
    let mut c = PlayerControl::new();
    c.add_control_effect(P2, P1);
    c.add_control_effect(P2, P2);
    assert_ne!(c.pending_controller(P2), Some(P1));
    c.begin_turn(P2);
    nobody_controlled(&c);

    let mut c = PlayerControl::new();
    c.add_control_effect(P2, P2);
    c.add_control_effect(P2, P1);
    assert_eq!(c.pending_controller(P2), Some(P1));
    c.begin_turn(P2);
    assert_eq!(c.controller_of(P2), P1);
}

/// REQ-TURN-003, CR 724.1b: the effect waits through a skipped turn (no `begin_turn` for it)
/// and applies to the turn the player actually takes.
#[test]
fn effect_waits_through_skipped_turn() {
    let mut c = PlayerControl::new();
    c.add_control_effect(P2, P1);
    c.begin_turn(P1);
    // P2's turn is skipped: no begin_turn(P2).
    c.begin_turn(P3);
    c.begin_turn(P1);
    nobody_controlled(&c);
    assert_eq!(c.pending_controller(P2), Some(P1));
    c.begin_turn(P2);
    assert_eq!(c.controller_of(P2), P1);
}

/// REQ-TURN-003: other players' pending effects are untouched by a turn beginning.
#[test]
fn other_pending_effects_untouched() {
    let mut c = PlayerControl::new();
    c.add_control_effect(P2, P1);
    c.add_control_effect(P3, P2);
    c.begin_turn(P2);
    assert_eq!(c.controller_of(P2), P1);
    assert_eq!(c.pending_controller(P3), Some(P2));
    c.begin_turn(P3);
    assert_eq!(c.controller_of(P3), P2);
    assert_eq!(c.controller_of(P2), P2);
}

/// REQ-TURN-003, CR 800.4a: when the controller or the controlled player leaves, the effect
/// ends, pending or active.
#[test]
fn player_left_removes_effects() {
    for leaver in [P1, P2] {
        // Active.
        let mut c = p1_controls_p2();
        c.player_left(leaver);
        nobody_controlled(&c);
        // Pending.
        let mut c = PlayerControl::new();
        c.add_control_effect(P2, P1);
        c.player_left(leaver);
        assert_eq!(c.pending_controller(P2), None);
        c.begin_turn(P2);
        nobody_controlled(&c);
    }
    // An unrelated player leaving changes nothing.
    let mut c = p1_controls_p2();
    c.add_control_effect(P3, P1);
    c.player_left(PlayerId::new(9));
    assert_eq!(c.controller_of(P2), P1);
    assert_eq!(c.pending_controller(P3), Some(P1));
    // P1 leaving removes both its active and its pending effect.
    c.player_left(P1);
    nobody_controlled(&c);
    assert_eq!(c.pending_controller(P3), None);
}

/// REQ-TURN-003, CR 724.5, CR 724.4: a request of the controlled player is decided by the
/// controller; the request itself is unchanged; both see it, plus `reveal_to`.
#[test]
fn route_controlled_request() {
    let c = p1_controls_p2();
    for (reveal, hidden) in [(vec![], false), (vec![P3], true), (vec![P1, P3], false)] {
        let req = request(P2, &reveal, hidden);
        let RoutedRequest {
            decider,
            request: routed,
            viewers,
        } = c.route(&req);
        assert_eq!(decider, P1);
        assert_eq!(routed, req);
        assert_eq!(routed.chooser, P2);
        let mut expected = set(&[P1, P2]);
        expected.extend(reveal.iter().copied());
        assert_eq!(viewers, expected);
    }
}

/// REQ-TURN-003, CR 724.8: the controller's own request is decided by the controller; an
/// uncontrolled player's by themselves.
#[test]
fn route_own_and_uncontrolled_requests() {
    let c = p1_controls_p2();
    for p in [P1, P3] {
        let req = request(p, &[], true);
        let r = c.route(&req);
        assert_eq!(r.decider, p);
        assert_eq!(r.request, req);
        assert_eq!(r.viewers, set(&[p]));
    }
    let r = c.route(&request(P3, &[P2], false));
    assert_eq!(r.viewers, set(&[P2, P3]));
}

/// REQ-TURN-003, REQ-CORE-004, CR 724.5: only the controller may answer for the controlled
/// player.
#[test]
fn check_answerer() {
    let c = p1_controls_p2();
    let req = request(P2, &[], false);
    assert_eq!(c.check_answerer(&req, P1), Ok(()));
    let err = c.check_answerer(&req, P2).expect_err("P2 is controlled");
    assert_eq!(
        err,
        ControlError::NotTheDecider {
            chooser: P2,
            expected: P1,
            got: P2
        }
    );
    assert!(!err.to_string().is_empty());
    assert_eq!(
        c.check_answerer(&req, P3),
        Err(ControlError::NotTheDecider {
            chooser: P2,
            expected: P1,
            got: P3
        })
    );
    // CR 724.8: P1's own requests are still P1's.
    let own = request(P1, &[], false);
    assert_eq!(c.check_answerer(&own, P1), Ok(()));
    assert!(c.check_answerer(&own, P2).is_err());
    // Uncontrolled: the chooser.
    let free = PlayerControl::new();
    assert_eq!(free.check_answerer(&req, P2), Ok(()));
    assert_eq!(
        free.check_answerer(&req, P1),
        Err(ControlError::NotTheDecider {
            chooser: P2,
            expected: P2,
            got: P1
        })
    );
}

/// REQ-TURN-003, CR 724.6: a controlled player may still concede; the controller cannot
/// concede for them.
#[test]
fn concession() {
    let c = p1_controls_p2();
    for p in ALL {
        assert!(c.can_concede(p));
        assert!(c.may_concede_for(p, p));
    }
    assert!(!c.may_concede_for(P1, P2));
    assert!(!c.may_concede_for(P2, P1));
    assert!(!c.may_concede_for(P3, P2));
}

/// REQ-TURN-003, CR 724.5a: costs are paid with the controlled player's own resources.
#[test]
fn cost_resources_are_the_controlled_players() {
    let c = p1_controls_p2();
    for p in ALL {
        assert_eq!(c.cost_resource_owner(p), p);
    }
}

/// REQ-TURN-003, CR 724.4: the controller sees the controlled player's in-game information,
/// never their outside-the-game information.
#[test]
fn viewing_players_and_can_view() {
    let c = p1_controls_p2();
    assert_eq!(c.viewing_players(P1), set(&[P1, P2]));
    assert_eq!(c.viewing_players(P2), set(&[P2]));
    assert_eq!(c.viewing_players(P3), set(&[P3]));
    assert!(c.can_view(P1, P2, InfoScope::InGame));
    assert!(!c.can_view(P1, P2, InfoScope::OutsideGame));
    assert!(c.can_view(P2, P2, InfoScope::InGame));
    assert!(c.can_view(P2, P2, InfoScope::OutsideGame));
    assert!(!c.can_view(P3, P2, InfoScope::InGame));
    assert!(!c.can_view(P3, P2, InfoScope::OutsideGame));
    // The controlled player gains no view of the controller.
    assert!(!c.can_view(P2, P1, InfoScope::InGame));
}

/// One control configuration with its expected active (controller, controlled) pair, written as
/// a literal and never derived from `controller_of` (`None`: nobody is controlled).
type Configuration = (String, PlayerControl, Option<(PlayerId, PlayerId)>);

/// The control configurations of the exhaustive redaction test: before activation (pending),
/// during control and after expiry, each with the literal expected active pair. A player
/// controlling themselves is no control at all.
fn configurations() -> Vec<Configuration> {
    let mut v: Vec<Configuration> = vec![("none".to_string(), PlayerControl::new(), None)];
    for controlled in ALL {
        for controller in ALL {
            let active = (controller != controlled).then_some((controller, controlled));
            let mut c = PlayerControl::new();
            c.add_control_effect(controlled, controller);
            v.push((
                format!("{controller:?} pending over {controlled:?}"),
                c.clone(),
                None,
            ));
            c.begin_turn(controlled);
            v.push((
                format!("{controller:?} controls {controlled:?}"),
                c.clone(),
                active,
            ));
            c.begin_turn(controlled);
            v.push((
                format!("{controller:?} controlled {controlled:?}, ended"),
                c,
                None,
            ));
        }
    }
    v
}

/// REQ-TURN-003, REQ-CORE-004 (R12 security test), CR 724.4: over every control
/// configuration of three players and every viewer/subject pair: outside-the-game
/// information is visible only to the subject; in-game information to the subject and the
/// subject's controller only (the expected pair is stored with the configuration);
/// `viewing_players` agrees.
#[test]
fn redaction_exhaustive_three_players() {
    for (name, c, active) in configurations() {
        for v in ALL {
            for s in ALL {
                assert_eq!(
                    c.can_view(v, s, InfoScope::OutsideGame),
                    v == s,
                    "{name}: OutsideGame {v:?} -> {s:?}"
                );
                let expect = v == s || active == Some((v, s));
                assert_eq!(
                    c.can_view(v, s, InfoScope::InGame),
                    expect,
                    "{name}: InGame {v:?} -> {s:?}"
                );
                let views = c.viewing_players(v);
                assert!(views.contains(&v));
                assert_eq!(
                    views.contains(&s),
                    expect,
                    "{name}: viewing_players({v:?}) vs {s:?}"
                );
            }
        }
    }
}

/// REQ-TURN-003, REQ-CORE-004, CR 724.1a, CR 724.1b, CR 800.4a: property test over random
/// effect sequences (seeded LCG): `controller_of` always matches the reference model (the last
/// pending effect created for the player becomes active at `begin_turn`), `route` never
/// changes the request or its chooser, `check_answerer` accepts exactly the decider, and
/// `can_view(v, s, OutsideGame)` holds only when `v == s`.
#[test]
fn control_property_random_sequences() {
    for seed in 0..300u64 {
        let mut rng = Lcg::new(seed);
        let mut c = PlayerControl::new();
        let mut m = ControlModel::default();
        let mut trace = Vec::new();
        for _ in 0..40 {
            let a = ALL[rng.below(3) as usize];
            let b = ALL[rng.below(3) as usize];
            match rng.below(10) {
                0..=3 => {
                    c.add_control_effect(a, b);
                    m.add(a, b);
                    trace.push(format!("add({a:?},{b:?})"));
                }
                4..=8 => {
                    c.begin_turn(a);
                    m.begin_turn(a);
                    trace.push(format!("begin({a:?})"));
                }
                _ => {
                    c.player_left(a);
                    m.left(a);
                    trace.push(format!("left({a:?})"));
                }
            }
            for p in ALL {
                assert_eq!(
                    c.controller_of(p),
                    m.controller_of(p),
                    "seed {seed}: {trace:?}: controller_of({p:?})"
                );
                assert_eq!(c.is_controlled(p), m.controller_of(p) != p);
                let pending = c.pending_controller(p).filter(|x| *x != p);
                assert_eq!(pending, m.pending_other(p), "seed {seed}: {trace:?}");
                let req = request(p, &[], true);
                let r = c.route(&req);
                assert_eq!(r.request, req);
                assert_eq!(r.request.chooser, p);
                assert_eq!(r.decider, m.controller_of(p));
                for ans in ALL {
                    assert_eq!(c.check_answerer(&req, ans).is_ok(), ans == r.decider);
                }
                for v in ALL {
                    assert_eq!(c.can_view(v, p, InfoScope::OutsideGame), v == p);
                }
            }
        }
    }
}
