// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the state-based action registry and pass: REQ-SBA-001 (CR 704.3, CR 704.5a-i,
// CR 704.7, CR 704.8), REQ-TURN-001 step 1 (one pass), REQ-CORE-001 (determinism). Synthetic
// cards only (`test.*` names); no card data.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::encode_to_vec;
use fineprint_base::hash::{Digest, leaf_digest};
use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::ObjectSelectorAST;

use super::*;
use crate::choice::{ChoiceContext, ChoiceResponse, ChoiceSession, Interrupt, Status};
use crate::events::{
    CounterTarget, DamageTarget, EventContext, EventState, GameEvent, Outcome, ZoneChangeCause,
};
use crate::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerEnvironment,
    LayerModification, MatchContext, ObjectBase, literal,
};
use crate::memory::{CharacteristicsProvider, LkiStore, MemoryTable};
use crate::objects::Characteristics;
use crate::permanent::Permanents;
use crate::replacement::{
    Action, BusError, BusEventKind, BusInterrupt, EventMatcher, Modification, ReplacementEffect,
    ReplacementRegistry, Shield,
};
use crate::state::{DesignationValue, GameState};
use crate::txn::Transactional;
use crate::zones::{MoveRequest, ZoneId, ZoneKind, Zones, ZonesConfig};

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A synthetic creature `name` with the given toughness (power 1).
fn creature(name: &str, toughness: i64) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
        power: Some(literal(1)),
        toughness: Some(literal(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A synthetic noncreature permanent of `card_type` without P/T.
fn noncreature(name: &str, card_type: &str) -> Characteristics {
    let mut c = creature(name, 0);
    c.card_types = set(&[card_type]);
    c.power = None;
    c.toughness = None;
    c
}

fn keyword(word: &str) -> AbilityAST {
    AbilityAST(
        [
            ("kind", "KEYWORD".to_owned()),
            ("abilityId", format!("test.kw.{word}")),
            ("keyword", word.to_owned()),
        ]
        .into_iter()
        .collect(),
    )
}

/// The world: every component of an event context, the replacement registry, the continuous
/// effects, the synthetic characteristics per card and the caller's SBA environment.
#[derive(Clone, Debug)]
struct World {
    game: GameState,
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    events: EventState,
    registry: ReplacementRegistry,
    effects: ContinuousEffects,
    chars: BTreeMap<CardId, Characteristics>,
    env: StoredSbaEnvironment,
    next_card: u64,
}

/// The layer environment and LKI provider over one snapshot of a world.
struct Snapshot {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
}

impl LayerEnvironment for Snapshot {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
        false
    }
}

impl CharacteristicsProvider for Snapshot {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        self.bases.get(&id).map(|b| b.characteristics.clone())
    }
}

