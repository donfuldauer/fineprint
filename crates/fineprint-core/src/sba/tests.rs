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
    ChoiceSession, ChoiceType, Interrupt, PendingChoiceRequest, RequestId, Status,
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
        rules_text: TextTokenStream::default(),
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

/// A synthetic `KEYWORD` ability with keyword name `word` (for example `INDESTRUCTIBLE`).
fn keyword(word: &str) -> AbilityAST {
    AbilityAST::Keyword(fineprint_dsl::ability::KeywordInstanceAST::new(
        fineprint_dsl::ability::AbilityBase::new(format!("test.kw.{word}")),
        word,
    ))
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

/// The layer environment over `zones` and the object markers of `events`, with the synthetic
/// characteristics `chars` per card.
fn snapshot_of(
    zones: &Zones,
    events: &EventState,
    chars: &BTreeMap<CardId, Characteristics>,
) -> Snapshot {
    let mut bases = BTreeMap::new();
    let mut counters = BTreeMap::new();
    for id in zones.all_object_ids() {
        let record = zones.object(id).unwrap();
        let Some(c) = chars.get(&record.card) else {
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
        counters.insert(id, events.markers().counters(id));
    }
    Snapshot { bases, counters }
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
        snapshot_of(&self.zones, &self.events, &self.chars)
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
        // CR 704.5x, CR 704.5y: the recorded protectors are part of what a pass may change.
        for (battle, protector) in &self.env.battle_protectors {
            bytes.extend(encode_to_vec(battle));
            bytes.extend(encode_to_vec(protector));
        }
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
/// that the world is exactly as before the call, then propagates. An invalid answer
/// (`ChoiceError::Invalid`, the procedure-level validation of decision "#28 FP-027 — Card-name
/// validation is structural") propagates as a host procedure does, through
/// `Interrupt::from`, so the session rejects it; every other error is the run's output.
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
    settle(w, before, result)
}

/// The session result of one pass of `w` whose hash before the pass was `before`: a pending
/// choice or an error asserts that the world is exactly as before the call, then propagates
/// (an invalid answer through `Interrupt::from`); every other error is the run's output.
fn settle(
    w: &World,
    before: Digest,
    result: Result<SbaOutcome, BusInterrupt>,
) -> Result<Out, Interrupt> {
    match result {
        Ok(outcome) => Ok(Ok(outcome)),
        Err(BusInterrupt::Pending(r)) => {
            assert_eq!(w.state_hash(), before, "a pending pass changed the world");
            Err(Interrupt::Pending(r))
        }
        Err(BusInterrupt::Error(err)) => {
            assert_eq!(w.state_hash(), before, "a failed pass changed the world");
            if matches!(err, BusError::Choice(ChoiceError::Invalid { .. })) {
                return Err(Interrupt::from(BusInterrupt::Error(err)));
            }
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

// REQ-SBA-001, SRS 9: one entry per rule CR 704.5a-704.5t and CR 704.5v-704.5aa (no 704.5l, no
// 704.5o, no 704.5u), in order, with id, crRef, action and variant gate; CR 704.5c is inactive in
// Two-Headed Giant games, every other entry (CR 704.5a, CR 704.5j-704.5t and CR 704.5v-704.5aa
// included) is active.
#[test]
fn registry_shape() {
    let r = SbaRegistry::standard();
    let ids: Vec<&str> = r.entries().iter().map(|e| e.id).collect();
    assert_eq!(
        ids,
        [
            "704.5a", "704.5b", "704.5c", "704.5d", "704.5e", "704.5f", "704.5g", "704.5h",
            "704.5i", "704.5j", "704.5k", "704.5m", "704.5n", "704.5p", "704.5q", "704.5r",
            "704.5s", "704.5t", "704.5v", "704.5w", "704.5x", "704.5y", "704.5z", "704.5aa"
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
            SbaAction::PutIntoGraveyard,
            SbaAction::PutIntoGraveyard,
            SbaAction::ChooseProtector,
            SbaAction::ChooseProtector,
            SbaAction::PutIntoGraveyard,
            SbaAction::SetSpeed,
        ]
    );
    for gone in ["704.5l", "704.5o", "704.5u"] {
        assert!(r.get(gone).is_none());
    }
    for id in [
        "704.5j", "704.5k", "704.5m", "704.5n", "704.5p", "704.5q", "704.5r", "704.5s", "704.5t",
        "704.5v", "704.5w", "704.5x", "704.5y", "704.5z", "704.5aa",
    ] {
        assert_eq!(r.get(id).unwrap().gate, VariantGate::All);
    }
    assert_eq!(r.clone().entries().len(), 24);
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
    tough.abilities.push(keyword("INDESTRUCTIBLE"));
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

// REQ-SBA-001, REQ-LAYER-001, CR 613.1, CR 613.4: the snapshot takes one layer pass; with an
// object of unknown base on the battlefield the whole-pass evaluation fails, so the snapshot
// falls back to per-object evaluation: the unknown object is skipped, the others are evaluated.
#[test]
fn unknown_layer_object_skipped_others_evaluated() {
    let mut w = World::new();
    let ghost = w.creature(1, 0);
    let dead = w.creature(1, 0);
    let fine = w.creature(1, 2);
    let card = w.card_of(ghost);
    w.chars.remove(&card);
    assert_eq!(w.found(obj(dead)), ["704.5f"]);
    assert!(w.found(obj(ghost)).is_empty());
    assert!(w.found(obj(fine)).is_empty());
    pass(&mut w);
    assert!(w.zones.object(ghost).is_some());
    assert!(w.zones.object(dead).is_none());
    assert!(w.zones.object(fine).is_some());
}

// REQ-SBA-001, REQ-LAYER-001, CR 613.1, CR 613.4: a normal multi-permanent board (all known)
// is evaluated in one pass and every permanent is read.
#[test]
fn multi_permanent_board_each_evaluated() {
    let mut w = World::new();
    let a = w.creature(1, 0);
    let b = w.creature(2, 3);
    let c = w.creature(1, 1);
    assert_eq!(w.found(obj(a)), ["704.5f"]);
    assert!(w.found(obj(b)).is_empty());
    assert!(w.found(obj(c)).is_empty());
    pass(&mut w);
    assert!(w.zones.object(a).is_none());
    assert!(w.zones.object(b).is_some());
    assert!(w.zones.object(c).is_some());
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
// is also a CR 704.5q finding: although it moves, its counter removal (N = 1 of each) is in the
// SAME batch (CR 704.3: every applicable SBA in one event; decision "#42 FP-041 — One SBA batch:
// moves, counter removals and unattach events"), executed before the zone changes, with no
// look-back: its last known information still shows every counter.
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
    assert_eq!(batch.events.len(), 5);
    let (moves, removals) = batch_shape(&out);
    assert_eq!(moves.len(), 3);
    // CR 701.8b (#209 FP-155): the lethal-damage creature (CR 704.5g) is destroyed, cause
    // `StateBasedDestroy`; the 0-toughness creature (CR 704.5f) and the 0-loyalty planeswalker
    // (CR 704.5i) are put into the graveyard, cause `StateBasedAction`.
    for &(o, cause) in &moves {
        let card = w.lki.get(o).expect("LKI of a moved permanent").card();
        let want = if card == cards[1] {
            ZoneChangeCause::StateBasedDestroy
        } else {
            ZoneChangeCause::StateBasedAction
        };
        assert_eq!(cause, want, "{o:?}");
    }
    let zero = moves
        .iter()
        .map(|&(o, _)| o)
        .find(|&o| w.lki.get(o).is_some_and(|r| r.card() == cards[0]))
        .expect("the zero-toughness creature moved");
    assert_eq!(
        removals,
        [(zero, "+1/+1".to_owned(), 1), (zero, "-1/-1".to_owned(), 1)]
    );
    assert!(unattach_events(&out).is_empty());
    // The bus executes the counter removals before the zone changes.
    assert!(matches!(
        batch.events[0].instance.event(),
        GameEvent::RemoveCounters { .. }
    ));
    assert!(matches!(
        batch.events[1].instance.event(),
        GameEvent::RemoveCounters { .. }
    ));
    for ev in &batch.events[2..] {
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
// SBA destruction (cause StateBasedDestroy).
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
    c.abilities.push(keyword("INDESTRUCTIBLE"));
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
/// removal submitted in the executed batches of `out`. Unattach events (CR 704.5n, CR 704.5p)
/// are read by [`unattach_events`].
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
                GameEvent::Unattach { .. } => {}
                other => panic!("unexpected event {other:?}"),
            }
        }
    }
    (moves, removals)
}

/// The attachment of every `Unattach` event executed in `out`, in execution order.
fn unattach_events(out: &SbaOutcome) -> Vec<ObjectId> {
    out.executed
        .iter()
        .flat_map(|b| &b.events)
        .filter_map(|ev| match ev.instance.event() {
            GameEvent::Unattach { attachment } => Some(*attachment),
            _ => None,
        })
        .collect()
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

/// The ids of the battlefield objects of `w` among `ids`, ascending.
fn on_battlefield(w: &World, ids: &[ObjectId]) -> Vec<ObjectId> {
    ids.iter()
        .copied()
        .filter(|&id| {
            w.zones
                .object(id)
                .is_some_and(|r| r.zone == ZoneId::Battlefield)
        })
        .collect()
}

/// The pass procedure as a function pointer.
type PassProcedure = fn(&mut World, &mut ChoiceContext<'_>) -> Result<Out, Interrupt>;

/// A choice session over one pass.
type PassSession = ChoiceSession<World, Out, PassProcedure>;

/// Asserts that answering `answer` to the only legend-rule request of `w` is rejected by
/// `ChoiceSession::submit` with `ChoiceError::Invalid` naming that request, records nothing and
/// leaves the same request pending with the state unchanged (also asserted inside the
/// procedure); returns the session for a corrected answer (REQ-SBA-001, CR 704.5j; decision
/// #42 FP-041, round 2).
fn assert_legend_answer_rejected(w: &World, answer: &[ObjectId]) -> PassSession {
    let procedure: PassProcedure = run_pass;
    let mut session = ChoiceSession::start(w.clone(), procedure).unwrap();
    let pending = session.pending().expect("a legend-rule request").clone();
    let err = session
        .submit(pending.request_id, select(answer))
        .expect_err("an invalid survivor set was accepted");
    match err {
        ChoiceError::Invalid {
            request_id,
            explanation,
        } => {
            assert_eq!(request_id, pending.request_id, "{answer:?}");
            assert!(!explanation.is_empty());
        }
        other => panic!("{answer:?}: {other:?}"),
    }
    assert_eq!(session.pending(), Some(&pending), "{answer:?}");
    assert!(session.answers().is_empty(), "{answer:?}");
    assert_eq!(session.state_hash(), w.state_hash());
    session
}

// REQ-SBA-001, CR 704.5j, CR 201.2a: names that overlap non-transitively (A {X, Y}, B {Y},
// C {X}) form ONE conflict component; "same name" is pairwise, so the survivors are a maximal
// set with no two sharing a name: {A} or {B, C} (decision #42 FP-041, conflict graph). One
// request: candidates [A, B, C], min 1, max 2 (not a clique), default {A}. Invalid answers
// ({A, B}: same name; {B} alone: C conflicts with nothing kept) are rejected with nothing
// changed.
#[test]
fn sba_704_5j_overlapping_names_keep_a_maximal_set() {
    let mut w = World::new();
    let a = w.create_controlled(1, 1, legendary(&["test.legend_x", "test.legend_y"]));
    let b = w.create_controlled(1, 1, legendary(&["test.legend_y"]));
    let c = w.create_controlled(1, 1, legendary(&["test.legend_x"]));
    assert_eq!(w.found_by("704.5j"), [a, b, c]);

    let (run, asked) = drive_answers(&w, &[select(&[a])]);
    assert_eq!(asked.len(), 1);
    let r = &asked[0];
    assert_eq!(r.chooser, p(1));
    assert_eq!((r.min, r.max), (1, 2));
    assert_eq!(
        r.payload,
        ChoicePayload::SelectObjectsFromZone {
            candidates: vec![a, b, c]
        }
    );
    assert_eq!(r.default_on_timeout, select(&[a]));
    let out = run.out.unwrap();
    assert_eq!(
        batch_shape(&out).0,
        [
            (b, ZoneChangeCause::StateBasedAction),
            (c, ZoneChangeCause::StateBasedAction)
        ]
    );
    assert_eq!(on_battlefield(&run.state, &[a, b, c]), [a]);

    // {B, C}: A goes, B and C (no name in common) both stay.
    let (run, _) = drive_answers(&w, &[select(&[c, b])]);
    let out = run.out.unwrap();
    assert_eq!(
        batch_shape(&out).0,
        [(a, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(on_battlefield(&run.state, &[a, b, c]), [b, c]);
    let mut after = run.state;
    assert!(after.found_by("704.5j").is_empty());

    for bad in [vec![a, b], vec![b], vec![c], vec![a, c]] {
        assert_legend_answer_rejected(&w, &bad);
    }

    // The timeout answer is the greedy set by ascending id: {A}.
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let id = session.pending().unwrap().request_id;
    session.submit_default(id).unwrap();
    let Status::Done { state, .. } = session.status() else {
        panic!("not done");
    };
    assert_eq!(on_battlefield(state, &[a, b, c]), [a]);
}

// REQ-SBA-001, CR 704.5j, CR 201.2a: a cyclic overlap ({X, Y}, {Y, Z}, {X, Z}): every pair has
// the same name, so it is a clique: min = max = 1, any single member stays, two are rejected by
// the session (max 1).
#[test]
fn sba_704_5j_cyclic_names_keep_one() {
    let mut w = World::new();
    let a = w.create_controlled(1, 1, legendary(&["test.legend_x", "test.legend_y"]));
    let b = w.create_controlled(1, 1, legendary(&["test.legend_y", "test.legend_z"]));
    let c = w.create_controlled(1, 1, legendary(&["test.legend_x", "test.legend_z"]));
    for kept in [a, b, c] {
        let (run, asked) = drive_answers(&w, &[select(&[kept])]);
        assert_eq!((asked[0].min, asked[0].max), (1, 1));
        assert_eq!(asked[0].default_on_timeout, select(&[a]));
        assert_eq!(on_battlefield(&run.state, &[a, b, c]), [kept]);
    }
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let r = session.pending().unwrap().clone();
    assert!(session.submit(r.request_id, select(&[a, b])).is_err());
}

// REQ-SBA-001, CR 704.5j, CR 201.2a: a path A {X}, B {X, Y}, C {Y}, D {Y, Z}, E {Z}: legal
// survivor sets are exactly the maximal independent sets; isolated legends (no same-name
// partner) are never asked and never move.
#[test]
fn sba_704_5j_path_and_isolated_legends() {
    let mut w = World::new();
    let a = w.create_controlled(1, 1, legendary(&["test.legend_x"]));
    let b = w.create_controlled(1, 1, legendary(&["test.legend_x", "test.legend_y"]));
    let c = w.create_controlled(1, 1, legendary(&["test.legend_y"]));
    let d = w.create_controlled(1, 1, legendary(&["test.legend_y", "test.legend_z"]));
    let e_ = w.create_controlled(1, 1, legendary(&["test.legend_z"]));
    let lone = w.create_controlled(1, 1, legendary(&["test.legend_lone"]));
    let lone2 = w.create_controlled(2, 2, legendary(&["test.legend_x"]));
    assert_eq!(w.found_by("704.5j"), [a, b, c, d, e_]);
    let all = [a, b, c, d, e_, lone, lone2];
    let (run, asked) = drive_answers(&w, &[select(&[a, c, e_])]);
    assert_eq!(asked.len(), 1);
    assert_eq!((asked[0].min, asked[0].max), (1, 4));
    assert_eq!(asked[0].default_on_timeout, select(&[a, c, e_]));
    assert_eq!(on_battlefield(&run.state, &all), [a, c, e_, lone, lone2]);
    let (run, _) = drive_answers(&w, &[select(&[a, d])]);
    assert_eq!(on_battlefield(&run.state, &all), [a, d, lone, lone2]);
    let (run, _) = drive_answers(&w, &[select(&[b, e_])]);
    assert_eq!(on_battlefield(&run.state, &all), [b, e_, lone, lone2]);
    // {A, C}: E has no name in common with a survivor; {A, E}: neither has C; {B, D} and
    // {C, D} share Y.
    for bad in [vec![a, c], vec![a, e_], vec![b, d], vec![c, d]] {
        assert_legend_answer_rejected(&w, &bad);
    }
}

// REQ-SBA-001, CR 704.5j: a legend-rule answer that is not a maximal set of candidates without
// two of the same name is an invalid answer to that request (the session itself rejects counts outside min..max
// and non-candidates before they reach the pass).
#[test]
fn sba_704_5j_selection_protocol() {
    let (a, b, c) = (ObjectId::new(1), ObjectId::new(2), ObjectId::new(3));
    let pair = LegendGroup {
        controller: p(1),
        members: vec![a, b],
        names: vec![set(&["test.legend_a"]), set(&["test.legend_a"])],
    };
    assert!(pair.is_clique());
    assert_eq!(pair.max_survivors(), 1);
    let rid = RequestId::new(7);
    assert_eq!(legend_survivors(&pair, rid, select(&[b])).unwrap(), [b]);
    // A {X, Y}, B {Y}, C {X}.
    let overlap = LegendGroup {
        controller: p(1),
        members: vec![a, b, c],
        names: vec![
            set(&["test.legend_x", "test.legend_y"]),
            set(&["test.legend_y"]),
            set(&["test.legend_x"]),
        ],
    };
    assert!(!overlap.is_clique());
    assert_eq!(overlap.max_survivors(), 2);
    assert_eq!(overlap.default_survivors(), [a]);
    assert_eq!(legend_survivors(&overlap, rid, select(&[a])).unwrap(), [a]);
    assert_eq!(
        legend_survivors(&overlap, rid, select(&[b, c])).unwrap(),
        [b, c]
    );
    let bad_cases: [(&LegendGroup, ChoiceResponse); 9] = [
        (&pair, select(&[])),
        (&pair, select(&[a, b])),
        (&pair, select(&[c])),
        (&pair, ChoiceResponse::SelectReplacement { index: 0 }),
        (&overlap, select(&[])),
        (&overlap, select(&[a, b])),
        (&overlap, select(&[b])),
        (&overlap, select(&[b, b, c])),
        (&overlap, select(&[ObjectId::new(9)])),
    ];
    for (group, bad) in bad_cases {
        // REQ-SBA-001, CR 704.5j: the procedure-level validation error names the request
        // (decision #28 FP-027 convention; decision #42 FP-041, round 2).
        let err = legend_survivors(group, rid, bad).unwrap_err();
        assert!(matches!(
            err,
            BusInterrupt::Error(BusError::Choice(ChoiceError::Invalid { request_id, .. }))
                if request_id == rid
        ));
    }
    // The session refuses a selection of two in a same-name pair (max 1).
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

// REQ-SBA-001, CR 310.10, CR 704.5p, CR 704.5m, CR 704.3: an attached battle-Aura on a legal
// creature is only unattached in the first check (it stays on the battlefield); the repeated
// check finds it "not attached" and puts it into the graveyard.
#[test]
fn sba_704_5m_aura_battle() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let mut c = aura();
    c.card_types = set(&["Battle", "Enchantment"]);
    let a = w.create(1, ZoneKind::Battlefield, c);
    w.stable_battle(a, 1);
    w.attach(a, AttachTarget::Permanent(host));
    // Check 1.
    assert_eq!(w.found(obj(a)), ["704.5p"]);
    let out = pass(&mut w);
    assert_eq!(out.unattached, [a]);
    assert!(batch_shape(&out).0.iter().all(|(id, _)| *id != a));
    let (card, host_card) = (w.card_of(a), w.card_of(host));
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Battlefield));
    assert_eq!(w.permanents.attached_to(a), None);
    assert_eq!(w.zone_of_card(host_card), Some(ZoneId::Battlefield));
    // Check 2 (the repeat).
    assert_eq!(w.found(obj(a)), ["704.5m"]);
    let out = pass(&mut w);
    assert!(out.unattached.is_empty());
    assert_eq!(
        batch_shape(&out).0,
        [(a, ZoneChangeCause::StateBasedAction)]
    );
}

// REQ-SBA-001, CR 704.5m, CR 303.4c, CR 704.5p, CR 310.10: a battle-Aura on a host that is
// independently illegal per the enchant ability fires 704.5m in the first check; the move wins
// over the unattach.
#[test]
fn sba_704_5m_aura_battle_illegal_host() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let mut c = aura();
    c.card_types = set(&["Battle", "Enchantment"]);
    let a = w.create(1, ZoneKind::Battlefield, c);
    w.stable_battle(a, 1);
    w.attach(a, AttachTarget::Permanent(host));
    w.env
        .illegal_aura_targets
        .insert((a, AttachTarget::Permanent(host)));
    assert_eq!(w.found(obj(a)), ["704.5m", "704.5p"]);
    let card = w.card_of(a);
    let out = pass(&mut w);
    assert!(out.unattached.is_empty());
    assert_eq!(
        batch_shape(&out).0,
        [(a, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    let host_card = w.card_of(host);
    assert_eq!(w.zone_of_card(host_card), Some(ZoneId::Battlefield));
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
    // CR 701.3d: each becomes unattached through an Unattach event, in one batch.
    assert_eq!(out.executed.len(), 1);
    assert_eq!(unattach_events(&out), [on_player, on_rock, fort_bad]);
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
    w.stable_battle(battle, 1);
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

// REQ-SBA-001, CR 704.3, CR 704.5p, CR 704.8, CR 701.3d: an attached creature that also dies in
// the pass is moved AND gets its one unattach event in the same batch (decision "#42 FP-041 —
// One SBA batch: moves, counter removals and unattach events"); it is not reported as unattached
// (it left the battlefield); its last known information keeps the attachment.
#[test]
fn sba_704_5p_moving_attachment_is_unattached_in_the_same_event() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let c = w.creature(1, 0);
    w.attach(c, AttachTarget::Permanent(host));
    assert_eq!(w.found(obj(c)), ["704.5f", "704.5p"]);
    let out = pass(&mut w);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(unattach_events(&out), [c]);
    assert_eq!(
        batch_shape(&out),
        (vec![(c, ZoneChangeCause::StateBasedAction)], vec![])
    );
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

// REQ-SBA-001, CR 704.5q, CR 704.5r, CR 122.1: 3 +1/+1 and 5 -1/-1 with a limit of 1 on +1/+1:
// 704.5q removes 3 of each, 704.5r would remove 2 of +1/+1; the larger amount per kind is 3.
#[test]
fn sba_704_5q_and_5r_larger_amount() {
    let mut w = World::new();
    let a = w.creature(1, 9);
    w.counters(a, "+1/+1", 3);
    w.counters(a, "-1/-1", 5);
    w.env.counter_limits = BTreeMap::from([(a, BTreeMap::from([("+1/+1".to_owned(), 1)]))]);
    assert_eq!(w.found(obj(a)), ["704.5q", "704.5r"]);
    let out = pass(&mut w);
    assert_eq!(
        batch_shape(&out).1,
        [(a, "+1/+1".to_owned(), 3), (a, "-1/-1".to_owned(), 3)]
    );
    assert_eq!((w.counter(a, "+1/+1"), w.counter(a, "-1/-1")), (0, 2));
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

impl Lcg {
    /// A uniform index below `len` (`len` > 0).
    fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.next(len as u64)).unwrap()
    }
}

/// CR 201.2a: whether two name sets have the same name (nameless never does).
fn same_name(x: &BTreeSet<String>, y: &BTreeSet<String>) -> bool {
    !x.is_disjoint(y)
}

/// The test's own model of the CR 704.5j conflict components of `board` (id, controller,
/// names): per controller, connected components of "same name" with two or more members,
/// each ascending, the set of all of them.
fn model_components(board: &[(ObjectId, PlayerId, BTreeSet<String>)]) -> BTreeSet<Vec<ObjectId>> {
    let mut out = BTreeSet::new();
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    for (start, controller, _) in board {
        if seen.contains(start) {
            continue;
        }
        let mut component = vec![*start];
        seen.insert(*start);
        let mut i = 0;
        while i < component.len() {
            let current = component[i];
            let names = &board.iter().find(|(id, _, _)| *id == current).unwrap().2;
            for (other, c, other_names) in board {
                if c == controller && !seen.contains(other) && same_name(names, other_names) {
                    seen.insert(*other);
                    component.push(*other);
                }
            }
            i += 1;
        }
        if component.len() >= 2 {
            component.sort();
            out.insert(component);
        }
    }
    out
}

// REQ-SBA-001, REQ-CORE-001, CR 704.5j, CR 201.2a (property): for seeded random legendary
// boards (random names, one or two per legend or none, random controllers) and random valid
// answers (a greedy maximal set over a random order), the requests are exactly the test's own
// conflict components, and the battlefield after the pass is EXACTLY the expected survivor set
// (every non-member untouched, per component the chosen survivors). No two survivors of one
// controller share a name; every removed legend shares a name with a survivor. Non-vacuous:
// survivors are never empty when a legend exists, and the seeded run both removes legends and
// keeps more than one member of some component.
#[test]
fn property_legend_rule() {
    let mut rng = Lcg(7);
    let names = [
        "test.legend_a",
        "test.legend_b",
        "test.legend_c",
        "test.legend_d",
    ];
    let mut iterations_with_removal = 0;
    let mut components_keeping_several = 0;
    for _ in 0..150 {
        let mut w = World::new();
        let n = rng.next(6) + 2;
        let mut board: Vec<(ObjectId, PlayerId, BTreeSet<String>)> = Vec::new();
        for _ in 0..n {
            let controller = rng.next(2) + 1;
            let count = [0, 1, 1, 1, 2, 2][rng.index(6)];
            let chosen: Vec<&str> = (0..count).map(|_| names[rng.index(names.len())]).collect();
            let id = w.create_controlled(1, controller, legendary(&chosen));
            board.push((id, p(controller), set(&chosen)));
        }
        let all: Vec<ObjectId> = board.iter().map(|(id, _, _)| *id).collect();
        let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
        let mut asked: BTreeSet<Vec<ObjectId>> = BTreeSet::new();
        let mut losers: BTreeSet<ObjectId> = BTreeSet::new();
        while let Some(r) = session.pending().cloned() {
            let ChoicePayload::SelectObjectsFromZone { candidates } = &r.payload else {
                panic!("payload");
            };
            let names_of = |id: ObjectId| &board.iter().find(|(o, _, _)| *o == id).unwrap().2;
            // A random maximal set with no two of the same name: greedy over a random order.
            let mut order = candidates.clone();
            for i in (1..order.len()).rev() {
                let j = rng.index(i + 1);
                order.swap(i, j);
            }
            let mut kept: Vec<ObjectId> = Vec::new();
            for id in order {
                if kept.iter().all(|&k| !same_name(names_of(id), names_of(k))) {
                    kept.push(id);
                }
            }
            if kept.len() > 1 {
                components_keeping_several += 1;
            }
            losers.extend(candidates.iter().filter(|id| !kept.contains(id)));
            asked.insert(candidates.clone());
            session.submit(r.request_id, select(&kept)).unwrap();
        }
        let Status::Done { state, output } = session.status() else {
            panic!("not done");
        };
        assert!(output.is_ok());
        // The requests are exactly the model's components.
        assert_eq!(asked, model_components(&board));
        let expected: Vec<ObjectId> = all
            .iter()
            .copied()
            .filter(|id| !losers.contains(id))
            .collect();
        let survivors = on_battlefield(state, &all);
        assert_eq!(survivors, expected);
        assert!(!survivors.is_empty());
        if !losers.is_empty() {
            iterations_with_removal += 1;
        }
        let entry = |id: &ObjectId| board.iter().find(|(o, _, _)| o == id).unwrap();
        for x in &survivors {
            for y in &survivors {
                let (ex, ey) = (entry(x), entry(y));
                assert!(x == y || ex.1 != ey.1 || !same_name(&ex.2, &ey.2));
            }
        }
        for gone in &losers {
            let eg = entry(gone);
            assert!(survivors.iter().any(|s| {
                let es = entry(s);
                es.1 == eg.1 && same_name(&es.2, &eg.2)
            }));
        }
        let mut after = state.clone();
        assert!(after.found_by("704.5j").is_empty());
    }
    assert!(iterations_with_removal > 0);
    assert!(components_keeping_several > 0);
}

// ---------------------------------------------------------------------------------------------
// Review round 1 (PR #214)
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.3, CR 704.5f, CR 704.5q, CR 704.5p: a creature with toughness 0, both
// counter kinds and attached to a permanent, whose graveyard move a Skip replacement prevents,
// stays on the battlefield and gets its counter removals (N of each) and its unattach in the
// SAME single batch as its (skipped) move (decision "#42 FP-041 — One SBA batch: moves, counter
// removals and unattach events"); the pass is performed. Control: without the Skip the creature
// leaves, and its counter removals and its unattach are still in that one batch (CR 704.3),
// executed before the move, so its last known information keeps every counter and the
// attachment (CR 704.8; no look-back for counters, CR 122.2).
#[test]
fn prevented_move_still_removes_counters_and_unattaches() {
    let build = || {
        let mut w = World::new();
        let host = w.creature(1, 3);
        let c = w.creature(1, 0);
        w.counters(c, "+1/+1", 2);
        w.counters(c, "-1/-1", 3);
        w.attach(c, AttachTarget::Permanent(host));
        (w, host, c)
    };
    let (mut w, _, c) = build();
    assert_eq!(w.found(obj(c)), ["704.5f", "704.5p", "704.5q"]);
    w.registry
        .add(ReplacementEffect::new(e(1), sba_move_of(c), Action::Skip))
        .unwrap();
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(w.zones.object(c).unwrap().zone, ZoneId::Battlefield);
    assert_eq!((w.counter(c, "+1/+1"), w.counter(c, "-1/-1")), (0, 1));
    assert_eq!(w.permanents.attached_to(c), None);
    assert_eq!(out.unattached, [c]);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        batch_shape(&out),
        (
            vec![],
            vec![(c, "+1/+1".to_owned(), 2), (c, "-1/-1".to_owned(), 2)]
        )
    );
    assert_eq!(unattach_events(&out), [c]);
    // The unattach precedes the counter removals (it is submitted first, decision "#42 FP-041 —
    // Planned unattach and pre-pass host (round 3)").
    let events: Vec<&GameEvent> = out.executed[0]
        .events
        .iter()
        .map(|ev| ev.instance.event())
        .collect();
    assert_eq!(events.len(), 3);
    assert!(matches!(events.first(), Some(GameEvent::Unattach { attachment }) if *attachment == c));

    // Control: without the Skip the creature leaves; same single batch.
    let (mut w, host, c) = build();
    let card = w.card_of(c);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        batch_shape(&out),
        (
            vec![(c, ZoneChangeCause::StateBasedAction)],
            vec![(c, "+1/+1".to_owned(), 2), (c, "-1/-1".to_owned(), 2)]
        )
    );
    assert_eq!(unattach_events(&out), [c]);
    assert!(out.unattached.is_empty());
    let lki = w.lki.get(c).unwrap();
    assert_eq!(lki.counters().get("+1/+1"), Some(&2));
    assert_eq!(lki.counters().get("-1/-1"), Some(&3));
    assert_eq!(
        lki.permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(host))
    );
}

// REQ-SBA-001, CR 704.3, CR 616.1: the counter removals of a creature whose move a Skip
// prevents are in the one batch, through the replacement bus: a choice between two replacements
// of each counter removal suspends the WHOLE pass with nothing changed (asserted inside the
// procedure), and the replayed pass finishes it in ONE executed batch.
#[test]
fn replacement_choice_in_the_one_batch_restores_the_whole_pass() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let c = w.creature(1, 0);
    let other = w.creature(2, 0);
    w.counters(c, "+1/+1", 1);
    w.counters(c, "-1/-1", 1);
    w.attach(c, AttachTarget::Permanent(host));
    let other_card = w.card_of(other);
    w.registry
        .add(ReplacementEffect::new(e(1), sba_move_of(c), Action::Skip))
        .unwrap();
    for id in [2, 3] {
        let mut matcher = EventMatcher::new(BusEventKind::RemoveCounters);
        matcher.affected_object = Some(c);
        w.registry
            .add(ReplacementEffect::new(e(id), matcher, Action::Skip))
            .unwrap();
    }
    let before = w.state_hash();
    let session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    assert!(matches!(session.status(), Status::Pending(_)));
    assert_eq!(session.state_hash(), before);
    let run = drive(&w, &[0, 0]);
    assert_eq!(run.asked, 2);
    let out = run.out.unwrap();
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        run.state.zone_of_card(other_card),
        Some(ZoneId::Graveyard(p(2)))
    );
    // Both removals were skipped by a replacement; the unattach happened.
    assert_eq!(
        (run.state.counter(c, "+1/+1"), run.state.counter(c, "-1/-1")),
        (1, 1)
    );
    assert_eq!(out.unattached, [c]);
    assert_eq!(run.state.permanents.attached_to(c), None);
}

// REQ-SBA-001, CR 704.3, CR 704.5f, CR 704.5q, CR 704.5p, CR 400.7: a move replaced into
// another zone (Instead: to exile) is still one batch with the creature's counter removals and
// its unattach, executed before the move; the creature is not reported as unattached (it left).
#[test]
fn replaced_move_elsewhere_is_in_the_one_batch() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let c = w.creature(1, 0);
    w.counters(c, "+1/+1", 1);
    w.counters(c, "-1/-1", 1);
    w.attach(c, AttachTarget::Permanent(host));
    let card = w.card_of(c);
    w.registry
        .add(ReplacementEffect::new(
            e(1),
            sba_move_of(c),
            Action::Instead(vec![zone_change(
                c,
                ZoneKind::Exile,
                ZoneChangeCause::StateBasedAction,
            )]),
        ))
        .unwrap();
    let out = pass(&mut w);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Exile));
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        batch_shape(&out),
        (
            vec![(c, ZoneChangeCause::StateBasedAction)],
            vec![(c, "+1/+1".to_owned(), 1), (c, "-1/-1".to_owned(), 1)]
        )
    );
    assert_eq!(unattach_events(&out), [c]);
    assert!(out.unattached.is_empty());
    let lki = w.lki.get(c).unwrap();
    assert_eq!(lki.counters().get("+1/+1"), Some(&1));
    assert_eq!(
        lki.permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(host))
    );
}

