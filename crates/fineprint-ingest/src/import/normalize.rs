// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Normalizing Scryfall Oracle Cards and Rulings into [`NormalizedCardRecord`]s keyed by
//! `oracle_id` (REQ-ING-001, REQ-ING-002 per SRS §16.3).
//!
//! The input is untrusted (R12): every field is type-checked, a wrong type rejects only that
//! record with a fixed reason naming the field, and no reason, warning or diagnostic ever quotes
//! a value from the input (only indexes, valid `oracle_id`s and fixed strings). Records are kept
//! in a `BTreeMap` by `oracle_id`, so the output is sorted and byte-identical for identical input
//! (R3). This unit keeps the raw Scryfall `layout` (the `LayoutBehavior` mapping is
//! REQ-ING-003, a later unit), splits the type line without validating it against the CR 205
//! vocabulary (a later unit) and keeps P/T, loyalty and defense as the source strings.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::sha256;
use fineprint_dsl::card::ColorBitmask;
use fineprint_dsl::mana::ManaCostDSL;
use serde_json::{Map, Value, json};

use super::is_uuid_shape;
use super::json_array::{ArrayError, split_array};
use super::recompute::{cmc_matches, color_identity, face_colors, letters_mask, record_mana_value};

/// The version of the normalized output file.
pub const FORMAT_VERSION: u64 = 1;

/// The supertype words of a type line (the split is lexical only; no CR 205 validation here).
pub const SUPERTYPES: [&str; 7] = [
    "Basic",
    "Legendary",
    "Ongoing",
    "Snow",
    "World",
    "Elite",
    "Host",
];

/// A fixed-string flag on a record: a cross-check mismatch or a shape remark. Never quotes
/// input text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Diagnostic {
    /// The source `cmc` does not equal the recomputed mana value.
    ManaValueMismatch,
    /// The source `cmc` is missing, not a number or not finite.
    CmcMissing,
    /// The source `colors` differ from the computed colors (front face).
    ColorsMismatch,
    /// The source `color_identity` differs from the recomputed identity.
    ColorIdentityMismatch,
    /// A face's type line is missing or empty.
    TypeLineEmpty,
    /// A combined `A // B` type line does not have one part per face.
    TypeLineFaceMismatch,
    /// A non-land face of a `normal` card has no mana cost.
    EmptyManaCostUnexpected,
}

impl Diagnostic {
    /// The fixed wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Diagnostic::ManaValueMismatch => "manaValueMismatch",
            Diagnostic::CmcMissing => "cmcMissing",
            Diagnostic::ColorsMismatch => "colorsMismatch",
            Diagnostic::ColorIdentityMismatch => "colorIdentityMismatch",
            Diagnostic::TypeLineEmpty => "typeLineEmpty",
            Diagnostic::TypeLineFaceMismatch => "typeLineFaceMismatch",
            Diagnostic::EmptyManaCostUnexpected => "emptyManaCostUnexpected",
        }
    }
}

/// One face of a [`NormalizedCardRecord`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedFace {
    /// The face name.
    pub name: String,
    /// The parsed mana cost; `None` for no mana cost (an empty or absent `mana_cost`).
    pub mana_cost: Option<ManaCostDSL>,
    /// The raw `mana_cost` string, or `None` when absent.
    pub mana_cost_text: Option<String>,
    /// Supertype words of the type line, in source order.
    pub supertypes: Vec<String>,
    /// Card type words of the type line, in source order.
    pub card_types: Vec<String>,
    /// Subtype words (right of the em dash), in source order.
    pub subtypes: Vec<String>,
    /// The raw Oracle text (tokenizer input for later units), or `None`.
    pub oracle_text: Option<String>,
    /// The source power string (such as `"*"` or `"1+*"`), never converted to a number.
    pub power: Option<String>,
    /// The source toughness string.
    pub toughness: Option<String>,
    /// The source loyalty string.
    pub loyalty: Option<String>,
    /// The source defense string.
    pub defense: Option<String>,
    /// The source `colors`, converted to a mask (`None` when absent).
    pub colors: Option<ColorBitmask>,
    /// The source `color_indicator`, converted to a mask (`None` when absent).
    pub color_indicator: Option<ColorBitmask>,
    /// The recomputed colors: the indicator when present, otherwise the cost colors.
    pub computed_colors: ColorBitmask,
}

