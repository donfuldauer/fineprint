// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `dslVersion` parsing and the major-version gate (REQ-DSL-001, SRS §14.1 item 5).

use std::fmt;

/// The DSL version this engine writes.
pub const DSL_VERSION: &str = "1.0";

/// The only DSL major version this engine accepts.
pub const DSL_MAJOR: u64 = 1;

/// A parsed `dslVersion` (semantic version without pre-release or build suffix).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DslVersion {
    /// Major version: a different major is rejected.
    pub major: u64,
    /// Minor version (any value accepted).
    pub minor: u64,
    /// Patch version (0 when the text has only `MAJOR.MINOR`).
    pub patch: u64,
}

impl DslVersion {
    /// Parses `MAJOR.MINOR` or `MAJOR.MINOR.PATCH`: ASCII decimal digits only, no leading zeros
    /// except a single `0`, no sign, no whitespace, no suffix, each component within `u64`.
    ///
    /// # Errors
    /// [`DslVersionError::Malformed`] carrying the whole input text.
    pub fn parse(text: &str) -> Result<DslVersion, DslVersionError> {
        let malformed = || DslVersionError::Malformed(text.to_owned());
        let mut parts = text.split('.');
        let major = parts.next().and_then(component).ok_or_else(malformed)?;
        let minor = parts.next().and_then(component).ok_or_else(malformed)?;
        let patch = match parts.next() {
            None => 0,
            Some(p) => component(p).ok_or_else(malformed)?,
        };
        if parts.next().is_some() {
            return Err(malformed());
        }
        Ok(DslVersion {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for DslVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// One version component: canonical non-negative decimal fitting `u64`.
fn component(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if bytes.len() > 1 && bytes.first() == Some(&b'0') {
        return None;
    }
    bytes.iter().try_fold(0u64, |acc, d| {
        acc.checked_mul(10)?.checked_add(u64::from(d - b'0'))
    })
}

/// Why a `dslVersion` was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DslVersionError {
    /// The text is not `MAJOR.MINOR[.PATCH]` (the offending text is carried).
    Malformed(String),
    /// The major version differs from the engine's ([`DSL_MAJOR`]).
    MajorMismatch {
        /// Major version found in the definition.
        found: u64,
        /// Major version this engine supports.
        supported: u64,
    },
}

impl fmt::Display for DslVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DslVersionError::Malformed(text) => write!(
                f,
                "malformed dslVersion {text:?} (expected MAJOR.MINOR or MAJOR.MINOR.PATCH)"
            ),
            DslVersionError::MajorMismatch { found, supported } => write!(
                f,
                "unsupported dslVersion major {found} (this engine supports major {supported})"
            ),
        }
    }
}

impl std::error::Error for DslVersionError {}

/// Parses `text` and accepts it only if its major version is [`DSL_MAJOR`] (SRS §14.1 item 5:
/// definitions whose major version does not match the engine's are rejected). Any minor and
/// patch are accepted.
///
/// # Errors
/// [`DslVersionError::Malformed`] or [`DslVersionError::MajorMismatch`].
pub fn check_dsl_version(text: &str) -> Result<DslVersion, DslVersionError> {
    let version = DslVersion::parse(text)?;
    if version.major == DSL_MAJOR {
        Ok(version)
    } else {
        Err(DslVersionError::MajorMismatch {
            found: version.major,
            supported: DSL_MAJOR,
        })
    }
}
