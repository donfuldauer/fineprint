// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for putting triggered abilities on the stack (REQ-EVENT-002, REQ-ARCH-002;
//! CR 101.4, 603.3, 603.3b, 603.3c, 603.3d, 603.5, 700.2b, 700.2c): APNAP order, the two parts
//! of CR 603.3b, ordering requests, modes then targets, removal for no legal choice, request
//! shapes and replay. Black-box: written from the work-item brief only.

#[path = "spec_triggers_stack_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::PlayerId;

use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceResponse,
    ChoiceSession, ChoiceType, PendingChoiceRequest, Status, TargetSlot, ValidatorContract,
    ValidatorRule,
};
use fineprint_core::triggers::{
    EventField, PendingTrigger, RemovedReason, TriggerOrigin, TriggerPlacement, place_triggers,
};
use fineprint_dsl::ability::{EventKind, FilterValue, StepId, Trigger};
use fineprint_dsl::expr::{PlayerGroup, PlayerSelectorAST};
use support::*;

/// `ids` permuted by `o` (o[0] goes first).
fn apply(o: &[u32], ids: &[&str]) -> Vec<String> {
    o.iter().map(|&i| ids[i as usize].to_string()).collect()
}

/// Asserts the shape of an ORDER_SIMULTANEOUS_TRIGGERS request (CR 603.3b).
fn assert_order_req(r: &PendingChoiceRequest, chooser: PlayerId, descs: &[String]) {
    assert_eq!(r.choice_type, ChoiceType::OrderSimultaneousTriggers);
    assert_eq!(r.chooser, chooser);
    assert_eq!(r.reason.cr, "CR 603.3b");
    assert_eq!(r.reason.source, None);
    assert_eq!(order_payload(r), descs.to_vec());
    let n = descs.len() as u64;
    assert_eq!(r.min, n);
    assert_eq!(r.max, n);
    let identity: Vec<u32> = (0..descs.len() as u32).collect();
    assert_eq!(r.default_on_timeout, order(&identity));
    assert!(r.validate(&r.default_on_timeout).is_ok());
}

const IDENT: [u32; 2] = [0, 1];
const REV: [u32; 2] = [1, 0];

// =============================================================================================
// 1. APNAP (CR 101.4, 603.3b)
// =============================================================================================

/// REQ-ARCH-002, REQ-EVENT-002; CR 603.3b, 101.4: two players with two part-1 triggers each,
/// every combination of answers: the active player's two (in chosen order) go on the stack
/// first, then the nonactive player's two (in chosen order); the active player is asked first.
#[test]
fn req_event_002_apnap_two_players_all_four_orders() {
    let waiting = vec![
        pt(20, P2, "test.trigger_p2a"),
        pt(10, P1, "test.trigger_p1a"),
        pt(11, P1, "test.trigger_p1b"),
        pt(21, P2, "test.trigger_p2b"),
    ];
    let env = ScriptEnv::new();
    for o1 in [IDENT, REV] {
        for o2 in [IDENT, REV] {
            let d = drive(&env, &[P1, P2], &waiting, &[order(&o1), order(&o2)]);
            assert_eq!(d.requests.len(), 2);
            assert_order_req(
                &d.requests[0],
                P1,
                &[
                    desc_obj(10, "test.trigger_p1a"),
                    desc_obj(11, "test.trigger_p1b"),
                ],
            );
            assert_order_req(
                &d.requests[1],
                P2,
                &[
                    desc_obj(20, "test.trigger_p2a"),
                    desc_obj(21, "test.trigger_p2b"),
                ],
            );
            let mut want = apply(&o1, &["test.trigger_p1a", "test.trigger_p1b"]);
            want.extend(apply(&o2, &["test.trigger_p2a", "test.trigger_p2b"]));
            assert_eq!(stacked_ids(d.placement()), want, "o1={o1:?} o2={o2:?}");
            assert!(d.placement().removed.is_empty());
            // The pending triggers are carried unchanged.
            for st in &d.placement().stacked {
                let orig = waiting
                    .iter()
                    .find(|w| w.ability_id == st.trigger.ability_id)
                    .expect("input trigger");
                assert_eq!(&st.trigger, orig);
                assert!(st.modes.is_empty());
                assert!(st.targets.is_empty());
            }
        }
    }
}

/// REQ-ARCH-002, REQ-EVENT-002; CR 101.4, 603.3b: APNAP order is the `apnap` argument, not id
/// order: with the higher-id player active, that player is asked first and stacks first.
#[test]
fn req_event_002_apnap_active_player_higher_id() {
    let waiting = vec![
        pt(10, P1, "test.trigger_p1a"),
        pt(11, P1, "test.trigger_p1b"),
        pt(20, P2, "test.trigger_p2a"),
        pt(21, P2, "test.trigger_p2b"),
    ];
    let env = ScriptEnv::new();
    for o1 in [IDENT, REV] {
        for o2 in [IDENT, REV] {
            // o2 answers the active player P2, o1 the nonactive P1.
            let d = drive(&env, &[P2, P1], &waiting, &[order(&o2), order(&o1)]);
            assert_order_req(
                &d.requests[0],
                P2,
                &[
                    desc_obj(20, "test.trigger_p2a"),
                    desc_obj(21, "test.trigger_p2b"),
                ],
            );
            assert_order_req(
                &d.requests[1],
                P1,
                &[
                    desc_obj(10, "test.trigger_p1a"),
                    desc_obj(11, "test.trigger_p1b"),
                ],
            );
            let mut want = apply(&o2, &["test.trigger_p2a", "test.trigger_p2b"]);
            want.extend(apply(&o1, &["test.trigger_p1a", "test.trigger_p1b"]));
            assert_eq!(stacked_ids(d.placement()), want);
        }
    }
}

/// REQ-ARCH-002, REQ-EVENT-002; CR 101.4, 603.3b: three players in APNAP order P2, P3, P1 are
/// asked in that order and their groups stack in that order, each in its chosen order.
#[test]
fn req_event_002_apnap_three_players() {
    let waiting = vec![
        pt(10, P1, "test.trigger_p1a"),
        pt(30, P3, "test.trigger_p3a"),
        pt(20, P2, "test.trigger_p2a"),
        pt(11, P1, "test.trigger_p1b"),
        pt(31, P3, "test.trigger_p3b"),
        pt(21, P2, "test.trigger_p2b"),
    ];
    let env = ScriptEnv::new();
    for (a, b, c) in [(IDENT, REV, IDENT), (REV, IDENT, REV), (REV, REV, REV)] {
        let d = drive(
            &env,
            &[P2, P3, P1],
            &waiting,
            &[order(&a), order(&b), order(&c)],
        );
        let choosers: Vec<_> = d.requests.iter().map(|r| r.chooser).collect();
        assert_eq!(choosers, vec![P2, P3, P1]);
        assert_order_req(
            &d.requests[1],
            P3,
            &[
                desc_obj(30, "test.trigger_p3a"),
                desc_obj(31, "test.trigger_p3b"),
            ],
        );
        let mut want = apply(&a, &["test.trigger_p2a", "test.trigger_p2b"]);
        want.extend(apply(&b, &["test.trigger_p3a", "test.trigger_p3b"]));
        want.extend(apply(&c, &["test.trigger_p1a", "test.trigger_p1b"]));
        assert_eq!(stacked_ids(d.placement()), want);
    }
}

