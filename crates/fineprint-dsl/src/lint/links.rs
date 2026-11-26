// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `FPL-TOKEN-REF` (REQ-ING-009: referenced token definitions exist) and `FPL-LINK-ID`
//! (REQ-ING-009, CR 607: linked abilities share a link id), both on the raw JSON of every
//! ability, decodable or not.

use std::collections::{BTreeMap, BTreeSet};

use super::walk::CardFacts;
use super::{CLIP, Lint, LintContext, LintId, LintLocation, Severity, clip, push};
use crate::card::CardDefinitionDSL;

/// `FPL-TOKEN-REF`: with `ctx.known_token_ids` present, every `CREATE_TOKEN` `tokenDef` and
/// every `relatedTokens` entry must name a known token definition. Skipped, with no finding,
/// when the set is absent (docs/DECISIONS.md, "#16 FP-015 — Source characteristics and token
/// ids").
pub(super) fn check_tokens(
    def: &CardDefinitionDSL,
    facts: &CardFacts<'_>,
    ctx: &LintContext,
    out: &mut Vec<Lint>,
) {
    let Some(known) = &ctx.known_token_ids else {
        return;
    };
    let unknown = |id: &str| {
        format!(
            "token definition {:?} is not a known token definition",
            clip(id, CLIP)
        )
    };
    let scans = facts
        .abilities
        .iter()
        .map(|a| (a.location(), &a.scan))
        .chain(facts.partners.iter().map(|p| (p.location(), &p.scan)));
    for (location, scan) in scans {
        for id in &scan.token_defs {
            if !known.contains(*id) {
                push(
                    out,
                    LintId::TOKEN_REF,
                    Severity::Error,
                    location.clone(),
                    unknown(id),
                );
            }
        }
    }
    for (k, id) in def.related_tokens.iter().flatten().enumerate() {
        if !known.contains(id) {
            push(
                out,
                LintId::TOKEN_REF,
                Severity::Error,
                LintLocation::card(format!("relatedTokens[{k}]")),
                unknown(id),
            );
        }
    }
}

/// `FPL-LINK-ID` over the face abilities. CR 607.1b: abilities on every face count as printed
/// on the one card. A `LINKED` reference to an id no ability declares is an Error at the
/// referencing ability; an id declared by exactly one ability and referenced nowhere is an
/// unpaired link, a Warning at the declaring ability. CR 607.1c: an ability that declares and
/// references the same id is linked to itself. CR 607.4: an id may be declared by several
/// abilities and an ability may declare several ids.
pub(super) fn check_links(facts: &CardFacts<'_>, out: &mut Vec<Lint>) {
    let mut declared: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    let mut referenced: BTreeSet<&str> = BTreeSet::new();
    for (n, a) in facts.abilities.iter().enumerate() {
        for id in &a.link_ids {
            declared.entry(id).or_default().insert(n);
        }
        referenced.extend(a.scan.linked_refs.iter().copied());
    }
    for a in &facts.abilities {
        let mut refs: Vec<&str> = a.scan.linked_refs.clone();
        refs.sort_unstable();
        refs.dedup();
        for id in refs {
            if !declared.contains_key(id) {
                push(
                    out,
                    LintId::LINK_ID,
                    Severity::Error,
                    a.location(),
                    format!(
                        "LINKED reference to link id {:?} that no ability on the card declares \
                         (CR 607.1)",
                        clip(id, CLIP)
                    ),
                );
            }
        }
    }
    for (id, owners) in &declared {
        if owners.len() != 1 || referenced.contains(id) {
            continue;
        }
        if let Some(a) = owners.first().and_then(|&n| facts.abilities.get(n)) {
            push(
                out,
                LintId::LINK_ID,
                Severity::Warning,
                a.location(),
                format!(
                    "link id {:?} is declared by this ability only and no LINKED reference \
                     uses it (CR 607.1)",
                    clip(id, CLIP)
                ),
            );
        }
    }
}