impl World {
    /// Two players at 20 life, turn 1, active player 1.
    fn new() -> World {
        let mut game = GameState::new(41, 2, 20).unwrap();
        game.set_turn_number(1);
        game.set_active_player(Some(p(1))).unwrap();
        World {
            game,
            zones: Zones::new(&[p(1), p(2)], ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            registry: ReplacementRegistry::new(),
            effects: ContinuousEffects::new(),
            chars: BTreeMap::new(),
            env: StoredSbaEnvironment::default(),
            next_card: 100,
        }
    }

    fn card(&mut self, c: Characteristics) -> CardId {
        self.next_card += 1;
        let card = CardId::new(self.next_card);
        self.chars.insert(card, c);
        card
    }

    /// A new object of `owner` in a zone of `kind` with characteristics `c`.
    fn create(&mut self, owner: u64, kind: ZoneKind, c: Characteristics) -> ObjectId {
        let card = self.card(c);
        let mut req = MoveRequest::new(kind);
        if kind == ZoneKind::Battlefield {
            req.controller = Some(p(owner));
        }
        let id = self
            .zones
            .create_object(&mut self.ids, card, p(owner), req)
            .unwrap();
        if kind == ZoneKind::Battlefield {
            self.permanents.enter(id, p(owner), 1).unwrap();
        }
        id
    }

    /// A creature of `owner` on the battlefield.
    fn creature(&mut self, owner: u64, toughness: i64) -> ObjectId {
        self.create(
            owner,
            ZoneKind::Battlefield,
            creature("test.vanilla_1_x", toughness),
        )
    }

    /// A token creature of player 1 created on the battlefield (through `CreateToken`).
    fn token(&mut self) -> ObjectId {
        let card = self.card(creature("test.token_1_1", 1));
        self.setup(GameEvent::CreateToken {
            owner: p(1),
            card,
            controller: None,
        });
        self.zones.object_of_card(card).unwrap()
    }

    fn snapshot(&self) -> Snapshot {
        let mut bases = BTreeMap::new();
        let mut counters = BTreeMap::new();
        for id in self.zones.all_object_ids() {
            let record = self.zones.object(id).unwrap();
            let Some(c) = self.chars.get(&record.card) else {
                continue;
            };
            bases.insert(
                id,
                ObjectBase {
                    characteristics: c.clone(),
                    controller: Some(record.controller),
                    on_battlefield: record.zone == ZoneId::Battlefield,
                },
            );
            counters.insert(id, self.events.markers().counters(id));
        }
        Snapshot { bases, counters }
    }

    /// Executes `event` directly (no bus), for setup.
    fn setup(&mut self, event: GameEvent) {
        let snap = self.snapshot();
        let mut ctx = EventContext {
            game: &mut self.game,
            zones: &mut self.zones,
            ids: &mut self.ids,
            permanents: &mut self.permanents,
            memories: &mut self.memories,
            lki: &mut self.lki,
            events: &mut self.events,
            provider: &snap,
        };
        ctx.execute(event).unwrap();
    }

    fn counters(&mut self, object: ObjectId, kind: &str, amount: i64) {
        self.setup(GameEvent::AddCounters {
            target: CounterTarget::Object(object),
            kind: kind.to_owned(),
            amount,
        });
    }

    fn damage(&mut self, object: ObjectId, amount: i64) {
        self.setup(GameEvent::Damage {
            source: None,
            source_controller: None,
            target: DamageTarget::Object(object),
            amount,
            combat: false,
        });
    }

    fn move_to(&mut self, object: ObjectId, kind: ZoneKind) -> ObjectId {
        let card = self.zones.object(object).unwrap().card;
        self.setup(GameEvent::ZoneChange {
            object,
            request: MoveRequest::new(kind),
            cause: ZoneChangeCause::Other,
        });
        self.zones.object_of_card(card).unwrap()
    }

    fn set_life(&mut self, player: u64, life: i64) {
        self.game.player_mut(p(player)).unwrap().set_life(life);
    }

    fn set_poison(&mut self, player: u64, n: i64) {
        self.game
            .player_mut(p(player))
            .unwrap()
            .set_counter("poison", n);
    }

    fn lost(&self, player: u64) -> bool {
        self.game
            .player(p(player))
            .unwrap()
            .designation(LOST_THE_GAME)
            .is_some()
    }

    fn zone_of_card(&self, card: CardId) -> Option<ZoneId> {
        self.zones
            .object_of_card(card)
            .and_then(|id| self.zones.object(id))
            .map(|r| r.zone)
    }

    fn card_of(&self, object: ObjectId) -> CardId {
        self.zones.object(object).unwrap().card
    }

    /// `check` on the current state.
    fn check(&mut self) -> Vec<SbaFinding> {
        let snap = self.snapshot();
        let registry = SbaRegistry::standard();
        let ctx = EventContext {
            game: &mut self.game,
            zones: &mut self.zones,
            ids: &mut self.ids,
            permanents: &mut self.permanents,
            memories: &mut self.memories,
            lki: &mut self.lki,
            events: &mut self.events,
            provider: &snap,
        };
        check(&registry, &ctx, &self.effects, &snap, &self.env)
    }

    /// The entry ids found for `subject`.
    fn found(&mut self, subject: SbaSubject) -> Vec<&'static str> {
        self.check()
            .into_iter()
            .filter(|f| f.subject == subject)
            .map(|f| f.entry_id)
            .collect()
    }
}