/// One `all_parts` entry, kept raw for the later layout unit (`relatedTokens`, `meldPartner`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedPart {
    /// The related object's printing id.
    pub id: Option<String>,
    /// The Scryfall component string.
    pub component: Option<String>,
    /// The related object's name.
    pub name: Option<String>,
}

/// A normalized Oracle card (SRS §16.3 REQ-ING-002), keyed by `oracle_id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedCardRecord {
    /// The `oracle_id` (never the printing `id`).
    pub card_def_id: String,
    /// The top-level `name`.
    pub name: String,
    /// The raw Scryfall `layout` string (mapped by a later unit, REQ-ING-003).
    pub layout: String,
    /// One entry per `card_faces` entry, or one face built from the top-level fields.
    pub faces: Vec<NormalizedFace>,
    /// The recomputed color identity (CR 903.4).
    pub color_identity: ColorBitmask,
    /// The source `color_identity`, converted (`None` when absent).
    pub source_color_identity: Option<ColorBitmask>,
    /// The recomputed integer mana value (CR 202.3).
    pub mana_value: u64,
    /// Sorted copy of the source `keywords` (a cross-check set, never behavior).
    pub keywords: Vec<String>,
    /// The raw `all_parts` entries.
    pub all_parts: Vec<RelatedPart>,
    /// Format → legality string, copied raw.
    pub legalities: BTreeMap<String, String>,
    /// The source `game_changer` (default false).
    pub game_changer: bool,
    /// `digital` is true, or `games` is a non-empty list without `"paper"`.
    pub digital_only: bool,
    /// Lowercase hex SHA-256 of the faces' Oracle texts joined with `\n` (null as empty).
    pub oracle_text_hash: String,
    /// Mismatch and shape flags, sorted, without duplicates.
    pub diagnostics: Vec<Diagnostic>,
}

fn opt_str(s: Option<&String>) -> Value {
    s.map_or(Value::Null, |s| Value::String(s.clone()))
}

fn opt_mask(m: Option<ColorBitmask>) -> Value {
    m.map_or(Value::Null, |m| Value::from(m.bits()))
}

fn str_list(v: &[String]) -> Value {
    Value::Array(v.iter().cloned().map(Value::String).collect())
}

impl NormalizedFace {
    /// The face as JSON (camelCase members; `manaCost` printed with
    /// `ManaCostDSL::print_optional`, or null for no cost).
    #[must_use]
    pub fn to_json_value(&self) -> Value {
        json!({
            "name": self.name,
            "manaCost": self.mana_cost.as_ref().map_or(Value::Null, |c| {
                Value::String(ManaCostDSL::print_optional(Some(c)))
            }),
            "manaCostText": opt_str(self.mana_cost_text.as_ref()),
            "supertypes": str_list(&self.supertypes),
            "cardTypes": str_list(&self.card_types),
            "subtypes": str_list(&self.subtypes),
            "oracleText": opt_str(self.oracle_text.as_ref()),
            "power": opt_str(self.power.as_ref()),
            "toughness": opt_str(self.toughness.as_ref()),
            "loyalty": opt_str(self.loyalty.as_ref()),
            "defense": opt_str(self.defense.as_ref()),
            "colors": opt_mask(self.colors),
            "colorIndicator": opt_mask(self.color_indicator),
            "computedColors": self.computed_colors.bits(),
        })
    }
}

impl NormalizedCardRecord {
    /// The record as JSON with camelCase members (`serde_json`'s map keeps them sorted, a fixed
    /// order).
    #[must_use]
    pub fn to_json_value(&self) -> Value {
        json!({
            "cardDefId": self.card_def_id,
            "name": self.name,
            "layout": self.layout,
            "faces": self.faces.iter().map(NormalizedFace::to_json_value).collect::<Vec<_>>(),
            "colorIdentity": self.color_identity.bits(),
            "sourceColorIdentity": opt_mask(self.source_color_identity),
            "manaValue": self.mana_value,
            "keywords": str_list(&self.keywords),
            "allParts": self.all_parts.iter().map(|p| json!({
                "id": opt_str(p.id.as_ref()),
                "component": opt_str(p.component.as_ref()),
                "name": opt_str(p.name.as_ref()),
            })).collect::<Vec<_>>(),
            "legalities": self.legalities.iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect::<Map<String, Value>>(),
            "gameChanger": self.game_changer,
            "digitalOnly": self.digital_only,
            "oracleTextHash": self.oracle_text_hash,
            "diagnostics": self.diagnostics.iter().map(|d| d.as_str()).collect::<Vec<_>>(),
        })
    }
}

