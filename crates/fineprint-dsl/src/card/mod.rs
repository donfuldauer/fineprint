// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Card definition top-level types (REQ-DSL-001, REQ-DSL-002, SRS §14.2).
//!
//! Field names are snake_case in Rust and camelCase in JSON, exactly as the SRS schema. Every
//! struct rejects unknown members (`deny_unknown_fields`), the string enums accept only their
//! declared strings, and optional members other than `power` and `toughness` must be omitted
//! rather than given as `null`.
//!
//! Plain serde `Deserialize` performs no version or hash check and is not lossless on its own
//! (for example it accepts a positional array for a struct); it is the unchecked building block
//! for value → JSON → value round trips. [`CardDefinitionDSL::from_json`] is the strict loader
//! for untrusted input: it also requires that decoding lost nothing and verifies the hash over
//! the checked input. Only sealed values ([`CardDefinitionDSL::seal`]) carry the guarantee
//! `from_json(&v.to_canonical_json()?) == v`.

use std::collections::BTreeMap;
use std::fmt;

use fineprint_base::hash::sha256;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;

use crate::canonical::{self, CanonicalError};
use crate::mana::ManaCostDSL;

mod version;

pub use version::{DSL_MAJOR, DSL_VERSION, DslVersion, DslVersionError, check_dsl_version};

#[cfg(test)]
mod tests;

