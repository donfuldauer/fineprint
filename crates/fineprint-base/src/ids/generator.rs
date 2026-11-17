// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Monotonic identifier generators held in the game state (REQ-CORE-002).

use core::fmt;
use core::marker::PhantomData;

use super::types::{
    AbilityInstanceId, CardId, EffectId, Id, LinkId, ObjectId, PlayerId, Timestamp,
};

/// The generator has already issued `u64::MAX`; no further identifier exists (REQ-CORE-002).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdExhausted;

impl fmt::Display for IdExhausted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("identifier space exhausted")
    }
}

impl std::error::Error for IdExhausted {}

/// A monotonic generator of identifiers of type `I` (REQ-CORE-002).
///
/// It stores the last issued raw value (0 means none yet; 0 is never issued). Issued values are
/// strictly increasing and never repeat; exhaustion is reported as [`IdExhausted`], never a wrap
/// or a panic. A clone replays the same sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdGenerator<I: Id> {
    last: u64,
    kind: PhantomData<I>,
}

impl<I: Id> IdGenerator<I> {
    /// A fresh generator; the first issued identifier has raw value 1.
    #[must_use]
    pub fn new() -> Self {
        Self::from_last_issued(0)
    }

    /// Restores a generator from saved state: the next issued raw value is `last + 1`.
    #[must_use]
    pub fn from_last_issued(last: u64) -> Self {
        IdGenerator {
            last,
            kind: PhantomData,
        }
    }

    /// The last issued raw value (0 when nothing has been issued).
    #[must_use]
    pub fn last_issued(&self) -> u64 {
        self.last
    }

    /// Issues the next identifier (`last + 1`) and records it.
    ///
    /// # Errors
    /// [`IdExhausted`] when `u64::MAX` has already been issued; the generator is then unchanged.
    pub fn issue(&mut self) -> Result<I, IdExhausted> {
        let next = self.last.checked_add(1).ok_or(IdExhausted)?;
        self.last = next;
        Ok(I::from_raw(next))
    }
}

impl<I: Id> Default for IdGenerator<I> {
    /// Same as [`IdGenerator::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// One independent generator per identifier kind, held in the game state (REQ-CORE-002).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdGenerators {
    /// Issues [`CardId`]s.
    pub card: IdGenerator<CardId>,
    /// Issues [`ObjectId`]s.
    pub object: IdGenerator<ObjectId>,
    /// Issues [`AbilityInstanceId`]s.
    pub ability_instance: IdGenerator<AbilityInstanceId>,
    /// Issues [`EffectId`]s.
    pub effect: IdGenerator<EffectId>,
    /// Issues [`Timestamp`]s (the single monotonic timestamp counter).
    pub timestamp: IdGenerator<Timestamp>,
    /// Issues [`LinkId`]s.
    pub link: IdGenerator<LinkId>,
    /// Issues [`PlayerId`]s.
    pub player: IdGenerator<PlayerId>,
}

impl IdGenerators {
    /// All generators fresh (each first issues raw value 1).
    #[must_use]
    pub fn new() -> IdGenerators {
        IdGenerators::default()
    }
}

#[cfg(test)]
mod tests {
    // REQ-CORE-002: monotonic generators.
    use super::*;
    use crate::num::testkit::Rng;

    // REQ-CORE-002
    #[test]
    fn first_value_is_one_and_strictly_increasing() {
        let mut g: IdGenerator<ObjectId> = IdGenerator::new();
        assert_eq!(g.last_issued(), 0);
        assert_eq!(g, IdGenerator::default());
        let mut prev = g.issue().unwrap();
        assert_eq!(prev, ObjectId::new(1));
        for _ in 0..5_000 {
            let next = g.issue().unwrap();
            assert!(next > prev);
            assert_eq!(next.get(), prev.get() + 1);
            prev = next;
        }
        assert_eq!(g.last_issued(), 5_001);
    }

    // REQ-CORE-002
    #[test]
    fn restore_and_exhaustion() {
        let mut rng = Rng::new(0x6E4_0006);
        for _ in 0..5_000 {
            let last = rng.next_u64() >> (rng.next_u64() % 64);
            let mut g: IdGenerator<Timestamp> = IdGenerator::from_last_issued(last);
            assert_eq!(g.last_issued(), last);
            match last.checked_add(1) {
                Some(n) => {
                    assert_eq!(g.issue(), Ok(Timestamp::new(n)));
                    assert_eq!(g.last_issued(), n);
                }
                None => assert_eq!(g.issue(), Err(IdExhausted)),
            }
        }
        let mut g: IdGenerator<EffectId> = IdGenerator::from_last_issued(u64::MAX - 1);
        assert_eq!(g.issue(), Ok(EffectId::new(u64::MAX)));
        let before = g.clone();
        assert_eq!(g.issue(), Err(IdExhausted));
        assert_eq!(g.issue(), Err(IdExhausted));
        assert_eq!(g, before);
        assert_eq!(g.last_issued(), u64::MAX);
    }

    // REQ-CORE-002
    #[test]
    fn exhausted_display_and_error() {
        assert_eq!(IdExhausted.to_string(), "identifier space exhausted");
        let e: &dyn std::error::Error = &IdExhausted;
        assert!(e.source().is_none());
        assert_eq!(format!("{IdExhausted:?}"), "IdExhausted");
    }

    // REQ-CORE-002: generators are independent and a clone replays the same sequence.
    #[test]
    fn generators_independent_and_clone_replays() {
        let mut gens = IdGenerators::new();
        assert_eq!(gens, IdGenerators::default());
        let mut rng = Rng::new(0x6E4_0007);
        let mut counts = [0_u64; 7];
        for _ in 0..5_000 {
            let k = usize::try_from(rng.next_u64() % 7).unwrap();
            let before = gens.clone();
            let raw = match k {
                0 => gens.card.issue().unwrap().get(),
                1 => gens.object.issue().unwrap().get(),
                2 => gens.ability_instance.issue().unwrap().get(),
                3 => gens.effect.issue().unwrap().get(),
                4 => gens.timestamp.issue().unwrap().get(),
                5 => gens.link.issue().unwrap().get(),
                _ => gens.player.issue().unwrap().get(),
            };
            counts[k] += 1;
            assert_eq!(raw, counts[k]);
            let lasts = |g: &IdGenerators| {
                [
                    g.card.last_issued(),
                    g.object.last_issued(),
                    g.ability_instance.last_issued(),
                    g.effect.last_issued(),
                    g.timestamp.last_issued(),
                    g.link.last_issued(),
                    g.player.last_issued(),
                ]
            };
            let (b, a) = (lasts(&before), lasts(&gens));
            for i in 0..7 {
                if i == k {
                    assert_eq!(a[i], b[i] + 1);
                } else {
                    assert_eq!(a[i], b[i]);
                }
            }
        }
        let mut c1 = gens.clone();
        let mut c2 = gens.clone();
        for _ in 0..100 {
            assert_eq!(c1.timestamp.issue(), c2.timestamp.issue());
            assert_eq!(c1.object.issue(), c2.object.issue());
        }
        assert_eq!(c1, c2);
        assert!(format!("{c1:?}").contains("timestamp"));
    }
}
