// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The committed citation index `docs/cr-citations.json` (REQ-DOC-001, REQ-NFR-007).
//!
//! Schema version 1:
//!
//! ```text
//! { "schemaVersion": 1,
//!   "baseline": { "id": "CR-<date>", "effectiveDate": "<date>", "sourceUrl": "<url>" },
//!   "hashAlgorithm": "sha256-normalized-v1",
//!   "citations": { "<rule>": { "sha256": "<64 hex>", "locations": ["path:line", ...] }, ... },
//!   "excluded": [ { "rule": "<rule>", "location": "path:line", "reason": "..." }, ... ] }
//! ```
//!
//! The file holds rule numbers, hashes and repository locations only, never rule prose.
//! Rendering is deterministic: fixed key order, rules sorted by [`RuleNum`] order, locations
//! sorted lexicographically, 2-space indent, LF line endings, one trailing newline.

use std::collections::BTreeMap;

use super::json::{self, Json};
use super::rules::{CrRules, RuleNum};
use super::scan::{ExcludedCite, ScanResult};

/// The only schema version this tool reads and writes.
pub const SCHEMA_VERSION: &str = "1";
/// Name of the hash scheme (see `rules.rs` "Normalization").
pub const HASH_ALGORITHM: &str = "sha256-normalized-v1";

/// The verified CR baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    /// `CR-` plus the effective date.
    pub id: String,
    /// ISO date (`YYYY-MM-DD`).
    pub effective_date: String,
    /// The URL the text was fetched from.
    pub source_url: String,
}

impl Baseline {
    /// A baseline for an effective date and source URL; the id is derived.
    pub fn new(effective_date: &str, source_url: &str) -> Baseline {
        Baseline {
            id: format!("CR-{effective_date}"),
            effective_date: effective_date.to_string(),
            source_url: source_url.to_string(),
        }
    }
}

/// One indexed rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Lowercase hex SHA-256 of the normalized rule text.
    pub sha256: String,
    /// Citing locations, sorted.
    pub locations: Vec<String>,
}

/// The whole index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    /// The verified baseline.
    pub baseline: Baseline,
    /// Rule -> entry.
    pub citations: BTreeMap<RuleNum, Entry>,
    /// Excluded citations.
    pub excluded: Vec<ExcludedCite>,
}

/// A scanned citation whose rule is absent from the CR text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    /// The rule number.
    pub rule: RuleNum,
    /// `path:line`.
    pub location: String,
}

/// Every (rule, location) of `scan` whose rule is not in `cr`, in rule then location order.
pub fn missing(scan: &ScanResult, cr: &CrRules) -> Vec<Missing> {
    let mut out = Vec::new();
    for (rule, locs) in &scan.cited {
        if !cr.hashes.contains_key(rule) {
            for l in locs {
                out.push(Missing {
                    rule: rule.clone(),
                    location: l.clone(),
                });
            }
        }
    }
    out
}

/// Build the index; fails with the missing citations if any cited rule is absent from `cr`.
pub fn build(scan: &ScanResult, cr: &CrRules, baseline: Baseline) -> Result<Index, Vec<Missing>> {
    let miss = missing(scan, cr);
    if !miss.is_empty() {
        return Err(miss);
    }
    let citations = scan
        .cited
        .iter()
        .filter_map(|(rule, locs)| {
            cr.hashes.get(rule).map(|h| {
                (
                    rule.clone(),
                    Entry {
                        sha256: h.clone(),
                        locations: locs.iter().cloned().collect(),
                    },
                )
            })
        })
        .collect();
    Ok(Index {
        baseline,
        citations,
        excluded: scan.excluded.clone(),
    })
}

