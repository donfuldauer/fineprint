// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Layout and face mapping (SRS §16.3 REQ-ING-003).
//!
//! Maps the raw Scryfall `layout` string of a [`NormalizedCardRecord`] (plus type lines, Oracle
//! text and `all_parts` where the table requires) to an engine [`LayoutBehavior`], and assigns
//! every face its [`FaceKind`]. The module is pure: no I/O, clock or environment, and every
//! result is a function of the input records (R3). `keywords[]` is a cross-check set and never
//! the sole source of behavior (SRS §16.3 REQ-ING-002).
//!
//! `UNSUPPORTED` is not a [`LayoutBehavior`]: it is the card's [`SupportStatus`], carried by
//! [`UnsupportedReason`] and [`LayoutError`]. This unit never produces `Verified` (R11).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_dsl::card::{FaceKind, LayoutBehavior, SupportStatus};

use crate::import::normalize::{NormalizedCardRecord, NormalizedFace};

#[cfg(test)]
mod tests;

/// Maximum digits accepted in the `N` of a station symbol line `N+ |`.
const MAX_STATION_DIGITS: usize = 6;

/// Whether a token-like record is a token or an emblem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TokenKind {
    /// A token record (SRS §4.2, CR 111).
    Token,
    /// An emblem record, modeled as `LayoutBehavior::Token` (decision D2).
    Emblem,
}

/// A successful layout mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutMapping {
    /// The engine layout.
    pub behavior: LayoutBehavior,
    /// One kind per `record.faces`, same order.
    pub face_kinds: Vec<FaceKind>,
    /// True for the variant-gated layouts: planar, scheme, vanguard (SRS §4.2, 901-905, 309).
    pub variant_gated: bool,
    /// Sorted, deduplicated meld partner names; empty unless `behavior` is `Meld`.
    pub meld_partners: Vec<String>,
    /// `Some` for the token, double-faced token and emblem rows; otherwise `None`.
    pub token_kind: Option<TokenKind>,
}

/// Why a record is not a gameplay object and is skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkipReason {
    /// Scryfall layout `art_series`.
    ArtSeries,
    /// Scryfall layout `reversible_card`.
    ReversibleCard,
    /// Scryfall layout `front_card`.
    FrontCard,
}

impl SkipReason {
    /// Fixed wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SkipReason::ArtSeries => "artSeries",
            SkipReason::ReversibleCard => "reversibleCard",
            SkipReason::FrontCard => "frontCard",
        }
    }
}

/// Why a known layout is UNSUPPORTED.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnsupportedReason {
    /// Un-set mechanic (`augment`, `host`): UNSUPPORTED unless extension `ext.unset` is enabled.
    /// No extension flag exists yet, so it is always unsupported.
    UnsetMechanic,
}

impl UnsupportedReason {
    /// Fixed wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            UnsupportedReason::UnsetMechanic => "unsetMechanic:ext.unset",
        }
    }

    /// Always [`SupportStatus::Unsupported`].
    #[must_use]
    pub const fn support_status(self) -> SupportStatus {
        SupportStatus::Unsupported
    }
}

/// The result of mapping one record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutOutcome {
    /// Mapped to a layout and face kinds.
    Mapped(LayoutMapping),
    /// Not a gameplay object; skipped.
    Skipped(SkipReason),
    /// A known layout whose card is UNSUPPORTED.
    Unsupported(UnsupportedReason),
}

/// An import error; the card is marked UNSUPPORTED.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayoutError {
    /// Unrecognized or empty `layout` string.
    UnknownLayout,
    /// The face count does not fit the layout.
    FaceCount,
    /// A `meld` record without a partner in `all_parts`.
    MeldPartnerMissing,
}

impl LayoutError {
    /// Fixed wire string; never quotes input (R12).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LayoutError::UnknownLayout => "unknownLayout",
            LayoutError::FaceCount => "faceCount",
            LayoutError::MeldPartnerMissing => "meldPartnerMissing",
        }
    }

    /// Always [`SupportStatus::Unsupported`].
    #[must_use]
    pub const fn support_status(self) -> SupportStatus {
        SupportStatus::Unsupported
    }
}

/// The batch result of [`map_records`].
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LayoutImport {
    /// Outcome per `oracle_id` for every record that did not error.
    pub outcomes: BTreeMap<String, LayoutOutcome>,
    /// Errors, sorted by `oracle_id` then error.
    pub errors: Vec<(String, LayoutError)>,
}

