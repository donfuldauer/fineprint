// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the player-scoped sourceless triggered abilities (REQ-EVENT-002,
//! REQ-ARCH-002; CR 113.8, 726.2, 727.2, 729.1, 702.179d, 603.3b): registration per player and
//! kind, the seven inherent abilities, the pending trigger they produce and their place in APNAP
//! placement. Black-box: written from the work-item brief only.

#[path = "spec_triggers_stack_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::triggers::{
    EventField, PendingTrigger, SourcelessAbility, SourcelessKind, SourcelessTriggers,
    TriggerOrigin,
};
use support::*;

use SourcelessAbility as A;
use SourcelessKind as K;

const ALL: [(SourcelessAbility, SourcelessKind, &str); 7] = [
    (A::MonarchEndStep, K::Monarch, "sourceless.monarch.end_step"),
    (
        A::MonarchCombatDamage,
        K::Monarch,
        "sourceless.monarch.combat_damage",
    ),
    (
        A::InitiativeUpkeep,
        K::Initiative,
        "sourceless.initiative.upkeep",
    ),
    (
        A::InitiativeCombatDamage,
        K::Initiative,
        "sourceless.initiative.combat_damage",
    ),
    (
        A::InitiativeTaken,
        K::Initiative,
        "sourceless.initiative.taken",
    ),
    (A::RadCounters, K::RadCounters, "sourceless.rad_counters"),
    (A::Speed, K::Speed, "sourceless.speed"),
];

/// REQ-EVENT-002; CR 726.2, 727.2, 729.1, 702.179d: the kind and ability id of all seven
/// inherent abilities (two for the monarch, three for the initiative, rad counters, speed).
#[test]
fn req_event_002_sourceless_kind_and_ability_id() {
    for (ability, kind, id) in ALL {
        assert_eq!(ability.kind(), kind, "{ability:?}");
        assert_eq!(ability.ability_id(), id, "{ability:?}");
    }
}

/// REQ-EVENT-002, REQ-ARCH-002; CR 113.8: register / unregister return values, `is_registered`
/// and `players_with` in ascending order; kinds and holders are independent.
#[test]
fn req_event_002_sourceless_registration() {
    let mut st = SourcelessTriggers::new();
    assert_eq!(st, SourcelessTriggers::default());
    assert!(st.players_with(K::Monarch).is_empty());
    assert!(st.register(P3, K::Monarch));
    assert!(!st.register(P3, K::Monarch));
    assert!(st.register(P1, K::Monarch));
    assert!(st.register(P2, K::RadCounters));
    assert!(st.register(P2, K::Speed));
    assert_eq!(st.players_with(K::Monarch), vec![P1, P3]);
    assert_eq!(st.players_with(K::RadCounters), vec![P2]);
    assert!(st.players_with(K::Initiative).is_empty());
    assert!(st.is_registered(P3, K::Monarch));
    assert!(!st.is_registered(P2, K::Monarch));
    assert!(st.unregister(P3, K::Monarch));
    assert!(!st.unregister(P3, K::Monarch));
    assert!(!st.unregister(P1, K::Initiative));
    assert!(!st.is_registered(P3, K::Monarch));
    assert_eq!(st.players_with(K::Monarch), vec![P1]);
    assert!(st.is_registered(P2, K::Speed));
}

/// REQ-EVENT-002, REQ-ARCH-002; CR 113.8, 726.2, 727.2, 702.179d: `fire` gives a pending trigger
/// with no source, origin Sourceless(ability), the ability id, binding "player" = the holder, not
/// optional, part 1, no targets or modes; None for an unregistered holder or kind.
#[test]
fn req_event_002_sourceless_fire_shape() {
    for (ability, kind, id) in ALL {
        let mut st = SourcelessTriggers::new();
        assert_eq!(st.fire(P2, P2, ability), None, "{ability:?} unregistered");
        st.register(P2, kind);
        let mut bindings = BTreeMap::new();
        bindings.insert(s("player"), EventField::Player(P2));
        assert_eq!(
            st.fire(P2, P2, ability),
            Some(PendingTrigger {
                origin: TriggerOrigin::Sourceless(ability),
                controller: P2,
                source: None,
                ability_id: s(id),
                bindings,
                optional: false,
                responds_to_trigger: false,
                targets: vec![],
                modes: None,
            }),
            "{ability:?}"
        );
        assert_eq!(st.fire(P1, P1, ability), None, "{ability:?} other holder");
        st.unregister(P2, kind);
        assert_eq!(
            st.fire(P2, P2, ability),
            None,
            "{ability:?} unregistered again"
        );
    }
}

