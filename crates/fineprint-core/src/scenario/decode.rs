// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! From the positioned JSON tree to a checked [`Scenario`] (REQ-TEST-001, R12).
//!
//! Every object rejects unknown members; every value is type-checked with its path; then the
//! cross-references are resolved: every player, object and card name used anywhere must be
//! declared, and no id is declared twice.

use std::collections::{BTreeMap, BTreeSet};

use super::error::{ScenarioError, ScenarioErrorKind, quote, segment};
use super::json::{Member, Node, Value};
use super::model::{
    ANSWER_TYPES, AbsentWhat, Assertion, CardDatabase, CardRef, CharacteristicsExpect,
    DesignationSpec, EventPattern, EventsMode, Expect, ObjectSetup, OptionsMode, OrderMode,
    PhaseName, PlayerSetup, PlayerZones, ResultStatus, Ruling, Scenario, ScriptStep, Setup,
    StackSetup, StepName, TurnSetup, ZoneName,
};
use crate::choice::ChoiceType;

type Result<T> = core::result::Result<T, ScenarioError>;

/// The largest number of players a scenario declares.
const MAX_PLAYERS: usize = 8;

/// The longest title, in characters.
const MAX_TITLE_CHARS: usize = 200;

fn err(kind: ScenarioErrorKind, path: &str, node: &Node, message: String) -> ScenarioError {
    ScenarioError::new(kind, path, Some(node.pos()), message)
}

fn wrong_type(path: &str, node: &Node, expected: &str) -> ScenarioError {
    err(
        ScenarioErrorKind::WrongType,
        path,
        node,
        format!("expected {expected}, found {}", node.type_name()),
    )
}

fn child(path: &str, key: &str) -> String {
    format!("{path}/{}", segment(key))
}

fn index(path: &str, i: usize) -> String {
    format!("{path}/{i}")
}

/// A checked object: its members, every one of them allowed.
struct Obj<'a> {
    node: &'a Node,
    members: &'a [Member],
    path: String,
}

impl<'a> Obj<'a> {
    fn new(node: &'a Node, path: &str, allowed: &[&str]) -> Result<Obj<'a>> {
        let Value::Object(members) = &node.value else {
            return Err(wrong_type(path, node, "an object"));
        };
        for m in members {
            if !allowed.contains(&m.key.as_str()) {
                // The path is the object holding the member; the position is the key's.
                return Err(ScenarioError::new(
                    ScenarioErrorKind::UnknownField,
                    path,
                    Some((m.line, m.column)),
                    format!("unknown field {}", quote(&m.key)),
                ));
            }
        }
        Ok(Obj {
            node,
            members,
            path: path.to_owned(),
        })
    }

    fn opt(&self, key: &str) -> Option<(&'a Node, String)> {
        self.members
            .iter()
            .find(|m| m.key == key)
            .map(|m| (&m.node, child(&self.path, key)))
    }

    fn req(&self, key: &str) -> Result<(&'a Node, String)> {
        self.opt(key).ok_or_else(|| {
            err(
                ScenarioErrorKind::MissingField,
                &self.path,
                self.node,
                format!("missing field \"{key}\""),
            )
        })
    }

    /// An optional member that is absent or `null`.
    fn opt_non_null(&self, key: &str) -> Option<(&'a Node, String)> {
        self.opt(key).filter(|(n, _)| n.value != Value::Null)
    }
}

fn string(node: &Node, path: &str) -> Result<String> {
    match &node.value {
        Value::Str(s) => Ok(s.clone()),
        _ => Err(wrong_type(path, node, "a string")),
    }
}

/// A non-empty string (an id or a name).
fn name(node: &Node, path: &str) -> Result<String> {
    let s = string(node, path)?;
    if s.is_empty() {
        return Err(err(
            ScenarioErrorKind::Invalid,
            path,
            node,
            "must not be an empty string".to_owned(),
        ));
    }
    Ok(s)
}

fn boolean(node: &Node, path: &str) -> Result<bool> {
    match node.value {
        Value::Bool(b) => Ok(b),
        _ => Err(wrong_type(path, node, "a boolean")),
    }
}

fn int_i64(node: &Node, path: &str) -> Result<i64> {
    match node.value {
        Value::Int(i) => i64::try_from(i).map_err(|_| {
            err(
                ScenarioErrorKind::OutOfRange,
                path,
                node,
                format!("{i} is outside the signed 64-bit range"),
            )
        }),
        _ => Err(wrong_type(path, node, "an integer")),
    }
}

fn int_u64(node: &Node, path: &str) -> Result<u64> {
    match node.value {
        Value::Int(i) => u64::try_from(i).map_err(|_| {
            err(
                ScenarioErrorKind::OutOfRange,
                path,
                node,
                format!("{i} is outside the unsigned 64-bit range"),
            )
        }),
        _ => Err(wrong_type(path, node, "an integer")),
    }
}

fn array<'a>(node: &'a Node, path: &str) -> Result<&'a [Node]> {
    match &node.value {
        Value::Array(items) => Ok(items),
        _ => Err(wrong_type(path, node, "an array")),
    }
}

fn map_members<'a>(node: &'a Node, path: &str) -> Result<&'a [Member]> {
    match &node.value {
        Value::Object(members) => Ok(members),
        _ => Err(wrong_type(path, node, "an object")),
    }
}