impl Transactional for World {
    fn state_hash(&self) -> Digest {
        let mut bytes = encode_to_vec(&self.game.state_hash());
        bytes.extend(encode_to_vec(&self.zones));
        bytes.extend(encode_to_vec(&self.permanents));
        bytes.extend(encode_to_vec(&self.memories));
        bytes.extend(encode_to_vec(&self.lki));
        bytes.extend(encode_to_vec(&self.events));
        bytes.extend(encode_to_vec(&self.registry));
        bytes.extend(encode_to_vec(&self.env.deathtouch));
        leaf_digest(&bytes)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        None
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

type Out = Result<SbaOutcome, BusError>;

/// The procedure body: one `perform_all` pass over `w`. A pending choice or an error asserts
/// that the world is exactly as before the call, then propagates.
fn run_pass(w: &mut World, ch: &mut ChoiceContext<'_>) -> Result<Out, Interrupt> {
    let before = w.state_hash();
    let snap = w.snapshot();
    let registry = SbaRegistry::standard();
    let World {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        registry: replacements,
        effects,
        env,
        ..
    } = w;
    let mut ctx = EventContext {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        provider: &snap,
    };
    let result = perform_all(&registry, &mut ctx, replacements, effects, &snap, env, ch);
    match result {
        Ok(outcome) => Ok(Ok(outcome)),
        Err(BusInterrupt::Pending(r)) => {
            assert_eq!(w.state_hash(), before, "a pending pass changed the world");
            Err(Interrupt::Pending(r))
        }
        Err(BusInterrupt::Error(err)) => {
            assert_eq!(w.state_hash(), before, "a failed pass changed the world");
            Ok(Err(err))
        }
    }
}

struct Run {
    asked: usize,
    state: World,
    out: Out,
    hash: Digest,
}

/// Runs one pass on a clone of `w` in a choice session, answering `answers` in order.
fn drive(w: &World, answers: &[u32]) -> Run {
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let mut asked = 0;
    let mut answers = answers.iter();
    loop {
        let pending = match session.status() {
            Status::Done { state, output } => {
                assert!(answers.next().is_none(), "unused answers");
                return Run {
                    asked,
                    state: state.clone(),
                    out: output.clone(),
                    hash: session.state_hash(),
                };
            }
            Status::Pending(r) => r.clone(),
        };
        let index = *answers.next().expect("more requests than answers");
        session
            .submit(
                pending.request_id,
                ChoiceResponse::SelectReplacement { index },
            )
            .unwrap();
        asked += 1;
    }
}

/// One pass with no choices; the world is replaced by the result.
fn pass(w: &mut World) -> SbaOutcome {
    let run = drive(w, &[]);
    assert_eq!(run.asked, 0);
    *w = run.state;
    run.out.expect("the pass failed")
}

fn obj(id: ObjectId) -> SbaSubject {
    SbaSubject::Object(id)
}

fn player(n: u64) -> SbaSubject {
    SbaSubject::Player(p(n))
}

// ---------------------------------------------------------------------------------------------
// Registry shape
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, SRS 9: one entry per rule CR 704.5a-704.5i, in order, with id, crRef, action and
// variant gate; CR 704.5c is inactive in Two-Headed Giant games, CR 704.5a is not.
#[test]
fn registry_shape() {
    let r = SbaRegistry::standard();
    let ids: Vec<&str> = r.entries().iter().map(|e| e.id).collect();
    assert_eq!(
        ids,
        [
            "704.5a", "704.5b", "704.5c", "704.5d", "704.5e", "704.5f", "704.5g", "704.5h",
            "704.5i"
        ]
    );
    for entry in r.entries() {
        assert_eq!(entry.cr_ref, format!("CR {}", entry.id));
        assert!(entry.active_in(GameVariant::Standard));
        assert_eq!(
            entry.active_in(GameVariant::TwoHeadedGiant),
            entry.id != "704.5c"
        );
        assert_eq!(r.get(entry.id).unwrap().id, entry.id);
        assert!(format!("{entry:?}").contains(entry.id));
    }
    assert_eq!(
        r.get("704.5c").unwrap().gate,
        VariantGate::Except(GameVariant::TwoHeadedGiant)
    );
    assert_eq!(r.get("704.5a").unwrap().gate, VariantGate::All);
    let actions: Vec<SbaAction> = r.entries().iter().map(|e| e.action).collect();
    assert_eq!(
        actions,
        [
            SbaAction::LoseGame,
            SbaAction::LoseGame,
            SbaAction::LoseGame,
            SbaAction::CeaseToExist,
            SbaAction::CeaseToExist,
            SbaAction::PutIntoGraveyard,
            SbaAction::Destroy,
            SbaAction::Destroy,
            SbaAction::PutIntoGraveyard,
        ]
    );
    assert!(r.get("704.5j").is_none());
    assert_eq!(r.clone().entries().len(), 9);
}

// REQ-SBA-001: the default environment is a standard game with no copies and no deathtouch.
#[test]
fn default_environment_answers() {
    struct Plain;
    impl SbaEnvironment for Plain {}
    let mut env = Plain;
    let o = ObjectId::new(1);
    assert_eq!(env.variant(), GameVariant::Standard);
    assert!(!env.is_spell_copy(o));
    assert!(!env.is_card_copy(o));
    assert!(!env.exempt_from_copy_sba(o));
    assert!(env.deathtouch_damaged().is_empty());
    env.clear_deathtouch_damaged();
    let mut stored = StoredSbaEnvironment::default();
    stored.deathtouch.insert(o);
    stored.spell_copies.insert(o);
    stored.card_copies.insert(o);
    stored.exempt.insert(o);
    stored.variant = GameVariant::TwoHeadedGiant;
    assert_eq!(stored.variant(), GameVariant::TwoHeadedGiant);
    assert!(stored.is_spell_copy(o) && stored.is_card_copy(o) && stored.exempt_from_copy_sba(o));
    assert_eq!(stored.deathtouch_damaged(), BTreeSet::from([o]));
    stored.clear_deathtouch_damaged();
    assert!(stored.deathtouch_damaged().is_empty());
}

// ---------------------------------------------------------------------------------------------
// One test per entry
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5a: 0 or less life loses (0 and -3); 1 does not; the loss is the
// LOST_THE_GAME flag and is not found again.
#[test]
fn sba_704_5a_life() {
    let mut w = World::new();
    w.set_life(1, 0);
    w.set_life(2, -3);
    assert_eq!(w.found(player(1)), ["704.5a"]);
    assert_eq!(w.found(player(2)), ["704.5a"]);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.lost_players, [p(1), p(2)]);
    assert!(w.lost(1) && w.lost(2));
    assert_eq!(
        w.game.player(p(1)).unwrap().designation(LOST_THE_GAME),
        Some(&DesignationValue::Flag)
    );
    // Already lost: not a finding again.
    assert!(w.check().is_empty());

    let mut w = World::new();
    w.set_life(1, 1);
    assert!(w.check().is_empty());
    let out = pass(&mut w);
    assert!(!out.performed);
    assert!(!w.lost(1));
}

// REQ-SBA-001, CR 704.5b, CR 121.4: the flag counts even if the library was refilled; it is
// consumed by the pass whether or not anyone loses.
#[test]
fn sba_704_5b_empty_library_draw() {
    let mut w = World::new();
    w.setup(GameEvent::Draw { player: p(2) });
    assert_eq!(w.events.empty_library_draws(), [p(2)]);
    // Refilled since.
    w.create(2, ZoneKind::Library, creature("test.vanilla_1_1", 1));
    assert_eq!(w.found(player(2)), ["704.5b"]);
    let out = pass(&mut w);
    assert_eq!(out.lost_players, [p(2)]);
    assert_eq!(out.loss_reasons[&p(2)], ["704.5b"]);
    assert!(w.events.empty_library_draws().is_empty());

    // Consumed even when nobody loses (the player already lost).
    w.setup(GameEvent::Draw { player: p(1) });
    w.game
        .player_mut(p(1))
        .unwrap()
        .set_designation(LOST_THE_GAME, DesignationValue::Flag);
    assert!(w.check().is_empty());
    let out = pass(&mut w);
    assert!(!out.performed);
    assert!(w.events.empty_library_draws().is_empty());
}

// REQ-SBA-001, CR 704.5c: ten poison counters lose, nine do not; ignored in Two-Headed Giant.
#[test]
fn sba_704_5c_poison() {
    let mut w = World::new();
    w.set_poison(1, 10);
    w.set_poison(2, 9);
    assert_eq!(w.found(player(1)), ["704.5c"]);
    assert!(w.found(player(2)).is_empty());
    w.env.variant = GameVariant::TwoHeadedGiant;
    assert!(w.check().is_empty());
    let out = pass(&mut w);
    assert!(!out.performed && !w.lost(1));
    // CR 704.5a is not ignored in Two-Headed Giant.
    w.set_life(2, 0);
    assert_eq!(w.found(player(2)), ["704.5a"]);
    w.env.variant = GameVariant::Standard;
    let out = pass(&mut w);
    assert_eq!(out.lost_players, [p(1), p(2)]);
}

// REQ-SBA-001, CR 704.5d, CR 111.7: a token in the graveyard, hand or exile ceases to exist; on
// the battlefield it does not.
#[test]
fn sba_704_5d_tokens() {
    let mut w = World::new();
    let on_bf = w.token();
    let t1 = w.token();
    let t2 = w.token();
    let t3 = w.token();
    let gy = w.move_to(t1, ZoneKind::Graveyard);
    let hand = w.move_to(t2, ZoneKind::Hand);
    let exile = w.move_to(t3, ZoneKind::Exile);
    let nontoken = w.create(1, ZoneKind::Graveyard, creature("test.vanilla_1_1", 1));
    let findings = w.check();
    let mut expected: Vec<SbaFinding> = [gy, hand, exile]
        .into_iter()
        .map(|id| SbaFinding {
            entry_id: "704.5d",
            subject: obj(id),
        })
        .collect();
    expected.sort_by_key(|f| f.subject);
    assert_eq!(findings, expected);
    let out = pass(&mut w);
    let mut ceased = vec![gy, hand, exile];
    ceased.sort();
    assert_eq!(out.ceased, ceased);
    assert!(out.executed.is_empty());
    for id in ceased {
        assert!(w.zones.object(id).is_none());
    }
    assert!(w.zones.object(on_bf).is_some());
    assert!(w.zones.object(nontoken).is_some());
    assert!(w.check().is_empty());
}

// REQ-SBA-001, CR 704.5e, CR 707.10a, CR 723.3c: a spell copy off the stack and a card copy in
// the graveyard cease to exist; a card copy on the battlefield or the stack, a spell copy on the
// stack and an exempt copy survive.
#[test]
fn sba_704_5e_copies() {
    let mut w = World::new();
    let spell_off = w.create(1, ZoneKind::Graveyard, creature("test.copy_a", 1));
    let spell_on = w.create(1, ZoneKind::Stack, creature("test.copy_b", 1));
    let card_gy = w.create(1, ZoneKind::Graveyard, creature("test.copy_c", 1));
    let card_bf = w.create(1, ZoneKind::Battlefield, creature("test.copy_d", 1));
    let card_stack = w.create(2, ZoneKind::Stack, creature("test.copy_e", 1));
    let exempt = w.create(1, ZoneKind::Exile, creature("test.copy_f", 1));
    let spell_bf = w.create(2, ZoneKind::Battlefield, creature("test.copy_g", 1));
    w.env.spell_copies = BTreeSet::from([spell_off, spell_on, spell_bf]);
    w.env.card_copies = BTreeSet::from([card_gy, card_bf, card_stack, exempt]);
    w.env.exempt = BTreeSet::from([exempt]);
    let found: Vec<ObjectId> = w
        .check()
        .into_iter()
        .map(|f| {
            assert_eq!(f.entry_id, "704.5e");
            match f.subject {
                SbaSubject::Object(o) => o,
                SbaSubject::Player(_) => panic!("player finding"),
            }
        })
        .collect();
    let mut expected = vec![spell_off, card_gy, spell_bf];
    expected.sort();
    assert_eq!(found, expected);
    let out = pass(&mut w);
    assert_eq!(out.ceased, expected);
    for id in expected {
        assert!(w.zones.object(id).is_none());
    }
    // A ceasing permanent also leaves the permanent container.
    assert!(w.permanents.get(spell_bf).is_none());
    for id in [spell_on, card_bf, card_stack, exempt] {
        assert!(w.zones.object(id).is_some());
    }
}

// REQ-SBA-001, CR 704.5f, CR 107.1b, CR 613.4: toughness 0 and -1 die; a toughness computed by
// a layer effect counts; toughness 1 survives. Put into the graveyard by a SBA cause.
#[test]
fn sba_704_5f_toughness() {
    let mut w = World::new();
    let zero = w.creature(1, 0);
    let negative = w.creature(2, -1);
    let computed = w.creature(1, 2);
    let alive = w.creature(1, 1);
    w.effects
        .register(
            e(1),
            ContinuousEffect {
                source: None,
                controller: p(1),
                timestamp: Timestamp::new(1),
                layer: Layer::L7c,
                is_cda: false,
                origin: EffectOrigin::Resolved,
                affected: Affected::Objects(BTreeSet::from([computed])),
                modification: LayerModification::ModifyPt {
                    power: 0,
                    toughness: -2,
                },
            },
        )
        .unwrap();
    assert_eq!(w.found(obj(zero)), ["704.5f"]);
    assert_eq!(w.found(obj(negative)), ["704.5f"]);
    assert_eq!(w.found(obj(computed)), ["704.5f"]);
    assert!(w.found(obj(alive)).is_empty());
    let cards = [w.card_of(zero), w.card_of(negative), w.card_of(computed)];
    let out = pass(&mut w);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(out.executed[0].events.len(), 3);
    for ev in &out.executed[0].events {
        assert!(matches!(
            ev.instance.event(),
            GameEvent::ZoneChange {
                cause: ZoneChangeCause::StateBasedAction,
                ..
            }
        ));
    }
    assert_eq!(w.zone_of_card(cards[0]), Some(ZoneId::Graveyard(p(1))));
    // The owner's graveyard (CR 400.3).
    assert_eq!(w.zone_of_card(cards[1]), Some(ZoneId::Graveyard(p(2))));
    assert_eq!(w.zone_of_card(cards[2]), Some(ZoneId::Graveyard(p(1))));
    assert!(w.zones.object(alive).is_some());
}

// REQ-SBA-001, CR 704.5g, CR 120.3e: damage equal to or greater than toughness destroys; less
// does not; toughness 0 with damage is 704.5f, not 704.5g.
#[test]
fn sba_704_5g_lethal_damage() {
    let mut w = World::new();
    let equal = w.creature(1, 2);
    let more = w.creature(1, 2);
    let less = w.creature(1, 3);
    let zero = w.creature(1, 0);
    w.damage(equal, 2);
    w.damage(more, 5);
    w.damage(less, 2);
    w.damage(zero, 1);
    assert_eq!(w.found(obj(equal)), ["704.5g"]);
    assert_eq!(w.found(obj(more)), ["704.5g"]);
    assert!(w.found(obj(less)).is_empty());
    assert_eq!(w.found(obj(zero)), ["704.5f"]);
    let out = pass(&mut w);
    assert_eq!(out.executed[0].events.len(), 3);
    assert!(w.zones.object(less).is_some());
    assert!(w.zones.object(equal).is_none());
}

// REQ-SBA-001, CR 704.5h, CR 702.2b: deathtouch damage since the last check destroys a creature
// with toughness greater than 0; toughness 0 is 704.5f only; the flag is cleared by the pass.
#[test]
fn sba_704_5h_deathtouch() {
    let mut w = World::new();
    let hit = w.creature(1, 4);
    let zero = w.creature(1, 0);
    let unhit = w.creature(1, 4);
    w.damage(hit, 1);
    w.env.deathtouch = BTreeSet::from([hit, zero]);
    assert_eq!(w.found(obj(hit)), ["704.5h"]);
    assert_eq!(w.found(obj(zero)), ["704.5f"]);
    assert!(w.found(obj(unhit)).is_empty());
    let out = pass(&mut w);
    assert_eq!(out.findings.len(), 2);
    assert!(w.zones.object(hit).is_none());
    assert!(w.env.deathtouch.is_empty());
    assert!(w.zones.object(unhit).is_some());
}

// REQ-SBA-001, CR 704.5i, CR 306.5c: a planeswalker with no loyalty counters is put into the
// graveyard; with 3 it is not; other permanents are unaffected.
#[test]
fn sba_704_5i_loyalty() {
    let mut w = World::new();
    let empty = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.walker_0", "Planeswalker"),
    );
    let loyal = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.walker_3", "Planeswalker"),
    );
    w.counters(loyal, "loyalty", 3);
    let artifact = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.artifact", "Artifact"),
    );
    // Not on the battlefield: not checked.
    w.create(
        1,
        ZoneKind::Graveyard,
        noncreature("test.walker_gy", "Planeswalker"),
    );
    let findings = w.check();
    assert_eq!(
        findings,
        [SbaFinding {
            entry_id: "704.5i",
            subject: obj(empty)
        }]
    );
    let card = w.card_of(empty);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    assert!(w.zones.object(loyal).is_some());
    assert!(w.zones.object(artifact).is_some());
}

