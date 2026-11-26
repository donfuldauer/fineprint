// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests for LKI with layer-evaluated characteristics (#184 FP-145): REQ-LAYER-001 (rules
// consumers see the characteristics evaluated through layers 1a-7d, CR 613.1-613.4) and
// REQ-CORE-003 (LKI holds the evaluated characteristics as of immediately before the event,
// CR 608.2h, CR 704.8). The provider is a `LayeredProvider` over a snapshot of the game state
// taken before the context is built (the documented recipe); a base-only provider is the
// control. Every scenario runs in both timestamp orders of the two layer-7b effects
// (CR 613.7). Synthetic cards only (`test.lki_granted_type_2_2`).

use fineprint_base::encode::encode_to_vec;
use fineprint_base::hash::{Digest, leaf_digest};

use super::*;
use crate::choice::{ChoiceContext, ChoiceSession, Interrupt, Status};
use crate::layers::LayeredProvider;
use crate::memory::LkiRecord;
use crate::memory::consumer_guard_tests::{
    SceneWorld, has_granted_type, pt_of, register_scene, scene_base,
};
use crate::replacement::ReplacementRegistry;
use crate::sba::{SbaOutcome, SbaRegistry, StoredSbaEnvironment, perform_all};
use crate::state::GameEnvironment;
use crate::txn::Transactional;

/// Which provider the context gets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    /// `LayeredProvider` over a snapshot of the state (the evaluated view).
    Layered,
    /// The scene world's base characteristics (the control).
    Base,
}

/// Executes `event` on `w` with the provider `source`, built over a snapshot taken before the
/// context (CR 603.10a, CR 704.8: the pre-event state).
fn exec_with(w: &mut World, source: Source, event: GameEvent) -> Option<ExecutedEvent> {
    let world = SceneWorld::of(&w.zones, w.events.markers());
    let snap = w.game.clone();
    let env = GameEnvironment::new(&snap, &world);
    let layered = LayeredProvider::new(snap.effects(), &env);
    let provider: &dyn CharacteristicsProvider = match source {
        Source::Layered => &layered,
        Source::Base => &world,
    };
    let mut ctx = EventContext {
        game: &mut w.game,
        zones: &mut w.zones,
        ids: &mut w.ids,
        permanents: &mut w.permanents,
        memories: &mut w.memories,
        lki: &mut w.lki,
        events: &mut w.events,
        provider,
    };
    ctx.execute(event).unwrap()
}

fn add_counters(object: ObjectId, kind: &str, amount: i64) -> GameEvent {
    GameEvent::AddCounters {
        target: CounterTarget::Object(object),
        kind: kind.to_owned(),
        amount,
    }
}

/// A battlefield creature with one +1/+1 counter and two neutral counters, the scene's effects
/// in the order `a_older`, destroyed through `EventContext::execute` with the provider
/// `source`. Returns its LKI record, the evaluated P/T the scene gives it before counters, and
/// its counters in the object markers just before the move.
fn destroyed(a_older: bool, source: Source) -> (LkiRecord, (i64, i64), BTreeMap<String, i64>) {
    let mut w = World::new();
    let id = w.create(1, 1, ZoneKind::Battlefield);
    w.exec(add_counters(id, "+1/+1", 1)).unwrap();
    w.exec(add_counters(id, "test.charge", 2)).unwrap();
    let pt = register_scene(&mut w.game, id, a_older);
    let markers = w.events.markers().counters(id);
    let executed = exec_with(&mut w, source, destroy(id)).expect("the move occurs");
    assert!(executed.lki_captured);
    (w.lki.get(id).expect("LKI captured").clone(), pt, markers)
}

// REQ-LAYER-001, REQ-CORE-003: CR 608.2h, CR 613.1d, CR 613.4b, CR 613.4c, CR 613.7. A
// zone change executed with the layered provider captures the evaluated characteristics: the
// layer-4 granted type and the layer-7b/7c P/T (the later 7b timestamp wins, in both orders,
// plus the +1/+1 counter). The base-only control captures the base 2/2 without the granted
// type, so the test discriminates. Counters equal the object markers either way (existing
// decision: LKI counters come only from `ObjectMarkers`).
#[test]
fn req_layer_001_req_core_003_cr_608_2h_zone_change_lki_is_evaluated() {
    let mut seen = Vec::new();
    for a_older in [true, false] {
        let (record, (p, t), markers) = destroyed(a_older, Source::Layered);
        let c = record.characteristics();
        assert!(has_granted_type(c), "a_older {a_older}: granted type");
        assert!(c.card_types.contains("Creature") && c.subtypes.contains("Testkin"));
        assert_eq!(pt_of(c), Some((p + 1, t + 1)), "a_older {a_older}: P/T");
        assert_eq!(record.counters(), &markers);
        assert_eq!(
            markers,
            BTreeMap::from([("+1/+1".to_owned(), 1), ("test.charge".to_owned(), 2)])
        );
        seen.push(pt_of(c));

        let (control, _, markers) = destroyed(a_older, Source::Base);
        assert_eq!(control.characteristics(), &scene_base());
        assert!(!has_granted_type(control.characteristics()));
        assert_eq!(pt_of(control.characteristics()), Some((2, 2)));
        assert_eq!(control.counters(), &markers);
    }
    // The two timestamp orders give different LKI values: B (6/6) wins when it is later, A
    // (4/4) otherwise, plus 7c +1/+1 and the counter.
    assert_eq!(seen, [Some((8, 8)), Some((6, 6))]);
}