/// REQ-EVENT-002; CR 603.3b: no ORDER request for a player with 0 or 1 triggers in a part; a
/// player with none is skipped, and the single trigger still stacks in its APNAP slot.
#[test]
fn req_event_002_no_order_ask_for_single_or_no_trigger() {
    let waiting = vec![
        pt(20, P2, "test.trigger_p2a"),
        pt(10, P1, "test.trigger_p1a"),
        pt(21, P2, "test.trigger_p2b"),
    ];
    let env = ScriptEnv::new();
    let d = drive(&env, &[P1, P2, P3], &waiting, &[order(&REV)]);
    assert_eq!(d.requests.len(), 1);
    assert_order_req(
        &d.requests[0],
        P2,
        &[
            desc_obj(20, "test.trigger_p2a"),
            desc_obj(21, "test.trigger_p2b"),
        ],
    );
    assert_eq!(
        stacked_ids(d.placement()),
        vec![
            s("test.trigger_p1a"),
            s("test.trigger_p2b"),
            s("test.trigger_p2a")
        ]
    );

    // Only singletons: nothing is asked at all; APNAP order still decides.
    let waiting = vec![
        pt(20, P2, "test.trigger_p2a"),
        pt(10, P1, "test.trigger_p1a"),
    ];
    let d = drive(&env, &[P1, P2], &waiting, &[]);
    assert!(d.requests.is_empty());
    assert_eq!(
        stacked_ids(d.placement()),
        vec![s("test.trigger_p1a"), s("test.trigger_p2a")]
    );
    let d = drive(&env, &[P2, P1], &waiting, &[]);
    assert_eq!(
        stacked_ids(d.placement()),
        vec![s("test.trigger_p2a"), s("test.trigger_p1a")]
    );

    // Nothing waiting: an empty placement and no request.
    let d = drive(&env, &[P1, P2], &[], &[]);
    assert!(d.requests.is_empty());
    assert_eq!(d.placement(), &TriggerPlacement::default());
}

/// REQ-EVENT-002; CR 603.3b: a controller missing from `apnap` is a procedure error.
#[test]
fn req_event_002_controller_not_in_apnap_is_an_error() {
    let env = ScriptEnv::new();
    let waiting = vec![pt(10, P1, "test.trigger_a"), pt(20, P2, "test.trigger_b")];
    match start_placement(&env, &[P1], &waiting) {
        Err(ChoiceError::Procedure(_)) => {}
        other => panic!("expected a procedure error, got {other:?}"),
    }
    match start_placement(&env, &[], &[pt(10, P1, "test.trigger_a")]) {
        Err(ChoiceError::Procedure(_)) => {}
        other => panic!("expected a procedure error, got {other:?}"),
    }
}

// =============================================================================================
// 2. the two parts of CR 603.3b
// =============================================================================================

/// REQ-EVENT-002, REQ-ARCH-002; CR 603.3b: every part-1 trigger precedes every part-2 trigger;
/// a player whose two triggers split 1+1 across the parts is asked nothing; a player with two
/// in each part is asked once per part.
#[test]
fn req_event_002_two_parts_split_player_not_asked() {
    let waiting = vec![
        pt2(23, P2, "test.trigger_y2a"),
        pt(10, P1, "test.trigger_x1"),
        pt(20, P2, "test.trigger_x2a"),
        pt2(13, P1, "test.trigger_y1"),
        pt2(24, P2, "test.trigger_y2b"),
        pt(21, P2, "test.trigger_x2b"),
    ];
    let env = ScriptEnv::new();
    for o1 in [IDENT, REV] {
        for o2 in [IDENT, REV] {
            let d = drive(&env, &[P1, P2], &waiting, &[order(&o1), order(&o2)]);
            assert_eq!(d.requests.len(), 2);
            assert_order_req(
                &d.requests[0],
                P2,
                &[
                    desc_obj(20, "test.trigger_x2a"),
                    desc_obj(21, "test.trigger_x2b"),
                ],
            );
            assert_order_req(
                &d.requests[1],
                P2,
                &[
                    desc_obj(23, "test.trigger_y2a"),
                    desc_obj(24, "test.trigger_y2b"),
                ],
            );
            let mut want = vec![s("test.trigger_x1")];
            want.extend(apply(&o1, &["test.trigger_x2a", "test.trigger_x2b"]));
            want.push(s("test.trigger_y1"));
            want.extend(apply(&o2, &["test.trigger_y2a", "test.trigger_y2b"]));
            assert_eq!(stacked_ids(d.placement()), want);
        }
    }
}

/// REQ-EVENT-002, REQ-ARCH-002; CR 603.3b, 101.4: with two players each holding two triggers
/// in each part, ORDER is asked part 1 (APNAP) then part 2 (APNAP).
#[test]
fn req_event_002_two_parts_order_asks_per_player_per_part() {
    let waiting = vec![
        pt2(13, P1, "test.trigger_y1a"),
        pt(10, P1, "test.trigger_x1a"),
        pt2(23, P2, "test.trigger_y2a"),
        pt(20, P2, "test.trigger_x2a"),
        pt(11, P1, "test.trigger_x1b"),
        pt2(14, P1, "test.trigger_y1b"),
        pt(21, P2, "test.trigger_x2b"),
        pt2(24, P2, "test.trigger_y2b"),
    ];
    let env = ScriptEnv::new();
    let script = [order(&REV), order(&IDENT), order(&IDENT), order(&REV)];
    let d = drive(&env, &[P2, P1], &waiting, &script);
    assert_eq!(d.requests.len(), 4);
    assert_order_req(
        &d.requests[0],
        P2,
        &[
            desc_obj(20, "test.trigger_x2a"),
            desc_obj(21, "test.trigger_x2b"),
        ],
    );
    assert_order_req(
        &d.requests[1],
        P1,
        &[
            desc_obj(10, "test.trigger_x1a"),
            desc_obj(11, "test.trigger_x1b"),
        ],
    );
    assert_order_req(
        &d.requests[2],
        P2,
        &[
            desc_obj(23, "test.trigger_y2a"),
            desc_obj(24, "test.trigger_y2b"),
        ],
    );
    assert_order_req(
        &d.requests[3],
        P1,
        &[
            desc_obj(13, "test.trigger_y1a"),
            desc_obj(14, "test.trigger_y1b"),
        ],
    );
    assert_eq!(
        stacked_ids(d.placement()),
        vec![
            s("test.trigger_x2b"),
            s("test.trigger_x2a"),
            s("test.trigger_x1a"),
            s("test.trigger_x1b"),
            s("test.trigger_y2a"),
            s("test.trigger_y2b"),
            s("test.trigger_y1b"),
            s("test.trigger_y1a"),
        ]
    );
}