fn string_array(out: &mut String, indent: &str, items: &[String]) {
    if items.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push_str("[\n");
    for (i, s) in items.iter().enumerate() {
        out.push_str(indent);
        out.push_str("  ");
        out.push_str(&json::quote(s));
        if i + 1 < items.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(indent);
    out.push(']');
}

/// Render the index as deterministic JSON text.
pub fn render(index: &Index) -> String {
    let q = json::quote;
    let b = &index.baseline;
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"schemaVersion\": {SCHEMA_VERSION},\n"));
    out.push_str("  \"baseline\": {\n");
    out.push_str(&format!("    \"id\": {},\n", q(&b.id)));
    out.push_str(&format!(
        "    \"effectiveDate\": {},\n",
        q(&b.effective_date)
    ));
    out.push_str(&format!("    \"sourceUrl\": {}\n", q(&b.source_url)));
    out.push_str("  },\n");
    out.push_str(&format!("  \"hashAlgorithm\": {},\n", q(HASH_ALGORITHM)));
    if index.citations.is_empty() {
        out.push_str("  \"citations\": {},\n");
    } else {
        out.push_str("  \"citations\": {\n");
        let n = index.citations.len();
        for (i, (rule, e)) in index.citations.iter().enumerate() {
            out.push_str(&format!("    {}: {{\n", q(rule.as_str())));
            out.push_str(&format!("      \"sha256\": {},\n", q(&e.sha256)));
            out.push_str("      \"locations\": ");
            string_array(&mut out, "      ", &e.locations);
            out.push_str("\n    }");
            out.push_str(if i + 1 < n { ",\n" } else { "\n" });
        }
        out.push_str("  },\n");
    }
    if index.excluded.is_empty() {
        out.push_str("  \"excluded\": []\n");
    } else {
        out.push_str("  \"excluded\": [\n");
        let n = index.excluded.len();
        for (i, e) in index.excluded.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"rule\": {},\n", q(e.rule.as_str())));
            out.push_str(&format!("      \"location\": {},\n", q(&e.location)));
            out.push_str(&format!("      \"reason\": {}\n", q(&e.reason)));
            out.push_str(if i + 1 < n { "    },\n" } else { "    }\n" });
        }
        out.push_str("  ]\n");
    }
    out.push_str("}\n");
    out
}

