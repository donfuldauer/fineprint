// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Static validation lints over a card definition (REQ-ING-009, with REQ-OBJ-001 and
//! REQ-ING-002 for mana value, colors and color identity).
//!
//! [`lint_definition`] runs one check per REQ-ING-009 bullet over a [`CardDefinitionDSL`] and
//! returns a [`LintReport`]: every finding carries a stable [`LintId`], a [`Severity`] and a
//! [`LintLocation`] (face, ability, JSON path and text span). The report is sorted by a total key
//! and de-duplicated, so the output is a pure, deterministic function of the input (R3).
//!
//! The checks take untrusted definitions (client overrides, R12), so every check is total. The
//! AST positions of a definition are the real typed ASTs (docs/DECISIONS.md, "#232 FP-164 —
//! Lint schema check after typed storage"); the checks that read JSON read each ability's JSON
//! form, encoded once with the depth-checked serializer and walked with an explicit stack (never
//! recursion). Messages carry only identifiers the definition itself holds (ability ids, token
//! ids, link ids, JSON member names), clipped to a short length.
//!
//! Raw versus typed (docs/DECISIONS.md, "#16 FP-015 — Raw versus typed lint inputs"): the
//! checks for violations the typed decoders make unrepresentable (`FPL-LOOK-BACK`,
//! `FPL-MAX-ITER`, `FPL-LAYER-KIND`) and the structural checks (`FPL-LINK-ID`,
//! `FPL-TOKEN-REF`, the span reading of `FPL-TEXT-COVERAGE`) read the JSON form of every
//! ability; `FPL-MANA-ABILITY`, `FPL-TIER-HINT` and the characteristics checks read the typed
//! abilities, always. `FPL-SCHEMA` reports every position whose typed value does not encode (a
//! hand-built value that breaks an invariant of its type), in addition.

use std::collections::BTreeSet;
use std::fmt;

use crate::card::{CardDefinitionDSL, ColorBitmask};

mod characteristics;
mod links;
mod mana_ability;
mod replacement;
mod schema;
mod statics;
mod text;
mod walk;

#[cfg(test)]
mod tests;

/// How serious a finding is. `Error` sorts before `Warning`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The definition violates a requirement.
    Error,
    /// The definition is suspicious but may be correct.
    Warning,
}

