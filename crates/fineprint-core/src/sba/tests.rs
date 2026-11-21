// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the state-based action registry and pass: REQ-SBA-001 (CR 704.3, CR 704.5a-t,
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
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceResponse,
    ChoiceSession, ChoiceType, Interrupt, PendingChoiceRequest, Status,
};
use crate::events::{
    CounterTarget, DamageTarget, EventContext, EventState, GameEvent, Outcome, ZoneChangeCause,
};
use crate::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerEnvironment,
    LayerModification, MatchContext, ObjectBase, literal,
};
use crate::memory::{CharacteristicsProvider, LkiStore, MemoryTable};
use crate::objects::Characteristics;
use crate::permanent::{AttachTarget, Permanents};
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
        // REQ-CORE-006: the id generator is part of the state a rollback must restore.
        bytes.extend(encode_to_vec(&self.ids.last_issued()));
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

// REQ-SBA-001, SRS 9: one entry per rule CR 704.5a-704.5t (no 704.5l, no 704.5o), in order, with
// id, crRef, action and variant gate; CR 704.5c is inactive in Two-Headed Giant games, every
// other entry (CR 704.5a and all of CR 704.5j-704.5t included) is active.
#[test]
fn registry_shape() {
    let r = SbaRegistry::standard();
    let ids: Vec<&str> = r.entries().iter().map(|e| e.id).collect();
    assert_eq!(
        ids,
        [
            "704.5a", "704.5b", "704.5c", "704.5d", "704.5e", "704.5f", "704.5g", "704.5h",
            "704.5i", "704.5j", "704.5k", "704.5m", "704.5n", "704.5p", "704.5q", "704.5r",
            "704.5s", "704.5t"
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
            SbaAction::LegendChoice,
            SbaAction::PutIntoGraveyard,
            SbaAction::PutIntoGraveyard,
            SbaAction::Unattach,
            SbaAction::Unattach,
            SbaAction::RemoveCounters,
            SbaAction::RemoveCounters,
            SbaAction::Sacrifice,
            SbaAction::RemoveFromGame,
        ]
    );
    for gone in ["704.5l", "704.5o", "704.5u"] {
        assert!(r.get(gone).is_none());
    }
    for id in [
        "704.5j", "704.5k", "704.5m", "704.5n", "704.5p", "704.5q", "704.5r", "704.5s", "704.5t",
    ] {
        assert_eq!(r.get(id).unwrap().gate, VariantGate::All);
    }
    assert_eq!(r.clone().entries().len(), 18);
}

// REQ-SBA-001: the default environment is a standard game with no copies, no deathtouch and the
// CR 704.5k-704.5t defaults.
#[test]
fn default_environment_answers() {
    struct Plain;
    impl SbaEnvironment for Plain {
        fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
            BTreeSet::new()
        }
        fn clear_deathtouch_damaged(&mut self) {}
    }
    let mut env = Plain;
    let o = ObjectId::new(1);
    assert_eq!(env.variant(), GameVariant::Standard);
    assert!(!env.is_spell_copy(o));
    assert!(!env.is_card_copy(o));
    assert!(!env.exempt_from_copy_sba(o));
    assert!(env.deathtouch_damaged().is_empty());
    env.clear_deathtouch_damaged();
    // CR 704.5k-704.5t defaults (REQ-SBA-001): stamp 0, every Aura target legal, no counter
    // limits, no chapter abilities, nothing pending, no completed dungeon.
    assert_eq!(env.world_supertype_since(o), 0);
    assert!(env.aura_can_enchant(o, AttachTarget::Player(p(1))));
    assert!(env.counter_limits(o).is_empty());
    assert_eq!(env.saga_final_chapter(o), None);
    assert!(!env.chapter_ability_pending(o));
    assert!(env.dungeons_at_bottommost_room().is_empty());
    assert!(!env.room_ability_pending(o));
    let mut stored = StoredSbaEnvironment::default();
    assert_eq!(stored.world_supertype_since(o), 0);
    assert!(stored.aura_can_enchant(o, AttachTarget::Player(p(1))));
    assert_eq!(stored.saga_final_chapter(o), None);
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
// (the +1/+1 counter of the creature killed by -1/-1 counters is in its LKI). The same creature
// is also a CR 704.5q finding, but it moves, so no counter is removed from it.
#[test]
fn several_sbas_are_one_event() {
    let (mut w, cards) = several(&[0, 1, 2, 3]);
    let log_before = w.events.log().len();
    let out = pass(&mut w);
    assert!(out.performed);
    let ids: BTreeSet<&str> = out.findings.iter().map(|f| f.entry_id).collect();
    assert_eq!(
        ids,
        BTreeSet::from(["704.5a", "704.5d", "704.5f", "704.5g", "704.5i", "704.5q"])
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
    // Answer 0 applies effect 1 (exile) first; effect 2 (hand) then still applies, last.
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Hand(p(1))));
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

    // A different answer: effect 2 (hand) first, then effect 1 (exile) last; equally
    // reproducible.
    let other = drive(&w, &[1]);
    assert_eq!(other.state.zone_of_card(card), Some(ZoneId::Exile));
    assert_eq!(other.hash, drive(&w, &[1]).hash);
    assert_ne!(other.hash, run.hash);
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

// ---------------------------------------------------------------------------------------------
// Phased-out permanents (CR 702.26b, CR 702.26d)
// ---------------------------------------------------------------------------------------------

impl World {
    fn phase(&mut self, id: ObjectId, out: bool) {
        self.permanents.set_phased_out(id, out).unwrap();
    }
}

// REQ-SBA-001, CR 702.26b, CR 704.5f: a phased-out creature with toughness 0 is not checked;
// once phased in, the same state is found and the creature is moved.
#[test]
fn phased_out_704_5f_toughness() {
    let mut w = World::new();
    let c = w.creature(1, 0);
    w.phase(c, true);
    assert!(w.check().is_empty());
    let before = w.zones.clone();
    assert!(!pass(&mut w).performed);
    assert_eq!(w.zones, before);
    w.phase(c, false);
    assert_eq!(w.found(obj(c)), ["704.5f"]);
    pass(&mut w);
    assert!(
        w.zones
            .object(c)
            .is_none_or(|r| r.zone != ZoneId::Battlefield)
    );
}

// REQ-SBA-001, CR 702.26b, CR 702.26d, CR 704.5g: marked damage is neither cleared on phasing out
// nor reset on phasing in.
#[test]
fn phased_out_704_5g_damage_kept() {
    let mut w = World::new();
    let c = w.creature(1, 2);
    w.damage(c, 2);
    w.phase(c, true);
    assert!(w.check().is_empty());
    assert!(!pass(&mut w).performed);
    assert_eq!(w.zones.object(c).unwrap().zone, ZoneId::Battlefield);
    w.phase(c, false);
    assert_eq!(w.found(obj(c)), ["704.5g"]);
    assert!(pass(&mut w).performed);
    assert!(
        w.zones
            .object(c)
            .is_none_or(|r| r.zone != ZoneId::Battlefield)
    );
}

// REQ-SBA-001, CR 702.26b, CR 704.5h: the deathtouch flag is not consulted while phased out and
// is still consumed by a successful pass (decision #41 FP-040); a flag set after phasing in works.
#[test]
fn phased_out_704_5h_deathtouch() {
    let mut w = World::new();
    let c = w.creature(1, 4);
    let other = w.creature(1, 0);
    w.env.deathtouch = BTreeSet::from([c]);
    w.phase(c, true);
    assert!(w.check().iter().all(|f| f.subject != obj(c)));
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(w.zones.object(c).unwrap().zone, ZoneId::Battlefield);
    assert!(
        w.zones
            .object(other)
            .is_none_or(|r| r.zone != ZoneId::Battlefield)
    );
    assert!(w.env.deathtouch.is_empty());
    w.phase(c, false);
    assert!(w.check().is_empty());
    w.env.deathtouch = BTreeSet::from([c]);
    assert_eq!(w.found(obj(c)), ["704.5h"]);
    pass(&mut w);
    assert!(
        w.zones
            .object(c)
            .is_none_or(|r| r.zone != ZoneId::Battlefield)
    );
}

// REQ-SBA-001, CR 702.26b, CR 704.5i: a phased-out planeswalker with 0 loyalty is not checked.
#[test]
fn phased_out_704_5i_loyalty() {
    let mut w = World::new();
    let pw = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.walker_0", "Planeswalker"),
    );
    w.phase(pw, true);
    assert!(w.check().is_empty());
    assert!(!pass(&mut w).performed);
    assert_eq!(w.zones.object(pw).unwrap().zone, ZoneId::Battlefield);
    w.phase(pw, false);
    assert_eq!(w.found(obj(pw)), ["704.5i"]);
    assert!(pass(&mut w).performed);
    assert!(
        w.zones
            .object(pw)
            .is_none_or(|r| r.zone != ZoneId::Battlefield)
    );
}

