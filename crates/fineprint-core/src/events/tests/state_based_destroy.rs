// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the state-based destruction cause (#209 FP-155): REQ-EVENT-001, REQ-SBA-001,
// REQ-CORE-006. `ZoneChangeCause::StateBasedDestroy` is the cause of a CR 704.5g / CR 704.5h
// move: the object is "destroyed" (CR 701.8b), but not as the result of an effect, so the
// shield counter's destroy replacement (CR 122.1c) does not apply to it. Synthetic ids and
// synthetic characteristics only.

use fineprint_base::encode::encode_to_vec;
use fineprint_base::hash::{Digest, leaf_digest};

use super::*;
use crate::choice::{ChoiceContext, ChoiceSession, Interrupt, Status};
use crate::replacement::{
    BusError, BusInterrupt, BusReport, ReplacementEffect, ReplacementRegistry, process_batch,
};
use crate::txn::Transactional;

/// Every cause with its documented canonical tag (the doc comment on [`ZoneChangeCause`]).
const TAGS: [(ZoneChangeCause, u8); 9] = [
    (ZoneChangeCause::Other, 0),
    (ZoneChangeCause::Destroy, 1),
    (ZoneChangeCause::Sacrifice, 2),
    (ZoneChangeCause::Discard, 3),
    (ZoneChangeCause::Mill, 4),
    (ZoneChangeCause::Exile, 5),
    (ZoneChangeCause::CounterSpell, 6),
    (ZoneChangeCause::StateBasedAction, 7),
    (ZoneChangeCause::StateBasedDestroy, 8),
];

// REQ-EVENT-001, REQ-SBA-001, REQ-CORE-006: CR 701.8b. `StateBasedDestroy` encodes as the
// one-byte tag 8; the tags 0-7 of the existing causes are unchanged; every tag is distinct.
#[test]
fn req_event_001_req_sba_001_cr_701_8b_state_based_destroy_tag() {
    let mut seen = BTreeSet::new();
    for (cause, tag) in TAGS {
        assert_eq!(encode_to_vec(&cause), vec![tag], "{cause:?}");
        assert!(seen.insert(tag), "{cause:?} shares tag {tag}");
    }
    assert_eq!(
        encode_to_vec(&ZoneChangeCause::StateBasedDestroy),
        vec![8_u8]
    );
    assert_ne!(
        ZoneChangeCause::StateBasedDestroy,
        ZoneChangeCause::StateBasedAction
    );
    assert_ne!(ZoneChangeCause::StateBasedDestroy, ZoneChangeCause::Destroy);
    // A zone-change event carrying the cause differs from the same event with tag 7 or 1 only
    // in its last byte (the cause is the event's last field).
    let event = |cause| GameEvent::ZoneChange {
        object: ObjectId::new(1),
        request: MoveRequest::new(ZoneKind::Graveyard),
        cause,
    };
    let sbd = encode_to_vec(&event(ZoneChangeCause::StateBasedDestroy));
    for other in [ZoneChangeCause::StateBasedAction, ZoneChangeCause::Destroy] {
        let bytes = encode_to_vec(&event(other));
        assert_eq!(bytes.len(), sbd.len());
        assert_eq!(bytes[..bytes.len() - 1], sbd[..sbd.len() - 1]);
        assert_ne!(bytes, sbd);
    }
}

/// The events world with a replacement registry, for running the bus in a choice session.
#[derive(Clone)]
struct Bused {
    w: World,
    registry: ReplacementRegistry,
}

impl Transactional for Bused {
    fn state_hash(&self) -> Digest {
        let mut bytes = encode_to_vec(&self.w.game.state_hash());
        bytes.extend(encode_to_vec(&self.w.zones));
        bytes.extend(encode_to_vec(&self.w.permanents));
        bytes.extend(encode_to_vec(&self.w.events));
        bytes.extend(encode_to_vec(&self.registry));
        leaf_digest(&bytes)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        None
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// Runs `events` as one batch through the bus (`sba` as given) on `b`; the run asks nothing.
fn run_bus(b: Bused, events: Vec<GameEvent>, sba: bool) -> (Bused, Result<BusReport, BusError>) {
    let batch = EventBatch::from_events(events);
    let procedure = move |s: &mut Bused, ch: &mut ChoiceContext<'_>| {
        let Bused { w, registry } = s;
        match process_batch(&mut w.ctx(), registry, &batch, sba, ch) {
            Ok(r) => Ok(Ok(r)),
            Err(BusInterrupt::Pending(r)) => Err(Interrupt::Pending(r)),
            Err(BusInterrupt::Error(e)) => Ok(Err(e)),
        }
    };
    let session = ChoiceSession::start(b, procedure).unwrap();
    let Status::Done { state, output } = session.status() else {
        panic!("the bus asks nothing here");
    };
    (state.clone(), output.clone())
}

/// A battlefield permanent of player 1 with one shield counter and the shield counter's
/// destroy replacement. Returns the world and the permanent.
fn shielded() -> (Bused, ObjectId) {
    let mut w = World::new();
    let x = w.create(1, 1, ZoneKind::Battlefield);
    w.exec(GameEvent::AddCounters {
        target: CounterTarget::Object(x),
        kind: "shield".to_owned(),
        amount: 1,
    })
    .unwrap();
    let mut registry = ReplacementRegistry::new();
    registry
        .add(ReplacementEffect::shield_counter_destroy(e(1), x))
        .unwrap();
    (Bused { w, registry }, x)
}

fn move_with(object: ObjectId, cause: ZoneChangeCause) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(ZoneKind::Graveyard),
        cause,
    }
}

// REQ-EVENT-001, REQ-SBA-001: CR 122.1c, CR 701.8b. The shield counter replaces destruction as
// the result of an effect only: a `StateBasedDestroy` move (the CR 704.5g / CR 704.5h SBA
// batch, `sba` true) is not replaced, the permanent goes to the graveyard and the counter is
// never removed. The control: the same permanent destroyed with cause `Destroy` keeps its place
// and loses the counter.
#[test]
fn req_event_001_cr_122_1c_shield_does_not_stop_state_based_destroy() {
    let (b, x) = shielded();
    for sba in [true, false] {
        let (after, out) = run_bus(
            b.clone(),
            vec![move_with(x, ZoneChangeCause::StateBasedDestroy)],
            sba,
        );
        let report = out.expect("the bus succeeds");
        assert!(report.applications.is_empty(), "sba {sba}");
        assert_eq!(report.executed.len(), 1);
        assert_eq!(report.executed[0].events.len(), 1);
        let ev = &report.executed[0].events[0];
        assert_eq!(
            ev.instance.event(),
            &move_with(x, ZoneChangeCause::StateBasedDestroy)
        );
        let m = moved_of(ev);
        assert_eq!(
            (m.from, m.to),
            (ZoneId::Battlefield, ZoneId::Graveyard(p(1)))
        );
        assert!(after.w.permanents.get(x).is_none());
        // The effect is still registered: it was never applied.
        assert_eq!(after.registry, b.registry);
    }
    // Control (CR 122.1c): destruction by an effect is replaced.
    let (after, out) = run_bus(
        b.clone(),
        vec![move_with(x, ZoneChangeCause::Destroy)],
        false,
    );
    let report = out.expect("the bus succeeds");
    assert_eq!(report.applications, vec![e(1)]);
    assert!(after.w.permanents.get(x).is_some());
    assert_eq!(after.w.events.markers().counter(x, "shield"), 0);
}