/// True when `line` is a station symbol line: 1..=6 ASCII digits, `+`, optional spaces, `|`,
/// anchored at the line start (SRS §4.2, CR 722.2).
fn is_station_symbol_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > MAX_STATION_DIGITS || bytes.get(digits) != Some(&b'+') {
        return false;
    }
    let rest = &bytes[digits + 1..];
    let spaces = rest.iter().take_while(|&&b| b == b' ').count();
    rest.get(spaces) == Some(&b'|')
}

/// True when `line` begins with the whole word `Station`.
fn starts_with_station_word(line: &str) -> bool {
    line.strip_prefix("Station")
        .is_some_and(|rest| !rest.chars().next().is_some_and(char::is_alphanumeric))
}

/// STATION detection (SRS §4.2, CR 722, 702.184): station symbol lines and the keyword.
fn is_station(record: &NormalizedCardRecord) -> bool {
    let mut symbol = false;
    let mut word = false;
    for text in record.faces.iter().filter_map(|f| f.oracle_text.as_deref()) {
        for line in text.lines() {
            symbol |= is_station_symbol_line(line);
            word |= starts_with_station_word(line);
        }
    }
    symbol && (word || record.keywords.iter().any(|k| k == "Station"))
}

/// Whole-word `word` among the card types or subtypes of `face`.
fn face_has_type(face: &NormalizedFace, word: &str) -> bool {
    face.card_types
        .iter()
        .chain(face.subtypes.iter())
        .any(|t| t == word)
}

/// Meld partner names (SRS §4.2, CR 713, 701.42): sorted, deduplicated, excluding own name.
fn meld_partners(record: &NormalizedCardRecord) -> Vec<String> {
    let set: BTreeSet<&String> = record
        .all_parts
        .iter()
        .filter(|p| matches!(p.component.as_deref(), Some("meld_part" | "meld_result")))
        .filter_map(|p| p.name.as_ref())
        .filter(|n| **n != record.name)
        .collect();
    set.into_iter().cloned().collect()
}

/// Builds a mapping for `behavior` with the given face kinds, checking the face count.
fn mapped(
    record: &NormalizedCardRecord,
    behavior: LayoutBehavior,
    kinds: &[FaceKind],
) -> Result<LayoutMapping, LayoutError> {
    if record.faces.len() != kinds.len() {
        return Err(LayoutError::FaceCount);
    }
    Ok(LayoutMapping {
        behavior,
        face_kinds: kinds.to_vec(),
        variant_gated: false,
        meld_partners: Vec::new(),
        token_kind: None,
    })
}

