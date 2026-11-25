// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The data-driven state-based action registry (REQ-SBA-001, SRS 9): one entry per rule, each
//! with its id, CR reference, predicate, action and variant gate. It holds CR 704.5a-704.5t and
//! CR 704.5v-704.5aa; CR 704.5u is not modelled here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fineprint_base::ids::{ObjectId, PlayerId};

use super::types::{GameVariant, LOST_THE_GAME, SbaEnvironment, SbaSubject};
use crate::events::EventState;
use crate::layers::{ContinuousEffects, LayerEnvironment, evaluate, literal_of};
use crate::permanent::{AttachTarget, AttackState, AttackTarget, Permanents};
use crate::state::{GameState, PlayerState};
use crate::zones::{ZoneId, Zones};

/// What performing a state-based action does to its subject (REQ-SBA-001).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SbaAction {
    /// The player loses the game (CR 704.5a, CR 704.5b, CR 704.5c): recorded as the
    /// [`LOST_THE_GAME`] designation.
    LoseGame,
    /// The object ceases to exist (CR 704.5d, CR 704.5e): it leaves every zone without a zone
    /// change.
    CeaseToExist,
    /// The permanent is put into its owner's graveyard (CR 704.5f, CR 704.5i, CR 704.5k,
    /// CR 704.5m).
    PutIntoGraveyard,
    /// The permanent is destroyed (CR 704.5g, CR 704.5h, CR 701.8b): a move to its owner's
    /// graveyard. Regeneration (CR 701.8c) could replace it but is not modelled yet.
    Destroy,
    /// The legend rule (CR 704.5j): the controller of two or more legendary permanents with
    /// the same name chooses one of them; the rest are put into their owners' graveyards.
    LegendChoice,
    /// The permanent becomes unattached and remains on the battlefield (CR 704.5n,
    /// CR 704.5p).
    Unattach,
    /// Counters are removed from the permanent (CR 704.5q, CR 704.5r).
    RemoveCounters,
    /// The permanent's controller sacrifices it (CR 704.5s, CR 715.4): a move to its owner's
    /// graveyard with the sacrifice cause.
    Sacrifice,
    /// The dungeon card's owner removes it from the game (CR 704.5t, CR 309.7): it leaves the
    /// command zone without a zone change.
    RemoveFromGame,
    /// The battle's controller chooses an appropriate player to be its protector (CR 704.5x,
    /// CR 704.5y, CR 310.11); if no player can be chosen, the battle is put into its owner's
    /// graveyard.
    ChooseProtector,
    /// The player's speed becomes 1 (CR 704.5aa, CR 702.179a): recorded as the
    /// [`SPEED`](super::SPEED) designation.
    SetSpeed,
}

/// In which game variants an entry is active (REQ-SBA-001).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariantGate {
    /// Active in every variant.
    All,
    /// Active in every variant except this one.
    Except(GameVariant),
}

/// The facts the object entries read about one permanent, from its evaluated characteristics
/// (CR 613.1, CR 613.4: the values after all layers).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct PermanentFacts {
    creature: bool,
    planeswalker: bool,
    toughness: Option<i64>,
    indestructible: bool,
    loyalty: i64,
    /// CR 201.2a: the names (none for a face-down object, CR 708.2a).
    names: BTreeSet<String>,
    legendary: bool,
    world: bool,
    battle: bool,
    land: bool,
    aura: bool,
    equipment: bool,
    fortification: bool,
    saga: bool,
    /// CR 310.12, CR 205.3q: a battle with the subtype Siege (the only battle type).
    siege: bool,
    /// CR 303.7: an Aura with the subtype Role.
    role: bool,
    /// CR 702.179a: it has start your engines! (evaluated abilities).
    engine: bool,
    /// The counters on it (CR 122.1), as the layer environment reports them.
    counters: BTreeMap<String, i64>,
    /// Its controller (CR 704.5j, CR 613.1b, CR 110.2): the controller after layer 2 of the
    /// layer evaluation; if the evaluation names none, the controller in the permanent
    /// container, if the container holds it.
    controller: Option<PlayerId>,
    /// What it is attached to, from the permanent container (CR 701.3a).
    attached_to: Option<AttachTarget>,
}

/// One snapshot of everything the predicates read (CR 704.3: the whole state is checked at
/// once). Built once per check.
pub(super) struct SbaView<'a> {
    game: &'a GameState,
    zones: &'a Zones,
    events: &'a EventState,
    env: &'a dyn SbaEnvironment,
    phasing: &'a Permanents,
    permanents: BTreeMap<ObjectId, PermanentFacts>,
}

/// The vocabulary id of the start your engines! keyword ability (CR 702.179).
const START_YOUR_ENGINES: &str = "START_YOUR_ENGINES";

/// Whether `abilities` hold the keyword ability `keyword` (an ability of kind `KEYWORD`).
fn has_keyword(abilities: &[fineprint_dsl::card::AbilityAST], keyword: &str) -> bool {
    abilities.iter().any(|a| {
        a.0.get("kind").and_then(|k| k.as_str()) == Some("KEYWORD")
            && a.0.get("keyword").and_then(|k| k.as_str()) == Some(keyword)
    })
}