/// REQ-EVENT-002; CR 113.8: a kind registered for a player does not enable another kind.
#[test]
fn req_event_002_sourceless_other_kind_none() {
    let mut st = SourcelessTriggers::new();
    st.register(P1, K::Monarch);
    for (ability, kind, _) in ALL {
        let fired = st.fire(P1, P1, ability);
        assert_eq!(fired.is_some(), kind == K::Monarch, "{ability:?}");
    }
}

/// REQ-EVENT-002; CR 729.1: the rad-counter ability is controlled by the active player: with
/// holder != controller the passed controller is kept and "player" binds the holder.
#[test]
fn req_event_002_sourceless_rad_counters_active_player_controls() {
    let mut st = SourcelessTriggers::new();
    st.register(P2, K::RadCounters);
    let p = st.fire(P2, P1, A::RadCounters).expect("fires");
    assert_eq!(p.controller, P1);
    assert_eq!(p.bindings.get("player"), Some(&EventField::Player(P2)));
    assert_eq!(p.source, None);
    assert_eq!(st.fire(P1, P1, A::RadCounters), None);
}

/// REQ-EVENT-002; CR 702.179d, 113.8: two holders of the same kind are independent.
#[test]
fn req_event_002_sourceless_two_holders_independent() {
    let mut st = SourcelessTriggers::new();
    st.register(P1, K::Speed);
    st.register(P3, K::Speed);
    let a = st.fire(P1, P1, A::Speed).expect("P1 fires");
    let b = st.fire(P3, P3, A::Speed).expect("P3 fires");
    assert_eq!(a.controller, P1);
    assert_eq!(b.controller, P3);
    assert_eq!(a.bindings.get("player"), Some(&EventField::Player(P1)));
    assert_eq!(b.bindings.get("player"), Some(&EventField::Player(P3)));
    assert!(st.unregister(P1, K::Speed));
    assert_eq!(st.fire(P1, P1, A::Speed), None);
    assert!(st.fire(P3, P3, A::Speed).is_some());
}

/// REQ-EVENT-002, REQ-ARCH-002; CR 726.2, 603.3b, 101.4: a sourceless trigger takes part in
/// APNAP placement under its controller, described as "player <controller> ability <id>".
#[test]
fn req_event_002_sourceless_in_apnap_placement() {
    let mut st = SourcelessTriggers::new();
    st.register(P2, K::Monarch);
    st.register(P2, K::RadCounters);
    let monarch = st.fire(P2, P2, A::MonarchEndStep).expect("fires");
    let rad = st.fire(P2, P1, A::RadCounters).expect("fires");
    let waiting = vec![
        pt(20, P2, "test.trigger_p2"),
        monarch,
        rad,
        pt(10, P1, "test.trigger_p1"),
    ];
    for o in [[0u32, 1], [1, 0]] {
        let d = drive(
            &ScriptEnv::new(),
            &[P1, P2],
            &waiting,
            &[order(&o), order(&o)],
        );
        assert_eq!(d.requests.len(), 2);
        assert_eq!(d.requests[0].chooser, P1);
        assert_eq!(
            order_payload(&d.requests[0]),
            vec![
                desc_player(P1, "sourceless.rad_counters"),
                desc_obj(10, "test.trigger_p1"),
            ]
        );
        assert_eq!(d.requests[1].chooser, P2);
        assert_eq!(
            order_payload(&d.requests[1]),
            vec![
                desc_obj(20, "test.trigger_p2"),
                desc_player(P2, "sourceless.monarch.end_step"),
            ]
        );
        assert_eq!(d.requests[0].reason.source, None);
        let p1: Vec<&str> = vec!["sourceless.rad_counters", "test.trigger_p1"];
        let p2: Vec<&str> = vec!["test.trigger_p2", "sourceless.monarch.end_step"];
        let mut want: Vec<String> = o.iter().map(|&i| s(p1[i as usize])).collect();
        want.extend(o.iter().map(|&i| s(p2[i as usize])));
        assert_eq!(stacked_ids(d.placement()), want);
        assert!(
            d.placement()
                .stacked
                .iter()
                .any(|t| t.trigger.source.is_none())
        );
    }
}
