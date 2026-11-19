// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for look-back trigger detection (REQ-EVENT-002; CR 603.10, 603.10a-g, 603.6a,
//! 603.6c, 701.6a): the derived look-back table, one test per look-back category, pre-event-only
//! visibility and the CR 603.10a example. Black-box: written from the work-item brief only.

#[path = "spec_triggers_support.rs"]
mod support;

#[path = "spec_events_support.rs"]
mod esupport;

use esupport::World;
use fineprint_core::events::{GameEvent, Outcome, ZoneChangeCause};
use fineprint_core::triggers::{
    EventField, LookBackKind, StateView, TriggerEvent, TriggerRegistration, events_from_batch,
    look_back_kind,
};
use fineprint_core::zones::{MoveRequest, ZoneKind};
use fineprint_dsl::ability::{EventBatch as DslBatch, EventKind, EventMatcher};
use fineprint_dsl::expr::Zone;
use support::*;

use EventKind as K;
use StateView::{PostEvent, PreEvent};

// =============================================================================================
// A. the look_back_kind table
// =============================================================================================

fn lb(kind: EventKind, filters: &[(&str, Zone)]) -> Option<LookBackKind> {
    let mut mm = m(kind);
    for (k, z) in filters {
        mm = filt(mm, k, zone(*z));
    }
    look_back_kind(&mm)
}

// REQ-EVENT-002, CR 603.10a, 603.6c: leaves-the-battlefield, sacrifice and leaves-graveyard
// kinds are look-back without filters.
#[test]
fn spec_triggers_lookback_kind_direct_kinds() {
    assert_eq!(
        lb(K::LeavesBattlefield, &[]),
        Some(LookBackKind::LeavesBattlefield)
    );
    assert_eq!(lb(K::Dies, &[]), Some(LookBackKind::LeavesBattlefield));
    assert_eq!(lb(K::Sacrificed, &[]), Some(LookBackKind::Sacrificed));
    assert_eq!(
        lb(K::LeavesGraveyard, &[]),
        Some(LookBackKind::LeavesGraveyard)
    );
}

// REQ-EVENT-002, CR 603.10a, 603.6c: ZONE_CHANGE is look-back only through its from/to filters.
#[test]
fn spec_triggers_lookback_kind_zone_change() {
    use Zone::*;
    assert_eq!(
        lb(K::ZoneChange, &[("from", Battlefield)]),
        Some(LookBackKind::LeavesBattlefield)
    );
    assert_eq!(
        lb(K::ZoneChange, &[("from", Graveyard)]),
        Some(LookBackKind::LeavesGraveyard)
    );
    assert_eq!(
        lb(K::ZoneChange, &[("to", Hand)]),
        Some(LookBackKind::ToHandOrLibrary)
    );
    assert_eq!(
        lb(K::ZoneChange, &[("to", Library)]),
        Some(LookBackKind::ToHandOrLibrary)
    );
    assert_eq!(
        lb(K::ZoneChange, &[("from", Battlefield), ("to", Hand)]),
        Some(LookBackKind::LeavesBattlefield),
        "from BATTLEFIELD wins over to HAND"
    );
    assert_eq!(lb(K::ZoneChange, &[]), None);
    assert_eq!(look_back_kind(&EventMatcher::new(K::ZoneChange)), None);
    assert_eq!(
        lb(K::ZoneChange, &[("from", Library), ("to", Battlefield)]),
        None
    );
}

// REQ-EVENT-002, CR 603.6c: "put into a graveyard from anywhere" is never leaves-the-battlefield;
// only an explicit from BATTLEFIELD makes PUT_INTO_GRAVEYARD look back.
#[test]
fn spec_triggers_lookback_kind_put_into_graveyard() {
    assert_eq!(
        lb(K::PutIntoGraveyard, &[("from", Zone::Battlefield)]),
        Some(LookBackKind::LeavesBattlefield)
    );
    assert_eq!(lb(K::PutIntoGraveyard, &[]), None);
}