// REQ-SBA-001, CR 702.26d, CR 704.5d: a phased-out token on the battlefield does not cease to
// exist; the same token in another zone does.
#[test]
fn phased_out_token_on_battlefield_survives() {
    let mut w = World::new();
    let t = w.token();
    w.phase(t, true);
    assert!(w.check().is_empty());
    let out = pass(&mut w);
    assert!(out.ceased.is_empty());
    assert!(w.zones.object(t).is_some());
    let gone = w.move_to(t, ZoneKind::Graveyard);
    let out = pass(&mut w);
    assert_eq!(out.ceased, [gone]);
    assert!(w.zones.object(gone).is_none());
}

// REQ-SBA-001, CR 702.26b: on a mixed board only the phased-in creature is moved, in one batch.
#[test]
fn phased_out_mixed_board_one_batch() {
    let mut w = World::new();
    let out_c = w.creature(1, 0);
    let in_c = w.creature(1, 0);
    w.phase(out_c, true);
    assert_eq!(w.check().len(), 1);
    let out = pass(&mut w);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(w.zones.object(out_c).unwrap().zone, ZoneId::Battlefield);
    assert!(
        w.zones
            .object(in_c)
            .is_none_or(|r| r.zone != ZoneId::Battlefield)
    );
}

// ---------------------------------------------------------------------------------------------
// Review round 1 (PR #207)
// ---------------------------------------------------------------------------------------------

/// A matcher for the SBA move of `object` (cause `StateBasedAction`).
fn sba_move_of(object: ObjectId) -> EventMatcher {
    let mut matcher = EventMatcher::new(BusEventKind::ZoneChange);
    matcher.affected_object = Some(object);
    matcher.cause = Some(ZoneChangeCause::StateBasedAction);
    matcher
}

/// A zone change of `object` to `kind` with `cause`.
fn zone_change(object: ObjectId, kind: ZoneKind, cause: ZoneChangeCause) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(kind),
        cause,
    }
}

/// A world with a 0-toughness creature `a` (CR 704.5f) whose SBA move is replaced by
/// `Instead([a to graveyard, other to kind])`. Returns the world and `a`.
fn instead_world(w: &mut World, other: ObjectId, kind: ZoneKind) -> ObjectId {
    let a = w.creature(1, 0);
    w.registry
        .add(ReplacementEffect::new(
            e(10),
            sba_move_of(a),
            Action::Instead(vec![
                zone_change(a, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
                zone_change(other, kind, ZoneChangeCause::Other),
            ]),
        ))
        .unwrap();
    a
}

/// The new id an executed batch of `out` gave `old` (one move).
fn moved_id(out: &SbaOutcome, old: ObjectId) -> ObjectId {
    out.executed
        .iter()
        .flat_map(|b| &b.events)
        .find_map(|ev| match ev.outcome {
            Outcome::ZoneChange(m) if m.old == old => Some(m.new),
            _ => None,
        })
        .expect("the object was not moved")
}

// REQ-SBA-001, CR 704.5e, CR 704.3, CR 400.7: a card copy in exile with a 704.5e finding, moved
// to the hand by an Instead replacement of another member's SBA move, ceases in the same pass
// under its new id; nothing of it is left. Deterministic, and a Pending pass (a choice between
// two replacements) rolls back fully (asserted inside the procedure).
#[test]
fn moved_card_copy_ceases_in_the_same_pass() {
    let mut w = World::new();
    let copy = w.create(1, ZoneKind::Exile, creature("test.copy_moved", 1));
    let copy_card = w.card_of(copy);
    w.env.card_copies = BTreeSet::from([copy]);
    let a = instead_world(&mut w, copy, ZoneKind::Hand);
    let a_card = w.card_of(a);
    let run = drive(&w, &[]);
    let out = run.out.as_ref().unwrap();
    let new = moved_id(out, copy);
    assert_ne!(new, copy);
    assert_eq!(out.ceased, [new]);
    assert!(!out.ceased.contains(&copy));
    assert!(run.state.zones.object(new).is_none());
    assert!(run.state.zones.object(copy).is_none());
    assert_eq!(run.state.zones.object_of_card(copy_card), None);
    assert_eq!(
        run.state.zone_of_card(a_card),
        Some(ZoneId::Graveyard(p(1)))
    );
    assert!(out.performed);
    // Deterministic.
    let again = drive(&w, &[]);
    assert_eq!(again.hash, run.hash);
    assert_eq!(again.out, run.out);

    // With a competing replacement the pass suspends first (rolled back), then completes.
    let mut chosen = w.clone();
    chosen
        .registry
        .add(ReplacementEffect::new(
            e(11),
            sba_move_of(a),
            Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
        ))
        .unwrap();
    let run = drive(&chosen, &[0]);
    assert_eq!(run.asked, 1);
    let out = run.out.as_ref().unwrap();
    let new = moved_id(out, copy);
    assert_eq!(out.ceased, [new]);
    assert_eq!(run.state.zones.object_of_card(copy_card), None);
    assert_eq!(drive(&chosen, &[0]).hash, run.hash);
}

// REQ-SBA-001, CR 704.5e, CR 707.10a: a card copy moved onto the battlefield by such an Instead
// is no longer out of place and stays.
#[test]
fn moved_card_copy_onto_battlefield_stays() {
    let mut w = World::new();
    let copy = w.create(1, ZoneKind::Exile, creature("test.copy_to_bf", 1));
    let copy_card = w.card_of(copy);
    w.env.card_copies = BTreeSet::from([copy]);
    instead_world(&mut w, copy, ZoneKind::Battlefield);
    let out = pass(&mut w);
    assert!(out.ceased.is_empty());
    let new = moved_id(&out, copy);
    assert_eq!(w.zones.object(new).unwrap().zone, ZoneId::Battlefield);
    assert_eq!(w.zones.object_of_card(copy_card), Some(new));
}

// REQ-SBA-001, CR 704.5d, CR 111.7: a token in the graveyard named by such an Instead (to exile)
// ceases in the same pass. The event executor never moves a token that has left the battlefield
// (CR 111.8, events/exec.rs), so the token keeps its id and ceases under it; a token is never
// moved before it ceases.
#[test]
fn token_named_by_instead_ceases_in_the_same_pass() {
    let mut w = World::new();
    let t = w.token();
    let card = w.card_of(t);
    let gy = w.move_to(t, ZoneKind::Graveyard);
    instead_world(&mut w, gy, ZoneKind::Exile);
    let out = pass(&mut w);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(out.executed[0].events.len(), 1);
    assert_eq!(out.ceased, [gy]);
    assert_eq!(w.zones.object_of_card(card), None);
    // REQ-CORE-003: its last known information in the graveyard is kept.
    assert_eq!(w.lki.get(gy).unwrap().zone(), ZoneId::Graveyard(p(1)));
}

// REQ-SBA-001, CR 704.5e, CR 723.3c: an exempt card copy moved by such an Instead stays.
#[test]
fn moved_exempt_copy_stays() {
    let mut w = World::new();
    let copy = w.create(1, ZoneKind::Exile, creature("test.copy_exempt", 1));
    let card = w.card_of(copy);
    w.env.card_copies = BTreeSet::from([copy]);
    w.env.exempt = BTreeSet::from([copy]);
    instead_world(&mut w, copy, ZoneKind::Hand);
    let out = pass(&mut w);
    assert!(out.ceased.is_empty());
    let new = moved_id(&out, copy);
    assert_eq!(w.zones.object(new).unwrap().zone, ZoneId::Hand(p(1)));
    assert_eq!(w.zones.object_of_card(card), Some(new));
}

// REQ-SBA-001, CR 704.3, CR 704.5f: a persistent Skip of the only move performs nothing: the
// finding is reported, `performed` is false and the state is unchanged.
#[test]
fn skipped_move_is_not_performed() {
    let mut w = World::new();
    let c = w.creature(1, 0);
    w.registry
        .add(ReplacementEffect::new(e(1), sba_move_of(c), Action::Skip))
        .unwrap();
    let before = w.state_hash();
    let out = pass(&mut w);
    assert!(!out.performed);
    assert_eq!(
        out.findings,
        [SbaFinding {
            entry_id: "704.5f",
            subject: obj(c)
        }]
    );
    assert!(out.executed.is_empty());
    assert_eq!(w.state_hash(), before);
}

// REQ-SBA-001, CR 704.3, CR 704.5f: a one-use Skip changes the registry (performed); the next
// pass moves the creature.
#[test]
fn one_use_skip_is_performed_then_the_move_happens() {
    let mut w = World::new();
    let c = w.creature(1, 0);
    let card = w.card_of(c);
    w.registry
        .add(
            ReplacementEffect::new(e(1), sba_move_of(c), Action::Skip).with_shield(Shield::Uses(1)),
        )
        .unwrap();
    let out = pass(&mut w);
    assert!(out.performed);
    assert!(out.executed.is_empty());
    assert!(w.registry.is_empty());
    assert_eq!(w.zones.object(c).unwrap().zone, ZoneId::Battlefield);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
}

// REQ-SBA-001, CR 704.3, CR 704.5a, CR 704.5d: a pass whose only effect is a lost player, or a
// ceased object, is performed.
#[test]
fn loss_only_and_cease_only_passes_are_performed() {
    let mut w = World::new();
    w.set_life(1, 0);
    let out = pass(&mut w);
    assert!(out.performed && out.executed.is_empty() && out.ceased.is_empty());
    assert_eq!(out.lost_players, [p(1)]);

    let mut w = World::new();
    let t = w.token();
    let gy = w.move_to(t, ZoneKind::Graveyard);
    let out = pass(&mut w);
    assert!(out.performed && out.executed.is_empty() && out.lost_players.is_empty());
    assert_eq!(out.ceased, [gy]);
}

/// A layer environment that delegates to a snapshot but declines every `eval_value`.
struct NoEval(Snapshot);

impl LayerEnvironment for NoEval {
    fn objects(&self) -> Vec<ObjectId> {
        self.0.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.0.base(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        LayerEnvironment::counters(&self.0, id)
    }

    fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
        false
    }

    fn eval_value(&self, _: &fineprint_dsl::card::ValueExpressionAST, _: ObjectId) -> Option<i64> {
        None
    }
}

// REQ-SBA-001, CR 613.4, CR 704.5f, CR 704.5g: the final toughness is read from the evaluated
// literal, so an environment that declines `eval_value` still sees 704.5f and 704.5g.
#[test]
fn toughness_read_without_eval_value() {
    let mut w = World::new();
    let zero = w.creature(1, 0);
    let lethal = w.creature(1, 2);
    w.damage(lethal, 2);
    let env = NoEval(w.snapshot());
    let registry = SbaRegistry::standard();
    let snap = w.snapshot();
    let ctx = EventContext {
        game: &mut w.game,
        zones: &mut w.zones,
        ids: &mut w.ids,
        permanents: &mut w.permanents,
        memories: &mut w.memories,
        lki: &mut w.lki,
        events: &mut w.events,
        provider: &snap,
    };
    let findings = check(&registry, &ctx, &w.effects, &env, &w.env);
    assert_eq!(
        findings,
        [
            SbaFinding {
                entry_id: "704.5f",
                subject: obj(zero)
            },
            SbaFinding {
                entry_id: "704.5g",
                subject: obj(lethal)
            }
        ]
    );
}

// REQ-SBA-001, CR 704.5h, CR 702.12b, CR 702.2b: the deathtouch window closes at every check,
// even for an indestructible creature; after losing indestructible it is not destroyed by the
// stale flag.
#[test]
fn deathtouch_window_closes_for_indestructible() {
    let mut w = World::new();
    let mut c = creature("test.indestructible_x_2", 2);
    c.abilities.push(keyword("indestructible"));
    let id = w.create(1, ZoneKind::Battlefield, c);
    w.env.deathtouch = BTreeSet::from([id]);
    let out = pass(&mut w);
    assert!(out.findings.is_empty());
    assert!(w.env.deathtouch.is_empty());
    assert_eq!(w.zones.object(id).unwrap().zone, ZoneId::Battlefield);
    // It loses indestructible.
    let card = w.card_of(id);
    w.chars.get_mut(&card).unwrap().abilities.clear();
    let out = pass(&mut w);
    assert!(!out.performed);
    assert_eq!(w.zones.object(id).unwrap().zone, ZoneId::Battlefield);
}

// REQ-SBA-001, REQ-CORE-003, CR 704.5e, CR 608.2h: a battlefield spell copy (creature and
// noncreature) that ceases keeps its last known information: owner, zone, characteristics,
// counters and permanent state; no ZoneChange is added; a Pending pass rolls the LKI back too
// (asserted inside the procedure).
#[test]
fn ceasing_battlefield_copy_captures_lki() {
    let mut w = World::new();
    let body = w.create(2, ZoneKind::Battlefield, creature("test.copy_lki", 3));
    let art = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.copy_lki_artifact", "Artifact"),
    );
    w.counters(body, "+1/+1", 2);
    w.env.spell_copies = BTreeSet::from([body, art]);
    let run = drive(&w, &[]);
    let out = run.out.as_ref().unwrap();
    assert_eq!(out.ceased, [body, art]);
    assert!(out.executed.is_empty());
    let lki = run.state.lki.get(body).unwrap();
    assert_eq!(lki.owner(), p(2));
    assert_eq!(lki.zone(), ZoneId::Battlefield);
    assert!(lki.characteristics().names.contains("test.copy_lki"));
    assert_eq!(lki.counters().get("+1/+1"), Some(&2));
    assert!(lki.permanent().is_some());
    let lki = run.state.lki.get(art).unwrap();
    assert_eq!(lki.owner(), p(1));
    assert!(lki.characteristics().card_types.contains("Artifact"));
    assert!(lki.permanent().is_some());

    // With a replacement choice on another member's move: Pending first, then the same LKI.
    let mut w2 = w.clone();
    let c = w2.creature(1, 0);
    for (n, to) in [(1, ZoneKind::Exile), (2, ZoneKind::Hand)] {
        w2.registry
            .add(ReplacementEffect::new(
                e(n),
                sba_move_of(c),
                Action::Modify(Modification::SetDestination(to)),
            ))
            .unwrap();
    }
    let run = drive(&w2, &[0]);
    assert_eq!(run.asked, 1);
    assert_eq!(
        run.state.lki.get(body).unwrap().counters().get("+1/+1"),
        Some(&2)
    );
}