// REQ-LAYER-001, REQ-CORE-003: CR 704.8, CR 608.2h. A batch of simultaneous moves captures
// every departing object from the one pre-batch snapshot, evaluated.
#[test]
fn req_layer_001_req_core_003_cr_704_8_batch_lki_is_evaluated() {
    for a_older in [true, false] {
        let mut w = World::new();
        let x = w.create(1, 1, ZoneKind::Battlefield);
        let y = w.create(2, 2, ZoneKind::Battlefield);
        let (p, t) = register_scene(&mut w.game, x, a_older);
        let world = SceneWorld::of(&w.zones, w.events.markers());
        let snap = w.game.clone();
        let env = GameEnvironment::new(&snap, &world);
        let provider = LayeredProvider::new(snap.effects(), &env);
        let mut ctx = EventContext {
            game: &mut w.game,
            zones: &mut w.zones,
            ids: &mut w.ids,
            permanents: &mut w.permanents,
            memories: &mut w.memories,
            lki: &mut w.lki,
            events: &mut w.events,
            provider: &provider,
        };
        ctx.execute_batch(&EventBatch::from_events(vec![destroy(x), destroy(y)]))
            .unwrap();
        let rx = w.lki.get(x).unwrap().characteristics();
        assert!(has_granted_type(rx));
        assert_eq!(pt_of(rx), Some((p, t)));
        // The other object has no effect on it: its base, evaluated.
        assert_eq!(w.lki.get(y).unwrap().characteristics(), &scene_base());
    }
}

// ---------------------------------------------------------------------------------------------
// State-based actions: a ceasing token (through the public SBA API)
// ---------------------------------------------------------------------------------------------

impl Transactional for World {
    fn state_hash(&self) -> Digest {
        let mut bytes = encode_to_vec(&self.game.state_hash());
        bytes.extend(encode_to_vec(&self.ids.last_issued()));
        bytes.extend(encode_to_vec(&self.zones));
        bytes.extend(encode_to_vec(&self.permanents));
        bytes.extend(encode_to_vec(&self.memories));
        bytes.extend(encode_to_vec(&self.lki));
        bytes.extend(encode_to_vec(&self.events));
        leaf_digest(&bytes)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        None
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// One state-based-action pass over `w` with the provider `source`, the layer inputs and the
/// provider built over one snapshot taken before the pass (CR 704.3, CR 704.8).
fn sba_pass(
    w: &mut World,
    source: Source,
    ch: &mut ChoiceContext<'_>,
) -> Result<SbaOutcome, Interrupt> {
    let world = SceneWorld::of(&w.zones, w.events.markers());
    let snap = w.game.clone();
    let env = GameEnvironment::new(&snap, &world);
    let layered = LayeredProvider::new(snap.effects(), &env);
    let provider: &dyn CharacteristicsProvider = match source {
        Source::Layered => &layered,
        Source::Base => &world,
    };
    let mut ctx = EventContext {
        game: &mut w.game,
        zones: &mut w.zones,
        ids: &mut w.ids,
        permanents: &mut w.permanents,
        memories: &mut w.memories,
        lki: &mut w.lki,
        events: &mut w.events,
        provider,
    };
    let mut replacements = ReplacementRegistry::new();
    let mut sba_env = StoredSbaEnvironment::default();
    Ok(perform_all(
        &SbaRegistry::standard(),
        &mut ctx,
        &mut replacements,
        snap.effects(),
        &env,
        &mut sba_env,
        ch,
    )?)
}

/// A token created on the battlefield and put into the graveyard, with the scene's effects on
/// its graveyard object in the order `a_older`, then one SBA pass with the provider `source`.
/// Returns the ceased object's LKI record and the scene's P/T.
fn ceased(a_older: bool, source: Source) -> (LkiRecord, (i64, i64)) {
    let mut w = World::new();
    let card = c(50);
    w.exec(GameEvent::CreateToken {
        owner: p(1),
        card,
        controller: None,
    })
    .unwrap();
    let token = w.zones.object_of_card(card).unwrap();
    w.exec(zc(token, ZoneKind::Graveyard)).unwrap();
    let gy = w.zones.object_of_card(card).unwrap();
    assert_ne!(gy, token);
    w.exec(add_counters(gy, "test.charge", 3)).unwrap();
    let pt = register_scene(&mut w.game, gy, a_older);
    let markers = w.events.markers().counters(gy);
    let procedure = move |s: &mut World, ch: &mut ChoiceContext<'_>| sba_pass(s, source, ch);
    let session = ChoiceSession::start(w, procedure).unwrap();
    let Status::Done { state, output } = session.status() else {
        panic!("the pass asks nothing");
    };
    // CR 704.5d: the token in the graveyard ceases to exist.
    assert_eq!(output.ceased, [gy]);
    assert!(state.zones.object(gy).is_none());
    let record = state.lki.get(gy).expect("LKI captured").clone();
    assert_eq!(record.counters(), &markers);
    (record, pt)
}

// REQ-LAYER-001, REQ-CORE-003: CR 704.5d, CR 704.8, CR 608.2h, CR 613.1d, CR 613.4b,
// CR 613.7. The LKI of a token that ceases to exist (the SBA ceasing capture, `CeasingLki`)
// holds the evaluated characteristics from the context's layered provider, in both timestamp
// orders; the base-only control holds the base.
#[test]
fn req_layer_001_req_core_003_cr_704_5d_ceasing_token_lki_is_evaluated() {
    let mut seen = Vec::new();
    for a_older in [true, false] {
        let (record, pt) = ceased(a_older, Source::Layered);
        assert!(has_granted_type(record.characteristics()));
        assert_eq!(pt_of(record.characteristics()), Some(pt));
        seen.push(pt);

        let (control, _) = ceased(a_older, Source::Base);
        assert_eq!(control.characteristics(), &scene_base());
    }
    assert_eq!(seen, [(7, 7), (5, 5)]);
}