// REQ-EVENT-002, CR 603.10a: EXILED and RETURNED_TO_HAND look back by their from filter.
#[test]
fn spec_triggers_lookback_kind_exiled_and_returned() {
    assert_eq!(
        lb(K::Exiled, &[("from", Zone::Battlefield)]),
        Some(LookBackKind::LeavesBattlefield)
    );
    assert_eq!(
        lb(K::Exiled, &[("from", Zone::Graveyard)]),
        Some(LookBackKind::LeavesGraveyard)
    );
    assert_eq!(lb(K::Exiled, &[]), None);
    assert_eq!(
        lb(K::ReturnedToHand, &[("from", Zone::Battlefield)]),
        Some(LookBackKind::LeavesBattlefield)
    );
    assert_eq!(
        lb(K::ReturnedToHand, &[("from", Zone::Graveyard)]),
        Some(LookBackKind::LeavesGraveyard)
    );
    assert_eq!(
        lb(K::ReturnedToHand, &[]),
        Some(LookBackKind::ToHandOrLibrary)
    );
}

// REQ-EVENT-002, CR 603.10b, 603.10c, 603.10d, 603.10e, 603.10f, 603.10g: the other look-back
// categories, one kind each.
#[test]
fn spec_triggers_lookback_kind_other_categories() {
    assert_eq!(lb(K::PhasedOut, &[]), Some(LookBackKind::PhasedOut));
    assert_eq!(lb(K::Unattached, &[]), Some(LookBackKind::Unattached));
    assert_eq!(lb(K::ControlChanged, &[]), Some(LookBackKind::ControlLost));
    assert_eq!(lb(K::Countered, &[]), Some(LookBackKind::Countered));
    assert_eq!(lb(K::PlayerLost, &[]), Some(LookBackKind::PlayerLost));
    assert_eq!(lb(K::Planeswalked, &[]), Some(LookBackKind::Planeswalked));
}

// REQ-EVENT-002, CR 701.8a, 701.8b, 603.6c, 603.10a: a destroyed permanent moves from the
// battlefield to its owner's graveyard, so DESTROYED is a leaves-the-battlefield look-back kind,
// with or without filters; PUT_INTO_GRAVEYARD without a from filter (or from anywhere but the
// battlefield) is not.
#[test]
fn spec_triggers_lookback_kind_destroyed() {
    assert_eq!(lb(K::Destroyed, &[]), Some(LookBackKind::LeavesBattlefield));
    assert_eq!(
        lb(K::Destroyed, &[("from", Zone::Battlefield)]),
        Some(LookBackKind::LeavesBattlefield)
    );
    assert_eq!(
        lb(K::Destroyed, &[("to", Zone::Graveyard)]),
        Some(LookBackKind::LeavesBattlefield)
    );
    assert_eq!(lb(K::PutIntoGraveyard, &[]), None);
    assert_eq!(lb(K::PutIntoGraveyard, &[("to", Zone::Graveyard)]), None);
}

// REQ-EVENT-002, CR 603.10, 603.6a: kinds outside the CR 603.10 list are not look-back.
#[test]
fn spec_triggers_lookback_kind_not_look_back() {
    for kind in [
        K::Enters,
        K::Drawn,
        K::Discarded,
        K::Milled,
        K::Cycled,
        K::Cast,
        K::Activated,
        K::Tapped,
        K::Untapped,
        K::Attached,
        K::DamageDealt,
        K::LifeGained,
        K::LifeLost,
        K::CountersPut,
        K::TokenCreated,
        K::TurnedFaceUp,
        K::Attacks,
    ] {
        assert_eq!(lb(kind, &[]), None, "{kind:?}");
    }
}