// REQ-SBA-001, REQ-CORE-003, CR 704.5d: a token in the graveyard that ceases keeps its last
// known information in the graveyard; an object the provider does not know still ceases, with
// no record.
#[test]
fn ceasing_token_lki_and_unknown_object() {
    let mut w = World::new();
    let t = w.token();
    let gy = w.move_to(t, ZoneKind::Graveyard);
    let unknown = w.create(1, ZoneKind::Graveyard, creature("test.copy_unknown", 1));
    w.env.card_copies = BTreeSet::from([unknown]);
    let card = w.card_of(unknown);
    w.chars.remove(&card);
    let out = pass(&mut w);
    let mut expected = vec![gy, unknown];
    expected.sort();
    assert_eq!(out.ceased, expected);
    assert!(out.executed.is_empty());
    let lki = w.lki.get(gy).unwrap();
    assert_eq!(lki.zone(), ZoneId::Graveyard(p(1)));
    assert_eq!(lki.owner(), p(1));
    assert!(lki.characteristics().names.contains("test.token_1_1"));
    assert!(w.lki.get(unknown).is_none());
    assert!(w.zones.object(unknown).is_none());
}

// REQ-SBA-001, REQ-CORE-003: a pass without ceasing objects captures no extra LKI.
#[test]
fn no_ceasing_no_extra_lki() {
    let mut w = World::new();
    w.set_life(1, 0);
    w.creature(1, 2);
    pass(&mut w);
    assert!(w.lki.is_empty());
}

// REQ-SBA-001, REQ-CORE-006: the all-or-nothing snapshot of a pass restores every component,
// the object-id generator included (the world digest encodes it): a rollback that forgot the
// generator would let the next creation issue a different id.
#[test]
fn saved_snapshot_restores_the_id_generator() {
    let mut w = World::new();
    w.creature(1, 2);
    let before = w.state_hash();
    let snap = w.snapshot();
    {
        let mut ctx = EventContext {
            game: &mut w.game,
            zones: &mut w.zones,
            ids: &mut w.ids,
            permanents: &mut w.permanents,
            memories: &mut w.memories,
            lki: &mut w.lki,
            events: &mut w.events,
            provider: &snap,
        };
        let saved = Saved::take(&ctx);
        ctx.ids.issue().unwrap();
        ctx.ids.issue().unwrap();
        saved.restore(&mut ctx);
    }
    assert_eq!(w.state_hash(), before);
    let mut issued = w.ids.clone();
    assert_eq!(issued.issue().unwrap(), ObjectId::new(2));
}

