// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The plain data of a parsed `.pzl` scenario (REQ-TEST-001).
//!
//! Every type here is plain data: `Clone + Debug + PartialEq` and serde (camelCase JSON), so a
//! parsed scenario re-serialises with [`Scenario::to_json`] and parses back to an equal value.
//! Maps are `BTreeMap`s (R3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One `.pzl` scenario (REQ-TEST-001 item 1): the header, the exact starting state
/// ([`Setup`]), the scripted choices and the assertions.
///
/// In JSON the members of [`Setup`] sit at the top level next to the header (serde goes
/// through a flat wire form, so no buffering of untyped values is needed).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "ScenarioWire", into = "ScenarioWire")]
pub struct Scenario {
    /// The format version; always 1.
    pub pzl_version: u32,
    /// A one-line title (1 to 200 characters).
    pub title: String,
    /// The `REQ-*` ids the scenario covers; empty only with a [`Ruling`].
    pub requirements: Vec<String>,
    /// The CR rule numbers the scenario exercises (free-form strings, may be empty).
    pub cr: Vec<String>,
    /// The ruling the scenario reproduces, if any.
    pub ruling: Option<Ruling>,
    /// The exact starting state (its members sit at the top level of the file).
    pub setup: Setup,
    /// The scripted answers, in order.
    pub script: Vec<ScriptStep>,
    /// The assertions, evaluated in order after the script.
    pub assertions: Vec<Assertion>,
}

/// The flat JSON form of a [`Scenario`].
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScenarioWire {
    pzl_version: u32,
    title: String,
    requirements: Vec<String>,
    cr: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ruling: Option<Ruling>,
    format: String,
    rules_baseline: String,
    card_database: CardDatabase,
    prng_seed: u64,
    players: Vec<PlayerSetup>,
    #[serde(default)]
    battlefield: Vec<ObjectSetup>,
    #[serde(default)]
    stack: Vec<StackSetup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    shared_exile: Vec<ObjectSetup>,
    turn: TurnSetup,
    priority: Option<String>,
    #[serde(default)]
    script: Vec<ScriptStep>,
    assertions: Vec<Assertion>,
}

impl From<ScenarioWire> for Scenario {
    fn from(w: ScenarioWire) -> Scenario {
        Scenario {
            pzl_version: w.pzl_version,
            title: w.title,
            requirements: w.requirements,
            cr: w.cr,
            ruling: w.ruling,
            setup: Setup {
                format: w.format,
                rules_baseline: w.rules_baseline,
                card_database: w.card_database,
                prng_seed: w.prng_seed,
                players: w.players,
                battlefield: w.battlefield,
                stack: w.stack,
                shared_exile: w.shared_exile,
                turn: w.turn,
                priority: w.priority,
            },
            script: w.script,
            assertions: w.assertions,
        }
    }
}

impl From<Scenario> for ScenarioWire {
    fn from(s: Scenario) -> ScenarioWire {
        let Setup {
            format,
            rules_baseline,
            card_database,
            prng_seed,
            players,
            battlefield,
            stack,
            shared_exile,
            turn,
            priority,
        } = s.setup;
        ScenarioWire {
            pzl_version: s.pzl_version,
            title: s.title,
            requirements: s.requirements,
            cr: s.cr,
            ruling: s.ruling,
            format,
            rules_baseline,
            card_database,
            prng_seed,
            players,
            battlefield,
            stack,
            shared_exile,
            turn,
            priority,
            script: s.script,
            assertions: s.assertions,
        }
    }
}