/// REQ-EVENT-002; CR 603.3b: only part-2 triggers: the player with two is asked once.
#[test]
fn req_event_002_two_parts_only_part_two() {
    let waiting = vec![
        pt2(10, P1, "test.trigger_y1a"),
        pt2(11, P1, "test.trigger_y1b"),
        pt2(20, P2, "test.trigger_y2"),
    ];
    let env = ScriptEnv::new();
    let d = drive(&env, &[P1, P2], &waiting, &[order(&REV)]);
    assert_eq!(d.requests.len(), 1);
    assert_eq!(d.requests[0].chooser, P1);
    assert_eq!(
        stacked_ids(d.placement()),
        vec![
            s("test.trigger_y1b"),
            s("test.trigger_y1a"),
            s("test.trigger_y2")
        ]
    );
}

// =============================================================================================
// 3. request shapes and rejected answers
// =============================================================================================

/// REQ-EVENT-002; CR 603.3b: the ORDER request rejects answers that are not a permutation of
/// the triggers, of the wrong response type or out of range; its default is creation order.
#[test]
fn req_event_002_order_request_rejects_invalid_answers() {
    let env = ScriptEnv::new();
    let waiting = vec![
        pt(10, P1, "test.trigger_a"),
        pt(11, P1, "test.trigger_b"),
        pt(12, P1, "test.trigger_c"),
    ];
    let apnap = [P1];
    let procedure = |_s: &mut Blank, ctx: &mut ChoiceContext<'_>| {
        place_triggers(&env, ctx, &apnap, waiting.clone())
    };
    let mut session = ChoiceSession::start(Blank, procedure).expect("starts");
    let pending = session.pending().cloned().expect("an ORDER request");
    assert_order_req(
        &pending,
        P1,
        &[
            desc_obj(10, "test.trigger_a"),
            desc_obj(11, "test.trigger_b"),
            desc_obj(12, "test.trigger_c"),
        ],
    );
    for bad in [
        order(&[0, 0, 1]),
        order(&[0, 1]),
        order(&[0, 1, 2, 2]),
        order(&[0, 1, 3]),
        order(&[]),
        ChoiceResponse::Yes,
        modes(&[0]),
        targets(vec![]),
    ] {
        match session.submit(pending.request_id, bad.clone()) {
            Err(ChoiceError::Invalid { .. }) => {}
            other => panic!("{bad:?} should be rejected, got {other:?}"),
        }
        assert_eq!(session.pending(), Some(&pending));
    }
    session.submit_default(pending.request_id).expect("default");
    match session.status() {
        Status::Done { output, .. } => assert_eq!(
            stacked_ids(output),
            vec![
                s("test.trigger_a"),
                s("test.trigger_b"),
                s("test.trigger_c")
            ]
        ),
        Status::Pending(r) => panic!("still pending: {r:?}"),
    }
}

/// REQ-EVENT-002; CR 603.3d: CHOOSE_TARGETS shape: chooser = controller, reason "CR 603.3d" with
/// the trigger's source, one slot per AST slot with its min and max clamped to the legal count,
/// no current targets, may_keep_illegal false; invalid answers are rejected.
#[test]
fn req_event_002_choose_targets_shape_and_rejections() {
    let env = ScriptEnv::new()
        .slot("test.trigger_t", 0, vec![obj(4), obj(6)])
        .slot("test.trigger_t", 1, vec![ply(P1), ply(P2)]);
    let waiting = vec![targeted(
        pt(40, P2, "test.trigger_t"),
        vec![slot(0, 1, 3), slot(1, 0, 1)],
    )];
    let apnap = [P1, P2];
    let procedure = |_s: &mut Blank, ctx: &mut ChoiceContext<'_>| {
        place_triggers(&env, ctx, &apnap, waiting.clone())
    };
    let mut session = ChoiceSession::start(Blank, procedure).expect("starts");
    let r = session
        .pending()
        .cloned()
        .expect("a CHOOSE_TARGETS request");
    assert_eq!(r.request_id.get(), 0);
    assert_eq!(r.choice_type, ChoiceType::ChooseTargets);
    assert_eq!(r.chooser, P2);
    assert_eq!(r.reason.cr, "CR 603.3d");
    assert_eq!(r.reason.source, Some(oid(40)));
    assert_eq!(
        r.payload,
        ChoicePayload::ChooseTargets {
            slots: vec![
                TargetSlot {
                    min: 1,
                    max: 2,
                    legal: vec![obj(4), obj(6)],
                    current: vec![],
                },
                TargetSlot {
                    min: 0,
                    max: 1,
                    legal: vec![ply(P1), ply(P2)],
                    current: vec![],
                },
            ],
            may_keep_illegal: false,
        }
    );
    assert!(r.validate(&r.default_on_timeout).is_ok());
    for bad in [
        targets(vec![vec![obj(5)], vec![]]),
        targets(vec![vec![], vec![]]),
        targets(vec![vec![obj(4)]]),
        targets(vec![vec![obj(4), obj(4)], vec![]]),
        targets(vec![vec![obj(4)], vec![ply(P1), ply(P2)]]),
        order(&[0]),
    ] {
        match session.submit(r.request_id, bad.clone()) {
            Err(ChoiceError::Invalid { .. }) => {}
            other => panic!("{bad:?} should be rejected, got {other:?}"),
        }
    }
    session
        .submit(
            r.request_id,
            targets(vec![vec![obj(4), obj(6)], vec![ply(P1)]]),
        )
        .expect("valid answer");
    match session.status() {
        Status::Done { output, .. } => {
            assert_eq!(output.stacked.len(), 1);
            assert_eq!(
                output.stacked[0].targets,
                vec![vec![obj(4), obj(6)], vec![ply(P1)]]
            );
            assert!(output.stacked[0].modes.is_empty());
        }
        Status::Pending(r) => panic!("still pending: {r:?}"),
    }
}

