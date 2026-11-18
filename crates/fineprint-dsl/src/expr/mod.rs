// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! References, selectors, predicates and integer value expressions of the card DSL
//! (REQ-DSL-003, REQ-CORE-006, SRS §14.4.0).
//!
//! The JSON form of every type is the FP-011 wire contract (docs/DECISIONS.md, "#12 FP-011 —
//! Wire catalogue"). Decoding is strict: unknown and missing members, a present `null` on an
//! optional member, unknown identifier strings, empty free-form strings, non-integer numbers and
//! input nested deeper than [`crate::canonical::MAX_DEPTH`] are all errors, never panics. Every
//! value has exactly one JSON spelling, so, for every value nested within
//! [`crate::canonical::MAX_DEPTH`], `decode(encode(x)) == x`, and the RFC 8785 form of a decoded
//! document is the canonical form of the value.
//!
//! Serialisation re-checks the invariants a hand-built value could break (empty strings,
//! duplicate zones, an empty `AND`, a filter selector with neither zones nor a predicate) and
//! fails instead of writing JSON that would not decode because of those value-level invariants.
//! Nesting depth is the exception: these re-checks are value-level only and do not check it. A hand-built value nested deeper than
//! [`crate::canonical::MAX_DEPTH`] (127 open containers) can be written by a plain
//! `serde_json::to_vec`/`to_string` but will not decode. Callers who need output that is
//! guaranteed to reload must use [`crate::canonical::to_canonical_vec`], which checks depth.
//!
//! Everything numeric is an `i64` within ±(2^53 − 1) or a `u32` slot; nothing here is evaluated
//! (the interpreter is another unit).

use serde::ser::{SerializeMap, Serializer};
use serde::{Serialize, ser};
use serde_json::Value;

use self::json::{
    AsMap, FromJson, JResult, Members, Obj, array_of, check_bound_var, check_free_str, check_int,
    deserialize_via_from_json, free_str, int_of, ser_check, single_member, str_of, tagged, u32_of,
    wire_enum,
};

pub(crate) mod json;

#[cfg(test)]
pub(crate) mod tests;

wire_enum! {
    /// A game zone (REQ-DSL-003).
    Zone {
        /// `LIBRARY`.
        Library => "LIBRARY",
        /// `HAND`.
        Hand => "HAND",
        /// `BATTLEFIELD`.
        Battlefield => "BATTLEFIELD",
        /// `GRAVEYARD`.
        Graveyard => "GRAVEYARD",
        /// `STACK`.
        Stack => "STACK",
        /// `EXILE`.
        Exile => "EXILE",
        /// `COMMAND`.
        Command => "COMMAND",
        /// `ANTE`.
        Ante => "ANTE",
        /// `OUTSIDE_GAME`.
        OutsideGame => "OUTSIDE_GAME",
    }
}

wire_enum! {
    /// An integer comparison operator (REQ-DSL-003).
    Cmp {
        /// `EQ`: equal.
        Eq => "EQ",
        /// `NE`: not equal.
        Ne => "NE",
        /// `LT`: less than.
        Lt => "LT",
        /// `LE`: less than or equal.
        Le => "LE",
        /// `GT`: greater than.
        Gt => "GT",
        /// `GE`: greater than or equal.
        Ge => "GE",
    }
}

wire_enum! {
    /// An integer operator of `MATH` (REQ-DSL-003, REQ-CORE-006).
    MathOp {
        /// `ADD`.
        Add => "ADD",
        /// `SUB`.
        Sub => "SUB",
        /// `MUL`.
        Mul => "MUL",
        /// `DIV_FLOOR`: division rounded toward negative infinity.
        DivFloor => "DIV_FLOOR",
        /// `DIV_CEIL`: division rounded toward positive infinity.
        DivCeil => "DIV_CEIL",
        /// `MIN`.
        Min => "MIN",
        /// `MAX`.
        Max => "MAX",
    }
}

wire_enum! {
    /// A characteristic counted by `DISTINCT` (REQ-DSL-003).
    DistinctAttr {
        /// `CARD_TYPE`.
        CardType => "CARD_TYPE",
        /// `SUBTYPE`.
        Subtype => "SUBTYPE",
        /// `SUPERTYPE`.
        Supertype => "SUPERTYPE",
        /// `COLOR`.
        Color => "COLOR",
        /// `NAME`.
        Name => "NAME",
        /// `MANA_VALUE`.
        ManaValue => "MANA_VALUE",
        /// `POWER`.
        Power => "POWER",
        /// `TOUGHNESS`.
        Toughness => "TOUGHNESS",
    }
}

wire_enum! {
    /// The key a selector's `ordering` sorts by (REQ-DSL-003).
    OrderKey {
        /// `TIMESTAMP`.
        Timestamp => "TIMESTAMP",
        /// `MANA_VALUE`.
        ManaValue => "MANA_VALUE",
        /// `POWER`.
        Power => "POWER",
        /// `TOUGHNESS`.
        Toughness => "TOUGHNESS",
        /// `ZONE_POSITION`.
        ZonePosition => "ZONE_POSITION",
    }
}

wire_enum! {
    /// Sort direction of a selector's `ordering` (REQ-DSL-003).
    Direction {
        /// `ASC`: ascending.
        Asc => "ASC",
        /// `DESC`: descending.
        Desc => "DESC",
    }
}

wire_enum! {
    /// A group of players named without a reference (REQ-DSL-003).
    PlayerGroup {
        /// `ALL_PLAYERS`.
        AllPlayers => "ALL_PLAYERS",
        /// `TEAMMATES`.
        Teammates => "TEAMMATES",
    }
}

/// A word an AST node depends on (REQ-DSL-003, REQ-DSL-012, SRS §14.5): either a rewritable text
/// token of the face, by id (`{"textToken":"t14"}`), or a fixed name (`{"name":"..."}`). Names
/// are not checked against any vocabulary here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WordRef {
    /// `{"textToken": id}`: reads the current value of a text token (rewritable by
    /// text-changing effects, CR 612).
    TextToken(String),
    /// `{"name": word}`: a fixed word.
    Name(String),
}