impl Scenario {
    /// The canonical JSON text of this scenario (UTF-8, object members in schema order,
    /// integers only). [`parse_scenario`](super::parse_scenario) of the result equals `self`.
    #[must_use]
    pub fn to_json(&self) -> String {
        // Serialising plain data with string keys cannot fail.
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// A ruling reference (REQ-TEST-001).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ruling {
    /// The ruling date, `YYYY-MM-DD`.
    pub date: String,
    /// A free-form reference.
    #[serde(rename = "ref")]
    pub reference: String,
}

/// The exact starting state of a scenario (REQ-TEST-001 item 1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    /// The format id (for example a `FormatConfig` preset id).
    pub format: String,
    /// The rules baseline the scenario is written against.
    pub rules_baseline: String,
    /// The card database version and the definition hashes the scenario requires.
    pub card_database: CardDatabase,
    /// The PRNG seed.
    pub prng_seed: u64,
    /// The players, in turn order (1 to 8).
    pub players: Vec<PlayerSetup>,
    /// The permanents on the battlefield.
    #[serde(default)]
    pub battlefield: Vec<ObjectSetup>,
    /// The stack, BOTTOM FIRST (the last entry is the top).
    #[serde(default)]
    pub stack: Vec<StackSetup>,
    /// Exiled objects not listed under a player.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_exile: Vec<ObjectSetup>,
    /// The turn, phase and step.
    pub turn: TurnSetup,
    /// The player who holds priority, or `None`.
    pub priority: Option<String>,
}

impl Setup {
    /// Every object of the setup with its id (declared or runner-assigned), card, owner and
    /// zone, in declaration order: each player's library, hand, graveyard, exile and command
    /// zone, then the battlefield, the stack and the shared exile.
    ///
    /// An unnamed object (a bare cardDefId string) gets the id `<player>.<zone>.<index>`, for
    /// example `p1.library.0` (REQ-TEST-001).
    #[must_use]
    pub fn objects(&self) -> Vec<SetupObject> {
        let mut out = Vec::new();
        for p in &self.players {
            for (zone, list) in p.zones.lists() {
                for (i, r) in list.iter().enumerate() {
                    out.push(SetupObject {
                        id: r.id().map_or_else(
                            || format!("{}.{}.{i}", p.id, zone.as_str()),
                            str::to_owned,
                        ),
                        card: r.card().to_owned(),
                        owner: p.id.clone(),
                        zone,
                    });
                }
            }
        }
        for o in &self.battlefield {
            out.push(SetupObject {
                id: o.id.clone(),
                card: o.card.clone(),
                owner: o.owner.clone(),
                zone: ZoneName::Battlefield,
            });
        }
        for o in &self.stack {
            out.push(SetupObject {
                id: o.id.clone(),
                card: o.card.clone(),
                owner: o.owner.clone(),
                zone: ZoneName::Stack,
            });
        }
        for o in &self.shared_exile {
            out.push(SetupObject {
                id: o.id.clone(),
                card: o.card.clone(),
                owner: o.owner.clone(),
                zone: ZoneName::SharedExile,
            });
        }
        out
    }
}

/// One object of a setup, as listed by [`Setup::objects`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupObject {
    /// The declared or assigned id.
    pub id: String,
    /// The cardDefId.
    pub card: String,
    /// The owner's player id.
    pub owner: String,
    /// The zone it starts in.
    pub zone: ZoneName,
}

/// The card database a scenario requires (REQ-TEST-001, R11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardDatabase {
    /// The card database version.
    pub version: String,
    /// cardDefId to its required `definitionHash` (64 lowercase hex characters). Every card the
    /// setup uses has an entry.
    pub definition_hashes: BTreeMap<String, String>,
}

/// One player of a setup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSetup {
    /// The player id used everywhere else in the file.
    pub id: String,
    /// The life total.
    pub life: i64,
    /// Player counters: kind to amount (each at least 1).
    #[serde(default)]
    pub counters: BTreeMap<String, i64>,
    /// Player designations.
    #[serde(default)]
    pub designations: BTreeMap<String, DesignationSpec>,
    /// The player's zones.
    #[serde(default)]
    pub zones: PlayerZones,
}