// REQ-SBA-001, REQ-CORE-003, CR 704.8, CR 704.5e: two battlefield spell copies cease in one pass;
// the host A (lower id) is removed first, yet the record of B (higher id, attached to A) keeps its
// attachment and the record of A lists B, because every record is captured before any removal.
#[test]
fn ceasing_copies_capture_all_lki_before_any_removal() {
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield, creature("test.copy_host", 3));
    let b = w.create(1, ZoneKind::Battlefield, creature("test.copy_attached", 3));
    assert!(a < b);
    w.permanents.attach(b, AttachTarget::Permanent(a)).unwrap();
    assert_eq!(w.permanents.attachments_of(AttachTarget::Permanent(a)), [b]);
    w.env.spell_copies = BTreeSet::from([a, b]);
    let out = pass(&mut w);
    assert_eq!(out.ceased, [a, b]);
    assert!(out.executed.is_empty());
    assert_eq!(w.lki.get(a).unwrap().attached_here(), [b]);
    assert_eq!(
        w.lki.get(b).unwrap().permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(a))
    );
}

// REQ-SBA-001, REQ-CORE-003, CR 704.8, CR 704.5f, CR 704.5e: the host A dies by a state-based
// action in the move batch while the attached spell copy B ceases in the same pass; the batch
// unattaches B, yet B's record keeps its attachment (derived from the state before any of the
// actions), and A's record lists B.
#[test]
fn ceasing_copy_lki_keeps_attachment_when_host_dies_in_batch() {
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield, creature("test.dying_host", 0));
    let b = w.create(1, ZoneKind::Battlefield, creature("test.copy_on_host", 3));
    w.permanents.attach(b, AttachTarget::Permanent(a)).unwrap();
    w.env.spell_copies = BTreeSet::from([b]);
    let out = pass(&mut w);
    assert_eq!(out.ceased, [b]);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        w.lki.get(b).unwrap().permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(a))
    );
}

// ---------------------------------------------------------------------------------------------
// CR 704.5j-704.5t (FP-041)
// ---------------------------------------------------------------------------------------------

/// A synthetic noncreature artifact with the given names (none: nameless, CR 201.2a).
fn named_artifact(names: &[&str]) -> Characteristics {
    let mut c = noncreature("test.unused", "Artifact");
    c.names = set(names);
    c
}

/// A synthetic legendary artifact with the given names.
fn legendary(names: &[&str]) -> Characteristics {
    let mut c = named_artifact(names);
    c.supertypes = set(&["Legendary"]);
    c
}

/// A synthetic world enchantment.
fn world_enchantment(name: &str) -> Characteristics {
    let mut c = noncreature(name, "Enchantment");
    c.supertypes = set(&["World"]);
    c
}

/// `c` with the subtypes `subtypes`.
fn with_subtypes(mut c: Characteristics, subtypes: &[&str]) -> Characteristics {
    c.subtypes = set(subtypes);
    c
}

/// A synthetic Aura.
fn aura() -> Characteristics {
    with_subtypes(noncreature("test.aura", "Enchantment"), &["Aura"])
}

/// A synthetic Equipment.
fn equipment() -> Characteristics {
    with_subtypes(noncreature("test.equipment", "Artifact"), &["Equipment"])
}

/// A synthetic Fortification.
fn fortification() -> Characteristics {
    with_subtypes(
        noncreature("test.fortification", "Artifact"),
        &["Fortification"],
    )
}

/// A synthetic Saga.
fn saga() -> Characteristics {
    with_subtypes(noncreature("test.saga", "Enchantment"), &["Saga"])
}

impl World {
    /// A permanent of `owner` controlled by `controller` with characteristics `c`.
    fn create_controlled(&mut self, owner: u64, controller: u64, c: Characteristics) -> ObjectId {
        let card = self.card(c);
        let mut req = MoveRequest::new(ZoneKind::Battlefield);
        req.controller = Some(p(controller));
        let id = self
            .zones
            .create_object(&mut self.ids, card, p(owner), req)
            .unwrap();
        self.permanents.enter(id, p(controller), 1).unwrap();
        id
    }

    fn attach(&mut self, id: ObjectId, target: AttachTarget) {
        self.permanents.attach(id, target).unwrap();
    }

    /// The object ids the check finds for `entry`, ascending.
    fn found_by(&mut self, entry: &str) -> Vec<ObjectId> {
        self.check()
            .into_iter()
            .filter(|f| f.entry_id == entry)
            .map(|f| match f.subject {
                SbaSubject::Object(o) => o,
                SbaSubject::Player(_) => panic!("player finding"),
            })
            .collect()
    }

    fn counter(&self, id: ObjectId, kind: &str) -> i64 {
        self.events
            .markers()
            .counters(id)
            .get(kind)
            .copied()
            .unwrap_or(0)
    }
}

/// Runs one pass on a clone of `w` in a choice session, answering `answers` in order; returns
/// the run and every request asked.
fn drive_answers(w: &World, answers: &[ChoiceResponse]) -> (Run, Vec<PendingChoiceRequest>) {
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let mut asked = Vec::new();
    let mut answers = answers.iter();
    loop {
        let pending = match session.status() {
            Status::Done { state, output } => {
                assert!(answers.next().is_none(), "unused answers");
                let run = Run {
                    asked: asked.len(),
                    state: state.clone(),
                    out: output.clone(),
                    hash: session.state_hash(),
                };
                return (run, asked);
            }
            Status::Pending(r) => r.clone(),
        };
        let answer = answers.next().expect("more requests than answers").clone();
        session.submit(pending.request_id, answer).unwrap();
        asked.push(pending);
    }
}

fn select(objects: &[ObjectId]) -> ChoiceResponse {
    ChoiceResponse::SubmitObjectSelection {
        objects: objects.to_vec(),
    }
}

/// The (object, cause) of every zone change and the (object, kind, amount) of every counter
/// removal submitted in the executed batches of `out`.
type BatchShape = (
    Vec<(ObjectId, ZoneChangeCause)>,
    Vec<(ObjectId, String, i64)>,
);

fn batch_shape(out: &SbaOutcome) -> BatchShape {
    let mut moves = Vec::new();
    let mut removals = Vec::new();
    for batch in &out.executed {
        for ev in &batch.events {
            match ev.instance.event() {
                GameEvent::ZoneChange { object, cause, .. } => moves.push((*object, *cause)),
                GameEvent::RemoveCounters {
                    target: CounterTarget::Object(o),
                    kind,
                    amount,
                } => removals.push((*o, kind.clone(), *amount)),
                other => panic!("unexpected event {other:?}"),
            }
        }
    }
    (moves, removals)
}

// REQ-SBA-001, CR 704.5j, CR 201.2a: two legendary permanents with the same name controlled by
// player 1 (one owned by player 2): player 1 is asked ONE SELECT_OBJECTS_FROM_ZONE choice
// (candidates ascending, exactly one, default the lowest id); the chosen one stays, the other goes
// to its OWNER's graveyard with an SBA cause. Player 2's legend with the same name does not
// interact (different controller).
#[test]
fn sba_704_5j_legend_rule() {
    let mut w = World::new();
    let a = w.create_controlled(2, 1, legendary(&["test.legend_a"]));
    let b = w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    let other = w.create_controlled(2, 2, legendary(&["test.legend_a"]));
    let unrelated = w.create_controlled(1, 1, legendary(&["test.legend_b"]));
    assert_eq!(w.found_by("704.5j"), [a, b]);
    let (card_a, card_b) = (w.card_of(a), w.card_of(b));

    let (run, asked) = drive_answers(&w, &[select(&[b])]);
    assert_eq!(run.asked, 1);
    let r = &asked[0];
    assert_eq!(r.chooser, p(1));
    assert_eq!(r.choice_type, ChoiceType::SelectObjectsFromZone);
    assert_eq!(r.reason.cr, "CR 704.5j");
    assert_eq!((r.min, r.max), (1, 1));
    assert_eq!(
        r.payload,
        ChoicePayload::SelectObjectsFromZone {
            candidates: vec![a, b]
        }
    );
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Object(a), ChoiceOption::Object(b)])
    );
    assert_eq!(r.default_on_timeout, select(&[a]));
    let out = run.out.unwrap();
    assert!(out.performed);
    assert_eq!(
        batch_shape(&out).0,
        [(a, ZoneChangeCause::StateBasedAction)]
    );
    // CR 704.5j: "their owners' graveyards".
    assert_eq!(
        run.state.zone_of_card(card_a),
        Some(ZoneId::Graveyard(p(2)))
    );
    assert_eq!(run.state.zone_of_card(card_b), Some(ZoneId::Battlefield));
    assert!(run.state.zones.object(other).is_some());
    assert!(run.state.zones.object(unrelated).is_some());

    // The other answer keeps the other one.
    let (run, _) = drive_answers(&w, &[select(&[a])]);
    assert_eq!(run.state.zone_of_card(card_a), Some(ZoneId::Battlefield));
    assert_eq!(
        run.state.zone_of_card(card_b),
        Some(ZoneId::Graveyard(p(1)))
    );
    // The default answer (timeout) is the lowest id.
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let id = session.pending().unwrap().request_id;
    session.submit_default(id).unwrap();
    let Status::Done { state, .. } = session.status() else {
        panic!("not done");
    };
    assert_eq!(state.zone_of_card(card_a), Some(ZoneId::Battlefield));
}