// REQ-SBA-001, CR 704.5n, CR 704.5p, CR 701.3d, CR 603.10c, CR 603.2, CR 704.8: an Equipment on
// a player (704.5n) and a creature attached to a host that dies in the same pass (704.5p) become
// unattached through one Unattach event each, ascending id, submitted before the zone changes
// and counter removals of the same ONE batch (round 3, H6). The bus executes the events that are
// not zone changes first, in submission order (unattaches, then counter removals; they name live
// objects), then the moves; the batch
// captures last known information before any of its events, so the dying host's record still
// lists the creature attached to it (look-back). The trigger normalisation sees an Unattached
// occurrence for each.
#[test]
fn unattach_events_in_the_batch_reach_triggers_with_look_back() {
    let mut w = World::new();
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    let dying = w.creature(1, 0);
    let cr = w.creature(1, 2);
    let q = w.creature(1, 9);
    w.counters(q, "+1/+1", 1);
    w.counters(q, "-1/-1", 1);
    w.attach(eq, AttachTarget::Player(p(2)));
    w.attach(cr, AttachTarget::Permanent(dying));
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    let kinds: Vec<(&str, ObjectId)> = out.executed[0]
        .events
        .iter()
        .map(|ev| match ev.instance.event() {
            GameEvent::ZoneChange { object, .. } => ("move", *object),
            GameEvent::RemoveCounters {
                target: CounterTarget::Object(o),
                ..
            } => ("counters", *o),
            GameEvent::Unattach { attachment } => ("unattach", *attachment),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("unattach", eq),
            ("unattach", cr),
            ("counters", q),
            ("counters", q),
            ("move", dying)
        ]
    );
    assert_eq!(out.unattached, [eq, cr]);
    let unattached: Vec<(Option<ObjectId>, Option<ObjectId>)> =
        crate::triggers::events_from_batch(&out.executed[0])
            .into_iter()
            .filter(|t| {
                t.kinds
                    .contains(&fineprint_dsl::ability::EventKind::Unattached)
            })
            .map(|t| (t.subject_pre, t.subject_post))
            .collect();
    assert_eq!(unattached, [(Some(eq), Some(eq)), (Some(cr), Some(cr))]);
    for id in [eq, cr] {
        assert_eq!(w.zones.object(id).unwrap().zone, ZoneId::Battlefield);
        assert_eq!(w.permanents.attached_to(id), None);
    }
    // CR 704.8: the host's last known information is from before the pass.
    assert_eq!(w.lki.get(dying).unwrap().attached_here(), [cr]);
}

// REQ-SBA-001, CR 704.5j, CR 101.4, CR 101.4c: two players each with a same-name pair; the
// active player's legends have the HIGHER ids, and the first request goes to the active player;
// nothing changes until both have answered; then both losers move in one batch.
#[test]
fn legend_choices_in_apnap_order() {
    let mut w = World::new();
    let n1 = w.create_controlled(2, 2, legendary(&["test.legend_a"]));
    let n2 = w.create_controlled(2, 2, legendary(&["test.legend_a"]));
    let a1 = w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    let a2 = w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    let before = w.state_hash();
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let first = session.pending().unwrap().clone();
    assert_eq!(first.chooser, p(1));
    assert_eq!(
        first.payload,
        ChoicePayload::SelectObjectsFromZone {
            candidates: vec![a1, a2]
        }
    );
    session.submit(first.request_id, select(&[a2])).unwrap();
    let second = session.pending().unwrap().clone();
    assert_eq!(second.chooser, p(2));
    // No move before every answer.
    assert_eq!(session.state_hash(), before);
    session.submit(second.request_id, select(&[n1])).unwrap();
    let Status::Done { state, output } = session.status() else {
        panic!("not done");
    };
    let out = output.clone().unwrap();
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        batch_shape(&out).0,
        [
            (n2, ZoneChangeCause::StateBasedAction),
            (a1, ZoneChangeCause::StateBasedAction)
        ]
    );
    assert_eq!(on_battlefield(state, &[n1, n2, a1, a2]), [n1, a2]);
}

impl World {
    /// Three players at 20 life, turn 1, turn order `order`, active player `active`.
    fn three(order: [u64; 3], active: u64) -> World {
        let mut w = World::new();
        let mut game = GameState::new(41, 3, 20).unwrap();
        game.set_turn_number(1);
        game.set_turn_order(order.iter().map(|&n| p(n)).collect())
            .unwrap();
        game.set_active_player(Some(p(active))).unwrap();
        w.game = game;
        w.zones = Zones::new(&[p(1), p(2), p(3)], ZonesConfig::default());
        w
    }

    /// A layer-2 effect (CR 613.1b) giving control of `id` to `player`.
    fn give_control(&mut self, effect: u64, id: ObjectId, player: u64) {
        self.effects
            .register(
                e(effect),
                ContinuousEffect {
                    source: None,
                    controller: p(player),
                    timestamp: Timestamp::new(effect),
                    layer: Layer::L2,
                    is_cda: false,
                    origin: EffectOrigin::Resolved,
                    affected: Affected::Objects(BTreeSet::from([id])),
                    modification: LayerModification::SetController(p(player)),
                },
            )
            .unwrap();
    }
}

// REQ-SBA-001, CR 704.5j, CR 101.4: three players, turn order 2, 1, 3, active player 1: APNAP
// order is 1, 3, 2, which differs from both id order and creation order (3, 2, 1).
#[test]
fn legend_choices_follow_turn_order_with_three_players() {
    let mut w = World::three([2, 1, 3], 1);
    let mut pairs = BTreeMap::new();
    for controller in [3, 2, 1] {
        let x = w.create_controlled(controller, controller, legendary(&["test.legend_a"]));
        let y = w.create_controlled(controller, controller, legendary(&["test.legend_a"]));
        pairs.insert(controller, (x, y));
    }
    let before = w.state_hash();
    let answers: Vec<ChoiceResponse> = [1, 3, 2].iter().map(|c| select(&[pairs[c].1])).collect();
    let (run, asked) = drive_answers(&w, &answers);
    let choosers: Vec<PlayerId> = asked.iter().map(|r| r.chooser).collect();
    assert_eq!(choosers, [p(1), p(3), p(2)]);
    for (r, c) in asked.iter().zip([1, 3, 2]) {
        assert_eq!(
            r.payload,
            ChoicePayload::SelectObjectsFromZone {
                candidates: vec![pairs[&c].0, pairs[&c].1]
            }
        );
    }
    let out = run.out.unwrap();
    assert_eq!(out.executed.len(), 1);
    let mut moved: Vec<ObjectId> = batch_shape(&out).0.into_iter().map(|(o, _)| o).collect();
    moved.sort();
    let mut expected: Vec<ObjectId> = pairs.values().map(|&(x, _)| x).collect();
    expected.sort();
    assert_eq!(moved, expected);
    assert_ne!(run.hash, before);
}

// REQ-SBA-001, CR 704.5j, CR 613.1b, CR 110.2: legends are grouped by the controller after
// layer 2. Stored controllers 1 and 2 with a layer-2 effect giving the second to player 1: one
// group, asked to player 1; the loser goes to its owner's graveyard. Reverse: the same stored
// controller and a layer-2 effect giving one to player 2: two controllers, no finding.
#[test]
fn legend_groups_use_the_evaluated_controller() {
    let mut w = World::new();
    let l1 = w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    let l2 = w.create_controlled(2, 2, legendary(&["test.legend_a"]));
    assert!(w.found_by("704.5j").is_empty());
    w.give_control(1, l2, 1);
    assert_eq!(w.found_by("704.5j"), [l1, l2]);
    let card2 = w.card_of(l2);
    let (run, asked) = drive_answers(&w, &[select(&[l1])]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].chooser, p(1));
    assert_eq!(run.state.zone_of_card(card2), Some(ZoneId::Graveyard(p(2))));

    let mut w = World::new();
    let m1 = w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    let m2 = w.create_controlled(1, 1, legendary(&["test.legend_a"]));
    assert_eq!(w.found_by("704.5j"), [m1, m2]);
    w.give_control(1, m2, 2);
    assert!(w.found_by("704.5j").is_empty());
    assert!(!pass(&mut w).performed);
}