/// The per-player zones of a setup. A missing list is empty.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerZones {
    /// The library, TOP FIRST.
    #[serde(default)]
    pub library: Vec<CardRef>,
    /// The hand.
    #[serde(default)]
    pub hand: Vec<CardRef>,
    /// The graveyard, BOTTOM FIRST.
    #[serde(default)]
    pub graveyard: Vec<CardRef>,
    /// The player's exiled cards.
    #[serde(default)]
    pub exile: Vec<CardRef>,
    /// The player's objects in the command zone.
    #[serde(default)]
    pub command: Vec<CardRef>,
}

impl PlayerZones {
    /// The five lists with their zone names, in schema order.
    #[must_use]
    pub fn lists(&self) -> [(ZoneName, &Vec<CardRef>); 5] {
        [
            (ZoneName::Library, &self.library),
            (ZoneName::Hand, &self.hand),
            (ZoneName::Graveyard, &self.graveyard),
            (ZoneName::Exile, &self.exile),
            (ZoneName::Command, &self.command),
        ]
    }
}

/// A card in a player zone: a bare cardDefId, or an id and a cardDefId.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CardRef {
    /// A bare cardDefId; the runner assigns the id.
    Card(String),
    /// A named object.
    Named {
        /// The object id.
        id: String,
        /// The cardDefId.
        card: String,
    },
}

impl CardRef {
    /// The declared id, if any.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        match self {
            CardRef::Card(_) => None,
            CardRef::Named { id, .. } => Some(id),
        }
    }

    /// The cardDefId.
    #[must_use]
    pub fn card(&self) -> &str {
        match self {
            CardRef::Card(card) | CardRef::Named { card, .. } => card,
        }
    }
}

/// A designation value in a setup or an assertion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DesignationSpec {
    /// `true`: present without a value; `false`: absent.
    Bool(bool),
    /// An integer value.
    Int(i64),
    /// A text value.
    Text(String),
}

impl DesignationSpec {
    /// The value as observed JSON: `true`, an integer or a string; `None` for `false`.
    #[must_use]
    pub fn to_observed(&self) -> Option<serde_json::Value> {
        match self {
            DesignationSpec::Bool(false) => None,
            DesignationSpec::Bool(true) => Some(serde_json::Value::Bool(true)),
            DesignationSpec::Int(i) => Some(serde_json::Value::from(*i)),
            DesignationSpec::Text(s) => Some(serde_json::Value::String(s.clone())),
        }
    }
}

/// An object on the battlefield or in the shared exile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectSetup {
    /// The object id.
    pub id: String,
    /// The cardDefId.
    pub card: String,
    /// The owner.
    pub owner: String,
    /// The controller; the owner when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<String>,
    /// Tapped.
    #[serde(default)]
    pub tapped: bool,
    /// Flipped.
    #[serde(default)]
    pub flipped: bool,
    /// Face down.
    #[serde(default)]
    pub face_down: bool,
    /// Phased out.
    #[serde(default)]
    pub phased_out: bool,
    /// Counters: kind to amount (each at least 1).
    #[serde(default)]
    pub counters: BTreeMap<String, i64>,
    /// Designations.
    #[serde(default)]
    pub designations: BTreeMap<String, DesignationSpec>,
    /// The object or player it is attached to (battlefield only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached_to: Option<String>,
    /// The relative timestamp: objects are stamped in ascending order of this value (absent
    /// counts as 0), ties in declaration order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    /// The turn since which the controller has controlled it continuously (default 0: since
    /// before the scenario's turn). An optional extension of the schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controlled_since_turn: Option<u64>,
}

/// An object on the stack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackSetup {
    /// The object id.
    pub id: String,
    /// The cardDefId.
    pub card: String,
    /// The owner.
    pub owner: String,
    /// The controller; the owner when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<String>,
}

/// The turn position of a setup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSetup {
    /// The turn number (at least 1).
    pub number: u64,
    /// The active player.
    pub active_player: String,
    /// The phase.
    pub phase: PhaseName,
    /// The step (`MAIN` in a main phase).
    pub step: StepName,
}