/// Declares a fieldless enum whose JSON form is a fixed string per variant, with `ALL` and
/// `as_str`.
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        $name:ident; $n:literal; $( $(#[$vmeta:meta])* $variant:ident => $text:literal ),+ $(,)?
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        pub enum $name {
            $( $(#[$vmeta])* #[serde(rename = $text)] $variant, )+
        }

        // Only a JSON string naming a declared variant is accepted; serde's derived decoder would
        // also accept a singleton object such as `{"VARIANT":null}`.
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_str(EnumVisitor {
                    expecting: stringify!($name),
                    variants: &[ $( $text ),+ ],
                    from_str: |text| $name::ALL.into_iter().find(|v| v.as_str() == text),
                })
            }
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: [$name; $n] = [ $( $name::$variant ),+ ];

            /// The JSON string of this variant.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( $name::$variant => $text, )+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

/// Visitor shared by the [`string_enum!`] types: accepts a string and maps it to a variant.
struct EnumVisitor<T> {
    /// Type name for error messages.
    expecting: &'static str,
    /// The declared variant strings, for the unknown-variant error.
    variants: &'static [&'static str],
    /// Variant for an exact declared string.
    from_str: fn(&str) -> Option<T>,
}

impl<T> de::Visitor<'_> for EnumVisitor<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a {} string", self.expecting)
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<T, E> {
        (self.from_str)(text).ok_or_else(|| E::unknown_variant(text, self.variants))
    }
}

string_enum! {
    /// Layout behavior of a card definition (SRS §4.2).
    LayoutBehavior; 25;
    /// `NORMAL`.
    Normal => "NORMAL",
    /// `SPLIT`.
    Split => "SPLIT",
    /// `ROOM`.
    Room => "ROOM",
    /// `FLIP`.
    Flip => "FLIP",
    /// `LEVELER`.
    Leveler => "LEVELER",
    /// `DFC_NONMODAL`.
    DfcNonmodal => "DFC_NONMODAL",
    /// `DFC_MODAL`.
    DfcModal => "DFC_MODAL",
    /// `MELD`.
    Meld => "MELD",
    /// `SAGA`.
    Saga => "SAGA",
    /// `ADVENTURER`.
    Adventurer => "ADVENTURER",
    /// `CLASS`.
    Class => "CLASS",
    /// `ATTRACTION`.
    Attraction => "ATTRACTION",
    /// `PROTOTYPE`.
    Prototype => "PROTOTYPE",
    /// `CASE`.
    Case => "CASE",
    /// `OMEN`.
    Omen => "OMEN",
    /// `STATION`.
    Station => "STATION",
    /// `PREPARATION`.
    Preparation => "PREPARATION",
    /// `MUTATE`.
    Mutate => "MUTATE",
    /// `TOKEN`.
    Token => "TOKEN",
    /// `DFC_TOKEN`.
    DfcToken => "DFC_TOKEN",
    /// `PLANAR`.
    Planar => "PLANAR",
    /// `SCHEME`.
    Scheme => "SCHEME",
    /// `VANGUARD`.
    Vanguard => "VANGUARD",
    /// `CONSPIRACY`.
    Conspiracy => "CONSPIRACY",
    /// `DUNGEON`.
    Dungeon => "DUNGEON",
}

string_enum! {
    /// Role of a face within its card (SRS §14.2 `faceKind`).
    FaceKind; 10;
    /// `MAIN`.
    Main => "MAIN",
    /// `BACK`.
    Back => "BACK",
    /// `LEFT_HALF`.
    LeftHalf => "LEFT_HALF",
    /// `RIGHT_HALF`.
    RightHalf => "RIGHT_HALF",
    /// `ADVENTURE`.
    Adventure => "ADVENTURE",
    /// `OMEN`.
    Omen => "OMEN",
    /// `PREPARE_SPELL`.
    PrepareSpell => "PREPARE_SPELL",
    /// `PROTOTYPE`.
    Prototype => "PROTOTYPE",
    /// `FLIPPED`.
    Flipped => "FLIPPED",
    /// `MELD_RESULT`.
    MeldResult => "MELD_RESULT",
}

string_enum! {
    /// Support status of a definition. This type only carries the status; no code path in this
    /// module assigns `VERIFIED` (R11).
    SupportStatus; 4;
    /// `VERIFIED`: earned only through passing behavioral tests.
    Verified => "VERIFIED",
    /// `AUTO_PARSED`.
    AutoParsed => "AUTO_PARSED",
    /// `PARTIAL`.
    Partial => "PARTIAL",
    /// `UNSUPPORTED`.
    Unsupported => "UNSUPPORTED",
}

string_enum! {
    /// Who produced a definition (provenance `origin`).
    Origin; 3;
    /// `PARSER`.
    Parser => "PARSER",
    /// `CLIENT_OVERRIDE`.
    ClientOverride => "CLIENT_OVERRIDE",
    /// `MIXED`.
    Mixed => "MIXED",
}

string_enum! {
    /// Legality of a card in one format.
    Legality; 4;
    /// `LEGAL`.
    Legal => "LEGAL",
    /// `BANNED`.
    Banned => "BANNED",
    /// `RESTRICTED`.
    Restricted => "RESTRICTED",
    /// `NOT_LEGAL`.
    NotLegal => "NOT_LEGAL",
}

/// A set of colors as a bitmask: W = 1, U = 2, B = 4, R = 8, G = 16. JSON form: an integer
/// `0..=31`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColorBitmask(u8);

impl ColorBitmask {
    /// The empty set (colorless).
    pub const NONE: ColorBitmask = ColorBitmask(0);
    /// White bit.
    pub const WHITE: ColorBitmask = ColorBitmask(1);
    /// Blue bit.
    pub const BLUE: ColorBitmask = ColorBitmask(2);
    /// Black bit.
    pub const BLACK: ColorBitmask = ColorBitmask(4);
    /// Red bit.
    pub const RED: ColorBitmask = ColorBitmask(8);
    /// Green bit.
    pub const GREEN: ColorBitmask = ColorBitmask(16);

    /// The mask for `bits`, or `None` if any bit above the five colors is set (`bits > 31`).
    #[must_use]
    pub const fn new(bits: u8) -> Option<ColorBitmask> {
        if bits <= 31 {
            Some(ColorBitmask(bits))
        } else {
            None
        }
    }

    /// The raw bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl Serialize for ColorBitmask {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for ColorBitmask {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bits = u8::deserialize(deserializer)?;
        ColorBitmask::new(bits).ok_or_else(|| {
            serde::de::Error::custom(format_args!("color bitmask {bits} is outside 0..=31"))
        })
    }
}

/// Placeholder for the value-expression AST (SRS §14.3); issue FP-011/012/013 replaces it with
/// the real schema. Its content still participates in canonical JSON and the hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ValueExpressionAST(pub Value);

/// Placeholder for the ability AST (SRS §14.3); issue FP-011/012/013 replaces it with the real
/// schema. Its content still participates in canonical JSON and the hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AbilityAST(pub Value);

