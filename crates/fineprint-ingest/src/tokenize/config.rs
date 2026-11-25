// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The defensive JSON loader of [`TokenizerConfig`] (REQ-ING-004(a)/(b), R12).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::normalize::normalize;
use super::{CardTextConfig, ConfigError, TokenizerConfig};

/// Largest accepted configuration document in bytes.
pub const MAX_CONFIG_BYTES: usize = 4 * 1024 * 1024;

/// Largest accepted configured string in bytes (after normalization).
pub const MAX_CONFIG_STRING_BYTES: usize = 512;

/// Largest accepted list length in one card entry.
pub const MAX_CONFIG_LIST_LEN: usize = 256;

/// The only accepted `formatVersion`.
pub const CONFIG_FORMAT_VERSION: u64 = 1;

/// Shorthand for a shape error.
fn shape(msg: impl Into<String>) -> ConfigError {
    ConfigError::Shape(msg.into())
}

/// Reads the members of `obj`, refusing members outside `allowed`.
fn check_members(obj: &Map<String, Value>, allowed: &[&str], at: &str) -> Result<(), ConfigError> {
    match obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(shape(format!("{at}: unknown member {k:?}"))),
        None => Ok(()),
    }
}

/// Reads an optional list of non-empty strings, normalized like Oracle text.
fn string_set(v: Option<&Value>, at: &str) -> Result<BTreeSet<String>, ConfigError> {
    let Some(v) = v else {
        return Ok(BTreeSet::new());
    };
    let arr = v
        .as_array()
        .ok_or_else(|| shape(format!("{at}: expected an array of strings")))?;
    if arr.len() > MAX_CONFIG_LIST_LEN {
        return Err(shape(format!(
            "{at}: more than {MAX_CONFIG_LIST_LEN} entries"
        )));
    }
    let mut out = BTreeSet::new();
    for (i, item) in arr.iter().enumerate() {
        let s = item
            .as_str()
            .ok_or_else(|| shape(format!("{at}[{i}]: expected a string")))?;
        let s = normalize(s).trim().to_string();
        if s.is_empty() || s.len() > MAX_CONFIG_STRING_BYTES || s.contains('\n') {
            return Err(shape(format!(
                "{at}[{i}]: empty, multi-line or longer than {MAX_CONFIG_STRING_BYTES} bytes"
            )));
        }
        out.insert(s);
    }
    Ok(out)
}

impl TokenizerConfig {
    /// Parses a client configuration document (REQ-ING-004(a)/(b), R12):
    ///
    /// ```json
    /// {"formatVersion": 1,
    ///  "cards": {"<card key>": {"keepParentheses": ["(...)"],
    ///                           "shortNames": ["..."], "otherNames": ["..."]}}}
    /// ```
    ///
    /// The card key is the record's `oracle_id` (`card_def_id`). Every card member is optional.
    /// Strings are normalized like Oracle text and trimmed; a `keepParentheses` entry must start
    /// with `(` and end with `)`. Unknown members, wrong types, empty strings and oversized input
    /// are refused.
    ///
    /// # Errors
    /// [`ConfigError::TooLarge`] above [`MAX_CONFIG_BYTES`], [`ConfigError::Json`] for malformed
    /// JSON (including nesting beyond `serde_json`'s recursion limit), [`ConfigError::Shape`] for
    /// a well-formed document of the wrong shape.
    pub fn from_json(bytes: &[u8]) -> Result<TokenizerConfig, ConfigError> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::TooLarge { len: bytes.len() });
        }
        let v: Value =
            serde_json::from_slice(bytes).map_err(|e| ConfigError::Json(e.to_string()))?;
        let root = v
            .as_object()
            .ok_or_else(|| shape("root: expected an object"))?;
        check_members(root, &["formatVersion", "cards"], "root")?;
        match root.get("formatVersion").and_then(Value::as_u64) {
            Some(CONFIG_FORMAT_VERSION) => {}
            _ => return Err(shape("formatVersion: expected 1")),
        }
        let cards_v = root
            .get("cards")
            .ok_or_else(|| shape("cards: missing"))?
            .as_object()
            .ok_or_else(|| shape("cards: expected an object"))?;
        let mut cards = BTreeMap::new();
        for (key, entry) in cards_v {
            if key.is_empty() || key.len() > MAX_CONFIG_STRING_BYTES {
                return Err(shape("cards: empty or oversized card key"));
            }
            let at = format!("cards[{key:?}]");
            let obj = entry
                .as_object()
                .ok_or_else(|| shape(format!("{at}: expected an object")))?;
            check_members(obj, &["keepParentheses", "shortNames", "otherNames"], &at)?;
            let keep_parentheses =
                string_set(obj.get("keepParentheses"), &format!("{at}.keepParentheses"))?;
            if keep_parentheses
                .iter()
                .any(|s| !s.starts_with('(') || !s.ends_with(')'))
            {
                return Err(shape(format!(
                    "{at}.keepParentheses: each entry must start with '(' and end with ')'"
                )));
            }
            let short_names = string_set(obj.get("shortNames"), &format!("{at}.shortNames"))?;
            let other_names = string_set(obj.get("otherNames"), &format!("{at}.otherNames"))?;
            cards.insert(
                key.clone(),
                CardTextConfig {
                    keep_parentheses,
                    short_names,
                    other_names,
                },
            );
        }
        Ok(TokenizerConfig { cards })
    }
}
