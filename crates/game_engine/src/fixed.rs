use core::fmt;
use core::ops::{Add, Div, Mul, Neg, Sub};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Fixed-point number in Q48.16 format (i64 with 16 fractional bits).
/// Range: approximately -140 trillion to +140 trillion.
/// Precision: 1/65536 ≈ 0.0000153.
///
/// Serializes as f64 in JSON so configs use normal numbers (800, 0.1, etc.)
/// instead of raw fixed-point values.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fixed(pub i64);

impl Serialize for Fixed {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0 as f64 / SCALE as f64)
    }
}

impl<'de> Deserialize<'de> for Fixed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = f64::deserialize(deserializer)?;
        Ok(Fixed((val * SCALE as f64) as i64))
    }
}

const FRAC_BITS: i64 = 16;
const SCALE: i64 = 1 << FRAC_BITS; // 65536

impl Fixed {
    pub const ZERO: Fixed = Fixed(0);
    pub const ONE: Fixed = Fixed(SCALE);
    pub const HALF: Fixed = Fixed(SCALE / 2);

    /// Create a Fixed from a ratio num/den.
    /// Example: `Fixed::from_ratio(1, 10)` ≈ 0.1
    pub fn from_ratio(num: i32, den: i32) -> Self {
        Fixed((num as i64 * SCALE) / den as i64)
    }

    /// Convert to f32 for output at the WASM boundary.
    pub fn to_f32(self) -> f32 {
        self.0 as f32 / SCALE as f32
    }

    /// Absolute value.
    pub fn abs(self) -> Self {
        if self.0 < 0 {
            Fixed(-self.0)
        } else {
            self
        }
    }

    /// Fixed-point square root using Newton's method.
    pub fn sqrt(self) -> Self {
        if self.0 <= 0 {
            return Fixed::ZERO;
        }
        // We need sqrt of the fixed-point value. If the raw value is V,
        // the fixed-point number represents V / 2^16.
        // sqrt(V / 2^16) = sqrt(V) / 2^8
        // But we want the result in fixed-point: result * 2^16 = sqrt(V) * 2^8
        // So: raw_result = sqrt(V * 65536)
        let val = self.0 as u128 * SCALE as u128;
        let mut guess = (val as f64).sqrt() as u128;
        // Newton's method refinement
        for _ in 0..6 {
            if guess == 0 {
                break;
            }
            guess = (guess + val / guess) / 2;
        }
        Fixed(guess as i64)
    }

    /// Sine using lookup table. Angle is in the 0-256 range where 256 = full circle.
    pub fn sin(self) -> Fixed {
        lookup_sin(self)
    }

    /// Cosine using lookup table. Angle is in the 0-256 range where 256 = full circle.
    pub fn cos(self) -> Fixed {
        // cos(a) = sin(a + 64) since 64 = quarter circle in 256-unit system
        lookup_sin(self + Fixed::from(64))
    }
}

impl From<i32> for Fixed {
    fn from(v: i32) -> Self {
        Fixed(v as i64 * SCALE)
    }
}

impl From<f64> for Fixed {
    fn from(v: f64) -> Self {
        Fixed((v * SCALE as f64) as i64)
    }
}

impl Add for Fixed {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Fixed(self.0 + rhs.0)
    }
}

impl Sub for Fixed {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Fixed(self.0 - rhs.0)
    }
}

impl Mul for Fixed {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        // Use i128 to avoid overflow during multiplication
        Fixed(((self.0 as i128 * rhs.0 as i128) >> FRAC_BITS) as i64)
    }
}

impl Div for Fixed {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        // Use i128 to avoid overflow during shift
        Fixed((((self.0 as i128) << FRAC_BITS) / rhs.0 as i128) as i64)
    }
}

impl Neg for Fixed {
    type Output = Self;
    fn neg(self) -> Self {
        Fixed(-self.0)
    }
}

impl fmt::Debug for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fixed({})", self.to_f32())
    }
}