impl Severity {
    /// `"ERROR"` or `"WARNING"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "ERROR",
            Severity::Warning => "WARNING",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stable lint identifier such as `"FPL-SCHEMA"` (REQ-ING-009). Ordered by its string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LintId(&'static str);

impl LintId {
    /// `FPL-SCHEMA`: every AST position decodes as its real DSL type (REQ-ING-009 schema
    /// validity; REQ-DSL-003, REQ-DSL-010, REQ-DSL-012).
    pub const SCHEMA: LintId = LintId("FPL-SCHEMA");
    /// `FPL-MANA-ABILITY`: a declared `isManaAbility: true` is consistent with CR 605.1a and
    /// CR 605.1b (REQ-ING-009).
    pub const MANA_ABILITY: LintId = LintId("FPL-MANA-ABILITY");
    /// `FPL-TIER-HINT`: a replacement's `tierHint` is consistent with CR 616.1b–d
    /// (REQ-ING-009, REQ-DSL-009).
    pub const TIER_HINT: LintId = LintId("FPL-TIER-HINT");
    /// `FPL-LAYER-KIND`: every static effect declares its layer or RuleSet kind (REQ-ING-009,
    /// REQ-DSL-008).
    pub const LAYER_KIND: LintId = LintId("FPL-LAYER-KIND");
    /// `FPL-TEXT-COVERAGE`: every text token is covered by some ability's `textSpan`
    /// (REQ-ING-009, SRS §14.3).
    pub const TEXT_COVERAGE: LintId = LintId("FPL-TEXT-COVERAGE");
    /// `FPL-LOOK-BACK`: no ability sets a look-back flag by hand; the engine derives it
    /// (REQ-ING-009, CR 603.10).
    pub const LOOK_BACK: LintId = LintId("FPL-LOOK-BACK");
    /// `FPL-MAX-ITER`: every `REPEAT_WHILE` declares `maxIter >= 1` (REQ-ING-009,
    /// REQ-DSL-010, SRS §14.4.5).
    pub const MAX_ITER: LintId = LintId("FPL-MAX-ITER");
    /// `FPL-TOKEN-REF`: referenced token definitions exist (REQ-ING-009).
    pub const TOKEN_REF: LintId = LintId("FPL-TOKEN-REF");
    /// `FPL-LINK-ID`: linked abilities share a link id (REQ-ING-009, CR 607).
    pub const LINK_ID: LintId = LintId("FPL-LINK-ID");
    /// `FPL-MANA-VALUE`: computed mana value matches the source data (REQ-ING-009,
    /// REQ-ING-002, REQ-OBJ-001, CR 202.3).
    pub const MANA_VALUE: LintId = LintId("FPL-MANA-VALUE");
    /// `FPL-COLORS`: computed colors match the source data (REQ-ING-009, REQ-ING-002,
    /// CR 202.2, CR 105.2, CR 204.2).
    pub const COLORS: LintId = LintId("FPL-COLORS");
    /// `FPL-COLOR-IDENTITY`: computed color identity matches the declared and source values
    /// (REQ-ING-009, REQ-ING-002, CR 903.4).
    pub const COLOR_IDENTITY: LintId = LintId("FPL-COLOR-IDENTITY");

    /// Every lint id, in declaration order.
    pub const ALL: [LintId; 12] = [
        LintId::SCHEMA,
        LintId::MANA_ABILITY,
        LintId::TIER_HINT,
        LintId::LAYER_KIND,
        LintId::TEXT_COVERAGE,
        LintId::LOOK_BACK,
        LintId::MAX_ITER,
        LintId::TOKEN_REF,
        LintId::LINK_ID,
        LintId::MANA_VALUE,
        LintId::COLORS,
        LintId::COLOR_IDENTITY,
    ];

    /// The id string, e.g. `"FPL-SCHEMA"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for LintId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Where a finding is. `path` grammar: `"$"` (card level), `"faces[i]"`, `"faces[i].power"`,
/// `"faces[i].toughness"`, `"faces[i].text"`, `"faces[i].abilities[j]"`,
/// `"commanderEligibility.partnerAbilities[k]"` and `"relatedTokens[k]"`. `face_index` and
/// `ability_index` are `Some` exactly for the face and ability the path names.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LintLocation {
    /// The face, when the path names one.
    pub face_index: Option<usize>,
    /// The ability within the face, when the path names one.
    pub ability_index: Option<usize>,
    /// The location as a path (grammar above).
    pub path: String,
    /// Inclusive token span of `face.text`: the uncovered run for a coverage finding, the
    /// ability's own well-formed `textSpan` for an ability-level finding, else `None`.
    pub text_span: Option<(u32, u32)>,
}

impl LintLocation {
    /// Card-level location `path` (no face, no ability, no span).
    fn card(path: impl Into<String>) -> LintLocation {
        LintLocation {
            face_index: None,
            ability_index: None,
            path: path.into(),
            text_span: None,
        }
    }

    /// Face-level location `faces[i]` plus `suffix` (e.g. `".power"`).
    fn face(i: usize, suffix: &str) -> LintLocation {
        LintLocation {
            face_index: Some(i),
            ability_index: None,
            path: format!("faces[{i}]{suffix}"),
            text_span: None,
        }
    }

    /// Ability-level location `faces[i].abilities[j]` with the ability's span.
    fn ability(i: usize, j: usize, text_span: Option<(u32, u32)>) -> LintLocation {
        LintLocation {
            face_index: Some(i),
            ability_index: Some(j),
            path: format!("faces[{i}].abilities[{j}]"),
            text_span,
        }
    }
}

/// One finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lint {
    /// Which check.
    pub id: LintId,
    /// How serious.
    pub severity: Severity,
    /// Where.
    pub location: LintLocation,
    /// What is wrong (identifiers only, never card text).
    pub message: String,
}

impl Lint {
    /// The total sort key of R-2: face, ability, id, severity, span, path, message.
    #[allow(clippy::type_complexity)]
    fn key(
        &self,
    ) -> (
        Option<usize>,
        Option<usize>,
        &'static str,
        Severity,
        Option<(u32, u32)>,
        &str,
        &str,
    ) {
        (
            self.location.face_index,
            self.location.ability_index,
            self.id.as_str(),
            self.severity,
            self.location.text_span,
            &self.location.path,
            &self.message,
        )
    }
}

impl fmt::Display for Lint {
    /// `"{SEVERITY} {id} {path}{span}: {message}"`, span = `" [start,end]"` when present.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.severity, self.id, self.location.path)?;
        if let Some((start, end)) = self.location.text_span {
            write!(f, " [{start},{end}]")?;
        }
        write!(f, ": {}", self.message)
    }
}

/// The sorted, de-duplicated findings of [`lint_definition`].
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LintReport {
    /// The findings, in the total order of R-2.
    pub lints: Vec<Lint>,
}

