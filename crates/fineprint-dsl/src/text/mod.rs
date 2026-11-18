// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Rules text as a stream of tokens (REQ-DSL-012, SRS §14.5).
//!
//! A face's text is a [`TextTokenStream`]: literal text, rewritable words and name references,
//! each with an id unique within the stream. AST nodes whose meaning depends on a rewritable word
//! reference the token by id (`{"textToken":"t14"}`, see [`crate::expr::WordRef`]) so that a
//! text-changing effect (CR 612; the effect and its layer evaluation are other units) only
//! rewrites the token table. Self references are [`NameRefKind::SelfRef`] and are never
//! rewritten (CR 201.5).
//!
//! JSON (the FP-011 wire contract): an array of token objects
//! `{"id","kind":"LITERAL","text"}`, `{"id","kind":"WORD","category","value"}` and
//! `{"id","kind":"NAME_REF","ref":"SELF"}` / `{"id","kind":"NAME_REF","ref":"OTHER","name"}`.
//! Deserialisation runs the same validation as [`TextTokenStream::new`].

use std::collections::BTreeSet;
use std::fmt;

use serde::ser::{SerializeMap, Serializer};
use serde::{Serialize, ser};
use serde_json::Value;

use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, array_of, deserialize_via_from_json, free_str,
    is_noncharacter, str_of, wire_enum,
};

#[cfg(test)]
mod tests;

wire_enum! {
    /// The category of a rewritable word (REQ-DSL-012, SRS §14.5).
    RewritableCategory {
        /// `COLOR_WORD`.
        ColorWord => "COLOR_WORD",
        /// `BASIC_LAND_TYPE`.
        BasicLandType => "BASIC_LAND_TYPE",
        /// `CREATURE_TYPE`.
        CreatureType => "CREATURE_TYPE",
        /// `NUMBER` (the value is still a string).
        Number => "NUMBER",
        /// `CARD_NAME`.
        CardName => "CARD_NAME",
    }
}

wire_enum! {
    /// What a name reference names (REQ-DSL-012, SRS §14.5).
    NameRefKind {
        /// `SELF`: the object's own name ("this creature", "~"); never rewritten (CR 201.5).
        SelfRef => "SELF",
        /// `OTHER`: another name, carried in the token's `name`.
        Other => "OTHER",
    }
}

/// One token of a rules-text stream (REQ-DSL-012).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextToken {
    /// `{"id","kind":"LITERAL","text"}`: literal text, never rewritten.
    Literal {
        /// Token id (non-empty, unique in the stream).
        id: String,
        /// The text (non-empty).
        text: String,
    },
    /// `{"id","kind":"WORD","category","value"}`: a word text-changing effects may rewrite
    /// (CR 612).
    Word {
        /// Token id (non-empty, unique in the stream).
        id: String,
        /// Word category.
        category: RewritableCategory,
        /// The word (non-empty; a `NUMBER` value is a string too).
        value: String,
    },
    /// `{"id","kind":"NAME_REF","ref", "name"?}`: a reference to a name. `SELF` has no `name`;
    /// `OTHER` requires one.
    NameRef {
        /// Token id (non-empty, unique in the stream).
        id: String,
        /// Self or other.
        kind: NameRefKind,
        /// The referenced name: `None` for `SELF`, `Some` (non-empty) for `OTHER`.
        name: Option<String>,
    },
}

/// Why a token stream is invalid (REQ-DSL-012).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextError {
    /// The token at this index has an empty id.
    EmptyId {
        /// Position in the stream.
        index: usize,
    },
    /// Two tokens share this id.
    DuplicateId {
        /// The repeated id.
        id: String,
    },
    /// A `LITERAL` token has empty text.
    EmptyText {
        /// Token id.
        id: String,
    },
    /// A `WORD` token has an empty value.
    EmptyValue {
        /// Token id.
        id: String,
    },
    /// A `NAME_REF` `SELF` token carries a `name`.
    SelfWithName {
        /// Token id.
        id: String,
    },
    /// A `NAME_REF` `OTHER` token has no `name`, or an empty one.
    OtherWithoutName {
        /// Token id.
        id: String,
    },
    /// A string of the token with this id (or at this position, for an id) contains a Unicode
    /// noncharacter, which canonical JSON refuses.
    Noncharacter {
        /// Token id, or the position for the id itself.
        id: String,
    },
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextError::EmptyId { index } => write!(f, "text token {index} has an empty id"),
            TextError::DuplicateId { id } => write!(f, "text token id {id:?} is used twice"),
            TextError::EmptyText { id } => write!(f, "LITERAL token {id:?} has empty text"),
            TextError::EmptyValue { id } => write!(f, "WORD token {id:?} has an empty value"),
            TextError::SelfWithName { id } => {
                write!(f, "NAME_REF SELF token {id:?} must not carry a name")
            }
            TextError::OtherWithoutName { id } => {
                write!(f, "NAME_REF OTHER token {id:?} needs a non-empty name")
            }
            TextError::Noncharacter { id } => {
                write!(f, "text token {id:?} contains a Unicode noncharacter")
            }
        }
    }
}

impl std::error::Error for TextError {}

