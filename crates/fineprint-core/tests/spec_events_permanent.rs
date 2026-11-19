// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the permanent events: Tap/Untap (CR 701.26a, 701.26b), Attach/Unattach
//! (CR 701.3a-d), BecomeCopy (CR 707.2, 707.4, 613.1a) and TurnFaceUp/TurnFaceDown
//! (CR 701.27b, 708.8) (REQ-EVENT-001). Black-box: written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_core::events::{EventError, GameEvent, Outcome};
use fineprint_core::permanent::AttachTarget;
use fineprint_core::zones::ZoneKind;
use support::*;

fn tapped(w: &World, id: fineprint_base::ids::ObjectId) -> bool {
    w.permanents.get(id).expect("held").status().tapped
}

fn face_down(w: &World, id: fineprint_base::ids::ObjectId) -> bool {
    w.permanents.get(id).expect("held").status().face_down
}

fn attached_ts(outcome: &Outcome) -> fineprint_base::ids::Timestamp {
    match outcome {
        Outcome::Attached { timestamp } => *timestamp,
        other => panic!("expected Attached outcome, got {other:?}"),
    }
}

// ----- tap / untap ----------------------------------------------------------------------------

// REQ-EVENT-001, CR 701.26a, 701.26b: tap and untap change the status.
#[test]
fn tap_and_untap_change_status() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    assert!(!tapped(&w, a));
    assert_eq!(w.ok(GameEvent::Tap { object: a }).outcome, Outcome::Applied);
    assert!(tapped(&w, a));
    assert_eq!(
        w.ok(GameEvent::Untap { object: a }).outcome,
        Outcome::Applied
    );
    assert!(!tapped(&w, a));
}

