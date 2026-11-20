// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of the layer system (REQ-LAYER-001, REQ-CORE-002).

use std::fmt;

use fineprint_base::ids::{EffectId, IdExhausted, ObjectId, PlayerId};
use fineprint_dsl::ability::Layer;

/// Why a layer-system operation was refused. Every failing operation leaves its inputs
/// unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerError {
    /// An effect with this id is already registered.
    DuplicateEffect(EffectId),
    /// No effect with this id is registered.
    UnknownEffect(EffectId),
    /// The environment knows no base characteristics for this object.
    UnknownObject(ObjectId),
    /// The modification does not belong to the layer the effect declares.
    LayerMismatch {
        /// The declared layer.
        layer: Layer,
    },
    /// A feature this unit does not support (for example a `SameAsLayer` affected set).
    Unsupported(&'static str),
    /// A player of a simultaneous-timestamp batch is not in the APNAP list.
    UnknownPlayer(PlayerId),
    /// An object appears twice in a simultaneous-timestamp batch.
    DuplicateObject(ObjectId),
    /// The timestamp generator is exhausted (REQ-CORE-002).
    IdExhausted,
}

impl fmt::Display for LayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayerError::DuplicateEffect(id) => {
                write!(f, "effect {} is already registered", id.get())
            }
            LayerError::UnknownEffect(id) => write!(f, "effect {} is not registered", id.get()),
            LayerError::UnknownObject(id) => write!(f, "object {} is unknown", id.get()),
            LayerError::LayerMismatch { layer } => {
                write!(f, "the modification does not belong to layer {layer}")
            }
            LayerError::Unsupported(what) => write!(f, "unsupported: {what}"),
            LayerError::UnknownPlayer(id) => {
                write!(f, "player {} is not in the APNAP order", id.get())
            }
            LayerError::DuplicateObject(id) => {
                write!(f, "object {} appears twice in the batch", id.get())
            }
            LayerError::IdExhausted => f.write_str("timestamp space exhausted"),
        }
    }
}

impl std::error::Error for LayerError {}

impl From<IdExhausted> for LayerError {
    fn from(_: IdExhausted) -> LayerError {
        LayerError::IdExhausted
    }
}