// REQ-EVENT-002, CR 603.10: subject, bind and batch never change the derived look-back kind.
#[test]
fn spec_triggers_lookback_kind_ignores_subject_bind_batch() {
    let filter_sets: [&[(&str, Zone)]; 5] = [
        &[],
        &[("from", Zone::Battlefield)],
        &[("from", Zone::Graveyard)],
        &[("to", Zone::Hand)],
        &[("from", Zone::Library), ("to", Zone::Battlefield)],
    ];
    for &kind in EventKind::ALL {
        for filters in filter_sets {
            let mut base = m(kind);
            for (k, z) in filters {
                base = filt(base, k, zone(*z));
            }
            let expected = look_back_kind(&base);
            let variants: Vec<EventMatcher> = vec![
                subj_obj(base.clone()),
                subj_players(base.clone()),
                bind(base.clone(), "object", "$X"),
                bind(bind(base.clone(), "player", "$P"), "amount", "$N"),
                batch(base.clone(), Some(DslBatch::Each)),
                batch(base.clone(), Some(DslBatch::OneOrMore)),
                batch(
                    bind(subj_obj(base.clone()), "object", "$X"),
                    Some(DslBatch::OneOrMore),
                ),
            ];
            for v in &variants {
                assert_eq!(look_back_kind(v), expected, "{kind:?} {filters:?} {v:?}");
            }
            // And removing them again (a matcher with every optional member stripped).
            let mut stripped = subj_obj(bind(batch(base.clone(), Some(DslBatch::Each)), "o", "$O"));
            stripped.subject = None;
            stripped.bind = None;
            stripped.batch = None;
            assert_eq!(look_back_kind(&stripped), expected, "{kind:?} {filters:?}");
        }
    }
}

// REQ-EVENT-002, CR 603.10a-g: the rule each category cites.
#[test]
fn spec_triggers_lookback_kind_rule_strings() {
    assert_eq!(LookBackKind::LeavesBattlefield.rule(), "603.10a");
    assert_eq!(LookBackKind::Sacrificed.rule(), "603.10a");
    assert_eq!(LookBackKind::LeavesGraveyard.rule(), "603.10a");
    assert_eq!(LookBackKind::ToHandOrLibrary.rule(), "603.10a");
    assert_eq!(LookBackKind::PhasedOut.rule(), "603.10b");
    assert_eq!(LookBackKind::Unattached.rule(), "603.10c");
    assert_eq!(LookBackKind::ControlLost.rule(), "603.10d");
    assert_eq!(LookBackKind::Countered.rule(), "603.10e");
    assert_eq!(LookBackKind::PlayerLost.rule(), "603.10f");
    assert_eq!(LookBackKind::Planeswalked.rule(), "603.10g");
}

// =============================================================================================
// B. one test per look-back category (SRS 8.2)
// =============================================================================================

/// Runs an object look-back case: `matcher` (an object subject and a `{"object":"$X"}` binding
/// are added) registered on source 1 in `pre` only; the env accepts the subject only in the
/// PreEvent view (pre id). Expects one trigger with `look_back == Some(expected)` and `$X` the
/// pre id. As a control, the same registration only in `post` (subject accepted in both views)
/// gives nothing.
fn object_case(matcher: EventMatcher, event: TriggerEvent, expected: LookBackKind) {
    let matcher = bind(subj_obj(matcher), "object", "$X");
    assert_eq!(look_back_kind(&matcher), Some(expected));
    let pre = event.subject_pre.expect("pre id").get();
    let r = reg(1, P1, ability("a1", matcher));
    let env = FakeEnv::new()
        .accept(PreEvent, &[pre])
        .visible(&[pre])
        .with_pre(r.clone());
    let out = run(&env, std::slice::from_ref(&event));
    assert_eq!(out.len(), 1, "{expected:?}: {out:?}");
    let t = &out[0];
    assert_eq!(t.source, oid(1));
    assert_eq!(t.controller, P1);
    assert_eq!(t.ability_id, "a1");
    assert_eq!(t.look_back, Some(expected));
    assert_eq!(t.event_indices, vec![0]);
    assert_eq!(t.bindings.get("$X"), Some(&EventField::Object(oid(pre))));
    assert!(
        env.object_calls
            .borrow()
            .iter()
            .all(|(v, _)| *v == PreEvent),
        "the subject is checked in the PreEvent view only"
    );

    // Control: the registration exists only after the event.
    let mut ids = vec![pre];
    ids.extend(event.subject_post.map(|o| o.get()));
    let env = FakeEnv::new()
        .accept(PreEvent, &ids)
        .accept(PostEvent, &ids)
        .visible(&[pre])
        .with_post(r);
    assert!(run(&env, &[event]).is_empty(), "{expected:?}: post-only");
}

