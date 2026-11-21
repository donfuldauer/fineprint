// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Ban lists as versioned data references and the card catalog trait (REQ-FMT-001).

use std::collections::BTreeMap;
use std::fmt;

use fineprint_dsl::card::{Legality, SupportStatus};
use serde::{Deserialize, Serialize};

use super::config::FormatConfigError;

/// A reference `banlists/<name>@<version>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BanListRef {
    /// Ban list name.
    pub name: String,
    /// Ban list version.
    pub version: String,
}

impl BanListRef {
    /// Parses `banlists/<name>@<version>`; name and version must be non-empty and contain
    /// neither `/` nor `@`.
    pub fn parse(s: &str) -> Result<BanListRef, FormatConfigError> {
        let bad = || FormatConfigError::BadBanlistRef(s.to_string());
        let rest = s.strip_prefix("banlists/").ok_or_else(bad)?;
        let (name, version) = rest.split_once('@').ok_or_else(bad)?;
        let ok = |p: &str| !p.is_empty() && !p.contains('/') && !p.contains('@');
        if ok(name) && ok(version) {
            Ok(BanListRef {
                name: name.to_string(),
                version: version.to_string(),
            })
        } else {
            Err(bad())
        }
    }
}

impl fmt::Display for BanListRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "banlists/{}@{}", self.name, self.version)
    }
}

/// A versioned ban list: data, never code. An id absent from `entries` is LEGAL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BanList {
    /// Ban list name.
    pub name: String,
    /// Ban list version.
    pub version: String,
    /// Legality per card definition id.
    pub entries: BTreeMap<String, Legality>,
}

/// Supplies ban lists by reference.
pub trait BanListSource {
    /// The ban list for `r`, if known.
    fn ban_list(&self, r: &BanListRef) -> Option<&BanList>;
}

/// The facts about a card that deck validation reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardFacts {
    /// The card's support status (read only; never assigned here, R11).
    pub support_status: SupportStatus,
    /// Whether the card is digital-only.
    pub digital_only: bool,
    /// Whether the card is a basic land.
    pub is_basic_land: bool,
}

/// Stands in for the registered card database.
pub trait CardCatalog {
    /// Facts for a card definition id; `None` when the id is not in the database.
    fn card_facts(&self, card_def_id: &str) -> Option<CardFacts>;
}