/// A player or object named relative to the ability (REQ-DSL-003, SRS §14.4.0). JSON:
/// `{"ref":NAME}` plus the variant's parameter member, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReferenceAST {
    /// `SELF`: the object the ability belongs to.
    SelfRef,
    /// `SOURCE_LKI`: the source as it last existed.
    SourceLki,
    /// `CONTROLLER`.
    Controller,
    /// `OWNER`.
    Owner,
    /// `ACTIVE_PLAYER`.
    ActivePlayer,
    /// `DEFENDING_PLAYER`.
    DefendingPlayer,
    /// `ENCHANTED`.
    Enchanted,
    /// `EQUIPPED`.
    Equipped,
    /// `ATTACHED_TO`.
    AttachedTo,
    /// `THAT_PLAYER`.
    ThatPlayer,
    /// `EACH_OPPONENT`.
    EachOpponent,
    /// `TARGET` with member `slot`: the target in that slot.
    Target {
        /// Target slot index.
        slot: u32,
    },
    /// `CHOSEN` with member `key`: a value chosen earlier.
    Chosen {
        /// Choice key (non-empty).
        key: String,
    },
    /// `BOUND` with member `var`: a bound variable (`$` then `A-Z a-z 0-9 _`).
    Bound {
        /// Variable name including the `$`.
        var: String,
    },
    /// `LINKED` with member `linkId`: the object a linked ability refers to.
    Linked {
        /// Link id (non-empty).
        link_id: String,
    },
    /// `REMEMBERED` with member `key`.
    Remembered {
        /// Memory key (non-empty).
        key: String,
    },
}

/// Selects players (REQ-DSL-003): a reference (`{"ref":...}`) or a group
/// (`{"players":"ALL_PLAYERS"}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerSelectorAST {
    /// A reference; whether it names a player is the toolchain's check.
    Ref(ReferenceAST),
    /// A group of players.
    Players(PlayerGroup),
}

/// A selector's sort order (REQ-DSL-003): `{"by": OrderKey, "direction": Direction}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Ordering {
    /// Sort key.
    pub by: OrderKey,
    /// Sort direction.
    pub direction: Direction,
}

/// The filter form of an [`ObjectSelectorAST`] (REQ-DSL-003): at least one of `zones` and
/// `predicate`; the predicate's members are written flat in the selector object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorFilter {
    /// Zones searched (non-empty, distinct, order kept); `None` means the toolchain default.
    pub zones: Option<Vec<Zone>>,
    /// How many objects are picked.
    pub count: Option<Box<ValueExpressionAST>>,
    /// Sort order of the candidates.
    pub ordering: Option<Ordering>,
    /// The test each object must pass.
    pub predicate: Option<Box<PredicateAST>>,
}

/// Picks objects (REQ-DSL-003, SRS §14.4.0): a reference, or zones plus a predicate tree with an
/// optional `count` and `ordering`, e.g. `{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectSelectorAST {
    /// Reference form: the object is exactly a [`ReferenceAST`].
    Ref(ReferenceAST),
    /// Filter form.
    Filter(SelectorFilter),
}

/// A test on an object or player (REQ-DSL-003, SRS §14.4.0): a boolean `op` node or a `pred`
/// node with the members its name requires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PredicateAST {
    /// `{"op":"AND","conditions":[...]}` (at least one).
    And(Vec<PredicateAST>),
    /// `{"op":"OR","conditions":[...]}` (at least one).
    Or(Vec<PredicateAST>),
    /// `{"op":"NOT","condition":P}`.
    Not(Box<PredicateAST>),
    /// `IS_CREATURE`.
    IsCreature,
    /// `IS_LAND`.
    IsLand,
    /// `IS_PERMANENT`.
    IsPermanent,
    /// `IS_PLAYER`.
    IsPlayer,
    /// `IS_CARD_OR_TOKEN`.
    IsCardOrToken,
    /// `IS_TOKEN`.
    IsToken,
    /// `IS_COLORLESS`.
    IsColorless,
    /// `IS_MULTICOLORED`.
    IsMulticolored,
    /// `FACE_UP`.
    FaceUp,
    /// `FACE_DOWN`.
    FaceDown,
    /// `TAPPED`.
    Tapped,
    /// `UNTAPPED`.
    Untapped,
    /// `ATTACKING`.
    Attacking,
    /// `BLOCKING`.
    Blocking,
    /// `ENTERED_THIS_TURN`.
    EnteredThisTurn,
    /// `WAS_CAST`.
    WasCast,
    /// `HAS_CARD_TYPE` with `value`.
    HasCardType {
        /// The card type.
        value: WordRef,
    },
    /// `HAS_SUBTYPE` with `value`.
    HasSubtype {
        /// The subtype.
        value: WordRef,
    },
    /// `HAS_SUPERTYPE` with `value`.
    HasSupertype {
        /// The supertype.
        value: WordRef,
    },
    /// `HAS_COLOR` with `value`.
    HasColor {
        /// The color.
        value: WordRef,
    },
    /// `HAS_KEYWORD` with `keyword`.
    HasKeyword {
        /// The keyword (non-empty).
        keyword: String,
    },
    /// `HAS_DESIGNATION` with `kind`.
    HasDesignation {
        /// The designation (non-empty).
        kind: String,
    },
    /// `HAS_COUNTER` with `kind` and optionally both `cmp` and `value` (the count of those
    /// counters compared with the value); without them, "has at least one".
    HasCounter {
        /// Counter kind (non-empty).
        kind: String,
        /// Optional `(cmp, value)` comparison of the counter count.
        compare: Option<(Cmp, Box<ValueExpressionAST>)>,
    },
    /// `CONTROLLED_BY` with `player`.
    ControlledBy {
        /// The controller.
        player: PlayerSelectorAST,
    },
    /// `OWNED_BY` with `player`.
    OwnedBy {
        /// The owner.
        player: PlayerSelectorAST,
    },
    /// `MANA_VALUE` with `cmp` and `value`.
    ManaValue {
        /// Comparison.
        cmp: Cmp,
        /// Right-hand side.
        value: Box<ValueExpressionAST>,
    },
    /// `POWER` with `cmp` and `value`.
    Power {
        /// Comparison.
        cmp: Cmp,
        /// Right-hand side.
        value: Box<ValueExpressionAST>,
    },
    /// `TOUGHNESS` with `cmp` and `value`.
    Toughness {
        /// Comparison.
        cmp: Cmp,
        /// Right-hand side.
        value: Box<ValueExpressionAST>,
    },
    /// `SAME_AS` with a reference written flat (`{"pred":"SAME_AS","ref":"TARGET","slot":0}`).
    SameAs {
        /// The reference.
        reference: ReferenceAST,
    },
    /// `CONTROLS` with `selector` and optionally `player`.
    Controls {
        /// What must be controlled.
        selector: Box<ObjectSelectorAST>,
        /// Who must control it.
        player: Option<PlayerSelectorAST>,
    },
    /// `PLAYER_HAS_MAX_SPEED` with `player`.
    PlayerHasMaxSpeed {
        /// The player.
        player: PlayerSelectorAST,
    },
    /// `COMPARE` with `cmp`, `lhs` and `rhs`.
    Compare {
        /// Comparison.
        cmp: Cmp,
        /// Left-hand side.
        lhs: Box<ValueExpressionAST>,
        /// Right-hand side.
        rhs: Box<ValueExpressionAST>,
    },
    /// `TEXT_TOKEN_MATCH` with `textToken` and `value` (SRS §14.5): the token's current value
    /// equals `value`.
    TextTokenMatch {
        /// Text token id (non-empty).
        text_token: String,
        /// Expected value (non-empty).
        value: String,
    },
}