/// Runs a player look-back case (CR 603.10f, 603.10g): a players subject and a
/// `{"player":"$P"}` binding; the env accepts the player only in the PreEvent view.
fn player_case(kind: EventKind, expected: LookBackKind) {
    let matcher = bind(subj_players(m(kind)), "player", "$P");
    assert_eq!(look_back_kind(&matcher), Some(expected));
    let event = ev_player(&[kind], P2);
    let r = reg(1, P1, ability("a1", matcher));
    let env = FakeEnv::new()
        .accept_player(PreEvent, P2)
        .with_pre(r.clone());
    let out = run(&env, std::slice::from_ref(&event));
    assert_eq!(out.len(), 1, "{expected:?}: {out:?}");
    assert_eq!(out[0].look_back, Some(expected));
    assert_eq!(out[0].event_indices, vec![0]);
    assert_eq!(out[0].bindings.get("$P"), Some(&EventField::Player(P2)));

    let env = FakeEnv::new()
        .accept_player(PreEvent, P2)
        .accept_player(PostEvent, P2)
        .with_post(r);
    assert!(run(&env, &[event]).is_empty(), "{expected:?}: post-only");
}

// REQ-EVENT-002, CR 603.10a, 603.6c: a leaves-the-battlefield ability looks back in time.
#[test]
fn spec_triggers_lookback_category_leaves_battlefield() {
    let event = ev_obj(
        &[K::ZoneChange, K::LeavesBattlefield, K::Exiled],
        Some(10),
        Some(11),
    )
    .with_field("from", vzone(Zone::Battlefield))
    .with_field("to", vzone(Zone::Exile));
    object_case(
        m(K::LeavesBattlefield),
        event,
        LookBackKind::LeavesBattlefield,
    );
}

// REQ-EVENT-002, CR 603.10a: an ability that triggers when a player sacrifices a permanent
// looks back in time.
#[test]
fn spec_triggers_lookback_category_sacrificed() {
    let event = ev_obj(
        &[
            K::ZoneChange,
            K::LeavesBattlefield,
            K::Dies,
            K::PutIntoGraveyard,
            K::Sacrificed,
        ],
        Some(12),
        Some(13),
    )
    .with_field("from", vzone(Zone::Battlefield))
    .with_field("to", vzone(Zone::Graveyard));
    object_case(m(K::Sacrificed), event, LookBackKind::Sacrificed);
}

// REQ-EVENT-002, CR 603.10a: an ability that triggers when a card leaves a graveyard looks back
// in time.
#[test]
fn spec_triggers_lookback_category_leaves_graveyard() {
    let event = ev_obj(
        &[K::ZoneChange, K::LeavesGraveyard, K::Exiled],
        Some(14),
        Some(15),
    )
    .with_field("from", vzone(Zone::Graveyard))
    .with_field("to", vzone(Zone::Exile));
    object_case(m(K::LeavesGraveyard), event, LookBackKind::LeavesGraveyard);
}

// REQ-EVENT-002, CR 603.10a: an ability that triggers when an object all players can see is put
// into a hand or library looks back in time.
#[test]
fn spec_triggers_lookback_category_to_hand_or_library() {
    let event = ev_obj(&[K::ZoneChange], Some(16), Some(17))
        .with_field("from", vzone(Zone::Exile))
        .with_field("to", vzone(Zone::Library));
    object_case(
        filt(m(K::ZoneChange), "to", zone(Zone::Library)),
        event,
        LookBackKind::ToHandOrLibrary,
    );
}

// REQ-EVENT-002, CR 603.10a: an object NOT all players can see is not looked back at: the
// pre-event-only registration does not trigger, the post-event one does without look-back.
#[test]
fn spec_triggers_lookback_category_to_hand_invisible() {
    let matcher = bind(
        subj_obj(filt(m(K::ZoneChange), "to", zone(Zone::Hand))),
        "object",
        "$X",
    );
    assert_eq!(
        look_back_kind(&matcher),
        Some(LookBackKind::ToHandOrLibrary)
    );
    let event = ev_obj(&[K::ZoneChange], Some(18), Some(19))
        .with_field("from", vzone(Zone::Library))
        .with_field("to", vzone(Zone::Hand));
    let r = reg(1, P1, ability("a1", matcher));

    // Registration only pre-event, object accepted only pre-event: nothing.
    let env = FakeEnv::new().accept(PreEvent, &[18]).with_pre(r.clone());
    assert!(run(&env, std::slice::from_ref(&event)).is_empty());

    // Registration post-event, object accepted post-event: triggers, no look-back.
    let env = FakeEnv::new().accept(PostEvent, &[19]).with_post(r);
    let out = run(&env, &[event]);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].look_back, None);
    assert_eq!(
        out[0].bindings.get("$X"),
        Some(&EventField::Object(oid(19)))
    );
}