/// Why a source object was rejected: a fixed reason, never input text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// The element is not a JSON object (or did not parse).
    NotObject,
    /// The object has no `oracle_id` (not a gameplay object).
    NotGameplayObject,
    /// The `oracle_id` is not a lowercase UUID shape.
    OracleIdShape,
    /// The `oracle_id` was already seen (the first object is kept).
    DuplicateOracleId,
    /// A required field is missing.
    MissingField(&'static str),
    /// A field has the wrong JSON type.
    FieldType(&'static str),
    /// A color letter list holds something other than `W U B R G`.
    UnknownColor(&'static str),
    /// A mana cost does not parse.
    ManaCostParse,
}

impl RejectReason {
    /// The fixed wire string (`fieldType:<field>` and similar name only a field).
    #[must_use]
    pub fn as_string(&self) -> String {
        match self {
            RejectReason::NotObject => "notObject".to_owned(),
            RejectReason::NotGameplayObject => "notGameplayObject".to_owned(),
            RejectReason::OracleIdShape => "oracleIdShape".to_owned(),
            RejectReason::DuplicateOracleId => "duplicateOracleId".to_owned(),
            RejectReason::MissingField(f) => format!("missingField:{f}"),
            RejectReason::FieldType(f) => format!("fieldType:{f}"),
            RejectReason::UnknownColor(f) => format!("unknownColor:{f}"),
            RejectReason::ManaCostParse => "manaCostParse".to_owned(),
        }
    }
}

/// A rejected source object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// The index of the element in the source array.
    pub index: usize,
    /// The `oracle_id`, only when it has the UUID shape.
    pub oracle_id: Option<String>,
    /// The fixed reason.
    pub reason: RejectReason,
}

impl Rejection {
    /// The rejection as JSON (`index`, `oracleId`, `reason`).
    #[must_use]
    pub fn to_json_value(&self) -> Value {
        json!({
            "index": self.index,
            "oracleId": opt_str(self.oracle_id.as_ref()),
            "reason": self.reason.as_string(),
        })
    }
}

type Obj = Map<String, Value>;

/// An optional string field: absent or null is `None`; any other non-string is a type error.
fn get_str(
    o: &Obj,
    key: &'static str,
    field: &'static str,
) -> Result<Option<String>, RejectReason> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(RejectReason::FieldType(field)),
    }
}

fn get_bool(o: &Obj, key: &'static str) -> Result<Option<bool>, RejectReason> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(RejectReason::FieldType(key)),
    }
}

fn get_str_list(
    o: &Obj,
    key: &'static str,
    field: &'static str,
) -> Result<Option<Vec<String>>, RejectReason> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| match v {
                Value::String(s) => Ok(s.clone()),
                _ => Err(RejectReason::FieldType(field)),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(RejectReason::FieldType(field)),
    }
}

fn get_colors(
    o: &Obj,
    key: &'static str,
    field: &'static str,
) -> Result<Option<ColorBitmask>, RejectReason> {
    match get_str_list(o, key, field)? {
        None => Ok(None),
        Some(l) => letters_mask(l.iter().map(String::as_str))
            .map(Some)
            .ok_or(RejectReason::UnknownColor(field)),
    }
}

/// The raw fields of one face before recomputation.
struct RawFace {
    name: String,
    mana_cost_text: Option<String>,
    mana_cost: Option<ManaCostDSL>,
    type_line: Option<String>,
    oracle_text: Option<String>,
    power: Option<String>,
    toughness: Option<String>,
    loyalty: Option<String>,
    defense: Option<String>,
    colors: Option<ColorBitmask>,
    color_indicator: Option<ColorBitmask>,
}

/// Field names for top-level and per-face reasons.
struct Names {
    name: &'static str,
    mana_cost: &'static str,
    type_line: &'static str,
    oracle_text: &'static str,
    power: &'static str,
    toughness: &'static str,
    loyalty: &'static str,
    defense: &'static str,
    colors: &'static str,
    color_indicator: &'static str,
}