// REQ-SBA-001, CR 704.5j: two players each control one legend of the same name: no finding, no
// choice.
#[test]
fn sba_704_5j_different_controllers_do_not_interact() {
    let mut w = World::new();
    w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    w.create_controlled(1, 2, legendary(&["test.legend_a"]));
    assert!(w.check().is_empty());
    let out = pass(&mut w);
    assert!(!out.performed);
}

// REQ-SBA-001, CR 704.5j, CR 201.2a, CR 708.2a: nameless legendary permanents (for example face
// down) never have the same name, not even as each other; a nonlegendary permanent with the
// same name does not count.
#[test]
fn sba_704_5j_nameless_never_match() {
    let mut w = World::new();
    w.create_controlled(1, 1, legendary(&[]));
    w.create_controlled(1, 1, legendary(&[]));
    w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    w.create_controlled(1, 1, named_artifact(&["test.legend_a"]));
    assert!(w.check().is_empty());
}

// REQ-SBA-001, CR 704.5j, CR 201.2a: one name in common is the same name, even with additional
// names; two groups of one controller are asked in ascending order of their lowest id.
#[test]
fn sba_704_5j_shared_name_and_two_groups() {
    let mut w = World::new();
    let x1 = w.create_controlled(1, 1, legendary(&["test.legend_x", "test.legend_y"]));
    let z1 = w.create_controlled(1, 1, legendary(&["test.legend_z"]));
    let x2 = w.create_controlled(1, 1, legendary(&["test.legend_y", "test.legend_w"]));
    let z2 = w.create_controlled(1, 1, legendary(&["test.legend_z"]));
    assert_eq!(w.found_by("704.5j"), [x1, z1, x2, z2]);
    let (run, asked) = drive_answers(&w, &[select(&[x2]), select(&[z1])]);
    assert_eq!(run.asked, 2);
    assert_eq!(
        asked[0].payload,
        ChoicePayload::SelectObjectsFromZone {
            candidates: vec![x1, x2]
        }
    );
    assert_eq!(
        asked[1].payload,
        ChoicePayload::SelectObjectsFromZone {
            candidates: vec![z1, z2]
        }
    );
    let out = run.out.unwrap();
    // Both losers move in the ONE batch.
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        batch_shape(&out).0,
        [
            (x1, ZoneChangeCause::StateBasedAction),
            (z2, ZoneChangeCause::StateBasedAction)
        ]
    );
}

// REQ-SBA-001, CR 704.5j, CR 201.2a: names that overlap non-transitively (A {X, Y}, B {Y},
// C {X}) form ONE group of three (connected components, decision #42 FP-041).
#[test]
fn sba_704_5j_non_transitive_names_form_one_group() {
    let mut w = World::new();
    let a = w.create_controlled(1, 1, legendary(&["test.legend_x", "test.legend_y"]));
    let b = w.create_controlled(1, 1, legendary(&["test.legend_y"]));
    let c = w.create_controlled(1, 1, legendary(&["test.legend_x"]));
    let (run, asked) = drive_answers(&w, &[select(&[b])]);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].payload,
        ChoicePayload::SelectObjectsFromZone {
            candidates: vec![a, b, c]
        }
    );
    let mut moved: Vec<ObjectId> = batch_shape(&run.out.unwrap())
        .0
        .into_iter()
        .map(|(o, _)| o)
        .collect();
    moved.sort();
    assert_eq!(moved, [a, c]);
}

// REQ-SBA-001, CR 704.5j: a legend-rule answer that is not exactly one candidate is a protocol
// error (the session itself rejects such answers before they reach the pass).
#[test]
fn sba_704_5j_selection_protocol() {
    let (a, b, c) = (ObjectId::new(1), ObjectId::new(2), ObjectId::new(3));
    assert_eq!(legend_kept(&[a, b], select(&[b])).unwrap(), b);
    for bad in [
        select(&[]),
        select(&[a, b]),
        select(&[c]),
        ChoiceResponse::SelectReplacement { index: 0 },
    ] {
        let err = legend_kept(&[a, b], bad).unwrap_err();
        assert!(matches!(
            err,
            BusInterrupt::Error(BusError::Choice(ChoiceError::Procedure(_)))
        ));
    }
    // The session refuses a selection of two.
    let mut w = World::new();
    w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let r = session.pending().unwrap().clone();
    let ids = match &r.payload {
        ChoicePayload::SelectObjectsFromZone { candidates } => candidates.clone(),
        other => panic!("payload {other:?}"),
    };
    assert!(session.submit(r.request_id, select(&ids)).is_err());
}

// REQ-SBA-001, CR 704.5j, CR 704.3: a pending legend-rule choice leaves every component, the
// replacement registry and the flags exactly as before (also asserted inside the procedure).
#[test]
fn sba_704_5j_pending_choice_restores() {
    let mut w = World::new();
    let c = w.creature(1, 0);
    w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    w.setup(GameEvent::Draw { player: p(2) });
    w.env.deathtouch = BTreeSet::from([c]);
    let before = w.state_hash();
    let session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    assert!(matches!(session.status(), Status::Pending(_)));
    assert_eq!(session.state_hash(), before);
    assert_eq!(session.base().env.deathtouch, BTreeSet::from([c]));
    assert_eq!(session.base().events.empty_library_draws(), [p(2)]);
    assert!(session.base().zones.object(c).is_some());
}

// REQ-SBA-001, CR 704.5k: the world permanent with the unique largest stamp (the shortest time)
// stays, every other goes; with three, stamps 1, 5 and 3, only 5 stays.
#[test]
fn sba_704_5k_world_rule_unique_newest() {
    let mut w = World::new();
    let old = w.create(1, ZoneKind::Battlefield, world_enchantment("test.world_a"));
    let newest = w.create(2, ZoneKind::Battlefield, world_enchantment("test.world_b"));
    let mid = w.create(2, ZoneKind::Battlefield, world_enchantment("test.world_c"));
    w.env.world_since = BTreeMap::from([(old, 1), (newest, 5), (mid, 3)]);
    assert_eq!(w.found_by("704.5k"), [old, mid]);
    let cards = [w.card_of(old), w.card_of(newest), w.card_of(mid)];
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(
        batch_shape(&out).0,
        [
            (old, ZoneChangeCause::StateBasedAction),
            (mid, ZoneChangeCause::StateBasedAction)
        ]
    );
    assert_eq!(w.zone_of_card(cards[0]), Some(ZoneId::Graveyard(p(1))));
    assert_eq!(w.zone_of_card(cards[1]), Some(ZoneId::Battlefield));
    assert_eq!(w.zone_of_card(cards[2]), Some(ZoneId::Graveyard(p(2))));
}

// REQ-SBA-001, CR 704.5k: a tie for the shortest time sends all (also the older ones); the
// default stamp 0 ties everything; one world permanent alone stays; a nonworld permanent is
// not compared.
#[test]
fn sba_704_5k_world_rule_ties_and_single() {
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield, world_enchantment("test.world_a"));
    let b = w.create(2, ZoneKind::Battlefield, world_enchantment("test.world_b"));
    let c = w.create(1, ZoneKind::Battlefield, world_enchantment("test.world_c"));
    w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.plain", "Enchantment"),
    );
    w.env.world_since = BTreeMap::from([(a, 2), (b, 7), (c, 7)]);
    assert_eq!(w.found_by("704.5k"), [a, b, c]);
    w.env.world_since.clear();
    assert_eq!(w.found_by("704.5k"), [a, b, c]);
    let out = pass(&mut w);
    assert_eq!(batch_shape(&out).0.len(), 3);

    let mut w = World::new();
    w.create(1, ZoneKind::Battlefield, world_enchantment("test.world_a"));
    assert!(w.check().is_empty());
}

// REQ-SBA-001, CR 704.5m, CR 303.4c: an Aura not attached, an Aura the environment says may not
// enchant its creature, and an Aura on a player who lost the game go to their owners'
// graveyards; a legal Aura on a creature and on a remaining player stays.
#[test]
fn sba_704_5m_aura_illegal_or_unattached() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let loose = w.create(2, ZoneKind::Battlefield, aura());
    let illegal = w.create(1, ZoneKind::Battlefield, aura());
    let legal = w.create(1, ZoneKind::Battlefield, aura());
    let on_loser = w.create(2, ZoneKind::Battlefield, aura());
    let on_player = w.create(1, ZoneKind::Battlefield, aura());
    w.attach(illegal, AttachTarget::Permanent(host));
    w.attach(legal, AttachTarget::Permanent(host));
    w.attach(on_loser, AttachTarget::Player(p(2)));
    w.attach(on_player, AttachTarget::Player(p(1)));
    w.env.illegal_aura_targets = BTreeSet::from([(illegal, AttachTarget::Permanent(host))]);
    w.game
        .player_mut(p(2))
        .unwrap()
        .set_designation(LOST_THE_GAME, DesignationValue::Flag);
    assert_eq!(w.found_by("704.5m"), [loose, illegal, on_loser]);
    let cards = [w.card_of(loose), w.card_of(illegal), w.card_of(on_loser)];
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(w.zone_of_card(cards[0]), Some(ZoneId::Graveyard(p(2))));
    assert_eq!(w.zone_of_card(cards[1]), Some(ZoneId::Graveyard(p(1))));
    assert_eq!(w.zone_of_card(cards[2]), Some(ZoneId::Graveyard(p(2))));
    assert!(w.zones.object(legal).is_some());
    assert!(w.zones.object(on_player).is_some());
    assert!(out.unattached.is_empty());
}

