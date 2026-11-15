// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! A tiny synthetic pest (PEG) grammar compiled with `pest_derive` (REQ-NFR-005).

use pest::Parser;
use pest_derive::Parser;

/// Parser generated from `src/sentence.pest`.
#[derive(Parser)]
#[grammar = "sentence.pest"]
pub struct SentenceParser;

/// Parses one sentence and returns the number of clauses, or `None` if it is rejected.
#[must_use]
pub fn count_clauses(input: &str) -> Option<usize> {
    let mut pairs = SentenceParser::parse(Rule::sentence, input).ok()?;
    let sentence = pairs.next()?;
    Some(
        sentence
            .into_inner()
            .filter(|pair| pair.as_rule() == Rule::clause)
            .count(),
    )
}