macro_rules! names {
    ($(#[$doc:meta])* $name:ident { $( $(#[$vdoc:meta])* $variant:ident => $wire:literal ),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum $name {
            $( $(#[$vdoc])* #[serde(rename = $wire)] $variant, )+
        }

        impl $name {
            /// Every value, in schema order.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];

            /// The schema name.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( $name::$variant => $wire, )+
                }
            }

            /// The value named `text` (exact), if any.
            #[must_use]
            pub fn from_name(text: &str) -> Option<$name> {
                $name::ALL.iter().copied().find(|v| v.as_str() == text)
            }

            /// The allowed names, comma-separated, for messages.
            #[must_use]
            pub fn names() -> String {
                $name::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>().join(", ")
            }
        }
    };
}

names! {
    /// A phase name of the schema.
    PhaseName {
        /// `BEGINNING`.
        Beginning => "BEGINNING",
        /// `PRECOMBAT_MAIN`.
        PrecombatMain => "PRECOMBAT_MAIN",
        /// `COMBAT`.
        Combat => "COMBAT",
        /// `POSTCOMBAT_MAIN`.
        PostcombatMain => "POSTCOMBAT_MAIN",
        /// `ENDING`.
        Ending => "ENDING",
    }
}

names! {
    /// A step name of the schema (`MAIN` stands for "no step" in a main phase).
    StepName {
        /// `UNTAP`.
        Untap => "UNTAP",
        /// `UPKEEP`.
        Upkeep => "UPKEEP",
        /// `DRAW`.
        Draw => "DRAW",
        /// `MAIN`.
        Main => "MAIN",
        /// `BEGINNING_OF_COMBAT`.
        BeginningOfCombat => "BEGINNING_OF_COMBAT",
        /// `DECLARE_ATTACKERS`.
        DeclareAttackers => "DECLARE_ATTACKERS",
        /// `DECLARE_BLOCKERS`.
        DeclareBlockers => "DECLARE_BLOCKERS",
        /// `FIRST_STRIKE_DAMAGE`.
        FirstStrikeDamage => "FIRST_STRIKE_DAMAGE",
        /// `COMBAT_DAMAGE`.
        CombatDamage => "COMBAT_DAMAGE",
        /// `END_OF_COMBAT`.
        EndOfCombat => "END_OF_COMBAT",
        /// `END`.
        End => "END",
        /// `CLEANUP`.
        Cleanup => "CLEANUP",
    }
}

impl StepName {
    /// The phase this step belongs to; `MAIN` belongs to both main phases.
    #[must_use]
    pub fn fits(self, phase: PhaseName) -> bool {
        match self {
            StepName::Untap | StepName::Upkeep | StepName::Draw => phase == PhaseName::Beginning,
            StepName::Main => {
                matches!(phase, PhaseName::PrecombatMain | PhaseName::PostcombatMain)
            }
            StepName::BeginningOfCombat
            | StepName::DeclareAttackers
            | StepName::DeclareBlockers
            | StepName::FirstStrikeDamage
            | StepName::CombatDamage
            | StepName::EndOfCombat => phase == PhaseName::Combat,
            StepName::End | StepName::Cleanup => phase == PhaseName::Ending,
        }
    }
}

names! {
    /// A zone name of the schema.
    ZoneName {
        /// `library` (per player).
        Library => "library",
        /// `hand` (per player).
        Hand => "hand",
        /// `graveyard` (per player).
        Graveyard => "graveyard",
        /// `exile` (the player's exiled cards).
        Exile => "exile",
        /// `command` (the player's objects in the command zone).
        Command => "command",
        /// `battlefield` (shared).
        Battlefield => "battlefield",
        /// `stack` (shared).
        Stack => "stack",
        /// `sharedExile` (shared).
        SharedExile => "sharedExile",
    }
}

