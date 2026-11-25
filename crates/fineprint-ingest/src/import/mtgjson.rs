// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! MTGJSON `AtomicCards` cross-validation (REQ-ING-001: "Any disagreement in a characteristic
//! SHALL be reported").
//!
//! Entries are matched to records through `identifiers.scryfallOracleId`. The entries of one
//! `oracle_id` are its faces, ordered by their `side` (`a`, `b`, ...; source order when any
//! entry has no string `side`), and face `i` is compared with the record's face `i`.
//! Card-wide, compared once (from the first entry): `manaValue` (the `*2` integer rule) and
//! `colorIdentity` (letters). Per face, each disagreement carrying its `face` index: `colors`
//! (letters), `types`, `supertypes`, `subtypes` (sorted lists) and `power`, `toughness`,
//! `loyalty`, `defense` (string equality). A different number of faces is the field `faces`
//! (the common leading faces are still compared). The report prints numbers and color letters,
//! and only the kind of any other value (never names or text, R12). It never changes the
//! normalized records.

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
    // Take the `data` object out of the tree and move its entries into the index (no copy).
    let Value::Object(mut root) = v else {
        return Err(AtomicError::NoData);
    };
    let Some(Value::Object(data)) = root.remove("data") else {
        return Err(AtomicError::NoData);
    };
    drop(root);
    let mut out = Atomic::default();
    for list in data.into_iter().map(|(_, l)| l) {
        let Value::Array(list) = list else {
            out.invalid_entries += 1;
            continue;
        };
        for e in list {
            let id = e
                .get("identifiers")
                .and_then(|i| i.get("scryfallOracleId"))
                .and_then(Value::as_str)
                .filter(|s| is_uuid_shape(s))
                .map(str::to_owned);
            match id {
                Some(id) if e.is_object() => out.by_oracle_id.entry(id).or_default().push(e),
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

/// The card-wide characteristics: `manaValue` and `colorIdentity`, compared once.
fn compare_card(r: &NormalizedCardRecord, m: &Value) -> Vec<Value> {
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
    // colorIdentity: color letters may be printed.
    let theirs = letters(m.get("colorIdentity"));
    if theirs != Some(r.color_identity) {
        out.push(json!({
            "field": "colorIdentity",
            "scryfall": mask_letters(r.color_identity),
            "mtgjson": theirs.map_or_else(|| "invalid".to_owned(), mask_letters),
        }));
    }
    out
}

/// The per-face characteristics of face `index` (`face` is the record's, `m` the matching
/// MTGJSON entry); each disagreement carries `"face": index`.
fn compare_face(index: usize, face: &NormalizedFace, m: &Value, out: &mut Vec<Value>) {
    // colors: color letters may be printed.
    let theirs = letters(m.get("colors"));
    if theirs != Some(face.computed_colors) {
        out.push(json!({
            "face": index,
            "field": "colors",
            "scryfall": mask_letters(face.computed_colors),
            "mtgjson": theirs.map_or_else(|| "invalid".to_owned(), mask_letters),
        }));
    }
    // Type words: only the kinds and counts.
    for (field, ours) in [
        ("types", &face.card_types),
        ("supertypes", &face.supertypes),
        ("subtypes", &face.subtypes),
    ] {
        let mut ours = ours.clone();
        ours.sort();
        let theirs = sorted_strings(m.get(field));
        if theirs.as_ref() != Some(&ours) {
            out.push(json!({
                "face": index,
                "field": field,
                "scryfall": format!("list({})", ours.len()),
                "mtgjson": theirs.map_or_else(|| kind(m.get(field)).to_owned(), |t| format!("list({})", t.len())),
            }));
        }
    }
    // P/T, loyalty and defense: string equality, kinds only.
    for (field, ours) in [
        ("power", face.power.as_ref()),
        ("toughness", face.toughness.as_ref()),
        ("loyalty", face.loyalty.as_ref()),
        ("defense", face.defense.as_ref()),
    ] {
        let theirs = m.get(field);
        let equal = match (ours, theirs) {
            (None, None | Some(Value::Null)) => true,
            (Some(a), Some(Value::String(b))) => a == b,
            _ => false,
        };
        if !equal {
            out.push(json!({
                "face": index,
                "field": field,
                "scryfall": str_kind(ours),
                "mtgjson": kind(theirs),
            }));
        }
    }
}

/// The MTGJSON entries of one card in face order: by `side` when every entry has a string
/// `side` (a stable sort, so equal sides keep source order), else in source order.
fn ordered_faces(entries: &[Value]) -> Vec<&Value> {
    let mut faces: Vec<&Value> = entries.iter().collect();
    if faces
        .iter()
        .all(|e| e.get("side").is_some_and(Value::is_string))
    {
        faces.sort_by(|a, b| {
            let side = |v: &Value| {
                v.get("side")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            side(a).cmp(&side(b))
        });
    }
    faces
}

/// The index of the normalized face an MTGJSON entry describes by its `side`, when EVERY entry
/// has a single-letter lowercase `side` in `a..=z` (`a` is face 0); `None` otherwise, when the
/// entries pair with the faces by position.
fn side_indices(entries: &[&Value]) -> Option<Vec<usize>> {
    entries
        .iter()
        .map(|e| {
            let side = e.get("side")?.as_str()?;
            let mut chars = side.chars();
            match (chars.next(), chars.next()) {
                (Some(c @ 'a'..='z'), None) => Some(usize::from(c as u8 - b'a')),
                _ => None,
            }
        })
        .collect()
}

/// Every disagreement of record `r` with its MTGJSON entries `faces` (non-empty, in face
/// order): card-wide fields, then `faces` when the counts differ, then each common face. Entries
/// that all carry a letter `side` pair with the face that letter names (a lone back face is
/// compared with the back face); an entry whose letter names a face the record lacks is a
/// `side` disagreement. Otherwise they pair by position.
fn compare(r: &NormalizedCardRecord, faces: &[&Value]) -> Vec<Value> {
    let Some(first) = faces.first() else {
        return Vec::new();
    };
    let mut out = compare_card(r, first);
    if faces.len() != r.faces.len() {
        out.push(json!({
            "field": "faces",
            "scryfall": r.faces.len(),
            "mtgjson": faces.len(),
        }));
    }
    match side_indices(faces) {
        Some(indices) => {
            for (index, m) in indices.into_iter().zip(faces) {
                if let Some(face) = r.faces.get(index) {
                    compare_face(index, face, m, &mut out);
                } else {
                    // The entry names a face the record lacks: report it (kinds only, never
                    // the entry's own text) instead of dropping it.
                    out.push(json!({
                        "face": index,
                        "field": "side",
                        "scryfall": "absent",
                        "mtgjson": "present",
                    }));
                }
            }
        }
        None => {
            for (i, (face, m)) in r.faces.iter().zip(faces).enumerate() {
                compare_face(i, face, m, &mut out);
            }
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
        let Some(entries) = atomic.by_oracle_id.get(id) else {
            only_scryfall.push(id.clone());
            continue;
        };
        if entries.is_empty() || r.faces.is_empty() {
            only_scryfall.push(id.clone());
            continue;
        }
        let fields = compare(r, &ordered_faces(entries));
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