// REQ-SBA-001, CR 704.5m, CR 303.4c: the enchanted creature leaves the battlefield (which
// unattaches the Aura) and, in a container whose link survived, the target no longer exists:
// both are found.
#[test]
fn sba_704_5m_aura_target_left() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let a = w.create(1, ZoneKind::Battlefield, aura());
    w.attach(a, AttachTarget::Permanent(host));
    w.move_to(host, ZoneKind::Graveyard);
    assert_eq!(w.found_by("704.5m"), [a]);

    let mut w = World::new();
    let host = w.creature(1, 3);
    let a = w.create(1, ZoneKind::Battlefield, aura());
    w.attach(a, AttachTarget::Permanent(host));
    // The object is gone from its zone; the stale link remains.
    w.zones.cease_to_exist(host).unwrap();
    assert_eq!(
        w.permanents.attached_to(a),
        Some(AttachTarget::Permanent(host))
    );
    assert_eq!(w.found_by("704.5m"), [a]);
    let card = w.card_of(a);
    pass(&mut w);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
}

// REQ-SBA-001, CR 704.5m, CR 303.4d, CR 301.5c: an attachment to itself (which the container
// refuses to create) and to a vanished permanent or a player who left is judged illegal.
#[test]
fn sba_704_5m_attached_to_self_is_illegal() {
    let me = ObjectId::new(7);
    let other = ObjectId::new(8);
    let yes = |_: ObjectId| true;
    let remaining = |_: PlayerId| true;
    assert!(target_self_or_gone(
        me,
        AttachTarget::Permanent(me),
        &yes,
        &remaining
    ));
    assert!(!target_self_or_gone(
        me,
        AttachTarget::Permanent(other),
        &yes,
        &remaining
    ));
    assert!(target_self_or_gone(
        me,
        AttachTarget::Permanent(other),
        &|_| false,
        &remaining
    ));
    assert!(!target_self_or_gone(
        me,
        AttachTarget::Player(p(1)),
        &yes,
        &remaining
    ));
    assert!(target_self_or_gone(
        me,
        AttachTarget::Player(p(1)),
        &yes,
        &|_| false
    ));
    // The container refuses a self-attachment outright.
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield, aura());
    assert!(w.permanents.attach(a, AttachTarget::Permanent(a)).is_err());
}

// REQ-SBA-001, CR 704.5m, CR 303.4d, CR 704.5p: an Aura that is also a creature and is attached
// goes to the graveyard (704.5m); its 704.5p finding is not performed as an unattach (the move
// wins).
#[test]
fn sba_704_5m_aura_creature() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let mut c = with_subtypes(creature("test.aura_creature", 2), &["Aura"]);
    c.card_types = set(&["Creature", "Enchantment"]);
    let a = w.create(1, ZoneKind::Battlefield, c);
    w.attach(a, AttachTarget::Permanent(host));
    assert_eq!(w.found(obj(a)), ["704.5m", "704.5p"]);
    let card = w.card_of(a);
    let out = pass(&mut w);
    assert!(out.unattached.is_empty());
    assert_eq!(
        batch_shape(&out).0,
        [(a, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
}

// REQ-SBA-001, CR 704.5m, CR 310.10, CR 704.5p: an attached Aura that is also a battle goes to
// the graveyard (decision #42 FP-041); the move wins over the unattach.
#[test]
fn sba_704_5m_aura_battle() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let mut c = aura();
    c.card_types = set(&["Battle", "Enchantment"]);
    let a = w.create(1, ZoneKind::Battlefield, c);
    w.attach(a, AttachTarget::Permanent(host));
    assert_eq!(w.found(obj(a)), ["704.5m", "704.5p"]);
    let out = pass(&mut w);
    assert!(out.unattached.is_empty());
    assert_eq!(
        batch_shape(&out).0,
        [(a, ZoneChangeCause::StateBasedAction)]
    );
}

// REQ-SBA-001, CR 704.5n, CR 301.5, CR 301.6: an Equipment on a player and on a noncreature
// artifact, and a Fortification on a creature (not a land), become unattached and stay on the
// battlefield; an Equipment on a creature, a Fortification on a land and an unattached
// Equipment are untouched.
#[test]
fn sba_704_5n_equipment_and_fortification() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let land = w.create(1, ZoneKind::Battlefield, noncreature("test.land", "Land"));
    let rock = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.rock", "Artifact"),
    );
    let on_player = w.create(1, ZoneKind::Battlefield, equipment());
    let on_rock = w.create(1, ZoneKind::Battlefield, equipment());
    let legal = w.create(1, ZoneKind::Battlefield, equipment());
    let loose = w.create(1, ZoneKind::Battlefield, equipment());
    let fort_bad = w.create(2, ZoneKind::Battlefield, fortification());
    let fort_ok = w.create(2, ZoneKind::Battlefield, fortification());
    w.attach(on_player, AttachTarget::Player(p(2)));
    w.attach(on_rock, AttachTarget::Permanent(rock));
    w.attach(legal, AttachTarget::Permanent(host));
    w.attach(fort_bad, AttachTarget::Permanent(host));
    w.attach(fort_ok, AttachTarget::Permanent(land));
    assert_eq!(w.found_by("704.5n"), [on_player, on_rock, fort_bad]);
    assert!(w.found(obj(loose)).is_empty());
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.unattached, [on_player, on_rock, fort_bad]);
    assert!(out.executed.is_empty());
    for id in [on_player, on_rock, fort_bad, loose] {
        assert_eq!(w.zones.object(id).unwrap().zone, ZoneId::Battlefield);
        assert_eq!(w.permanents.attached_to(id), None);
    }
    assert_eq!(
        w.permanents.attached_to(legal),
        Some(AttachTarget::Permanent(host))
    );
    assert_eq!(
        w.permanents.attached_to(fort_ok),
        Some(AttachTarget::Permanent(land))
    );
    assert!(w.check().is_empty());
}

// REQ-SBA-001, CR 704.5n, CR 301.5c: an Equipment whose link names a permanent that no longer
// exists becomes unattached; an Equipment attached to a permanent the layers cannot evaluate is
// not judged.
#[test]
fn sba_704_5n_equipment_target_gone_or_unknown() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    w.attach(eq, AttachTarget::Permanent(host));
    w.zones.cease_to_exist(host).unwrap();
    assert_eq!(w.found_by("704.5n"), [eq]);

    let mut w = World::new();
    let host = w.creature(1, 3);
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    w.attach(eq, AttachTarget::Permanent(host));
    let card = w.card_of(host);
    w.chars.remove(&card);
    assert!(w.found_by("704.5n").is_empty());
}

// REQ-SBA-001, CR 704.5p, CR 310.10: an attached creature, an attached battle and an attached
// plain artifact become unattached and stay; a creature Equipment is unattached by 704.5p.
#[test]
fn sba_704_5p_cannot_be_attached() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let c = w.creature(1, 2);
    let battle = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.battle", "Battle"),
    );
    let rock = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.rock", "Artifact"),
    );
    let mut eq = with_subtypes(creature("test.equipment_creature", 2), &["Equipment"]);
    eq.card_types = set(&["Artifact", "Creature"]);
    let eq = w.create(1, ZoneKind::Battlefield, eq);
    w.attach(c, AttachTarget::Permanent(host));
    w.attach(battle, AttachTarget::Permanent(host));
    w.attach(rock, AttachTarget::Player(p(1)));
    w.attach(eq, AttachTarget::Permanent(host));
    assert_eq!(w.found_by("704.5p"), [c, battle, rock, eq]);
    assert!(w.found_by("704.5n").is_empty());
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.unattached, [c, battle, rock, eq]);
    for id in [c, battle, rock, eq] {
        assert_eq!(w.zones.object(id).unwrap().zone, ZoneId::Battlefield);
        assert_eq!(w.permanents.attached_to(id), None);
    }
    assert!(!pass(&mut w).performed);
}

