//! A small deterministic generator (SplitMix64), so that the engine needs no
//! dependency for randomness and a seeded configuration replays the same
//! moves — SEI §8.4 asks a `fresh` search with fixed options to be
//! reproducible.

use std::time::{SystemTime, UNIX_EPOCH};

/// SplitMix64: 64 bits of state, one multiplication and three xor-shifts per
/// output. Enough to pick a move.
pub struct SplitMix64(u64);

impl SplitMix64 {
    /// A generator seeded from the `seed` option: `0` draws from the clock and
    /// the process id, any other value is replayed exactly.
    pub fn seeded(seed: u32) -> Self {
        if seed == 0 {
            Self::from_entropy()
        } else {
            Self(u64::from(seed))
        }
    }

    /// A generator seeded from the clock and the process id.
    pub fn from_entropy() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mixed = (nanos as u64) ^ (u64::from(std::process::id()).rotate_left(32));
        Self(mixed | 1)
    }

    /// The next 64 bits.
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform index in `0..n`; `n` must be positive.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        // The modulo bias is below 2^-40 for any n that fits a board game.
        let index = self.next_u64().checked_rem(n as u64).unwrap_or(0);
        usize::try_from(index).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::SplitMix64;

    #[test]
    fn a_seed_replays() {
        let mut a = SplitMix64::seeded(7);
        let mut b = SplitMix64::seeded(7);
        for _ in 0..100 {
            assert_eq!(a.below(1000), b.below(1000));
        }
    }

    #[test]
    fn below_stays_in_range() {
        let mut g = SplitMix64::seeded(3);
        for _ in 0..1000 {
            assert!(g.below(22) < 22);
        }
        assert_eq!(g.below(0), 0);
    }
}
