// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the "may" choice of an optional triggered ability, made as it resolves
//! (REQ-EVENT-002; CR 603.5): `ask_optional_trigger`. Black-box: written from the work-item
//! brief only.

#[path = "spec_triggers_stack_support.rs"]
mod support;

use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceResponse,
    ChoiceSession, ChoiceType, Status,
};
use fineprint_core::triggers::{PendingTrigger, ask_optional_trigger};
use support::*;

type Outcome = (Vec<fineprint_core::choice::PendingChoiceRequest>, bool);

/// Runs `ask_optional_trigger` in a session, answering with `answer` (if asked).
fn run(trigger: &PendingTrigger, answer: Option<ChoiceResponse>) -> Outcome {
    let procedure =
        |_s: &mut Blank, ctx: &mut ChoiceContext<'_>| ask_optional_trigger(ctx, trigger);
    let mut session = ChoiceSession::start(Blank, procedure).expect("starts");
    let mut asked = Vec::new();
    if let Some(r) = session.pending().cloned() {
        asked.push(r.clone());
        let a = answer.expect("an answer was needed");
        session.submit(r.request_id, a).expect("accepted");
    }
    match session.status() {
        Status::Done { output, .. } => (asked, *output),
        Status::Pending(r) => panic!("asked twice: {r:?}"),
    }
}

/// REQ-EVENT-002; CR 603.5: a trigger that is not optional returns true with no request.
#[test]
fn req_event_002_optional_not_optional_no_ask() {
    let (asked, out) = run(&pt(1, P1, "test.trigger_plain"), None);
    assert!(asked.is_empty());
    assert!(out);
}

/// REQ-EVENT-002; CR 603.5: an optional trigger asks CHOOSE_OPTIONAL_TRIGGER once: chooser =
/// controller, reason "CR 603.5" with the trigger's source, payload description =
/// `description()`, min 1, max 1, options [Yes, No], default No; Yes -> true, No -> false.
#[test]
fn req_event_002_optional_asks_yes_no() {
    let t = optional(pt(7, P2, "test.trigger_may"));
    for (answer, want) in [(ChoiceResponse::Yes, true), (ChoiceResponse::No, false)] {
        let (asked, out) = run(&t, Some(answer));
        assert_eq!(out, want);
        assert_eq!(asked.len(), 1);
        let r = &asked[0];
        assert_eq!(r.request_id.get(), 0);
        assert_eq!(r.choice_type, ChoiceType::ChooseOptionalTrigger);
        assert_eq!(r.chooser, P2);
        assert_eq!(r.reason.cr, "CR 603.5");
        assert_eq!(r.reason.source, Some(oid(7)));
        assert_eq!(
            r.payload,
            ChoicePayload::ChooseOptionalTrigger {
                description: t.description(),
            }
        );
        assert_eq!(
            r.payload,
            ChoicePayload::ChooseOptionalTrigger {
                description: desc_obj(7, "test.trigger_may"),
            }
        );
        assert_eq!(r.min, 1);
        assert_eq!(r.max, 1);
        assert_eq!(
            r.options,
            ChoiceOptions::Enumerated(vec![ChoiceOption::Yes, ChoiceOption::No])
        );
        assert_eq!(r.default_on_timeout, ChoiceResponse::No);
        assert!(r.validate(&r.default_on_timeout).is_ok());
    }
}

/// REQ-EVENT-002; CR 603.5: the timeout default declines; a wrong response type is rejected.
#[test]
fn req_event_002_optional_default_declines_and_rejects_wrong_type() {
    let t = optional(pt(7, P1, "test.trigger_may"));
    let procedure = |_s: &mut Blank, ctx: &mut ChoiceContext<'_>| ask_optional_trigger(ctx, &t);
    let mut session = ChoiceSession::start(Blank, procedure).expect("starts");
    let id = session.pending().expect("asked").request_id;
    for bad in [order(&[0]), modes(&[0]), ChoiceResponse::PassPriority] {
        match session.submit(id, bad.clone()) {
            Err(ChoiceError::Invalid { .. }) => {}
            other => panic!("{bad:?} should be rejected, got {other:?}"),
        }
    }
    session.submit_default(id).expect("default");
    match session.status() {
        Status::Done { output, .. } => assert!(!*output),
        Status::Pending(r) => panic!("still pending: {r:?}"),
    }
}

/// REQ-EVENT-002; CR 603.5, 113.8: a sourceless optional trigger asks with reason source None
/// and the "player ..." description.
#[test]
fn req_event_002_optional_sourceless() {
    let mut t = optional(pt(0, P3, "sourceless.monarch.end_step"));
    t.source = None;
    let (asked, out) = run(&t, Some(ChoiceResponse::Yes));
    assert!(out);
    assert_eq!(asked[0].chooser, P3);
    assert_eq!(asked[0].reason.source, None);
    assert_eq!(
        asked[0].payload,
        ChoicePayload::ChooseOptionalTrigger {
            description: desc_player(P3, "sourceless.monarch.end_step"),
        }
    );
}