// =============================================================================================
// 4. modes then targets (CR 603.3c, 700.2b, 700.2c)
// =============================================================================================

/// The modal trigger of the mode tests: one ability-level slot; mode m0 needs a target that
/// does not exist; m1 and m2 each have their own slot with legal targets; m3 has no targets.
fn modal_trigger(min: u32, max: u32) -> PendingTrigger {
    modal(
        targeted(pt(30, P1, "test.trigger_modal"), vec![slot(0, 1, 1)]),
        modeset(
            min,
            max,
            vec![
                mode("m0", vec![slot(0, 1, 1)]),
                mode("m1", vec![slot(0, 1, 1)]),
                mode("m2", vec![slot(0, 1, 2)]),
                mode("m3", vec![]),
            ],
        ),
    )
}

fn modal_env() -> ScriptEnv {
    ScriptEnv::new()
        .slot("test.trigger_modal", 0, vec![ply(P2)])
        .mode_slot("test.trigger_modal", 1, 0, vec![obj(5)])
        .mode_slot("test.trigger_modal", 2, 0, vec![obj(7), obj(8)])
}

/// REQ-EVENT-002; CR 603.3c, 700.2b, 700.2c: only legal modes are offered (a mode whose own
/// required slot has no legal target is not); the response is mapped back to ORIGINAL mode
/// indices, ascending; the single CHOOSE_TARGETS lists ability-level slots first, then the
/// chosen modes' slots in ascending mode order; `legal_targets` gets the mode argument.
#[test]
fn req_event_002_modes_legal_only_then_targets_in_mode_order() {
    let env = modal_env();
    let waiting = vec![modal_trigger(1, 4)];
    // The request lists m1, m2, m3: answer [2, 1] = m3 and m2, given out of order.
    let d = drive(
        &env,
        &[P1, P2],
        &waiting,
        &[
            modes(&[2, 1]),
            targets(vec![vec![ply(P2)], vec![obj(7), obj(8)]]),
        ],
    );
    let r = &d.requests[0];
    assert_eq!(r.choice_type, ChoiceType::ChooseModes);
    assert_eq!(r.chooser, P1);
    assert_eq!(r.reason.cr, "CR 603.3c");
    assert_eq!(r.reason.source, Some(oid(30)));
    assert_eq!(
        r.payload,
        ChoicePayload::ChooseModes {
            labels: vec![s("m1"), s("m2"), s("m3")],
            repeats_allowed: false,
            mode_costs: vec![],
        }
    );
    assert_eq!(r.min, 1);
    assert_eq!(r.max, 3, "max clamped to the number of legal modes");
    assert!(matches!(r.options, ChoiceOptions::Enumerated(_)));

    let t = &d.requests[1];
    assert_eq!(t.choice_type, ChoiceType::ChooseTargets);
    assert_eq!(t.chooser, P1);
    assert_eq!(t.reason.cr, "CR 603.3d");
    assert_eq!(t.reason.source, Some(oid(30)));
    assert_eq!(
        t.payload,
        ChoicePayload::ChooseTargets {
            slots: vec![
                TargetSlot {
                    min: 1,
                    max: 1,
                    legal: vec![ply(P2)],
                    current: vec![],
                },
                TargetSlot {
                    min: 1,
                    max: 2,
                    legal: vec![obj(7), obj(8)],
                    current: vec![],
                },
            ],
            may_keep_illegal: false,
        }
    );
    assert_eq!(d.requests.len(), 2);
    let p = d.placement();
    assert_eq!(p.stacked.len(), 1);
    assert_eq!(p.stacked[0].modes, vec![2, 3]);
    assert_eq!(
        p.stacked[0].targets,
        vec![vec![ply(P2)], vec![obj(7), obj(8)]]
    );
    assert!(env.saw("test.trigger_modal", None, 0, &[]));
    assert!(env.saw("test.trigger_modal", Some(0), 0, &[]));
    assert!(env.saw("test.trigger_modal", Some(1), 0, &[]));
    assert!(env.saw("test.trigger_modal", Some(2), 0, &[]));
}

/// REQ-EVENT-002; CR 700.2c: two chosen modes with their own slots: the slots follow ascending
/// ORIGINAL mode order whatever order the response lists the modes in.
#[test]
fn req_event_002_mode_slots_in_ascending_mode_order() {
    let env = modal_env();
    let waiting = vec![modal_trigger(2, 2)];
    for answer in [modes(&[1, 0]), modes(&[0, 1])] {
        let d = drive(
            &env,
            &[P1],
            &waiting,
            &[
                answer,
                targets(vec![vec![ply(P2)], vec![obj(5)], vec![obj(7)]]),
            ],
        );
        assert_eq!(d.requests[0].min, 2);
        assert_eq!(d.requests[0].max, 2);
        match &d.requests[1].payload {
            ChoicePayload::ChooseTargets { slots, .. } => {
                let legal: Vec<_> = slots.iter().map(|sl| sl.legal.clone()).collect();
                assert_eq!(
                    legal,
                    vec![vec![ply(P2)], vec![obj(5)], vec![obj(7), obj(8)]]
                );
            }
            other => panic!("expected CHOOSE_TARGETS, got {other:?}"),
        }
        assert_eq!(d.placement().stacked[0].modes, vec![1, 2]);
    }
}

/// REQ-EVENT-002; CR 700.2c: a mode's own slots are asked only if the mode is chosen; a chosen
/// mode without targets and no ability-level slot means no CHOOSE_TARGETS at all.
#[test]
fn req_event_002_unchosen_mode_slots_not_asked() {
    let env = modal_env();
    // Only m1 chosen (request index 0): ability slot + m1's slot.
    let d = drive(
        &env,
        &[P1],
        &[modal_trigger(1, 1)],
        &[modes(&[0]), targets(vec![vec![ply(P2)], vec![obj(5)]])],
    );
    match &d.requests[1].payload {
        ChoicePayload::ChooseTargets { slots, .. } => assert_eq!(slots.len(), 2),
        other => panic!("expected CHOOSE_TARGETS, got {other:?}"),
    }
    assert_eq!(d.placement().stacked[0].modes, vec![1]);

    // No ability-level slot, a target-less mode: no CHOOSE_TARGETS.
    let t = modal(
        pt(31, P1, "test.trigger_plain_modal"),
        modeset(1, 1, vec![mode("a", vec![]), mode("b", vec![])]),
    );
    let d = drive(&ScriptEnv::new(), &[P1], &[t], &[modes(&[1])]);
    assert_eq!(d.types(), vec![ChoiceType::ChooseModes]);
    assert_eq!(d.placement().stacked[0].modes, vec![1]);
    assert!(d.placement().stacked[0].targets.is_empty());
}

