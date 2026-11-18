// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Validity helpers of the effect-instruction schema (REQ-DSL-010, REQ-NFR-006): opaque JSON
//! values (rule A5), non-empty lists, optional members and distinctness.

use std::collections::BTreeMap;

use serde::Serialize;
use serde::ser::SerializeMap;
use serde_json::Value;

use crate::canonical::MAX_DEPTH;
use crate::expr::json::{FromJson, JResult, Obj, array_of, bool_of, check_int, is_noncharacter};

/// Validity of an opaque JSON value (rule A5): no `null` anywhere, integers only and within
/// ±(2^53 − 1), no noncharacter in any string or member name (member names may be empty), and
/// nesting within [`MAX_DEPTH`] counting from `depth` (the depth a container at the position of
/// `v` has).
pub(crate) fn check_opaque(v: &Value, depth: usize, what: &str) -> JResult<()> {
    match v {
        Value::Null => Err(format!("{what} must not contain null")),
        Value::Bool(_) => Ok(()),
        Value::Number(n) => match n.as_i64() {
            Some(i) => check_int(i, what),
            None => Err(format!("{what} holds a non-integer or unsafe number {n}")),
        },
        Value::String(s) => check_chars(s, what),
        Value::Array(items) => {
            check_depth(depth, what)?;
            items
                .iter()
                .try_for_each(|i| check_opaque(i, depth + 1, what))
        }
        Value::Object(map) => {
            check_depth(depth, what)?;
            map.iter().try_for_each(|(k, i)| {
                check_chars(k, what)?;
                check_opaque(i, depth + 1, what)
            })
        }
    }
}

/// Fails when a container at `depth` would exceed [`MAX_DEPTH`].
fn check_depth(depth: usize, what: &str) -> JResult<()> {
    if depth > MAX_DEPTH {
        Err(format!(
            "{what} is nested deeper than {MAX_DEPTH} arrays and objects"
        ))
    } else {
        Ok(())
    }
}

/// Rejects a string holding a noncharacter.
fn check_chars(s: &str, what: &str) -> JResult<()> {
    if s.chars().any(is_noncharacter) {
        Err(format!(
            "{what}: string {s:?} contains a Unicode noncharacter"
        ))
    } else {
        Ok(())
    }
}

/// Validity of an opaque object held as a map (rule A5, counted from the object itself);
/// `non_empty` requires at least one member.
pub(crate) fn check_opaque_map(
    map: &BTreeMap<String, Value>,
    non_empty: bool,
    what: &str,
) -> JResult<()> {
    if non_empty && map.is_empty() {
        return Err(format!("{what} must not be an empty object"));
    }
    map.iter().try_for_each(|(k, v)| {
        check_chars(k, what)?;
        check_opaque(v, 2, what)
    })
}

/// Decodes an opaque object (rule A5); `non_empty` requires at least one member.
pub(crate) fn opaque_map(
    v: &Value,
    non_empty: bool,
    what: &str,
) -> JResult<BTreeMap<String, Value>> {
    let obj = v
        .as_object()
        .ok_or_else(|| format!("{what} must be an object"))?;
    let map: BTreeMap<String, Value> = obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    check_opaque_map(&map, non_empty, what)?;
    Ok(map)
}

/// Fails when `items` is empty.
pub(crate) fn check_non_empty<T>(items: &[T], what: &str) -> JResult<()> {
    if items.is_empty() {
        Err(format!("{what} must not be empty"))
    } else {
        Ok(())
    }
}

/// Fails when `items` lists one value twice (order is kept, so this is a pairwise scan over an
/// ordered slice; no hash container, R3).
pub(crate) fn check_distinct<T: PartialEq + std::fmt::Debug>(
    items: &[T],
    what: &str,
) -> JResult<()> {
    for (i, item) in items.iter().enumerate() {
        if items[..i].contains(item) {
            return Err(format!("{what} lists {item:?} twice"));
        }
    }
    Ok(())
}

/// Decodes a non-empty array whose elements are converted with `T::from_json`.
pub(crate) fn non_empty_vec<T: FromJson>(v: &Value, what: &str) -> JResult<Vec<T>> {
    let items = array_of(v, what)?;
    if items.is_empty() {
        return Err(format!("{what} must not be empty"));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(T::from_json(item)?);
    }
    Ok(out)
}

/// Decodes the required member `key` with `T::from_json`.
pub(crate) fn req<T: FromJson>(o: &mut Obj<'_>, key: &str) -> JResult<T> {
    T::from_json(o.req(key)?)
}

/// Decodes the optional member `key` with `T::from_json`.
pub(crate) fn opt<T: FromJson>(o: &mut Obj<'_>, key: &str) -> JResult<Option<T>> {
    o.opt(key)?.map(T::from_json).transpose()
}

/// Decodes the optional boolean member `key`.
pub(crate) fn opt_bool(o: &mut Obj<'_>, key: &'static str) -> JResult<Option<bool>> {
    o.opt(key)?.map(|b| bool_of(b, key)).transpose()
}

/// Writes `key: value` when `value` is present.
pub(crate) fn put_opt<M: SerializeMap, T: Serialize + ?Sized>(
    m: &mut M,
    key: &'static str,
    value: Option<&T>,
) -> Result<(), M::Error> {
    match value {
        Some(v) => m.serialize_entry(key, v),
        None => Ok(()),
    }
}