// REQ-SBA-001, CR 704.5n, CR 702.16d, CR 301.5c: an Equipment on a creature and a Fortification
// on a land that the environment marks illegal (a protection stand-in) become unattached and
// stay on the battlefield; unmarked pairs stay attached; an unattached Equipment named in a
// marked pair is untouched.
#[test]
fn sba_704_5n_effects_make_the_attachment_illegal() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let host_ok = w.creature(1, 3);
    let land = w.create(1, ZoneKind::Battlefield, noncreature("test.land", "Land"));
    let land_ok = w.create(1, ZoneKind::Battlefield, noncreature("test.land_b", "Land"));
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    let eq_ok = w.create(1, ZoneKind::Battlefield, equipment());
    let loose = w.create(1, ZoneKind::Battlefield, equipment());
    let fort = w.create(1, ZoneKind::Battlefield, fortification());
    let fort_ok = w.create(1, ZoneKind::Battlefield, fortification());
    w.attach(eq, AttachTarget::Permanent(host));
    w.attach(eq_ok, AttachTarget::Permanent(host_ok));
    w.attach(fort, AttachTarget::Permanent(land));
    w.attach(fort_ok, AttachTarget::Permanent(land_ok));
    w.env.illegal_equipment_targets = BTreeSet::from([
        (eq, AttachTarget::Permanent(host)),
        (fort, AttachTarget::Permanent(land)),
        (loose, AttachTarget::Permanent(host)),
    ]);
    assert!(
        !w.env
            .equipment_can_attach(eq, AttachTarget::Permanent(host))
    );
    assert!(
        w.env
            .equipment_can_attach(eq_ok, AttachTarget::Permanent(host_ok))
    );
    assert_eq!(w.found_by("704.5n"), [eq, fort]);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.unattached, [eq, fort]);
    for id in [eq, fort, loose] {
        assert_eq!(w.zones.object(id).unwrap().zone, ZoneId::Battlefield);
        assert_eq!(w.permanents.attached_to(id), None);
    }
    assert_eq!(
        w.permanents.attached_to(eq_ok),
        Some(AttachTarget::Permanent(host_ok))
    );
    assert_eq!(
        w.permanents.attached_to(fort_ok),
        Some(AttachTarget::Permanent(land_ok))
    );
    assert!(w.check().is_empty());
}

// REQ-SBA-001, CR 704.5n, CR 702.26b: a marked pair whose target is phased out is not checked.
#[test]
fn sba_704_5n_marked_pair_with_phased_out_target_is_not_checked() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    w.attach(eq, AttachTarget::Permanent(host));
    w.env.illegal_equipment_targets = BTreeSet::from([(eq, AttachTarget::Permanent(host))]);
    w.phase(host, true);
    assert!(w.found_by("704.5n").is_empty());
}

// ---------------------------------------------------------------------------------------------
// Review round 2 (PR #214): one SBA batch (decision "#42 FP-041 — One SBA batch: moves, counter
// removals and unattach events")
// ---------------------------------------------------------------------------------------------

/// A matcher for the counter removal of `kind` from `object`.
fn remove_counters_of(object: ObjectId, kind: &str) -> EventMatcher {
    let mut m = EventMatcher::new(BusEventKind::RemoveCounters);
    m.affected_object = Some(object);
    m.counter_kind = Some(kind.to_owned());
    m
}

/// X (base toughness 0) and Y (toughness 3) each with one +1/+1 and one -1/-1 counter; a Skip
/// on removing X's +1/+1 counter that requires Y to have a +1/+1 counter; with `skip_move`, a
/// Skip on X's SBA move too. With `y_first`, Y is created before X (Y has the lower object id).
fn eligibility_world(skip_move: bool, y_first: bool) -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let x_toughness = if skip_move { 0 } else { 2 };
    let (x, y) = if y_first {
        let y = w.creature(1, 3);
        (w.creature(1, x_toughness), y)
    } else {
        let x = w.creature(1, x_toughness);
        (x, w.creature(1, 3))
    };
    for id in [x, y] {
        w.counters(id, "+1/+1", 1);
        w.counters(id, "-1/-1", 1);
    }
    if skip_move {
        w.registry
            .add(ReplacementEffect::new(e(1), sba_move_of(x), Action::Skip))
            .unwrap();
    }
    let mut m = remove_counters_of(x, "+1/+1");
    m.requires_counter = Some((y, "+1/+1".to_owned()));
    w.registry
        .add(ReplacementEffect::new(e(2), m, Action::Skip))
        .unwrap();
    (w, x, y)
}

// REQ-SBA-001, CR 704.3, CR 704.5f, CR 704.5q, CR 614.1 (G1): replacement eligibility of the
// counter removal of a creature whose move a Skip prevents is judged against the state before
// the one event: Y has its +1/+1 counter before the event, so the Skip on X's +1/+1 removal
// applies although Y's counters are removed in the same event. X ends with exactly one +1/+1
// counter and no -1/-1 counter (1/0); Y with none.
#[test]
fn prevented_move_counter_removal_eligibility_is_judged_before_the_event() {
    let (mut w, x, y) = eligibility_world(true, false);
    assert_eq!(w.found(obj(x)), ["704.5f", "704.5q"]);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(w.zones.object(x).unwrap().zone, ZoneId::Battlefield);
    assert_eq!((w.counter(x, "+1/+1"), w.counter(x, "-1/-1")), (1, 0));
    assert_eq!((w.counter(y, "+1/+1"), w.counter(y, "-1/-1")), (0, 0));
    assert!(w.registry.get(e(2)).is_some());
}

// REQ-SBA-001, CR 704.3, CR 704.5f, CR 704.5q, CR 614.1 (G1, id-order twin): the same fixture
// with Y created before X (Y has the lower object id, so Y's counter removals are submitted
// before X's): the result does not depend on the id order. The Skip on X's +1/+1 removal
// applies, X ends 1/0 and Y 0/0.
#[test]
fn prevented_move_counter_removal_eligibility_is_judged_before_the_event_y_lower_id() {
    let (mut w, x, y) = eligibility_world(true, true);
    assert!(y < x);
    assert_eq!(w.found(obj(x)), ["704.5f", "704.5q"]);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(w.zones.object(x).unwrap().zone, ZoneId::Battlefield);
    assert_eq!((w.counter(x, "+1/+1"), w.counter(x, "-1/-1")), (1, 0));
    assert_eq!((w.counter(y, "+1/+1"), w.counter(y, "-1/-1")), (0, 0));
    // The Skip applied: X's +1/+1 counter removal is not among the executed events.
    assert!(!batch_shape(&out).1.contains(&(x, "+1/+1".to_owned(), 1)));
    assert!(w.registry.get(e(2)).is_some());
}

// REQ-SBA-001, CR 704.5q, CR 614.1 (G1 control): the same Skip when X does not move: one batch,
// same result.
#[test]
fn counter_removal_eligibility_is_judged_before_the_event_control() {
    let (mut w, x, y) = eligibility_world(false, false);
    assert_eq!(w.found(obj(x)), ["704.5q"]);
    let out = pass(&mut w);
    assert_eq!(out.executed.len(), 1);
    assert_eq!((w.counter(x, "+1/+1"), w.counter(x, "-1/-1")), (1, 0));
    assert_eq!((w.counter(y, "+1/+1"), w.counter(y, "-1/-1")), (0, 0));
}

/// X with one +1/+1 and one -1/-1 counter (base toughness 0 with `skip_move`, then its move is
/// Skipped; otherwise 2) and Y (toughness 0, dies in the pass); X's +1/+1 removal is replaced
/// by tapping Y.
fn retired_instead_world(skip_move: bool) -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let x = w.creature(1, if skip_move { 0 } else { 2 });
    let y = w.creature(1, 0);
    w.counters(x, "+1/+1", 1);
    w.counters(x, "-1/-1", 1);
    if skip_move {
        w.registry
            .add(ReplacementEffect::new(e(1), sba_move_of(x), Action::Skip))
            .unwrap();
    }
    w.registry
        .add(ReplacementEffect::new(
            e(2),
            remove_counters_of(x, "+1/+1"),
            Action::Instead(vec![GameEvent::Tap { object: y }]),
        ))
        .unwrap();
    (w, x, y)
}

/// Whether `out` executed a `Tap` of `object`.
fn tapped_in(out: &SbaOutcome, object: ObjectId) -> bool {
    out.executed
        .iter()
        .flat_map(|b| &b.events)
        .any(|ev| matches!(ev.instance.event(), GameEvent::Tap { object: o } if *o == object))
}

// REQ-SBA-001, CR 704.3, CR 704.5f, CR 704.5q, CR 614.1 (G2): an Instead on the counter removal
// of a creature whose move is Skipped names Y, which dies in the same pass. In the one batch
// (one bus call) the pass completes: Y is tapped as part of the event (before the zone changes)
// and then put into its owner's graveyard; X keeps its +1/+1 counter and loses its -1/-1
// counter.
#[test]
fn prevented_move_instead_naming_a_dying_object_completes() {
    let (w, x, y) = retired_instead_world(true);
    let y_card = w.card_of(y);
    let run = drive(&w, &[]);
    let out = run.out.expect("the pass failed");
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert!(tapped_in(&out, y));
    assert_eq!(
        run.state.zone_of_card(y_card),
        Some(ZoneId::Graveyard(p(1)))
    );
    assert_eq!(run.state.zones.object(x).unwrap().zone, ZoneId::Battlefield);
    assert_eq!(
        (run.state.counter(x, "+1/+1"), run.state.counter(x, "-1/-1")),
        (1, 0)
    );
}

// REQ-SBA-001, CR 704.5q, CR 614.1 (G2 control): the same Instead when X does not move.
#[test]
fn instead_naming_a_dying_object_completes_control() {
    let (w, x, y) = retired_instead_world(false);
    let y_card = w.card_of(y);
    let run = drive(&w, &[]);
    let out = run.out.expect("the pass failed");
    assert_eq!(out.executed.len(), 1);
    assert!(tapped_in(&out, y));
    assert_eq!(
        run.state.zone_of_card(y_card),
        Some(ZoneId::Graveyard(p(1)))
    );
    assert_eq!(
        (run.state.counter(x, "+1/+1"), run.state.counter(x, "-1/-1")),
        (1, 0)
    );
}

// REQ-SBA-001, CR 704.3, CR 704.8 (design check of the one batch): ZoneChange X, ZoneChange Y,
// both counter removals of X, Unattach X and an Instead(Tap Y) on X's +1/+1 removal, in ONE
// bus batch: the bus executes the events that are not zone changes first and completes; X's
// last known information (captured before the batch) still shows its counters and attachment.
#[test]
fn design_same_object_side_events_in_one_batch() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let x = w.creature(1, 0);
    let y = w.creature(1, 0);
    w.counters(x, "+1/+1", 1);
    w.counters(x, "-1/-1", 1);
    w.attach(x, AttachTarget::Permanent(host));
    w.registry
        .add(ReplacementEffect::new(
            e(2),
            remove_counters_of(x, "+1/+1"),
            Action::Instead(vec![GameEvent::Tap { object: y }]),
        ))
        .unwrap();
    let events = vec![
        zone_change(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
        zone_change(y, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
        GameEvent::RemoveCounters {
            target: CounterTarget::Object(x),
            kind: "+1/+1".into(),
            amount: 1,
        },
        GameEvent::RemoveCounters {
            target: CounterTarget::Object(x),
            kind: "-1/-1".into(),
            amount: 1,
        },
        GameEvent::Unattach { attachment: x },
    ];
    let procedure = move |w: &mut World,
                          ch: &mut ChoiceContext<'_>|
          -> Result<Result<usize, BusError>, Interrupt> {
        let snap = w.snapshot();
        let World {
            game,
            zones,
            ids,
            permanents,
            memories,
            lki,
            events: evs,
            registry,
            ..
        } = w;
        let mut ctx = EventContext {
            game,
            zones,
            ids,
            permanents,
            memories,
            lki,
            events: evs,
            provider: &snap,
        };
        match crate::replacement::process_batch(
            &mut ctx,
            registry,
            &crate::events::EventBatch::from_events(events.clone()),
            true,
            ch,
        ) {
            Ok(r) => Ok(Ok(r.executed.iter().map(|b| b.events.len()).sum())),
            Err(BusInterrupt::Error(e)) => Ok(Err(e)),
            Err(BusInterrupt::Pending(r)) => Err(Interrupt::Pending(r)),
        }
    };
    let session = ChoiceSession::start(w.clone(), procedure).unwrap();
    let Status::Done { state, output } = session.status() else {
        panic!("pending");
    };
    // Two moves, the -1/-1 removal, the Tap (instead of the +1/+1 removal) and the unattach.
    assert_eq!(output.clone().expect("the batch failed"), 5);
    let lki = state.lki.get(x).unwrap();
    assert_eq!(lki.counters().get("+1/+1"), Some(&1));
    assert_eq!(lki.counters().get("-1/-1"), Some(&1));
    assert_eq!(
        lki.permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(host))
    );
}

/// A host (toughness 0, dies) and creature X (toughness 0) attached to it; with `skip_move` a
/// Skip on X's SBA move.
fn host_leaves_world(skip_move: bool) -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let host = w.creature(1, 0);
    let x = w.creature(1, 0);
    w.attach(x, AttachTarget::Permanent(host));
    if skip_move {
        w.registry
            .add(ReplacementEffect::new(e(1), sba_move_of(x), Action::Skip))
            .unwrap();
    }
    (w, host, x)
}

// REQ-SBA-001, CR 704.3, CR 704.5p, CR 701.3d, CR 603.10c, CR 704.8 (G9): X is attached to a
// host that leaves in the same pass and X's own move is Skipped: X stays and gets exactly ONE
// Unattach event, against its attachment from before the pass (executed before the host's
// zone change, so the host leaving unattaches nothing more); X is detached and reported in
// `unattached`; the host's last known information still lists X.
#[test]
fn attachment_of_a_leaving_host_is_unattached_once() {
    let (mut w, host, x) = host_leaves_world(true);
    assert_eq!(w.found(obj(x)), ["704.5f", "704.5p"]);
    let host_card = w.card_of(host);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(out.unattached, [x]);
    assert_eq!(unattach_events(&out), [x]);
    assert_eq!(
        batch_shape(&out),
        (vec![(host, ZoneChangeCause::StateBasedAction)], vec![])
    );
    assert_eq!(w.zones.object(x).unwrap().zone, ZoneId::Battlefield);
    assert_eq!(w.permanents.attached_to(x), None);
    assert_eq!(w.zone_of_card(host_card), Some(ZoneId::Graveyard(p(1))));
    assert_eq!(w.lki.get(host).unwrap().attached_here(), [x]);
}

// REQ-SBA-001, CR 704.3, CR 704.5p, CR 701.3d (G9 control): without the Skip, X and its host
// both leave; X still has exactly one Unattach event (no extra event from either zone change)
// and is not reported (it left the battlefield).
#[test]
fn attachment_and_host_leaving_together_unattach_once() {
    let (mut w, host, x) = host_leaves_world(false);
    let cards = [w.card_of(host), w.card_of(x)];
    let out = pass(&mut w);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(unattach_events(&out), [x]);
    assert_eq!(out.executed[0].events.len(), 3);
    assert_eq!(
        batch_shape(&out).0,
        [
            (host, ZoneChangeCause::StateBasedAction),
            (x, ZoneChangeCause::StateBasedAction)
        ]
    );
    assert!(out.unattached.is_empty());
    for card in cards {
        assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    }
    assert_eq!(
        w.lki.get(x).unwrap().permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(host))
    );
}

/// An Equipment attached to player 2 (CR 704.5n) and a creature D with toughness 0; with
/// `exile_it`, D's SBA move is replaced by D's move plus the Equipment's move to exile.
fn equipment_leaves_world(exile_it: bool) -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    w.attach(eq, AttachTarget::Player(p(2)));
    let d = w.creature(1, 0);
    if exile_it {
        w.registry
            .add(ReplacementEffect::new(
                e(1),
                sba_move_of(d),
                Action::Instead(vec![
                    zone_change(d, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
                    zone_change(eq, ZoneKind::Exile, ZoneChangeCause::Other),
                ]),
            ))
            .unwrap();
    }
    (w, eq, d)
}

// REQ-SBA-001, CR 704.5n (G6): an Equipment unattached by its Unattach event and then moved to
// exile by an unrelated replacement in the same pass is not in `unattached` (it did not remain
// on the battlefield); its Unattach event stays in `executed`.
#[test]
fn unattached_lists_only_objects_still_on_the_battlefield() {
    let (mut w, eq, _) = equipment_leaves_world(true);
    let card = w.card_of(eq);
    let out = pass(&mut w);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Exile));
    assert_eq!(unattach_events(&out), [eq]);
    assert!(out.unattached.is_empty());

    // Control: the Equipment stays and is listed.
    let (mut w, eq, _) = equipment_leaves_world(false);
    let out = pass(&mut w);
    assert_eq!(unattach_events(&out), [eq]);
    assert_eq!(out.unattached, [eq]);
    assert_eq!(w.zones.object(eq).unwrap().zone, ZoneId::Battlefield);
}