// REQ-SBA-001, CR 702.12b, CR 704.5f: an indestructible creature ignores 704.5g and 704.5h but
// not 704.5f.
#[test]
fn indestructible_survives_5g_and_5h_not_5f() {
    let mut w = World::new();
    let mut tough = creature("test.indestructible_2_2", 2);
    tough.abilities.push(keyword("indestructible"));
    let mut frail = tough.clone();
    frail.toughness = Some(literal(0));
    let a = w.create(1, ZoneKind::Battlefield, tough);
    let b = w.create(1, ZoneKind::Battlefield, frail);
    w.damage(a, 5);
    w.env.deathtouch = BTreeSet::from([a]);
    assert!(w.found(obj(a)).is_empty());
    assert_eq!(w.found(obj(b)), ["704.5f"]);
    pass(&mut w);
    assert!(w.zones.object(a).is_some());
    assert!(w.zones.object(b).is_none());
}

// REQ-SBA-001: objects the layer environment does not know are skipped (not an error).
#[test]
fn unknown_layer_object_is_skipped() {
    let mut w = World::new();
    let id = w.creature(1, 0);
    let card = w.card_of(id);
    w.chars.remove(&card);
    assert!(w.check().is_empty());
    let out = pass(&mut w);
    assert!(!out.performed);
}