impl fmt::Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.4}", self.to_f32())
    }
}

// ---- Sin/Cos lookup table (256 entries for a full circle) ----

/// Precomputed sin values for angles 0..255 in 256-unit circle.
/// sin_table[i] = sin(i * 2*PI / 256) as Fixed raw value.
const SIN_TABLE: [i64; 256] = generate_sin_table();

#[allow(clippy::approx_constant)]
const fn generate_sin_table() -> [i64; 256] {
    let mut table = [0i64; 256];
    let mut i = 0u32;
    while i < 256 {
        let angle = i as f64 * 6.283185307179586 / 256.0;
        let sin_val = const_sin(angle);
        table[i as usize] = (sin_val * 65536.0) as i64;
        i += 1;
    }
    table
}

/// Compile-time sin approximation using Taylor series (enough terms for good precision).
#[allow(clippy::approx_constant)]
const fn const_sin(x: f64) -> f64 {
    let pi = 3.141592653589793;
    let two_pi = 6.283185307179586;
    let mut x = x;
    // Reduce to [0, 2*PI)
    while x < 0.0 {
        x += two_pi;
    }
    while x >= two_pi {
        x -= two_pi;
    }
    // Reduce to [-PI, PI]
    if x > pi {
        x -= two_pi;
    }

    // Taylor series: sin(x) = x - x^3/3! + x^5/5! - x^7/7! + x^9/9! - x^11/11!
    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    let x9 = x7 * x2;
    let x11 = x9 * x2;
    let x13 = x11 * x2;

    x - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0 + x9 / 362880.0 - x11 / 39916800.0 + x13 / 6227020800.0
}

/// Look up sin with linear interpolation between table entries.
/// Angle is in fixed-point 0-256 range.
fn lookup_sin(angle: Fixed) -> Fixed {
    // Normalize angle to [0, 256) in raw fixed-point units
    // 256 in fixed = 256 * 65536 = 16777216
    let full_circle_raw = 256i64 * SCALE;
    let mut raw = angle.0 % full_circle_raw;
    if raw < 0 {
        raw += full_circle_raw;
    }

    // The integer part (in 256-unit angle) is raw >> 16
    let index = ((raw >> FRAC_BITS) & 255) as usize;
    let frac = raw & (SCALE - 1); // fractional part (0..65535)

    let next_index = (index + 1) & 255;

    let a = SIN_TABLE[index] as i128;
    let b = SIN_TABLE[next_index] as i128;

    // Linear interpolation: a + (b - a) * frac / 65536
    let result = a + ((b - a) * frac as i128) / SCALE as i128;
    Fixed(result as i64)
}

