// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! MTGJSON `AtomicCards` cross-validation (REQ-ING-001: "Any disagreement in a characteristic
//! SHALL be reported").
//!
//! Entries are matched to records through `identifiers.scryfallOracleId`. The list elements
//! under one name are the faces, in order; the first is compared with the record's front face.
//! Compared: `manaValue` (the `*2` integer rule), `colors` and `colorIdentity` (letters),
//! `types`, `supertypes`, `subtypes` (sorted lists) and `power`, `toughness`, `loyalty`
//! (string equality). The report prints numbers and color letters, and only the kind of any
//! other value (never names or text, R12). It never changes the normalized records.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::is_uuid_shape;
use super::normalize::{NormalizedCardRecord, NormalizedFace};
use super::recompute::{cmc_matches, letters_mask, mask_letters};

/// The version of the report.
pub const REPORT_FORMAT_VERSION: u64 = 1;

/// Why an `AtomicCards` file cannot be used at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomicError {
    /// Not JSON.
    NotJson,
    /// Not an object with a `data` object.
    NoData,
}

impl std::fmt::Display for AtomicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AtomicError::NotJson => "the AtomicCards file is not well-formed JSON",
            AtomicError::NoData => "the AtomicCards file is not an object with a `data` object",
        })
    }
}

/// The parsed `AtomicCards`: the face lists by `scryfallOracleId`, and the count of elements
/// that could not be used.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Atomic {
    /// Faces (in order) by `oracle_id`.
    pub by_oracle_id: BTreeMap<String, Vec<Value>>,
    /// Elements that are not objects or lack a UUID-shaped `identifiers.scryfallOracleId`.
    pub invalid_entries: usize,
}

/// Parses an `AtomicCards` file.
///
/// # Errors
/// [`AtomicError`].
pub fn parse_atomic(bytes: &[u8]) -> Result<Atomic, AtomicError> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| AtomicError::NotJson)?;
    let Some(Value::Object(data)) = v.get("data") else {
        return Err(AtomicError::NoData);
    };
    let mut out = Atomic::default();
    for list in data.values() {
        let Some(list) = list.as_array() else {
            out.invalid_entries += 1;
            continue;
        };
        for e in list {
            let id = e
                .get("identifiers")
                .and_then(|i| i.get("scryfallOracleId"))
                .and_then(Value::as_str)
                .filter(|s| is_uuid_shape(s));
            match id {
                Some(id) if e.is_object() => {
                    out.by_oracle_id
                        .entry(id.to_owned())
                        .or_default()
                        .push(e.clone());
                }
                _ => out.invalid_entries += 1,
            }
        }
    }
    Ok(out)
}

fn kind(v: Option<&Value>) -> &'static str {
    match v {
        None | Some(Value::Null) => "absent",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "list",
        Some(Value::Number(_)) => "number",
        Some(Value::Bool(_)) => "bool",
        Some(Value::Object(_)) => "object",
    }
}

fn str_kind(v: Option<&String>) -> &'static str {
    if v.is_some() { "string" } else { "absent" }
}

fn sorted_strings(v: Option<&Value>) -> Option<Vec<String>> {
    match v {
        None | Some(Value::Null) => Some(Vec::new()),
        Some(Value::Array(a)) => {
            let mut l = a
                .iter()
                .map(|x| x.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()?;
            l.sort();
            Some(l)
        }
        Some(_) => None,
    }
}

fn letters(v: Option<&Value>) -> Option<fineprint_dsl::card::ColorBitmask> {
    match v {
        None | Some(Value::Null) => letters_mask([]),
        Some(Value::Array(a)) => {
            let l = a.iter().map(Value::as_str).collect::<Option<Vec<_>>>()?;
            letters_mask(l)
        }
        Some(_) => None,
    }
}

fn compare(r: &NormalizedCardRecord, front: &NormalizedFace, m: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    // manaValue: numbers may be printed.
    let mv = m.get("manaValue");
    if !mv
        .and_then(Value::as_f64)
        .is_some_and(|x| cmc_matches(x, r.mana_value))
    {
        out.push(json!({
            "field": "manaValue",
            "scryfall": r.mana_value,
            "mtgjson": mv.filter(|v| v.is_number()).cloned().unwrap_or(Value::String(kind(mv).to_owned())),
        }));
    }
    // colors and colorIdentity: color letters may be printed.
    for (field, ours) in [
        ("colors", front.computed_colors),
        ("colorIdentity", r.color_identity),
    ] {
        let theirs = letters(m.get(field));
        if theirs != Some(ours) {
            out.push(json!({
                "field": field,
                "scryfall": mask_letters(ours),
                "mtgjson": theirs.map_or_else(|| "invalid".to_owned(), mask_letters),
            }));
        }
    }
    // Type words: only the kinds and counts.
    for (field, ours) in [
        ("types", &front.card_types),
        ("supertypes", &front.supertypes),
        ("subtypes", &front.subtypes),
    ] {
        let mut ours = ours.clone();
        ours.sort();
        let theirs = sorted_strings(m.get(field));
        if theirs.as_ref() != Some(&ours) {
            out.push(json!({
                "field": field,
                "scryfall": format!("list({})", ours.len()),
                "mtgjson": theirs.map_or_else(|| kind(m.get(field)).to_owned(), |t| format!("list({})", t.len())),
            }));
        }
    }
    // P/T and loyalty: string equality, kinds only.
    for (field, ours) in [
        ("power", front.power.as_ref()),
        ("toughness", front.toughness.as_ref()),
        ("loyalty", front.loyalty.as_ref()),
    ] {
        let theirs = m.get(field);
        let equal = match (ours, theirs) {
            (None, None | Some(Value::Null)) => true,
            (Some(a), Some(Value::String(b))) => a == b,
            _ => false,
        };
        if !equal {
            out.push(json!({
                "field": field,
                "scryfall": str_kind(ours),
                "mtgjson": kind(theirs),
            }));
        }
    }
    out
}

/// Builds the cross-validation report: per matched `oracle_id` the disagreeing fields, the
/// `oracle_id`s present in only one source, and counts. Deterministic (sorted by `oracle_id`).
#[must_use]
pub fn cross_check(
    records: &BTreeMap<String, NormalizedCardRecord>,
    atomic: &Atomic,
    source: &Value,
) -> Value {
    let mut disagreements = Vec::new();
    let mut agreed = 0usize;
    let mut only_scryfall = Vec::new();
    for (id, r) in records {
        let (Some(faces), Some(front)) = (atomic.by_oracle_id.get(id), r.faces.first()) else {
            only_scryfall.push(id.clone());
            continue;
        };
        let Some(m) = faces.first() else {
            only_scryfall.push(id.clone());
            continue;
        };
        let fields = compare(r, front, m);
        if fields.is_empty() {
            agreed += 1;
        } else {
            disagreements.push(json!({ "oracleId": id, "fields": fields }));
        }
    }
    let only_mtgjson: Vec<&String> = atomic
        .by_oracle_id
        .keys()
        .filter(|k| !records.contains_key(*k))
        .collect();
    json!({
        "formatVersion": REPORT_FORMAT_VERSION,
        "source": source,
        "matched": agreed + disagreements.len(),
        "agreed": agreed,
        "disagreements": disagreements,
        "onlyInScryfall": { "count": only_scryfall.len(), "oracleIds": only_scryfall },
        "onlyInMtgjson": { "count": only_mtgjson.len(), "oracleIds": only_mtgjson },
        "invalidEntries": atomic.invalid_entries,
    })
}