// REQ-SBA-001, CR 704.5j, CR 201.2a, CR 101.4b (G5): an invalid survivor set ({B} for A {X, Y},
// B {Y}, C {X}) is rejected by `ChoiceSession::submit` with `ChoiceError::Invalid` naming the
// pending request; nothing is recorded and the same request stays pending, so the player can
// answer again: {B, C} completes the pass with A removed and B, C kept. A non-selection answer
// is rejected the same way.
#[test]
fn invalid_legend_answer_is_rejected_and_can_be_corrected() {
    let mut w = World::new();
    let a = w.create_controlled(1, 1, legendary(&["test.legend_x", "test.legend_y"]));
    let b = w.create_controlled(1, 1, legendary(&["test.legend_y"]));
    let c = w.create_controlled(1, 1, legendary(&["test.legend_x"]));
    let a_card = w.card_of(a);
    let mut session = assert_legend_answer_rejected(&w, &[b]);
    let pending = session.pending().unwrap().clone();

    // A non-selection answer: rejected, still pending, nothing recorded.
    let err = session
        .submit(
            pending.request_id,
            ChoiceResponse::SelectReplacement { index: 0 },
        )
        .unwrap_err();
    assert!(
        matches!(err, ChoiceError::Invalid { request_id, .. } if request_id == pending.request_id)
    );
    assert_eq!(session.pending(), Some(&pending));
    assert!(session.answers().is_empty());

    // The corrected answer to the SAME request.
    session.submit(pending.request_id, select(&[b, c])).unwrap();
    assert_eq!(session.answers(), [select(&[b, c])]);
    let Status::Done { state, output } = session.status() else {
        panic!("not done");
    };
    let out = output.clone().unwrap();
    assert_eq!(
        batch_shape(&out).0,
        [(a, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(on_battlefield(state, &[a, b, c]), [b, c]);
    assert_eq!(state.zone_of_card(a_card), Some(ZoneId::Graveyard(p(1))));
}

// REQ-SBA-001, CR 704.5j (G5): the conversion of a bus interrupt into a choice interrupt keeps
// a choice error's class and fields (`Invalid` with its request id and explanation); a bus
// error that is not a choice error still becomes `ChoiceError::Procedure`.
#[test]
fn bus_choice_error_keeps_its_class_through_interrupt_conversion() {
    let invalid = ChoiceError::Invalid {
        request_id: RequestId::new(3),
        explanation: "test explanation".to_owned(),
    };
    match Interrupt::from(BusInterrupt::Error(BusError::Choice(invalid.clone()))) {
        Interrupt::Error(e) => assert_eq!(e, invalid),
        Interrupt::Pending(_) => panic!("pending"),
    }
    match Interrupt::from(BusInterrupt::Error(BusError::Choice(
        ChoiceError::NotPending,
    ))) {
        Interrupt::Error(e) => assert_eq!(e, ChoiceError::NotPending),
        Interrupt::Pending(_) => panic!("pending"),
    }
    let event = BusError::Event(crate::events::EventError::Overflow);
    match Interrupt::from(BusInterrupt::Error(event.clone())) {
        Interrupt::Error(ChoiceError::Procedure(m)) => assert_eq!(m, event.to_string()),
        other => panic!("{other:?}"),
    }
}

// REQ-SBA-001, CR 704.5n, CR 702.16d, CR 301.5c (G4): an Equipment on a creature the layer
// environment cannot evaluate, in a pair the environment marks illegal, becomes unattached (the
// mark is asked before the target's facts); unmarked, the same Equipment is not judged.
#[test]
fn sba_704_5n_marked_pair_with_unevaluable_target_is_unattached() {
    let build = |marked: bool| {
        let mut w = World::new();
        let host = w.creature(1, 3);
        let eq = w.create(1, ZoneKind::Battlefield, equipment());
        w.attach(eq, AttachTarget::Permanent(host));
        let card = w.card_of(host);
        w.chars.remove(&card);
        if marked {
            w.env.illegal_equipment_targets = BTreeSet::from([(eq, AttachTarget::Permanent(host))]);
        }
        (w, eq)
    };
    let (mut w, eq) = build(true);
    assert_eq!(w.found_by("704.5n"), [eq]);
    let out = pass(&mut w);
    assert_eq!(out.unattached, [eq]);
    assert_eq!(w.permanents.attached_to(eq), None);
    assert_eq!(w.zones.object(eq).unwrap().zone, ZoneId::Battlefield);

    let (mut w, _) = build(false);
    assert!(w.found_by("704.5n").is_empty());
}

// REQ-SBA-001, CR 704.5n, CR 301.5, CR 301.6 (G3): an artifact that is both an Equipment and a
// Fortification must satisfy both restrictions: on a nonland creature and on a noncreature land
// it becomes unattached; on a permanent that is both a creature and a land it stays attached.
// A plain Equipment on a creature and a plain Fortification on a land stay attached.
#[test]
fn sba_704_5n_equipment_fortification_needs_a_creature_land() {
    let both = || {
        with_subtypes(
            noncreature("test.equipment_fortification", "Artifact"),
            &["Equipment", "Fortification"],
        )
    };
    let mut creature_land = creature("test.creature_land", 3);
    creature_land.card_types = set(&["Creature", "Land"]);
    let mut w = World::new();
    let nonland_creature = w.creature(1, 3);
    let land = w.create(1, ZoneKind::Battlefield, noncreature("test.land", "Land"));
    let cl = w.create(1, ZoneKind::Battlefield, creature_land);
    let on_creature = w.create(1, ZoneKind::Battlefield, both());
    let on_land = w.create(1, ZoneKind::Battlefield, both());
    let on_both = w.create(1, ZoneKind::Battlefield, both());
    let plain_eq = w.create(1, ZoneKind::Battlefield, equipment());
    let plain_fort = w.create(1, ZoneKind::Battlefield, fortification());
    w.attach(on_creature, AttachTarget::Permanent(nonland_creature));
    w.attach(on_land, AttachTarget::Permanent(land));
    w.attach(on_both, AttachTarget::Permanent(cl));
    w.attach(plain_eq, AttachTarget::Permanent(nonland_creature));
    w.attach(plain_fort, AttachTarget::Permanent(land));
    assert_eq!(w.found_by("704.5n"), [on_creature, on_land]);
    let out = pass(&mut w);
    assert_eq!(out.unattached, [on_creature, on_land]);
    for id in [on_creature, on_land] {
        assert_eq!(w.permanents.attached_to(id), None);
        assert_eq!(w.zones.object(id).unwrap().zone, ZoneId::Battlefield);
    }
    assert_eq!(
        w.permanents.attached_to(on_both),
        Some(AttachTarget::Permanent(cl))
    );
    assert_eq!(
        w.permanents.attached_to(plain_eq),
        Some(AttachTarget::Permanent(nonland_creature))
    );
    assert_eq!(
        w.permanents.attached_to(plain_fort),
        Some(AttachTarget::Permanent(land))
    );
    assert!(w.check().is_empty());
}

// REQ-SBA-001, REQ-CORE-001, CR 704.3: the one-batch pass is deterministic: run twice from
// equal inputs (moves, counter removals of a moving and a staying permanent, unattach events of
// a staying attachment whose host leaves, replacements), it gives equal outcomes and states.
#[test]
fn one_batch_pass_is_deterministic() {
    let (mut w, _, _) = host_leaves_world(true);
    let q = w.creature(2, 0);
    w.counters(q, "+1/+1", 2);
    w.counters(q, "-1/-1", 3);
    let s = w.creature(2, 5);
    w.counters(s, "+1/+1", 1);
    w.counters(s, "-1/-1", 2);
    let first = drive(&w, &[]);
    let second = drive(&w, &[]);
    assert_eq!(first.out, second.out);
    assert_eq!(first.hash, second.hash);
    let out = first.out.unwrap();
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Review round 3 (PR #214)
// ---------------------------------------------------------------------------------------------

/// Equipment `eq` attached to a creature it may legally equip (no CR 704.5n finding) and a
/// creature D (toughness 0) whose SBA move a replacement turns into `Instead([ZoneChange D,
/// Unattach eq])`, plus `ZoneChange eq` to exile with `eq_leaves`.
fn replacement_unattach_world(eq_leaves: bool) -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    w.attach(eq, AttachTarget::Permanent(host));
    let d = w.creature(1, 0);
    let mut instead = vec![
        zone_change(d, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
        GameEvent::Unattach { attachment: eq },
    ];
    if eq_leaves {
        instead.push(zone_change(eq, ZoneKind::Exile, ZoneChangeCause::Other));
    }
    w.registry
        .add(ReplacementEffect::new(
            e(1),
            sba_move_of(d),
            Action::Instead(instead),
        ))
        .unwrap();
    (w, eq, d)
}

// REQ-SBA-001, CR 704.5n, CR 701.3d, CR 614.1 (round 3, H4): an Unattach event that a
// replacement effect of the batch produced (an `Instead` of D's SBA move) for an Equipment the
// pass did not plan to unattach (its attachment is legal) is listed in `unattached`: it became
// unattached and remains on the battlefield. Its event is in `executed`.
#[test]
fn unattached_lists_an_unattach_produced_by_a_replacement() {
    let (mut w, eq, d) = replacement_unattach_world(false);
    assert!(w.found_by("704.5n").is_empty());
    let d_card = w.card_of(d);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(unattach_events(&out), [eq]);
    assert_eq!(w.zones.object(eq).unwrap().zone, ZoneId::Battlefield);
    assert_eq!(w.permanents.attached_to(eq), None);
    assert_eq!(out.unattached, [eq]);
    assert_eq!(w.zone_of_card(d_card), Some(ZoneId::Graveyard(p(1))));
}

// REQ-SBA-001, CR 704.5n, CR 701.3d, CR 614.1 (round 3, H4 control): the same replacement also
// moves the Equipment to exile in the same pass: its Unattach event is in `executed`, but it did
// not remain on the battlefield, so it is not listed.
#[test]
fn unattached_omits_a_replacement_unattach_whose_attachment_left() {
    let (mut w, eq, _) = replacement_unattach_world(true);
    let card = w.card_of(eq);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(unattach_events(&out), [eq]);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Exile));
    assert!(out.unattached.is_empty());
}

/// Equipment `eq` attached to player 2 (illegal, CR 704.5n), a creature H2 it may legally equip,
/// and a creature Q (toughness 3) with one +1/+1 and one -1/-1 counter (CR 704.5q). With
/// `via_counters`, a replacement turns Q's +1/+1 removal into `Attach eq -> H2`; otherwise a
/// creature D (toughness 0) dies and a replacement turns its SBA move into `Instead([ZoneChange D,
/// Attach eq -> H2])`. Returns (world, eq, H2, Q).
fn reattach_world(via_counters: bool) -> (World, ObjectId, ObjectId, ObjectId) {
    let mut w = World::new();
    let eq = w.create(1, ZoneKind::Battlefield, equipment());
    let h2 = w.creature(1, 3);
    let q = w.creature(1, 3);
    w.counters(q, "+1/+1", 1);
    w.counters(q, "-1/-1", 1);
    w.attach(eq, AttachTarget::Player(p(2)));
    let attach = GameEvent::Attach {
        attachment: eq,
        target: AttachTarget::Permanent(h2),
    };
    let effect = if via_counters {
        ReplacementEffect::new(
            e(1),
            remove_counters_of(q, "+1/+1"),
            Action::Instead(vec![attach]),
        )
    } else {
        let d = w.creature(1, 0);
        ReplacementEffect::new(
            e(1),
            sba_move_of(d),
            Action::Instead(vec![
                zone_change(d, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
                attach,
            ]),
        )
    };
    w.registry.add(effect).unwrap();
    (w, eq, h2, q)
}

// REQ-SBA-001, CR 704.3, CR 704.5n, CR 701.3d, CR 614.1 (round 3, H6): a replacement turns a
// counter removal of the same SBA event into attaching the illegally attached Equipment to a
// new, legal host. The pass's planned Unattach clears only the link from before the pass: it
// is executed before the replacement's Attach (the Unattach events are submitted first, and
// the bus executes the events that are not zone changes in submission-lineage order), so the
// Equipment ends attached to the NEW host. Exactly one Unattach event, one batch.
#[test]
fn replacement_reattach_from_a_counter_removal_keeps_the_new_host() {
    let (mut w, eq, h2, q) = reattach_world(true);
    assert_eq!(w.found_by("704.5n"), [eq]);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(unattach_events(&out), [eq]);
    assert_eq!(
        w.permanents.attached_to(eq),
        Some(AttachTarget::Permanent(h2))
    );
    assert_eq!(w.zones.object(eq).unwrap().zone, ZoneId::Battlefield);
    // The replacement applied: Q's +1/+1 counter stays; its -1/-1 counter is removed.
    assert_eq!((w.counter(q, "+1/+1"), w.counter(q, "-1/-1")), (1, 0));
    // Its unattach event occurred and it remains on the battlefield (decision "#42 FP-041 —
    // Planned unattach and pre-pass host (round 3)").
    assert_eq!(out.unattached, [eq]);
    assert!(w.found_by("704.5n").is_empty());
}

// REQ-SBA-001, CR 704.3, CR 704.5n, CR 701.3d, CR 614.1 (round 3, H6): the same when the Attach
// comes from an `Instead` of another permanent's SBA move: the Equipment ends attached to the
// new host.
#[test]
fn replacement_reattach_from_a_zone_change_keeps_the_new_host() {
    let (mut w, eq, h2, _) = reattach_world(false);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(unattach_events(&out), [eq]);
    assert_eq!(
        w.permanents.attached_to(eq),
        Some(AttachTarget::Permanent(h2))
    );
}

// REQ-SBA-001, CR 704.5n, CR 701.3d (round 3, H6 control): the illegal link with no replacement
// is still cleared by exactly one Unattach event; the Equipment stays on the battlefield.
#[test]
fn illegal_link_without_replacement_is_still_unattached_once() {
    let (mut w, eq, _, q) = reattach_world(true);
    w.registry = ReplacementRegistry::default();
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(unattach_events(&out), [eq]);
    assert_eq!(w.permanents.attached_to(eq), None);
    assert_eq!(w.zones.object(eq).unwrap().zone, ZoneId::Battlefield);
    assert_eq!(out.unattached, [eq]);
    assert_eq!((w.counter(q, "+1/+1"), w.counter(q, "-1/-1")), (0, 0));
}

// ---------------------------------------------------------------------------------------------
// CR 704.5v-704.5aa (FP-042): battles, protectors, the Role rule and speed
// ---------------------------------------------------------------------------------------------

/// A synthetic battle; a Siege (CR 310.12, CR 205.3q) if `siege`, otherwise with no battle type.
fn battle(siege: bool) -> Characteristics {
    let c = noncreature("test.battle", "Battle");
    if siege {
        with_subtypes(c, &["Siege"])
    } else {
        c
    }
}

/// A synthetic Role (CR 303.7): an Aura with the subtype Role.
fn role() -> Characteristics {
    with_subtypes(noncreature("test.role", "Enchantment"), &["Aura", "Role"])
}

/// A synthetic creature with start your engines! (CR 702.179a).
fn engine_creature() -> Characteristics {
    let mut c = creature("test.engine_1_3", 3);
    c.abilities = vec![keyword("START_YOUR_ENGINES")];
    c
}

fn submit_player(n: u64) -> ChoiceResponse {
    ChoiceResponse::SubmitPlayer { player: p(n) }
}

impl World {
    /// Gives battle `id` 3 defense counters (CR 310.4c) and `protector` as its protector.
    fn stable_battle(&mut self, id: ObjectId, protector: u64) {
        self.counters(id, "defense", 3);
        self.env.battle_protectors.insert(id, p(protector));
    }

    /// A battle of `owner` controlled by `controller` with `defense` defense counters and
    /// protector `protector`.
    fn new_battle(
        &mut self,
        owner: u64,
        controller: u64,
        siege: bool,
        defense: i64,
        protector: Option<u64>,
    ) -> ObjectId {
        let id = self.create_controlled(owner, controller, battle(siege));
        if defense > 0 {
            self.counters(id, "defense", defense);
        }
        if let Some(pr) = protector {
            self.env.battle_protectors.insert(id, p(pr));
        }
        id
    }

    /// `attacker` attacks battle `target` (CR 508.1).
    fn attack_battle(&mut self, attacker: ObjectId, target: ObjectId) {
        self.permanents
            .set_attack(
                attacker,
                crate::permanent::AttackState::Attacking(Some(
                    crate::permanent::AttackTarget::Permanent(target),
                )),
            )
            .unwrap();
    }

    /// A Role of `owner` controlled by `controller` attached to `target` with timestamp `stamp`.
    fn role_on(&mut self, owner: u64, controller: u64, target: ObjectId, stamp: u64) -> ObjectId {
        let id = self.create_controlled(owner, controller, role());
        self.attach(id, AttachTarget::Permanent(target));
        self.env.attachment_since.insert(id, stamp);
        id
    }

    fn speed(&self, player: u64) -> Option<DesignationValue> {
        self.game
            .player(p(player))
            .unwrap()
            .designation(SPEED)
            .cloned()
    }

    fn set_speed(&mut self, player: u64, n: i64) {
        self.game
            .player_mut(p(player))
            .unwrap()
            .set_designation(SPEED, DesignationValue::Int(n));
    }

    fn lose(&mut self, player: u64) {
        self.game
            .player_mut(p(player))
            .unwrap()
            .set_designation(LOST_THE_GAME, DesignationValue::Flag);
    }

    /// The player subjects the check finds for `entry`, ascending.
    fn players_found_by(&mut self, entry: &str) -> Vec<PlayerId> {
        self.check()
            .into_iter()
            .filter(|f| f.entry_id == entry)
            .map(|f| match f.subject {
                SbaSubject::Player(pl) => pl,
                SbaSubject::Object(_) => panic!("object finding"),
            })
            .collect()
    }
}

/// Runs one pass on a clone of `w`, answering every request with its default; returns the run
/// and every request asked.
fn drive_defaults(w: &World) -> (Run, Vec<PendingChoiceRequest>) {
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let mut asked = Vec::new();
    loop {
        let pending = match session.status() {
            Status::Done { state, output } => {
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
        session.submit_default(pending.request_id).unwrap();
        asked.push(pending);
    }
}

// REQ-SBA-001, CR 704.5v, CR 310.7, CR 310.4c: a Siege with no defense counters (defense 0) is
// put into its OWNER's graveyard with an SBA cause, without a request; a Siege with defense 1
// stays; it is not a CR 704.5w finding.
#[test]
fn sba_704_5v_siege_at_zero_defense() {
    let mut w = World::new();
    let dead = w.new_battle(2, 1, true, 0, Some(2));
    let alive = w.new_battle(1, 1, true, 1, Some(2));
    assert_eq!(w.found_by("704.5v"), [dead]);
    assert!(w.found_by("704.5w").is_empty());
    assert_eq!(w.found(obj(dead)), ["704.5v"]);
    let card = w.card_of(dead);
    let out = pass(&mut w);
    assert!(out.performed);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        batch_shape(&out).0,
        [(dead, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(2))));
    assert!(w.zones.object(alive).is_some());
    assert!(!pass(&mut w).performed);
}

// REQ-SBA-001, CR 704.5v, CR 310.7: a Siege at defense 0 that is the source of an ability that
// has triggered but not yet left the stack survives; once the ability has left the stack, the
// repeated check collects it.
#[test]
fn sba_704_5v_pending_ability_keeps_the_siege() {
    let mut w = World::new();
    let s = w.new_battle(1, 1, true, 0, Some(2));
    w.env.battle_pending.insert(s);
    assert!(w.check().is_empty());
    assert!(!pass(&mut w).performed);
    assert!(w.zones.object(s).is_some());
    w.env.battle_pending.clear();
    assert_eq!(w.found_by("704.5v"), [s]);
    let card = w.card_of(s);
    assert!(pass(&mut w).performed);
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
}

// REQ-SBA-001, CR 704.5w, CR 310.8: a non-Siege battle at defense 0 goes to its OWNER's
// graveyard even while it is the source of a pending triggered ability (704.5w has no such
// clause); defense 2 stays; it is not a CR 704.5v finding.
#[test]
fn sba_704_5w_non_siege_at_zero_defense() {
    let mut w = World::new();
    let dead = w.new_battle(2, 1, false, 0, Some(1));
    let alive = w.new_battle(1, 1, false, 2, Some(1));
    w.env.battle_pending = BTreeSet::from([dead, alive]);
    assert_eq!(w.found_by("704.5w"), [dead]);
    assert!(w.found_by("704.5v").is_empty());
    let card = w.card_of(dead);
    let out = pass(&mut w);
    assert_eq!(
        batch_shape(&out).0,
        [(dead, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(2))));
    assert!(w.zones.object(alive).is_some());
}

// REQ-SBA-001, CR 704.5x, CR 310.12a, CR 310.9f: a Siege with no protector, not attacked: its
// controller is asked ONE CHOOSE_PLAYER request (reason CR 704.5x, candidates the opponents,
// enumerated, min = max = 1, default the lowest); the answer is recorded through
// `set_battle_protector`, reported in `protectors_chosen`, the pass is performed with no event,
// and the repeated check finds nothing.
#[test]
fn sba_704_5x_siege_without_protector() {
    let mut w = World::new();
    let s = w.new_battle(2, 1, true, 3, None);
    assert_eq!(w.found(obj(s)), ["704.5x"]);
    let (run, asked) = drive_answers(&w, &[submit_player(2)]);
    assert_eq!(asked.len(), 1);
    let r = &asked[0];
    assert_eq!(r.chooser, p(1));
    assert_eq!(r.choice_type, ChoiceType::ChoosePlayer);
    assert_eq!(r.reason.cr, "CR 704.5x");
    assert_eq!(r.reason.source, Some(s));
    assert_eq!((r.min, r.max), (1, 1));
    assert_eq!(
        r.payload,
        ChoicePayload::ChoosePlayer {
            players: vec![p(2)]
        }
    );
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Player(p(2))])
    );
    assert_eq!(r.default_on_timeout, submit_player(2));
    let out = run.out.unwrap();
    assert!(out.performed);
    assert!(out.executed.is_empty());
    assert_eq!(out.protectors_chosen, [(s, p(2))]);
    let mut w = run.state;
    assert_eq!(w.env.battle_protectors.get(&s), Some(&p(2)));
    assert!(w.check().is_empty());
    assert!(!pass(&mut w).performed);
}

// REQ-SBA-001, CR 704.5x, CR 310.9a: a battle with no battle type only offers its controller
// (even when another player owns it).
#[test]
fn sba_704_5x_typeless_battle_offers_only_its_controller() {
    let mut w = World::three([1, 2, 3], 1);
    let b = w.new_battle(3, 2, false, 3, None);
    let (run, asked) = drive_answers(&w, &[submit_player(2)]);
    assert_eq!(asked[0].chooser, p(2));
    assert_eq!(
        asked[0].payload,
        ChoicePayload::ChoosePlayer {
            players: vec![p(2)]
        }
    );
    assert_eq!(run.out.unwrap().protectors_chosen, [(b, p(2))]);
}

// REQ-SBA-001, CR 704.5x, CR 310.12a: a Siege controlled by player 2 in a three-player game
// offers exactly its controller's opponents, ascending, and never its controller; the
// controller after layer 2 (CR 613.1b) is the one asked.
#[test]
fn sba_704_5x_siege_offers_only_opponents() {
    let mut w = World::three([1, 2, 3], 1);
    let s = w.new_battle(1, 1, true, 3, None);
    w.give_control(1, s, 2);
    let (run, asked) = drive_answers(&w, &[submit_player(3)]);
    assert_eq!(asked[0].chooser, p(2));
    assert_eq!(
        asked[0].payload,
        ChoicePayload::ChoosePlayer {
            players: vec![p(1), p(3)]
        }
    );
    assert_eq!(asked[0].default_on_timeout, submit_player(1));
    assert_eq!(run.out.unwrap().protectors_chosen, [(s, p(3))]);
    // A lost player is not a candidate.
    w.lose(3);
    let (_, asked) = drive_answers(&w, &[submit_player(1)]);
    assert_eq!(
        asked[0].payload,
        ChoicePayload::ChoosePlayer {
            players: vec![p(1)]
        }
    );
}

// REQ-SBA-001, CR 704.5x, CR 704.3: the protector request is asked before any state change:
// a pending request leaves every component, the flags and the recorded protectors exactly as
// before (also asserted inside the procedure).
#[test]
fn sba_704_5x_pending_request_leaves_the_state() {
    let mut w = World::new();
    let c = w.creature(1, 0);
    w.new_battle(1, 1, true, 3, None);
    w.setup(GameEvent::Draw { player: p(2) });
    w.env.deathtouch = BTreeSet::from([c]);
    let before = w.state_hash();
    let session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let Status::Pending(r) = session.status() else {
        panic!("not pending");
    };
    assert_eq!(r.choice_type, ChoiceType::ChoosePlayer);
    assert_eq!(session.state_hash(), before);
    assert!(session.base().env.battle_protectors.is_empty());
    assert_eq!(session.base().env.deathtouch, BTreeSet::from([c]));
    assert_eq!(session.base().events.empty_library_draws(), [p(2)]);
    assert!(session.base().zones.object(c).is_some());
}

// REQ-SBA-001, CR 704.5x, CR 704.5y: an answer that is not a SubmitPlayer of a candidate is
// `ChoiceError::Invalid` naming the request (the session rejects it without recording it, the
// request stays pending); a correct answer then completes the pass.
#[test]
fn sba_704_5x_invalid_answer_is_rejected() {
    let ask = ProtectorAsk {
        battle: ObjectId::new(5),
        controller: Some(p(1)),
        candidates: vec![p(2), p(3)],
        cr: "CR 704.5x",
    };
    let rid = RequestId::new(4);
    assert_eq!(protector_answer(&ask, rid, submit_player(3)).unwrap(), p(3));
    for bad in [
        submit_player(1),
        submit_player(9),
        ChoiceResponse::SelectReplacement { index: 0 },
        select(&[ObjectId::new(5)]),
    ] {
        let err = protector_answer(&ask, rid, bad.clone()).unwrap_err();
        assert!(
            matches!(
                err,
                BusInterrupt::Error(BusError::Choice(ChoiceError::Invalid { request_id, .. }))
                    if request_id == rid
            ),
            "{bad:?}"
        );
    }
    let mut w = World::new();
    let s = w.new_battle(1, 1, true, 3, None);
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let pending = session.pending().unwrap().clone();
    assert!(
        session
            .submit(pending.request_id, submit_player(1))
            .is_err()
    );
    assert!(
        session
            .submit(pending.request_id, select(&[ObjectId::new(1)]))
            .is_err()
    );
    assert_eq!(session.pending(), Some(&pending));
    assert!(session.answers().is_empty());
    assert_eq!(session.state_hash(), w.state_hash());
    session
        .submit(pending.request_id, submit_player(2))
        .unwrap();
    let Status::Done { output, .. } = session.status() else {
        panic!("not done");
    };
    assert_eq!(output.clone().unwrap().protectors_chosen, [(s, p(2))]);
}

// REQ-SBA-001, CR 704.5x, CR 101.4: the requests of two controllers are asked in APNAP order
// (the active player's battle has the HIGHER id and is asked first); one controller's battles
// in ascending id; nothing changes before every answer; the chosen protectors are ascending by
// battle.
#[test]
fn sba_704_5x_requests_in_apnap_order() {
    let mut w = World::new();
    let n1 = w.new_battle(2, 2, true, 3, None);
    let a1 = w.new_battle(1, 1, true, 3, None);
    let a2 = w.new_battle(1, 1, false, 3, None);
    let before = w.state_hash();
    let mut session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let mut order = Vec::new();
    for answer in [submit_player(2), submit_player(1), submit_player(1)] {
        let r = session.pending().unwrap().clone();
        assert_eq!(session.state_hash(), before);
        order.push((r.chooser, r.payload.clone()));
        session.submit(r.request_id, answer).unwrap();
    }
    assert_eq!(
        order,
        [
            (
                p(1),
                ChoicePayload::ChoosePlayer {
                    players: vec![p(2)]
                }
            ),
            (
                p(1),
                ChoicePayload::ChoosePlayer {
                    players: vec![p(1)]
                }
            ),
            (
                p(2),
                ChoicePayload::ChoosePlayer {
                    players: vec![p(1)]
                }
            ),
        ]
    );
    let Status::Done { state, output } = session.status() else {
        panic!("not done");
    };
    assert_eq!(
        output.clone().unwrap().protectors_chosen,
        [(n1, p(1)), (a1, p(2)), (a2, p(1))]
    );
    assert_eq!(state.env.battle_protectors.len(), 3);
    // Three players, turn order 2, 1, 3, active 1: APNAP order 1, 3, 2.
    let mut w = World::three([2, 1, 3], 1);
    for c in [3, 2, 1] {
        w.new_battle(c, c, false, 3, None);
    }
    let (_, asked) = drive_defaults(&w);
    let choosers: Vec<PlayerId> = asked.iter().map(|r| r.chooser).collect();
    assert_eq!(choosers, [p(1), p(3), p(2)]);
}

// REQ-SBA-001, CR 704.5x, CR 310.11: no player can be chosen: no request, the battle goes to its
// OWNER's graveyard with an SBA cause in the one batch (a typeless battle whose controller
// can't protect it; a Siege whose only opponent has lost the game).
#[test]
fn sba_704_5x_no_candidate_goes_to_the_owners_graveyard() {
    let mut w = World::new();
    let b = w.new_battle(2, 1, false, 3, None);
    w.env.illegal_protectors.insert((b, p(1)));
    let card = w.card_of(b);
    let out = pass(&mut w);
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].entry_id, "704.5x");
    assert_eq!(
        batch_shape(&out).0,
        [(b, ZoneChangeCause::StateBasedAction)]
    );
    assert!(out.protectors_chosen.is_empty());
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(2))));

    let mut w = World::new();
    let s = w.new_battle(2, 1, true, 3, None);
    w.lose(2);
    let card = w.card_of(s);
    let out = pass(&mut w);
    assert_eq!(
        batch_shape(&out).0,
        [(s, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(2))));
}