/// Integer square root (for level scaling).
pub fn isqrt(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_i32() {
        assert_eq!(Fixed::from(0), Fixed::ZERO);
        assert_eq!(Fixed::from(1), Fixed::ONE);
        assert_eq!(Fixed::from(5).0, 5 * 65536);
    }

    #[test]
    fn test_from_f64() {
        let half = Fixed::from(0.5);
        assert_eq!(half, Fixed::HALF);
    }

    #[test]
    fn test_add() {
        assert_eq!(Fixed::ONE + Fixed::ONE, Fixed::from(2));
        assert_eq!(Fixed::HALF + Fixed::HALF, Fixed::ONE);
        // Large values that would overflow i32
        let big = Fixed::from(800) + Fixed::from(600);
        assert_eq!(big, Fixed::from(1400));
    }

    #[test]
    fn test_sub() {
        assert_eq!(Fixed::ONE - Fixed::HALF, Fixed::HALF);
        assert_eq!(Fixed::ZERO - Fixed::ONE, Fixed::from(-1));
    }

    #[test]
    fn test_mul() {
        assert_eq!(Fixed::from(3) * Fixed::from(4), Fixed::from(12));
        assert_eq!(Fixed::HALF * Fixed::from(6), Fixed::from(3));
        // Large multiplication that would overflow i32
        let result = Fixed::from(800) * Fixed::from(600);
        assert_eq!(result, Fixed::from(480000));
        // 0.25 * 0.25 = 0.0625
        let quarter = Fixed::from_ratio(1, 4);
        let result = quarter * quarter;
        let expected = Fixed::from_ratio(1, 16);
        assert!((result.0 - expected.0).abs() <= 1);
    }

    #[test]
    fn test_div() {
        assert_eq!(Fixed::from(12) / Fixed::from(4), Fixed::from(3));
        let result = Fixed::ONE / Fixed::from(3);
        let expected = Fixed::from_ratio(1, 3);
        assert!((result.0 - expected.0).abs() <= 1);
    }

    #[test]
    fn test_neg() {
        assert_eq!(-Fixed::ONE, Fixed::from(-1));
        assert_eq!(-Fixed::ZERO, Fixed::ZERO);
    }

    #[test]
    fn test_from_ratio() {
        let tenth = Fixed::from_ratio(1, 10);
        // 0.1 * 65536 = 6553.6, so raw should be 6553 or 6554
        assert!((tenth.0 - 6553).abs() <= 1);
    }

    #[test]
    fn test_sin_cos_known_values() {
        // sin(0) = 0
        let s = Fixed::ZERO.sin();
        assert!(s.0.abs() <= 2, "sin(0) should be ~0, got {}", s.to_f32());

        // sin(64) = sin(quarter circle) = 1.0
        let s = Fixed::from(64).sin();
        assert!(
            (s.0 - Fixed::ONE.0).abs() <= 4,
            "sin(64) should be ~1.0, got {}",
            s.to_f32()
        );

        // cos(0) = 1.0
        let c = Fixed::ZERO.cos();
        assert!(
            (c.0 - Fixed::ONE.0).abs() <= 4,
            "cos(0) should be ~1.0, got {}",
            c.to_f32()
        );

        // sin(128) = sin(half circle) = 0
        let s = Fixed::from(128).sin();
        assert!(s.0.abs() <= 4, "sin(128) should be ~0, got {}", s.to_f32());

        // cos(64) = cos(quarter circle) = 0
        let c = Fixed::from(64).cos();
        assert!(c.0.abs() <= 4, "cos(64) should be ~0, got {}", c.to_f32());
    }

    #[test]
    fn test_sqrt() {
        let four = Fixed::from(4);
        let result = four.sqrt();
        assert!(
            (result.0 - Fixed::from(2).0).abs() <= 2,
            "sqrt(4) should be ~2.0, got {}",
            result.to_f32()
        );

        let one = Fixed::ONE;
        let result = one.sqrt();
        assert!(
            (result.0 - Fixed::ONE.0).abs() <= 2,
            "sqrt(1) should be ~1.0, got {}",
            result.to_f32()
        );
    }

    #[test]
    fn test_abs() {
        assert_eq!(Fixed::from(-5).abs(), Fixed::from(5));
        assert_eq!(Fixed::from(5).abs(), Fixed::from(5));
        assert_eq!(Fixed::ZERO.abs(), Fixed::ZERO);
    }

    #[test]
    fn test_isqrt() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(10), 3);
        assert_eq!(isqrt(16), 4);
    }

    #[test]
    fn test_to_f32() {
        assert!((Fixed::ONE.to_f32() - 1.0).abs() < 0.001);
        assert!((Fixed::HALF.to_f32() - 0.5).abs() < 0.001);
        assert!((Fixed::from_ratio(1, 3).to_f32() - 0.333333).abs() < 0.001);
    }

    #[test]
    fn test_distance_squared_no_overflow() {
        // This was the original crash: dx=400, dy=300 → dx*dx + dy*dy
        let dx = Fixed::from(400);
        let dy = Fixed::from(300);
        let dist_sq = dx * dx + dy * dy;
        // 400*400 + 300*300 = 160000 + 90000 = 250000
        assert_eq!(dist_sq, Fixed::from(250000));
    }
}
