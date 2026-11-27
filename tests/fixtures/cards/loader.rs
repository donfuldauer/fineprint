// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Loader for the shared synthetic card library (REQ-TEST-001, REQ-NFR-004).
//!
//! Include it from any crate's tests with
//! `#[path = "../../../tests/fixtures/cards/loader.rs"] mod card_library;`
//! (adjust the number of `../` to the including file's depth), then call
//! `card_library::load("test.flier_2_2")`. Uses only `std` and `fineprint_dsl`; reads files
//! under the library root only (R3: no clock, no environment besides `CARGO_MANIFEST_DIR`).
//! Every card is loaded through the strict `CardDefinitionDSL::from_json`.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use fineprint_dsl::card::CardDefinitionDSL;

/// One library card with the file it came from.
#[derive(Clone, Debug)]
pub struct LibraryCard {
    /// Path of the JSON file.
    pub path: PathBuf,
    /// Name of the parent directory (the family, for example `creatures`).
    pub family: String,
    /// The strictly loaded definition.
    pub def: CardDefinitionDSL,
}

/// The `tests/fixtures/cards` directory: found by walking up from the including crate's
/// `CARGO_MANIFEST_DIR`. Panics when no ancestor contains it.
pub fn library_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut dir: Option<&Path> = Some(start.as_path());
    while let Some(d) = dir {
        let candidate = d.join("tests").join("fixtures").join("cards");
        if candidate.is_dir() {
            return candidate;
        }
        dir = d.parent();
    }
    panic!(
        "card library: no tests/fixtures/cards directory in any ancestor of {}",
        start.display()
    );
}

/// Every `*.json` file under the library root, recursively, sorted by path.
pub fn library_paths() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![library_root()];
    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("card library: cannot read {}: {e}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|e| panic!("card library: bad entry in {}: {e}", dir.display()))
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|x| x == "json") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Strictly loads one library file; panics with the path on failure.
pub fn load_path(path: &Path) -> CardDefinitionDSL {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|e| panic!("card library: cannot read {}: {e}", path.display()));
    CardDefinitionDSL::from_json(&bytes)
        .unwrap_or_else(|e| panic!("card library: {} failed to load: {e}", path.display()))
}

/// Loads every library card, in path order.
pub fn load_all() -> Vec<LibraryCard> {
    library_paths()
        .into_iter()
        .map(|path| {
            let family = path
                .parent()
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let def = load_path(&path);
            LibraryCard { path, family, def }
        })
        .collect()
}

/// Loads the card with id `card_def_id` (for example `test.flier_2_2`); panics when the id is
/// not `test.`-prefixed, or no library file of that name exists.
pub fn load(card_def_id: &str) -> CardDefinitionDSL {
    let stem = card_def_id
        .strip_prefix("test.")
        .unwrap_or_else(|| panic!("card library: {card_def_id:?} does not start with \"test.\""));
    let file = format!("{stem}.json");
    let path = library_paths()
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| *n == *file))
        .unwrap_or_else(|| panic!("card library: no file {file} for {card_def_id}"));
    let def = load_path(&path);
    assert_eq!(
        def.card_def_id,
        card_def_id,
        "card library: {} holds a different cardDefId",
        path.display()
    );
    def
}