impl<'a> SbaView<'a> {
    /// Takes the snapshot: every battlefield object is evaluated through the layers once.
    /// Objects the layer environment does not know (or cannot evaluate) are skipped, and so are
    /// phased-out permanents: CR 702.26b treats them as though they do not exist, so the object
    /// entries CR 704.5f-704.5s never see them. CR 704.5d and CR 704.5e iterate by zone
    /// (CR 702.26d: phasing is not a zone change) and are unaffected. An object with no entry in
    /// `phasing` is treated as phased in. The entries CR 704.5j-704.5s read the same snapshot.
    pub(super) fn new(
        game: &'a GameState,
        zones: &'a Zones,
        events: &'a EventState,
        phasing: &'a Permanents,
        effects: &ContinuousEffects,
        layer_env: &dyn LayerEnvironment,
        env: &'a dyn SbaEnvironment,
    ) -> SbaView<'a> {
        let mut permanents = BTreeMap::new();
        for id in zones.contents(ZoneId::Battlefield) {
            // CR 702.26b
            if phasing.get(id).is_some_and(|p| p.status().phased_out) {
                continue;
            }
            // CR 613.1, CR 613.4: the characteristics after every layer.
            let Ok(evaluated) = evaluate(effects, layer_env, id) else {
                continue;
            };
            let c = &evaluated.characteristics;
            // CR 613.4: the pipeline writes the final P/T as numeric literals; read the
            // literal itself, not the environment's expression hook (which may decline it).
            let toughness = c.toughness.as_ref().and_then(literal_of);
            // CR 306.5c: loyalty is the number of loyalty counters; none means 0.
            let counters = layer_env.counters(id);
            let loyalty = counters.get("loyalty").copied().unwrap_or(0);
            let permanent = phasing.get(id);
            permanents.insert(
                id,
                PermanentFacts {
                    creature: c.card_types.contains("Creature"),
                    planeswalker: c.card_types.contains("Planeswalker"),
                    toughness,
                    // CR 702.12b
                    indestructible: has_keyword(&c.abilities, "indestructible"),
                    loyalty,
                    names: c.names.clone(),
                    // CR 205.4: supertypes.
                    legendary: c.supertypes.contains("Legendary"),
                    world: c.supertypes.contains("World"),
                    battle: c.card_types.contains("Battle"),
                    land: c.card_types.contains("Land"),
                    // CR 205.3: subtypes.
                    aura: c.subtypes.contains("Aura"),
                    equipment: c.subtypes.contains("Equipment"),
                    fortification: c.subtypes.contains("Fortification"),
                    saga: c.subtypes.contains("Saga"),
                    siege: c.subtypes.contains("Siege"),
                    role: c.subtypes.contains("Role"),
                    // CR 702.179a: the vocabulary id of the keyword (decision #43 FP-042).
                    engine: has_keyword(&c.abilities, START_YOUR_ENGINES),
                    counters,
                    // CR 613.1b: control-changing effects apply in layer 2; the stored
                    // controller is only the fallback for an evaluation that names none.
                    controller: evaluated
                        .controller
                        .or_else(|| permanent.map(crate::permanent::Permanent::controller)),
                    attached_to: permanent.and_then(crate::permanent::Permanent::attached_to),
                },
            );
        }
        SbaView {
            game,
            zones,
            events,
            env,
            phasing,
            permanents,
        }
    }

    /// The players who have not lost the game, ascending.
    fn remaining_players(&self) -> impl Iterator<Item = &PlayerState> + '_ {
        let mut players: Vec<&PlayerState> = self
            .game
            .players()
            .filter(|p| p.designation(LOST_THE_GAME).is_none())
            .collect();
        players.sort_by_key(|p| p.id());
        players.into_iter()
    }

    fn remaining(&self, player: PlayerId) -> bool {
        self.game
            .player(player)
            .is_some_and(|p| p.designation(LOST_THE_GAME).is_none())
    }

    /// Every object in a zone with its zone, ascending id.
    fn objects(&self) -> impl Iterator<Item = (ObjectId, ZoneId, fineprint_base::ids::CardId)> {
        let zones = self.zones;
        zones
            .all_object_ids()
            .into_iter()
            .filter_map(move |id| zones.object(id).map(|r| (id, r.zone, r.card)))
    }

    /// Whether permanent `id` is phased out (CR 702.26b).
    fn phased_out(&self, id: ObjectId) -> bool {
        self.phasing.get(id).is_some_and(|p| p.status().phased_out)
    }

    /// Whether object `id` is on the battlefield.
    fn on_battlefield(&self, id: ObjectId) -> bool {
        self.zones
            .object(id)
            .is_some_and(|r| r.zone == ZoneId::Battlefield)
    }

    /// The checked permanents that are attached to something whose attachment can be judged:
    /// a phased-out target permanent is not checked (CR 702.26b).
    fn attachments(&self) -> impl Iterator<Item = (ObjectId, &PermanentFacts, AttachTarget)> + '_ {
        self.permanents.iter().filter_map(|(&id, f)| {
            let target = f.attached_to?;
            match target {
                AttachTarget::Permanent(x) if self.phased_out(x) => None,
                _ => Some((id, f, target)),
            }
        })
    }

    /// The creatures with toughness greater than 0 that are not indestructible.
    fn destructible_creatures(&self) -> impl Iterator<Item = (ObjectId, i64)> + '_ {
        self.permanents.iter().filter_map(|(&id, f)| {
            let t = f.toughness?;
            (f.creature && t > 0 && !f.indestructible).then_some((id, t))
        })
    }
}

type Predicate = fn(&SbaView<'_>) -> Vec<SbaSubject>;

/// One state-based action of the registry (REQ-SBA-001, SRS 9): id, CR reference, predicate,
/// action and variant gate.
#[derive(Clone)]
pub struct SbaEntry {
    /// The entry id, the rule number (for example `"704.5a"`).
    pub id: &'static str,
    /// The CR reference, for example `"CR 704.5a"`.
    pub cr_ref: &'static str,
    /// What performing it does.
    pub action: SbaAction,
    /// In which variants it is active.
    pub gate: VariantGate,
    predicate: Predicate,
}

impl fmt::Debug for SbaEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SbaEntry")
            .field("id", &self.id)
            .field("cr_ref", &self.cr_ref)
            .field("action", &self.action)
            .field("gate", &self.gate)
            .finish_non_exhaustive()
    }
}