/// A characteristic of an object read by `ATTR` (REQ-DSL-003).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectAttr {
    /// `"POWER"`.
    Power,
    /// `"TOUGHNESS"`.
    Toughness,
    /// `"MANA_VALUE"`.
    ManaValue,
    /// `"LOYALTY"`.
    Loyalty,
    /// `"DEFENSE"`.
    Defense,
    /// `"COLOR_COUNT"`.
    ColorCount,
    /// `{"COUNTERS": kind}`: the number of counters of that kind.
    Counters(String),
}

/// A property of a player read by `PLAYER_ATTR` (REQ-DSL-003).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerAttr {
    /// `"LIFE"`.
    Life,
    /// `"SPEED"`.
    Speed,
    /// `"HAND_SIZE"`.
    HandSize,
    /// `"LIBRARY_SIZE"`.
    LibrarySize,
    /// `"GRAVEYARD_SIZE"`.
    GraveyardSize,
    /// `{"COUNTERS": kind}`.
    Counters(String),
    /// `{"DEVOTION": [color, ...]}` (at least one).
    Devotion(Vec<WordRef>),
}

/// A question about what happened earlier (REQ-DSL-003): externally tagged, one key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryQuery {
    /// `SPELLS_CAST_THIS_TURN` with optional `player` and `where`.
    SpellsCastThisTurn {
        /// Whose spells; `None` means every player's.
        player: Option<PlayerSelectorAST>,
        /// Which spells (JSON member `where`).
        condition: Option<PredicateAST>,
    },
    /// `LIFE_LOST_THIS_TURN` with required `player`.
    LifeLostThisTurn {
        /// The player.
        player: PlayerSelectorAST,
    },
    /// `LIFE_GAINED_THIS_TURN` with required `player`.
    LifeGainedThisTurn {
        /// The player.
        player: PlayerSelectorAST,
    },
    /// `CARDS_DRAWN_THIS_TURN` with required `player`.
    CardsDrawnThisTurn {
        /// The player.
        player: PlayerSelectorAST,
    },
    /// `CREATURES_DIED_THIS_TURN` with optional `where`.
    CreaturesDiedThisTurn {
        /// Which creatures (JSON member `where`).
        condition: Option<PredicateAST>,
    },
    /// `MANA_SPENT_TO_CAST` with a flat reference and optional `color`.
    ManaSpentToCast {
        /// The spell or permanent.
        reference: ReferenceAST,
        /// Only mana of this color.
        color: Option<WordRef>,
    },
}

/// An integer-valued expression (REQ-DSL-003, REQ-CORE-006, SRS §14.4.0): externally tagged with
/// one key, except `X`, which is the bare string `"X"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueExpressionAST {
    /// `{"LITERAL": n}` with `n` within ±(2^53 − 1).
    Literal(i64),
    /// `"X"`.
    X,
    /// `{"COUNT": selector}`.
    Count(Box<ObjectSelectorAST>),
    /// `{"ATTR": {ref members..., "attr": ObjectAttr}}`.
    Attr {
        /// Whose characteristic.
        reference: ReferenceAST,
        /// Which characteristic.
        attr: ObjectAttr,
    },
    /// `{"PLAYER_ATTR": {ref members..., "attr": PlayerAttr}}`.
    PlayerAttr {
        /// Which player.
        reference: ReferenceAST,
        /// Which property.
        attr: PlayerAttr,
    },
    /// `{"MATH": {"op": MathOp, "lhs": V, "rhs": V}}`.
    Math {
        /// Operator.
        op: MathOp,
        /// Left operand.
        lhs: Box<ValueExpressionAST>,
        /// Right operand.
        rhs: Box<ValueExpressionAST>,
    },
    /// `{"DISTINCT": {"attr": DistinctAttr, "selector": selector}}`.
    Distinct {
        /// What is counted.
        attr: DistinctAttr,
        /// Among which objects.
        selector: Box<ObjectSelectorAST>,
    },
    /// `{"HISTORY": HistoryQuery}`.
    History(Box<HistoryQuery>),
    /// `{"MEMORY": key}`.
    Memory(String),
}

// ---------------------------------------------------------------------------------------------
// WordRef

impl FromJson for WordRef {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "word reference")?;
        let out = if o.has("textToken") {
            WordRef::TextToken(free_str(o.req("textToken")?, "textToken")?)
        } else {
            WordRef::Name(free_str(o.req("name")?, "name")?)
        };
        o.finish()?;
        Ok(out)
    }
}

impl Members for WordRef {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        let (key, s) = match self {
            WordRef::TextToken(s) => ("textToken", s),
            WordRef::Name(s) => ("name", s),
        };
        ser_check(check_free_str(s, key))?;
        m.serialize_entry(key, s)
    }
}

impl Serialize for WordRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

// ---------------------------------------------------------------------------------------------
// ReferenceAST