// ---------------------------------------------------------------------------------------------
// Simultaneity, merging, LKI
// ---------------------------------------------------------------------------------------------

/// The scenario of several SBAs at once, objects created in the given order of kinds.
/// Returns the world and (zero-toughness card, lethal card, walker card).
fn several(order: &[u8]) -> (World, [CardId; 3]) {
    let mut w = World::new();
    let mut cards = [CardId::new(0); 3];
    for &k in order {
        match k {
            0 => {
                let c = w.creature(1, 2);
                w.counters(c, "+1/+1", 1);
                w.counters(c, "-1/-1", 4);
                cards[0] = w.card_of(c);
            }
            1 => {
                let c = w.creature(2, 3);
                w.damage(c, 3);
                cards[1] = w.card_of(c);
            }
            2 => {
                let c = w.create(
                    2,
                    ZoneKind::Battlefield,
                    noncreature("test.walker_0", "Planeswalker"),
                );
                cards[2] = w.card_of(c);
            }
            _ => {
                let t = w.token();
                w.move_to(t, ZoneKind::Graveyard);
            }
        }
    }
    w.set_life(2, 0);
    (w, cards)
}

// REQ-SBA-001, CR 704.3, CR 704.8: a 0-toughness creature, a lethal-damage creature, a
// 0-loyalty planeswalker, a token in the graveyard and a 0-life player are handled in ONE pass:
// one outcome, the moves in ONE executed batch, LKI of every dying permanent from before the pass
// (the +1/+1 counter of the creature killed by -1/-1 counters is in its LKI).
#[test]
fn several_sbas_are_one_event() {
    let (mut w, cards) = several(&[0, 1, 2, 3]);
    let log_before = w.events.log().len();
    let out = pass(&mut w);
    assert!(out.performed);
    let ids: BTreeSet<&str> = out.findings.iter().map(|f| f.entry_id).collect();
    assert_eq!(
        ids,
        BTreeSet::from(["704.5a", "704.5d", "704.5f", "704.5g", "704.5i"])
    );
    assert_eq!(out.executed.len(), 1);
    assert_eq!(w.events.log().len(), log_before + 1);
    let batch = &out.executed[0];
    assert_eq!(batch.events.len(), 3);
    for ev in &batch.events {
        assert!(ev.lki_captured);
        let Outcome::ZoneChange(m) = ev.outcome else {
            panic!("not a zone change");
        };
        let lki = w.lki.get(m.old).unwrap();
        assert_eq!(lki.zone(), ZoneId::Battlefield);
        if lki.card() == cards[0] {
            assert_eq!(lki.counters().get("+1/+1"), Some(&1));
            assert_eq!(lki.counters().get("-1/-1"), Some(&4));
        }
    }
    assert_eq!(out.ceased.len(), 1);
    assert_eq!(out.lost_players, [p(2)]);
    for card in cards {
        assert!(matches!(w.zone_of_card(card), Some(ZoneId::Graveyard(_))));
    }
    // Nothing left to do.
    let again = pass(&mut w);
    assert!(!again.performed);
}