// REQ-EVENT-002, CR 603.10a: the visibility question is asked of the PRE-event object id.
#[test]
fn spec_triggers_lookback_category_to_hand_visibility_uses_pre_id() {
    let matcher = subj_obj(filt(m(K::ZoneChange), "to", zone(Zone::Hand)));
    let event = ev_obj(&[K::ZoneChange], Some(20), Some(21))
        .with_field("from", vzone(Zone::Graveyard))
        .with_field("to", vzone(Zone::Hand));
    let r = reg(1, P1, ability("a1", matcher));
    // Only the post id is "visible": the look-back path must not be taken.
    let env = FakeEnv::new()
        .accept(PreEvent, &[20])
        .visible(&[21])
        .with_pre(r.clone());
    assert!(run(&env, std::slice::from_ref(&event)).is_empty());
    assert!(
        env.visible_calls.borrow().iter().all(|o| *o == oid(20)),
        "{:?}",
        env.visible_calls.borrow()
    );
    // The pre id visible: look-back taken.
    let env = FakeEnv::new()
        .accept(PreEvent, &[20])
        .visible(&[20])
        .with_pre(r);
    let out = run(&env, &[event]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].look_back, Some(LookBackKind::ToHandOrLibrary));
}

// REQ-EVENT-002, CR 603.10b: an ability that triggers when a permanent phases out looks back.
#[test]
fn spec_triggers_lookback_category_phased_out() {
    let event = ev_obj(&[K::PhasedOut], Some(22), Some(22));
    object_case(m(K::PhasedOut), event, LookBackKind::PhasedOut);
}

// REQ-EVENT-002, CR 603.10c: an ability that triggers when an object becomes unattached looks
// back.
#[test]
fn spec_triggers_lookback_category_unattached() {
    let event = ev_obj(&[K::Unattached], Some(23), Some(23));
    object_case(m(K::Unattached), event, LookBackKind::Unattached);
}

// REQ-EVENT-002, CR 603.10d: an ability that triggers when a player loses control of an object
// looks back.
#[test]
fn spec_triggers_lookback_category_control_lost() {
    let event = ev_obj(&[K::ControlChanged], Some(24), Some(24));
    object_case(m(K::ControlChanged), event, LookBackKind::ControlLost);
}

// REQ-EVENT-002, CR 603.10e, 701.6a: an ability that triggers when a spell is countered looks
// back.
#[test]
fn spec_triggers_lookback_category_countered() {
    let event = ev_obj(
        &[K::ZoneChange, K::PutIntoGraveyard, K::Countered],
        Some(25),
        Some(26),
    )
    .with_field("from", vzone(Zone::Stack))
    .with_field("to", vzone(Zone::Graveyard));
    object_case(m(K::Countered), event, LookBackKind::Countered);
}

// REQ-EVENT-002, CR 603.10f: an ability that triggers when a player loses the game looks back.
#[test]
fn spec_triggers_lookback_category_player_lost() {
    player_case(K::PlayerLost, LookBackKind::PlayerLost);
}

// REQ-EVENT-002, CR 603.10g: an ability that triggers when a player planeswalks away from a
// plane looks back.
#[test]
fn spec_triggers_lookback_category_planeswalked() {
    player_case(K::Planeswalked, LookBackKind::Planeswalked);
}

