//! A tiny deterministic random number generator for the soak and chaos runs.
//!
//! SplitMix64: no state to seed beyond one number, no external crate, and the same sequence on
//! every platform, which is what makes a seeded run comparable between machines and between
//! runs of the same pull request. The modulo in [`SplitMix64::below`] has a slight bias; for
//! scene and fault planning that is far below anything that matters.

#[derive(Debug, Clone, Copy)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A whole number below `limit`, uniform enough for scene and fault planning.
    pub fn below(&mut self, limit: u64) -> u64 {
        if limit <= 1 {
            return 0;
        }
        self.next_u64() % limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_sequence() {
        let mut a = SplitMix64::new(7);
        let mut b = SplitMix64::new(7);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_give_different_sequences() {
        let mut a = SplitMix64::new(7);
        let mut b = SplitMix64::new(8);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn below_never_reaches_the_limit() {
        let mut rng = SplitMix64::new(1);
        for _ in 0..1000 {
            assert!(rng.below(10) < 10);
        }
        assert_eq!(rng.below(1), 0);
        assert_eq!(rng.below(0), 0);
    }
}
