// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Facts read once from a definition for every check (REQ-ING-009): the typed view of every
//! ability and one iterative walk over its raw JSON, plus the raw checks `FPL-LOOK-BACK`
//! (CR 603.10) and `FPL-MAX-ITER` (REQ-DSL-010, SRS §14.4.5).
//!
//! The walk uses an explicit stack, so a value nested far beyond
//! [`crate::canonical::MAX_DEPTH`] is walked without recursion (R12). Paths in messages are
//! rendered from the last few segments only, so rendering is bounded whatever the depth.

use serde_json::Value;

use super::{CLIP, Lint, LintId, LintLocation, Severity, clip, push};
use crate::ability;
use crate::card::CardDefinitionDSL;

/// How many trailing path segments a message shows.
const PATH_SEGMENTS: usize = 8;

/// Longest key shown in a rendered path segment (in `char`s).
const SEGMENT_CLIP: usize = 32;

/// The raw `textSpan` member of an ability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RawSpan {
    /// No `textSpan` member (or the ability is not an object).
    Absent,
    /// Two non-negative integers with `start <= end`.
    Valid(u64, u64),
    /// Anything else.
    Malformed,
}

impl RawSpan {
    /// Reads the `textSpan` member of `ability`.
    fn read(ability: &Value) -> RawSpan {
        let Some(span) = ability.get("textSpan") else {
            return RawSpan::Absent;
        };
        match span.as_array().map(Vec::as_slice) {
            Some([a, b]) => match (a.as_u64(), b.as_u64()) {
                (Some(start), Some(end)) if start <= end => RawSpan::Valid(start, end),
                _ => RawSpan::Malformed,
            },
            _ => RawSpan::Malformed,
        }
    }

    /// The span as `u32` values, when valid and in range.
    pub(super) fn as_u32(self) -> Option<(u32, u32)> {
        match self {
            RawSpan::Valid(a, b) => Some((u32::try_from(a).ok()?, u32::try_from(b).ok()?)),
            RawSpan::Absent | RawSpan::Malformed => None,
        }
    }
}

/// What one raw walk found inside an ability's JSON.
#[derive(Debug, Default)]
pub(super) struct Scan<'a> {
    /// Look-back keys: (clipped key, rendered path).
    pub(super) lookback: Vec<(String, String)>,
    /// `REPEAT_WHILE` objects without a valid `maxIter`: (rendered path, reason).
    pub(super) bad_repeat: Vec<(String, &'static str)>,
    /// `tokenDef` strings of `CREATE_TOKEN` objects.
    pub(super) token_defs: Vec<&'a str>,
    /// `linkId` strings of `LINKED` references.
    pub(super) linked_refs: Vec<&'a str>,
    /// `mana` strings of `PAY_MANA` and `WATERBEND` cost nodes.
    pub(super) mana_strings: Vec<&'a str>,
}

/// One path segment.
#[derive(Clone, Copy, Debug)]
enum Seg<'a> {
    /// The ability itself.
    Root,
    /// An object member.
    Key(&'a str),
    /// An array element.
    Index(usize),
}

/// Arena node: a segment and its parent.
#[derive(Clone, Copy, Debug)]
struct Node<'a> {
    /// Parent node index (`None` for the root).
    parent: Option<usize>,
    /// This node's segment.
    seg: Seg<'a>,
}