// REQ-SBA-001, CR 704.5x, CR 704.5y, CR 506.4c (gate 02-state-objects F4): a battle being
// attacked with NO protector gets no request (CR 704.5x has the attack condition), but a battle
// being attacked whose protector can't be its protector IS a CR 704.5y finding and is asked;
// an attacker whose battle was removed from combat (`Attacking(None)`) attacks nothing and
// exempts nothing.
#[test]
fn sba_704_5x_attacked_exempt_but_704_5y_attacked_asked() {
    let mut w = World::new();
    let none = w.new_battle(1, 1, true, 3, None);
    let wrong = w.new_battle(1, 1, true, 3, Some(1));
    let attacker = w.creature(2, 2);
    let attacker2 = w.creature(2, 2);
    w.attack_battle(attacker, none);
    w.attack_battle(attacker2, wrong);
    assert!(w.found_by("704.5x").is_empty());
    assert_eq!(w.found_by("704.5y"), [wrong]);
    let (run, asked) = drive_answers(&w, &[submit_player(2)]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].reason.cr, "CR 704.5y");
    assert_eq!(asked[0].reason.source, Some(wrong));
    assert_eq!(run.out.unwrap().protectors_chosen, [(wrong, p(2))]);
    assert_eq!(run.state.env.battle_protectors.get(&wrong), Some(&p(2)));
    w.permanents
        .set_attack(attacker, crate::permanent::AttackState::Attacking(None))
        .unwrap();
    assert_eq!(w.found_by("704.5x"), [none]);
}

// REQ-SBA-001, CR 704.5x: a protector who lost the game, or who is not a player in the game, is
// no protector: 704.5x (not 704.5y) applies.
#[test]
fn sba_704_5x_protector_left_or_lost() {
    let mut w = World::three([1, 2, 3], 1);
    let lost = w.new_battle(1, 1, true, 3, Some(2));
    let gone = w.new_battle(1, 1, true, 3, Some(9));
    let fine = w.new_battle(1, 1, true, 3, Some(3));
    w.lose(2);
    assert_eq!(w.found_by("704.5x"), [lost, gone]);
    assert!(w.found_by("704.5y").is_empty());
    let (run, asked) = drive_answers(&w, &[submit_player(3), submit_player(3)]);
    assert!(asked.iter().all(|r| r.reason.cr == "CR 704.5x"));
    assert!(asked.iter().all(|r| r.payload
        == ChoicePayload::ChoosePlayer {
            players: vec![p(3)]
        }));
    let out = run.out.unwrap();
    assert_eq!(out.protectors_chosen, [(lost, p(3)), (gone, p(3))]);
    assert_eq!(run.state.env.battle_protectors[&fine], p(3));
}

// REQ-SBA-001, CR 704.5y, CR 310.11, CR 310.12a, CR 310.9a: a Siege protected by its own
// controller, a typeless battle protected by an opponent, and a Siege whose protector effects
// forbid are 704.5y findings; each controller chooses again (reason CR 704.5y; the old
// protector is not a candidate); with no candidate left the battle goes to its owner's
// graveyard.
#[test]
fn sba_704_5y_illegal_protector() {
    let mut w = World::new();
    let own = w.new_battle(1, 1, true, 3, Some(1));
    let typeless = w.new_battle(1, 1, false, 3, Some(2));
    let forbidden = w.new_battle(2, 1, true, 3, Some(2));
    w.env.illegal_protectors.insert((forbidden, p(2)));
    assert_eq!(w.found_by("704.5y"), [own, typeless, forbidden]);
    assert!(w.found_by("704.5x").is_empty());
    let card = w.card_of(forbidden);
    let (run, asked) = drive_answers(&w, &[submit_player(2), submit_player(1)]);
    assert_eq!(asked.len(), 2);
    assert!(asked.iter().all(|r| r.reason.cr == "CR 704.5y"));
    assert_eq!(
        asked[0].payload,
        ChoicePayload::ChoosePlayer {
            players: vec![p(2)]
        }
    );
    assert_eq!(
        asked[1].payload,
        ChoicePayload::ChoosePlayer {
            players: vec![p(1)]
        }
    );
    let out = run.out.unwrap();
    assert_eq!(out.protectors_chosen, [(own, p(2)), (typeless, p(1))]);
    assert_eq!(
        batch_shape(&out).0,
        [(forbidden, ZoneChangeCause::StateBasedAction)]
    );
    let mut w = run.state;
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(2))));
    assert!(w.check().is_empty());
}

// REQ-SBA-001, CR 704.5v, CR 704.5w, CR 704.5x, CR 704.5y: a battle that goes to the graveyard
// through 704.5v or 704.5w in the same pass gets no protector request; its x/y finding is still
// reported; it moves once.
#[test]
fn sba_704_5x_y_battle_at_zero_defense_asks_nothing() {
    let mut w = World::new();
    let s = w.new_battle(1, 1, true, 0, None);
    let b = w.new_battle(1, 1, false, 0, Some(2));
    assert_eq!(w.found(obj(s)), ["704.5v", "704.5x"]);
    assert_eq!(w.found(obj(b)), ["704.5w", "704.5y"]);
    let out = pass(&mut w);
    assert!(out.protectors_chosen.is_empty());
    assert_eq!(
        batch_shape(&out).0,
        [
            (s, ZoneChangeCause::StateBasedAction),
            (b, ZoneChangeCause::StateBasedAction)
        ]
    );
    assert_eq!(out.findings.len(), 4);
    assert!(!w.env.battle_protectors.contains_key(&s));
}

// Gate 02-state-objects R3: a battle at defense 0 whose graveyard move a replacement effect
// skipped stays on the battlefield and gets its protector choice after the batch.

impl World {
    /// A persistent `Skip` of the state-based graveyard move of `object` (CR 614.6).
    fn skip_sba_move(&mut self, effect: u64, object: ObjectId) {
        self.registry
            .add(ReplacementEffect::new(
                e(effect),
                sba_move_of(object),
                Action::Skip,
            ))
            .unwrap();
    }
}

/// Runs the retained-battle scenario of `w` (battle `b`, expected entries `entries`, reason
/// `cr`, one candidate `candidate`): exactly one request after the batch, a pending pass leaves
/// the state unchanged, the answer is recorded, the pass is deterministic, and the next pass asks
/// nothing and still skips the move. Returns the world after the first pass.
fn assert_retained_battle_asked(
    mut w: World,
    b: ObjectId,
    entries: [&str; 2],
    cr: &str,
    candidate: u64,
) -> World {
    assert_eq!(w.found(obj(b)), entries);
    let before = w.state_hash();
    // Pending: the request is the only one, and the state is exactly as before the pass (also
    // asserted inside the procedure).
    let session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let pending = session.pending().expect("no request").clone();
    assert_eq!(session.state_hash(), before);
    assert_eq!(pending.choice_type, ChoiceType::ChoosePlayer);
    assert_eq!(pending.chooser, p(1));
    assert_eq!(pending.reason.cr, cr);
    assert_eq!(pending.reason.source, Some(b));
    assert_eq!((pending.min, pending.max), (1, 1));
    assert!(!pending.hidden);
    assert_eq!(pending.default_on_timeout, submit_player(candidate));
    assert_eq!(
        pending.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Player(p(candidate))])
    );
    assert_eq!(
        pending.payload,
        ChoicePayload::ChoosePlayer {
            players: vec![p(candidate)]
        }
    );
    let (run, asked) = drive_answers(&w, &[submit_player(candidate)]);
    assert_eq!(asked, [pending]);
    let out = run.out.as_ref().unwrap().clone();
    assert_eq!(out.protectors_chosen, [(b, p(candidate))]);
    assert!(out.performed);
    assert!(out.executed.is_empty());
    assert_eq!(run.state.zones.object(b).unwrap().zone, ZoneId::Battlefield);
    assert_eq!(run.state.env.battle_protectors.get(&b), Some(&p(candidate)));
    // CR 704.3, REQ-CORE-001: the same requests (ids included) and the same result again.
    let (again, asked_again) = drive_answers(&w, &[submit_player(candidate)]);
    assert_eq!(asked_again, asked);
    assert_eq!(again.hash, run.hash);
    assert_eq!(again.out, run.out);
    // T2: the next pass asks nothing more for it; the move is still skipped.
    w = run.state;
    let out = pass(&mut w);
    assert!(out.protectors_chosen.is_empty());
    assert!(out.executed.is_empty());
    assert!(!out.performed);
    assert_eq!(out.findings.len(), 1);
    assert_eq!(w.zones.object(b).unwrap().zone, ZoneId::Battlefield);
    w
}

// REQ-SBA-001, CR 704.5w, CR 704.5x, CR 614.6, CR 704.3 (gate 02-state-objects R3): a non-Siege
// battle at defense 0 with no protector whose graveyard move a persistent replacement skips stays
// on the battlefield: its controller is asked its protector after the batch (one CHOOSE_PLAYER,
// reason CR 704.5x, source the battle); the next pass asks nothing.
#[test]
fn sba_704_5w_x_retained_battle_is_asked_after_the_batch() {
    let mut w = World::new();
    let b = w.new_battle(1, 1, false, 0, None);
    w.skip_sba_move(1, b);
    assert_retained_battle_asked(w, b, ["704.5w", "704.5x"], "CR 704.5x", 1);
}

// REQ-SBA-001, CR 704.5v, CR 704.5x, CR 614.6 (gate 02-state-objects R3): the same for a Siege at
// defense 0 (its candidates are its controller's opponents).
#[test]
fn sba_704_5v_x_retained_siege_is_asked_after_the_batch() {
    let mut w = World::new();
    let s = w.new_battle(1, 1, true, 0, None);
    w.skip_sba_move(1, s);
    assert_retained_battle_asked(w, s, ["704.5v", "704.5x"], "CR 704.5x", 2);
}

// REQ-SBA-001, CR 704.5w, CR 704.5y, CR 614.6 (gate 02-state-objects R3): the same for a
// typeless battle at defense 0 whose protector (an opponent) can't be its protector: reason
// CR 704.5y, the old protector replaced.
#[test]
fn sba_704_5w_y_retained_battle_is_asked_after_the_batch() {
    let mut w = World::new();
    let b = w.new_battle(1, 1, false, 0, Some(2));
    w.skip_sba_move(1, b);
    let w = assert_retained_battle_asked(w, b, ["704.5w", "704.5y"], "CR 704.5y", 1);
    assert_eq!(w.env.battle_protectors[&b], p(1));
}

// REQ-SBA-001, CR 704.3, CR 704.5f, CR 704.5w, CR 704.5x, CR 704.8 (gate 02-state-objects R3):
// the post-batch request of a retained battle comes after a batch that changed the state (a
// creature died); a pending post-batch request still leaves every component exactly as before
// the pass, and answering it completes the pass with both results.
#[test]
fn sba_704_5x_retained_request_pending_after_a_real_batch_restores() {
    let mut w = World::new();
    let c = w.creature(2, 0);
    let c_card = w.card_of(c);
    let b = w.new_battle(1, 1, false, 0, None);
    w.skip_sba_move(1, b);
    let before = w.state_hash();
    let session = ChoiceSession::start(w.clone(), run_pass).unwrap();
    let pending = session.pending().expect("no request").clone();
    assert_eq!(pending.reason.source, Some(b));
    assert_eq!(session.state_hash(), before);
    assert!(session.base().zones.object(c).is_some());
    let (run, asked) = drive_answers(&w, &[submit_player(1)]);
    assert_eq!(asked, [pending]);
    let out = run.out.unwrap();
    // The skipped move of the battle never happens; only the creature's move occurred.
    assert_eq!(
        batch_shape(&out).0,
        [(c, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(out.protectors_chosen, [(b, p(1))]);
    assert_eq!(
        run.state.zone_of_card(c_card),
        Some(ZoneId::Graveyard(p(2)))
    );
    assert_eq!(run.state.zones.object(b).unwrap().zone, ZoneId::Battlefield);
}

// REQ-SBA-001, CR 704.5x, CR 704.5w, CR 614.6 (gate 02-state-objects R3): two retained battles of
// different controllers are asked in APNAP order of the choosers (the active player's battle has
// the HIGHER id and is asked first), then ascending battle id; a retained battle comes after an
// ordinary pre-batch protector request even when the ordinary chooser is later in APNAP order.
#[test]
fn sba_704_5x_retained_requests_in_apnap_order_after_the_pre_batch_ones() {
    let mut w = World::new();
    let n = w.new_battle(2, 2, false, 0, None);
    let a1 = w.new_battle(1, 1, false, 0, None);
    let a2 = w.new_battle(1, 1, true, 0, None);
    for (i, b) in [n, a1, a2].into_iter().enumerate() {
        w.skip_sba_move(u64::try_from(i).unwrap() + 1, b);
    }
    let (run, asked) = drive_answers(&w, &[submit_player(1), submit_player(2), submit_player(2)]);
    let order: Vec<(PlayerId, Option<ObjectId>)> =
        asked.iter().map(|r| (r.chooser, r.reason.source)).collect();
    assert_eq!(order, [(p(1), Some(a1)), (p(1), Some(a2)), (p(2), Some(n))]);
    assert!(asked.windows(2).all(|r| r[0].request_id < r[1].request_id));
    assert_eq!(
        run.out.unwrap().protectors_chosen,
        [(n, p(2)), (a1, p(1)), (a2, p(2))]
    );

    // An ordinary protector choice of player 2 (asked before the batch) comes before the
    // retained battle of the active player 1 (asked after it).
    let mut w = World::new();
    let ordinary = w.new_battle(2, 2, false, 3, None);
    let retained = w.new_battle(1, 1, false, 0, None);
    w.skip_sba_move(1, retained);
    let (run, asked) = drive_answers(&w, &[submit_player(2), submit_player(1)]);
    let order: Vec<(PlayerId, Option<ObjectId>)> =
        asked.iter().map(|r| (r.chooser, r.reason.source)).collect();
    assert_eq!(order, [(p(2), Some(ordinary)), (p(1), Some(retained))]);
    assert!(asked[0].request_id < asked[1].request_id);
    assert_eq!(
        run.out.unwrap().protectors_chosen,
        [(ordinary, p(2)), (retained, p(1))]
    );
}

// REQ-SBA-001, CR 704.5v, CR 704.5w, CR 704.5x, CR 704.8 (gate 02-state-objects R3): a battle at
// defense 0 whose move is NOT replaced leaves with the batch and gets no request, not even after
// the batch; the pass consumes no request id (the next request has the same id as in a world
// without that battle).
#[test]
fn sba_704_5x_leaving_battle_consumes_no_request_id() {
    let mut base = World::new();
    let ordinary = base.new_battle(2, 2, false, 3, None);
    let (_, asked_base) = drive_answers(&base, &[submit_player(2)]);
    let mut w = base.clone();
    let s = w.new_battle(1, 1, true, 0, None);
    let b = w.new_battle(1, 1, false, 0, None);
    let (run, asked) = drive_answers(&w, &[submit_player(2)]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].reason.source, Some(ordinary));
    assert_eq!(asked[0].request_id, asked_base[0].request_id);
    let out = run.out.unwrap();
    assert_eq!(out.protectors_chosen, [(ordinary, p(2))]);
    assert_eq!(
        batch_shape(&out).0,
        [
            (s, ZoneChangeCause::StateBasedAction),
            (b, ZoneChangeCause::StateBasedAction)
        ]
    );
}

// REQ-SBA-001, CR 704.5w, CR 704.5x, CR 614.6 (gate 02-state-objects R3): a retained battle with
// no candidate (its controller can't protect it) gets no request and no panic; the pass is not
// performed by it and the state is unchanged.
#[test]
fn sba_704_5x_retained_battle_without_candidate_asks_nothing() {
    let mut w = World::new();
    let b = w.new_battle(1, 1, false, 0, None);
    w.env.illegal_protectors.insert((b, p(1)));
    w.skip_sba_move(1, b);
    let before = w.state_hash();
    let out = pass(&mut w);
    assert!(out.protectors_chosen.is_empty());
    assert!(!out.performed);
    assert_eq!(out.findings.len(), 2);
    assert_eq!(w.zones.object(b).unwrap().zone, ZoneId::Battlefield);
    assert_eq!(w.state_hash(), before);
}

// Gate 02-state-objects R3b: the retained-battle request reads the layer inputs of the state
// after the batch, which `perform_all_relayered` receives from the caller's provider.

/// How a test caller of [`perform_all_relayered`] derives its layer inputs (CR 613.1, CR 613.4).
#[derive(Clone, Copy, Debug)]
struct Relayer {
    /// `(equipment, battle, player)`: the caller's own rule "while `equipment` is attached to
    /// `battle`, a control-changing layer effect gives `battle` to `player`", applied to the
    /// permanents of the state it describes.
    rule: Option<(ObjectId, ObjectId, u64)>,
    /// The provider hands back the PRE-batch inputs instead of inputs of the state after the
    /// batch (what [`perform_all`] does).
    stale: bool,
}

/// `base` plus the control effect of `rule` if its equipment is attached to its battle in
/// `permanents`.
fn rule_effects(
    base: &ContinuousEffects,
    permanents: &Permanents,
    rule: Option<(ObjectId, ObjectId, u64)>,
) -> ContinuousEffects {
    let mut effects = base.clone();
    if let Some((equipment, battle, player)) = rule
        && permanents.attached_to(equipment) == Some(AttachTarget::Permanent(battle))
    {
        effects
            .register(
                e(90),
                ContinuousEffect {
                    source: Some(equipment),
                    controller: p(player),
                    timestamp: Timestamp::new(90),
                    layer: Layer::L2,
                    is_cda: false,
                    origin: EffectOrigin::Resolved,
                    affected: Affected::Objects(BTreeSet::from([battle])),
                    modification: LayerModification::SetController(p(player)),
                },
            )
            .unwrap();
    }
    effects
}

/// One `perform_all_relayered` pass over `w` whose layer inputs follow `relayer`; every call of
/// the provider increments `calls`. Pending and error handling as [`run_pass`].
fn run_pass_relayered(
    w: &mut World,
    ch: &mut ChoiceContext<'_>,
    relayer: Relayer,
    calls: &std::cell::Cell<usize>,
) -> Result<Out, Interrupt> {
    let before = w.state_hash();
    let snap = w.snapshot();
    let pre_effects = rule_effects(&w.effects, &w.permanents, relayer.rule);
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
        chars,
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
    let mut provider =
        |after: &EventContext<'_>,
         view: &mut dyn FnMut(&ContinuousEffects, &dyn LayerEnvironment)| {
            calls.set(calls.get() + 1);
            if relayer.stale {
                view(&pre_effects, &snap);
                return;
            }
            // A fresh snapshot of the context after the batch, and the caller's rule applied
            // to its permanents.
            let fresh = snapshot_of(after.zones, after.events, chars);
            let fresh_effects = rule_effects(effects, after.permanents, relayer.rule);
            view(&fresh_effects, &fresh);
        };
    let result = perform_all_relayered(
        &registry,
        &mut ctx,
        replacements,
        &pre_effects,
        &snap,
        env,
        ch,
        &mut provider,
    );
    settle(w, before, result)
}

/// Runs one `perform_all_relayered` pass on a clone of `w` in a choice session, answering
/// `answers` in order; returns the run, every request asked and the number of provider calls of
/// each run of the procedure (the start, then one per answer).
fn drive_relayered(
    w: &World,
    answers: &[ChoiceResponse],
    relayer: Relayer,
) -> (Run, Vec<PendingChoiceRequest>, Vec<usize>) {
    let calls = std::cell::Cell::new(0);
    let mut session =
        ChoiceSession::start(w.clone(), |w: &mut World, ch: &mut ChoiceContext<'_>| {
            run_pass_relayered(w, ch, relayer, &calls)
        })
        .unwrap();
    let mut per_run = vec![calls.replace(0)];
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
                return (run, asked, per_run);
            }
            Status::Pending(r) => r.clone(),
        };
        let answer = answers.next().expect("more requests than answers").clone();
        session.submit(pending.request_id, answer).unwrap();
        per_run.push(calls.replace(0));
        asked.push(pending);
    }
}

