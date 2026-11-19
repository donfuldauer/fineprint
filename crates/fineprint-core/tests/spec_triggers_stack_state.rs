// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for state triggers (REQ-EVENT-002; CR 603.8, 603.4, 113.2c): `StateTriggers`
//! triggers once while the condition holds, does not retrigger while the instance is
//! outstanding, re-checks after it leaves the stack, and honours the intervening "if" at trigger
//! time. Black-box: written from the work-item brief only.

#[path = "spec_triggers_stack_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::triggers::{StateTriggers, StateView, WaitingTrigger};
use fineprint_dsl::ability::{EventKind, StepId, Trigger};
use fineprint_dsl::expr::{PlayerGroup, PlayerSelectorAST};
use support::*;

fn summary(ts: &[WaitingTrigger]) -> Vec<(u64, String)> {
    ts.iter()
        .map(|t| (t.source.get(), t.ability_id.clone()))
        .collect()
}

/// REQ-EVENT-002; CR 603.8: a state trigger triggers as soon as its condition holds, with no
/// look-back, event indices or bindings, and its controller.
#[test]
fn req_event_002_state_trigger_fires_once() {
    let env = CondEnv::new()
        .with_post(reg(4, P2, state_ability("test.trigger_state", "cond")))
        .cond_true("cond");
    let mut st = StateTriggers::new();
    let out = st.check(&env);
    assert_eq!(
        out,
        vec![WaitingTrigger {
            source: oid(4),
            controller: P2,
            ability_id: s("test.trigger_state"),
            look_back: None,
            event_indices: vec![],
            bindings: BTreeMap::new(),
        }]
    );
    assert!(st.is_outstanding(oid(4), "test.trigger_state"));
    // The condition is checked in the post-event view with empty bindings.
    let calls = env.cond_calls.borrow();
    assert!(!calls.is_empty());
    for c in calls.iter() {
        assert_eq!(c.0, StateView::PostEvent);
        assert!(c.2.is_empty());
    }
}

/// REQ-EVENT-002; CR 603.8: a state trigger does not retrigger while on the stack (outstanding),
/// however often the game checks; after it leaves the stack it triggers again if the condition
/// still holds.
#[test]
fn req_event_002_state_trigger_no_retrigger_while_on_stack() {
    let env = CondEnv::new()
        .with_post(reg(4, P1, state_ability("test.trigger_state", "cond")))
        .cond_true("cond");
    let mut st = StateTriggers::new();
    assert_eq!(st.check(&env).len(), 1);
    for _ in 0..3 {
        assert!(st.check(&env).is_empty());
        assert!(st.is_outstanding(oid(4), "test.trigger_state"));
    }
    assert!(st.instance_left_stack(oid(4), "test.trigger_state"));
    assert!(!st.is_outstanding(oid(4), "test.trigger_state"));
    assert!(!st.instance_left_stack(oid(4), "test.trigger_state"));
    assert_eq!(summary(&st.check(&env)), vec![(4, s("test.trigger_state"))]);
    assert!(st.is_outstanding(oid(4), "test.trigger_state"));
    assert!(st.check(&env).is_empty());
}

/// REQ-EVENT-002; CR 603.8: after leaving the stack, a false condition does not retrigger (and
/// the instance is not outstanding); it triggers once the condition holds again.
#[test]
fn req_event_002_state_trigger_recheck_condition_false() {
    let mut env = CondEnv::new()
        .with_post(reg(4, P1, state_ability("test.trigger_state", "cond")))
        .cond_true("cond");
    let mut st = StateTriggers::new();
    assert_eq!(st.check(&env).len(), 1);
    env.set("cond", false);
    assert!(st.instance_left_stack(oid(4), "test.trigger_state"));
    assert!(st.check(&env).is_empty());
    assert!(!st.is_outstanding(oid(4), "test.trigger_state"));
    env.set("cond", true);
    assert_eq!(st.check(&env).len(), 1);
}

/// REQ-EVENT-002; CR 603.8: the object must still be in the same zone (still listed by the
/// environment) for the ability to trigger again.
#[test]
fn req_event_002_state_trigger_object_gone() {
    let mut env = CondEnv::new()
        .with_post(reg(4, P1, state_ability("test.trigger_state", "cond")))
        .cond_true("cond");
    let mut st = StateTriggers::new();
    assert_eq!(st.check(&env).len(), 1);
    env.post.clear();
    assert!(st.instance_left_stack(oid(4), "test.trigger_state"));
    assert!(st.check(&env).is_empty());
    // A registration listed only in the pre-event view does not count either.
    env.pre
        .push(reg(4, P1, state_ability("test.trigger_state", "cond")));
    assert!(st.check(&env).is_empty());
    assert!(!st.is_outstanding(oid(4), "test.trigger_state"));
}

