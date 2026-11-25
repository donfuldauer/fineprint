// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `fineprint-ingest fetch` and `import`: the Scryfall bulk import (REQ-ING-001, REQ-ING-002,
//! REQ-NFR-004; #87 FP-085).
//!
//! - [`fetch`] downloads the Scryfall Oracle Cards and Rulings bulk files (and, with
//!   `--mtgjson`, MTGJSON `AtomicCards`) into the git-ignored cache directory (default
//!   [`DEFAULT_CACHE`]), at most once per 24 hours per file, with exactly one bulk-data metadata
//!   request per run and never a per-card API request.
//! - [`normalize`] reads the cached bulk files defensively (R12) and produces one
//!   [`normalize::NormalizedCardRecord`] per `oracle_id`, recomputing mana value, colors and
//!   color identity from the parsed mana cost with integer arithmetic ([`recompute`]); the float
//!   `cmc` is used only to flag a mismatch.
//! - [`mtgjson`] cross-validates the records against MTGJSON `AtomicCards` and reports every
//!   disagreeing characteristic.
//! - [`cli`] is the command line of both subcommands.
//!
//! Cache layout (all under the cache directory, which is git-ignored and never committed):
//! `scryfall/oracle-cards.json`, `scryfall/rulings.json`, `scryfall/source.json`,
//! `mtgjson/AtomicCards.json`, `mtgjson/source.json`, `normalized/cards.json`, and a `.lock`
//! file per fetched group (`fetch` holds it exclusively while it updates the group; it is left
//! in place). The file names are fixed constants, never taken from a server response.

pub mod cli;
pub mod fetch;
pub mod json_array;
pub mod mtgjson;
pub mod normalize;
pub mod recompute;

#[cfg(test)]
mod tests;

use std::io::Read;
use std::path::Path;

/// The default cache directory, relative to the current directory (run from the repository
/// root, where `.gitignore` lists `ingest-cache/`).
pub const DEFAULT_CACHE: &str = "ingest-cache";
/// The Scryfall group directory inside the cache.
pub const SCRYFALL_DIR: &str = "scryfall";
/// The MTGJSON group directory inside the cache.
pub const MTGJSON_DIR: &str = "mtgjson";
/// The normalized-output directory inside the cache.
pub const NORMALIZED_DIR: &str = "normalized";
/// The cached Oracle Cards bulk file (inside [`SCRYFALL_DIR`]).
pub const ORACLE_CARDS_FILE: &str = "oracle-cards.json";
/// The cached Rulings bulk file (inside [`SCRYFALL_DIR`]).
pub const RULINGS_FILE: &str = "rulings.json";
/// The cached MTGJSON `AtomicCards` file (inside [`MTGJSON_DIR`]).
pub const ATOMIC_CARDS_FILE: &str = "AtomicCards.json";
/// The per-group cache metadata file.
pub const SOURCE_FILE: &str = "source.json";
/// The default normalized output file (inside [`NORMALIZED_DIR`]).
pub const NORMALIZED_FILE: &str = "cards.json";

/// The largest input file the toolchain reads or downloads: 4 GiB. A larger file is an input
/// error; it is never read into memory (R12).
pub const MAX_INPUT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Reads `path` into memory, refusing a file larger than `cap` bytes before reading it and
/// reading at most `cap + 1` bytes even if the file grows meanwhile.
///
/// # Errors
/// The I/O error, or an `InvalidData` error when the file exceeds `cap`.
pub fn read_capped(path: &Path, cap: u64) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let too_big = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("the file is larger than the {cap}-byte cap"),
        )
    };
    if len > cap {
        return Err(too_big());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    file.take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > cap {
        return Err(too_big());
    }
    Ok(bytes)
}

/// Whether `s` has the lowercase UUID shape `8-4-4-4-12` (36 characters: lowercase hex digits
/// and four hyphens), the shape required of every `oracle_id` (REQ-ING-001).
#[must_use]
pub fn is_uuid_shape(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
}
