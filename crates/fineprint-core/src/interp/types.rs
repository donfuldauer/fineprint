// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The interpreter's data types: referents, chosen values, object views, the injected
//! environment, the evaluation context and results (REQ-DSL-003, REQ-ARCH-002).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId, Timestamp};
use fineprint_dsl::expr::{ReferenceAST, Zone};

use crate::events::History;
use crate::memory::Memory;
use crate::objects::Characteristics;

/// What a reference names: one object or one player (REQ-DSL-003, SRS §14.4.0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Referent {
    /// An object, by its current id.
    Object(ObjectId),
    /// A player.
    Player(PlayerId),
}

/// A value chosen earlier and read back by `CHOSEN(key)` (REQ-DSL-003).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChosenValue {
    /// A chosen object.
    Object(ObjectId),
    /// A chosen player.
    Player(PlayerId),
    /// A chosen number (not a referent: `CHOSEN` of a number is unresolved).
    Number(i64),
}

/// One object as the interpreter reads it (REQ-DSL-003): every field is already
/// layer-evaluated by the environment. The interpreter never reads the raw card.
///
/// This view is omniscient (it carries hidden data such as a face-down object's
/// characteristics); it never leaves the rules engine except through the redacted view
/// (REQ-CORE-004, R12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectView {
    /// The object's id.
    pub id: ObjectId,
    /// The zone the object is in (or was in, for last known information).
    pub zone: Zone,
    /// The owner.
    pub owner: PlayerId,
    /// The controller; `None` for an object without one (e.g. a card in a graveyard).
    pub controller: Option<PlayerId>,
    /// The evaluated characteristics (names, types, subtypes, supertypes, colors, loyalty,
    /// defense).
    pub characteristics: Characteristics,
    /// The evaluated mana value.
    pub mana_value: i64,
    /// The evaluated power, if the object has one.
    pub power: Option<i64>,
    /// The evaluated toughness, if the object has one.
    pub toughness: Option<i64>,
    /// The loyalty (counters on the battlefield, printed elsewhere), if any.
    pub loyalty: Option<i64>,
    /// The defense, if any.
    pub defense: Option<i64>,
    /// The object's timestamp.
    pub timestamp: Timestamp,
    /// Position in its zone (0 = first).
    pub zone_position: u32,
    /// Whether the object is a token.
    pub is_token: bool,
    /// Whether the object is face down.
    pub face_down: bool,
    /// Whether the object is tapped.
    pub tapped: bool,
    /// Whether the object is attacking.
    pub attacking: bool,
    /// Whether the object is blocking.
    pub blocking: bool,
    /// Whether the object entered the battlefield this turn.
    pub entered_this_turn: bool,
    /// Whether the object was cast.
    pub was_cast: bool,
    /// The keyword abilities the object has (evaluated).
    pub keywords: BTreeSet<String>,
    /// The designations the object has.
    pub designations: BTreeSet<String>,
    /// Counters on the object by kind.
    pub counters: BTreeMap<String, i64>,
}

impl ObjectView {
    /// A view with the given identity and characteristics (REQ-DSL-003): controller `None`;
    /// mana value 0; power, toughness, loyalty and defense `None`; `Timestamp::new(0)`; zone
    /// position 0; every flag false; keywords, designations and counters empty.
    #[must_use]
    pub fn new(
        id: ObjectId,
        zone: Zone,
        owner: PlayerId,
        characteristics: Characteristics,
    ) -> ObjectView {
        ObjectView {
            id,
            zone,
            owner,
            controller: None,
            characteristics,
            mana_value: 0,
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            timestamp: Timestamp::new(0),
            zone_position: 0,
            is_token: false,
            face_down: false,
            tapped: false,
            attacking: false,
            blocking: false,
            entered_this_turn: false,
            was_cast: false,
            keywords: BTreeSet::new(),
            designations: BTreeSet::new(),
            counters: BTreeMap::new(),
        }
    }
}

/// A numeric property of a player read through [`InterpEnv::player_stat`] (REQ-DSL-003).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerStat {
    /// Life total.
    Life,
    /// Speed.
    Speed,
    /// Number of cards in hand.
    HandSize,
    /// Number of cards in library.
    LibrarySize,
    /// Number of cards in graveyard.
    GraveyardSize,
    /// Number of counters of that kind on the player.
    Counters(String),
}