impl ZoneName {
    /// Whether the zone belongs to one player (the five player-zone lists).
    #[must_use]
    pub fn is_per_player(self) -> bool {
        matches!(
            self,
            ZoneName::Library
                | ZoneName::Hand
                | ZoneName::Graveyard
                | ZoneName::Exile
                | ZoneName::Command
        )
    }

    /// The key of this zone in [`Observed::zones`](super::Observed::zones): `<owner>/<zone>`
    /// for a per-player zone with an owner, else the zone name (a shared zone ignores `owner`).
    #[must_use]
    pub fn key(self, owner: Option<&str>) -> String {
        match owner {
            Some(o) if self.is_per_player() => format!("{o}/{}", self.as_str()),
            _ => self.as_str().to_owned(),
        }
    }
}

names! {
    /// How a zone assertion compares order.
    #[derive(Default)]
    OrderMode {
        /// The same objects in the same order.
        #[default]
        Exact => "exact",
        /// The same objects in any order.
        Any => "any",
    }
}

names! {
    /// How an events assertion matches the stream.
    #[derive(Default)]
    EventsMode {
        /// The listed events occur in this order, not necessarily adjacent.
        #[default]
        Subsequence => "subsequence",
        /// The whole stream is exactly the listed events.
        Exact => "exact",
    }
}

names! {
    /// How a pending-choice assertion compares options.
    #[derive(Default)]
    OptionsMode {
        /// Exactly the listed options (in any order).
        #[default]
        Exact => "exact",
        /// At least the listed options.
        Contains => "contains",
    }
}

names! {
    /// What an `absent` assertion says is absent.
    AbsentWhat {
        /// No event in the stream matches.
        Event => "event",
        /// An object is not in a zone.
        ZoneMember => "zoneMember",
    }
}

names! {
    /// A game status.
    ResultStatus {
        /// The game goes on.
        InProgress => "IN_PROGRESS",
        /// The game ended with winners.
        Won => "WON",
        /// The game ended in a draw.
        Drawn => "DRAWN",
    }
}

/// One scripted answer (REQ-TEST-001).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptStep {
    /// What the pending choice must be before the answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<Expect>,
    /// The answer: an object whose `type` is an SRS 15.3 response name (`PASS_PRIORITY`, ...)
    /// with that response's other fields.
    pub answer: serde_json::Value,
}

/// The expected pending choice of a script step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expect {
    /// The chooser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chooser: Option<String>,
    /// The SRS 15.3 choice type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice_type: Option<String>,
}

/// The expected characteristics of a `characteristics` assertion; any subset. Arrays compare
/// as sets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacteristicsExpect {
    /// Power.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<i64>,
    /// Toughness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toughness: Option<i64>,
    /// Card types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<String>>,
    /// Subtypes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtypes: Option<Vec<String>>,
    /// Supertypes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supertypes: Option<Vec<String>>,
    /// Colors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<String>>,
    /// Ability keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abilities: Option<Vec<String>>,
    /// Controller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<String>,
    /// Mana value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mana_value: Option<i64>,
}

/// An event pattern: a type and the fields that must match.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPattern {
    /// The event type name.
    #[serde(rename = "type")]
    pub event_type: String,
    /// The fields that must be present with these values (extra fields are ignored).
    #[serde(default)]
    pub fields: BTreeMap<String, serde_json::Value>,
}