impl ReferenceAST {
    /// The `ref` name of this reference, e.g. `"TARGET"` (REQ-DSL-003).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            ReferenceAST::SelfRef => "SELF",
            ReferenceAST::SourceLki => "SOURCE_LKI",
            ReferenceAST::Controller => "CONTROLLER",
            ReferenceAST::Owner => "OWNER",
            ReferenceAST::ActivePlayer => "ACTIVE_PLAYER",
            ReferenceAST::DefendingPlayer => "DEFENDING_PLAYER",
            ReferenceAST::Enchanted => "ENCHANTED",
            ReferenceAST::Equipped => "EQUIPPED",
            ReferenceAST::AttachedTo => "ATTACHED_TO",
            ReferenceAST::ThatPlayer => "THAT_PLAYER",
            ReferenceAST::EachOpponent => "EACH_OPPONENT",
            ReferenceAST::Target { .. } => "TARGET",
            ReferenceAST::Chosen { .. } => "CHOSEN",
            ReferenceAST::Bound { .. } => "BOUND",
            ReferenceAST::Linked { .. } => "LINKED",
            ReferenceAST::Remembered { .. } => "REMEMBERED",
        }
    }

    /// Consumes `ref` and the variant's parameter member from `o` (the flat style).
    fn take_from(o: &mut Obj<'_>) -> JResult<Self> {
        let name = str_of(o.req("ref")?, "ref")?;
        Ok(match name {
            "SELF" => ReferenceAST::SelfRef,
            "SOURCE_LKI" => ReferenceAST::SourceLki,
            "CONTROLLER" => ReferenceAST::Controller,
            "OWNER" => ReferenceAST::Owner,
            "ACTIVE_PLAYER" => ReferenceAST::ActivePlayer,
            "DEFENDING_PLAYER" => ReferenceAST::DefendingPlayer,
            "ENCHANTED" => ReferenceAST::Enchanted,
            "EQUIPPED" => ReferenceAST::Equipped,
            "ATTACHED_TO" => ReferenceAST::AttachedTo,
            "THAT_PLAYER" => ReferenceAST::ThatPlayer,
            "EACH_OPPONENT" => ReferenceAST::EachOpponent,
            "TARGET" => ReferenceAST::Target {
                slot: u32_of(o.req("slot")?, "slot")?,
            },
            "CHOSEN" => ReferenceAST::Chosen {
                key: free_str(o.req("key")?, "key")?,
            },
            "BOUND" => {
                let var = str_of(o.req("var")?, "var")?;
                check_bound_var(var)?;
                ReferenceAST::Bound {
                    var: var.to_owned(),
                }
            }
            "LINKED" => ReferenceAST::Linked {
                link_id: free_str(o.req("linkId")?, "linkId")?,
            },
            "REMEMBERED" => ReferenceAST::Remembered {
                key: free_str(o.req("key")?, "key")?,
            },
            other => return Err(format!("unknown reference {other:?}")),
        })
    }
}

impl FromJson for ReferenceAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "reference")?;
        let out = ReferenceAST::take_from(&mut o)?;
        o.finish()?;
        Ok(out)
    }
}

impl Members for ReferenceAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        m.serialize_entry("ref", self.name())?;
        match self {
            ReferenceAST::Target { slot } => m.serialize_entry("slot", slot),
            ReferenceAST::Chosen { key } | ReferenceAST::Remembered { key } => {
                ser_check(check_free_str(key, "key"))?;
                m.serialize_entry("key", key)
            }
            ReferenceAST::Bound { var } => {
                ser_check(check_bound_var(var))?;
                m.serialize_entry("var", var)
            }
            ReferenceAST::Linked { link_id } => {
                ser_check(check_free_str(link_id, "linkId"))?;
                m.serialize_entry("linkId", link_id)
            }
            _ => Ok(()),
        }
    }
}

impl Serialize for ReferenceAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

// ---------------------------------------------------------------------------------------------
// PlayerSelectorAST

impl FromJson for PlayerSelectorAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "player selector")?;
        let out = if o.has("ref") {
            PlayerSelectorAST::Ref(ReferenceAST::take_from(&mut o)?)
        } else {
            PlayerSelectorAST::Players(PlayerGroup::from_json(o.req("players")?)?)
        };
        o.finish()?;
        Ok(out)
    }
}

impl Members for PlayerSelectorAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        match self {
            PlayerSelectorAST::Ref(r) => r.members(m),
            PlayerSelectorAST::Players(g) => m.serialize_entry("players", g),
        }
    }
}

impl Serialize for PlayerSelectorAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

// ---------------------------------------------------------------------------------------------
// Ordering and zones

impl FromJson for Ordering {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "ordering")?;
        let out = Ordering {
            by: OrderKey::from_json(o.req("by")?)?,
            direction: Direction::from_json(o.req("direction")?)?,
        };
        o.finish()?;
        Ok(out)
    }
}

/// Validity of a `zones` list: non-empty and without repeats.
fn check_zones(zones: &[Zone]) -> JResult<()> {
    if zones.is_empty() {
        return Err("zones must not be empty".to_owned());
    }
    for (i, z) in zones.iter().enumerate() {
        if zones[..i].contains(z) {
            return Err(format!("zone {z} is listed twice"));
        }
    }
    Ok(())
}

/// Decodes a `zones` list.
fn zones_from(v: &Value) -> JResult<Vec<Zone>> {
    let zones = array_of(v, "zones")?
        .iter()
        .map(Zone::from_json)
        .collect::<JResult<Vec<_>>>()?;
    check_zones(&zones)?;
    Ok(zones)
}

// ---------------------------------------------------------------------------------------------
// ObjectSelectorAST

impl FromJson for ObjectSelectorAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "object selector")?;
        if o.has("ref") && !o.has("pred") && !o.has("op") {
            let r = ReferenceAST::take_from(&mut o)?;
            o.finish()?;
            return Ok(ObjectSelectorAST::Ref(r));
        }
        let filter = SelectorFilter::take_from(&mut o)?;
        o.finish()?;
        Ok(ObjectSelectorAST::Filter(filter))
    }
}

impl SelectorFilter {
    /// Consumes the filter members (and the flat predicate) from `o`.
    fn take_from(o: &mut Obj<'_>) -> JResult<Self> {
        let zones = o.opt("zones")?.map(zones_from).transpose()?;
        let count = o
            .opt("count")?
            .map(|c| ValueExpressionAST::from_json(c).map(Box::new))
            .transpose()?;
        let ordering = o.opt("ordering")?.map(Ordering::from_json).transpose()?;
        let predicate = if o.has("pred") || o.has("op") {
            Some(Box::new(PredicateAST::take_from(o)?))
        } else {
            None
        };
        let filter = SelectorFilter {
            zones,
            count,
            ordering,
            predicate,
        };
        check_filter(&filter)?;
        Ok(filter)
    }
}