// REQ-SBA-001, REQ-CORE-001: the result does not depend on the ObjectId order of the objects.
#[test]
fn result_independent_of_object_order() {
    let shape = |order: &[u8]| {
        let (mut w, cards) = several(order);
        let out = pass(&mut w);
        let zones: Vec<Option<ZoneId>> = cards.iter().map(|&c| w.zone_of_card(c)).collect();
        let mut entries: Vec<&str> = out.findings.iter().map(|f| f.entry_id).collect();
        entries.sort_unstable();
        (
            zones,
            entries,
            out.lost_players,
            out.ceased.len(),
            out.executed[0].events.len(),
        )
    };
    let a = shape(&[0, 1, 2, 3]);
    assert_eq!(a, shape(&[3, 2, 1, 0]));
    assert_eq!(a, shape(&[2, 0, 3, 1]));
}

// REQ-SBA-001, REQ-CORE-001: same inputs, same output and same state hash.
#[test]
fn deterministic_pass() {
    let (w, _) = several(&[0, 1, 2, 3]);
    let a = drive(&w, &[]);
    let b = drive(&w, &[]);
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.out, b.out);
}

// REQ-SBA-001, CR 704.7: two losses of one player are one loser with both reasons; two players
// losing are two losers.
#[test]
fn same_result_merge_for_players() {
    let mut w = World::new();
    w.set_life(1, 0);
    w.setup(GameEvent::Draw { player: p(1) });
    w.set_poison(1, 12);
    w.set_life(2, -1);
    let out = pass(&mut w);
    assert_eq!(out.lost_players, [p(1), p(2)]);
    assert_eq!(out.loss_reasons[&p(1)], ["704.5a", "704.5b", "704.5c"]);
    assert_eq!(out.loss_reasons[&p(2)], ["704.5a"]);
    assert_eq!(out.findings.len(), 4);
}