/// REQ-EVENT-002; CR 603.3c, 700.2b: the default answer selects the first `min` legal modes.
#[test]
fn req_event_002_modes_default_first_min_legal() {
    let env = modal_env();
    let d = drive_with(&env, &[P1], &[modal_trigger(2, 3)], |r| {
        match r.choice_type {
            ChoiceType::ChooseModes => r.default_on_timeout.clone(),
            _ => targets(vec![vec![ply(P2)], vec![obj(5)], vec![obj(7)]]),
        }
    });
    assert_eq!(d.requests[0].default_on_timeout, modes(&[0, 1]));
    assert_eq!(d.requests[0].min, 2);
    assert_eq!(d.requests[0].max, 3);
    assert_eq!(d.placement().stacked[0].modes, vec![1, 2]);
}

/// REQ-EVENT-002; CR 603.3c, 700.2b: fewer legal modes than the minimum removes the trigger
/// (NoLegalTargets) without asking anything for it.
#[test]
fn req_event_002_too_few_legal_modes_removes() {
    let t = modal(
        pt(32, P1, "test.trigger_modal_dead"),
        modeset(
            2,
            2,
            vec![
                mode("a", vec![slot(0, 1, 1)]),
                mode("b", vec![]),
                mode("c", vec![slot(0, 1, 1)]),
            ],
        ),
    );
    let d = drive(&ScriptEnv::new(), &[P1], std::slice::from_ref(&t), &[]);
    assert!(d.requests.is_empty());
    let p = d.placement();
    assert!(p.stacked.is_empty());
    assert_eq!(p.removed.len(), 1);
    assert_eq!(p.removed[0].trigger, t);
    assert_eq!(p.removed[0].reason, RemovedReason::NoLegalTargets);
}

/// REQ-EVENT-002; CR 603.3c, 700.2b: a mode whose own slot has min 0 and no legal target is still
/// legal; its slot appears with empty legal and max 0.
#[test]
fn req_event_002_mode_with_optional_empty_slot_is_legal() {
    let t = modal(
        pt(33, P1, "test.trigger_modal_opt"),
        modeset(1, 1, vec![mode("only", vec![slot(0, 0, 1)])]),
    );
    let d = drive(
        &ScriptEnv::new(),
        &[P1],
        &[t],
        &[modes(&[0]), targets(vec![vec![]])],
    );
    match &d.requests[0].payload {
        ChoicePayload::ChooseModes { labels, .. } => assert_eq!(labels, &vec![s("only")]),
        other => panic!("expected CHOOSE_MODES, got {other:?}"),
    }
    assert_eq!(
        d.requests[1].payload,
        ChoicePayload::ChooseTargets {
            slots: vec![TargetSlot {
                min: 0,
                max: 0,
                legal: vec![],
                current: vec![],
            }],
            may_keep_illegal: false,
        }
    );
    assert_eq!(d.placement().stacked[0].modes, vec![0]);
    assert_eq!(d.placement().stacked[0].targets, vec![Vec::new()]);
}

/// REQ-EVENT-002; CR 603.3d: `legal_targets` sees what this placement has already stacked,
/// including earlier players' triggers, in stack order.
#[test]
fn req_event_002_legal_targets_sees_stacked_so_far() {
    for (o, first, second) in [
        (IDENT, "test.trigger_a", "test.trigger_b"),
        (REV, "test.trigger_b", "test.trigger_a"),
    ] {
        let env = ScriptEnv::new()
            .slot("test.trigger_a", 0, vec![obj(1)])
            .slot("test.trigger_b", 0, vec![obj(2)])
            .slot("test.trigger_c", 0, vec![obj(3)]);
        let waiting = vec![
            targeted(pt(50, P2, "test.trigger_c"), vec![slot(0, 1, 1)]),
            targeted(pt(40, P1, "test.trigger_a"), vec![slot(0, 1, 1)]),
            targeted(pt(41, P1, "test.trigger_b"), vec![slot(0, 1, 1)]),
        ];
        let d = drive_with(&env, &[P1, P2], &waiting, |r| match &r.payload {
            ChoicePayload::OrderSimultaneousTriggers { .. } => order(&o),
            ChoicePayload::ChooseTargets { slots, .. } => {
                targets(slots.iter().map(|sl| sl.legal[..1].to_vec()).collect())
            }
            other => panic!("unexpected {other:?}"),
        });
        assert_eq!(
            stacked_ids(d.placement()),
            vec![s(first), s(second), s("test.trigger_c")]
        );
        assert!(env.saw(first, None, 0, &[]));
        assert!(env.saw(second, None, 0, &[first]));
        assert!(env.saw("test.trigger_c", None, 0, &[first, second]));
        for c in env.calls_for("test.trigger_c") {
            assert_eq!(c.3, vec![s(first), s(second)]);
        }
        // Each stacked trigger keeps the target its CHOOSE_TARGETS answer gave.
        let got: Vec<_> = d
            .placement()
            .stacked
            .iter()
            .map(|t| t.targets.clone())
            .collect();
        let want_for = |id: &str| match id {
            "test.trigger_a" => vec![vec![obj(1)]],
            "test.trigger_b" => vec![vec![obj(2)]],
            _ => vec![vec![obj(3)]],
        };
        assert_eq!(
            got,
            vec![
                want_for(first),
                want_for(second),
                want_for("test.trigger_c")
            ]
        );
    }
}

// =============================================================================================
// 5. removal (CR 603.3d)
// =============================================================================================