impl FromJson for SelectorFilter {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "selector filter")?;
        let out = SelectorFilter::take_from(&mut o)?;
        o.finish()?;
        Ok(out)
    }
}

impl Members for SelectorFilter {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(check_filter(self))?;
        if let Some(zones) = &self.zones {
            m.serialize_entry("zones", zones)?;
        }
        if let Some(count) = &self.count {
            m.serialize_entry("count", count)?;
        }
        if let Some(ordering) = &self.ordering {
            m.serialize_entry("ordering", ordering)?;
        }
        match &self.predicate {
            Some(p) => p.members(m),
            None => Ok(()),
        }
    }
}

impl Serialize for SelectorFilter {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// Validity of a filter selector: `zones` or a predicate, and a valid zone list.
fn check_filter(f: &SelectorFilter) -> JResult<()> {
    if let Some(zones) = &f.zones {
        check_zones(zones)?;
    }
    if f.zones.is_none() && f.predicate.is_none() {
        return Err("a filter selector needs `zones` or a predicate".to_owned());
    }
    Ok(())
}

impl Members for ObjectSelectorAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        match self {
            ObjectSelectorAST::Ref(r) => r.members(m),
            ObjectSelectorAST::Filter(f) => f.members(m),
        }
    }
}

impl Serialize for ObjectSelectorAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

// ---------------------------------------------------------------------------------------------
// PredicateAST

impl PredicateAST {
    /// The `pred` name, or the `op` name for `AND`/`OR`/`NOT` (REQ-DSL-003).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            PredicateAST::And(_) => "AND",
            PredicateAST::Or(_) => "OR",
            PredicateAST::Not(_) => "NOT",
            PredicateAST::IsCreature => "IS_CREATURE",
            PredicateAST::IsLand => "IS_LAND",
            PredicateAST::IsPermanent => "IS_PERMANENT",
            PredicateAST::IsPlayer => "IS_PLAYER",
            PredicateAST::IsCardOrToken => "IS_CARD_OR_TOKEN",
            PredicateAST::IsToken => "IS_TOKEN",
            PredicateAST::IsColorless => "IS_COLORLESS",
            PredicateAST::IsMulticolored => "IS_MULTICOLORED",
            PredicateAST::FaceUp => "FACE_UP",
            PredicateAST::FaceDown => "FACE_DOWN",
            PredicateAST::Tapped => "TAPPED",
            PredicateAST::Untapped => "UNTAPPED",
            PredicateAST::Attacking => "ATTACKING",
            PredicateAST::Blocking => "BLOCKING",
            PredicateAST::EnteredThisTurn => "ENTERED_THIS_TURN",
            PredicateAST::WasCast => "WAS_CAST",
            PredicateAST::HasCardType { .. } => "HAS_CARD_TYPE",
            PredicateAST::HasSubtype { .. } => "HAS_SUBTYPE",
            PredicateAST::HasSupertype { .. } => "HAS_SUPERTYPE",
            PredicateAST::HasColor { .. } => "HAS_COLOR",
            PredicateAST::HasKeyword { .. } => "HAS_KEYWORD",
            PredicateAST::HasDesignation { .. } => "HAS_DESIGNATION",
            PredicateAST::HasCounter { .. } => "HAS_COUNTER",
            PredicateAST::ControlledBy { .. } => "CONTROLLED_BY",
            PredicateAST::OwnedBy { .. } => "OWNED_BY",
            PredicateAST::ManaValue { .. } => "MANA_VALUE",
            PredicateAST::Power { .. } => "POWER",
            PredicateAST::Toughness { .. } => "TOUGHNESS",
            PredicateAST::SameAs { .. } => "SAME_AS",
            PredicateAST::Controls { .. } => "CONTROLS",
            PredicateAST::PlayerHasMaxSpeed { .. } => "PLAYER_HAS_MAX_SPEED",
            PredicateAST::Compare { .. } => "COMPARE",
            PredicateAST::TextTokenMatch { .. } => "TEXT_TOKEN_MATCH",
        }
    }

    /// Consumes this predicate's members from `o` (the predicate may sit flat in a selector).
    fn take_from(o: &mut Obj<'_>) -> JResult<Self> {
        if o.has("op") && o.has("pred") {
            return Err("a predicate has either `op` or `pred`, not both".to_owned());
        }
        if o.has("op") {
            return match str_of(o.req("op")?, "op")? {
                "AND" => Ok(PredicateAST::And(conditions(o)?)),
                "OR" => Ok(PredicateAST::Or(conditions(o)?)),
                "NOT" => Ok(PredicateAST::Not(Box::new(PredicateAST::from_json(
                    o.req("condition")?,
                )?))),
                other => Err(format!("unknown predicate op {other:?}")),
            };
        }
        let value = |o: &mut Obj<'_>| -> JResult<Box<ValueExpressionAST>> {
            ValueExpressionAST::from_json(o.req("value")?).map(Box::new)
        };
        let word = |o: &mut Obj<'_>| WordRef::from_json(o.req("value")?);
        let player = |o: &mut Obj<'_>| PlayerSelectorAST::from_json(o.req("player")?);
        let cmp = |o: &mut Obj<'_>| Cmp::from_json(o.req("cmp")?);
        Ok(match str_of(o.req("pred")?, "pred")? {
            "IS_CREATURE" => PredicateAST::IsCreature,
            "IS_LAND" => PredicateAST::IsLand,
            "IS_PERMANENT" => PredicateAST::IsPermanent,
            "IS_PLAYER" => PredicateAST::IsPlayer,
            "IS_CARD_OR_TOKEN" => PredicateAST::IsCardOrToken,
            "IS_TOKEN" => PredicateAST::IsToken,
            "IS_COLORLESS" => PredicateAST::IsColorless,
            "IS_MULTICOLORED" => PredicateAST::IsMulticolored,
            "FACE_UP" => PredicateAST::FaceUp,
            "FACE_DOWN" => PredicateAST::FaceDown,
            "TAPPED" => PredicateAST::Tapped,
            "UNTAPPED" => PredicateAST::Untapped,
            "ATTACKING" => PredicateAST::Attacking,
            "BLOCKING" => PredicateAST::Blocking,
            "ENTERED_THIS_TURN" => PredicateAST::EnteredThisTurn,
            "WAS_CAST" => PredicateAST::WasCast,
            "HAS_CARD_TYPE" => PredicateAST::HasCardType { value: word(o)? },
            "HAS_SUBTYPE" => PredicateAST::HasSubtype { value: word(o)? },
            "HAS_SUPERTYPE" => PredicateAST::HasSupertype { value: word(o)? },
            "HAS_COLOR" => PredicateAST::HasColor { value: word(o)? },
            "HAS_KEYWORD" => PredicateAST::HasKeyword {
                keyword: free_str(o.req("keyword")?, "keyword")?,
            },
            "HAS_DESIGNATION" => PredicateAST::HasDesignation {
                kind: free_str(o.req("kind")?, "kind")?,
            },
            "HAS_COUNTER" => {
                let kind = free_str(o.req("kind")?, "kind")?;
                let compare = match (o.opt("cmp")?, o.opt("value")?) {
                    (None, None) => None,
                    (Some(c), Some(v)) => Some((
                        Cmp::from_json(c)?,
                        Box::new(ValueExpressionAST::from_json(v)?),
                    )),
                    _ => {
                        return Err(
                            "HAS_COUNTER needs both `cmp` and `value`, or neither".to_owned()
                        );
                    }
                };
                PredicateAST::HasCounter { kind, compare }
            }
            "CONTROLLED_BY" => PredicateAST::ControlledBy { player: player(o)? },
            "OWNED_BY" => PredicateAST::OwnedBy { player: player(o)? },
            "MANA_VALUE" => PredicateAST::ManaValue {
                cmp: cmp(o)?,
                value: value(o)?,
            },
            "POWER" => PredicateAST::Power {
                cmp: cmp(o)?,
                value: value(o)?,
            },
            "TOUGHNESS" => PredicateAST::Toughness {
                cmp: cmp(o)?,
                value: value(o)?,
            },
            "SAME_AS" => PredicateAST::SameAs {
                reference: ReferenceAST::take_from(o)?,
            },
            "CONTROLS" => PredicateAST::Controls {
                selector: Box::new(ObjectSelectorAST::from_json(o.req("selector")?)?),
                player: o
                    .opt("player")?
                    .map(PlayerSelectorAST::from_json)
                    .transpose()?,
            },
            "PLAYER_HAS_MAX_SPEED" => PredicateAST::PlayerHasMaxSpeed { player: player(o)? },
            "COMPARE" => PredicateAST::Compare {
                cmp: cmp(o)?,
                lhs: Box::new(ValueExpressionAST::from_json(o.req("lhs")?)?),
                rhs: Box::new(ValueExpressionAST::from_json(o.req("rhs")?)?),
            },
            "TEXT_TOKEN_MATCH" => PredicateAST::TextTokenMatch {
                text_token: free_str(o.req("textToken")?, "textToken")?,
                value: free_str(o.req("value")?, "value")?,
            },
            other => return Err(format!("unknown predicate {other:?}")),
        })
    }
}