impl TextToken {
    /// The token id (REQ-DSL-012).
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            TextToken::Literal { id, .. }
            | TextToken::Word { id, .. }
            | TextToken::NameRef { id, .. } => id,
        }
    }

    /// Whether a text-changing effect may rewrite this token (REQ-DSL-012, CR 612): true only
    /// for [`TextToken::Word`]. Literal text and name references (including `SELF`, CR 201.5)
    /// are never rewritten.
    #[must_use]
    pub fn is_rewritable(&self) -> bool {
        matches!(self, TextToken::Word { .. })
    }

    /// Checks this token on its own (everything but id uniqueness); `index` is its position.
    fn validate(&self, index: usize) -> Result<(), TextError> {
        let id = self.id();
        if id.is_empty() {
            return Err(TextError::EmptyId { index });
        }
        let nonchar = |s: &str| s.chars().any(is_noncharacter);
        if nonchar(id) {
            return Err(TextError::Noncharacter {
                id: index.to_string(),
            });
        }
        let bad = |s: &str| nonchar(s).then(|| TextError::Noncharacter { id: id.to_owned() });
        match self {
            TextToken::Literal { text, .. } => {
                if text.is_empty() {
                    return Err(TextError::EmptyText { id: id.to_owned() });
                }
                bad(text).map_or(Ok(()), Err)
            }
            TextToken::Word { value, .. } => {
                if value.is_empty() {
                    return Err(TextError::EmptyValue { id: id.to_owned() });
                }
                bad(value).map_or(Ok(()), Err)
            }
            TextToken::NameRef { kind, name, .. } => match (kind, name) {
                (NameRefKind::SelfRef, None) => Ok(()),
                (NameRefKind::SelfRef, Some(_)) => {
                    Err(TextError::SelfWithName { id: id.to_owned() })
                }
                (NameRefKind::Other, Some(n)) if !n.is_empty() => bad(n).map_or(Ok(()), Err),
                (NameRefKind::Other, _) => Err(TextError::OtherWithoutName { id: id.to_owned() }),
            },
        }
    }
}

impl FromJson for TextToken {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "text token")?;
        let id = str_of(o.req("id")?, "id")?.to_owned();
        let token = match str_of(o.req("kind")?, "kind")? {
            "LITERAL" => TextToken::Literal {
                id,
                text: free_str(o.req("text")?, "text")?,
            },
            "WORD" => TextToken::Word {
                id,
                category: RewritableCategory::from_json(o.req("category")?)?,
                value: free_str(o.req("value")?, "value")?,
            },
            "NAME_REF" => {
                let kind = NameRefKind::from_json(o.req("ref")?)?;
                let name = match kind {
                    // A SELF reference with `name` is left for `finish` to reject.
                    NameRefKind::SelfRef => None,
                    NameRefKind::Other => Some(free_str(o.req("name")?, "name")?),
                };
                TextToken::NameRef { id, kind, name }
            }
            other => return Err(format!("unknown text token kind {other:?}")),
        };
        o.finish()?;
        token.validate(0).map_err(|e| e.to_string())?;
        Ok(token)
    }
}

impl Members for TextToken {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        self.validate(0).map_err(ser::Error::custom)?;
        m.serialize_entry("id", self.id())?;
        match self {
            TextToken::Literal { text, .. } => {
                m.serialize_entry("kind", "LITERAL")?;
                m.serialize_entry("text", text)
            }
            TextToken::Word {
                category, value, ..
            } => {
                m.serialize_entry("kind", "WORD")?;
                m.serialize_entry("category", category)?;
                m.serialize_entry("value", value)
            }
            TextToken::NameRef { kind, name, .. } => {
                m.serialize_entry("kind", "NAME_REF")?;
                m.serialize_entry("ref", kind)?;
                match name {
                    Some(n) => m.serialize_entry("name", n),
                    None => Ok(()),
                }
            }
        }
    }
}

impl Serialize for TextToken {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// A validated stream of [`TextToken`]s in text order with unique ids (REQ-DSL-012). The empty
/// stream is valid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextTokenStream {
    /// The tokens, in order.
    tokens: Vec<TextToken>,
}

impl TextTokenStream {
    /// Validates and wraps `tokens` (REQ-DSL-012).
    ///
    /// # Errors
    /// [`TextError`] for an empty id, a repeated id, empty literal text or word value, a `SELF`
    /// name reference with a name, an `OTHER` one without, or a noncharacter.
    pub fn new(tokens: Vec<TextToken>) -> Result<TextTokenStream, TextError> {
        let mut seen = BTreeSet::new();
        for (index, token) in tokens.iter().enumerate() {
            token.validate(index)?;
            if !seen.insert(token.id()) {
                return Err(TextError::DuplicateId {
                    id: token.id().to_owned(),
                });
            }
        }
        Ok(TextTokenStream { tokens })
    }

    /// The tokens in text order (REQ-DSL-012).
    #[must_use]
    pub fn tokens(&self) -> &[TextToken] {
        &self.tokens
    }

    /// The token with id `id`, if any (REQ-DSL-012).
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&TextToken> {
        self.tokens.iter().find(|t| t.id() == id)
    }

    /// The number of tokens (REQ-DSL-012).
    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Whether the stream has no tokens (REQ-DSL-012).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

impl FromJson for TextTokenStream {
    fn from_json(v: &Value) -> JResult<Self> {
        let tokens = array_of(v, "text token stream")?
            .iter()
            .map(TextToken::from_json)
            .collect::<JResult<Vec<_>>>()?;
        TextTokenStream::new(tokens).map_err(|e| e.to_string())
    }
}

impl Serialize for TextTokenStream {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.tokens.serialize(s)
    }
}

deserialize_via_from_json!(TextToken, TextTokenStream);
