// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Facts read once from a definition for every check (REQ-ING-009): every typed ability, its
//! JSON form (encoded once, [`RawJson`]) and one iterative walk over that JSON, plus the raw
//! checks `FPL-LOOK-BACK` (CR 603.10) and `FPL-MAX-ITER` (REQ-DSL-010, SRS §14.4.5).
//!
//! The AST positions of a [`CardDefinitionDSL`] hold the real typed ASTs (docs/DECISIONS.md,
//! "#232 FP-164 — Lint schema check after typed storage"), so every decoded ability is valid;
//! only a hand-built value that breaks an invariant of its type fails to encode, and then its
//! JSON form is `null` (nothing to walk) and the encoder's message is kept for `FPL-SCHEMA`.
//!
//! The walk uses an explicit stack, so a value nested far beyond
//! [`crate::canonical::MAX_DEPTH`] is walked without recursion (R12). Paths in messages are
//! rendered from the last few segments only, so rendering is bounded whatever the depth.

use serde_json::Value;

use super::{CLIP, Lint, LintId, LintLocation, Severity, clip, push};
use crate::ability::AbilityAST;
use crate::canonical;
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
///
/// The `params` member of a `NATIVE_EFFECT` instruction is opaque data handed to the
/// registered native effect (REQ-DSL-015, SRS §14.8), so a look-back-like key inside it is not
/// a look-back flag and is not recorded (docs/DECISIONS.md, "Gate 06-turn F53 — Look-back scan
/// skips native-effect params"). Every other fact is still recorded there, and look-back keys
/// everywhere else (trigger matchers, filters, bindings, keyword `params`) are recorded.
pub(super) fn scan(root: &Value) -> Scan<'_> {
    let mut out = Scan::default();
    let mut arena = vec![Node {
        parent: None,
        seg: Seg::Root,
    }];
    // (value, arena node, inside native-effect params)
    let mut stack: Vec<(&Value, usize, bool)> = vec![(root, 0, false)];
    while let Some((value, node, opaque)) = stack.pop() {
        match value {
            Value::Object(map) => {
                visit_object(map, node, &arena, &mut out);
                let native = str_member(map, "op") == Some("NATIVE_EFFECT");
                for (key, child) in map {
                    let child_opaque = opaque || (native && key == "params");
                    let lookback = !child_opaque && is_lookback_key(key);
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
                        stack.push((child, idx, child_opaque));
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
                        stack.push((child, arena.len() - 1, opaque));
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
    /// The JSON form (`null` when the ability does not encode).
    pub(super) raw: &'a Value,
    /// The typed ability.
    pub(super) typed: &'a AbilityAST,
    /// The encoder's message when the typed ability does not encode (a hand-built value that
    /// breaks an invariant of its type).
    pub(super) error: Option<&'a str>,
    /// The raw `textSpan` (the typed base span when the ability does not encode).
    pub(super) span: RawSpan,
    /// The raw walk.
    pub(super) scan: Scan<'a>,
    /// Distinct string `linkIds` the raw ability declares.
    pub(super) link_ids: Vec<&'a str>,
}

impl<'a> AbilityFacts<'a> {
    /// The facts of the ability at `faces[face].abilities[index]`: `typed`, its JSON form `raw`
    /// and the encoder's message `error` when it does not encode (then `raw` is `null`).
    pub(super) fn new(
        face: usize,
        index: usize,
        typed: &'a AbilityAST,
        raw: &'a Value,
        error: Option<&'a str>,
    ) -> AbilityFacts<'a> {
        let mut link_ids: Vec<&str> = raw
            .get("linkIds")
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        link_ids.sort_unstable();
        link_ids.dedup();
        // An ability that does not encode has no JSON form to read: its span is the typed
        // base `text_span` (malformed unless `start <= end`), and the failed JSON (`null`) is
        // kept out of the raw scan (docs/DECISIONS.md, "Gate 06-turn F104 — Span of an
        // ability that does not encode").
        let (span, scan) = if error.is_some() {
            let span = match typed.base().text_span {
                None => RawSpan::Absent,
                Some((start, end)) if start <= end => {
                    RawSpan::Valid(u64::from(start), u64::from(end))
                }
                Some(_) => RawSpan::Malformed,
            };
            (span, Scan::default())
        } else {
            (RawSpan::read(raw), scan(raw))
        };
        AbilityFacts {
            face,
            index,
            raw,
            typed,
            error,
            span,
            scan,
            link_ids,
        }
    }

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
    /// The encoder's message when the partner ability does not encode.
    pub(super) error: Option<&'a str>,
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

/// The JSON form of one typed value: its checked JSON, or `null` and the encoder's message.
#[derive(Debug)]
pub(super) struct Encoded {
    /// The JSON form (`null` when it does not encode).
    value: Value,
    /// The encoder's message when it does not encode.
    error: Option<String>,
}

impl Encoded {
    /// Encodes `value` with the checked serializer of [`crate::canonical`] (depth-checked,
    /// never recursing past the budget).
    fn of<T: serde::Serialize>(value: &T) -> Encoded {
        match canonical::to_checked_value(value) {
            Ok(value) => Encoded { value, error: None },
            Err(e) => Encoded {
                value: Value::Null,
                error: Some(e.to_string()),
            },
        }
    }
}

/// The JSON forms of every face ability (per face) and partner ability of a definition,
/// encoded once so the facts can borrow them.
#[derive(Debug)]
pub(super) struct RawJson {
    /// Per face, per ability.
    abilities: Vec<Vec<Encoded>>,
    /// Per partner ability.
    partners: Vec<Encoded>,
}

impl RawJson {
    /// Encodes every face and partner ability of `def`.
    pub(super) fn of(def: &CardDefinitionDSL) -> RawJson {
        RawJson {
            abilities: def
                .faces
                .iter()
                .map(|f| f.abilities.iter().map(Encoded::of).collect())
                .collect(),
            partners: def
                .commander_eligibility
                .iter()
                .flat_map(|e| e.partner_abilities.iter())
                .map(|p| Encoded::of(p.as_ability()))
                .collect(),
        }
    }
}

impl<'a> CardFacts<'a> {
    /// Collects the facts of every ability of `def`, walking the JSON forms in `raw` (which
    /// must be [`RawJson::of`] the same definition) once.
    pub(super) fn collect(def: &'a CardDefinitionDSL, raw: &'a RawJson) -> CardFacts<'a> {
        let mut abilities = Vec::new();
        for (i, (face, encoded)) in def.faces.iter().zip(&raw.abilities).enumerate() {
            for (j, (a, e)) in face.abilities.iter().zip(encoded).enumerate() {
                abilities.push(AbilityFacts::new(i, j, a, &e.value, e.error.as_deref()));
            }
        }
        let partners = raw
            .partners
            .iter()
            .enumerate()
            .map(|(k, e)| PartnerFacts {
                index: k,
                error: e.error.as_deref(),
                scan: scan(&e.value),
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
/// ability, on its JSON form.
pub(super) fn check_raw(facts: &CardFacts<'_>, out: &mut Vec<Lint>) {
    for a in &facts.abilities {
        raw_findings(&a.scan, &a.location(), out);
    }
    for p in &facts.partners {
        raw_findings(&p.scan, &p.location(), out);
    }
}
