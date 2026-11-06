//! `ExprTriggerAll`: reduces the Band 10 (1-3 Hz) and Band 80 (23-36 Hz)
//! amplitudes of the four channels to the threshold's input, selected by the
//! Evaluator code. The expression is the design's own (`audio-feedback.md`,
//! "SMART-2 Evaluator"): one branch per code, summing E..H for any other code.

/// Band amplitudes in volts: A, B = Ch1, Ch2 in Band 10; C, D = Ch1, Ch2 in
/// Band 80; E, F = Ch3, Ch4 in Band 10; G, H = Ch3, Ch4 in Band 80.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandAmplitudes {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
    pub g: f32,
    pub h: f32,
}

/// The expression's result in volts. Sums are formed in double precision and
/// rounded once.
pub fn trigger_all(control: u32, v: BandAmplitudes) -> f32 {
    let sum = |xs: &[f32]| xs.iter().map(|&x| f64::from(x)).sum::<f64>() as f32;
    match control {
        11 => sum(&[v.a, v.b, v.c, v.d]),
        12 => v.a,
        13 => v.b,
        81 => sum(&[v.c, v.d]),
        82 => v.c,
        83 => v.d,
        112 => v.e,
        113 => v.f,
        181 => sum(&[v.g, v.h]),
        182 => v.g,
        183 => v.h,
        _ => sum(&[v.e, v.f, v.g, v.h]),
    }
}