impl LintReport {
    /// No findings at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lints.is_empty()
    }

    /// At least one `Error`.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.errors().next().is_some()
    }

    /// The `Error` findings, in report order.
    pub fn errors(&self) -> impl Iterator<Item = &Lint> {
        self.lints.iter().filter(|l| l.severity == Severity::Error)
    }

    /// The `Warning` findings, in report order.
    pub fn warnings(&self) -> impl Iterator<Item = &Lint> {
        self.lints
            .iter()
            .filter(|l| l.severity == Severity::Warning)
    }

    /// The findings of one lint id, in report order.
    pub fn with_id(&self, id: LintId) -> impl Iterator<Item = &Lint> {
        self.lints.iter().filter(move |l| l.id == id)
    }
}

/// Characteristics of one face from the source data (REQ-ING-002), compared with the face of
/// the same index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceFaceCharacteristics {
    /// Source mana value (integer; the source's floating `cmc` is converted by the caller).
    pub mana_value: Option<u32>,
    /// Source colors.
    pub colors: Option<ColorBitmask>,
}

/// Characteristics from the source data (REQ-ING-002). `faces[i]` is compared with definition
/// face `i`; extra or missing entries are ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceCharacteristics {
    /// Per-face characteristics.
    pub faces: Vec<SourceFaceCharacteristics>,
    /// Source color identity.
    pub color_identity: Option<ColorBitmask>,
}

/// Optional inputs of the checks. An absent input skips the checks that need it, with no
/// finding (docs/DECISIONS.md, "#16 FP-015 — Source characteristics and token ids").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LintContext {
    /// Source characteristics for `FPL-MANA-VALUE`, `FPL-COLORS` and the source comparison of
    /// `FPL-COLOR-IDENTITY`.
    pub source: Option<SourceCharacteristics>,
    /// Known token definition ids for `FPL-TOKEN-REF`.
    pub known_token_ids: Option<BTreeSet<String>>,
}

/// Runs every check of REQ-ING-009 over `def` and returns the sorted, de-duplicated findings.
///
/// Pure and total (R3, R12): the result depends only on the arguments, and no input (deep
/// nesting, wrong types, `null`, huge integers, no faces) makes it panic or overflow the stack.
#[must_use]
pub fn lint_definition(def: &CardDefinitionDSL, ctx: &LintContext) -> LintReport {
    let raw = walk::RawJson::of(def);
    let facts = walk::CardFacts::collect(def, &raw);
    lint_facts(def, &facts, ctx)
}

/// Runs every check over `facts` (the facts of `def`) and returns the sorted, de-duplicated
/// report.
fn lint_facts(
    def: &CardDefinitionDSL,
    facts: &walk::CardFacts<'_>,
    ctx: &LintContext,
) -> LintReport {
    let mut out = Vec::new();
    schema::check(def, facts, &mut out);
    mana_ability::check(def, facts, &mut out);
    replacement::check(def, facts, &mut out);
    statics::check(def, facts, &mut out);
    text::check(def, facts, &mut out);
    walk::check_raw(facts, &mut out);
    links::check_tokens(def, facts, ctx, &mut out);
    links::check_links(facts, &mut out);
    characteristics::check(def, facts, ctx, &mut out);
    out.sort_by(|a, b| a.key().cmp(&b.key()));
    out.dedup();
    LintReport { lints: out }
}

/// Longest message fragment copied from the input (in `char`s).
const CLIP: usize = 64;

/// Longest decoder message kept (in `char`s).
const CLIP_MESSAGE: usize = 240;

/// `text` cut to at most `max` characters, with `"..."` appended when cut.
fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}...", &text[..at]),
        None => text.to_owned(),
    }
}

/// `id` quoted for a message: `{:?}` of [`clip`]`(id, CLIP)`, and when the id was cut, its
/// length in `char`s and a 64-bit FNV-1a digest of its bytes in hex, so two ids that differ
/// only after the cut still give distinct findings while repeats of one id stay equal (and
/// collapse in the report). Pure and deterministic (R3).
fn quote_id(id: &str) -> String {
    let clipped = clip(id, CLIP);
    if id.chars().nth(CLIP).is_none() {
        return format!("{clipped:?}");
    }
    // FNV-1a, 64-bit: offset basis and prime of the published parameters.
    let digest = id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!(
        "{clipped:?} ({} chars, digest {digest:016x})",
        id.chars().count()
    )
}

/// Pushes a finding.
fn push(
    out: &mut Vec<Lint>,
    id: LintId,
    severity: Severity,
    location: LintLocation,
    message: String,
) {
    out.push(Lint {
        id,
        severity,
        location,
        message,
    });
}