/// One assertion (REQ-TEST-001 item 1). Every form has an optional `note`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Assertion {
    /// Evaluated characteristics of an object.
    Characteristics {
        /// A free-form note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The object id.
        object: String,
        /// The expected values.
        expect: CharacteristicsExpect,
    },
    /// The contents of a zone.
    Zone {
        /// A free-form note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The owner (required for per-player zones; for a shared zone it narrows the contents
        /// to that owner's objects).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<String>,
        /// The zone.
        zone: ZoneName,
        /// The object ids, in the setup ordering convention.
        contents: Vec<String>,
        /// Exact order or any order.
        #[serde(default)]
        order: OrderMode,
    },
    /// A life total.
    Life {
        /// A free-form note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The player.
        player: String,
        /// The expected life.
        equals: i64,
    },
    /// The number of counters of one kind on a player or object.
    Counters {
        /// A free-form note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The player or object id.
        target: String,
        /// The counter kind.
        counter: String,
        /// The expected amount (0 = none).
        equals: i64,
    },
    /// A designation of a player or object.
    Designation {
        /// A free-form note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The player or object id.
        target: String,
        /// The designation key.
        key: String,
        /// The expected value; `None` (JSON `null`) or `false` = absent.
        equals: Option<DesignationSpec>,
    },
    /// The event stream.
    Events {
        /// A free-form note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// Subsequence (default) or exact.
        #[serde(default)]
        mode: EventsMode,
        /// The expected events.
        events: Vec<EventPattern>,
    },
    /// Absence of an event or of a zone member.
    Absent {
        /// A free-form note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// Which form.
        what: AbsentWhat,
        /// The event that must not occur (`event` form).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<EventPattern>,
        /// The owner of the zone (`zoneMember` form, per-player zones).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<String>,
        /// The zone (`zoneMember` form).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<ZoneName>,
        /// The object that must not be in the zone (`zoneMember` form).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object: Option<String>,
    },
    /// The pending choice.
    PendingChoice {
        /// A free-form note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The chooser; `None` (JSON `null`) = no choice pending.
        chooser: Option<String>,
        /// The SRS 15.3 choice type.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        choice_type: Option<String>,
        /// The expected options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<Vec<String>>,
        /// Exact (default) or contains.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options_mode: Option<OptionsMode>,
    },
    /// The game result.
    Result {
        /// A free-form note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The status.
        status: ResultStatus,
        /// The winners (compared as a set when given).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        winners: Option<Vec<String>>,
        /// The losers (compared as a set when given).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        losers: Option<Vec<String>>,
    },
}

impl Assertion {
    /// The `kind` name.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Assertion::Characteristics { .. } => "characteristics",
            Assertion::Zone { .. } => "zone",
            Assertion::Life { .. } => "life",
            Assertion::Counters { .. } => "counters",
            Assertion::Designation { .. } => "designation",
            Assertion::Events { .. } => "events",
            Assertion::Absent { .. } => "absent",
            Assertion::PendingChoice { .. } => "pendingChoice",
            Assertion::Result { .. } => "result",
        }
    }

    /// Every `kind` name, in schema order.
    pub const KINDS: [&'static str; 9] = [
        "characteristics",
        "zone",
        "life",
        "counters",
        "designation",
        "events",
        "absent",
        "pendingChoice",
        "result",
    ];
}

/// Every SRS 15.3 response name a script answer's `type` may have (the `ChoiceResponse`
/// variants).
pub const ANSWER_TYPES: [&str; 29] = [
    "PASS_PRIORITY",
    "PLAY_LAND",
    "INITIATE_CAST",
    "ACTIVATE",
    "SPECIAL_ACTION",
    "SELECT_MODES",
    "CONFIGURE_COSTS",
    "SELECT_TARGETS",
    "SUBMIT_DIVISION",
    "PAY_COSTS",
    "DECLINE_OPTIONAL_PAYMENT",
    "CANCEL_AND_ROLLBACK",
    "SUBMIT_ATTACKERS",
    "SUBMIT_BLOCKERS",
    "ORDER_TRIGGERS",
    "YES",
    "NO",
    "SELECT_REPLACEMENT",
    "SUBMIT_OBJECT_SELECTION",
    "SUBMIT_ORDERING",
    "SUBMIT_PLAYER",
    "SUBMIT_CHOICE_VALUE",
    "SUBMIT_CARD_NAME",
    "SUBMIT_SET",
    "ACCEPT",
    "SHORTEN",
    "KEEP_HAND",
    "TAKE_MULLIGAN",
    "SUBMIT_PREGAME",
];