const TOP: Names = Names {
    name: "name",
    mana_cost: "mana_cost",
    type_line: "type_line",
    oracle_text: "oracle_text",
    power: "power",
    toughness: "toughness",
    loyalty: "loyalty",
    defense: "defense",
    colors: "colors",
    color_indicator: "color_indicator",
};

const FACE: Names = Names {
    name: "card_faces.name",
    mana_cost: "card_faces.mana_cost",
    type_line: "card_faces.type_line",
    oracle_text: "card_faces.oracle_text",
    power: "card_faces.power",
    toughness: "card_faces.toughness",
    loyalty: "card_faces.loyalty",
    defense: "card_faces.defense",
    colors: "card_faces.colors",
    color_indicator: "card_faces.color_indicator",
};

fn raw_face(o: &Obj, n: &Names) -> Result<RawFace, RejectReason> {
    let name = get_str(o, "name", n.name)?.ok_or(RejectReason::MissingField(n.name))?;
    let mana_cost_text = get_str(o, "mana_cost", n.mana_cost)?;
    let mana_cost = match &mana_cost_text {
        None => None,
        Some(t) => ManaCostDSL::parse_optional(t).map_err(|_| RejectReason::ManaCostParse)?,
    };
    Ok(RawFace {
        name,
        mana_cost_text,
        mana_cost,
        type_line: get_str(o, "type_line", n.type_line)?,
        oracle_text: get_str(o, "oracle_text", n.oracle_text)?,
        power: get_str(o, "power", n.power)?,
        toughness: get_str(o, "toughness", n.toughness)?,
        loyalty: get_str(o, "loyalty", n.loyalty)?,
        defense: get_str(o, "defense", n.defense)?,
        colors: get_colors(o, "colors", n.colors)?,
        color_indicator: get_colors(o, "color_indicator", n.color_indicator)?,
    })
}

/// A type line split on the first em dash (U+2014): supertypes and card types on the left,
/// subtypes on the right (lexical split only, no CR 205 vocabulary validation in this unit).
#[must_use]
pub fn split_type_line(line: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (left, right) = line.split_once('\u{2014}').unwrap_or((line, ""));
    let mut supertypes = Vec::new();
    let mut card_types = Vec::new();
    for w in left.split_whitespace() {
        if SUPERTYPES.contains(&w) {
            supertypes.push(w.to_owned());
        } else {
            card_types.push(w.to_owned());
        }
    }
    let subtypes = right.split_whitespace().map(str::to_owned).collect();
    (supertypes, card_types, subtypes)
}

fn digital_only(digital: Option<bool>, games: Option<&[String]>) -> bool {
    digital == Some(true) || games.is_some_and(|g| !g.is_empty() && !g.iter().any(|x| x == "paper"))
}

/// The lowercase hex SHA-256 of the faces' Oracle texts joined with `\n` (null as empty).
#[must_use]
pub fn oracle_text_hash(texts: &[Option<&str>]) -> String {
    let joined = texts
        .iter()
        .map(|t| t.unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    sha256(joined.as_bytes()).to_hex()
}

fn all_parts(o: &Obj) -> Result<Vec<RelatedPart>, RejectReason> {
    match o.get("all_parts") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .map(|p| {
                let Value::Object(p) = p else {
                    return Err(RejectReason::FieldType("all_parts"));
                };
                Ok(RelatedPart {
                    id: get_str(p, "id", "all_parts.id")?,
                    component: get_str(p, "component", "all_parts.component")?,
                    name: get_str(p, "name", "all_parts.name")?,
                })
            })
            .collect(),
        Some(_) => Err(RejectReason::FieldType("all_parts")),
    }
}

fn legalities(o: &Obj) -> Result<BTreeMap<String, String>, RejectReason> {
    match o.get("legalities") {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(m)) => m
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => Ok((k.clone(), s.clone())),
                _ => Err(RejectReason::FieldType("legalities")),
            })
            .collect(),
        Some(_) => Err(RejectReason::FieldType("legalities")),
    }
}

/// The `oracle_id` of a source object: `Err(NotGameplayObject)` when absent, a type or shape
/// error otherwise.
///
/// # Errors
/// The [`RejectReason`].
pub fn oracle_id_of(v: &Value) -> Result<String, RejectReason> {
    let Value::Object(o) = v else {
        return Err(RejectReason::NotObject);
    };
    match o.get("oracle_id") {
        None | Some(Value::Null) => Err(RejectReason::NotGameplayObject),
        Some(Value::String(s)) if is_uuid_shape(s) => Ok(s.clone()),
        Some(Value::String(_)) => Err(RejectReason::OracleIdShape),
        Some(_) => Err(RejectReason::FieldType("oracle_id")),
    }
}