// REQ-EVENT-001, CR 701.26a, 701.26b: tapping a tapped / untapping an untapped permanent is no
// event and changes nothing.
#[test]
fn tap_tapped_and_untap_untapped_are_no_events() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let before = w.snap();
    assert_eq!(w.exec(GameEvent::Untap { object: a }), Ok(None));
    assert_eq!(w.snap(), before);
    w.ok(GameEvent::Tap { object: a });
    let before = w.snap();
    assert_eq!(w.exec(GameEvent::Tap { object: a }), Ok(None));
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 701.26a: only permanents tap: NotOnBattlefield for an object in another
// zone, UnknownObject for an object in no zone.
#[test]
fn tap_errors() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P1);
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::Tap { object: g }),
        Err(EventError::NotOnBattlefield(g))
    );
    assert_eq!(
        w.exec(GameEvent::Untap { object: g }),
        Err(EventError::NotOnBattlefield(g))
    );
    assert_eq!(
        w.exec(GameEvent::Tap { object: GHOST }),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(
        w.exec(GameEvent::Untap { object: GHOST }),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(w.snap(), before);
}

// ----- attach / unattach ----------------------------------------------------------------------

// REQ-EVENT-001, CR 701.3a, 701.3c: attaching attaches and issues a new timestamp from the
// game; moving to a different host issues a strictly later one each time.
#[test]
fn attach_issues_new_timestamps() {
    let mut w = World::new();
    let host1 = w.create(ZoneKind::Battlefield, P1);
    let host2 = w.create(ZoneKind::Battlefield, P2);
    let aura = w.create(ZoneKind::Battlefield, P1);

    let t1 = attached_ts(
        &w.ok(GameEvent::Attach {
            attachment: aura,
            target: AttachTarget::Permanent(host1),
        })
        .outcome,
    );
    assert_eq!(
        w.permanents.attached_to(aura),
        Some(AttachTarget::Permanent(host1))
    );
    let t2 = attached_ts(
        &w.ok(GameEvent::Attach {
            attachment: aura,
            target: AttachTarget::Permanent(host2),
        })
        .outcome,
    );
    assert_eq!(
        w.permanents.attached_to(aura),
        Some(AttachTarget::Permanent(host2))
    );
    let t3 = attached_ts(
        &w.ok(GameEvent::Attach {
            attachment: aura,
            target: AttachTarget::Player(P2),
        })
        .outcome,
    );
    assert_eq!(
        w.permanents.attached_to(aura),
        Some(AttachTarget::Player(P2))
    );
    assert!(t1 < t2 && t2 < t3, "{t1:?} < {t2:?} < {t3:?}");
    // The timestamp comes from the game's generator: the next one is later still.
    let next = w.game.issue_timestamp().unwrap();
    assert!(t3 < next);
}

// REQ-EVENT-001, CR 701.3b: attaching to the host it already has does nothing.
#[test]
fn reattach_to_same_host_is_no_event() {
    let mut w = World::new();
    let host = w.create(ZoneKind::Battlefield, P1);
    let aura = w.create(ZoneKind::Battlefield, P1);
    w.ok(GameEvent::Attach {
        attachment: aura,
        target: AttachTarget::Permanent(host),
    });
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::Attach {
            attachment: aura,
            target: AttachTarget::Permanent(host),
        }),
        Ok(None)
    );
    assert_eq!(w.snap(), before);
    w.ok(GameEvent::Attach {
        attachment: aura,
        target: AttachTarget::Player(P1),
    });
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::Attach {
            attachment: aura,
            target: AttachTarget::Player(P1),
        }),
        Ok(None)
    );
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001: an attachment cycle is EventError::Permanent and nothing changes (no
// timestamp consumed).
#[test]
fn attach_cycle_fails_cleanly() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P1);
    w.ok(GameEvent::Attach {
        attachment: b,
        target: AttachTarget::Permanent(a),
    });
    let before = w.snap();
    let r = w.exec(GameEvent::Attach {
        attachment: a,
        target: AttachTarget::Permanent(b),
    });
    assert!(matches!(r, Err(EventError::Permanent(_))), "{r:?}");
    let r = w.exec(GameEvent::Attach {
        attachment: a,
        target: AttachTarget::Permanent(a),
    });
    assert!(matches!(r, Err(EventError::Permanent(_))), "{r:?}");
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 701.3a: the attachment and a permanent target must be on the battlefield;
// a player target must exist.
#[test]
fn attach_errors() {
    let mut w = World::new();
    let host = w.create(ZoneKind::Battlefield, P1);
    let aura = w.create(ZoneKind::Battlefield, P1);
    let g = w.create(ZoneKind::Graveyard, P1);
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::Attach {
            attachment: g,
            target: AttachTarget::Permanent(host),
        }),
        Err(EventError::NotOnBattlefield(g))
    );
    assert_eq!(
        w.exec(GameEvent::Attach {
            attachment: aura,
            target: AttachTarget::Permanent(g),
        }),
        Err(EventError::NotOnBattlefield(g))
    );
    assert_eq!(
        w.exec(GameEvent::Attach {
            attachment: GHOST,
            target: AttachTarget::Permanent(host),
        }),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(
        w.exec(GameEvent::Attach {
            attachment: aura,
            target: AttachTarget::Permanent(GHOST),
        }),
        Err(EventError::UnknownObject(GHOST))
    );
    let r = w.exec(GameEvent::Attach {
        attachment: aura,
        target: AttachTarget::Player(NOBODY),
    });
    assert!(r.is_err(), "{r:?}");
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 701.3d: unattach detaches; unattaching an unattached object is no event.
#[test]
fn unattach() {
    let mut w = World::new();
    let host = w.create(ZoneKind::Battlefield, P1);
    let aura = w.create(ZoneKind::Battlefield, P1);
    let before = w.snap();
    assert_eq!(w.exec(GameEvent::Unattach { attachment: aura }), Ok(None));
    assert_eq!(w.snap(), before);
    w.ok(GameEvent::Attach {
        attachment: aura,
        target: AttachTarget::Permanent(host),
    });
    assert_eq!(
        w.ok(GameEvent::Unattach { attachment: aura }).outcome,
        Outcome::Applied
    );
    assert_eq!(w.permanents.attached_to(aura), None);
    let before = w.snap();
    assert_eq!(w.exec(GameEvent::Unattach { attachment: aura }), Ok(None));
    assert_eq!(w.snap(), before);
}

// ----- become copy ----------------------------------------------------------------------------

// REQ-EVENT-001, CR 707.2, 707.4, 613.1a: BecomeCopy records copy_of; no zone or permanent
// change; repeating it is no event; copying something else replaces the link.
#[test]
fn become_copy_sets_copy_of() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let g = w.create(ZoneKind::Graveyard, P2);
    let c = w.create(ZoneKind::Battlefield, P2);
    let zones = bytes(&w.zones);
    let perms = bytes(&w.permanents);
    let ids = w.ids.clone();
    assert_eq!(w.events.markers().copy_of(a), None);
    assert_eq!(
        w.ok(GameEvent::BecomeCopy {
            object: a,
            copied: g
        })
        .outcome,
        Outcome::Applied
    );
    assert_eq!(w.events.markers().copy_of(a), Some(g));
    // CR 707.4: no zone change, no permanent change, no new id.
    assert_eq!(bytes(&w.zones), zones);
    assert_eq!(bytes(&w.permanents), perms);
    assert_eq!(w.ids, ids);

    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::BecomeCopy {
            object: a,
            copied: g
        }),
        Ok(None)
    );
    assert_eq!(w.snap(), before);

    w.ok(GameEvent::BecomeCopy {
        object: a,
        copied: c,
    });
    assert_eq!(w.events.markers().copy_of(a), Some(c));
    assert_eq!(w.events.markers().copy_of(c), None);
}