/// Decodes the non-empty `conditions` of `AND`/`OR`.
fn conditions(o: &mut Obj<'_>) -> JResult<Vec<PredicateAST>> {
    let items = array_of(o.req("conditions")?, "conditions")?;
    if items.is_empty() {
        return Err("conditions must hold at least one predicate".to_owned());
    }
    items.iter().map(PredicateAST::from_json).collect()
}

impl FromJson for PredicateAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "predicate")?;
        let out = PredicateAST::take_from(&mut o)?;
        o.finish()?;
        Ok(out)
    }
}

impl Members for PredicateAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        match self {
            PredicateAST::And(cs) | PredicateAST::Or(cs) => {
                if cs.is_empty() {
                    return Err(ser::Error::custom(
                        "conditions must hold at least one predicate",
                    ));
                }
                m.serialize_entry("op", self.name())?;
                return m.serialize_entry("conditions", cs);
            }
            PredicateAST::Not(c) => {
                m.serialize_entry("op", "NOT")?;
                return m.serialize_entry("condition", c);
            }
            _ => {}
        }
        m.serialize_entry("pred", self.name())?;
        match self {
            PredicateAST::HasCardType { value }
            | PredicateAST::HasSubtype { value }
            | PredicateAST::HasSupertype { value }
            | PredicateAST::HasColor { value } => m.serialize_entry("value", value),
            PredicateAST::HasKeyword { keyword } => {
                ser_check(check_free_str(keyword, "keyword"))?;
                m.serialize_entry("keyword", keyword)
            }
            PredicateAST::HasDesignation { kind } => {
                ser_check(check_free_str(kind, "kind"))?;
                m.serialize_entry("kind", kind)
            }
            PredicateAST::HasCounter { kind, compare } => {
                ser_check(check_free_str(kind, "kind"))?;
                m.serialize_entry("kind", kind)?;
                if let Some((cmp, value)) = compare {
                    m.serialize_entry("cmp", cmp)?;
                    m.serialize_entry("value", value)?;
                }
                Ok(())
            }
            PredicateAST::ControlledBy { player }
            | PredicateAST::OwnedBy { player }
            | PredicateAST::PlayerHasMaxSpeed { player } => m.serialize_entry("player", player),
            PredicateAST::ManaValue { cmp, value }
            | PredicateAST::Power { cmp, value }
            | PredicateAST::Toughness { cmp, value } => {
                m.serialize_entry("cmp", cmp)?;
                m.serialize_entry("value", value)
            }
            PredicateAST::SameAs { reference } => reference.members(m),
            PredicateAST::Controls { selector, player } => {
                m.serialize_entry("selector", selector)?;
                if let Some(p) = player {
                    m.serialize_entry("player", p)?;
                }
                Ok(())
            }
            PredicateAST::Compare { cmp, lhs, rhs } => {
                m.serialize_entry("cmp", cmp)?;
                m.serialize_entry("lhs", lhs)?;
                m.serialize_entry("rhs", rhs)
            }
            PredicateAST::TextTokenMatch { text_token, value } => {
                ser_check(check_free_str(text_token, "textToken"))?;
                ser_check(check_free_str(value, "value"))?;
                m.serialize_entry("textToken", text_token)?;
                m.serialize_entry("value", value)
            }
            _ => Ok(()),
        }
    }
}

impl Serialize for PredicateAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

// ---------------------------------------------------------------------------------------------
// ObjectAttr / PlayerAttr