// REQ-SBA-001, CR 704.5p, CR 704.8: an attached creature that also dies in the pass is moved,
// not unattached; its last known information keeps the attachment.
#[test]
fn sba_704_5p_move_wins_over_unattach() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let c = w.creature(1, 0);
    w.attach(c, AttachTarget::Permanent(host));
    assert_eq!(w.found(obj(c)), ["704.5f", "704.5p"]);
    let out = pass(&mut w);
    assert!(out.unattached.is_empty());
    assert_eq!(
        w.lki.get(c).unwrap().permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(host))
    );
}

// REQ-SBA-001, CR 704.5q, CR 122.3: 3 +1/+1 and 5 -1/-1 counters: 3 of each are removed, 2
// -1/-1 remain; in the other direction 1 +1/+1 remains. Only one kind: untouched.
#[test]
fn sba_704_5q_opposed_counters() {
    let mut w = World::new();
    let a = w.creature(1, 9);
    let b = w.creature(2, 9);
    let only = w.creature(1, 9);
    w.counters(a, "+1/+1", 3);
    w.counters(a, "-1/-1", 5);
    w.counters(b, "+1/+1", 4);
    w.counters(b, "-1/-1", 3);
    w.counters(only, "+1/+1", 2);
    assert_eq!(w.found_by("704.5q"), [a, b]);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        batch_shape(&out).1,
        [
            (a, "+1/+1".to_owned(), 3),
            (a, "-1/-1".to_owned(), 3),
            (b, "+1/+1".to_owned(), 3),
            (b, "-1/-1".to_owned(), 3),
        ]
    );
    assert_eq!((w.counter(a, "+1/+1"), w.counter(a, "-1/-1")), (0, 2));
    assert_eq!((w.counter(b, "+1/+1"), w.counter(b, "-1/-1")), (1, 0));
    assert_eq!(w.counter(only, "+1/+1"), 2);
    assert!(!pass(&mut w).performed);
}

// REQ-SBA-001, CR 704.5r: 5 counters with a limit of 2: 3 are removed; at the limit untouched; a
// negative limit counts as 0.
#[test]
fn sba_704_5r_counter_limit() {
    let mut w = World::new();
    let over = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.limited", "Artifact"),
    );
    let at = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.limited_b", "Artifact"),
    );
    let neg = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.limited_c", "Artifact"),
    );
    w.counters(over, "charge", 5);
    w.counters(at, "charge", 2);
    w.counters(neg, "charge", 1);
    let limit = |n: i64| BTreeMap::from([("charge".to_owned(), n)]);
    w.env.counter_limits = BTreeMap::from([(over, limit(2)), (at, limit(2)), (neg, limit(-1))]);
    assert_eq!(w.found_by("704.5r"), [over, neg]);
    let out = pass(&mut w);
    assert_eq!(
        batch_shape(&out).1,
        [
            (over, "charge".to_owned(), 3),
            (neg, "charge".to_owned(), 1)
        ]
    );
    assert_eq!(w.counter(over, "charge"), 2);
    assert_eq!(w.counter(at, "charge"), 2);
    assert_eq!(w.counter(neg, "charge"), 0);
}

// REQ-SBA-001, CR 704.5q, CR 704.5r: both entries remove +1/+1 counters of one permanent: one
// event of that kind with the larger amount (decision #42 FP-041).
#[test]
fn sba_704_5q_and_5r_same_kind() {
    let mut w = World::new();
    let a = w.creature(1, 9);
    w.counters(a, "+1/+1", 5);
    w.counters(a, "-1/-1", 2);
    w.env.counter_limits = BTreeMap::from([(a, BTreeMap::from([("+1/+1".to_owned(), 1)]))]);
    assert_eq!(w.found(obj(a)), ["704.5q", "704.5r"]);
    let out = pass(&mut w);
    assert_eq!(
        batch_shape(&out).1,
        [(a, "+1/+1".to_owned(), 4), (a, "-1/-1".to_owned(), 2)]
    );
    assert_eq!((w.counter(a, "+1/+1"), w.counter(a, "-1/-1")), (1, 0));
}

// REQ-SBA-001, CR 704.5s, CR 715.2d, CR 715.4: a Saga with lore counters equal to (and above)
// its final chapter is sacrificed (cause Sacrifice, owner's graveyard); fewer lore counters, a
// pending chapter ability, no chapter abilities, or a non-Saga: untouched.
#[test]
fn sba_704_5s_saga() {
    let mut w = World::new();
    let done = w.create_controlled(2, 1, saga());
    let over = w.create(1, ZoneKind::Battlefield, saga());
    let short = w.create(1, ZoneKind::Battlefield, saga());
    let pending = w.create(1, ZoneKind::Battlefield, saga());
    let no_chapters = w.create(1, ZoneKind::Battlefield, saga());
    let not_saga = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.ench", "Enchantment"),
    );
    for (id, lore) in [
        (done, 3),
        (over, 4),
        (short, 2),
        (pending, 3),
        (no_chapters, 3),
        (not_saga, 3),
    ] {
        w.counters(id, "lore", lore);
    }
    w.env.saga_final_chapter = BTreeMap::from([
        (done, 3),
        (over, 3),
        (short, 3),
        (pending, 3),
        (not_saga, 3),
    ]);
    w.env.chapter_pending = BTreeSet::from([pending]);
    assert_eq!(w.found_by("704.5s"), [done, over]);
    let card = w.card_of(done);
    let out = pass(&mut w);
    assert_eq!(
        batch_shape(&out).0,
        [
            (done, ZoneChangeCause::Sacrifice),
            (over, ZoneChangeCause::Sacrifice)
        ]
    );
    // Controlled by player 1, owned by player 2: the owner's graveyard.
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(2))));
    for id in [short, pending, no_chapters, not_saga] {
        assert!(w.zones.object(id).is_some());
    }
}

// REQ-SBA-001, CR 704.5s, CR 704.5f: a Saga creature that is also a 704.5f finding moves once,
// with the Sacrifice cause (decision #42 FP-041).
#[test]
fn sba_704_5s_sacrifice_cause_wins() {
    let mut w = World::new();
    let mut c = with_subtypes(creature("test.saga_creature", 0), &["Saga"]);
    c.card_types = set(&["Creature", "Enchantment"]);
    let s = w.create(1, ZoneKind::Battlefield, c);
    w.counters(s, "lore", 1);
    w.env.saga_final_chapter = BTreeMap::from([(s, 1)]);
    assert_eq!(w.found(obj(s)), ["704.5f", "704.5s"]);
    let out = pass(&mut w);
    assert_eq!(batch_shape(&out).0, [(s, ZoneChangeCause::Sacrifice)]);
}

// REQ-SBA-001, CR 704.5t, CR 309.2c, CR 309.7: a dungeon card in the command zone with the
// venture marker on its bottommost room is removed from the game (not a zone change, reported in
// removed_from_game, not in ceased); a pending room ability blocks it; an id that is not in the
// command zone is ignored.
#[test]
fn sba_704_5t_dungeon() {
    let mut w = World::new();
    let done = w.create(
        1,
        ZoneKind::Command,
        noncreature("test.dungeon_a", "Dungeon"),
    );
    let pending = w.create(
        2,
        ZoneKind::Command,
        noncreature("test.dungeon_b", "Dungeon"),
    );
    let elsewhere = w.create(1, ZoneKind::Exile, noncreature("test.dungeon_c", "Dungeon"));
    w.env.bottommost_dungeons = BTreeSet::from([done, pending, elsewhere, ObjectId::new(999)]);
    w.env.room_pending = BTreeSet::from([pending]);
    assert_eq!(w.found_by("704.5t"), [done]);
    let card = w.card_of(done);
    let log_before = w.events.log().len();
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.removed_from_game, [done]);
    assert!(out.ceased.is_empty());
    assert!(out.executed.is_empty());
    assert_eq!(w.events.log().len(), log_before);
    assert!(w.zones.object(done).is_none());
    assert_eq!(w.zone_of_card(card), None);
    // Last known information is captured from the command zone.
    assert_eq!(w.lki.get(done).unwrap().zone(), ZoneId::Command);
    assert!(w.zones.object(pending).is_some());
    assert!(w.zones.object(elsewhere).is_some());
}