impl SbaEntry {
    /// Whether the entry is checked in `variant`.
    #[must_use]
    pub fn active_in(&self, variant: GameVariant) -> bool {
        match self.gate {
            VariantGate::All => true,
            VariantGate::Except(v) => v != variant,
        }
    }

    /// The subjects the entry applies to in `view`, ascending.
    pub(super) fn subjects(&self, view: &SbaView<'_>) -> Vec<SbaSubject> {
        let mut subjects = (self.predicate)(view);
        subjects.sort();
        subjects.dedup();
        subjects
    }
}

/// The state-based action registry (REQ-SBA-001, SRS 9). A new state-based action is a new
/// entry, not a new control path.
#[derive(Clone, Debug)]
pub struct SbaRegistry {
    entries: Vec<SbaEntry>,
}

impl SbaRegistry {
    /// The state-based actions of CR 704.5a-704.5t, then CR 704.5v-704.5aa, in that order (the
    /// CR has no 704.5l and no 704.5o; CR 704.5u is not modelled here).
    #[must_use]
    pub fn standard() -> SbaRegistry {
        SbaRegistry {
            entries: vec![
                // CR 704.5a: a player with 0 or less life loses the game. Not ignored in
                // Two-Headed Giant games (the rule has no such clause).
                SbaEntry {
                    id: "704.5a",
                    cr_ref: "CR 704.5a",
                    action: SbaAction::LoseGame,
                    gate: VariantGate::All,
                    predicate: life_zero_or_less,
                },
                // CR 704.5b, CR 121.4: a player who attempted to draw from an empty library
                // since the last check loses the game.
                SbaEntry {
                    id: "704.5b",
                    cr_ref: "CR 704.5b",
                    action: SbaAction::LoseGame,
                    gate: VariantGate::All,
                    predicate: drew_from_empty_library,
                },
                // CR 704.5c: ten or more poison counters; ignored in Two-Headed Giant games.
                SbaEntry {
                    id: "704.5c",
                    cr_ref: "CR 704.5c",
                    action: SbaAction::LoseGame,
                    gate: VariantGate::Except(GameVariant::TwoHeadedGiant),
                    predicate: ten_or_more_poison,
                },
                // CR 704.5d, CR 111.7: a token in a zone other than the battlefield ceases to
                // exist.
                SbaEntry {
                    id: "704.5d",
                    cr_ref: "CR 704.5d",
                    action: SbaAction::CeaseToExist,
                    gate: VariantGate::All,
                    predicate: token_off_battlefield,
                },
                // CR 704.5e, CR 707.10a (exception CR 723.3c): a copy of a spell off the
                // stack, or a copy of a card off the stack and the battlefield, ceases to
                // exist.
                SbaEntry {
                    id: "704.5e",
                    cr_ref: "CR 704.5e",
                    action: SbaAction::CeaseToExist,
                    gate: VariantGate::All,
                    predicate: copy_out_of_place,
                },
                // CR 704.5f: a creature with toughness 0 or less is put into its owner's
                // graveyard (not destroyed; indestructible does not stop it).
                SbaEntry {
                    id: "704.5f",
                    cr_ref: "CR 704.5f",
                    action: SbaAction::PutIntoGraveyard,
                    gate: VariantGate::All,
                    predicate: toughness_zero_or_less,
                },
                // CR 704.5g, CR 702.12b: lethal damage destroys a creature with toughness
                // greater than 0 unless it is indestructible. Regeneration (which can replace
                // it) is not modelled yet.
                SbaEntry {
                    id: "704.5g",
                    cr_ref: "CR 704.5g",
                    action: SbaAction::Destroy,
                    gate: VariantGate::All,
                    predicate: lethal_damage,
                },
                // CR 704.5h, CR 702.2b, CR 702.12b: damage from a deathtouch source since the
                // last check destroys a creature with toughness greater than 0 unless it is
                // indestructible. Regeneration is not modelled yet.
                SbaEntry {
                    id: "704.5h",
                    cr_ref: "CR 704.5h",
                    action: SbaAction::Destroy,
                    gate: VariantGate::All,
                    predicate: deathtouch_damage,
                },
                // CR 704.5i, CR 306.5c: a planeswalker with loyalty 0 is put into its owner's
                // graveyard.
                SbaEntry {
                    id: "704.5i",
                    cr_ref: "CR 704.5i",
                    action: SbaAction::PutIntoGraveyard,
                    gate: VariantGate::All,
                    predicate: zero_loyalty,
                },
                // CR 704.5j, CR 201.2a: the legend rule. Two or more legendary permanents with
                // the same name controlled by the same player: that player chooses one, the
                // rest are put into their owners' graveyards.
                SbaEntry {
                    id: "704.5j",
                    cr_ref: "CR 704.5j",
                    action: SbaAction::LegendChoice,
                    gate: VariantGate::All,
                    predicate: legend_rule,
                },
                // CR 704.5k: the world rule. All world permanents except the one that has had
                // the supertype for the shortest time go to their owners' graveyards; on a tie
                // for the shortest time, all go.
                SbaEntry {
                    id: "704.5k",
                    cr_ref: "CR 704.5k",
                    action: SbaAction::PutIntoGraveyard,
                    gate: VariantGate::All,
                    predicate: world_rule,
                },
                // CR 704.5m, CR 303.4c, CR 303.4d: an Aura attached to an illegal object or
                // player, or not attached, is put into its owner's graveyard.
                SbaEntry {
                    id: "704.5m",
                    cr_ref: "CR 704.5m",
                    action: SbaAction::PutIntoGraveyard,
                    gate: VariantGate::All,
                    predicate: illegal_aura,
                },
                // CR 704.5n, CR 301.5, CR 301.5c, CR 301.6: an Equipment or Fortification
                // attached to an illegal permanent or to a player becomes unattached and
                // remains on the battlefield.
                SbaEntry {
                    id: "704.5n",
                    cr_ref: "CR 704.5n",
                    action: SbaAction::Unattach,
                    gate: VariantGate::All,
                    predicate: illegal_equipment,
                },
                // CR 704.5p, CR 310.10: an attached battle or creature, or an attached
                // nonbattle, noncreature permanent that is neither an Aura, an Equipment nor a
                // Fortification, becomes unattached and remains on the battlefield.
                SbaEntry {
                    id: "704.5p",
                    cr_ref: "CR 704.5p",
                    action: SbaAction::Unattach,
                    gate: VariantGate::All,
                    predicate: cannot_be_attached,
                },
                // CR 704.5q, CR 122.3: N +1/+1 and N -1/-1 counters are removed, N the smaller
                // count.
                SbaEntry {
                    id: "704.5q",
                    cr_ref: "CR 704.5q",
                    action: SbaAction::RemoveCounters,
                    gate: VariantGate::All,
                    predicate: opposed_counters,
                },
                // CR 704.5r: a permanent with more than the N counters of a kind its ability
                // allows loses all but N of them.
                SbaEntry {
                    id: "704.5r",
                    cr_ref: "CR 704.5r",
                    action: SbaAction::RemoveCounters,
                    gate: VariantGate::All,
                    predicate: counters_over_limit,
                },
                // CR 704.5s, CR 715.2d, CR 715.4: a Saga permanent with one or more chapter
                // abilities whose lore counters are greater than or equal to its final chapter
                // number, and that isn't the source of a chapter ability that has triggered but
                // not yet left the stack, is sacrificed by its controller.
                SbaEntry {
                    id: "704.5s",
                    cr_ref: "CR 704.5s",
                    action: SbaAction::Sacrifice,
                    gate: VariantGate::All,
                    predicate: saga_complete,
                },
                // CR 704.5t, CR 309.2c, CR 309.7: a dungeon card with its owner's venture
                // marker on its bottommost room, and that isn't the source of a room ability
                // that has triggered but not yet left the stack, is removed from the game by
                // its owner.
                SbaEntry {
                    id: "704.5t",
                    cr_ref: "CR 704.5t",
                    action: SbaAction::RemoveFromGame,
                    gate: VariantGate::All,
                    predicate: dungeon_complete,
                },
                // CR 704.5v, CR 310.7, CR 310.4c: a Siege battle with defense 0 that isn't the
                // source of an ability that has triggered but not yet left the stack is put into
                // its owner's graveyard.
                SbaEntry {
                    id: "704.5v",
                    cr_ref: "CR 704.5v",
                    action: SbaAction::PutIntoGraveyard,
                    gate: VariantGate::All,
                    predicate: siege_zero_defense,
                },
                // CR 704.5w, CR 310.8: a non-Siege battle with defense 0 is put into its owner's
                // graveyard.
                SbaEntry {
                    id: "704.5w",
                    cr_ref: "CR 704.5w",
                    action: SbaAction::PutIntoGraveyard,
                    gate: VariantGate::All,
                    predicate: non_siege_zero_defense,
                },
                // CR 704.5x, CR 310.11, CR 310.9a, CR 310.12a: a battle with no player in the
                // game as its protector and no creature attacking it: its controller chooses an
                // appropriate protector, or it is put into its owner's graveyard.
                SbaEntry {
                    id: "704.5x",
                    cr_ref: "CR 704.5x",
                    action: SbaAction::ChooseProtector,
                    gate: VariantGate::All,
                    predicate: battle_without_protector,
                },
                // CR 704.5y, CR 310.11: a battle whose protector can't be its protector: its
                // controller chooses an appropriate protector, or it is put into its owner's
                // graveyard.
                SbaEntry {
                    id: "704.5y",
                    cr_ref: "CR 704.5y",
                    action: SbaAction::ChooseProtector,
                    gate: VariantGate::All,
                    predicate: battle_with_illegal_protector,
                },
                // CR 704.5z, CR 303.7a: of the Roles one player controls attached to one
                // permanent, all but the most recent are put into their owners' graveyards.
                SbaEntry {
                    id: "704.5z",
                    cr_ref: "CR 704.5z",
                    action: SbaAction::PutIntoGraveyard,
                    gate: VariantGate::All,
                    predicate: role_rule,
                },
                // CR 704.5aa, CR 702.179a: a player who controls a permanent with start your
                // engines! and has no speed gets speed 1.
                SbaEntry {
                    id: "704.5aa",
                    cr_ref: "CR 704.5aa",
                    action: SbaAction::SetSpeed,
                    gate: VariantGate::All,
                    predicate: engines_without_speed,
                },
            ],
        }
    }