/// The game as the interpreter reads it (REQ-DSL-003). The integration unit implements it on
/// the real game state; tests implement it with synthetic fakes. Every method is a pure
/// function of the caller's state.
pub trait InterpEnv {
    /// Every player, in seat (turn) order.
    fn players(&self) -> Vec<PlayerId>;
    /// The active player, if any.
    fn active_player(&self) -> Option<PlayerId>;
    /// The current game turn number (the history ledger's key).
    fn turn_number(&self) -> u64;
    /// Every object currently in a zone of that kind, all owners, in any order.
    fn objects_in(&self, zone: Zone) -> Vec<ObjectId>;
    /// Current, layer-evaluated information about an object; `None` if it is not in a zone now.
    fn object(&self, id: ObjectId) -> Option<ObjectView>;
    /// Last known information about an object, if it was captured (CR 608.2h, CR 113.7a).
    fn lki(&self, id: ObjectId) -> Option<ObjectView>;
    /// A player's numeric property; `None` for an unknown player.
    fn player_stat(&self, player: PlayerId, stat: &PlayerStat) -> Option<i64>;
    /// A player's devotion to the given colors.
    fn devotion(&self, player: PlayerId, colors: &[String]) -> i64;
    /// Whether the player has the maximum speed.
    fn has_max_speed(&self, player: PlayerId) -> bool;
    /// The history ledger.
    fn history(&self) -> &History;
    /// The object `object` is attached to, if any.
    fn attached_to(&self, object: ObjectId) -> Option<ObjectId>;
    /// The player `attacker` is attacking, if any.
    fn defending_player(&self, attacker: ObjectId) -> Option<PlayerId>;
    /// The Memory record of an object, if any.
    fn memory(&self, object: ObjectId) -> Option<&Memory>;
    /// Spells cast this turn: the caster and the spell as it was cast.
    fn spells_cast_this_turn(&self) -> Vec<(PlayerId, ObjectView)>;
    /// Last-known-information views of the creatures that died this turn.
    fn creatures_died_this_turn(&self) -> Vec<ObjectView>;
    /// Mana spent to cast `spell`, only of that color when `color` is given.
    fn mana_spent_to_cast(&self, spell: ObjectId, color: Option<&str>) -> i64;
    /// The current value of a rewritable word token of `source` (SRS §14.5).
    fn text_token_value(&self, source: ObjectId, token_id: &str) -> Option<String>;
}

/// The context an expression is evaluated in (REQ-DSL-003): the ability's source and
/// controller, X, "that player", the chosen targets, chosen values, bound variables and linked
/// objects. Last known information is read through [`InterpEnv::lki`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalContext {
    /// The object the ability belongs to.
    pub source: Option<ObjectId>,
    /// The ability's controller.
    pub controller: PlayerId,
    /// The value of X, if any.
    pub x: Option<i64>,
    /// The player `THAT_PLAYER` names, if any.
    pub that_player: Option<PlayerId>,
    /// Targets by slot; `None` = no target chosen in that slot.
    pub targets: Vec<Option<Referent>>,
    /// Values chosen earlier, by key.
    pub chosen: BTreeMap<String, ChosenValue>,
    /// Bound variables; the key includes the `$` (e.g. `"$it"`).
    pub bindings: BTreeMap<String, Vec<Referent>>,
    /// Linked objects by link id.
    pub linked: BTreeMap<String, Vec<Referent>>,
}

impl EvalContext {
    /// A context for an ability controlled by `controller` (REQ-DSL-003): source, X and
    /// "that player" `None`; targets, chosen values, bindings and links empty.
    #[must_use]
    pub fn new(controller: PlayerId) -> EvalContext {
        EvalContext {
            source: None,
            controller,
            x: None,
            that_player: None,
            targets: Vec::new(),
            chosen: BTreeMap::new(),
            bindings: BTreeMap::new(),
            linked: BTreeMap::new(),
        }
    }
}

/// A part of an instruction that could not be performed and is ignored (REQ-ARCH-002,
/// CR 101.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ignored {
    /// The reference named nothing (or nothing of the kind required).
    Unresolved(ReferenceAST),
    /// The reference named something that is no longer there.
    Gone(Referent),
}

/// What a reference or selector resolved to, plus the parts ignored (REQ-ARCH-002,
/// CR 101.3): the rest of the instruction is still carried out with `items`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution<T> {
    /// What was resolved, in order.
    pub items: Vec<T>,
    /// The impossible parts, in order.
    pub ignored: Vec<Ignored>,
}

impl<T> Default for Resolution<T> {
    fn default() -> Self {
        Resolution {
            items: Vec::new(),
            ignored: Vec::new(),
        }
    }
}

/// A list of parts split into the ones performed and the ones ignored, each in input order
/// (REQ-ARCH-002, CR 101.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partition<T> {
    /// The parts that are possible, in input order.
    pub performed: Vec<T>,
    /// The parts that are impossible, in input order.
    pub ignored: Vec<T>,
}

/// A malformed expression (REQ-DSL-003). An impossible game instruction is never an error: it
/// is an [`Ignored`] part (REQ-ARCH-002).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InterpError {
    /// `BOUND($var)` with no binding of that variable.
    UnboundVariable(String),
    /// `X` with no value of X in the context.
    MissingX,
    /// `DIV_FLOOR` or `DIV_CEIL` by zero.
    DivisionByZero,
    /// A text token the source does not have (or no source to read it from).
    UnknownTextToken(String),
}

impl std::fmt::Display for InterpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterpError::UnboundVariable(v) => write!(f, "unbound variable {v}"),
            InterpError::MissingX => write!(f, "X has no value"),
            InterpError::DivisionByZero => write!(f, "division by zero"),
            InterpError::UnknownTextToken(t) => write!(f, "unknown text token {t}"),
        }
    }
}

impl std::error::Error for InterpError {}