// REQ-EVENT-001: the copying object must be a permanent; the copied object must exist.
#[test]
fn become_copy_errors() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let g = w.create(ZoneKind::Graveyard, P2);
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::BecomeCopy {
            object: g,
            copied: a
        }),
        Err(EventError::NotOnBattlefield(g))
    );
    assert_eq!(
        w.exec(GameEvent::BecomeCopy {
            object: a,
            copied: GHOST
        }),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(w.snap(), before);
}

// ----- face up / face down --------------------------------------------------------------------

// REQ-EVENT-001, CR 701.27b, 708.8: TurnFaceDown / TurnFaceUp change the status on the same
// object identity; repeating is no event.
#[test]
fn turn_face_down_and_up() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let ids = w.ids.clone();
    let before = w.snap();
    assert_eq!(w.exec(GameEvent::TurnFaceUp { object: a }), Ok(None));
    assert_eq!(w.snap(), before);

    assert_eq!(
        w.ok(GameEvent::TurnFaceDown { object: a }).outcome,
        Outcome::Applied
    );
    assert!(face_down(&w, a));
    assert!(w.zones.object(a).is_some(), "same identity");
    let before = w.snap();
    assert_eq!(w.exec(GameEvent::TurnFaceDown { object: a }), Ok(None));
    assert_eq!(w.snap(), before);

    assert_eq!(
        w.ok(GameEvent::TurnFaceUp { object: a }).outcome,
        Outcome::Applied
    );
    assert!(!face_down(&w, a));
    assert!(w.zones.object(a).is_some(), "same identity");
    assert_eq!(w.ids, ids, "no new object id issued");
}

// REQ-EVENT-001: face events need a permanent.
#[test]
fn face_events_need_a_permanent() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P1);
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::TurnFaceDown { object: g }),
        Err(EventError::NotOnBattlefield(g))
    );
    assert_eq!(
        w.exec(GameEvent::TurnFaceUp { object: g }),
        Err(EventError::NotOnBattlefield(g))
    );
    assert_eq!(w.snap(), before);
}
