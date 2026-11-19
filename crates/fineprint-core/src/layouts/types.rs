// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The public types of the layouts module (REQ-OBJ-002).

use core::fmt;

use fineprint_dsl::card::{AbilityAST, CardDefinitionDSL, LayoutBehavior, ValueExpressionAST};
use fineprint_dsl::vocab::RulesVocabulary;

use crate::objects::{Characteristics, ManaValueContext, ObjectError, SplitHalf};
use crate::zones::ZoneKind;

/// How the object was cast or put in its current place.
///
/// Only consulted where a layout's selection depends on it: on the stack, and for a prototype
/// card also on the battlefield (CR 719.3b). Everywhere else it is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastChoice {
    /// Not cast with a choice (the default; also every object off the stack and battlefield).
    None,
    /// A split spell: the half cast (CR 709.3b).
    SplitHalf(SplitHalf),
    /// A fused split spell (CR 709.4d, 702.102b).
    Fused,
    /// An adventurer card cast as an Adventure (CR 716.3b).
    Adventure,
    /// An omen card cast as an Omen (CR 721.3b).
    Omen,
    /// A prototype card cast prototyped; also remembered for the permanent it becomes
    /// (CR 719.3b).
    Prototype,
    /// A nonmodal double-faced card cast "transformed" or "converted": back face up on the
    /// stack (CR 712.6b, 712.8a).
    Transformed,
    /// A modal double-faced spell: the face chosen when it is cast, consulted on the stack only
    /// (CR 712.8b).
    ///
    /// On the battlefield the selector reads [`LayoutStatus::face_up`] instead: a modal land
    /// played with its back face up (CR 712.9), or any permanent whose back face is up
    /// (CR 712.6e), is queried with `face_up = FaceUp::Back`.
    ModalFace(FaceUp),
}

/// Which face of a double-faced card is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceUp {
    /// The front face (the definition's `Main` face).
    Front,
    /// The back face (the definition's `Back` face).
    Back,
}

/// Status and copy facts the selection depends on.
///
/// `LayoutStatus::default()` is unflipped, front face up and not copying a back face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutStatus {
    /// Flipped status (CR 110.5, 710.2): consulted for a flip card on the battlefield only.
    pub flipped: bool,
    /// Which face of a double-faced permanent is up on the battlefield (CR 712.6c-712.6e).
    pub face_up: FaceUp,
    /// The object is a copy of the back face of a nonmodal double-faced object: as a spell on
    /// the stack or a permanent on the battlefield its mana value is 0 (CR 202.3b, 712.6d).
    /// Ignored in every other zone, where only the front face applies (CR 712.6a).
    pub copying_back_face: bool,
}

impl Default for LayoutStatus {
    fn default() -> LayoutStatus {
        LayoutStatus {
            flipped: false,
            face_up: FaceUp::Front,
            copying_back_face: false,
        }
    }
}

/// Where an object is, how it was cast and its status: the inputs of base-characteristic
/// selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutQuery {
    /// The zone the object is in.
    pub zone: ZoneKind,
    /// How it was cast or put there.
    pub cast: CastChoice,
    /// Its status and copy facts.
    pub status: LayoutStatus,
}

impl LayoutQuery {
    /// A query for `zone` with [`CastChoice::None`] and the default [`LayoutStatus`].
    #[must_use]
    pub fn new(zone: ZoneKind) -> LayoutQuery {
        LayoutQuery {
            zone,
            cast: CastChoice::None,
            status: LayoutStatus::default(),
        }
    }

    /// This query with `cast` as the cast choice.
    #[must_use]
    pub fn with_cast(self, cast: CastChoice) -> LayoutQuery {
        LayoutQuery { cast, ..self }
    }

    /// This query with `status` as the status.
    #[must_use]
    pub fn with_status(self, status: LayoutStatus) -> LayoutQuery {
        LayoutQuery { status, ..self }
    }
}