/// REQ-EVENT-002; CR 603.3d: an ability-level slot with min > 0 and no legal target removes the
/// trigger (NoLegalTargets) with no CHOOSE_MODES / CHOOSE_TARGETS for it; it still took part in
/// the ORDER request; the others keep their relative order.
#[test]
fn req_event_002_no_legal_target_removes_after_ordering() {
    let dead = modal(
        targeted(pt(2, P1, "test.trigger_dead"), vec![slot(0, 1, 1)]),
        modeset(1, 1, vec![mode("a", vec![]), mode("b", vec![])]),
    );
    let waiting = vec![
        pt(1, P1, "test.trigger_a"),
        dead.clone(),
        pt(3, P1, "test.trigger_c"),
        pt(9, P2, "test.trigger_other"),
    ];
    for (o, want) in [
        ([2u32, 1, 0], vec!["test.trigger_c", "test.trigger_a"]),
        ([0, 1, 2], vec!["test.trigger_a", "test.trigger_c"]),
        ([1, 2, 0], vec!["test.trigger_c", "test.trigger_a"]),
    ] {
        let d = drive(&ScriptEnv::new(), &[P1, P2], &waiting, &[order(&o)]);
        assert_eq!(d.types(), vec![ChoiceType::OrderSimultaneousTriggers]);
        assert_order_req(
            &d.requests[0],
            P1,
            &[
                desc_obj(1, "test.trigger_a"),
                desc_obj(2, "test.trigger_dead"),
                desc_obj(3, "test.trigger_c"),
            ],
        );
        let p = d.placement();
        let mut want: Vec<String> = want.iter().map(|x| s(x)).collect();
        want.push(s("test.trigger_other"));
        assert_eq!(stacked_ids(p), want);
        assert_eq!(p.removed.len(), 1);
        assert_eq!(p.removed[0].trigger, dead);
        assert_eq!(p.removed[0].reason, RemovedReason::NoLegalTargets);
    }
}

/// REQ-EVENT-002; CR 603.3d: removed triggers are listed in the order they were found, which is
/// stack order across players.
#[test]
fn req_event_002_removed_in_order_found() {
    let waiting = vec![
        targeted(pt(20, P2, "test.trigger_dead_p2"), vec![slot(0, 1, 1)]),
        targeted(pt(10, P1, "test.trigger_dead_p1a"), vec![slot(0, 1, 1)]),
        targeted(pt(11, P1, "test.trigger_dead_p1b"), vec![slot(0, 2, 2)]),
        pt(12, P1, "test.trigger_live"),
    ];
    let d = drive(&ScriptEnv::new(), &[P1, P2], &waiting, &[order(&[2, 1, 0])]);
    assert_eq!(stacked_ids(d.placement()), vec![s("test.trigger_live")]);
    assert_eq!(
        removed_ids(d.placement()),
        vec![
            s("test.trigger_dead_p1b"),
            s("test.trigger_dead_p1a"),
            s("test.trigger_dead_p2")
        ]
    );
}

/// REQ-EVENT-002; CR 603.3d: a slot with min 0 and no legal target does not remove the trigger:
/// it stays in the request with empty legal and max 0.
#[test]
fn req_event_002_optional_slot_without_targets_does_not_remove() {
    let t = targeted(pt(5, P1, "test.trigger_opt_slot"), vec![slot(0, 0, 2)]);
    let d = drive(&ScriptEnv::new(), &[P1], &[t], &[targets(vec![vec![]])]);
    assert_eq!(
        d.requests[0].payload,
        ChoicePayload::ChooseTargets {
            slots: vec![TargetSlot {
                min: 0,
                max: 0,
                legal: vec![],
                current: vec![],
            }],
            may_keep_illegal: false,
        }
    );
    assert!(d.placement().removed.is_empty());
    assert_eq!(d.placement().stacked[0].targets, vec![Vec::new()]);
}

// =============================================================================================
// 6. optional triggers are not asked at placement (CR 603.5)
// =============================================================================================

/// REQ-EVENT-002; CR 603.5: `place_triggers` never asks CHOOSE_OPTIONAL_TRIGGER: an optional
/// trigger goes on the stack regardless; the "may" is decided on resolution.
#[test]
fn req_event_002_placement_never_asks_optional() {
    let env = ScriptEnv::new().slot("test.trigger_may_t", 0, vec![obj(1)]);
    let single = vec![optional(pt(1, P1, "test.trigger_may"))];
    let d = drive(&env, &[P1], &single, &[]);
    assert!(d.requests.is_empty());
    assert_eq!(stacked_ids(d.placement()), vec![s("test.trigger_may")]);
    assert!(d.placement().stacked[0].trigger.optional);

    let several = vec![
        optional(pt(1, P1, "test.trigger_may")),
        optional(targeted(
            pt(2, P1, "test.trigger_may_t"),
            vec![slot(0, 1, 1)],
        )),
        optional(pt2(3, P2, "test.trigger_may_2")),
    ];
    let d = drive_default(&env, &[P1, P2], &several);
    assert!(
        d.requests
            .iter()
            .all(|r| r.choice_type != ChoiceType::ChooseOptionalTrigger)
    );
    assert_eq!(d.placement().stacked.len(), 3);
}

// =============================================================================================
// replay and determinism
// =============================================================================================

/// REQ-ARCH-002, REQ-EVENT-002; CR 603.3b: the same answers give the same placement, whether
/// submitted one by one or through `ChoiceSession::replay`.
#[test]
fn req_arch_002_placement_replays_identically() {
    let env = modal_env().slot("test.trigger_t", 0, vec![obj(4), obj(6)]);
    let waiting = vec![
        pt(20, P2, "test.trigger_p2a"),
        modal_trigger(1, 2),
        targeted(pt(11, P1, "test.trigger_t"), vec![slot(0, 1, 1)]),
        pt2(21, P2, "test.trigger_p2b"),
        pt(22, P2, "test.trigger_p2c"),
    ];
    let mut lcg = Lcg::new(7);
    let answer = |r: &PendingChoiceRequest, lcg: &mut Lcg| match &r.payload {
        ChoicePayload::OrderSimultaneousTriggers { triggers } => order(&lcg.perm(triggers.len())),
        ChoicePayload::ChooseModes { .. } => modes(&[1, 0]),
        ChoicePayload::ChooseTargets { slots, .. } => targets(
            slots
                .iter()
                .map(|sl| {
                    sl.legal
                        .iter()
                        .rev()
                        .take(sl.min as usize)
                        .copied()
                        .collect()
                })
                .collect(),
        ),
        other => panic!("unexpected {other:?}"),
    };
    let d = drive_with(&env, &[P1, P2], &waiting, |r| answer(r, &mut lcg));
    let again = drive(&env, &[P1, P2], &waiting, &d.answers);
    assert_eq!(again.requests, d.requests);
    assert_eq!(again.placement, d.placement);

    let apnap = [P1, P2];
    let procedure = |_s: &mut Blank, ctx: &mut ChoiceContext<'_>| {
        place_triggers(&env, ctx, &apnap, waiting.clone())
    };
    let replayed = ChoiceSession::replay(Blank, procedure, &d.answers).expect("replays");
    match replayed.status() {
        Status::Done { output, .. } => assert_eq!(Some(output), d.placement.as_ref()),
        Status::Pending(r) => panic!("still pending: {r:?}"),
    }
}

// =============================================================================================
// PendingTrigger::from_waiting and description
// =============================================================================================