    /// Every entry, in check order.
    #[must_use]
    pub fn entries(&self) -> &[SbaEntry] {
        &self.entries
    }

    /// The entry with this id, if any.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&SbaEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

// CR 704.5a
fn life_zero_or_less(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.remaining_players()
        .filter(|p| p.life() <= 0)
        .map(|p| SbaSubject::Player(p.id()))
        .collect()
}

// CR 704.5b, CR 121.4: the flag is "attempted since the last check", whatever the library
// holds now.
fn drew_from_empty_library(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.events
        .empty_library_draws()
        .into_iter()
        .filter(|&p| view.remaining(p))
        .map(SbaSubject::Player)
        .collect()
}

// CR 704.5c
fn ten_or_more_poison(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.remaining_players()
        .filter(|p| p.counter("poison") >= 10)
        .map(|p| SbaSubject::Player(p.id()))
        .collect()
}

// CR 704.5d, CR 111.7
fn token_off_battlefield(view: &SbaView<'_>) -> Vec<SbaSubject> {
    let markers = view.events.markers();
    view.objects()
        .filter(|&(_, zone, card)| zone != ZoneId::Battlefield && markers.is_token(card))
        .map(|(id, _, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5e, CR 707.10a, CR 723.3c
fn copy_out_of_place(view: &SbaView<'_>) -> Vec<SbaSubject> {
    let env = view.env;
    view.objects()
        .filter(|&(id, zone, _)| {
            if env.exempt_from_copy_sba(id) {
                return false;
            }
            let spell_copy = env.is_spell_copy(id) && zone != ZoneId::Stack;
            let card_copy =
                env.is_card_copy(id) && !matches!(zone, ZoneId::Stack | ZoneId::Battlefield);
            spell_copy || card_copy
        })
        .map(|(id, _, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5f, CR 107.1b (negative toughness compares as negative), CR 613.4
fn toughness_zero_or_less(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.permanents
        .iter()
        .filter(|(_, f)| f.creature && f.toughness.is_some_and(|t| t <= 0))
        .map(|(&id, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5g, CR 120.3e, CR 120.6, CR 702.12b
fn lethal_damage(view: &SbaView<'_>) -> Vec<SbaSubject> {
    let markers = view.events.markers();
    view.destructible_creatures()
        .filter(|&(id, t)| markers.damage(id) >= t)
        .map(|(id, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5h, CR 702.2b, CR 702.12b
fn deathtouch_damage(view: &SbaView<'_>) -> Vec<SbaSubject> {
    let damaged: BTreeSet<ObjectId> = view.env.deathtouch_damaged();
    view.destructible_creatures()
        .filter(|(id, _)| damaged.contains(id))
        .map(|(id, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5i, CR 306.5c
fn zero_loyalty(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.permanents
        .iter()
        .filter(|(_, f)| f.planeswalker && f.loyalty == 0)
        .map(|(&id, _)| SbaSubject::Object(id))
        .collect()
}

/// The representative of `i` in the union-find forest `parent` (with path halving).
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// One CR 704.5j conflict component: legendary permanents of one controller connected by
/// shared names (decision "#42 FP-041 — Legend rule: conflict graph, maximal independent set").
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LegendGroup {
    /// The (evaluated) controller who chooses (CR 704.5j "that player", CR 613.1b).
    pub(super) controller: PlayerId,
    /// The members, ascending (two or more).
    pub(super) members: Vec<ObjectId>,
    /// The names of each member, parallel to `members` (CR 201.2a).
    pub(super) names: Vec<BTreeSet<String>>,
}

impl LegendGroup {
    /// CR 201.2a: whether members `i` and `j` have the same name (at least one name in common).
    /// A nameless object never has the same name as anything.
    fn same_name(&self, i: usize, j: usize) -> bool {
        !self.names[i].is_disjoint(&self.names[j])
    }

    /// Whether every pair of members has the same name (the usual one-name group): then
    /// exactly one survives.
    pub(super) fn is_clique(&self) -> bool {
        let n = self.members.len();
        (0..n).all(|i| ((i + 1)..n).all(|j| self.same_name(i, j)))
    }

    /// The largest number of survivors a request offers: 1 for a clique, otherwise one less
    /// than the size (a connected component with an edge never keeps everything).
    pub(super) fn max_survivors(&self) -> usize {
        if self.is_clique() {
            1
        } else {
            self.members.len().saturating_sub(1)
        }
    }

    /// The deterministic default survivor set: the greedy maximal independent set by
    /// ascending object id (take the lowest id, then each next id that has no name in common
    /// with any already taken).
    pub(super) fn default_survivors(&self) -> Vec<ObjectId> {
        let mut taken: Vec<usize> = Vec::new();
        for i in 0..self.members.len() {
            if taken.iter().all(|&t| !self.same_name(i, t)) {
                taken.push(i);
            }
        }
        taken.into_iter().map(|i| self.members[i]).collect()
    }

    /// CR 704.5j, CR 201.2a: checks that `selected` is a valid survivor set, a MAXIMAL
    /// independent set of the conflict graph: every selected id is a member, none twice, no two
    /// selected have the same name, and every member not selected has the same name as at least
    /// one selected member.
    ///
    /// # Errors
    /// A message naming the violated condition.
    pub(super) fn check_survivors(&self, selected: &[ObjectId]) -> Result<(), String> {
        if selected.is_empty() {
            return Err("the legend rule keeps at least one permanent".to_owned());
        }
        let mut indices: BTreeSet<usize> = BTreeSet::new();
        for id in selected {
            let Some(i) = self.members.iter().position(|m| m == id) else {
                return Err(format!("{id:?} is not a candidate"));
            };
            if !indices.insert(i) {
                return Err(format!("{id:?} is selected twice"));
            }
        }
        for &i in &indices {
            for &j in &indices {
                if i < j && self.same_name(i, j) {
                    return Err(format!(
                        "{:?} and {:?} have the same name and cannot both stay",
                        self.members[i], self.members[j]
                    ));
                }
            }
        }
        for i in 0..self.members.len() {
            if !indices.contains(&i) && indices.iter().all(|&k| !self.same_name(i, k)) {
                return Err(format!(
                    "{:?} has no name in common with a kept permanent and must stay",
                    self.members[i]
                ));
            }
        }
        Ok(())
    }
}

/// CR 101.4: the players in APNAP order: the turn order starting from the active player (all
/// players ascending if the turn order is empty); players not in the turn order last,
/// ascending.
pub(super) fn apnap_order(game: &GameState) -> Vec<PlayerId> {
    let mut order: Vec<PlayerId> = if game.turn_order().is_empty() {
        let mut all: Vec<PlayerId> = game.players().map(PlayerState::id).collect();
        all.sort_unstable();
        all
    } else {
        game.turn_order().to_vec()
    };
    if let Some(active) = game.turn().active_player()
        && let Some(pos) = order.iter().position(|&p| p == active)
    {
        order.rotate_left(pos);
    }
    let mut rest: Vec<PlayerId> = game
        .players()
        .map(PlayerState::id)
        .filter(|id| !order.contains(id))
        .collect();
    rest.sort_unstable();
    order.extend(rest);
    order
}

// CR 704.5j, CR 201.2a: the conflict components of legendary permanents, per (evaluated)
// controller. The vertices are the evaluated, phased-in, named legendary permanents of one
// controller; an edge joins two that have at least one name in common (an object with no name
// matches nothing: CR 201.2a, CR 708.2a). Every connected component with at least one edge is
// a group (members ascending). CR 101.4: the groups are ordered by their controller's APNAP
// rank, then (CR 101.4c, decision #42 FP-041) by their lowest id.
pub(super) fn legend_groups(view: &SbaView<'_>) -> Vec<LegendGroup> {
    let mut by_controller: BTreeMap<PlayerId, Vec<(ObjectId, &BTreeSet<String>)>> = BTreeMap::new();
    for (&id, f) in &view.permanents {
        let Some(controller) = f.controller else {
            continue;
        };
        if f.legendary && !f.names.is_empty() {
            by_controller
                .entry(controller)
                .or_default()
                .push((id, &f.names));
        }
    }
    let mut groups = Vec::new();
    for (controller, members) in by_controller {
        // Union-find over indices (ascending id order), integer only.
        let mut parent: Vec<usize> = (0..members.len()).collect();
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                // CR 201.2a: at least one name in common.
                if !members[i].1.is_disjoint(members[j].1) {
                    let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                    if a != b {
                        parent[a.max(b)] = a.min(b);
                    }
                }
            }
        }
        let mut components: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..members.len() {
            let r = root(&mut parent, i);
            components.entry(r).or_default().push(i);
        }
        for indices in components.into_values() {
            // Two or more vertices in one component means at least one edge.
            if indices.len() >= 2 {
                groups.push(LegendGroup {
                    controller,
                    members: indices.iter().map(|&i| members[i].0).collect(),
                    names: indices.iter().map(|&i| members[i].1.clone()).collect(),
                });
            }
        }
    }
    let order = apnap_order(view.game);
    let rank = |p: PlayerId| order.iter().position(|&q| q == p).unwrap_or(order.len());
    groups.sort_by_key(|g| (rank(g.controller), g.members[0]));
    groups
}

// CR 704.5j
fn legend_rule(view: &SbaView<'_>) -> Vec<SbaSubject> {
    legend_groups(view)
        .into_iter()
        .flat_map(|g| g.members)
        .map(SbaSubject::Object)
        .collect()
}

// CR 704.5k: the stamp of `world_supertype_since` measures how long each permanent has had the
// world supertype (larger = gained later = shorter time). A unique largest stamp survives;
// a tie for the largest sends all.
fn world_rule(view: &SbaView<'_>) -> Vec<SbaSubject> {
    let worlds: Vec<(ObjectId, u64)> = view
        .permanents
        .iter()
        .filter(|(_, f)| f.world)
        .map(|(&id, _)| (id, view.env.world_supertype_since(id)))
        .collect();
    if worlds.len() < 2 {
        return Vec::new();
    }
    let newest = worlds.iter().map(|&(_, s)| s).max().unwrap_or(0);
    let holders = worlds.iter().filter(|&&(_, s)| s == newest).count();
    worlds
        .into_iter()
        .filter(|&(_, s)| holders > 1 || s != newest)
        .map(|(id, _)| SbaSubject::Object(id))
        .collect()
}

/// CR 303.4c, CR 303.4d, CR 301.5c: whether the attachment target of `id` is `id` itself, a
/// permanent that is not on the battlefield (it no longer exists) or a player who has left the
/// game. The permanent container refuses a self-attachment, so the first case is defensive.
pub(super) fn target_self_or_gone(
    id: ObjectId,
    target: AttachTarget,
    on_battlefield: &dyn Fn(ObjectId) -> bool,
    remaining: &dyn Fn(PlayerId) -> bool,
) -> bool {
    match target {
        AttachTarget::Permanent(x) => x == id || !on_battlefield(x),
        AttachTarget::Player(pl) => !remaining(pl),
    }
}

// CR 704.5m, CR 303.4c, CR 303.4d
fn illegal_aura(view: &SbaView<'_>) -> Vec<SbaSubject> {
    let mut out = Vec::new();
    for (&id, f) in &view.permanents {
        if !f.aura {
            continue;
        }
        let illegal = match f.attached_to {
            // CR 704.5m: not attached to an object or player.
            None => true,
            // CR 702.26b: an attachment to a phased-out permanent is not checked.
            Some(AttachTarget::Permanent(x)) if view.phased_out(x) => false,
            Some(target) => {
                let bad_target =
                    target_self_or_gone(id, target, &|x| view.on_battlefield(x), &|pl| {
                        view.remaining(pl)
                    });
                // CR 303.4d: an Aura that's also a creature can't enchant anything. An Aura
                // that's also a battle is not illegal here: CR 310.10 / 704.5p unattach it and
                // CR 704.5m takes it as "not attached" in the next check (decision #42 FP-041).
                bad_target
                    || f.creature
                    // CR 303.4c: as defined by its enchant ability and other effects.
                    || !view.env.aura_can_enchant(id, target)
            }
        };
        if illegal {
            out.push(SbaSubject::Object(id));
        }
    }
    out
}

// CR 704.5n, CR 301.5, CR 301.5c, CR 301.6, CR 702.16d
fn illegal_equipment(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.attachments()
        .filter(|(id, f, target)| {
            if !(f.equipment || f.fortification) {
                return false;
            }
            match *target {
                // CR 704.5n: attached to a player.
                AttachTarget::Player(_) => true,
                // CR 301.5c: an Equipment can't equip itself; nor a nonexistent permanent.
                AttachTarget::Permanent(_)
                    if target_self_or_gone(*id, *target, &|x| view.on_battlefield(x), &|_| {
                        true
                    }) =>
                {
                    true
                }
                AttachTarget::Permanent(x) => {
                    // CR 702.16d, CR 301.5c: protection and other effects that say the
                    // permanent can't be equipped or fortified by it make the pair illegal.
                    // Asked for the exact pair after the phased-out guard (`attachments`) and
                    // before the target's facts, so a marked pair is unattached even when the
                    // target cannot be evaluated (decision #42 FP-041, round 2).
                    if !view.env.equipment_can_attach(*id, *target) {
                        return true;
                    }
                    // A target the layer environment cannot evaluate is not judged further.
                    let Some(t) = view.permanents.get(&x) else {
                        return false;
                    };
                    // CR 301.5: an Equipment only on a creature; CR 301.6: a Fortification
                    // only on a land. Each subtype's restriction applies, so an artifact that
                    // is both is legal only on a permanent that is both (decision #42 FP-041).
                    let legal = (!f.equipment || t.creature) && (!f.fortification || t.land);
                    !legal
                }
            }
        })
        .map(|(id, _, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5p, CR 310.10
fn cannot_be_attached(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.attachments()
        .filter(|(_, f, _)| f.creature || f.battle || !(f.aura || f.equipment || f.fortification))
        .map(|(id, _, _)| SbaSubject::Object(id))
        .collect()
}

/// CR 704.5q, CR 122.3: the counters 704.5q removes from `id` (N of each, N the smaller count).
fn opposed_removal(view: &SbaView<'_>, id: ObjectId) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    if let Some(f) = view.permanents.get(&id) {
        let plus = f.counters.get("+1/+1").copied().unwrap_or(0);
        let minus = f.counters.get("-1/-1").copied().unwrap_or(0);
        let n = plus.min(minus);
        if n > 0 {
            out.insert("+1/+1".to_owned(), n);
            out.insert("-1/-1".to_owned(), n);
        }
    }
    out
}

/// CR 704.5r: the counters 704.5r removes from `id` (all but N of each limited kind).
fn excess_removal(view: &SbaView<'_>, id: ObjectId) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    if let Some(f) = view.permanents.get(&id) {
        for (kind, limit) in view.env.counter_limits(id) {
            let count = f.counters.get(&kind).copied().unwrap_or(0);
            let limit = limit.max(0);
            if count > limit {
                out.insert(kind, count - limit);
            }
        }
    }
    out
}

/// The counters a `RemoveCounters` finding of entry `entry_id` removes from `id`, by kind.
pub(super) fn counter_removal(
    view: &SbaView<'_>,
    entry_id: &str,
    id: ObjectId,
) -> BTreeMap<String, i64> {
    match entry_id {
        // CR 704.5q
        "704.5q" => opposed_removal(view, id),
        // CR 704.5r
        "704.5r" => excess_removal(view, id),
        _ => BTreeMap::new(),
    }
}

// CR 704.5q, CR 122.3
fn opposed_counters(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.permanents
        .keys()
        .filter(|&&id| !opposed_removal(view, id).is_empty())
        .map(|&id| SbaSubject::Object(id))
        .collect()
}

// CR 704.5r
fn counters_over_limit(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.permanents
        .keys()
        .filter(|&&id| !excess_removal(view, id).is_empty())
        .map(|&id| SbaSubject::Object(id))
        .collect()
}

// CR 704.5s, CR 715.2d, CR 715.4
fn saga_complete(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.permanents
        .iter()
        .filter(|&(&id, f)| {
            if !f.saga {
                return false;
            }
            // "with one or more chapter abilities": `None` means it has none.
            let Some(final_chapter) = view.env.saga_final_chapter(id) else {
                return false;
            };
            let lore = f.counters.get("lore").copied().unwrap_or(0);
            lore >= final_chapter && !view.env.chapter_ability_pending(id)
        })
        .map(|(&id, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5t, CR 309.2c: dungeon cards are command-zone objects, not permanents.
fn dungeon_complete(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.env
        .dungeons_at_bottommost_room()
        .into_iter()
        .filter(|&id| {
            view.zones
                .object(id)
                .is_some_and(|r| r.zone == ZoneId::Command)
                && !view.env.room_ability_pending(id)
        })
        .map(SbaSubject::Object)
        .collect()
}

/// CR 310.4c: the defense of a battle on the battlefield is the number of defense counters on
/// it (none, or a negative count, is 0 or less).
fn defense(f: &PermanentFacts) -> i64 {
    f.counters.get("defense").copied().unwrap_or(0)
}

// CR 704.5v, CR 310.7, CR 310.4c
fn siege_zero_defense(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.permanents
        .iter()
        .filter(|&(&id, f)| {
            f.battle && f.siege && defense(f) <= 0 && !view.env.battle_ability_pending(id)
        })
        .map(|(&id, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5w, CR 310.8
fn non_siege_zero_defense(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.permanents
        .iter()
        .filter(|(_, f)| f.battle && !f.siege && defense(f) <= 0)
        .map(|(&id, _)| SbaSubject::Object(id))
        .collect()
}

impl SbaView<'_> {
    /// CR 310.11, CR 506.4c: whether a creature is attacking battle `battle` (an attacker whose
    /// battle was removed from combat attacks nothing).
    fn battle_attacked(&self, battle: ObjectId) -> bool {
        let attacked = AttackState::Attacking(Some(AttackTarget::Permanent(battle)));
        self.phasing.ids().into_iter().any(|id| {
            self.phasing
                .get(id)
                .is_some_and(|p| p.combat().attack == attacked)
        })
    }

    /// CR 310.9a, CR 310.12a, CR 704.5y: whether `player` is an appropriate protector of
    /// battle `battle` with facts `f`: in the game, allowed by effects, and for a Siege an
    /// opponent of its controller, for a battle with no battle type its controller (CR 205.3q:
    /// Siege is the only battle type).
    fn appropriate_protector(
        &self,
        battle: ObjectId,
        f: &PermanentFacts,
        player: PlayerId,
    ) -> bool {
        let Some(controller) = f.controller else {
            return false;
        };
        let by_type = if f.siege {
            player != controller
        } else {
            player == controller
        };
        by_type && self.remaining(player) && self.env.can_be_protector(battle, player)
    }

    /// The phased-in battles that are not being attacked (CR 310.11), with their protector
    /// if it is a player in the game (CR 704.5x).
    fn unattacked_battles(
        &self,
    ) -> impl Iterator<Item = (ObjectId, &PermanentFacts, Option<PlayerId>)> + '_ {
        self.permanents
            .iter()
            .filter(|&(&id, f)| f.battle && !self.battle_attacked(id))
            .map(|(&id, f)| {
                let protector = self.env.battle_protector(id).filter(|&p| self.remaining(p));
                (id, f, protector)
            })
    }
}

/// One protector choice of a pass (CR 704.5x, CR 704.5y).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProtectorAsk {
    /// The battle.
    pub(super) battle: ObjectId,
    /// Its (evaluated) controller, who chooses; `None` only when there are no candidates.
    pub(super) controller: Option<PlayerId>,
    /// The appropriate players, ascending (CR 310.9a, CR 310.12a).
    pub(super) candidates: Vec<PlayerId>,
    /// The CR reference of the entry, `"CR 704.5x"` or `"CR 704.5y"`.
    pub(super) cr: &'static str,
}

/// CR 704.5x, CR 704.5y, CR 310.11: the protector choices for `battles` (battle to the CR
/// reference of its finding): for each, its controller and the appropriate candidates,
/// ascending by player id. Ordered by the controller's APNAP rank (CR 101.4), then ascending
/// battle id (decision #43 FP-042); a battle without a controller (no candidates) last. Battles
/// not in the snapshot are skipped.
pub(super) fn protector_asks(
    view: &SbaView<'_>,
    battles: &BTreeMap<ObjectId, &'static str>,
) -> Vec<ProtectorAsk> {
    let order = apnap_order(view.game);
    let rank = |p: Option<PlayerId>| {
        p.and_then(|p| order.iter().position(|&q| q == p))
            .unwrap_or(order.len())
    };
    let mut asks: Vec<ProtectorAsk> = battles
        .iter()
        .filter_map(|(&battle, &cr)| {
            let f = view.permanents.get(&battle)?;
            let candidates = view
                .remaining_players()
                .map(PlayerState::id)
                .filter(|&p| view.appropriate_protector(battle, f, p))
                .collect();
            Some(ProtectorAsk {
                battle,
                controller: f.controller,
                candidates,
                cr,
            })
        })
        .collect();
    asks.sort_by_key(|a| (rank(a.controller), a.battle));
    asks
}

// CR 704.5x, CR 310.11: no player in the game is its protector and it is not being attacked.
fn battle_without_protector(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.unattacked_battles()
        .filter(|(_, _, protector)| protector.is_none())
        .map(|(id, _, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5y, CR 310.11: its protector, a player in the game, can't be its protector (decision
// #43 FP-042: not appropriate for its battle type, or not allowed by effects). A battle being
// attacked is exempt (CR 310.11).
fn battle_with_illegal_protector(view: &SbaView<'_>) -> Vec<SbaSubject> {
    view.unattacked_battles()
        .filter(|&(id, f, protector)| {
            protector.is_some_and(|p| !view.appropriate_protector(id, f, p))
        })
        .map(|(id, _, _)| SbaSubject::Object(id))
        .collect()
}

// CR 704.5z, CR 303.7a, CR 613.7e, CR 613.7m: the Roles attached to one permanent or player,
// grouped by the Role's (evaluated) controller; in each group of two or more every Role but
// the most recent goes. A tie of the latest timestamp keeps the higher ObjectId (decision
// #43 FP-042).
fn role_rule(view: &SbaView<'_>) -> Vec<SbaSubject> {
    let mut groups: BTreeMap<(AttachTarget, PlayerId), Vec<(u64, ObjectId)>> = BTreeMap::new();
    for (id, f, target) in view.attachments() {
        let Some(controller) = f.controller else {
            continue;
        };
        if f.aura && f.role {
            groups
                .entry((target, controller))
                .or_default()
                .push((view.env.attachment_timestamp(id), id));
        }
    }
    let mut out = Vec::new();
    for members in groups.into_values() {
        if members.len() < 2 {
            continue;
        }
        let newest = members.iter().max().copied();
        out.extend(
            members
                .into_iter()
                .filter(|m| Some(*m) != newest)
                .map(|(_, id)| SbaSubject::Object(id)),
        );
    }
    out
}

// CR 704.5aa, CR 702.179a, CR 702.179b: a player in the game who controls a phased-in permanent
// with start your engines! and has no speed.
fn engines_without_speed(view: &SbaView<'_>) -> Vec<SbaSubject> {
    let engines: BTreeSet<PlayerId> = view
        .permanents
        .values()
        .filter(|f| f.engine)
        .filter_map(|f| f.controller)
        .collect();
    view.remaining_players()
        .filter(|p| engines.contains(&p.id()) && p.designation(super::types::SPEED).is_none())
        .map(|p| SbaSubject::Player(p.id()))
        .collect()
}
