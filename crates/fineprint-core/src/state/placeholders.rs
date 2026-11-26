// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The placeholder for the history ledger (REQ-CORE-001). A later unit replaces its contents; it
//! encodes canonically as an empty collection now.

use fineprint_base::encode::{CanonicalEncode, put_len};

/// The ledger of game history the rules look back on (REQ-CORE-001). Placeholder: always
/// empty until the history unit fills it in.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct HistoryLedger {}

impl HistoryLedger {
    /// `true` when the ledger holds no entry (always, for now).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        true
    }
}

impl CanonicalEncode for HistoryLedger {
    /// An empty collection: [`put_len`] of 0 (4 zero bytes).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_len(out, 0);
    }
}