/// A typeless battle `b` of player 2 at defense 0 with no protector whose graveyard move a
/// persistent replacement skips, with an Equipment `gear` of player 1 attached to it (an
/// Equipment can't equip a battle, so the batch unattaches it). The caller's rule gives control
/// of `b` to player 1 while `gear` is attached: before the batch player 1 controls `b`, after it
/// player 2 does. Returns (world, b, gear, rule).
fn relayer_world() -> (World, ObjectId, ObjectId, Option<(ObjectId, ObjectId, u64)>) {
    let mut w = World::new();
    let b = w.new_battle(2, 2, false, 0, None);
    w.skip_sba_move(1, b);
    let gear = w.create(1, ZoneKind::Battlefield, equipment());
    w.attach(gear, AttachTarget::Permanent(b));
    (w, b, gear, Some((gear, b, 1)))
}

// REQ-SBA-001, REQ-LAYER-001, CR 704.5w, CR 704.5x, CR 614.6, CR 704.3, CR 613.1 (gate
// 02-state-objects R3b, thread 4697581022): a retained battle whose controller, through a layer
// effect, changes with the batch is asked by its controller AFTER the batch, with the candidates
// of the state after the batch, when the caller supplies post-batch layer inputs. With the
// pre-batch inputs (a stale provider, or `perform_all`) the request would go to the pre-batch
// controller: both are asserted, so the test fails if the post-batch view used stale inputs.
#[test]
fn sba_704_5x_retained_request_reads_post_batch_layer_inputs() {
    let (w, b, gear, rule) = relayer_world();
    let fresh = Relayer { rule, stale: false };
    let (run, asked, calls) = drive_relayered(&w, &[submit_player(2)], fresh);
    assert_eq!(calls, [1, 1]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].chooser, p(2));
    assert_eq!(asked[0].reason.cr, "CR 704.5x");
    assert_eq!(asked[0].reason.source, Some(b));
    assert_eq!(
        asked[0].payload,
        ChoicePayload::ChoosePlayer {
            players: vec![p(2)]
        }
    );
    assert_eq!(asked[0].default_on_timeout, submit_player(2));
    let out = run.out.unwrap();
    assert_eq!(out.unattached, [gear]);
    assert_eq!(out.protectors_chosen, [(b, p(2))]);
    assert_eq!(run.state.permanents.attached_to(gear), None);
    assert_eq!(run.state.zones.object(b).unwrap().zone, ZoneId::Battlefield);
    assert_eq!(run.state.env.battle_protectors.get(&b), Some(&p(2)));

    // The stale pre-batch inputs: the pre-batch controller (player 1) and its candidates.
    let stale = Relayer { rule, stale: true };
    let (run_stale, asked_stale, _) = drive_relayered(&w, &[submit_player(1)], stale);
    assert_eq!(asked_stale.len(), 1);
    assert_eq!(asked_stale[0].chooser, p(1));
    assert_eq!(
        asked_stale[0].payload,
        ChoicePayload::ChoosePlayer {
            players: vec![p(1)]
        }
    );
    assert_eq!(asked_stale[0].request_id, asked[0].request_id);

    // T2: `perform_all` with the same pre-batch inputs gives exactly the stale result.
    let mut w_all = w.clone();
    w_all.effects = rule_effects(&w.effects, &w.permanents, rule);
    let (run_all, asked_all) = drive_answers(&w_all, &[submit_player(1)]);
    assert_eq!(asked_all, asked_stale);
    assert_eq!(run_all.out, run_stale.out);
}

// REQ-SBA-001, REQ-CORE-001, CR 704.3, CR 704.5x, CR 704.5v, CR 704.5w, CR 704.8 (gate
// 02-state-objects R3b): the provider is never called by a pass without a retained battle (an
// ordinary protector request before the batch) nor by one whose zero-defense battles all leave
// with the batch; it is called exactly once per run of a pass with two retained battles, and
// that pass asks exactly what `perform_all` asks.
#[test]
fn sba_704_5x_post_batch_provider_called_only_for_retained_battles() {
    let plain = Relayer {
        rule: None,
        stale: false,
    };
    let mut w = World::new();
    let ordinary = w.new_battle(2, 2, false, 3, None);
    let leaving = w.new_battle(1, 1, true, 0, None);
    let (run, asked, calls) = drive_relayered(&w, &[submit_player(2)], plain);
    assert_eq!(calls, [0, 0]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].reason.source, Some(ordinary));
    let out = run.out.unwrap();
    assert_eq!(
        batch_shape(&out).0,
        [(leaving, ZoneChangeCause::StateBasedAction)]
    );
    let (run_all, asked_all) = drive_answers(&w, &[submit_player(2)]);
    assert_eq!(asked_all, asked);
    assert_eq!(run_all.out.unwrap(), out);

    let mut w = World::new();
    let a1 = w.new_battle(1, 1, false, 0, None);
    let a2 = w.new_battle(1, 1, true, 0, None);
    w.skip_sba_move(1, a1);
    w.skip_sba_move(2, a2);
    let answers = [submit_player(1), submit_player(2)];
    let (run, asked, calls) = drive_relayered(&w, &answers, plain);
    assert_eq!(calls, [1, 1, 1]);
    let sources: Vec<Option<ObjectId>> = asked.iter().map(|r| r.reason.source).collect();
    assert_eq!(sources, [Some(a1), Some(a2)]);
    let (run_all, asked_all) = drive_answers(&w, &answers);
    assert_eq!(asked_all, asked);
    assert_eq!(run_all.out, run.out);
    assert_eq!(run_all.hash, run.hash);
    assert_eq!(run.out.unwrap().protectors_chosen, [(a1, p(1)), (a2, p(2))]);
}

// REQ-SBA-001, REQ-CORE-001, CR 704.3, CR 704.5x, CR 614.6 (gate 02-state-objects R3b): a
// pending post-batch request through `perform_all_relayered` leaves the world exactly as before
// the pass (also asserted inside the procedure), although the provider read the state after the
// batch; answering completes the pass, and a second run reproduces the same request ids and the
// same result.
#[test]
fn sba_704_5x_relayered_pending_restores_and_replays() {
    let (w, b, _, rule) = relayer_world();
    let fresh = Relayer { rule, stale: false };
    let before = w.state_hash();
    let calls = std::cell::Cell::new(0);
    let session = ChoiceSession::start(w.clone(), |w: &mut World, ch: &mut ChoiceContext<'_>| {
        run_pass_relayered(w, ch, fresh, &calls)
    })
    .unwrap();
    let pending = session.pending().expect("no request").clone();
    assert_eq!(pending.reason.source, Some(b));
    assert_eq!(session.state_hash(), before);
    assert_eq!(session.base().state_hash(), before);
    let (run, asked, _) = drive_relayered(&w, &[submit_player(2)], fresh);
    assert_eq!(asked, [pending]);
    let (again, asked_again, _) = drive_relayered(&w, &[submit_player(2)], fresh);
    assert_eq!(asked_again, asked);
    assert_eq!(again.hash, run.hash);
    assert_eq!(again.out, run.out);
    assert_ne!(run.hash, before);
}

// Gate 02-state-objects R3c: the retained-battle request reads the protector facts of the state
// after the batch from the caller's read-only view (`perform_all_post_batch`), names the CR
// rule of the entry that flags the battle after the batch, and is never addressed to a player
// who has lost the game.

/// How a test caller of [`perform_all_post_batch`] derives its read-only post-batch protector
/// view from the pre-batch environment and the context after the batch.
type ViewRule<'a> = &'a dyn Fn(&StoredSbaEnvironment, &EventContext<'_>) -> StoredSbaEnvironment;

/// One `perform_all_post_batch` pass over `w`: fresh layer inputs of the state after the batch,
/// and the protector view `rule` derives. Pending and error handling as [`run_pass`].
fn run_pass_post_batch(
    w: &mut World,
    ch: &mut ChoiceContext<'_>,
    rule: ViewRule<'_>,
) -> Result<Out, Interrupt> {
    let before = w.state_hash();
    let snap = w.snapshot();
    let pre_env = w.env.clone();
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
        chars,
        env,
        ..
    } = w;
    let effects: &ContinuousEffects = effects;
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
    let mut provider = |after: &EventContext<'_>,
                        view: &mut dyn FnMut(
        &ContinuousEffects,
        &dyn LayerEnvironment,
        &dyn SbaEnvironment,
    )| {
        let fresh = snapshot_of(after.zones, after.events, chars);
        let facts = rule(&pre_env, after);
        view(effects, &fresh, &facts);
    };
    let result = perform_all_post_batch(
        &registry,
        &mut ctx,
        replacements,
        effects,
        &snap,
        env,
        ch,
        &mut provider,
    );
    settle(w, before, result)
}

/// Runs one `perform_all_post_batch` pass on a clone of `w` in a choice session, answering
/// `answers` in order; returns the run and every request asked.
fn drive_post_batch(
    w: &World,
    answers: &[ChoiceResponse],
    rule: ViewRule<'_>,
) -> (Run, Vec<PendingChoiceRequest>) {
    let mut session =
        ChoiceSession::start(w.clone(), |w: &mut World, ch: &mut ChoiceContext<'_>| {
            run_pass_post_batch(w, ch, rule)
        })
        .unwrap();
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

/// The protector candidates of a `CHOOSE_PLAYER` request.
fn player_candidates(r: &PendingChoiceRequest) -> Vec<PlayerId> {
    match &r.payload {
        ChoicePayload::ChoosePlayer { players } => players.clone(),
        other => panic!("not a CHOOSE_PLAYER payload: {other:?}"),
    }
}

/// Three players (active player 1): a Siege `s` of player 1 at defense 0 with no protector,
/// unattacked, whose graveyard move a persistent replacement skips, with an Equipment `gear`
/// attached to it (the batch unattaches it). The caller's environment says player 2
/// can't be the protector of `s` while `gear` is attached to it. Returns (world, s, gear).
fn post_batch_eligibility_world() -> (World, ObjectId, ObjectId) {
    let mut w = World::three([1, 2, 3], 1);
    let s = w.new_battle(1, 1, true, 0, None);
    w.skip_sba_move(1, s);
    let gear = w.create(1, ZoneKind::Battlefield, equipment());
    w.attach(gear, AttachTarget::Permanent(s));
    w.env.illegal_protectors.insert((s, p(2)));
    (w, s, gear)
}

// REQ-SBA-001, REQ-LAYER-001, CR 704.5v, CR 704.5x, CR 614.6, CR 704.3 (gate 02-state-objects
// R3c, thread 4698236604): the post-batch request of a retained Siege reads who can be its
// protector from the caller's post-batch view: player 2 becomes eligible when the batch
// unattaches the Equipment, so the candidates are players 2 and 3; with the pre-batch
// environment (the view routed back to it, or `perform_all`) they would be player 3 only. The
// chosen protector is written through the ORIGINAL environment after the pass succeeded (the
// view is never written back); a pending request leaves the world, its environment flags and
// recorded protectors included, exactly as before the pass.
#[test]
fn sba_704_5x_retained_request_reads_post_batch_eligibility_view() {
    let (w, s, gear) = post_batch_eligibility_world();
    assert_eq!(w.clone().found(obj(s)), ["704.5v", "704.5x"]);
    let unattached = move |pre: &StoredSbaEnvironment, after: &EventContext<'_>| {
        let mut view = pre.clone();
        if after.permanents.attached_to(gear) != Some(AttachTarget::Permanent(s)) {
            view.illegal_protectors.remove(&(s, p(2)));
        }
        view
    };
    let before = w.state_hash();
    let session = ChoiceSession::start(w.clone(), |w: &mut World, ch: &mut ChoiceContext<'_>| {
        run_pass_post_batch(w, ch, &unattached)
    })
    .unwrap();
    let pending = session.pending().expect("no request").clone();
    assert_eq!(session.state_hash(), before);
    assert_eq!(session.base().env, w.env);
    assert_eq!(pending.chooser, p(1));
    assert_eq!(pending.reason.cr, "CR 704.5x");
    assert_eq!(pending.reason.source, Some(s));
    assert_eq!(player_candidates(&pending), [p(2), p(3)]);
    assert_eq!(pending.default_on_timeout, submit_player(2));

    let (run, asked) = drive_post_batch(&w, &[submit_player(2)], &unattached);
    assert_eq!(asked, std::slice::from_ref(&pending));
    let out = run.out.unwrap();
    assert_eq!(out.unattached, [gear]);
    assert_eq!(out.protectors_chosen, [(s, p(2))]);
    assert_eq!(run.state.env.battle_protectors.get(&s), Some(&p(2)));
    // The view's eligibility never reaches the original environment.
    assert!(run.state.env.illegal_protectors.contains(&(s, p(2))));
    assert_eq!(run.state.zones.object(s).unwrap().zone, ZoneId::Battlefield);

    // The view routed back to the pre-batch environment: player 3 only, same request id.
    let stale = |pre: &StoredSbaEnvironment, _: &EventContext<'_>| pre.clone();
    let (run_stale, asked_stale) = drive_post_batch(&w, &[submit_player(3)], &stale);
    assert_eq!(asked_stale.len(), 1);
    assert_eq!(player_candidates(&asked_stale[0]), [p(3)]);
    assert_eq!(asked_stale[0].request_id, pending.request_id);
    // T2: `perform_all` (and so `perform_all_relayered`) reads the pass's own environment.
    let (run_all, asked_all) = drive_answers(&w, &[submit_player(3)]);
    assert_eq!(asked_all, asked_stale);
    assert_eq!(run_all.out, run_stale.out);
    assert_eq!(run_all.hash, run_stale.hash);
}

// REQ-SBA-001, CR 704.5v, CR 704.5x, CR 704.5y, CR 614.6 (gate 02-state-objects R3c,
// thread 4698236617): an unattacked Siege at defense 0 whose protector (player 2) can't be its
// protector is a CR 704.5y subject before the batch; player 2 loses the game in the same pass,
// so after the batch the Siege has no protector and the entry that flags it is CR 704.5x: the
// post-batch request names CR 704.5x, not the pre-batch CR 704.5y.
#[test]
fn sba_704_5x_retained_request_names_the_post_batch_rule() {
    let mut w = World::three([1, 2, 3], 1);
    let s = w.new_battle(1, 1, true, 0, Some(2));
    w.env.illegal_protectors.insert((s, p(2)));
    w.skip_sba_move(1, s);
    w.set_life(2, 0);
    assert_eq!(w.found(obj(s)), ["704.5v", "704.5y"]);
    let (run, asked) = drive_answers(&w, &[submit_player(3)]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].chooser, p(1));
    assert_eq!(asked[0].reason.cr, "CR 704.5x");
    assert_eq!(asked[0].reason.source, Some(s));
    assert_eq!(player_candidates(&asked[0]), [p(3)]);
    let out = run.out.unwrap();
    assert_eq!(out.lost_players, [p(2)]);
    assert_eq!(out.protectors_chosen, [(s, p(3))]);
    assert_eq!(run.state.env.battle_protectors.get(&s), Some(&p(3)));
}

// REQ-SBA-001, CR 704.5v, CR 704.5x, CR 704.5y, CR 614.6 (gate 02-state-objects R3c, thread
// 4698236617): controls. A retained Siege that is CR 704.5y before and after the batch keeps
// CR 704.5y (also when the post-batch view changes why its protector is illegal); one that is
// CR 704.5x both times keeps CR 704.5x; one that is CR 704.5x before the batch and, in the
// caller's post-batch view, has a protector that can't protect it (its own controller) is
// asked with CR 704.5y.
#[test]
fn sba_704_5x_y_retained_request_rule_controls() {
    // 704.5y both times (the pass's own environment).
    let mut w = World::three([1, 2, 3], 1);
    let y = w.new_battle(1, 1, true, 0, Some(2));
    w.env.illegal_protectors.insert((y, p(2)));
    w.skip_sba_move(1, y);
    assert_eq!(w.found(obj(y)), ["704.5v", "704.5y"]);
    let (run, asked) = drive_answers(&w, &[submit_player(3)]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].reason.cr, "CR 704.5y");
    assert_eq!(player_candidates(&asked[0]), [p(3)]);
    assert_eq!(run.out.unwrap().protectors_chosen, [(y, p(3))]);

    // 704.5y both times, the eligibility changed by the view: the protector is now the Siege's
    // own controller (illegal by battle type) and player 2 is eligible again.
    let own = move |pre: &StoredSbaEnvironment, _: &EventContext<'_>| {
        let mut view = pre.clone();
        view.illegal_protectors.remove(&(y, p(2)));
        view.battle_protectors.insert(y, p(1));
        view
    };
    let (run, asked) = drive_post_batch(&w, &[submit_player(2)], &own);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].reason.cr, "CR 704.5y");
    assert_eq!(player_candidates(&asked[0]), [p(2), p(3)]);
    assert_eq!(run.out.unwrap().protectors_chosen, [(y, p(2))]);
    assert_eq!(run.state.env.battle_protectors.get(&y), Some(&p(2)));

    // 704.5x both times.
    let mut w = World::three([1, 2, 3], 1);
    let x = w.new_battle(1, 1, true, 0, None);
    w.skip_sba_move(1, x);
    assert_eq!(w.found(obj(x)), ["704.5v", "704.5x"]);
    let (_, asked) = drive_answers(&w, &[submit_player(2)]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].reason.cr, "CR 704.5x");
    assert_eq!(player_candidates(&asked[0]), [p(2), p(3)]);

    // 704.5x before the batch, 704.5y in the post-batch view.
    let protected_by_self = move |pre: &StoredSbaEnvironment, _: &EventContext<'_>| {
        let mut view = pre.clone();
        view.battle_protectors.insert(x, p(1));
        view
    };
    let (run, asked) = drive_post_batch(&w, &[submit_player(3)], &protected_by_self);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].reason.cr, "CR 704.5y");
    assert_eq!(player_candidates(&asked[0]), [p(2), p(3)]);
    assert_eq!(run.out.unwrap().protectors_chosen, [(x, p(3))]);
}

