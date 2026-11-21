// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The data-driven state-based action registry (REQ-SBA-001, SRS 9): one entry per rule, each
//! with its id, CR reference, predicate, action and variant gate.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fineprint_base::ids::{ObjectId, PlayerId};

use super::types::{GameVariant, LOST_THE_GAME, SbaEnvironment, SbaSubject};
use crate::events::EventState;
use crate::layers::{ContinuousEffects, LayerEnvironment, evaluate};
use crate::permanent::Permanents;
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
    /// The permanent is put into its owner's graveyard (CR 704.5f, CR 704.5i).
    PutIntoGraveyard,
    /// The permanent is destroyed (CR 704.5g, CR 704.5h, CR 701.8b): a move to its owner's
    /// graveyard. Regeneration (CR 701.8c) could replace it but is not modelled yet.
    Destroy,
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
}

/// One snapshot of everything the predicates read (CR 704.3: the whole state is checked at
/// once). Built once per check.
pub(super) struct SbaView<'a> {
    game: &'a GameState,
    zones: &'a Zones,
    events: &'a EventState,
    env: &'a dyn SbaEnvironment,
    permanents: BTreeMap<ObjectId, PermanentFacts>,
}

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
    /// entries CR 704.5f-704.5i never see them. CR 704.5d and CR 704.5e iterate by zone
    /// (CR 702.26d: phasing is not a zone change) and are unaffected. An object with no entry in
    /// `phasing` is treated as phased in.
    pub(super) fn new(
        game: &'a GameState,
        zones: &'a Zones,
        events: &'a EventState,
        phasing: &Permanents,
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
            let toughness = c
                .toughness
                .as_ref()
                .and_then(|t| layer_env.eval_value(t, id));
            // CR 306.5c: loyalty is the number of loyalty counters; none means 0.
            let loyalty = layer_env.counters(id).get("loyalty").copied().unwrap_or(0);
            permanents.insert(
                id,
                PermanentFacts {
                    creature: c.card_types.contains("Creature"),
                    planeswalker: c.card_types.contains("Planeswalker"),
                    toughness,
                    // CR 702.12b
                    indestructible: has_keyword(&c.abilities, "indestructible"),
                    loyalty,
                },
            );
        }
        SbaView {
            game,
            zones,
            events,
            env,
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
    /// The state-based actions of CR 704.5a-704.5i, in that order.
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