/// Placeholder for a partner ability (commander eligibility); a later DSL issue replaces it
/// with the real schema. Its content still participates in canonical JSON and the hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PartnerAbilityDSL(pub Value);

/// Placeholder for the rules-text token stream (SRS §14.5); issue FP-011/012/013 replaces it
/// with the real schema. Its content still participates in canonical JSON and the hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TextTokenStream(pub Vec<Value>);

/// Deserialises a present member as `Some(T)`; an absent member is `None` through
/// `#[serde(default)]`. A present JSON `null` is decoded by `T` itself: for `power` and
/// `toughness` that is `Some(ValueExpressionAST(Value::Null))`, which survives a value → JSON →
/// value round trip; for every other optional member (`manaCost`, `loyalty`, `knownGaps`, ...)
/// `T` refuses `null`, so `null` is an error rather than a silent `None`.
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// Provenance of a definition (JSON member `source`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvenanceDSL {
    /// Hash of the normalized rules text and characteristics the definition was written against.
    pub oracle_text_hash: String,
    /// Who produced the definition.
    pub origin: Origin,
    /// Parser version, if parsed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub parser_version: Option<String>,
    /// Client-supplied identifier of the override's author.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub override_source: Option<String>,
    /// Definition hashes of the tests that earned `VERIFIED`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub verified_by: Option<Vec<String>>,
    /// Rules baseline the definition was checked against.
    pub rules_baseline: String,
}

/// Commander eligibility (JSON member `commanderEligibility`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommanderEligibilityDSL {
    /// Whether the card can be a commander.
    pub eligible: bool,
    /// Partner-style abilities.
    pub partner_abilities: Vec<PartnerAbilityDSL>,
}

/// One face of a card (SRS §14.2 `CardFaceDSL`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardFaceDSL {
    /// Role of this face.
    pub face_kind: FaceKind,
    /// Face name.
    pub name: String,
    /// Mana cost. `None` = "no mana cost" (member omitted); `Some` cost `{0}` is different.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub mana_cost: Option<ManaCostDSL>,
    /// Color indicator, if any.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub color_indicator: Option<ColorBitmask>,
    /// Supertypes.
    pub supertypes: Vec<String>,
    /// Card types.
    pub card_types: Vec<String>,
    /// Subtypes.
    pub subtypes: Vec<String>,
    /// Printed power, if any.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub power: Option<ValueExpressionAST>,
    /// Printed toughness, if any.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub toughness: Option<ValueExpressionAST>,
    /// Printed loyalty, if any.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub loyalty: Option<i64>,
    /// Printed defense, if any.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub defense: Option<i64>,
    /// Hand modifier, if any.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub hand_modifier: Option<i64>,
    /// Life modifier, if any.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub life_modifier: Option<i64>,
    /// Rules text token stream.
    pub text: TextTokenStream,
    /// Abilities.
    pub abilities: Vec<AbilityAST>,
}

/// A card definition (SRS §14.2 `CardDefinitionDSL`, REQ-DSL-002).
///
/// Plain serde `Deserialize` does not check `dslVersion` or `definitionHash`; use
/// [`CardDefinitionDSL::from_json`] for untrusted input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardDefinitionDSL {
    /// Stable card identifier.
    pub card_def_id: String,
    /// DSL schema version, for example `"1.0"`.
    pub dsl_version: String,
    /// SHA-256 (64 lowercase hex) of the canonical JSON of this definition without this member.
    pub definition_hash: String,
    /// Provenance.
    pub source: ProvenanceDSL,
    /// Support status.
    pub support_status: SupportStatus,
    /// Unimplemented clauses (for `PARTIAL`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub known_gaps: Option<Vec<String>>,
    /// Layout behavior.
    pub layout: LayoutBehavior,
    /// Faces, ordered per layout.
    pub faces: Vec<CardFaceDSL>,
    /// Meld partner reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub meld_partner: Option<String>,
    /// Precomputed color identity.
    pub color_identity: ColorBitmask,
    /// Commander eligibility.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub commander_eligibility: Option<CommanderEligibilityDSL>,
    /// Format → legality (ordered map, REQ-CORE-006).
    pub legalities: BTreeMap<String, Legality>,
    /// Digital-only flag.
    pub digital_only: bool,
    /// Token definitions this card can create.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub related_tokens: Option<Vec<String>>,
}