// REQ-SBA-001, CR 704.5v, CR 704.5x, CR 614.6, CR 704.3 (gate 02-state-objects R3c,
// thread 4698236625): an unattacked Siege at defense 0 with no protector whose graveyard move a
// replacement skips, and whose controller loses the game in the same pass: no request names the
// lost controller (none is made, no request id consumed, nothing pending), the pass succeeds
// with the loss, the Siege gets no protector and the next pass finds it again without a
// request. Control: the same Siege whose controller stays in the game is asked.
#[test]
fn sba_704_5x_retained_battle_of_a_lost_controller_asks_nothing() {
    let mut w = World::new();
    let s = w.new_battle(1, 1, true, 0, None);
    w.skip_sba_move(1, s);

    // Control: the controller stays in the game and is asked.
    let (run, asked) = drive_answers(&w, &[submit_player(2)]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].chooser, p(1));
    assert_eq!(asked[0].reason.cr, "CR 704.5x");
    assert_eq!(player_candidates(&asked[0]), [p(2)]);
    assert_eq!(run.out.unwrap().protectors_chosen, [(s, p(2))]);

    w.set_life(1, 0);
    let (run, asked) = drive_answers(&w, &[]);
    // No request at all: nothing pending, no request id consumed.
    assert!(asked.is_empty());
    assert_eq!(run.asked, 0);
    let out = run.out.unwrap();
    assert_eq!(out.lost_players, [p(1)]);
    assert!(out.protectors_chosen.is_empty());
    assert!(out.performed);
    let mut w = run.state;
    assert!(w.lost(1));
    assert!(!w.env.battle_protectors.contains_key(&s));
    assert_eq!(w.zones.object(s).unwrap().zone, ZoneId::Battlefield);
    // The next pass finds it again and still asks nothing.
    let out = pass(&mut w);
    assert!(out.protectors_chosen.is_empty());
    assert!(
        out.findings
            .iter()
            .any(|f| f.subject == obj(s) && f.entry_id == "704.5x")
    );

    // The same through `perform_all_post_batch`.
    let mut w = World::new();
    let s = w.new_battle(1, 1, true, 0, None);
    w.skip_sba_move(1, s);
    w.set_life(1, 0);
    let same = |pre: &StoredSbaEnvironment, _: &EventContext<'_>| pre.clone();
    let (run, asked) = drive_post_batch(&w, &[], &same);
    assert!(asked.is_empty());
    assert!(run.out.unwrap().protectors_chosen.is_empty());
}

// REQ-SBA-001, CR 704.5v, CR 704.5x, CR 800.4a, CR 614.6, CR 704.3 (gate 02-state-objects R3d,
// thread 4699044484): the ORDINARY (pre-batch) protector request makes the same live-controller
// check as the post-batch one. Three players. Pass 1: an unattacked Siege of player 2 at
// defense 0 with no protector, whose graveyard move a replacement turns into one defense
// counter, while player 2 loses the game: player 2 is not asked (the post-batch check). Pass 2:
// the Siege is at defense 1, a CR 704.5x subject on the ordinary path, and its controller has
// lost the game (CR 800.4a: it has left the game and can't answer): no request, no request id
// consumed, nothing pending, and the Siege neither gets a protector nor moves. Control: the same
// Siege whose controller stays in the game is asked in pass 1 with no ordinary request in pass 2.
#[test]
fn sba_704_5x_ordinary_request_skips_a_lost_controller() {
    fn world(lose: bool) -> (World, ObjectId) {
        let mut w = World::three([1, 2, 3], 1);
        let s = w.new_battle(2, 2, true, 0, None);
        w.registry
            .add(ReplacementEffect::new(
                e(10),
                sba_move_of(s),
                Action::Instead(vec![GameEvent::AddCounters {
                    target: CounterTarget::Object(s),
                    kind: "defense".to_owned(),
                    amount: 1,
                }]),
            ))
            .unwrap();
        if lose {
            w.set_life(2, 0);
        }
        (w, s)
    }

    // Control: player 2 stays in the game and is asked after the batch of pass 1.
    let (w, s) = world(false);
    let (run, asked) = drive_answers(&w, &[submit_player(3)]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].chooser, p(2));
    assert_eq!(asked[0].reason.cr, "CR 704.5x");
    assert_eq!(player_candidates(&asked[0]), [p(1), p(3)]);
    assert_eq!(run.out.unwrap().protectors_chosen, [(s, p(3))]);
    let mut w = run.state;
    assert_eq!(w.counter(s, "defense"), 1);
    assert!(pass(&mut w).protectors_chosen.is_empty());

    // Pass 1: player 2 loses; the retained Siege gets a defense counter and no request.
    let (w, s) = world(true);
    let (run, asked) = drive_answers(&w, &[]);
    assert!(asked.is_empty());
    let out = run.out.unwrap();
    assert_eq!(out.lost_players, [p(2)]);
    assert!(out.protectors_chosen.is_empty());
    let mut w = run.state;
    assert!(w.lost(2));
    assert_eq!(w.counter(s, "defense"), 1);
    assert_eq!(w.found_by("704.5x"), [s]);
    assert!(w.found_by("704.5v").is_empty());

    // Pass 2: the ordinary path asks nothing (`pass` asserts that no request was made) and the
    // Siege stays on the battlefield without a protector.
    let (run, asked) = drive_answers(&w, &[]);
    assert!(asked.is_empty());
    assert_eq!(run.asked, 0);
    let out = run.out.unwrap();
    assert!(out.protectors_chosen.is_empty());
    assert!(out.executed.is_empty());
    let out = pass(&mut w);
    assert!(out.protectors_chosen.is_empty());
    assert!(
        out.findings
            .iter()
            .any(|f| f.subject == obj(s) && f.entry_id == "704.5x")
    );
    assert_eq!(w.zones.object(s).unwrap().zone, ZoneId::Battlefield);
    assert!(!w.env.battle_protectors.contains_key(&s));
}

// REQ-SBA-001, CR 704.5x, CR 800.4a, CR 101.4 (gate 02-state-objects R3d, thread 4699044484):
// on the ordinary path a battle whose controller has lost the game (here the active player 1,
// first in APNAP order) or whose controller is not a player of the game gets no request and
// consumes no request id: the battle of the live player 3 is the only request, with the SAME
// request id, chooser, reason and candidates as in a world without the other battles, and the
// skipped battles stay on the battlefield without a protector. A typeless battle of a lost
// controller has no candidate and still goes to its owner's graveyard without a request.
#[test]
fn sba_704_5x_ordinary_request_ids_unchanged_by_a_lost_controller() {
    // Control: only the live player's Siege (player 1 has lost the game here too, so the
    // candidates are the same).
    let mut control = World::three([1, 2, 3], 1);
    let live = control.new_battle(3, 3, true, 3, None);
    control.lose(1);
    let (run, asked) = drive_answers(&control, &[submit_player(2)]);
    assert_eq!(asked.len(), 1);
    let expected = asked[0].clone();
    assert_eq!(expected.chooser, p(3));
    assert_eq!(player_candidates(&expected), [p(2)]);
    assert_eq!(run.out.unwrap().protectors_chosen, [(live, p(2))]);

    let mut w = World::three([1, 2, 3], 1);
    let siege = w.new_battle(1, 1, true, 3, None);
    let typeless = w.new_battle(1, 1, false, 3, None);
    // `gone` is controlled (layer 2, CR 613.1b) by a player who is not in the game.
    let gone = w.new_battle(1, 1, true, 3, None);
    w.give_control(20, gone, 9);
    let live = w.new_battle(3, 3, true, 3, None);
    w.lose(1);
    assert_eq!(w.found_by("704.5x"), [siege, typeless, gone, live]);
    let (run, asked) = drive_answers(&w, &[submit_player(2)]);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].request_id, expected.request_id);
    assert_eq!(asked[0].chooser, expected.chooser);
    assert_eq!(asked[0].reason.cr, expected.reason.cr);
    assert_eq!(asked[0].payload, expected.payload);
    assert_eq!(asked[0].reason.source, Some(live));
    let out = run.out.unwrap();
    assert_eq!(out.protectors_chosen, [(live, p(2))]);
    // The typeless battle has no candidate (its only appropriate player, its controller, left
    // the game): it goes to its owner's graveyard without a request, as before.
    assert_eq!(
        batch_shape(&out).0,
        [(typeless, ZoneChangeCause::StateBasedAction)]
    );
    let w = run.state;
    for b in [siege, gone] {
        assert_eq!(w.zones.object(b).unwrap().zone, ZoneId::Battlefield);
        assert!(!w.env.battle_protectors.contains_key(&b));
    }
}

// REQ-SBA-001, CR 704.5z, CR 303.7a, CR 613.7e: two Roles of player 1 on one creature: the older
// goes to its OWNER's graveyard; one Role each of players 1 and 2 on the same creature do not
// interact; a non-Role Aura is unaffected; a Role controlled by player 1 but owned by player 2
// groups with player 1's Roles and goes to player 2's graveyard.
#[test]
fn sba_704_5z_role_rule() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let older = w.role_on(2, 1, host, 1);
    let newer = w.role_on(1, 1, host, 5);
    let other = w.role_on(2, 2, host, 0);
    let plain = w.create(1, ZoneKind::Battlefield, aura());
    w.attach(plain, AttachTarget::Permanent(host));
    w.env.attachment_since.insert(plain, 0);
    assert_eq!(w.found_by("704.5z"), [older]);
    let card = w.card_of(older);
    let out = pass(&mut w);
    assert_eq!(
        batch_shape(&out).0,
        [(older, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(2))));
    assert_eq!(
        on_battlefield(&w, &[newer, other, plain]),
        [newer, other, plain]
    );
    assert!(!pass(&mut w).performed);

    // Two controllers, two Roles each: each keeps its own most recent one.
    let mut w = World::new();
    let host = w.creature(1, 3);
    let a_new = w.role_on(1, 1, host, 7);
    let b_old = w.role_on(2, 2, host, 1);
    let a_old = w.role_on(1, 1, host, 2);
    let b_new = w.role_on(2, 2, host, 9);
    assert_eq!(w.found_by("704.5z"), [b_old, a_old]);
    pass(&mut w);
    assert_eq!(
        on_battlefield(&w, &[a_new, b_old, a_old, b_new]),
        [a_new, b_new]
    );
    // Roles on different permanents do not interact.
    let mut w = World::new();
    let h1 = w.creature(1, 3);
    let h2 = w.creature(1, 3);
    w.role_on(1, 1, h1, 1);
    w.role_on(1, 1, h2, 2);
    assert!(w.check().is_empty());
}

// REQ-SBA-001, CR 704.5z, CR 613.7m: a tie of the latest timestamp keeps the HIGHER ObjectId
// (decision #43 FP-042), whatever the creation order of the stamps; three Roles with the two
// latest tied: both lower ones go.
#[test]
fn sba_704_5z_tie_keeps_the_higher_id() {
    for stamps in [[3, 3, 1], [1, 3, 3], [3, 1, 3]] {
        let mut w = World::new();
        let host = w.creature(1, 3);
        let ids: Vec<ObjectId> = stamps.iter().map(|&s| w.role_on(1, 1, host, s)).collect();
        let mut gone = ids.clone();
        let keep = *ids
            .iter()
            .zip(stamps)
            .max_by_key(|&(&id, s)| (s, id))
            .unwrap()
            .0;
        gone.retain(|&id| id != keep);
        assert_eq!(w.found_by("704.5z"), gone, "{stamps:?}");
        pass(&mut w);
        assert_eq!(on_battlefield(&w, &ids), [keep], "{stamps:?}");
    }
}

// REQ-SBA-001, CR 704.5z, CR 704.5d, CR 111.7: a token Role that loses goes to its owner's
// graveyard, then ceases to exist in the next pass (CR 704.5d).
#[test]
fn sba_704_5z_token_role_ceases_next_pass() {
    let mut w = World::new();
    let host = w.creature(1, 3);
    let card = w.card(role());
    w.setup(GameEvent::CreateToken {
        owner: p(1),
        card,
        controller: None,
    });
    let token = w.zones.object_of_card(card).unwrap();
    w.attach(token, AttachTarget::Permanent(host));
    w.env.attachment_since.insert(token, 1);
    let newer = w.role_on(1, 1, host, 2);
    assert_eq!(w.found_by("704.5z"), [token]);
    let out = pass(&mut w);
    assert_eq!(
        batch_shape(&out).0,
        [(token, ZoneChangeCause::StateBasedAction)]
    );
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    let out = pass(&mut w);
    assert_eq!(out.ceased.len(), 1);
    assert_eq!(w.zone_of_card(card), None);
    assert!(w.zones.object(newer).is_some());
}

// REQ-SBA-001, CR 704.5aa, CR 702.179a, CR 702.179b: a player with no speed who controls two
// permanents with start your engines! gets speed 1 once (one finding, `speed_set`, no event);
// the repeated check finds nothing; a player with speed 3 keeps it.
#[test]
fn sba_704_5aa_speed_becomes_one() {
    let mut w = World::new();
    w.create(1, ZoneKind::Battlefield, engine_creature());
    w.create(1, ZoneKind::Battlefield, engine_creature());
    w.create(2, ZoneKind::Battlefield, engine_creature());
    w.set_speed(2, 3);
    assert_eq!(w.players_found_by("704.5aa"), [p(1)]);
    assert_eq!(w.speed(1), None);
    let out = pass(&mut w);
    assert!(out.performed);
    assert!(out.executed.is_empty());
    assert_eq!(out.speed_set, [p(1)]);
    assert_eq!(w.speed(1), Some(DesignationValue::Int(1)));
    assert_eq!(w.speed(2), Some(DesignationValue::Int(3)));
    assert!(w.check().is_empty());
    assert!(!pass(&mut w).performed);
    // A player with no engine gets no speed; a lost player gets none either.
    let mut w = World::new();
    w.creature(1, 2);
    w.create(2, ZoneKind::Battlefield, engine_creature());
    w.lose(2);
    assert!(w.players_found_by("704.5aa").is_empty());
}

// REQ-SBA-001, CR 704.5aa, CR 613.1, CR 613.4: a start your engines! ability granted by a layer-6
// effect counts; the controller after layer 2 (CR 613.1b) is the one whose speed is set.
#[test]
fn sba_704_5aa_layer_granted_engine_counts() {
    let mut w = World::new();
    let c = w.creature(1, 2);
    assert!(w.check().is_empty());
    w.effects
        .register(
            e(1),
            ContinuousEffect {
                source: None,
                controller: p(1),
                timestamp: Timestamp::new(1),
                layer: Layer::L6,
                is_cda: false,
                origin: EffectOrigin::Resolved,
                affected: Affected::Objects(BTreeSet::from([c])),
                modification: LayerModification::AddAbility(keyword("START_YOUR_ENGINES")),
            },
        )
        .unwrap();
    assert_eq!(w.players_found_by("704.5aa"), [p(1)]);
    w.give_control(2, c, 2);
    assert_eq!(w.players_found_by("704.5aa"), [p(2)]);
    let out = pass(&mut w);
    assert_eq!(out.speed_set, [p(2)]);
    assert_eq!(w.speed(1), None);
}

// REQ-SBA-001, CR 702.26b: a phased-out battle (defense 0, no protector), a phased-out Role and a
// phased-out engine permanent are ignored by CR 704.5v-704.5aa.
#[test]
fn sba_704_5v_to_aa_skip_phased_out() {
    let mut w = World::new();
    let s = w.new_battle(1, 1, true, 0, None);
    let b = w.new_battle(1, 1, false, 0, Some(2));
    let host = w.creature(1, 3);
    w.role_on(1, 1, host, 2);
    let old = w.role_on(1, 1, host, 1);
    let engine = w.create(1, ZoneKind::Battlefield, engine_creature());
    for id in [s, b, old, engine] {
        w.phase(id, true);
    }
    assert!(w.check().is_empty());
    assert!(!pass(&mut w).performed);
    w.phase(engine, false);
    assert_eq!(w.players_found_by("704.5aa"), [p(1)]);
}

/// A board with findings of CR 704.5a, 704.5f, 704.5j and every one of CR 704.5v-704.5aa,
/// created in ascending or descending order. Returns the world and the named objects:
/// [legend 1, legend 2, siege, non-siege, unprotected, wrong, older role, newer role, creature].
fn simultaneity_board(reverse: bool) -> (World, [ObjectId; 9]) {
    let mut w = World::new();
    w.set_life(2, 0);
    let mut ids = [ObjectId::new(0); 9];
    let mut steps: Vec<usize> = (0..9).collect();
    if reverse {
        steps.reverse();
    }
    let host = w.creature(1, 3);
    for i in steps {
        ids[i] = match i {
            0 | 1 => w.create_controlled(1, 1, legendary(&["test.legend_a"])),
            2 => w.new_battle(2, 1, true, 0, Some(2)),
            3 => w.new_battle(2, 1, false, 0, Some(1)),
            4 => w.new_battle(1, 1, false, 3, None),
            5 => w.new_battle(1, 1, true, 3, Some(1)),
            6 => w.role_on(1, 1, host, 1),
            7 => w.role_on(1, 1, host, 2),
            _ => w.creature(1, 0),
        };
    }
    w.create(1, ZoneKind::Battlefield, engine_creature());
    (w, ids)
}

// REQ-SBA-001, REQ-TEST-001, CR 704.3, CR 704.5a, CR 704.5f, CR 704.5j, CR 704.5v-704.5aa: all
// found on one snapshot and performed in ONE pass with ONE batch, in both object-id orders; the
// legend request comes first, then the protector requests (CR 101.4: every request is player
// 1's, so within that player legend groups first, then ascending battle id); nothing remains for the next pass.
#[test]
fn sba_704_5v_to_aa_simultaneous_with_old_entries() {
    for reverse in [false, true] {
        let (
            w,
            [
                l1,
                l2,
                siege,
                nonsiege,
                unprotected,
                wrong,
                older,
                newer,
                dead,
            ],
        ) = simultaneity_board(reverse);
        let mut probe = w.clone();
        let entries: BTreeSet<&str> = probe.check().into_iter().map(|f| f.entry_id).collect();
        let expected: BTreeSet<&str> = [
            "704.5a", "704.5f", "704.5j", "704.5v", "704.5w", "704.5x", "704.5y", "704.5z",
            "704.5aa",
        ]
        .into_iter()
        .collect();
        assert_eq!(entries, expected, "reverse {reverse}");
        // The typeless battle offers only its controller (1), the Siege only the opponent (2).
        let x_first = unprotected < wrong;
        let answers = if x_first {
            [select(&[l2]), submit_player(1), submit_player(2)]
        } else {
            [select(&[l2]), submit_player(2), submit_player(1)]
        };
        let (run, asked) = drive_answers(&w, &answers);
        assert_eq!(asked[0].reason.cr, "CR 704.5j");
        let protector_order: Vec<&str> = asked[1..].iter().map(|r| r.reason.cr.as_str()).collect();
        let expected_order = if x_first {
            ["CR 704.5x", "CR 704.5y"]
        } else {
            ["CR 704.5y", "CR 704.5x"]
        };
        assert_eq!(protector_order, expected_order, "reverse {reverse}");
        let out = run.out.unwrap();
        assert!(out.performed);
        assert_eq!(out.executed.len(), 1);
        let mut moved: Vec<ObjectId> = batch_shape(&out).0.into_iter().map(|(o, _)| o).collect();
        moved.sort();
        let mut want = vec![l1, siege, nonsiege, older, dead];
        want.sort();
        assert_eq!(moved, want, "reverse {reverse}");
        assert_eq!(out.lost_players, [p(2)]);
        assert_eq!(out.speed_set, [p(1)]);
        let mut chosen = vec![(unprotected, p(1)), (wrong, p(2))];
        chosen.sort();
        assert_eq!(out.protectors_chosen, chosen);
        assert!(run.state.zones.object(newer).is_some());
        // Player 2 lost in this pass, so the Siege's new protector is no longer in the game:
        // the repeated check (CR 704.3) finds 704.5x for it, and nothing else.
        let mut w = run.state;
        assert_eq!(
            w.check(),
            [SbaFinding {
                entry_id: "704.5x",
                subject: obj(wrong)
            }],
            "reverse {reverse}"
        );
    }
}