fn str_field<'a>(obj: &'a Json, key: &str, ctx: &str) -> Result<&'a str, String> {
    obj.get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| format!("{ctx}: missing or non-string `{key}`"))
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Read an index file. Errors describe what is missing or malformed.
pub fn read(text: &str) -> Result<Index, String> {
    let doc = json::parse(text).map_err(|e| format!("not valid JSON: {e}"))?;
    if !matches!(doc, Json::Obj(_)) {
        return Err("the top level is not an object".to_string());
    }
    match doc.get("schemaVersion").and_then(Json::as_num) {
        Some(SCHEMA_VERSION) => {}
        Some(other) => {
            return Err(format!(
                "unknown schemaVersion {other} (expected {SCHEMA_VERSION})"
            ));
        }
        None => return Err("missing or non-numeric `schemaVersion`".to_string()),
    }
    let alg = str_field(&doc, "hashAlgorithm", "index")?;
    if alg != HASH_ALGORITHM {
        return Err(format!(
            "unknown hashAlgorithm `{alg}` (expected `{HASH_ALGORITHM}`)"
        ));
    }
    let b = doc.get("baseline").ok_or("missing `baseline`")?;
    let baseline = Baseline {
        id: str_field(b, "id", "baseline")?.to_string(),
        effective_date: str_field(b, "effectiveDate", "baseline")?.to_string(),
        source_url: str_field(b, "sourceUrl", "baseline")?.to_string(),
    };
    let Some(Json::Obj(items)) = doc.get("citations") else {
        return Err("missing or non-object `citations`".to_string());
    };
    let mut citations = BTreeMap::new();
    for (key, v) in items {
        let rule = RuleNum::parse(key).ok_or_else(|| format!("`{key}` is not a rule number"))?;
        let ctx = format!("citation {key}");
        let sha = str_field(v, "sha256", &ctx)?;
        if !is_hex64(sha) {
            return Err(format!("{ctx}: `sha256` is not 64 lowercase hex digits"));
        }
        let Some(Json::Arr(locs)) = v.get("locations") else {
            return Err(format!("{ctx}: missing or non-array `locations`"));
        };
        let locations = locs
            .iter()
            .map(|l| l.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("{ctx}: non-string location"))?;
        if citations
            .insert(
                rule,
                Entry {
                    sha256: sha.to_string(),
                    locations,
                },
            )
            .is_some()
        {
            return Err(format!("duplicate citation key `{key}`"));
        }
    }
    let Some(Json::Arr(ex)) = doc.get("excluded") else {
        return Err("missing or non-array `excluded`".to_string());
    };
    let mut excluded = Vec::new();
    for e in ex {
        let r = str_field(e, "rule", "excluded entry")?;
        excluded.push(ExcludedCite {
            rule: RuleNum::parse(r)
                .ok_or_else(|| format!("excluded `{r}` is not a rule number"))?,
            location: str_field(e, "location", "excluded entry")?.to_string(),
            reason: str_field(e, "reason", "excluded entry")?.to_string(),
        });
    }
    Ok(Index {
        baseline,
        citations,
        excluded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cr::rules::{parse_cr, tests::synthetic_cr};
    use crate::cr::scan::{SRS_PATH, ScanFile, scan_files};

    fn sample_index() -> Index {
        let cr = parse_cr(&synthetic_cr(&[])).unwrap();
        let srs = format!(
            "CR {a}.2 and {b}\n| {a}.1a |\n| 31 | Mandatory loops cited {c}; x |\nCR {a}.2\n",
            a = 100,
            b = 101,
            c = "729.4"
        );
        let scan = scan_files(&[
            ScanFile {
                path: SRS_PATH.to_string(),
                bytes: srs.into_bytes(),
            },
            ScanFile {
                path: "crates/z/src/a.rs".to_string(),
                bytes: format!("// CR {}.1 \"q\\\n", 100).into_bytes(),
            },
        ]);
        build(
            &scan,
            &cr,
            Baseline::new("2026-11-06", "https://example.invalid/a b\"c"),
        )
        .unwrap()
    }

    /// REQ-NFR-007, REQ-DOC-001: rendering is deterministic, sorted, prose-free and round-trips.
    #[test]
    fn req_nfr_007_index_render_round_trip() {
        let idx = sample_index();
        let text = render(&idx);
        assert_eq!(text, render(&sample_index()));
        assert!(text.ends_with("}\n") && !text.ends_with("\n\n") && !text.contains('\r'));
        assert_eq!(read(&text).unwrap(), idx);
        let keys: Vec<&str> = idx.citations.keys().map(RuleNum::as_str).collect();
        assert_eq!(keys, ["100.1", "100.1a", "100.2", "101"]);
        assert_eq!(
            idx.citations[&RuleNum::parse("100.2").unwrap()].locations,
            ["docs/SRS.md:1", "docs/SRS.md:4"]
        );
        assert_eq!(idx.excluded.len(), 1);
        assert!(text.contains("\"excluded\": [\n    {\n      \"rule\": \"729.4\""));
        // no synthetic prose leaks into the index
        for word in ["zephyr", "quokka", "Synthetic", "Alpha", "Beta"] {
            assert!(!text.contains(word), "{word}");
        }
        assert!(text.contains("\"id\": \"CR-2026-11-06\""));
    }

    /// REQ-NFR-007: empty collections render and read back.
    #[test]
    fn req_nfr_007_index_empty() {
        let idx = Index {
            baseline: Baseline::new("2026-11-06", "u"),
            citations: BTreeMap::new(),
            excluded: vec![],
        };
        let text = render(&idx);
        assert!(text.contains("\"citations\": {},") && text.contains("\"excluded\": []"));
        assert_eq!(read(&text).unwrap(), idx);
        let mut one = idx.clone();
        one.citations.insert(
            RuleNum::parse("100").unwrap(),
            Entry {
                sha256: "0".repeat(64),
                locations: vec![],
            },
        );
        assert_eq!(read(&render(&one)).unwrap(), one);
    }

    /// REQ-NFR-007: malformed index files are rejected with a reason.
    #[test]
    fn req_nfr_007_index_read_errors() {
        let good = render(&sample_index());
        let cases = [
            "not json".to_string(),
            "[]".to_string(),
            good.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2"),
            good.replace("\"schemaVersion\": 1", "\"schemaVersion\": \"1\""),
            good.replace(HASH_ALGORITHM, "md5"),
            good.replace("\"id\"", "\"ident\""),
            good.replace("\"citations\": {", "\"citations\": [{")
                .replace("  },\n  \"excluded\"", "  }],\n  \"excluded\""),
            good.replace("\"100.1a\"", "\"nope\""),
            good.replace("\"sha256\": \"", "\"sha256\": \"X"),
            good.replace("\"locations\": [", "\"locations\": [1, "),
            good.replace("\"locations\"", "\"places\""),
            good.replace("\"excluded\": [", "\"excluded\": {")
                .replace("  ]\n}", "  }\n}"),
            good.replace("\"rule\": \"729.4\"", "\"rule\": \"x\""),
            good.replace("\"reason\"", "\"why\""),
            good.replace("\"100.1a\"", "\"100.1\""),
        ];
        for (i, c) in cases.iter().enumerate() {
            assert!(read(c).is_err(), "case {i}");
        }
    }

    /// REQ-NFR-007: a citation missing from the CR blocks the build.
    #[test]
    fn req_nfr_007_build_missing() {
        let cr = parse_cr(&synthetic_cr(&[])).unwrap();
        let srs = format!("CR {}.4\n", 100);
        let scan = scan_files(&[ScanFile {
            path: SRS_PATH.to_string(),
            bytes: srs.into_bytes(),
        }]);
        let err = build(&scan, &cr, Baseline::new("d", "u")).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].location, "docs/SRS.md:1");
        assert_eq!(err[0].rule.as_str(), "100.4");
    }
}
