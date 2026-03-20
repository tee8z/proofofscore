use crate::fixed::Fixed;

/// Deterministic xorshift64 PRNG. Same seed always produces same sequence.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Ensure state is never zero (xorshift produces only zeros from zero state)
        let state = if seed == 0 { 1 } else { seed };
        Rng { state }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Returns a Fixed in range [0, 1).
    pub fn next_fixed(&mut self) -> Fixed {
        // Use top 16 bits of u32 to get a value in [0, 65536) then interpret as fractional
        let val = self.next_u32();
        // raw fixed value in [0, 65535] representing [0, ~1.0)
        Fixed((val & 0xFFFF) as i64)
    }

    /// Returns a Fixed in range [min, max).
    pub fn next_range(&mut self, min: Fixed, max: Fixed) -> Fixed {
        let t = self.next_fixed();
        min + (max - min) * t
    }

    /// Returns an integer in range [min, max).
    pub fn next_int_range(&mut self, min: i32, max: i32) -> i32 {
        if min >= max {
            return min;
        }
        let range = (max - min) as u32;
        let val = self.next_u32() % range;
        min + val as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic() {
        let mut rng1 = Rng::new(12345);
        let mut rng2 = Rng::new(12345);
        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_different_seeds() {
        let mut rng1 = Rng::new(12345);
        let mut rng2 = Rng::new(54321);
        // Very unlikely to be equal
        let a = rng1.next_u64();
        let b = rng2.next_u64();
        assert_ne!(a, b);
    }

    #[test]
    fn test_next_fixed_range() {
        let mut rng = Rng::new(42);
        for _ in 0..1000 {
            let v = rng.next_fixed();
            assert!(v.0 >= 0, "next_fixed should be >= 0");
            assert!(v.0 < Fixed::ONE.0, "next_fixed should be < 1.0");
        }
    }

    #[test]
    fn test_next_range() {
        let mut rng = Rng::new(99);
        let min = Fixed::from(10);
        let max = Fixed::from(20);
        for _ in 0..1000 {
            let v = rng.next_range(min, max);
            assert!(v.0 >= min.0, "value should be >= min");
            assert!(v.0 < max.0, "value should be < max");
        }
    }

    #[test]
    fn test_next_int_range() {
        let mut rng = Rng::new(77);
        for _ in 0..1000 {
            let v = rng.next_int_range(5, 10);
            assert!(v >= 5);
            assert!(v < 10);
        }
    }

    #[test]
    fn test_zero_seed_handled() {
        let mut rng = Rng::new(0);
        // Should not get stuck at zero
        let v = rng.next_u64();
        assert_ne!(v, 0);
    }
}