/// REQ-EVENT-002; CR 603.3b, 603.5, 700.2b: `from_waiting` copies optional, targets and modes
/// from the ability, keeps the waiting trigger's ability id, source, controller and bindings,
/// takes the given origin, and marks a trigger on another ability triggering as part 2.
#[test]
fn req_event_002_from_waiting_copies_ability_fields() {
    let mut a = ability_with("test.trigger_q", Trigger::Event(m(EventKind::Triggered)));
    a.optional = Some(true);
    a.targets = Some(vec![slot(0, 1, 1)]);
    let ms = modeset(
        1,
        1,
        vec![mode("x", vec![]), mode("y", vec![slot(0, 1, 1)])],
    );
    a.modes = Some(ms.clone());
    let mut bindings = BTreeMap::new();
    bindings.insert(s("$P"), EventField::Player(P3));
    let w = wt(5, P2, "test.trigger_q.instance", bindings.clone());
    let p = PendingTrigger::from_waiting(w, &a, TriggerOrigin::State);
    assert_eq!(
        p,
        PendingTrigger {
            origin: TriggerOrigin::State,
            controller: P2,
            source: Some(oid(5)),
            ability_id: s("test.trigger_q.instance"),
            bindings,
            optional: true,
            responds_to_trigger: true,
            targets: vec![slot(0, 1, 1)],
            modes: Some(ms),
        }
    );
}

/// REQ-EVENT-002; CR 603.3b: only an event trigger on TRIGGERED is part 2; absent optional,
/// targets and modes default to false, empty and None.
#[test]
fn req_event_002_from_waiting_defaults_and_part_one() {
    let triggers = [
        Trigger::Event(m(EventKind::Dies)),
        Trigger::Event(m(EventKind::Activated)),
        Trigger::State(tag_pred("cond")),
        Trigger::PhaseStep {
            step: StepId::Upkeep,
            whose: PlayerSelectorAST::Players(PlayerGroup::AllPlayers),
        },
    ];
    for trig in triggers {
        let a = ability_with("test.trigger_r", trig.clone());
        let p = PendingTrigger::from_waiting(
            wt(6, P1, "test.trigger_r", BTreeMap::new()),
            &a,
            TriggerOrigin::Event,
        );
        assert!(!p.responds_to_trigger, "{trig:?}");
        assert!(!p.optional);
        assert!(p.targets.is_empty());
        assert_eq!(p.modes, None);
        assert_eq!(p.origin, TriggerOrigin::Event);
    }
    let mut a = ev_ability("test.trigger_r", EventKind::Dies);
    a.optional = Some(false);
    let p = PendingTrigger::from_waiting(
        wt(6, P1, "test.trigger_r", BTreeMap::new()),
        &a,
        TriggerOrigin::Delayed,
    );
    assert!(!p.optional);
    assert_eq!(p.origin, TriggerOrigin::Delayed);
}

/// REQ-EVENT-002; CR 603.3b: the ORDER text of a trigger is "object <id> ability <ability id>",
/// or "player <controller> ability <ability id>" without a source.
#[test]
fn req_event_002_description_format() {
    assert_eq!(
        pt(12, P1, "test.trigger_x").description(),
        "object 12 ability test.trigger_x"
    );
    let mut t = pt(12, P3, "sourceless.speed");
    t.source = None;
    assert_eq!(t.description(), "player 3 ability sourceless.speed");
}

// =============================================================================================
// 9. ORDER labels disambiguated by public bindings (CR 603.3b, CR 101.4c)
// =============================================================================================

/// `t` with the bindings `pairs`.
fn bound(mut t: PendingTrigger, pairs: &[(&str, EventField)]) -> PendingTrigger {
    t.bindings = pairs.iter().map(|(k, v)| (s(k), v.clone())).collect();
    t
}

/// REQ-EVENT-002, REQ-ARCH-002; CR 603.3b, 101.4c: two triggers of one ability and source bound
/// to different public objects get distinct labels `<label> with <name>=object <n>`; the answer
/// indices still refer to the payload positions (reversed answer stacks them reversed).
#[test]
fn req_event_002_order_labels_show_public_object_bindings() {
    let env = PublicEnv::new(&[9, 8]);
    let a = bound(pt(3, P1, "a"), &[("target", EventField::Object(oid(9)))]);
    let b = bound(pt(3, P1, "a"), &[("target", EventField::Object(oid(8)))]);
    for (answer, want) in [(order(&[0, 1]), [9u64, 8]), (order(&[1, 0]), [8, 9])] {
        let d = drive(&env, &[P1, P2], &[a.clone(), b.clone()], &[answer]);
        assert_eq!(
            order_payload(&d.requests[0]),
            vec![
                s("object 3 ability a with target=object 9"),
                s("object 3 ability a with target=object 8"),
            ]
        );
        let got: Vec<EventField> = d
            .placement()
            .stacked
            .iter()
            .map(|st| st.trigger.bindings["target"].clone())
            .collect();
        let want: Vec<EventField> = want.iter().map(|&n| EventField::Object(oid(n))).collect();
        assert_eq!(got, want);
    }
}

/// REQ-EVENT-002; CR 603.3b, 101.4c: with the default `is_public_object` (false) nothing about
/// the bindings leaks: both labels stay the bare identical label.
#[test]
fn req_event_002_order_labels_do_not_leak_non_public_objects() {
    let env = ScriptEnv::new();
    let a = bound(pt(3, P1, "a"), &[("target", EventField::Object(oid(9)))]);
    let b = bound(pt(3, P1, "a"), &[("target", EventField::Object(oid(8)))]);
    let d = drive(&env, &[P1], &[a, b], &[order(&IDENT)]);
    assert_eq!(
        order_payload(&d.requests[0]),
        vec![desc_obj(3, "a"), desc_obj(3, "a")]
    );
    // An object the environment does not call public is hidden even when another is public.
    let env = PublicEnv::new(&[8]);
    let a = bound(pt(3, P1, "a"), &[("target", EventField::Object(oid(9)))]);
    let b = bound(pt(3, P1, "a"), &[("target", EventField::Object(oid(8)))]);
    let d = drive(&env, &[P1], &[a, b], &[order(&IDENT)]);
    assert_eq!(
        order_payload(&d.requests[0]),
        vec![
            desc_obj(3, "a"),
            s("object 3 ability a with target=object 8")
        ]
    );
}