// REQ-SBA-001, CR 704.7: a creature with both 704.5g and 704.5h moves once; a one-use
// replacement applies once to it.
#[test]
fn one_use_replacement_applies_once_to_merged_object() {
    let mut w = World::new();
    let c = w.creature(1, 2);
    w.damage(c, 2);
    w.env.deathtouch = BTreeSet::from([c]);
    let card = w.card_of(c);
    let mut matcher = EventMatcher::new(BusEventKind::ZoneChange);
    matcher.affected_object = Some(c);
    w.registry
        .add(
            ReplacementEffect::new(
                e(1),
                matcher,
                Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
            )
            .with_shield(Shield::Uses(1)),
        )
        .unwrap();
    let out = pass(&mut w);
    assert_eq!(
        out.findings,
        [
            SbaFinding {
                entry_id: "704.5g",
                subject: obj(c)
            },
            SbaFinding {
                entry_id: "704.5h",
                subject: obj(c)
            }
        ]
    );
    assert_eq!(out.executed[0].events.len(), 1);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Exile));
    assert!(w.registry.is_empty());
}

// REQ-SBA-001, CR 122.1c, CR 701.8b: the shield counter's destroy replacement does not apply to
// SBA destruction (cause StateBasedAction).
#[test]
fn shield_counter_does_not_stop_sba_destruction() {
    let mut w = World::new();
    let c = w.creature(1, 2);
    w.counters(c, "shield", 1);
    w.damage(c, 3);
    w.registry
        .add(ReplacementEffect::shield_counter_destroy(e(1), c))
        .unwrap();
    let card = w.card_of(c);
    pass(&mut w);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
}