/// Why base characteristics, mana value or gating data could not be produced.
///
/// Checks run in this order and the first failure is returned: [`LayoutError::LayoutMismatch`],
/// [`LayoutError::BadFaces`], [`LayoutError::InvalidCast`], then [`LayoutError::Object`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// The layout behavior is not supported by this module.
    UnsupportedLayout(LayoutBehavior),
    /// A layout object was given a definition of another layout.
    LayoutMismatch {
        /// The layout of the object asked.
        expected: LayoutBehavior,
        /// The layout of the definition.
        found: LayoutBehavior,
    },
    /// The definition's faces are not exactly the face kinds the layout requires.
    BadFaces {
        /// The layout whose face set was violated.
        layout: LayoutBehavior,
    },
    /// The cast choice is not allowed for this layout in this zone.
    InvalidCast {
        /// The layout.
        layout: LayoutBehavior,
        /// The zone.
        zone: ZoneKind,
    },
    /// A selected face's type line was rejected ([`Characteristics::from_face`]).
    Object(ObjectError),
}

impl From<ObjectError> for LayoutError {
    fn from(e: ObjectError) -> LayoutError {
        LayoutError::Object(e)
    }
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::UnsupportedLayout(b) => write!(f, "layout {b} is not supported"),
            LayoutError::LayoutMismatch { expected, found } => {
                write!(f, "expected a {expected} definition, found {found}")
            }
            LayoutError::BadFaces { layout } => {
                write!(f, "the faces do not match the face set of layout {layout}")
            }
            LayoutError::InvalidCast { layout, zone } => {
                write!(
                    f,
                    "cast choice not allowed for layout {layout} in zone {zone:?}"
                )
            }
            LayoutError::Object(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LayoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LayoutError::Object(e) => Some(e),
            _ => None,
        }
    }
}

/// Gating data of a leveler, Saga, Class or Case card: data only, no counters, no
/// designations, no evaluation.
///
/// The DSL's ability AST is a placeholder, so level bands, chapter numbers, class level bars
/// and Case to-solve/solved abilities are not split out yet: each variant carries the Main
/// face's printed abilities in definition order (DECISIONS.md #22).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gating {
    /// A leveler card (CR 711): the uppermost printed power/toughness (CR 711.5, 711.6) and the
    /// printed abilities, level bands included (CR 711.2a, 711.2b).
    Leveler {
        /// The uppermost printed power.
        uppermost_power: Option<ValueExpressionAST>,
        /// The uppermost printed toughness.
        uppermost_toughness: Option<ValueExpressionAST>,
        /// The printed abilities in definition order.
        abilities: Vec<AbilityAST>,
    },
    /// A Saga card: the printed abilities, chapter abilities included (CR 715.2).
    Saga {
        /// The printed abilities in definition order.
        abilities: Vec<AbilityAST>,
    },
    /// A Class card: the printed abilities, class levels included (CR 717.2a).
    Class {
        /// The printed abilities in definition order.
        abilities: Vec<AbilityAST>,
    },
    /// A Case card: the printed abilities, to-solve and solved abilities included (CR 720.3b).
    Case {
        /// The printed abilities in definition order.
        abilities: Vec<AbilityAST>,
    },
}

/// One card layout: selects an object's base characteristics and mana value from its
/// definition (REQ-OBJ-002).
///
/// Alternative characteristics are part of the definition and are selected here as base
/// characteristics; nothing here is a layer effect.
pub trait Layout {
    /// The layout behavior this object handles.
    fn behavior(&self) -> LayoutBehavior;

    /// The base characteristics of an object with definition `def` for `query`, before any
    /// layer effect.
    ///
    /// # Errors
    /// [`LayoutError::LayoutMismatch`], [`LayoutError::BadFaces`],
    /// [`LayoutError::InvalidCast`] or [`LayoutError::Object`], in that order of precedence.
    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError>;

    /// The mana value of that object (CR 202.3), with `ctx` for variable symbols.
    ///
    /// `ctx` applies only when `query.zone` is the stack; in every other zone X, Y and Z count 0
    /// whatever `ctx` says (CR 202.3e, 107.3g).
    ///
    /// # Errors
    /// As [`Layout::base_characteristics`]: the same selection runs first.
    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError>;

    /// The gating data of `def`: `Some` for leveler, Saga, Class and Case layouts only.
    ///
    /// # Errors
    /// [`LayoutError::LayoutMismatch`] or [`LayoutError::BadFaces`].
    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError>;
}