/// REQ-EVENT-002; CR 603.3b, 101.4c: a player binding is shown (`name=player <n>`); Value
/// bindings are never shown; several public bindings are listed in binding-name order, joined by
/// `, `.
#[test]
fn req_event_002_order_labels_player_and_value_bindings() {
    let env = PublicEnv::new(&[5]);
    let a = bound(
        pt(3, P1, "a"),
        &[
            ("who", EventField::Player(P2)),
            ("amount", EventField::Value(FilterValue::Int(4))),
            ("obj", EventField::Object(oid(5))),
        ],
    );
    let b = bound(
        pt(3, P1, "a"),
        &[
            ("who", EventField::Player(P3)),
            ("amount", EventField::Value(FilterValue::Int(4))),
        ],
    );
    let d = drive(&env, &[P1], &[a, b], &[order(&IDENT)]);
    assert_eq!(
        order_payload(&d.requests[0]),
        vec![
            s("object 3 ability a with obj=object 5, who=player 2"),
            s("object 3 ability a with who=player 3"),
        ]
    );
    // Only Value bindings: nothing to show, the bare label stays.
    let a = bound(
        pt(3, P1, "a"),
        &[("amount", EventField::Value(FilterValue::Int(1)))],
    );
    let b = bound(
        pt(3, P1, "a"),
        &[("amount", EventField::Value(FilterValue::Int(2)))],
    );
    let d = drive(&env, &[P1], &[a, b], &[order(&IDENT)]);
    assert_eq!(
        order_payload(&d.requests[0]),
        vec![desc_obj(3, "a"), desc_obj(3, "a")]
    );
}

/// REQ-EVENT-002; CR 603.3b, 101.4c: labels that are already unique keep the bare form even when
/// the triggers have public bindings; a sourceless label gets the same suffix rule.
#[test]
fn req_event_002_order_labels_unique_stay_bare() {
    let env = PublicEnv::new(&[9, 8]);
    let a = bound(pt(3, P1, "a"), &[("target", EventField::Object(oid(9)))]);
    let b = bound(pt(4, P1, "a"), &[("target", EventField::Object(oid(8)))]);
    let d = drive(&env, &[P1], &[a, b], &[order(&IDENT)]);
    assert_eq!(
        order_payload(&d.requests[0]),
        vec![desc_obj(3, "a"), desc_obj(4, "a")]
    );
    let mut c = bound(pt(3, P1, "a"), &[("who", EventField::Player(P2))]);
    c.source = None;
    let mut e = bound(pt(3, P1, "a"), &[("who", EventField::Player(P3))]);
    e.source = None;
    let d = drive(&env, &[P1], &[c, e], &[order(&IDENT)]);
    assert_eq!(
        order_payload(&d.requests[0]),
        vec![
            s("player 1 ability a with who=player 2"),
            s("player 1 ability a with who=player 3"),
        ]
    );
}

// =============================================================================================
// 10. CHOOSE_MODES options (CR 603.3c, CR 700.2b)
// =============================================================================================

fn plain_modes(n: usize) -> Vec<fineprint_dsl::ability::ModeAST> {
    (0..n).map(|i| mode(&format!("m{i}"), vec![])).collect()
}

/// REQ-EVENT-002; CR 603.3c, 700.2b: 16 legal modes, exactly 8 chosen (12,870 answers, at least
/// ENUMERATION_LIMIT): the request carries the Validator contract and the eight chosen indices
/// land in `StackedTrigger.modes`.
#[test]
fn req_event_002_modes_many_answers_use_validator() {
    let t = modal(
        pt(30, P1, "test.trigger_modal"),
        modeset(8, 8, plain_modes(16)),
    );
    let chosen = [15u32, 1, 3, 5, 7, 9, 11, 13];
    let d = drive(&ScriptEnv::new(), &[P1], &[t], &[modes(&chosen)]);
    let r = &d.requests[0];
    assert_eq!(r.choice_type, ChoiceType::ChooseModes);
    assert_eq!(r.reason.cr, "CR 603.3c");
    assert_eq!((r.min, r.max), (8, 8));
    assert_eq!(
        r.options,
        ChoiceOptions::Validator(ValidatorContract {
            min: 8,
            max: 8,
            rule: ValidatorRule::Modes {
                count: 16,
                repeats_allowed: false,
            },
        })
    );
    assert_eq!(d.requests.len(), 1);
    assert_eq!(
        d.placement().stacked[0].modes,
        vec![1, 3, 5, 7, 9, 11, 13, 15]
    );
}

/// REQ-EVENT-002; CR 603.3c, 700.2b: of 16 modes two have an unfillable required target, so 14
/// are legal; min 1 max 14 gives 2^14 - 1 answers: Validator with count 14; the answer's indices
/// are positions among the legal modes and map back to the original mode indices.
#[test]
fn req_event_002_modes_validator_count_is_legal_modes() {
    let ms: Vec<_> = (0..16)
        .map(|i| {
            if i == 3 || i == 10 {
                mode(&format!("m{i}"), vec![slot(0, 1, 1)])
            } else {
                mode(&format!("m{i}"), vec![])
            }
        })
        .collect();
    let t = modal(pt(30, P1, "test.trigger_modal"), modeset(1, 14, ms));
    let d = drive(&ScriptEnv::new(), &[P1], &[t], &[modes(&[13, 0, 3])]);
    let r = &d.requests[0];
    assert_eq!((r.min, r.max), (1, 14));
    assert_eq!(
        r.options,
        ChoiceOptions::Validator(ValidatorContract {
            min: 1,
            max: 14,
            rule: ValidatorRule::Modes {
                count: 14,
                repeats_allowed: false,
            },
        })
    );
    // Legal originals: 0,1,2,4,5,6,7,8,9,11,12,13,14,15: positions 0, 3, 13 are 0, 4, 15.
    assert_eq!(d.placement().stacked[0].modes, vec![0, 4, 15]);
}

/// REQ-EVENT-002; CR 603.3c, 700.2b: a small mode set (3 modes, 1 to 2 chosen) still gets
/// Enumerated `Mode(0..k)` options.
#[test]
fn req_event_002_modes_small_set_stays_enumerated() {
    let t = modal(
        pt(30, P1, "test.trigger_modal"),
        modeset(1, 2, plain_modes(3)),
    );
    let d = drive(&ScriptEnv::new(), &[P1], &[t], &[modes(&[2, 0])]);
    let r = &d.requests[0];
    match &r.options {
        ChoiceOptions::Enumerated(items) => {
            assert!(!items.is_empty());
            assert!(
                items
                    .iter()
                    .all(|o| matches!(o, ChoiceOption::Mode(i) if *i < 3))
            );
        }
        other => panic!("expected Enumerated, got {other:?}"),
    }
    assert_eq!(d.placement().stacked[0].modes, vec![0, 2]);
}