/// Maps one record's layout and faces (SRS §16.3 REQ-ING-003).
///
/// # Errors
/// [`LayoutError::UnknownLayout`] for an unrecognized or empty layout,
/// [`LayoutError::FaceCount`] when the face count does not fit a mapped layout, and
/// [`LayoutError::MeldPartnerMissing`] for a meld record without a partner.
pub fn map_layout(record: &NormalizedCardRecord) -> Result<LayoutOutcome, LayoutError> {
    use FaceKind::{Adventure, Back, Flipped, LeftHalf, Main, Omen, PrepareSpell, RightHalf};
    use LayoutBehavior as L;
    let one = [Main];
    let m = |b: L, k: &[FaceKind]| mapped(record, b, k).map(LayoutOutcome::Mapped);
    match record.layout.as_str() {
        // SRS §4.2, CR 722 (STATION); otherwise NORMAL.
        "normal" if is_station(record) => m(L::Station, &one),
        "normal" => m(L::Normal, &one),
        // SRS §4.2: Rooms arrive as split (ROOM); every other split is SPLIT.
        "split" => {
            let room = record.faces.iter().any(|f| face_has_type(f, "Room"));
            m(
                if room { L::Room } else { L::Split },
                &[LeftHalf, RightHalf],
            )
        }
        // SRS §4.2, CR 710.
        "flip" => m(L::Flip, &[Main, Flipped]),
        // SRS §4.2, CR 712.
        "transform" => m(L::DfcNonmodal, &[Main, Back]),
        "modal_dfc" => m(L::DfcModal, &[Main, Back]),
        // SRS §4.2, CR 713, 701.42 (decision D3: one face; result is another record).
        "meld" => {
            let partners = meld_partners(record);
            if record.faces.len() != 1 {
                return Err(LayoutError::FaceCount);
            }
            if partners.is_empty() {
                return Err(LayoutError::MeldPartnerMissing);
            }
            let mut mapping = mapped(record, L::Meld, &one)?;
            mapping.meld_partners = partners;
            Ok(LayoutOutcome::Mapped(mapping))
        }
        // SRS §4.2, CR 716 (ADVENTURER), CR 721 (OMEN).
        "adventure" => {
            let omen = record
                .faces
                .get(1)
                .is_some_and(|f| face_has_type(f, "Omen"));
            if omen {
                m(L::Omen, &[Main, Omen])
            } else {
                m(L::Adventurer, &[Main, Adventure])
            }
        }
        // SRS §4.2, CR 723.
        "prepare" => m(L::Preparation, &[Main, PrepareSpell]),
        // SRS §4.2, CR 719, 702.160.
        "prototype" => m(L::Prototype, &one),
        // SRS §4.2, CR 702.140, 731.
        "mutate" => m(L::Mutate, &one),
        // SRS §4.2, CR 711.
        "leveler" => m(L::Leveler, &one),
        // SRS §4.2, CR 717.
        "class" => m(L::Class, &one),
        // SRS §4.2, CR 720.
        "case" => m(L::Case, &one),
        // SRS §4.2, CR 715.
        "saga" => m(L::Saga, &one),
        // Decision D1: battle is double-faced, mapped to DFC_NONMODAL (SRS §4.2, CR 712).
        "battle" => match record.faces.len() {
            1 => m(L::DfcNonmodal, &one),
            _ => m(L::DfcNonmodal, &[Main, Back]),
        },
        // Variant-gated (SRS §4.2 layout table).
        "planar" => gated(record, L::Planar),
        "scheme" => gated(record, L::Scheme),
        "vanguard" => gated(record, L::Vanguard),
        // SRS §4.2, CR 111; emblem is a Token with token_kind Emblem (decision D2).
        "token" => token(record, L::Token, &one, TokenKind::Token),
        "double_faced_token" => token(record, L::DfcToken, &[Main, Back], TokenKind::Token),
        "emblem" => token(record, L::Token, &one, TokenKind::Emblem),
        "augment" | "host" => Ok(LayoutOutcome::Unsupported(UnsupportedReason::UnsetMechanic)),
        "art_series" => Ok(LayoutOutcome::Skipped(SkipReason::ArtSeries)),
        "reversible_card" => Ok(LayoutOutcome::Skipped(SkipReason::ReversibleCard)),
        "front_card" => Ok(LayoutOutcome::Skipped(SkipReason::FrontCard)),
        _ => Err(LayoutError::UnknownLayout),
    }
}

/// A variant-gated single-face mapping.
fn gated(
    record: &NormalizedCardRecord,
    behavior: LayoutBehavior,
) -> Result<LayoutOutcome, LayoutError> {
    let mut mapping = mapped(record, behavior, &[FaceKind::Main])?;
    mapping.variant_gated = true;
    Ok(LayoutOutcome::Mapped(mapping))
}

/// A token or emblem mapping.
fn token(
    record: &NormalizedCardRecord,
    behavior: LayoutBehavior,
    kinds: &[FaceKind],
    kind: TokenKind,
) -> Result<LayoutOutcome, LayoutError> {
    let mut mapping = mapped(record, behavior, kinds)?;
    mapping.token_kind = Some(kind);
    Ok(LayoutOutcome::Mapped(mapping))
}

/// Maps a batch. Every record is mapped independently; errors are collected against the
/// record's `oracle_id`. Output is deterministic: a `BTreeMap` and errors sorted by id (R3).
/// Records sharing an `oracle_id` are not expected; the later one wins.
#[must_use]
pub fn map_records(records: &[NormalizedCardRecord]) -> LayoutImport {
    let mut out = LayoutImport::default();
    for record in records {
        match map_layout(record) {
            Ok(o) => {
                out.outcomes.insert(record.card_def_id.clone(), o);
            }
            Err(e) => out.errors.push((record.card_def_id.clone(), e)),
        }
    }
    out.errors.sort();
    out
}