/// Normalizes one Scryfall card object (REQ-ING-002).
///
/// # Errors
/// The fixed [`RejectReason`] of the first problem found; nothing else is rejected.
#[allow(clippy::too_many_lines)]
pub fn normalize_card(v: &Value) -> Result<NormalizedCardRecord, RejectReason> {
    let card_def_id = oracle_id_of(v)?;
    let Value::Object(o) = v else {
        return Err(RejectReason::NotObject);
    };
    let name = get_str(o, "name", "name")?.ok_or(RejectReason::MissingField("name"))?;
    let layout = get_str(o, "layout", "layout")?.ok_or(RejectReason::MissingField("layout"))?;
    let top_type_line = get_str(o, "type_line", "type_line")?;
    let mut raw = match o.get("card_faces") {
        None | Some(Value::Null) => vec![raw_face(o, &TOP)?],
        Some(Value::Array(fs)) if !fs.is_empty() => fs
            .iter()
            .map(|f| match f {
                Value::Object(f) => raw_face(f, &FACE),
                _ => Err(RejectReason::FieldType("card_faces")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(RejectReason::FieldType("card_faces")),
    };
    let multi = o.get("card_faces").is_some_and(Value::is_array);
    let top_colors = get_colors(o, "colors", "colors")?;
    let source_color_identity = get_colors(o, "color_identity", "color_identity")?;
    let mut keywords = get_str_list(o, "keywords", "keywords")?.unwrap_or_default();
    keywords.sort();
    let games = get_str_list(o, "games", "games")?;
    let digital = get_bool(o, "digital")?;
    let game_changer = get_bool(o, "game_changer")?.unwrap_or(false);
    let all_parts = all_parts(o)?;
    let legalities = legalities(o)?;

    let mut diagnostics = BTreeSet::new();

    // Type lines: a face's own line, else the per-face part of a combined top-level line.
    let any_face_line = multi && raw.iter().any(|f| f.type_line.is_some());
    if multi && !any_face_line {
        let line = top_type_line.clone().unwrap_or_default();
        let parts: Vec<&str> = line.split(" // ").collect();
        if parts.len() == raw.len() {
            for (f, p) in raw.iter_mut().zip(parts) {
                f.type_line = Some(p.to_owned());
            }
        } else if parts.len() == 1 && !line.is_empty() {
            for f in &mut raw {
                f.type_line = Some(line.clone());
            }
        } else if !line.is_empty() {
            diagnostics.insert(Diagnostic::TypeLineFaceMismatch);
        }
    } else if multi {
        for f in &mut raw {
            if f.type_line.is_none() {
                f.type_line.clone_from(&top_type_line);
            }
        }
    }

    let mut faces = Vec::with_capacity(raw.len());
    for f in raw {
        let line = f.type_line.as_deref().unwrap_or("");
        if line.trim().is_empty() {
            diagnostics.insert(Diagnostic::TypeLineEmpty);
        }
        let (supertypes, card_types, subtypes) = split_type_line(line);
        if f.mana_cost.is_none() && layout == "normal" && !card_types.iter().any(|t| t == "Land") {
            diagnostics.insert(Diagnostic::EmptyManaCostUnexpected);
        }
        let computed_colors = face_colors(f.mana_cost.as_ref(), f.color_indicator);
        faces.push(NormalizedFace {
            name: f.name,
            mana_cost: f.mana_cost,
            mana_cost_text: f.mana_cost_text,
            supertypes,
            card_types,
            subtypes,
            oracle_text: f.oracle_text,
            power: f.power,
            toughness: f.toughness,
            loyalty: f.loyalty,
            defense: f.defense,
            colors: f.colors,
            color_indicator: f.color_indicator,
            computed_colors,
        });
    }

    // CR 202.3 (per layout, see recompute::record_mana_value).
    let costs: Vec<Option<&ManaCostDSL>> = faces.iter().map(|f| f.mana_cost.as_ref()).collect();
    let mana_value = record_mana_value(&layout, &costs);
    match o.get("cmc").and_then(Value::as_f64) {
        None => {
            diagnostics.insert(Diagnostic::CmcMissing);
        }
        Some(c) if !c.is_finite() => {
            diagnostics.insert(Diagnostic::CmcMissing);
        }
        Some(c) => {
            if !cmc_matches(c, mana_value) {
                diagnostics.insert(Diagnostic::ManaValueMismatch);
            }
        }
    }

    // CR 903.4, 903.4c, 903.4d: every face, rules-text symbols outside reminder text.
    let identity_in: Vec<(ColorBitmask, Option<&str>)> = faces
        .iter()
        .map(|f| (f.computed_colors, f.oracle_text.as_deref()))
        .collect();
    let color_identity = color_identity(&identity_in);
    if source_color_identity.is_some_and(|s| s != color_identity) {
        diagnostics.insert(Diagnostic::ColorIdentityMismatch);
    }

    // Colors cross-check against the front (or only) face.
    if let Some(front) = faces.first() {
        let source = if multi {
            front.colors.or(top_colors)
        } else {
            top_colors
        };
        if source.is_some_and(|s| s != front.computed_colors) {
            diagnostics.insert(Diagnostic::ColorsMismatch);
        }
    }

    let texts: Vec<Option<&str>> = faces.iter().map(|f| f.oracle_text.as_deref()).collect();
    let oracle_text_hash = oracle_text_hash(&texts);
    let digital_only = digital_only(digital, games.as_deref());

    Ok(NormalizedCardRecord {
        card_def_id,
        name,
        layout,
        faces,
        color_identity,
        source_color_identity,
        mana_value,
        keywords,
        all_parts,
        legalities,
        game_changer,
        digital_only,
        oracle_text_hash,
        diagnostics: diagnostics.into_iter().collect(),
    })
}

/// The result of normalizing an Oracle Cards array.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OracleImport {
    /// Accepted records by `oracle_id`.
    pub records: BTreeMap<String, NormalizedCardRecord>,
    /// Rejected source objects, in source order.
    pub rejected: Vec<Rejection>,
}

/// Normalizes a Scryfall Oracle Cards bulk file (a JSON array). Each element is parsed and
/// normalized on its own; a bad element rejects only itself.
///
/// # Errors
/// [`ArrayError`] when the file is not a JSON array at all.
pub fn import_oracle_cards(bytes: &[u8]) -> Result<OracleImport, ArrayError> {
    let mut out = OracleImport::default();
    for (index, slice) in split_array(bytes)?.into_iter().enumerate() {
        let Ok(v) = serde_json::from_slice::<Value>(slice) else {
            out.rejected.push(Rejection {
                index,
                oracle_id: None,
                reason: RejectReason::NotObject,
            });
            continue;
        };
        let id = oracle_id_of(&v).ok();
        match normalize_card(&v) {
            Ok(r) if out.records.contains_key(&r.card_def_id) => out.rejected.push(Rejection {
                index,
                oracle_id: Some(r.card_def_id),
                reason: RejectReason::DuplicateOracleId,
            }),
            Ok(r) => {
                out.records.insert(r.card_def_id.clone(), r);
            }
            Err(reason) => out.rejected.push(Rejection {
                index,
                oracle_id: id,
                reason,
            }),
        }
    }
    Ok(out)
}

/// One ruling, in source order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ruling {
    /// The ruling's `source` (such as `wotc`).
    pub source: String,
    /// The `published_at` date string.
    pub published_at: String,
    /// The ruling text.
    pub comment: String,
}

/// The result of importing the Rulings bulk file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RulingsImport {
    /// Rulings by `oracle_id` (only ids that have a record), each list in source order.
    pub rulings: BTreeMap<String, Vec<Ruling>>,
    /// Rulings whose `oracle_id` has no record.
    pub unknown_oracle_id: usize,
    /// Elements that are not ruling objects (wrong shape or types).
    pub invalid: usize,
}