impl FromJson for ObjectAttr {
    fn from_json(v: &Value) -> JResult<Self> {
        if let Value::String(s) = v {
            return match s.as_str() {
                "POWER" => Ok(ObjectAttr::Power),
                "TOUGHNESS" => Ok(ObjectAttr::Toughness),
                "MANA_VALUE" => Ok(ObjectAttr::ManaValue),
                "LOYALTY" => Ok(ObjectAttr::Loyalty),
                "DEFENSE" => Ok(ObjectAttr::Defense),
                "COLOR_COUNT" => Ok(ObjectAttr::ColorCount),
                other => Err(format!("unknown object attr {other:?}")),
            };
        }
        match single_member(v, "object attr")? {
            ("COUNTERS", kind) => Ok(ObjectAttr::Counters(free_str(kind, "COUNTERS")?)),
            (other, _) => Err(format!("unknown object attr {other:?}")),
        }
    }
}

impl Serialize for ObjectAttr {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let name = match self {
            ObjectAttr::Power => "POWER",
            ObjectAttr::Toughness => "TOUGHNESS",
            ObjectAttr::ManaValue => "MANA_VALUE",
            ObjectAttr::Loyalty => "LOYALTY",
            ObjectAttr::Defense => "DEFENSE",
            ObjectAttr::ColorCount => "COLOR_COUNT",
            ObjectAttr::Counters(kind) => {
                ser_check(check_free_str(kind, "COUNTERS"))?;
                return tagged(s, "COUNTERS", kind);
            }
        };
        s.serialize_str(name)
    }
}

impl FromJson for PlayerAttr {
    fn from_json(v: &Value) -> JResult<Self> {
        if let Value::String(s) = v {
            return match s.as_str() {
                "LIFE" => Ok(PlayerAttr::Life),
                "SPEED" => Ok(PlayerAttr::Speed),
                "HAND_SIZE" => Ok(PlayerAttr::HandSize),
                "LIBRARY_SIZE" => Ok(PlayerAttr::LibrarySize),
                "GRAVEYARD_SIZE" => Ok(PlayerAttr::GraveyardSize),
                other => Err(format!("unknown player attr {other:?}")),
            };
        }
        match single_member(v, "player attr")? {
            ("COUNTERS", kind) => Ok(PlayerAttr::Counters(free_str(kind, "COUNTERS")?)),
            ("DEVOTION", colors) => {
                let colors = array_of(colors, "DEVOTION")?
                    .iter()
                    .map(WordRef::from_json)
                    .collect::<JResult<Vec<_>>>()?;
                if colors.is_empty() {
                    return Err("DEVOTION needs at least one color".to_owned());
                }
                Ok(PlayerAttr::Devotion(colors))
            }
            (other, _) => Err(format!("unknown player attr {other:?}")),
        }
    }
}

impl Serialize for PlayerAttr {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let name = match self {
            PlayerAttr::Life => "LIFE",
            PlayerAttr::Speed => "SPEED",
            PlayerAttr::HandSize => "HAND_SIZE",
            PlayerAttr::LibrarySize => "LIBRARY_SIZE",
            PlayerAttr::GraveyardSize => "GRAVEYARD_SIZE",
            PlayerAttr::Counters(kind) => {
                ser_check(check_free_str(kind, "COUNTERS"))?;
                return tagged(s, "COUNTERS", kind);
            }
            PlayerAttr::Devotion(colors) => {
                if colors.is_empty() {
                    return Err(ser::Error::custom("DEVOTION needs at least one color"));
                }
                return tagged(s, "DEVOTION", colors);
            }
        };
        s.serialize_str(name)
    }
}

// ---------------------------------------------------------------------------------------------
// HistoryQuery

impl HistoryQuery {
    /// The tag of this query, e.g. `"LIFE_LOST_THIS_TURN"` (REQ-DSL-003).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            HistoryQuery::SpellsCastThisTurn { .. } => "SPELLS_CAST_THIS_TURN",
            HistoryQuery::LifeLostThisTurn { .. } => "LIFE_LOST_THIS_TURN",
            HistoryQuery::LifeGainedThisTurn { .. } => "LIFE_GAINED_THIS_TURN",
            HistoryQuery::CardsDrawnThisTurn { .. } => "CARDS_DRAWN_THIS_TURN",
            HistoryQuery::CreaturesDiedThisTurn { .. } => "CREATURES_DIED_THIS_TURN",
            HistoryQuery::ManaSpentToCast { .. } => "MANA_SPENT_TO_CAST",
        }
    }
}

impl FromJson for HistoryQuery {
    fn from_json(v: &Value) -> JResult<Self> {
        let (tag, body) = single_member(v, "history query")?;
        let mut o = Obj::new(body, "history query body")?;
        let player = |o: &mut Obj<'_>| PlayerSelectorAST::from_json(o.req("player")?);
        let condition = |o: &mut Obj<'_>| -> JResult<Option<PredicateAST>> {
            o.opt("where")?.map(PredicateAST::from_json).transpose()
        };
        let out = match tag {
            "SPELLS_CAST_THIS_TURN" => HistoryQuery::SpellsCastThisTurn {
                player: o
                    .opt("player")?
                    .map(PlayerSelectorAST::from_json)
                    .transpose()?,
                condition: condition(&mut o)?,
            },
            "LIFE_LOST_THIS_TURN" => HistoryQuery::LifeLostThisTurn {
                player: player(&mut o)?,
            },
            "LIFE_GAINED_THIS_TURN" => HistoryQuery::LifeGainedThisTurn {
                player: player(&mut o)?,
            },
            "CARDS_DRAWN_THIS_TURN" => HistoryQuery::CardsDrawnThisTurn {
                player: player(&mut o)?,
            },
            "CREATURES_DIED_THIS_TURN" => HistoryQuery::CreaturesDiedThisTurn {
                condition: condition(&mut o)?,
            },
            "MANA_SPENT_TO_CAST" => HistoryQuery::ManaSpentToCast {
                reference: ReferenceAST::take_from(&mut o)?,
                color: o.opt("color")?.map(WordRef::from_json).transpose()?,
            },
            other => return Err(format!("unknown history query {other:?}")),
        };
        o.finish()?;
        Ok(out)
    }
}

/// The body object of a [`HistoryQuery`].
struct HistoryBody<'a>(&'a HistoryQuery);