// REQ-SBA-001: a move target that also ceases to exist only ceases.
#[test]
fn ceasing_wins_over_a_move() {
    let mut w = World::new();
    let c = w.creature(1, 0);
    w.env.spell_copies = BTreeSet::from([c]);
    let out = pass(&mut w);
    assert_eq!(out.findings.len(), 2);
    assert!(out.executed.is_empty());
    assert_eq!(out.ceased, [c]);
    assert!(w.zones.object(c).is_none());
    assert!(w.permanents.get(c).is_none());
}

// REQ-SBA-001, CR 704.3: `check` is pure (no state change, the flags are not consumed).
#[test]
fn check_is_pure() {
    let (mut w, _) = several(&[0, 1, 2, 3]);
    w.setup(GameEvent::Draw { player: p(1) });
    w.env.deathtouch = BTreeSet::from([ObjectId::new(1)]);
    let before = w.state_hash();
    let a = w.check();
    let b = w.check();
    assert_eq!(a, b);
    assert!(!a.is_empty());
    assert_eq!(w.state_hash(), before);
    // Sorted by registry order, then subject.
    let order: Vec<usize> = a
        .iter()
        .map(|f| {
            SbaRegistry::standard()
                .entries()
                .iter()
                .position(|e| e.id == f.entry_id)
                .unwrap()
        })
        .collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted);
}

// ---------------------------------------------------------------------------------------------
// Choice suspension and atomicity
// ---------------------------------------------------------------------------------------------

fn choosing_world() -> (World, CardId, ObjectId) {
    let mut w = World::new();
    let c = w.creature(1, 0);
    let other = w.creature(2, 2);
    w.damage(other, 2);
    w.setup(GameEvent::Draw { player: p(2) });
    w.env.deathtouch = BTreeSet::from([other]);
    let token = w.token();
    let token = w.move_to(token, ZoneKind::Exile);
    for (id, to) in [(1, ZoneKind::Exile), (2, ZoneKind::Hand)] {
        let mut matcher = EventMatcher::new(BusEventKind::ZoneChange);
        matcher.affected_object = Some(c);
        w.registry
            .add(
                ReplacementEffect::new(
                    e(id),
                    matcher,
                    Action::Modify(Modification::SetDestination(to)),
                )
                .with_description(format!("effect {id}")),
            )
            .unwrap();
    }
    let card = w.card_of(c);
    (w, card, token)
}

// REQ-SBA-001, CR 616.1, CR 704.3: when the bus asks CHOOSE_REPLACEMENT_EFFECT, the pass leaves
// every component as before (asserted inside the procedure); after the answer the pass
// completes, and a replay from the base state gives an identical state hash.
#[test]
fn choice_suspension_and_replay() {
    let (w, card, token) = choosing_world();
    let run = drive(&w, &[0]);
    assert_eq!(run.asked, 1);
    let out = run.out.as_ref().unwrap();
    assert!(out.performed);
    assert_eq!(out.lost_players, [p(2)]);
    assert_eq!(out.ceased, [token]);
    assert!(matches!(
        run.state.zone_of_card(card),
        Some(ZoneId::Hand(_) | ZoneId::Exile)
    ));
    assert!(run.state.env.deathtouch.is_empty());
    assert!(run.state.events.empty_library_draws().is_empty());

    let replayed = ChoiceSession::replay(
        w.clone(),
        run_pass,
        &[ChoiceResponse::SelectReplacement { index: 0 }],
    )
    .unwrap();
    assert_eq!(replayed.state_hash(), run.hash);

    // Before the answer the session is pending.
    let session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    assert!(matches!(session.status(), Status::Pending(_)));

    // A different answer is equally reproducible.
    let other = drive(&w, &[1]);
    assert_eq!(other.hash, drive(&w, &[1]).hash);
}

// REQ-SBA-001: a bus error leaves every component, the replacement registry and the
// environment flags exactly as before (also asserted inside the procedure).
#[test]
fn bus_error_rolls_back_everything() {
    let mut w = World::new();
    let c = w.creature(1, 0);
    w.setup(GameEvent::Draw { player: p(1) });
    w.env.deathtouch = BTreeSet::from([c]);
    let mut matcher = EventMatcher::new(BusEventKind::ZoneChange);
    matcher.affected_object = Some(c);
    // An effect that re-applies itself forever (CR 614.5 overridden) exceeds the repeat limit.
    w.registry
        .add(
            ReplacementEffect::new(
                e(1),
                matcher,
                Action::Modify(Modification::SetDestination(ZoneKind::Graveyard)),
            )
            .with_overrides_614_5(true),
        )
        .unwrap();
    let before = w.state_hash();
    let run = drive(&w, &[]);
    assert!(run.out.is_err());
    assert_eq!(run.state.state_hash(), before);
    assert_eq!(run.state.env.deathtouch, BTreeSet::from([c]));
    assert_eq!(run.state.events.empty_library_draws(), [p(1)]);
}