/// Imports the Rulings bulk file and attaches the rulings to `records` by `oracle_id`.
///
/// # Errors
/// [`ArrayError`] when the file is not a JSON array at all.
pub fn import_rulings(
    bytes: &[u8],
    records: &BTreeMap<String, NormalizedCardRecord>,
) -> Result<RulingsImport, ArrayError> {
    let mut out = RulingsImport::default();
    for slice in split_array(bytes)? {
        let ruling = serde_json::from_slice::<Value>(slice).ok().and_then(|v| {
            let o = v.as_object()?;
            let id = o.get("oracle_id")?.as_str().filter(|s| is_uuid_shape(s))?;
            let field = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_owned);
            Some((
                id.to_owned(),
                Ruling {
                    source: field("source")?,
                    published_at: field("published_at")?,
                    comment: field("comment")?,
                },
            ))
        });
        match ruling {
            None => out.invalid += 1,
            Some((id, _)) if !records.contains_key(&id) => out.unknown_oracle_id += 1,
            Some((id, r)) => out.rulings.entry(id).or_default().push(r),
        }
    }
    Ok(out)
}

/// The provenance of one imported source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInfo {
    /// The bulk entry's `updated_at`, verbatim from the cache metadata (`None` if unknown).
    pub updated_at: Option<String>,
    /// Lowercase hex SHA-256 of the bytes actually read.
    pub sha256: String,
    /// The number of bytes read.
    pub byte_count: u64,
}