/// REQ-EVENT-002; CR 603.8: `forget` drops an outstanding entry (true iff present); the next
/// check may trigger it again.
#[test]
fn req_event_002_state_trigger_forget() {
    let env = CondEnv::new()
        .with_post(reg(4, P1, state_ability("test.trigger_state", "cond")))
        .cond_true("cond");
    let mut st = StateTriggers::new();
    assert!(!st.forget(oid(4), "test.trigger_state"));
    assert_eq!(st.check(&env).len(), 1);
    assert!(st.forget(oid(4), "test.trigger_state"));
    assert!(!st.is_outstanding(oid(4), "test.trigger_state"));
    assert!(!st.forget(oid(4), "test.trigger_state"));
    assert!(!st.instance_left_stack(oid(4), "test.trigger_state"));
    assert_eq!(st.check(&env).len(), 1);
}

/// REQ-EVENT-002; CR 603.8, 113.2c: instances on other sources or with other ability ids are
/// independent; output is ordered by (source, ability id).
#[test]
fn req_event_002_state_trigger_instances_independent_and_ordered() {
    let env = CondEnv::new()
        .with_post(reg(5, P1, state_ability("test.trigger_b", "cond")))
        .with_post(reg(2, P2, state_ability("test.trigger_z", "cond")))
        .with_post(reg(5, P1, state_ability("test.trigger_a", "cond")))
        .with_post(reg(2, P2, state_ability("test.trigger_a", "cond")))
        .cond_true("cond");
    let mut st = StateTriggers::new();
    assert_eq!(
        summary(&st.check(&env)),
        vec![
            (2, s("test.trigger_a")),
            (2, s("test.trigger_z")),
            (5, s("test.trigger_a")),
            (5, s("test.trigger_b")),
        ]
    );
    assert!(st.instance_left_stack(oid(5), "test.trigger_a"));
    assert_eq!(summary(&st.check(&env)), vec![(5, s("test.trigger_a"))]);
    assert!(st.is_outstanding(oid(2), "test.trigger_a"));
    assert!(st.instance_left_stack(oid(2), "test.trigger_a"));
    assert!(st.is_outstanding(oid(5), "test.trigger_a"));
    assert_eq!(summary(&st.check(&env)), vec![(2, s("test.trigger_a"))]);
}

/// REQ-EVENT-002; CR 603.8: each instance's own condition decides; only State triggers count.
#[test]
fn req_event_002_state_trigger_ignores_other_triggers() {
    let env = CondEnv::new()
        .with_post(reg(1, P1, state_ability("test.trigger_s1", "yes")))
        .with_post(reg(1, P1, state_ability("test.trigger_s2", "no")))
        .with_post(reg(1, P1, ev_ability("test.trigger_ev", EventKind::Dies)))
        .with_post(reg(
            1,
            P1,
            ability_with(
                "test.trigger_step",
                Trigger::PhaseStep {
                    step: StepId::Upkeep,
                    whose: PlayerSelectorAST::Players(PlayerGroup::AllPlayers),
                },
            ),
        ))
        .cond_true("yes");
    let mut st = StateTriggers::new();
    assert_eq!(summary(&st.check(&env)), vec![(1, s("test.trigger_s1"))]);
    assert!(!st.is_outstanding(oid(1), "test.trigger_s2"));
    assert!(!st.is_outstanding(oid(1), "test.trigger_ev"));
    assert!(!st.is_outstanding(oid(1), "test.trigger_step"));
}

/// REQ-EVENT-002; CR 603.4, 603.8: a false intervening "if" means no trigger and no outstanding
/// entry; a later check with it true triggers.
#[test]
fn req_event_002_state_trigger_intervening_if() {
    let mut env = CondEnv::new()
        .with_post(reg(
            3,
            P1,
            with_if(state_ability("test.trigger_if", "cond"), "if"),
        ))
        .cond_true("cond");
    let mut st = StateTriggers::new();
    assert!(st.check(&env).is_empty());
    assert!(!st.is_outstanding(oid(3), "test.trigger_if"));
    assert!(st.check(&env).is_empty());
    env.set("if", true);
    assert_eq!(summary(&st.check(&env)), vec![(3, s("test.trigger_if"))]);
    assert!(st.is_outstanding(oid(3), "test.trigger_if"));
    // While outstanding, nothing more, even with both true.
    assert!(st.check(&env).is_empty());
    // With the condition false the intervening if does not matter.
    env.set("cond", false);
    assert!(st.instance_left_stack(oid(3), "test.trigger_if"));
    assert!(st.check(&env).is_empty());
    assert!(!st.is_outstanding(oid(3), "test.trigger_if"));
}

/// REQ-EVENT-002; CR 603.8: a fresh tracker equals the default one; nothing is outstanding.
#[test]
fn req_event_002_state_trigger_new_is_empty() {
    let st = StateTriggers::new();
    assert_eq!(st, StateTriggers::default());
    assert!(!st.is_outstanding(oid(1), "test.trigger_state"));
    let env = CondEnv::new().with_post(reg(4, P1, state_ability("test.trigger_state", "cond")));
    let mut st = StateTriggers::new();
    assert!(st.check(&env).is_empty());
    assert_eq!(st, StateTriggers::new());
}