/// Renders the path of `node`: `$` then at most [`PATH_SEGMENTS`] trailing segments, with
/// `...` when earlier segments are omitted.
fn render(arena: &[Node<'_>], node: usize) -> String {
    let mut segs = Vec::new();
    let mut at = Some(node);
    let mut truncated = false;
    while let Some(i) = at {
        let Some(n) = arena.get(i) else { break };
        if matches!(n.seg, Seg::Root) {
            break;
        }
        if segs.len() == PATH_SEGMENTS {
            truncated = true;
            break;
        }
        segs.push(n.seg);
        at = n.parent;
    }
    let mut out = String::from("$");
    if truncated {
        out.push_str("...");
    }
    for seg in segs.iter().rev() {
        match seg {
            Seg::Root => {}
            Seg::Key(k) => {
                out.push('.');
                out.push_str(&clip(k, SEGMENT_CLIP));
            }
            Seg::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

/// Rule E5 form (CR 603.10): the key lower-cased, with `_` and `-` removed, starts with
/// `lookback`.
pub(super) fn is_lookback_key(key: &str) -> bool {
    key.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| *c != '_' && *c != '-')
        .take(8)
        .eq("lookback".chars())
}

/// The string member `key` of `obj`, if any.
fn str_member<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

/// Walks `root` iteratively and records every fact of [`Scan`].
pub(super) fn scan(root: &Value) -> Scan<'_> {
    let mut out = Scan::default();
    let mut arena = vec![Node {
        parent: None,
        seg: Seg::Root,
    }];
    let mut stack: Vec<(&Value, usize)> = vec![(root, 0)];
    while let Some((value, node)) = stack.pop() {
        match value {
            Value::Object(map) => {
                visit_object(map, node, &arena, &mut out);
                for (key, child) in map {
                    let lookback = is_lookback_key(key);
                    let container = child.is_object() || child.is_array();
                    if !lookback && !container {
                        continue;
                    }
                    arena.push(Node {
                        parent: Some(node),
                        seg: Seg::Key(key),
                    });
                    let idx = arena.len() - 1;
                    if lookback {
                        out.lookback.push((clip(key, CLIP), render(&arena, idx)));
                    }
                    if container {
                        stack.push((child, idx));
                    }
                }
            }
            Value::Array(items) => {
                for (i, child) in items.iter().enumerate() {
                    if child.is_object() || child.is_array() {
                        arena.push(Node {
                            parent: Some(node),
                            seg: Seg::Index(i),
                        });
                        stack.push((child, arena.len() - 1));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The object-level facts of one object at `node`.
fn visit_object<'a>(
    map: &'a serde_json::Map<String, Value>,
    node: usize,
    arena: &[Node<'a>],
    out: &mut Scan<'a>,
) {
    match str_member(map, "op") {
        // REQ-DSL-010 loop safety (SRS §14.4.5): REPEAT_WHILE declares maxIter >= 1.
        Some("REPEAT_WHILE") => {
            let why = match map.get("maxIter") {
                None => Some("REPEAT_WHILE has no maxIter"),
                Some(v) => match v.as_u64() {
                    Some(n) if n >= 1 => None,
                    Some(_) => Some("REPEAT_WHILE maxIter is below 1"),
                    None if v.as_i64().is_some() => Some("REPEAT_WHILE maxIter is below 1"),
                    None => Some("REPEAT_WHILE maxIter is not an integer"),
                },
            };
            if let Some(why) = why {
                out.bad_repeat.push((render(arena, node), why));
            }
        }
        Some("CREATE_TOKEN") => {
            if let Some(def) = str_member(map, "tokenDef") {
                out.token_defs.push(def);
            }
        }
        _ => {}
    }
    // CR 607.1: a LINKED reference names the link id of the ability it refers to.
    if str_member(map, "ref") == Some("LINKED")
        && let Some(id) = str_member(map, "linkId")
    {
        out.linked_refs.push(id);
    }
    if matches!(str_member(map, "cost"), Some("PAY_MANA" | "WATERBEND"))
        && let Some(mana) = str_member(map, "mana")
    {
        out.mana_strings.push(mana);
    }
}

/// Facts of one face ability.
#[derive(Debug)]
pub(super) struct AbilityFacts<'a> {
    /// Face index.
    pub(super) face: usize,
    /// Ability index within the face.
    pub(super) index: usize,
    /// The raw JSON.
    pub(super) raw: &'a Value,
    /// The typed view, or the decoder's message.
    pub(super) typed: Result<ability::AbilityAST, String>,
    /// The raw `textSpan`.
    pub(super) span: RawSpan,
    /// The raw walk.
    pub(super) scan: Scan<'a>,
    /// Distinct string `linkIds` the raw ability declares.
    pub(super) link_ids: Vec<&'a str>,
}

impl AbilityFacts<'_> {
    /// The ability's location, with its span when well-formed.
    pub(super) fn location(&self) -> LintLocation {
        LintLocation::ability(self.face, self.index, self.span.as_u32())
    }
}

/// Facts of one partner ability (`commanderEligibility.partnerAbilities[k]`).
#[derive(Debug)]
pub(super) struct PartnerFacts<'a> {
    /// Index in `partnerAbilities`.
    pub(super) index: usize,
    /// The typed view's error, if it fails.
    pub(super) error: Option<String>,
    /// The raw walk.
    pub(super) scan: Scan<'a>,
}

impl PartnerFacts<'_> {
    /// The partner ability's location.
    pub(super) fn location(&self) -> LintLocation {
        LintLocation::card(format!(
            "commanderEligibility.partnerAbilities[{}]",
            self.index
        ))
    }
}

/// Facts of a whole definition.
#[derive(Debug)]
pub(super) struct CardFacts<'a> {
    /// Every face ability, in face then ability order.
    pub(super) abilities: Vec<AbilityFacts<'a>>,
    /// Every partner ability.
    pub(super) partners: Vec<PartnerFacts<'a>>,
}

impl<'a> CardFacts<'a> {
    /// Decodes and walks every ability of `def` once.
    pub(super) fn collect(def: &'a CardDefinitionDSL) -> CardFacts<'a> {
        let mut abilities = Vec::new();
        for (i, face) in def.faces.iter().enumerate() {
            for (j, a) in face.abilities.iter().enumerate() {
                let mut link_ids: Vec<&str> =
                    a.0.get("linkIds")
                        .and_then(Value::as_array)
                        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                link_ids.sort_unstable();
                link_ids.dedup();
                abilities.push(AbilityFacts {
                    face: i,
                    index: j,
                    raw: &a.0,
                    typed: a.to_real(),
                    span: RawSpan::read(&a.0),
                    scan: scan(&a.0),
                    link_ids,
                });
            }
        }
        let partners = def
            .commander_eligibility
            .iter()
            .flat_map(|e| e.partner_abilities.iter().enumerate())
            .map(|(k, p)| PartnerFacts {
                index: k,
                error: p.to_real().err(),
                scan: scan(&p.0),
            })
            .collect();
        CardFacts {
            abilities,
            partners,
        }
    }
}

/// Emits `FPL-LOOK-BACK` and `FPL-MAX-ITER` for one scan at `location`.
fn raw_findings(scan: &Scan<'_>, location: &LintLocation, out: &mut Vec<Lint>) {
    // CR 603.10: the engine decides which triggers look back in time; no definition sets it.
    for (key, path) in &scan.lookback {
        push(
            out,
            LintId::LOOK_BACK,
            Severity::Error,
            location.clone(),
            format!(
                "member {key:?} at {path} sets a look-back flag by hand; the engine derives it \
                 (CR 603.10)"
            ),
        );
    }
    // REQ-DSL-010 loop safety (SRS §14.4.5).
    for (path, why) in &scan.bad_repeat {
        push(
            out,
            LintId::MAX_ITER,
            Severity::Error,
            location.clone(),
            format!("{why} at {path}"),
        );
    }
}

/// `FPL-LOOK-BACK` (CR 603.10) and `FPL-MAX-ITER` (REQ-DSL-010) over every face and partner
/// ability, on the raw JSON (decodable or not).
pub(super) fn check_raw(facts: &CardFacts<'_>, out: &mut Vec<Lint>) {
    for a in &facts.abilities {
        raw_findings(&a.scan, &a.location(), out);
    }
    for p in &facts.partners {
        raw_findings(&p.scan, &p.location(), out);
    }
}