impl SourceInfo {
    /// The provenance as JSON (`updatedAt`, `sha256`, `byteCount`).
    #[must_use]
    pub fn to_json_value(&self) -> Value {
        json!({
            "updatedAt": opt_str(self.updated_at.as_ref()),
            "sha256": self.sha256,
            "byteCount": self.byte_count,
        })
    }
}

/// A complete import: provenance, records, rulings, rejections and warnings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportResult {
    /// Provenance of the Oracle Cards file.
    pub oracle_cards: SourceInfo,
    /// Provenance of the Rulings file, when present.
    pub rulings_source: Option<SourceInfo>,
    /// The records and rejections.
    pub oracle: OracleImport,
    /// The rulings by `oracle_id`.
    pub rulings: BTreeMap<String, Vec<Ruling>>,
    /// Fixed-string warnings (`code` or `code:count`), never input text.
    pub warnings: Vec<String>,
}

impl ImportResult {
    /// The number of records with at least one diagnostic.
    #[must_use]
    pub fn records_with_diagnostics(&self) -> usize {
        self.oracle
            .records
            .values()
            .filter(|r| !r.diagnostics.is_empty())
            .count()
    }

    /// The output file bytes: members in the fixed order `formatVersion`, `sources`,
    /// `records`, `rulings`, `rejected`, `warnings`, one record per line, UTF-8 with a final
    /// newline. Identical results give identical bytes (R3).
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let push = |out: &mut Vec<u8>, v: &Value| {
            out.extend_from_slice(serde_json::to_string(v).unwrap_or_default().as_bytes());
        };
        out.extend_from_slice(
            format!("{{\"formatVersion\":{FORMAT_VERSION},\n\"sources\":").as_bytes(),
        );
        push(
            &mut out,
            &json!({
                "oracleCards": self.oracle_cards.to_json_value(),
                "rulings": self.rulings_source.as_ref().map_or(Value::Null, SourceInfo::to_json_value),
            }),
        );
        out.extend_from_slice(b",\n\"records\":[");
        for (i, r) in self.oracle.records.values().enumerate() {
            out.extend_from_slice(if i == 0 { b"\n" } else { b",\n" });
            push(&mut out, &r.to_json_value());
        }
        out.extend_from_slice(b"\n],\n\"rulings\":{");
        for (i, (id, list)) in self.rulings.iter().enumerate() {
            out.extend_from_slice(if i == 0 { b"\n" } else { b",\n" });
            push(&mut out, &Value::String(id.clone()));
            out.push(b':');
            let list: Vec<Value> = list
                .iter()
                .map(|r| {
                    json!({
                        "source": r.source,
                        "publishedAt": r.published_at,
                        "comment": r.comment,
                    })
                })
                .collect();
            push(&mut out, &Value::Array(list));
        }
        out.extend_from_slice(b"\n},\n\"rejected\":");
        push(
            &mut out,
            &Value::Array(
                self.oracle
                    .rejected
                    .iter()
                    .map(Rejection::to_json_value)
                    .collect(),
            ),
        );
        out.extend_from_slice(b",\n\"warnings\":");
        push(
            &mut out,
            &Value::Array(self.warnings.iter().cloned().map(Value::String).collect()),
        );
        out.extend_from_slice(b"}\n");
        out
    }
}