// REQ-SBA-001, CR 704.5t, CR 704.3: a completed dungeon removed in the same pass as a move:
// its last known information is from before the pass.
#[test]
fn sba_704_5t_with_moves() {
    let mut w = World::new();
    let c = w.creature(1, 0);
    let d = w.create(
        1,
        ZoneKind::Command,
        noncreature("test.dungeon_a", "Dungeon"),
    );
    w.counters(d, "test.marker", 1);
    w.env.bottommost_dungeons = BTreeSet::from([d]);
    let out = pass(&mut w);
    assert_eq!(out.removed_from_game, [d]);
    assert_eq!(
        batch_shape(&out).0,
        [(c, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(
        w.lki.get(d).unwrap().counters().get("test.marker"),
        Some(&1)
    );
}

/// A board with at least one finding of each of CR 704.5j-704.5t, plus a creature with
/// toughness 0 (CR 704.5f). Returns the world and the legend pair.
fn new_entries_board() -> (World, [ObjectId; 2]) {
    let mut w = World::new();
    let l1 = w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    let l2 = w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    w.create(1, ZoneKind::Battlefield, world_enchantment("test.world_a"));
    w.create(2, ZoneKind::Battlefield, world_enchantment("test.world_b"));
    w.create(1, ZoneKind::Battlefield, aura());
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    w.attach(eq, AttachTarget::Player(p(1)));
    let host = w.creature(2, 3);
    let attached = w.creature(2, 2);
    w.attach(attached, AttachTarget::Permanent(host));
    let q = w.creature(1, 9);
    w.counters(q, "+1/+1", 1);
    w.counters(q, "-1/-1", 1);
    let r = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.limited", "Artifact"),
    );
    w.counters(r, "charge", 3);
    w.env.counter_limits = BTreeMap::from([(r, BTreeMap::from([("charge".to_owned(), 1)]))]);
    let s = w.create(2, ZoneKind::Battlefield, saga());
    w.counters(s, "lore", 2);
    w.env.saga_final_chapter = BTreeMap::from([(s, 2)]);
    let d = w.create(2, ZoneKind::Command, noncreature("test.dungeon", "Dungeon"));
    w.env.bottommost_dungeons = BTreeSet::from([d]);
    w.creature(2, 0);
    (w, [l1, l2])
}

const NEW_ENTRIES: [&str; 9] = [
    "704.5j", "704.5k", "704.5m", "704.5n", "704.5p", "704.5q", "704.5r", "704.5s", "704.5t",
];

// REQ-SBA-001, CR 704.3, CR 704.6: every new entry is active in Two-Headed Giant games, and the
// old and new entries are found together and performed in ONE pass with ONE batch.
#[test]
fn new_entries_one_pass_and_two_headed_giant() {
    for variant in [GameVariant::Standard, GameVariant::TwoHeadedGiant] {
        let (mut w, [l1, l2]) = new_entries_board();
        w.env.variant = variant;
        let entries: BTreeSet<&str> = w.check().into_iter().map(|f| f.entry_id).collect();
        let mut expected: BTreeSet<&str> = NEW_ENTRIES.into_iter().collect();
        expected.insert("704.5f");
        assert_eq!(entries, expected, "{variant:?}");
        let (run, asked) = drive_answers(&w, &[select(&[l2])]);
        assert_eq!(asked.len(), 1);
        let out = run.out.unwrap();
        assert!(out.performed);
        assert_eq!(out.executed.len(), 1);
        let (moves, removals) = batch_shape(&out);
        // l1, two worlds, the Aura, the Saga, the 0-toughness creature.
        assert_eq!(moves.len(), 6);
        assert!(moves.contains(&(l1, ZoneChangeCause::StateBasedAction)));
        assert_eq!(removals.len(), 3);
        assert_eq!(out.unattached.len(), 2);
        assert_eq!(out.removed_from_game.len(), 1);
        // Nothing left: the next pass does nothing.
        let mut w = run.state;
        assert!(!pass(&mut w).performed);
    }
}

// REQ-SBA-001, REQ-CORE-001: the same input gives the same outcome and state hash twice.
#[test]
fn new_entries_deterministic() {
    let (w, [_, l2]) = new_entries_board();
    let (a, _) = drive_answers(&w, &[select(&[l2])]);
    let (b, _) = drive_answers(&w, &[select(&[l2])]);
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.out, b.out);
}

// REQ-SBA-001, CR 702.26b: phased-out permanents are skipped as legend-group members, world
// permanents, Auras, attachments, attachment targets, counter holders and Sagas.
#[test]
fn new_entries_skip_phased_out() {
    let mut w = World::new();
    w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    let l2 = w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    w.phase(l2, true);
    w.create(1, ZoneKind::Battlefield, world_enchantment("test.world_a"));
    let w2 = w.create(1, ZoneKind::Battlefield, world_enchantment("test.world_b"));
    w.phase(w2, true);
    let loose = w.create(1, ZoneKind::Battlefield, aura());
    w.phase(loose, true);
    let host = w.creature(1, 3);
    let on_host = w.create(1, ZoneKind::Battlefield, aura());
    w.env.illegal_aura_targets = BTreeSet::from([(on_host, AttachTarget::Permanent(host))]);
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    let rock = w.create(
        1,
        ZoneKind::Battlefield,
        noncreature("test.rock", "Artifact"),
    );
    w.attach(on_host, AttachTarget::Permanent(host));
    w.attach(eq, AttachTarget::Permanent(rock));
    w.phase(host, true);
    w.phase(rock, true);
    let q = w.creature(1, 9);
    w.counters(q, "+1/+1", 1);
    w.counters(q, "-1/-1", 1);
    w.phase(q, true);
    let s = w.create(1, ZoneKind::Battlefield, saga());
    w.counters(s, "lore", 1);
    w.env.saga_final_chapter = BTreeMap::from([(s, 1)]);
    w.phase(s, true);
    assert!(w.check().is_empty());
    assert!(!pass(&mut w).performed);
}

/// A tiny deterministic generator for the property tests (no external crate; R3).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }
}

// REQ-SBA-001, REQ-CORE-001, CR 704.5k (property): for random world stamps, at most one world
// permanent survives, it survives exactly when its stamp is the unique maximum, and the result
// does not depend on the object-id order (the same stamps assigned in reverse creation order).
#[test]
fn property_world_rule() {
    let mut rng = Lcg(42);
    for _ in 0..200 {
        let n = usize::try_from(rng.next(5) + 1).unwrap();
        let stamps: Vec<u64> = (0..n).map(|_| rng.next(4)).collect();
        let survivors = |order_reversed: bool| {
            let mut w = World::new();
            let mut ids = vec![ObjectId::new(0); n];
            let order: Vec<usize> = if order_reversed {
                (0..n).rev().collect()
            } else {
                (0..n).collect()
            };
            for &i in &order {
                ids[i] = w.create(1, ZoneKind::Battlefield, world_enchantment("test.world"));
                w.env.world_since.insert(ids[i], stamps[i]);
            }
            let gone: BTreeSet<ObjectId> = w.found_by("704.5k").into_iter().collect();
            (0..n)
                .filter(|&i| !gone.contains(&ids[i]))
                .collect::<Vec<usize>>()
        };
        let a = survivors(false);
        assert!(a.len() <= 1);
        assert_eq!(a, survivors(true));
        let max = stamps.iter().max().copied().unwrap();
        let unique = stamps.iter().filter(|&&s| s == max).count() == 1;
        if n == 1 || unique {
            assert_eq!(a.len(), 1);
            assert_eq!(stamps[a[0]], max);
        } else {
            assert!(a.is_empty());
        }
    }
}

// REQ-SBA-001, CR 704.5j (property): for random legendary boards (random names, controllers and
// answers), after the pass each controller keeps at most one legend of each name, every legend
// without a same-named partner stays, and a second pass finds no legend-rule finding.
#[test]
fn property_legend_rule() {
    let mut rng = Lcg(7);
    let names = ["test.legend_a", "test.legend_b", "test.legend_c"];
    for _ in 0..100 {
        let mut w = World::new();
        let n = rng.next(6) + 2;
        for _ in 0..n {
            let controller = rng.next(2) + 1;
            let name = names[usize::try_from(rng.next(3)).unwrap()];
            w.create_controlled(1, controller, legendary(&[name]));
        }
        let groups = {
            let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
            let mut count = 0;
            while let Some(r) = session.pending().cloned() {
                let ChoicePayload::SelectObjectsFromZone { candidates } = &r.payload else {
                    panic!("payload");
                };
                let pick = candidates[usize::try_from(rng.next(candidates.len() as u64)).unwrap()];
                session.submit(r.request_id, select(&[pick])).unwrap();
                count += 1;
            }
            let Status::Done { state, .. } = session.status() else {
                panic!("not done");
            };
            let mut after = state.clone();
            let mut kept: BTreeMap<(PlayerId, String), usize> = BTreeMap::new();
            for id in after.zones.contents(ZoneId::Battlefield) {
                let controller = after.permanents.get(id).unwrap().controller();
                let card = after.card_of(id);
                let name = after.chars[&card].names.iter().next().unwrap().clone();
                *kept.entry((controller, name)).or_default() += 1;
            }
            assert!(kept.values().all(|&k| k == 1));
            assert!(after.found_by("704.5j").is_empty());
            count
        };
        assert!(groups <= 6);
    }
}