// REQ-EVENT-002, CR 603.10, 701.6a: a plain stack-to-graveyard move is not "countered": a
// matcher over PUT_INTO_GRAVEYARD (from anywhere) uses the post-event view.
#[test]
fn spec_triggers_lookback_plain_stack_to_graveyard_not_look_back() {
    let matcher = bind(subj_obj(m(K::PutIntoGraveyard)), "object", "$X");
    assert_eq!(look_back_kind(&matcher), None);
    let event = ev_obj(&[K::ZoneChange, K::PutIntoGraveyard], Some(27), Some(28))
        .with_field("from", vzone(Zone::Stack))
        .with_field("to", vzone(Zone::Graveyard));
    let r = reg(1, P1, ability("a1", matcher));
    let env = FakeEnv::new()
        .accept(PreEvent, &[27])
        .accept(PostEvent, &[28])
        .with_post(r.clone());
    let out = run(&env, std::slice::from_ref(&event));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].look_back, None);
    assert_eq!(
        out[0].bindings.get("$X"),
        Some(&EventField::Object(oid(28)))
    );
    let env = FakeEnv::new()
        .accept(PreEvent, &[27])
        .accept(PostEvent, &[28])
        .with_pre(r);
    assert!(run(&env, &[event]).is_empty());
}

// =============================================================================================
// C. pre-event-only visibility (acceptance criterion)
// =============================================================================================

// REQ-EVENT-002, CR 603.10, 603.6c: one real battlefield-to-graveyard move, seen only in the
// pre-event state, triggers a look-back matcher (DIES) and not an unfiltered PUT_INTO_GRAVEYARD
// matcher (which never looks back): normalisation supplies both kinds and both ids.
#[test]
fn spec_triggers_lookback_pre_event_only_object() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let batch = w
        .batch(vec![GameEvent::ZoneChange {
            object: a,
            request: MoveRequest::new(ZoneKind::Graveyard),
            cause: ZoneChangeCause::Other,
        }])
        .expect("batch executes");
    let moved = match batch.events[0].outcome {
        Outcome::ZoneChange(m) => m,
        other => panic!("{other:?}"),
    };
    assert_ne!(moved.old, moved.new);
    let events = events_from_batch(&batch);
    assert_eq!(events.len(), 1);
    let event = events[0].clone();
    assert!(event.kinds.contains(&K::Dies));
    assert!(event.kinds.contains(&K::PutIntoGraveyard));
    assert_eq!(event.subject_pre, Some(moved.old));
    assert_eq!(event.subject_post, Some(moved.new));
    let (pre_id, post_id) = (moved.old.get(), moved.new.get());

    let dies = reg(1, P1, ability("dies", subj_obj(m(K::Dies))));
    let put = reg(2, P1, ability("put", subj_obj(m(K::PutIntoGraveyard))));
    let env = FakeEnv::new()
        .accept(PreEvent, &[pre_id])
        .with_both(dies.clone())
        .with_both(put.clone());
    let out = run(&env, std::slice::from_ref(&event));
    assert_eq!(summary(&out), vec![(1, "dies".to_string(), vec![0])]);
    assert_eq!(out[0].look_back, Some(LookBackKind::LeavesBattlefield));

    // Control: once the object is also seen post-event, PUT_INTO_GRAVEYARD triggers too (no
    // look-back).
    let env = FakeEnv::new()
        .accept(PreEvent, &[pre_id])
        .accept(PostEvent, &[post_id])
        .with_both(dies)
        .with_both(put);
    let out = run(&env, &[event]);
    assert_eq!(
        summary(&out),
        vec![
            (1, "dies".to_string(), vec![0]),
            (2, "put".to_string(), vec![0])
        ]
    );
    assert_eq!(out[0].look_back, Some(LookBackKind::LeavesBattlefield));
    assert_eq!(out[1].look_back, None);
}

// REQ-EVENT-002, CR 603.10: a registration existing only post-event does not trigger a
// look-back matcher; one existing only pre-event does not trigger a non-look-back matcher.
#[test]
fn spec_triggers_lookback_registration_existence() {
    let event = ev_obj(&[K::ZoneChange, K::Dies, K::Enters], Some(30), Some(31));
    let dies = reg(1, P1, ability("dies", m(K::Dies)));
    let enters = reg(2, P1, ability("enters", m(K::Enters)));

    let env = FakeEnv::new()
        .with_post(dies.clone())
        .with_pre(enters.clone());
    assert!(run(&env, std::slice::from_ref(&event)).is_empty());

    let env = FakeEnv::new().with_pre(dies).with_post(enters);
    let out = run(&env, &[event]);
    assert_eq!(
        summary(&out),
        vec![
            (1, "dies".to_string(), vec![0]),
            (2, "enters".to_string(), vec![0])
        ]
    );
    assert_eq!(out[0].look_back, Some(LookBackKind::LeavesBattlefield));
    assert_eq!(out[1].look_back, None);
}