// REQ-SBA-001, REQ-CORE-001, CR 704.5v-704.5aa: the same input gives the same outcome, requests
// and state hash twice.
#[test]
fn sba_704_5v_to_aa_deterministic() {
    let (w, _) = simultaneity_board(true);
    let (a, asked_a) = drive_defaults(&w);
    let (b, asked_b) = drive_defaults(&w);
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.out, b.out);
    assert_eq!(asked_a, asked_b);
    assert_eq!(a.state.env, b.state.env);
}

const FP042_ENTRIES: [&str; 6] = ["704.5v", "704.5w", "704.5x", "704.5y", "704.5z", "704.5aa"];

// REQ-SBA-001, REQ-CORE-001, CR 704.3, CR 704.5v-704.5aa (property): for seeded random boards of
// battles (Siege or typeless, defense 0-2, protector none / either player / absent, pending
// abilities, forbidden protectors, attacked or not), Roles (random controllers, hosts and
// stamps) and engine permanents: `check` is pure (twice the same, state unchanged); findings are
// in registry order, then subject order; after one pass answered with the defaults, the
// repeated check finds no CR 704.5v-704.5aa finding.
#[test]
fn property_fp042_entries() {
    let registry = SbaRegistry::standard();
    let rank = |id: &str| registry.entries().iter().position(|e| e.id == id).unwrap();
    let mut rng = Lcg(43);
    for round in 0..150 {
        let mut w = World::new();
        let attacker = w.creature(2, 5);
        let mut attacked = false;
        for _ in 0..rng.next(4) {
            let controller = rng.next(2) + 1;
            let protector = match rng.next(4) {
                0 => None,
                1 => Some(1),
                2 => Some(2),
                _ => Some(7),
            };
            let defense = i64::try_from(rng.next(3)).unwrap();
            let b = w.new_battle(
                rng.next(2) + 1,
                controller,
                rng.next(2) == 0,
                defense,
                protector,
            );
            if rng.next(4) == 0 {
                w.env.battle_pending.insert(b);
            }
            if rng.next(4) == 0 {
                w.env.illegal_protectors.insert((b, p(rng.next(2) + 1)));
            }
            if !attacked && rng.next(4) == 0 {
                w.attack_battle(attacker, b);
                attacked = true;
            }
        }
        let hosts = [w.creature(1, 4), w.creature(2, 4)];
        for _ in 0..rng.next(5) {
            let host = hosts[rng.index(2)];
            w.role_on(rng.next(2) + 1, rng.next(2) + 1, host, rng.next(3));
        }
        for _ in 0..rng.next(3) {
            w.create(rng.next(2) + 1, ZoneKind::Battlefield, engine_creature());
        }
        if rng.next(3) == 0 {
            w.set_speed(2, 2);
        }
        let before = w.state_hash();
        let first = w.check();
        assert_eq!(w.check(), first, "round {round}");
        assert_eq!(w.state_hash(), before, "round {round}");
        let keys: Vec<(usize, SbaSubject)> = first
            .iter()
            .map(|f| (rank(f.entry_id), f.subject))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "round {round}");
        let (run, _) = drive_defaults(&w);
        run.out.expect("the pass failed");
        let mut after = run.state;
        let left: Vec<SbaFinding> = after
            .check()
            .into_iter()
            .filter(|f| FP042_ENTRIES.contains(&f.entry_id))
            .collect();
        assert!(left.is_empty(), "round {round}: {left:?}");
    }
}

// REQ-SBA-001, CR 704.5v-704.5z: the defaults of the new seams (no protector, every player
// allowed, nothing pending, stamp 0, recording does nothing) and the stored environment's
// answers.
#[test]
fn fp042_environment_defaults() {
    struct Plain;
    impl SbaEnvironment for Plain {
        fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
            BTreeSet::new()
        }
        fn clear_deathtouch_damaged(&mut self) {}
    }
    let o = ObjectId::new(3);
    let mut plain = Plain;
    assert_eq!(plain.battle_protector(o), None);
    assert!(plain.can_be_protector(o, p(1)));
    assert!(!plain.battle_ability_pending(o));
    assert_eq!(plain.attachment_timestamp(o), 0);
    plain.set_battle_protector(o, p(2));
    assert_eq!(plain.battle_protector(o), None);
    let mut stored = StoredSbaEnvironment::default();
    assert_eq!(stored.battle_protector(o), None);
    assert!(stored.can_be_protector(o, p(1)));
    assert!(!stored.battle_ability_pending(o));
    assert_eq!(stored.attachment_timestamp(o), 0);
    stored.set_battle_protector(o, p(2));
    stored.illegal_protectors.insert((o, p(1)));
    stored.battle_pending.insert(o);
    stored.attachment_since.insert(o, 8);
    assert_eq!(stored.battle_protector(o), Some(p(2)));
    assert!(!stored.can_be_protector(o, p(1)));
    assert!(stored.can_be_protector(o, p(2)));
    assert!(stored.battle_ability_pending(o));
    assert_eq!(stored.attachment_timestamp(o), 8);
    assert_eq!(SPEED, "speed");
}

// ---------------------------------------------------------------------------------------------
// The state-based destruction cause (#209 FP-155, CR 701.8b)
// ---------------------------------------------------------------------------------------------

/// The `Destroyed` trigger kind of each executed zone change of `out`, by pre-event object id.
fn destroyed_kinds(out: &SbaOutcome) -> BTreeMap<ObjectId, bool> {
    out.executed
        .iter()
        .flat_map(crate::triggers::events_from_batch)
        .filter_map(|e| {
            e.subject_pre.map(|o| {
                (
                    o,
                    e.kinds
                        .contains(&fineprint_dsl::ability::EventKind::Destroyed),
                )
            })
        })
        .collect()
}

// REQ-SBA-001, REQ-EVENT-001, CR 701.8b, CR 704.5g, CR 704.5h, CR 704.5f, CR 704.5i: a
// creature with lethal damage (704.5g) and one with deathtouch damage (704.5h) move with cause
// `StateBasedDestroy` and their trigger events carry `Destroyed`; a 0-toughness creature
// (704.5f) and a 0-loyalty planeswalker (704.5i) move with `StateBasedAction` and carry no
// `Destroyed`. All four moves are in the one SBA batch.
#[test]
fn req_sba_001_cr_701_8b_destroy_findings_move_as_state_based_destroy() {
    let mut w = World::new();
    let lethal = w.creature(1, 2);
    w.damage(lethal, 2);
    let touched = w.creature(1, 4);
    w.damage(touched, 1);
    w.env.deathtouch = BTreeSet::from([touched]);
    let zero = w.creature(2, 0);
    let walker = w.create(
        2,
        ZoneKind::Battlefield,
        noncreature("test.walker_0", "Planeswalker"),
    );
    assert_eq!(w.found(obj(lethal)), ["704.5g"]);
    assert_eq!(w.found(obj(touched)), ["704.5h"]);
    assert_eq!(w.found(obj(zero)), ["704.5f"]);
    assert_eq!(w.found(obj(walker)), ["704.5i"]);
    let out = pass(&mut w);
    assert_eq!(out.executed.len(), 1);
    assert_eq!(
        batch_shape(&out).0,
        [
            (lethal, ZoneChangeCause::StateBasedDestroy),
            (touched, ZoneChangeCause::StateBasedDestroy),
            (zero, ZoneChangeCause::StateBasedAction),
            (walker, ZoneChangeCause::StateBasedAction),
        ]
    );
    assert_eq!(
        destroyed_kinds(&out),
        BTreeMap::from([
            (lethal, true),
            (touched, true),
            (zero, false),
            (walker, false)
        ])
    );
    for o in [lethal, touched, zero, walker] {
        assert!(w.zones.object(o).is_none());
    }
}

// REQ-SBA-001, CR 704.7, CR 701.8b, CR 704.5g, CR 704.5h: a creature with both a lethal-damage
// and a deathtouch finding moves once, cause `StateBasedDestroy`; both findings are reported.
#[test]
fn req_sba_001_cr_704_7_two_destroy_findings_one_state_based_destroy_move() {
    let mut w = World::new();
    let c = w.creature(1, 2);
    w.damage(c, 2);
    w.env.deathtouch = BTreeSet::from([c]);
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
    assert_eq!(
        batch_shape(&out).0,
        [(c, ZoneChangeCause::StateBasedDestroy)]
    );
}

// REQ-SBA-001, CR 704.7, CR 704.5j, CR 704.5g, CR 701.8b: a legendary creature that loses the
// legend rule (the legend-rule pre-insert, cause `StateBasedAction`) and also has lethal damage
// moves once, cause `StateBasedDestroy`, whichever legend is kept; the kept one (undamaged)
// stays.
#[test]
fn req_sba_001_cr_704_7_legend_loser_with_lethal_damage_is_destroyed() {
    let mut legend = legendary(&["test.legend_a"]);
    legend.card_types = set(&["Artifact", "Creature"]);
    legend.power = Some(literal(1));
    legend.toughness = Some(literal(2));
    let mut w = World::new();
    let a = w.create_controlled(1, 1, legend.clone());
    let b = w.create_controlled(1, 1, legend);
    w.damage(a, 2);
    let mut found = w.found(obj(a));
    found.sort_unstable();
    assert_eq!(found, ["704.5g", "704.5j"]);
    // Keep b: a is both a legend-rule loser and destroyed.
    let (run, asked) = drive_answers(&w, &[select(&[b])]);
    assert_eq!(asked.len(), 1);
    let out = run.out.unwrap();
    assert_eq!(
        batch_shape(&out).0,
        [(a, ZoneChangeCause::StateBasedDestroy)]
    );
    assert_eq!(destroyed_kinds(&out), BTreeMap::from([(a, true)]));
    // Keep a: a is destroyed (704.5g), b is a plain legend-rule loser.
    let (run, _) = drive_answers(&w, &[select(&[a])]);
    let out = run.out.unwrap();
    assert_eq!(
        batch_shape(&out).0,
        [
            (a, ZoneChangeCause::StateBasedDestroy),
            (b, ZoneChangeCause::StateBasedAction)
        ]
    );
}

// REQ-SBA-001, CR 704.7, CR 704.5x, CR 704.5g, CR 701.8b: a battle creature no player can
// protect (the unprotectable pre-insert, cause `StateBasedAction`) that also has lethal damage
// moves once, cause `StateBasedDestroy`.
#[test]
fn req_sba_001_cr_704_7_unprotectable_battle_with_lethal_damage_is_destroyed() {
    let mut c = battle(false);
    c.card_types = set(&["Battle", "Creature"]);
    c.power = Some(literal(1));
    c.toughness = Some(literal(2));
    let mut w = World::new();
    let b = w.create_controlled(2, 1, c);
    w.counters(b, "defense", 3);
    w.env.illegal_protectors.insert((b, p(1)));
    w.damage(b, 2);
    let mut found = w.found(obj(b));
    found.sort_unstable();
    assert_eq!(found, ["704.5g", "704.5x"]);
    let card = w.card_of(b);
    let out = pass(&mut w);
    assert_eq!(
        batch_shape(&out).0,
        [(b, ZoneChangeCause::StateBasedDestroy)]
    );
    assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(2))));
}

// REQ-SBA-001, CR 704.7, CR 704.5s, CR 704.5g: a Saga creature that is sacrificed (704.5s) and
// also has lethal damage (704.5g) moves once with the Sacrifice cause (decision #42 FP-041;
// decision "#209 FP-155 - SBA destruction cause").
#[test]
fn req_sba_001_cr_704_7_sacrifice_wins_over_state_based_destroy() {
    let mut w = World::new();
    let mut c = with_subtypes(creature("test.saga_creature", 2), &["Saga"]);
    c.card_types = set(&["Creature", "Enchantment"]);
    let s = w.create(1, ZoneKind::Battlefield, c);
    w.counters(s, "lore", 1);
    w.env.saga_final_chapter = BTreeMap::from([(s, 1)]);
    w.damage(s, 2);
    w.env.deathtouch = BTreeSet::from([s]);
    assert_eq!(w.found(obj(s)), ["704.5g", "704.5h", "704.5s"]);
    let out = pass(&mut w);
    assert_eq!(out.findings.len(), 3);
    assert_eq!(batch_shape(&out).0, [(s, ZoneChangeCause::Sacrifice)]);
}

// REQ-SBA-001, REQ-EVENT-001, CR 122.1c, CR 701.8b, CR 704.5g, CR 704.5h: a shield counter does
// not stop a lethal-damage or a deathtouch death: the shield counter's destroy replacement
// matches the effect cause `Destroy` only; it is never applied and no counter is removed.
#[test]
fn req_sba_001_cr_122_1c_shield_counter_does_not_stop_5g_or_5h() {
    for deathtouch in [false, true] {
        let mut w = World::new();
        let c = w.creature(1, 4);
        w.counters(c, "shield", 1);
        if deathtouch {
            w.damage(c, 1);
            w.env.deathtouch = BTreeSet::from([c]);
        } else {
            w.damage(c, 4);
        }
        w.registry
            .add(ReplacementEffect::shield_counter_destroy(e(1), c))
            .unwrap();
        let registry = w.registry.clone();
        let card = w.card_of(c);
        let out = pass(&mut w);
        assert_eq!(
            out.findings[0].entry_id,
            if deathtouch { "704.5h" } else { "704.5g" }
        );
        assert_eq!(
            batch_shape(&out),
            (vec![(c, ZoneChangeCause::StateBasedDestroy)], vec![])
        );
        assert_eq!(w.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
        // The replacement was never applied: still registered, unchanged.
        assert_eq!(w.registry, registry);
        // The LKI shows the shield counter still on it as it died.
        assert_eq!(w.lki.get(c).unwrap().counters().get("shield"), Some(&1));
    }
}

// ---------------------------------------------------------------------------------------------
// Gate 06-turn F6/F13: the `Prevent` follow-up batches of the SBA batch
// ---------------------------------------------------------------------------------------------

/// `base` plus a static layer-2 effect of `source` giving `target` to player 2 (CR 613.1b),
/// registered only while `source` is on the battlefield of `zones` (the effect exists only
/// while its source does).
fn control_while_on_battlefield(
    base: &ContinuousEffects,
    zones: &Zones,
    source: ObjectId,
    target: ObjectId,
) -> ContinuousEffects {
    let mut effects = base.clone();
    if zones
        .object(source)
        .is_some_and(|r| r.zone == ZoneId::Battlefield)
    {
        effects
            .register(
                e(91),
                ContinuousEffect {
                    source: Some(source),
                    controller: p(2),
                    timestamp: Timestamp::new(91),
                    layer: Layer::L2,
                    is_cda: false,
                    origin: EffectOrigin::Static { granted_at: None },
                    affected: Affected::Objects(BTreeSet::from([target])),
                    modification: LayerModification::SetController(p(2)),
                },
            )
            .unwrap();
    }
    effects
}

/// The F13 world of the SBA pass: player 1's 1/1 `t` and player 2's 0-toughness creature `s`
/// (CR 704.5f), the source of a static effect giving `t` to player 2. The SBA move of `s` is
/// replaced by `Instead([s to graveyard, 1 damage to player 1])` (CR 614.6); that damage is
/// prevented and queues `Tap(t)` as a follow-up batch; two `Skip` effects compete for the tap
/// (CR 616.1). Returns (world, t, s).
fn f13_sba_world() -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let t = w.creature(1, 1);
    let s = w.creature(2, 0);
    let damage = GameEvent::Damage {
        source: None,
        source_controller: None,
        target: DamageTarget::Player(p(1)),
        amount: 1,
        combat: false,
    };
    w.registry
        .add(ReplacementEffect::new(
            e(10),
            sba_move_of(s),
            Action::Instead(vec![
                zone_change(s, ZoneKind::Graveyard, ZoneChangeCause::Other),
                damage,
            ]),
        ))
        .unwrap();
    w.registry
        .add(ReplacementEffect::new(
            e(11),
            EventMatcher::new(BusEventKind::Damage),
            Action::Prevent {
                then: vec![GameEvent::Tap { object: t }],
            },
        ))
        .unwrap();
    for id in [12, 13] {
        w.registry
            .add(ReplacementEffect::new(
                e(id),
                EventMatcher::new(BusEventKind::Tap),
                Action::Skip,
            ))
            .unwrap();
    }
    (w, t, s)
}

/// One pass over the F13 world: the context's provider is a `LayeredProvider` over the state
/// before the pass; through `perform_all_relayered` (whose post-batch provider describes the
/// live state it is given, counting its calls in `calls`) when `relayered`, else through
/// `perform_all`. Pending and error handling as [`run_pass`].
fn run_f13_pass(
    w: &mut World,
    ch: &mut ChoiceContext<'_>,
    (t, s): (ObjectId, ObjectId),
    relayered: bool,
    calls: &std::cell::Cell<usize>,
) -> Result<Out, Interrupt> {
    let before = w.state_hash();
    let snap = w.snapshot();
    let pre_effects = control_while_on_battlefield(&w.effects, &w.zones, s, t);
    let root = crate::layers::LayeredProvider::new(&pre_effects, &snap);
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
        chars,
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
        provider: &root,
    };
    let result = if relayered {
        let mut provider =
            |after: &EventContext<'_>,
             view: &mut dyn FnMut(&ContinuousEffects, &dyn LayerEnvironment)| {
                calls.set(calls.get() + 1);
                let fresh = snapshot_of(after.zones, after.events, chars);
                let fresh_effects = control_while_on_battlefield(effects, after.zones, s, t);
                view(&fresh_effects, &fresh);
            };
        perform_all_relayered(
            &registry,
            &mut ctx,
            replacements,
            &pre_effects,
            &snap,
            env,
            ch,
            &mut provider,
        )
    } else {
        perform_all(
            &registry,
            &mut ctx,
            replacements,
            &pre_effects,
            &snap,
            env,
            ch,
        )
    };
    settle(w, before, result)
}

/// Runs one F13 pass on a clone of `w` in a choice session, answering every request with
/// replacement index 0; returns the run, every request asked and the post-batch provider calls
/// of each run of the procedure (the start, then one per answer).
fn drive_f13(
    w: &World,
    objects: (ObjectId, ObjectId),
    relayered: bool,
) -> (Run, Vec<PendingChoiceRequest>, Vec<usize>) {
    let calls = std::cell::Cell::new(0);
    let mut session =
        ChoiceSession::start(w.clone(), |w: &mut World, ch: &mut ChoiceContext<'_>| {
            run_f13_pass(w, ch, objects, relayered, &calls)
        })
        .unwrap();
    let mut per_run = vec![calls.replace(0)];
    let mut asked = Vec::new();
    loop {
        let pending = match session.status() {
            Status::Done { state, output } => {
                let run = Run {
                    asked: asked.len(),
                    state: state.clone(),
                    out: output.clone(),
                    hash: session.state_hash(),
                };
                return (run, asked, per_run);
            }
            Status::Pending(r) => r.clone(),
        };
        assert!(asked.len() < 4, "too many requests");
        session
            .submit(
                pending.request_id,
                ChoiceResponse::SelectReplacement { index: 0 },
            )
            .unwrap();
        per_run.push(calls.replace(0));
        asked.push(pending);
    }
}

// REQ-SBA-001, REQ-EVENT-001, REQ-LAYER-001, REQ-CORE-001; CR 704.3, CR 704.5f, CR 614.6,
// CR 613.1b, CR 616.1 (gate 06-turn F6/F13): the replacement choice of a `Prevent` follow-up
// batch of the SBA batch goes to the controller of the tapped creature as evaluated AFTER the
// SBA batch (player 1: the batch moved the source of the control effect) through
// `perform_all_relayered`, whose post-batch provider is called once per run for that follow-up
// batch; `perform_all`, which has no post-batch inputs, keeps the pre-pass provider and asks
// player 2. Both are deterministic (a second run asks the same request and ends in the same
// state).
#[test]
fn sba_f13_follow_up_chooser_is_controller_after_sba_batch() {
    let (w, t, s) = f13_sba_world();
    let card_s = w.card_of(s);
    // Sanity: before the pass the evaluated controller of `t` is player 2.
    let snap = w.snapshot();
    let pre = control_while_on_battlefield(&w.effects, &w.zones, s, t);
    assert_eq!(
        crate::layers::LayeredProvider::new(&pre, &snap).controller(t),
        Some(p(2))
    );

    let (run, asked, calls) = drive_f13(&w, (t, s), true);
    assert_eq!(asked.len(), 1);
    assert!(matches!(
        asked[0].payload,
        ChoicePayload::ChooseReplacementEffect { .. }
    ));
    assert_eq!(asked[0].reason.cr, "CR 616.1");
    assert_eq!(asked[0].chooser, p(1));
    assert_eq!(calls, [1, 1]);
    let out = run.out.as_ref().expect("the pass failed");
    assert!(out.performed);
    assert_eq!(
        run.state.zone_of_card(card_s),
        Some(ZoneId::Graveyard(p(2)))
    );
    assert!(!run.state.permanents.get(t).unwrap().status().tapped);

    let (again, asked_again, _) = drive_f13(&w, (t, s), true);
    assert_eq!(asked_again, asked);
    assert_eq!(again.hash, run.hash);
    assert_eq!(again.out, run.out);

    let (stale, asked_stale, calls) = drive_f13(&w, (t, s), false);
    assert_eq!(asked_stale.len(), 1);
    assert_eq!(asked_stale[0].chooser, p(2));
    assert_eq!(calls, [0, 0]);
    stale.out.expect("the pass failed");
}
