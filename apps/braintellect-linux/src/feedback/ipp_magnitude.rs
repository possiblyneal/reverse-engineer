//! The `float32` complex magnitude the original links from IPP (`0x66ff00`, SIMD kernel
//! `0x6b8650`; `sensor-status.md`, "Level bit exactness"): not a square root but the
//! hardware reciprocal-square-root estimate (`rsqrtps`) refined by one Newton step, so a
//! magnitude is within a few ulp of the correctly rounded one and the original's values
//! are reproduced only by modelling the estimate.
//!
//! The estimate is Intel's: its result depends only on the exponent's parity and the top
//! 11 mantissa bits, and `rsqrt_table.txt` holds its 12-bit result for each of those 4096
//! keys, read from an Intel Core i3-10100T. The original's captured magnitudes agree with
//! it to the bit; other vendors' `rsqrtps` differ.

use super::ipp_fft::Complex;

/// The estimate's 12 result mantissa bits per key (`parity << 11 | top mantissa bits`),
/// one hex entry per line, for inputs in `[1, 2)` and `[2, 4)`.
const TABLE: &str = include_str!("rsqrt_table.txt");
const ENTRY_LEN: usize = 4;
const MANTISSA_BITS: u32 = 23;
const KEY_BITS: u32 = 11;
/// The biased exponent of results from the table's inputs, which lie in `(0.5, 1]`.
const RESULT_EXPONENT: i32 = 126;
const EXPONENT_BIAS: i32 = 127;

/// `rsqrtps` of a positive number: a subnormal reads as zero, so its estimate is infinite
/// (observed on the table's i3-10100T).
fn rsqrt_estimate(x: f32) -> f32 {
    if x < f32::MIN_POSITIVE {
        return f32::INFINITY;
    }
    let bits = x.to_bits();
    let exponent = (bits >> MANTISSA_BITS) as i32 - EXPONENT_BIAS;
    let parity = exponent.rem_euclid(2);
    let top = (bits >> (MANTISSA_BITS - KEY_BITS)) & ((1 << KEY_BITS) - 1);
    let at = ENTRY_LEN * ((parity as usize) << KEY_BITS | top as usize);
    let mantissa = u32::from_str_radix(&TABLE[at..at + ENTRY_LEN - 1], 16).expect("table entry");
    let result_exponent = RESULT_EXPONENT - (exponent - parity) / 2;
    f32::from_bits((result_exponent as u32) << MANTISSA_BITS | mantissa << (MANTISSA_BITS - 12))
}

/// The magnitude of one bin: `s = re^2 + im^2`, `r = rsqrt(s)`, then
/// `((3 - r * (s * r)) * 0.5) * (s * r)`; zero stays zero.
pub fn magnitude(bin: Complex) -> f32 {
    let s = bin.0 * bin.0 + bin.1 * bin.1;
    if s == 0.0 {
        return 0.0;
    }
    let r = rsqrt_estimate(s);
    let q = s * r;
    ((3.0 - r * q) * 0.5) * q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_estimate_is_twelve_bits_of_one_over_the_root() {
        for x in [1.0_f32, 1.9, 2.0, 3.7, 0.3, 1234.5, 1e-9] {
            let want = 1.0 / f64::from(x).sqrt();
            let got = f64::from(rsqrt_estimate(x));
            assert!(
                ((got - want) / want).abs() < 1.5 / 4096.0,
                "{x}: {got} {want}"
            );
        }
    }

    #[test]
    fn the_estimate_at_one_is_the_hardware_value() {
        // Intel `rsqrtps` of 1.0 is 0.99975586 (1 - 2^-12), not 1.0.
        assert_eq!(rsqrt_estimate(1.0).to_bits(), 0x3f7f_f000);
    }

    #[test]
    fn a_subnormal_power_is_the_hardware_negative_infinity() {
        // The i3-10100T's `rsqrtps` of a subnormal is inf, so the Newton step gives -inf.
        assert_eq!(magnitude((1e-20, 0.0)), f32::NEG_INFINITY);
    }

    #[test]
    fn a_zero_bin_stays_zero() {
        assert_eq!(magnitude((0.0, 0.0)).to_bits(), 0);
    }
}
