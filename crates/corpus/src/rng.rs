//! The corpus's own deterministic generator.
//!
//! The corpus must produce the same scenes on every machine and in every run, and it must not grow
//! a dependency tree to do it, so the randomness is SplitMix64: one `u64` of state, no tables,
//! the same sequence everywhere the arithmetic is IEEE-conformant.

/// SplitMix64 over one `u64` of state.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next number in the sequence.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D049BB133111EB);
        mixed ^ (mixed >> 31)
    }

    /// A value in `0..max`. `max` must not be zero.
    pub fn pick(&mut self, max: u32) -> u32 {
        (self.next_u64() % u64::from(max)) as u32
    }

    /// A value in `0.0..1.0`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Permutes `order` in place; the permutation depends only on the generator's state.
    pub fn shuffle(&mut self, order: &mut [usize]) {
        for index in (1..order.len()).rev() {
            let swap_with = self.pick(index as u32 + 1) as usize;
            order.swap(index, swap_with);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_depends_only_on_the_seed() {
        let mut left = Rng::new(42);
        let mut right = Rng::new(42);
        for _ in 0..64 {
            assert_eq!(left.next_u64(), right.next_u64());
        }
        assert_ne!(Rng::new(42).next_u64(), Rng::new(43).next_u64());
    }

    #[test]
    fn pick_stays_in_range() {
        let mut rng = Rng::new(7);
        for _ in 0..256 {
            let value = rng.pick(6);
            assert!(value < 6);
        }
    }

    #[test]
    fn unit_stays_in_range() {
        let mut rng = Rng::new(7);
        for _ in 0..256 {
            let value = rng.unit();
            assert!((0.0..1.0).contains(&value));
        }
    }

    #[test]
    fn shuffle_is_deterministic_and_a_permutation() {
        let mut left: Vec<usize> = (0..16).collect();
        let mut right = left.clone();
        Rng::new(9).shuffle(&mut left);
        Rng::new(9).shuffle(&mut right);
        assert_eq!(left, right);
        let mut sorted = left.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..16).collect::<Vec<_>>());
    }
}