impl Members for HistoryBody<'_> {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        match self.0 {
            HistoryQuery::SpellsCastThisTurn { player, condition } => {
                if let Some(p) = player {
                    m.serialize_entry("player", p)?;
                }
                if let Some(c) = condition {
                    m.serialize_entry("where", c)?;
                }
                Ok(())
            }
            HistoryQuery::LifeLostThisTurn { player }
            | HistoryQuery::LifeGainedThisTurn { player }
            | HistoryQuery::CardsDrawnThisTurn { player } => m.serialize_entry("player", player),
            HistoryQuery::CreaturesDiedThisTurn { condition } => match condition {
                Some(c) => m.serialize_entry("where", c),
                None => Ok(()),
            },
            HistoryQuery::ManaSpentToCast { reference, color } => {
                reference.members(m)?;
                match color {
                    Some(c) => m.serialize_entry("color", c),
                    None => Ok(()),
                }
            }
        }
    }
}

impl Serialize for HistoryQuery {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        tagged(s, self.name(), &AsMap(&HistoryBody(self)))
    }
}

// ---------------------------------------------------------------------------------------------
// ValueExpressionAST

impl ValueExpressionAST {
    /// The wire tag of this expression (`"LITERAL"`, `"X"`, `"COUNT"`, ...) (REQ-DSL-003).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            ValueExpressionAST::Literal(_) => "LITERAL",
            ValueExpressionAST::X => "X",
            ValueExpressionAST::Count(_) => "COUNT",
            ValueExpressionAST::Attr { .. } => "ATTR",
            ValueExpressionAST::PlayerAttr { .. } => "PLAYER_ATTR",
            ValueExpressionAST::Math { .. } => "MATH",
            ValueExpressionAST::Distinct { .. } => "DISTINCT",
            ValueExpressionAST::History(_) => "HISTORY",
            ValueExpressionAST::Memory(_) => "MEMORY",
        }
    }
}

impl FromJson for ValueExpressionAST {
    fn from_json(v: &Value) -> JResult<Self> {
        if let Value::String(s) = v {
            return if s == "X" {
                Ok(ValueExpressionAST::X)
            } else {
                Err(format!(
                    "a value expression string must be \"X\", found {s:?}"
                ))
            };
        }
        let (tag, body) = single_member(v, "value expression")?;
        Ok(match tag {
            "LITERAL" => ValueExpressionAST::Literal(int_of(body, "LITERAL")?),
            "COUNT" => ValueExpressionAST::Count(Box::new(ObjectSelectorAST::from_json(body)?)),
            "ATTR" => {
                let mut o = Obj::new(body, "ATTR")?;
                let reference = ReferenceAST::take_from(&mut o)?;
                let attr = ObjectAttr::from_json(o.req("attr")?)?;
                o.finish()?;
                ValueExpressionAST::Attr { reference, attr }
            }
            "PLAYER_ATTR" => {
                let mut o = Obj::new(body, "PLAYER_ATTR")?;
                let reference = ReferenceAST::take_from(&mut o)?;
                let attr = PlayerAttr::from_json(o.req("attr")?)?;
                o.finish()?;
                ValueExpressionAST::PlayerAttr { reference, attr }
            }
            "MATH" => {
                let mut o = Obj::new(body, "MATH")?;
                let op = MathOp::from_json(o.req("op")?)?;
                let lhs = Box::new(ValueExpressionAST::from_json(o.req("lhs")?)?);
                let rhs = Box::new(ValueExpressionAST::from_json(o.req("rhs")?)?);
                o.finish()?;
                ValueExpressionAST::Math { op, lhs, rhs }
            }
            "DISTINCT" => {
                let mut o = Obj::new(body, "DISTINCT")?;
                let attr = DistinctAttr::from_json(o.req("attr")?)?;
                let selector = Box::new(ObjectSelectorAST::from_json(o.req("selector")?)?);
                o.finish()?;
                ValueExpressionAST::Distinct { attr, selector }
            }
            "HISTORY" => ValueExpressionAST::History(Box::new(HistoryQuery::from_json(body)?)),
            "MEMORY" => ValueExpressionAST::Memory(free_str(body, "MEMORY")?),
            other => return Err(format!("unknown value expression {other:?}")),
        })
    }
}

/// The body object of `ATTR`, `PLAYER_ATTR`, `MATH` and `DISTINCT`.
struct ValueBody<'a>(&'a ValueExpressionAST);

impl Members for ValueBody<'_> {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        match self.0 {
            ValueExpressionAST::Attr { reference, attr } => {
                reference.members(m)?;
                m.serialize_entry("attr", attr)
            }
            ValueExpressionAST::PlayerAttr { reference, attr } => {
                reference.members(m)?;
                m.serialize_entry("attr", attr)
            }
            ValueExpressionAST::Math { op, lhs, rhs } => {
                m.serialize_entry("op", op)?;
                m.serialize_entry("lhs", lhs)?;
                m.serialize_entry("rhs", rhs)
            }
            ValueExpressionAST::Distinct { attr, selector } => {
                m.serialize_entry("attr", attr)?;
                m.serialize_entry("selector", selector)
            }
            _ => Ok(()),
        }
    }
}

impl Serialize for ValueExpressionAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ValueExpressionAST::Literal(i) => {
                ser_check(check_int(*i, "LITERAL"))?;
                tagged(s, "LITERAL", i)
            }
            ValueExpressionAST::X => s.serialize_str("X"),
            ValueExpressionAST::Count(sel) => tagged(s, "COUNT", sel),
            ValueExpressionAST::History(q) => tagged(s, "HISTORY", q),
            ValueExpressionAST::Memory(key) => {
                ser_check(check_free_str(key, "MEMORY"))?;
                tagged(s, "MEMORY", key)
            }
            ValueExpressionAST::Attr { .. }
            | ValueExpressionAST::PlayerAttr { .. }
            | ValueExpressionAST::Math { .. }
            | ValueExpressionAST::Distinct { .. } => {
                tagged(s, self.name(), &AsMap(&ValueBody(self)))
            }
        }
    }
}

deserialize_via_from_json!(
    WordRef,
    ReferenceAST,
    PlayerSelectorAST,
    Ordering,
    SelectorFilter,
    ObjectSelectorAST,
    PredicateAST,
    ObjectAttr,
    PlayerAttr,
    HistoryQuery,
    ValueExpressionAST,
);