fn string_list(node: &Node, path: &str) -> Result<Vec<String>> {
    array(node, path)?
        .iter()
        .enumerate()
        .map(|(i, n)| string(n, &index(path, i)))
        .collect()
}

fn name_list(node: &Node, path: &str) -> Result<Vec<String>> {
    array(node, path)?
        .iter()
        .enumerate()
        .map(|(i, n)| name(n, &index(path, i)))
        .collect()
}

fn tag<T: Copy>(
    node: &Node,
    path: &str,
    from: fn(&str) -> Option<T>,
    names: fn() -> String,
) -> Result<T> {
    let s = string(node, path)?;
    from(&s).ok_or_else(|| {
        err(
            ScenarioErrorKind::UnknownTag,
            path,
            node,
            format!("unknown value {}; expected one of {}", quote(&s), names()),
        )
    })
}

fn counter_map(node: &Node, path: &str) -> Result<BTreeMap<String, i64>> {
    let mut out = BTreeMap::new();
    for m in map_members(node, path)? {
        let p = child(path, &m.key);
        if m.key.is_empty() {
            return Err(ScenarioError::new(
                ScenarioErrorKind::Invalid,
                p,
                Some((m.line, m.column)),
                "a counter kind must not be empty".to_owned(),
            ));
        }
        let amount = int_i64(&m.node, &p)?;
        if amount < 1 {
            return Err(err(
                ScenarioErrorKind::OutOfRange,
                &p,
                &m.node,
                format!("a counter amount must be at least 1, found {amount}"),
            ));
        }
        out.insert(m.key.clone(), amount);
    }
    Ok(out)
}

fn designation_value(node: &Node, path: &str) -> Result<DesignationSpec> {
    match &node.value {
        Value::Bool(b) => Ok(DesignationSpec::Bool(*b)),
        Value::Str(s) => Ok(DesignationSpec::Text(s.clone())),
        Value::Int(_) => Ok(DesignationSpec::Int(int_i64(node, path)?)),
        _ => Err(wrong_type(path, node, "a boolean, an integer or a string")),
    }
}

fn designation_map(node: &Node, path: &str) -> Result<BTreeMap<String, DesignationSpec>> {
    let mut out = BTreeMap::new();
    for m in map_members(node, path)? {
        let p = child(path, &m.key);
        if m.key.is_empty() {
            return Err(ScenarioError::new(
                ScenarioErrorKind::Invalid,
                p,
                Some((m.line, m.column)),
                "a designation key must not be empty".to_owned(),
            ));
        }
        out.insert(m.key.clone(), designation_value(&m.node, &p)?);
    }
    Ok(out)
}

fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: core::ops::Range<usize>| -> Option<u32> {
        let part = s.get(r)?;
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    let (Some(_), Some(m), Some(d)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let Some(year) = digits(0..4) else {
        return false;
    };
    // The Gregorian leap rule: divisible by 4, except centuries not divisible by 400.
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

/// Decodes and checks a whole scenario.
pub(crate) fn decode(root: &Node) -> Result<Scenario> {
    let Value::Object(members) = &root.value else {
        return Err(wrong_type("", root, "an object at the top level"));
    };
    // The version first: a file of another version is reported as such, whatever it holds.
    let Some(version) = members.iter().find(|m| m.key == "pzlVersion") else {
        return Err(err(
            ScenarioErrorKind::MissingField,
            "",
            root,
            "missing field \"pzlVersion\"".to_owned(),
        ));
    };
    // Compared as the reader's integer, without narrowing: -1 and 0 are as unsupported as 2.
    let Value::Int(v) = version.node.value else {
        return Err(wrong_type("/pzlVersion", &version.node, "an integer"));
    };
    if v != 1 {
        return Err(err(
            ScenarioErrorKind::Unsupported,
            "/pzlVersion",
            &version.node,
            format!("pzlVersion {v} is not supported; this runner reads version 1"),
        ));
    }
    let o = Obj::new(
        root,
        "",
        &[
            "pzlVersion",
            "title",
            "requirements",
            "cr",
            "ruling",
            "format",
            "rulesBaseline",
            "cardDatabase",
            "prngSeed",
            "players",
            "battlefield",
            "stack",
            "sharedExile",
            "turn",
            "priority",
            "script",
            "assertions",
        ],
    )?;
    let (n, p) = o.req("title")?;
    let title = string(n, &p)?;
    let len = title.chars().count();
    if len == 0 || len > MAX_TITLE_CHARS {
        return Err(err(
            ScenarioErrorKind::OutOfRange,
            &p,
            n,
            format!("a title has 1 to {MAX_TITLE_CHARS} characters, found {len}"),
        ));
    }
    let (n, p) = o.req("requirements")?;
    let requirements = string_list(n, &p)?;
    for (i, r) in requirements.iter().enumerate() {
        if !r.starts_with("REQ-") || r.len() == 4 {
            let ip = index(&p, i);
            let node = &array(n, &p)?[i];
            return Err(err(
                ScenarioErrorKind::Invalid,
                &ip,
                node,
                format!("a requirement id starts with \"REQ-\", found {}", quote(r)),
            ));
        }
    }
    let (n, p) = o.req("cr")?;
    let cr = string_list(n, &p)?;
    let ruling = match o.opt("ruling") {
        Some((n, p)) => Some(decode_ruling(n, &p)?),
        None => None,
    };
    if requirements.is_empty() && ruling.is_none() {
        return Err(err(
            ScenarioErrorKind::MissingField,
            "",
            root,
            "missing field \"ruling\" (required when \"requirements\" is empty)".to_owned(),
        ));
    }
    let setup = decode_setup(&o)?;
    let script = match o.opt("script") {
        Some((n, p)) => array(n, &p)?
            .iter()
            .enumerate()
            .map(|(i, s)| decode_step(s, &index(&p, i)))
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let (n, p) = o.req("assertions")?;
    let assertions = array(n, &p)?
        .iter()
        .enumerate()
        .map(|(i, a)| decode_assertion(a, &index(&p, i)))
        .collect::<Result<Vec<_>>>()?;
    let scenario = Scenario {
        pzl_version: 1,
        title,
        requirements,
        cr,
        ruling,
        setup,
        script,
        assertions,
    };
    resolve(&scenario, root)?;
    Ok(scenario)
}

fn decode_ruling(node: &Node, path: &str) -> Result<Ruling> {
    let o = Obj::new(node, path, &["date", "ref"])?;
    let (n, p) = o.req("date")?;
    let date = string(n, &p)?;
    if !is_date(&date) {
        return Err(err(
            ScenarioErrorKind::Invalid,
            &p,
            n,
            format!("a ruling date is YYYY-MM-DD, found {}", quote(&date)),
        ));
    }
    let (n, p) = o.req("ref")?;
    Ok(Ruling {
        date,
        reference: string(n, &p)?,
    })
}

fn decode_setup(o: &Obj<'_>) -> Result<Setup> {
    let (n, p) = o.req("format")?;
    let format = name(n, &p)?;
    let (n, p) = o.req("rulesBaseline")?;
    let rules_baseline = name(n, &p)?;
    let (n, p) = o.req("cardDatabase")?;
    let card_database = decode_card_database(n, &p)?;
    let (n, p) = o.req("prngSeed")?;
    let prng_seed = int_u64(n, &p)?;
    let (n, p) = o.req("players")?;
    let list = array(n, &p)?;
    if list.is_empty() || list.len() > MAX_PLAYERS {
        return Err(err(
            ScenarioErrorKind::OutOfRange,
            &p,
            n,
            format!(
                "a scenario has 1 to {MAX_PLAYERS} players, found {}",
                list.len()
            ),
        ));
    }
    let players = list
        .iter()
        .enumerate()
        .map(|(i, pl)| decode_player(pl, &index(&p, i)))
        .collect::<Result<Vec<_>>>()?;
    let battlefield = match o.opt("battlefield") {
        Some((n, p)) => array(n, &p)?
            .iter()
            .enumerate()
            .map(|(i, x)| decode_object(x, &index(&p, i), true))
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let stack = match o.opt("stack") {
        Some((n, p)) => array(n, &p)?
            .iter()
            .enumerate()
            .map(|(i, x)| decode_stack(x, &index(&p, i)))
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let shared_exile = match o.opt("sharedExile") {
        Some((n, p)) => array(n, &p)?
            .iter()
            .enumerate()
            .map(|(i, x)| decode_object(x, &index(&p, i), false))
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let (n, p) = o.req("turn")?;
    let turn = decode_turn(n, &p)?;
    let (n, p) = o.req("priority")?;
    let priority = match n.value {
        Value::Null => None,
        _ => Some(name(n, &p)?),
    };
    Ok(Setup {
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
    })
}

fn decode_card_database(node: &Node, path: &str) -> Result<CardDatabase> {
    let o = Obj::new(node, path, &["version", "definitionHashes"])?;
    let (n, p) = o.req("version")?;
    let version = name(n, &p)?;
    let (n, p) = o.req("definitionHashes")?;
    let mut definition_hashes = BTreeMap::new();
    for m in map_members(n, &p)? {
        let mp = child(&p, &m.key);
        if m.key.is_empty() {
            return Err(ScenarioError::new(
                ScenarioErrorKind::Invalid,
                mp,
                Some((m.line, m.column)),
                "a cardDefId must not be empty".to_owned(),
            ));
        }
        let hash = string(&m.node, &mp)?;
        let len = hash.chars().count();
        if len != 64 {
            return Err(err(
                ScenarioErrorKind::OutOfRange,
                &mp,
                &m.node,
                format!("a definitionHash has 64 characters, found {len}"),
            ));
        }
        if !is_hash(&hash) {
            return Err(err(
                ScenarioErrorKind::OutOfRange,
                &mp,
                &m.node,
                format!(
                    "a definitionHash is 64 lowercase hex characters, found {}",
                    quote(&hash)
                ),
            ));
        }
        definition_hashes.insert(m.key.clone(), hash);
    }
    Ok(CardDatabase {
        version,
        definition_hashes,
    })
}

fn decode_player(node: &Node, path: &str) -> Result<PlayerSetup> {
    let o = Obj::new(
        node,
        path,
        &["id", "life", "counters", "designations", "zones"],
    )?;
    let (n, p) = o.req("id")?;
    let id = name(n, &p)?;
    let (n, p) = o.req("life")?;
    let life = int_i64(n, &p)?;
    let counters = match o.opt("counters") {
        Some((n, p)) => counter_map(n, &p)?,
        None => BTreeMap::new(),
    };
    let designations = match o.opt("designations") {
        Some((n, p)) => designation_map(n, &p)?,
        None => BTreeMap::new(),
    };
    let zones = match o.opt("zones") {
        Some((n, p)) => decode_zones(n, &p)?,
        None => PlayerZones::default(),
    };
    Ok(PlayerSetup {
        id,
        life,
        counters,
        designations,
        zones,
    })
}

fn decode_zones(node: &Node, path: &str) -> Result<PlayerZones> {
    let o = Obj::new(
        node,
        path,
        &["library", "hand", "graveyard", "exile", "command"],
    )?;
    let list = |key: &str| -> Result<Vec<CardRef>> {
        match o.opt(key) {
            Some((n, p)) => array(n, &p)?
                .iter()
                .enumerate()
                .map(|(i, r)| decode_card_ref(r, &index(&p, i)))
                .collect(),
            None => Ok(Vec::new()),
        }
    };
    Ok(PlayerZones {
        library: list("library")?,
        hand: list("hand")?,
        graveyard: list("graveyard")?,
        exile: list("exile")?,
        command: list("command")?,
    })
}

fn decode_card_ref(node: &Node, path: &str) -> Result<CardRef> {
    match &node.value {
        Value::Str(_) => Ok(CardRef::Card(name(node, path)?)),
        Value::Object(_) => {
            let o = Obj::new(node, path, &["id", "card"])?;
            let (n, p) = o.req("id")?;
            let id = name(n, &p)?;
            let (n, p) = o.req("card")?;
            Ok(CardRef::Named {
                id,
                card: name(n, &p)?,
            })
        }
        _ => Err(wrong_type(
            path,
            node,
            "a cardDefId string or an {id, card} object",
        )),
    }
}

fn decode_object(node: &Node, path: &str, battlefield: bool) -> Result<ObjectSetup> {
    let mut allowed = vec![
        "id",
        "card",
        "owner",
        "controller",
        "tapped",
        "flipped",
        "faceDown",
        "phasedOut",
        "counters",
        "designations",
        "timestamp",
        "controlledSinceTurn",
    ];
    if battlefield {
        allowed.push("attachedTo");
    }
    let o = Obj::new(node, path, &allowed)?;
    let (n, p) = o.req("id")?;
    let id = name(n, &p)?;
    let (n, p) = o.req("card")?;
    let card = name(n, &p)?;
    let (n, p) = o.req("owner")?;
    let owner = name(n, &p)?;
    let opt_name = |key: &str| -> Result<Option<String>> {
        o.opt_non_null(key).map(|(n, p)| name(n, &p)).transpose()
    };
    let flag =
        |key: &str| -> Result<bool> { o.opt(key).map_or(Ok(false), |(n, p)| boolean(n, &p)) };
    let opt_u64 = |key: &str| -> Result<Option<u64>> {
        o.opt_non_null(key).map(|(n, p)| int_u64(n, &p)).transpose()
    };
    Ok(ObjectSetup {
        id,
        card,
        owner,
        controller: opt_name("controller")?,
        tapped: flag("tapped")?,
        flipped: flag("flipped")?,
        face_down: flag("faceDown")?,
        phased_out: flag("phasedOut")?,
        counters: match o.opt("counters") {
            Some((n, p)) => counter_map(n, &p)?,
            None => BTreeMap::new(),
        },
        designations: match o.opt("designations") {
            Some((n, p)) => designation_map(n, &p)?,
            None => BTreeMap::new(),
        },
        attached_to: opt_name("attachedTo")?,
        timestamp: opt_u64("timestamp")?,
        controlled_since_turn: opt_u64("controlledSinceTurn")?,
    })
}

fn decode_stack(node: &Node, path: &str) -> Result<StackSetup> {
    let o = Obj::new(node, path, &["id", "card", "owner", "controller"])?;
    let (n, p) = o.req("id")?;
    let id = name(n, &p)?;
    let (n, p) = o.req("card")?;
    let card = name(n, &p)?;
    let (n, p) = o.req("owner")?;
    let owner = name(n, &p)?;
    let controller = o
        .opt_non_null("controller")
        .map(|(n, p)| name(n, &p))
        .transpose()?;
    Ok(StackSetup {
        id,
        card,
        owner,
        controller,
    })
}

fn decode_turn(node: &Node, path: &str) -> Result<TurnSetup> {
    let o = Obj::new(node, path, &["number", "activePlayer", "phase", "step"])?;
    let (n, p) = o.req("number")?;
    let number = int_u64(n, &p)?;
    if number == 0 {
        return Err(err(
            ScenarioErrorKind::OutOfRange,
            &p,
            n,
            "the turn number is at least 1".to_owned(),
        ));
    }
    let (n, p) = o.req("activePlayer")?;
    let active_player = name(n, &p)?;
    let (n, p) = o.req("phase")?;
    let phase = tag(n, &p, PhaseName::from_name, PhaseName::names)?;
    let (sn, sp) = o.req("step")?;
    let step = tag(sn, &sp, StepName::from_name, StepName::names)?;
    if !step.fits(phase) {
        return Err(err(
            ScenarioErrorKind::Invalid,
            &sp,
            sn,
            format!(
                "step {} does not belong to phase {}",
                step.as_str(),
                phase.as_str()
            ),
        ));
    }
    Ok(TurnSetup {
        number,
        active_player,
        phase,
        step,
    })
}

fn choice_type(node: &Node, path: &str) -> Result<String> {
    let s = string(node, path)?;
    if ChoiceType::from_wire(&s).is_none() {
        return Err(err(
            ScenarioErrorKind::UnknownTag,
            path,
            node,
            format!("unknown choice type {}", quote(&s)),
        ));
    }
    Ok(s)
}

/// The answer types that carry no payload: the unit variants of `ChoiceResponse` (REQ-TEST-001).
/// Such an answer is exactly `{"type": <name>}`; any other member is `UnknownField`. Keep in step
/// with `ANSWER_TYPES`.
pub(super) const FIELDLESS_ANSWER_TYPES: [&str; 7] = [
    "PASS_PRIORITY",
    "DECLINE_OPTIONAL_PAYMENT",
    "CANCEL_AND_ROLLBACK",
    "YES",
    "NO",
    "ACCEPT",
    "TAKE_MULLIGAN",
];

fn decode_step(node: &Node, path: &str) -> Result<ScriptStep> {
    let o = Obj::new(node, path, &["expect", "answer"])?;
    let expect = match o.opt("expect") {
        Some((n, p)) => {
            let e = Obj::new(n, &p, &["chooser", "choiceType"])?;
            Some(Expect {
                chooser: e
                    .opt_non_null("chooser")
                    .map(|(n, p)| name(n, &p))
                    .transpose()?,
                choice_type: e
                    .opt_non_null("choiceType")
                    .map(|(n, p)| choice_type(n, &p))
                    .transpose()?,
            })
        }
        None => None,
    };
    let (n, p) = o.req("answer")?;
    let Value::Object(_) = &n.value else {
        return Err(wrong_type(&p, n, "an answer object with a \"type\""));
    };
    let answer_obj = Obj {
        node: n,
        members: map_members(n, &p)?,
        path: p.clone(),
    };
    let (tn, tp) = answer_obj.req("type")?;
    let t = string(tn, &tp)?;
    if !ANSWER_TYPES.contains(&t.as_str()) {
        return Err(err(
            ScenarioErrorKind::UnknownTag,
            &tp,
            tn,
            format!("unknown answer type {}", quote(&t)),
        ));
    }
    // An answer that carries no payload takes no member besides its `type`.
    if FIELDLESS_ANSWER_TYPES.contains(&t.as_str()) {
        Obj::new(n, &p, &["type"])?;
    }
    Ok(ScriptStep {
        expect,
        answer: n.to_json(),
    })
}

fn decode_pattern(node: &Node, path: &str) -> Result<EventPattern> {
    let o = Obj::new(node, path, &["type", "fields"])?;
    let (n, p) = o.req("type")?;
    let event_type = name(n, &p)?;
    let mut fields = BTreeMap::new();
    if let Some((n, p)) = o.opt("fields") {
        for m in map_members(n, &p)? {
            fields.insert(m.key.clone(), m.node.to_json());
        }
    }
    Ok(EventPattern { event_type, fields })
}

const NOTE: &str = "note";

fn note(o: &Obj<'_>) -> Result<Option<String>> {
    o.opt_non_null(NOTE).map(|(n, p)| string(n, &p)).transpose()
}

fn decode_assertion(node: &Node, path: &str) -> Result<Assertion> {
    let Value::Object(members) = &node.value else {
        return Err(wrong_type(path, node, "an assertion object"));
    };
    let probe = Obj {
        node,
        members,
        path: path.to_owned(),
    };
    let (kn, kp) = probe.req("kind")?;
    let kind = string(kn, &kp)?;
    let opt_i = |o: &Obj<'_>, key: &str| -> Result<Option<Vec<String>>> {
        o.opt_non_null(key)
            .map(|(n, p)| string_list(n, &p))
            .transpose()
    };
    match kind.as_str() {
        "characteristics" => {
            let o = Obj::new(node, path, &["kind", NOTE, "object", "expect"])?;
            let (n, p) = o.req("object")?;
            let object = name(n, &p)?;
            let (en, ep) = o.req("expect")?;
            let e = Obj::new(
                en,
                &ep,
                &[
                    "power",
                    "toughness",
                    "types",
                    "subtypes",
                    "supertypes",
                    "colors",
                    "abilities",
                    "controller",
                    "manaValue",
                ],
            )?;
            let opt_int = |key: &str| -> Result<Option<i64>> {
                e.opt_non_null(key).map(|(n, p)| int_i64(n, &p)).transpose()
            };
            let expect = CharacteristicsExpect {
                power: opt_int("power")?,
                toughness: opt_int("toughness")?,
                types: opt_i(&e, "types")?,
                subtypes: opt_i(&e, "subtypes")?,
                supertypes: opt_i(&e, "supertypes")?,
                colors: opt_i(&e, "colors")?,
                abilities: opt_i(&e, "abilities")?,
                controller: e
                    .opt_non_null("controller")
                    .map(|(n, p)| name(n, &p))
                    .transpose()?,
                mana_value: opt_int("manaValue")?,
            };
            Ok(Assertion::Characteristics {
                note: note(&o)?,
                object,
                expect,
            })
        }
        "zone" => {
            let o = Obj::new(
                node,
                path,
                &["kind", NOTE, "owner", "zone", "contents", "order"],
            )?;
            let (n, p) = o.req("zone")?;
            let zone = tag(n, &p, ZoneName::from_name, ZoneName::names)?;
            let owner = o
                .opt_non_null("owner")
                .map(|(n, p)| name(n, &p))
                .transpose()?;
            if zone.is_per_player() && owner.is_none() {
                return Err(err(
                    ScenarioErrorKind::MissingField,
                    path,
                    node,
                    format!(
                        "missing field \"owner\" (zone {} is per player)",
                        zone.as_str()
                    ),
                ));
            }
            let (n, p) = o.req("contents")?;
            let contents = name_list(n, &p)?;
            let order = match o.opt("order") {
                Some((n, p)) => tag(n, &p, OrderMode::from_name, OrderMode::names)?,
                None => OrderMode::Exact,
            };
            Ok(Assertion::Zone {
                note: note(&o)?,
                owner,
                zone,
                contents,
                order,
            })
        }
        "life" => {
            let o = Obj::new(node, path, &["kind", NOTE, "player", "equals"])?;
            let (n, p) = o.req("player")?;
            let player = name(n, &p)?;
            let (n, p) = o.req("equals")?;
            Ok(Assertion::Life {
                note: note(&o)?,
                player,
                equals: int_i64(n, &p)?,
            })
        }
        "counters" => {
            let o = Obj::new(node, path, &["kind", NOTE, "target", "counter", "equals"])?;
            let (n, p) = o.req("target")?;
            let target = name(n, &p)?;
            let (n, p) = o.req("counter")?;
            let counter = name(n, &p)?;
            let (n, p) = o.req("equals")?;
            Ok(Assertion::Counters {
                note: note(&o)?,
                target,
                counter,
                equals: int_i64(n, &p)?,
            })
        }
        "designation" => {
            let o = Obj::new(node, path, &["kind", NOTE, "target", "key", "equals"])?;
            let (n, p) = o.req("target")?;
            let target = name(n, &p)?;
            let (n, p) = o.req("key")?;
            let key = name(n, &p)?;
            let (n, p) = o.req("equals")?;
            let equals = match n.value {
                Value::Null => None,
                _ => Some(designation_value(n, &p)?),
            };
            Ok(Assertion::Designation {
                note: note(&o)?,
                target,
                key,
                equals,
            })
        }
        "events" => {
            let o = Obj::new(node, path, &["kind", NOTE, "mode", "events"])?;
            let mode = match o.opt("mode") {
                Some((n, p)) => tag(n, &p, EventsMode::from_name, EventsMode::names)?,
                None => EventsMode::Subsequence,
            };
            let (n, p) = o.req("events")?;
            let events = array(n, &p)?
                .iter()
                .enumerate()
                .map(|(i, e)| decode_pattern(e, &index(&p, i)))
                .collect::<Result<Vec<_>>>()?;
            Ok(Assertion::Events {
                note: note(&o)?,
                mode,
                events,
            })
        }
        "absent" => {
            let o0 = Obj::new(
                node,
                path,
                &["kind", NOTE, "what", "event", "owner", "zone", "object"],
            )?;
            let (n, p) = o0.req("what")?;
            let what = tag(n, &p, AbsentWhat::from_name, AbsentWhat::names)?;
            match what {
                AbsentWhat::Event => {
                    let o = Obj::new(node, path, &["kind", NOTE, "what", "event"])?;
                    let (n, p) = o.req("event")?;
                    Ok(Assertion::Absent {
                        note: note(&o)?,
                        what,
                        event: Some(decode_pattern(n, &p)?),
                        owner: None,
                        zone: None,
                        object: None,
                    })
                }
                AbsentWhat::ZoneMember => {
                    let o = Obj::new(
                        node,
                        path,
                        &["kind", NOTE, "what", "owner", "zone", "object"],
                    )?;
                    let (n, p) = o.req("zone")?;
                    let zone = tag(n, &p, ZoneName::from_name, ZoneName::names)?;
                    let owner = o
                        .opt_non_null("owner")
                        .map(|(n, p)| name(n, &p))
                        .transpose()?;
                    if zone.is_per_player() && owner.is_none() {
                        return Err(err(
                            ScenarioErrorKind::MissingField,
                            path,
                            node,
                            format!(
                                "missing field \"owner\" (zone {} is per player)",
                                zone.as_str()
                            ),
                        ));
                    }
                    let (n, p) = o.req("object")?;
                    Ok(Assertion::Absent {
                        note: note(&o)?,
                        what,
                        event: None,
                        owner,
                        zone: Some(zone),
                        object: Some(name(n, &p)?),
                    })
                }
            }
        }
        "pendingChoice" => {
            let o = Obj::new(
                node,
                path,
                &[
                    "kind",
                    NOTE,
                    "chooser",
                    "choiceType",
                    "options",
                    "optionsMode",
                ],
            )?;
            let (n, p) = o.req("chooser")?;
            let chooser = match n.value {
                Value::Null => None,
                _ => Some(name(n, &p)?),
            };
            let choice_type = o
                .opt_non_null("choiceType")
                .map(|(n, p)| choice_type(n, &p))
                .transpose()?;
            let options = opt_i(&o, "options")?;
            let options_mode = o
                .opt_non_null("optionsMode")
                .map(|(n, p)| tag(n, &p, OptionsMode::from_name, OptionsMode::names))
                .transpose()?;
            Ok(Assertion::PendingChoice {
                note: note(&o)?,
                chooser,
                choice_type,
                options,
                options_mode,
            })
        }
        "result" => {
            let o = Obj::new(node, path, &["kind", NOTE, "status", "winners", "losers"])?;
            let (n, p) = o.req("status")?;
            let status = tag(n, &p, ResultStatus::from_name, ResultStatus::names)?;
            let names_of = |key: &str| -> Result<Option<Vec<String>>> {
                o.opt_non_null(key)
                    .map(|(n, p)| name_list(n, &p))
                    .transpose()
            };
            Ok(Assertion::Result {
                note: note(&o)?,
                status,
                winners: names_of("winners")?,
                losers: names_of("losers")?,
            })
        }
        _ => Err(err(
            ScenarioErrorKind::UnknownTag,
            &kp,
            kn,
            format!(
                "unknown assertion kind {}; expected one of {}",
                quote(&kind),
                Assertion::KINDS.join(", ")
            ),
        )),
    }
}

// -------------------------------------------------------------------------------------------
// Cross-references
// -------------------------------------------------------------------------------------------

/// The node at `pointer` under `root`, for positions of semantic errors.
fn at<'a>(root: &'a Node, pointer: &str) -> Option<&'a Node> {
    let mut node = root;
    for raw in pointer.strip_prefix('/')?.split('/') {
        let key = raw.replace("~1", "/").replace("~0", "~");
        node = match &node.value {
            Value::Object(members) => &members.iter().find(|m| m.key == key)?.node,
            Value::Array(items) => items.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(node)
}

struct Names<'r> {
    root: &'r Node,
    players: BTreeSet<String>,
    objects: BTreeSet<String>,
    battlefield: BTreeSet<String>,
}

impl Names<'_> {
    fn error(&self, kind: ScenarioErrorKind, path: &str, message: String) -> ScenarioError {
        ScenarioError::new(kind, path, at(self.root, path).map(Node::pos), message)
    }

    fn player(&self, name: &str, path: &str) -> Result<()> {
        if self.players.contains(name) {
            Ok(())
        } else {
            Err(self.error(
                ScenarioErrorKind::UnresolvedReference,
                path,
                format!("player {} is not declared in \"players\"", quote(name)),
            ))
        }
    }

    fn object(&self, name: &str, path: &str) -> Result<()> {
        if self.objects.contains(name) {
            Ok(())
        } else {
            Err(self.error(
                ScenarioErrorKind::UnresolvedReference,
                path,
                format!("object {} is not declared in the setup", quote(name)),
            ))
        }
    }

    fn player_or_object(&self, name: &str, path: &str) -> Result<()> {
        if self.players.contains(name) || self.objects.contains(name) {
            Ok(())
        } else {
            Err(self.error(
                ScenarioErrorKind::UnresolvedReference,
                path,
                format!(
                    "{} is neither a declared player nor a declared object",
                    quote(name)
                ),
            ))
        }
    }
}

/// Checks every id is declared once and every reference resolves (REQ-TEST-001, R11).
fn resolve(s: &Scenario, root: &Node) -> Result<()> {
    let setup = &s.setup;
    let mut names = Names {
        root,
        players: BTreeSet::new(),
        objects: BTreeSet::new(),
        battlefield: BTreeSet::new(),
    };
    for (i, p) in setup.players.iter().enumerate() {
        let path = format!("/players/{i}/id");
        if !names.players.insert(p.id.clone()) {
            return Err(names.error(
                ScenarioErrorKind::Duplicate,
                &path,
                format!("player id {} is declared twice", quote(&p.id)),
            ));
        }
    }
    let hashes = &setup.card_database.definition_hashes;
    let card_check = |names: &Names<'_>, card: &str, path: &str| -> Result<()> {
        if hashes.contains_key(card) {
            Ok(())
        } else {
            Err(names.error(
                ScenarioErrorKind::UnresolvedReference,
                path,
                format!(
                    "card {} has no entry in cardDatabase.definitionHashes",
                    quote(card)
                ),
            ))
        }
    };
    let declare = |names: &mut Names<'_>, id: &str, path: &str| -> Result<()> {
        if names.players.contains(id) || !names.objects.insert(id.to_owned()) {
            return Err(names.error(
                ScenarioErrorKind::Duplicate,
                path,
                format!("id {} is declared twice", quote(id)),
            ));
        }
        Ok(())
    };
    for (i, p) in setup.players.iter().enumerate() {
        for (zone, list) in p.zones.lists() {
            for (j, r) in list.iter().enumerate() {
                let base = format!("/players/{i}/zones/{}/{j}", zone.as_str());
                let assigned = format!("{}.{}.{j}", p.id, zone.as_str());
                match r {
                    CardRef::Card(card) => {
                        declare(&mut names, &assigned, &base)?;
                        card_check(&names, card, &base)?;
                    }
                    CardRef::Named { id, card } => {
                        declare(&mut names, id, &format!("{base}/id"))?;
                        card_check(&names, card, &format!("{base}/card"))?;
                    }
                }
            }
        }
    }
    for (i, o) in setup.battlefield.iter().enumerate() {
        let base = format!("/battlefield/{i}");
        declare(&mut names, &o.id, &format!("{base}/id"))?;
        names.battlefield.insert(o.id.clone());
        card_check(&names, &o.card, &format!("{base}/card"))?;
    }
    for (i, o) in setup.stack.iter().enumerate() {
        let base = format!("/stack/{i}");
        declare(&mut names, &o.id, &format!("{base}/id"))?;
        card_check(&names, &o.card, &format!("{base}/card"))?;
    }
    for (i, o) in setup.shared_exile.iter().enumerate() {
        let base = format!("/sharedExile/{i}");
        declare(&mut names, &o.id, &format!("{base}/id"))?;
        card_check(&names, &o.card, &format!("{base}/card"))?;
    }
    // Owners, controllers, attachments.
    for (i, o) in setup.battlefield.iter().enumerate() {
        let base = format!("/battlefield/{i}");
        names.player(&o.owner, &format!("{base}/owner"))?;
        if let Some(c) = &o.controller {
            names.player(c, &format!("{base}/controller"))?;
        }
        if let Some(a) = &o.attached_to {
            let path = format!("{base}/attachedTo");
            if *a == o.id || !(names.players.contains(a) || names.battlefield.contains(a)) {
                return Err(names.error(
                    ScenarioErrorKind::UnresolvedReference,
                    &path,
                    format!(
                        "attachedTo {} must name a player or another battlefield object",
                        quote(a)
                    ),
                ));
            }
        }
    }
    for (i, o) in setup.stack.iter().enumerate() {
        let base = format!("/stack/{i}");
        names.player(&o.owner, &format!("{base}/owner"))?;
        if let Some(c) = &o.controller {
            names.player(c, &format!("{base}/controller"))?;
        }
    }
    for (i, o) in setup.shared_exile.iter().enumerate() {
        let base = format!("/sharedExile/{i}");
        names.player(&o.owner, &format!("{base}/owner"))?;
        if let Some(c) = &o.controller {
            names.player(c, &format!("{base}/controller"))?;
        }
    }
    names.player(&setup.turn.active_player, "/turn/activePlayer")?;
    if let Some(p) = &setup.priority {
        names.player(p, "/priority")?;
    }
    // Script.
    for (i, step) in s.script.iter().enumerate() {
        let base = format!("/script/{i}");
        if let Some(c) = step.expect.as_ref().and_then(|e| e.chooser.as_ref()) {
            names.player(c, &format!("{base}/expect/chooser"))?;
        }
        if let serde_json::Value::Object(map) = &step.answer {
            for (k, v) in map {
                let path = format!("{base}/answer/{}", segment(k));
                match (k.as_str(), v) {
                    ("object" | "objectId", serde_json::Value::String(n)) => {
                        names.object(n, &path)?;
                    }
                    ("player" | "playerId", serde_json::Value::String(n)) => {
                        names.player(n, &path)?;
                    }
                    _ => {}
                }
            }
        }
    }
    // Assertions.
    for (i, a) in s.assertions.iter().enumerate() {
        let base = format!("/assertions/{i}");
        match a {
            Assertion::Characteristics { object, expect, .. } => {
                names.object(object, &format!("{base}/object"))?;
                if let Some(c) = &expect.controller {
                    names.player(c, &format!("{base}/expect/controller"))?;
                }
            }
            Assertion::Zone {
                owner, contents, ..
            } => {
                if let Some(o) = owner {
                    names.player(o, &format!("{base}/owner"))?;
                }
                for (j, c) in contents.iter().enumerate() {
                    names.object(c, &format!("{base}/contents/{j}"))?;
                }
            }
            Assertion::Life { player, .. } => names.player(player, &format!("{base}/player"))?,
            Assertion::Counters { target, .. } | Assertion::Designation { target, .. } => {
                names.player_or_object(target, &format!("{base}/target"))?;
            }
            Assertion::Events { .. } => {}
            Assertion::Absent { owner, object, .. } => {
                if let Some(o) = owner {
                    names.player(o, &format!("{base}/owner"))?;
                }
                if let Some(o) = object {
                    names.object(o, &format!("{base}/object"))?;
                }
            }
            Assertion::PendingChoice { chooser, .. } => {
                if let Some(c) = chooser {
                    names.player(c, &format!("{base}/chooser"))?;
                }
            }
            Assertion::Result {
                winners, losers, ..
            } => {
                for (key, list) in [("winners", winners), ("losers", losers)] {
                    for (j, p) in list.iter().flatten().enumerate() {
                        names.player(p, &format!("{base}/{key}/{j}"))?;
                    }
                }
            }
        }
    }
    Ok(())
}