/// JSON member name of the hash, excluded from the hashed bytes.
const DEFINITION_HASH_MEMBER: &str = "definitionHash";

/// JSON member name of the version, checked before the rest is decoded.
const DSL_VERSION_MEMBER: &str = "dslVersion";

impl CardDefinitionDSL {
    /// RFC 8785 canonical JSON of the whole definition, including the current
    /// `definitionHash` value (REQ-NFR-006).
    ///
    /// # Errors
    /// [`CanonicalError`] if the value cannot be canonicalised (for example an integer outside
    /// the safe range in a placeholder AST).
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, CanonicalError> {
        canonical::to_canonical_vec(self)
    }

    /// SHA-256 (64 lowercase hex characters) of the RFC 8785 bytes of this definition with the
    /// `definitionHash` member omitted, so the result does not depend on the current
    /// `definition_hash` value (REQ-DSL-002, REQ-NFR-006).
    ///
    /// # Errors
    /// [`CanonicalError`] if the value cannot be canonicalised.
    pub fn compute_hash(&self) -> Result<String, CanonicalError> {
        // The member is omitted from the hash input, so a stale value is never validated here.
        let mut copy = self.clone();
        copy.definition_hash.clear();
        hash_without_member(canonical::to_checked_value(&copy)?)
    }

    /// Returns `self` normalised and with `definition_hash` set to
    /// [`CardDefinitionDSL::compute_hash`].
    ///
    /// Normalising means replacing the value by what its canonical JSON decodes to, so numbers
    /// inside placeholder ASTs take their canonical form (`-0.0` becomes `0`, `1.0` becomes `1`).
    /// The canonical bytes and the hash are unchanged by this, and the result `v` satisfies
    /// `CardDefinitionDSL::from_json(&v.to_canonical_json()?) == Ok(v)`. The previous
    /// `definition_hash` is ignored, whatever it contains.
    ///
    /// # Errors
    /// [`CanonicalError::Serialize`] carrying the [`DslVersionError`] message if `dsl_version`
    /// is one [`CardDefinitionDSL::from_json`] would reject (malformed, or a different major
    /// version); otherwise [`CanonicalError`] if the value cannot be canonicalised.
    pub fn seal(mut self) -> Result<CardDefinitionDSL, CanonicalError> {
        check_dsl_version(&self.dsl_version)
            .map_err(|e| CanonicalError::Serialize(e.to_string()))?;
        self.definition_hash.clear();
        let bytes = self.to_canonical_json()?;
        let value = canonical::parse_checked(&bytes)?;
        let mut sealed: CardDefinitionDSL =
            serde_json::from_value(value).map_err(|e| CanonicalError::Serialize(e.to_string()))?;
        sealed.definition_hash = sealed.compute_hash()?;
        Ok(sealed)
    }

    /// Checks that `definition_hash` equals [`CardDefinitionDSL::compute_hash`] exactly
    /// (uppercase hex is a mismatch).
    ///
    /// # Errors
    /// [`CardDefinitionError::HashMismatch`], or [`CardDefinitionError::Canonical`] if the hash
    /// cannot be computed.
    pub fn verify_hash(&self) -> Result<(), CardDefinitionError> {
        let computed = self.compute_hash()?;
        if computed == self.definition_hash {
            Ok(())
        } else {
            Err(CardDefinitionError::HashMismatch {
                declared: self.definition_hash.clone(),
                computed,
            })
        }
    }