// =============================================================================================
// D. the CR 603.10a example
// =============================================================================================

/// Source A (1) and creatures B (2), C (3) all move battlefield -> graveyard (new ids +100) in
/// one batch, in the order `order`.
fn wipe(order: &[u64]) -> Vec<TriggerEvent> {
    order.iter().map(|&o| dies_event(o, o + 100)).collect()
}

fn source_reg(b: Option<DslBatch>) -> TriggerRegistration {
    reg(
        1,
        P1,
        ability("a1", batch(bind(subj_obj(m(K::Dies)), "object", "$X"), b)),
    )
}

// REQ-EVENT-002, CR 603.10a (example), 603.2c: the artifact's ability triggers once for each
// creature even though the artifact leaves at the same time.
#[test]
fn spec_triggers_lookback_example_two_creatures() {
    let env = FakeEnv::new()
        .accept(PreEvent, &[2, 3])
        .with_pre(source_reg(None));
    let out = run(&env, &wipe(&[1, 2, 3]));
    assert_eq!(
        summary(&out),
        vec![
            (1, "a1".to_string(), vec![1]),
            (1, "a1".to_string(), vec![2])
        ]
    );
    assert_eq!(out[0].bindings.get("$X"), Some(&EventField::Object(oid(2))));
    assert_eq!(out[1].bindings.get("$X"), Some(&EventField::Object(oid(3))));
    for t in &out {
        assert_eq!(t.look_back, Some(LookBackKind::LeavesBattlefield));
        assert_eq!(t.controller, P1);
    }
}

// REQ-EVENT-002, CR 603.10a, REQ-TEST-001 "look-back": the batch [B, C] and [C, B] both give
// triggers in event order.
#[test]
fn spec_triggers_lookback_example_both_event_orders() {
    let env = FakeEnv::new()
        .accept(PreEvent, &[2, 3])
        .with_pre(source_reg(None));
    let bc = run(&env, &wipe(&[1, 2, 3]));
    let cb = run(&env, &wipe(&[1, 3, 2]));
    let bound = |ts: &[fineprint_core::triggers::WaitingTrigger]| {
        ts.iter()
            .map(|t| (t.event_indices.clone(), t.bindings.get("$X").cloned()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        bound(&bc),
        vec![
            (vec![1], Some(EventField::Object(oid(2)))),
            (vec![2], Some(EventField::Object(oid(3))))
        ]
    );
    assert_eq!(
        bound(&cb),
        vec![
            (vec![1], Some(EventField::Object(oid(3)))),
            (vec![2], Some(EventField::Object(oid(2))))
        ]
    );
    // Source last in the batch: still two triggers, still event order.
    let last = run(&env, &wipe(&[3, 2, 1]));
    assert_eq!(
        bound(&last),
        vec![
            (vec![0], Some(EventField::Object(oid(3)))),
            (vec![1], Some(EventField::Object(oid(2))))
        ]
    );
}

// REQ-EVENT-002, CR 603.10a, REQ-TEST-001 "look-back": registrations listed in either order give
// the same output.
#[test]
fn spec_triggers_lookback_example_both_registration_orders() {
    let other = reg(
        4,
        P2,
        ability("b1", bind(subj_obj(m(K::Dies)), "object", "$Y")),
    );
    let env1 = FakeEnv::new()
        .accept(PreEvent, &[2, 3])
        .with_pre(source_reg(None))
        .with_pre(other.clone());
    let env2 = FakeEnv::new()
        .accept(PreEvent, &[2, 3])
        .with_pre(other)
        .with_pre(source_reg(None));
    let events = wipe(&[1, 2, 3]);
    let a = run(&env1, &events);
    let b = run(&env2, &events);
    assert_eq!(a, b);
    assert_eq!(
        summary(&a),
        vec![
            (1, "a1".to_string(), vec![1]),
            (4, "b1".to_string(), vec![1]),
            (1, "a1".to_string(), vec![2]),
            (4, "b1".to_string(), vec![2]),
        ]
    );
}

// REQ-EVENT-002, CR 603.10a: when the matcher accepts every creature-like object, the source
// sees itself die too.
#[test]
fn spec_triggers_lookback_example_source_sees_itself() {
    let env = FakeEnv::new()
        .accept(PreEvent, &[1, 2, 3])
        .with_pre(source_reg(None));
    let out = run(&env, &wipe(&[1, 2, 3]));
    assert_eq!(
        summary(&out),
        vec![
            (1, "a1".to_string(), vec![0]),
            (1, "a1".to_string(), vec![1]),
            (1, "a1".to_string(), vec![2])
        ]
    );
    assert_eq!(out[0].bindings.get("$X"), Some(&EventField::Object(oid(1))));
}

// REQ-EVENT-002, CR 603.10a, 603.2c: a ONE_OR_MORE look-back ability triggers once for the
// batch, naming every matching event in ascending order.
#[test]
fn spec_triggers_lookback_example_one_or_more() {
    let env = FakeEnv::new()
        .accept(PreEvent, &[2, 3])
        .with_pre(source_reg(Some(DslBatch::OneOrMore)));
    let out = run(&env, &wipe(&[1, 2, 3]));
    assert_eq!(summary(&out), vec![(1, "a1".to_string(), vec![1, 2])]);
    assert_eq!(out[0].look_back, Some(LookBackKind::LeavesBattlefield));
    assert_eq!(out[0].bindings.get("$X"), Some(&EventField::Object(oid(2))));

    let out = run(&env, &wipe(&[3, 1, 2]));
    assert_eq!(summary(&out), vec![(1, "a1".to_string(), vec![0, 2])]);
    assert_eq!(out[0].bindings.get("$X"), Some(&EventField::Object(oid(3))));
}

// REQ-EVENT-002, CR 701.8a, 701.8b, 603.10a: a destroy-all: the observer is destroyed together with
// another permanent. A DESTROYED matcher on the observer triggers (it looks back as a
// leaves-the-battlefield ability), from a registration that exists only in the pre-event list.
#[test]
fn spec_triggers_lookback_destroy_all_observer() {
    // Observer 1 and permanent 2 are destroyed together (new ids +100).
    let destroyed =
        |pre: u64| dies_event(pre, pre + 100).with_object(Some(oid(pre)), Some(oid(pre + 100)));
    let mut events: Vec<TriggerEvent> = [1, 2].into_iter().map(destroyed).collect();
    for e in &mut events {
        e.kinds.push(K::Destroyed);
    }
    let mm = bind(subj_obj(m(K::Destroyed)), "object", "$X");
    assert_eq!(look_back_kind(&mm), Some(LookBackKind::LeavesBattlefield));
    let observer = reg(9, P1, ability("obs", mm));
    let env = FakeEnv::new()
        .accept(PreEvent, &[1, 2])
        .with_pre(observer.clone());
    let out = run(&env, &events);
    assert_eq!(
        summary(&out),
        vec![
            (9, "obs".to_string(), vec![0]),
            (9, "obs".to_string(), vec![1])
        ]
    );
    for t in &out {
        assert_eq!(t.look_back, Some(LookBackKind::LeavesBattlefield));
        assert_eq!(t.controller, P1);
    }
    assert_eq!(out[0].bindings.get("$X"), Some(&EventField::Object(oid(1))));
    assert_eq!(out[1].bindings.get("$X"), Some(&EventField::Object(oid(2))));

    // The observer itself, listed only in the pre-event state, is among the destroyed.
    let own = reg(1, P1, ability("self", subj_obj(m(K::Destroyed))));
    let env = FakeEnv::new().accept(PreEvent, &[1]).with_pre(own.clone());
    let out = run(&env, &events);
    assert_eq!(summary(&out), vec![(1, "self".to_string(), vec![0])]);
    assert_eq!(out[0].look_back, Some(LookBackKind::LeavesBattlefield));

    // Control: a post-event-only registration does not trigger.
    let env = FakeEnv::new()
        .accept(PreEvent, &[1, 2])
        .accept(PostEvent, &[101, 102])
        .with_post(own);
    assert!(run(&env, &events).is_empty());
}