    /// Strict loader for untrusted JSON, in this order:
    ///
    /// (a) valid JSON with no duplicate member names, no unsafe integers, no noncharacters and
    /// no nesting deeper than [`canonical::MAX_DEPTH`]; the document is then taken in its
    /// canonical form (numbers normalised, so `1.0` and `1` load the same);
    /// (b) a top-level object whose `dslVersion` is a string with the engine's major version,
    /// checked before anything else is decoded;
    /// (c) the typed schema, with unknown and missing members rejected, and decoding must be
    /// lossless: the decoded definition re-serialised must equal the document exactly, which
    /// rejects `null` for an optional member, a positional array where an object is required,
    /// a non-string enum and any other content the typed value would not reproduce;
    /// (d) the `definitionHash`, compared exactly with the SHA-256 of the RFC 8785 bytes of the
    /// checked document with only the `definitionHash` member removed.
    ///
    /// # Errors
    /// [`CardDefinitionError::Canonical`], [`CardDefinitionError::Schema`],
    /// [`CardDefinitionError::Version`] or [`CardDefinitionError::HashMismatch`].
    pub fn from_json(bytes: &[u8]) -> Result<CardDefinitionDSL, CardDefinitionError> {
        let value = canonical::parse_normalized(bytes)?;
        let version = match &value {
            Value::Object(map) => match map.get(DSL_VERSION_MEMBER) {
                Some(Value::String(text)) => text,
                Some(_) => {
                    return Err(CardDefinitionError::Schema(
                        "member \"dslVersion\" must be a string".to_owned(),
                    ));
                }
                None => {
                    return Err(CardDefinitionError::Schema(
                        "missing required member \"dslVersion\"".to_owned(),
                    ));
                }
            },
            _ => {
                return Err(CardDefinitionError::Schema(
                    "a card definition must be a JSON object".to_owned(),
                ));
            }
        };
        check_dsl_version(version)?;
        let definition: CardDefinitionDSL = serde_json::from_value(value.clone())
            .map_err(|e| CardDefinitionError::Schema(e.to_string()))?;
        if canonical::to_checked_value(&definition)? != value {
            return Err(CardDefinitionError::Schema(
                "decoding changed the content (for example a null member, a positional array \
                 or a non-object where an object is required, or a non-string enum value)"
                    .to_owned(),
            ));
        }
        let computed = hash_without_member(value)?;
        if computed != definition.definition_hash {
            return Err(CardDefinitionError::HashMismatch {
                declared: definition.definition_hash,
                computed,
            });
        }
        Ok(definition)
    }
}

/// SHA-256 (lowercase hex) of the RFC 8785 bytes of `value` with its top-level
/// `definitionHash` member removed.
fn hash_without_member(mut value: Value) -> Result<String, CanonicalError> {
    if let Value::Object(map) = &mut value {
        map.remove(DEFINITION_HASH_MEMBER);
    }
    let bytes = canonical::to_canonical_vec(&value)?;
    Ok(sha256(&bytes).to_hex())
}

/// Why [`CardDefinitionDSL::from_json`] rejected a definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardDefinitionError {
    /// Invalid JSON, a duplicate member name or an unsafe integer.
    Canonical(CanonicalError),
    /// A malformed `dslVersion` or a different major version.
    Version(DslVersionError),
    /// The JSON does not match the definition schema.
    Schema(String),
    /// The declared `definitionHash` differs from the computed one.
    HashMismatch {
        /// The `definitionHash` in the input.
        declared: String,
        /// The hash computed from the definition.
        computed: String,
    },
}

impl fmt::Display for CardDefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CardDefinitionError::Canonical(e) => write!(f, "card definition JSON rejected: {e}"),
            CardDefinitionError::Version(e) => write!(f, "card definition rejected: {e}"),
            CardDefinitionError::Schema(msg) => {
                write!(f, "card definition does not match the schema: {msg}")
            }
            CardDefinitionError::HashMismatch { declared, computed } => write!(
                f,
                "definitionHash mismatch: declared {declared:?}, computed {computed:?}"
            ),
        }
    }
}

impl std::error::Error for CardDefinitionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CardDefinitionError::Canonical(e) => Some(e),
            CardDefinitionError::Version(e) => Some(e),
            CardDefinitionError::Schema(_) | CardDefinitionError::HashMismatch { .. } => None,
        }
    }
}

impl From<CanonicalError> for CardDefinitionError {
    fn from(e: CanonicalError) -> Self {
        CardDefinitionError::Canonical(e)
    }
}

impl From<DslVersionError> for CardDefinitionError {
    fn from(e: DslVersionError) -> Self {
        CardDefinitionError::Version(e)
    }
}
