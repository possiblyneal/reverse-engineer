//! `ExprTriggerAll`: reduces the Band 10 (1-3 Hz) and Band 80 (23-36 Hz)
//! amplitudes of channels 3 and 4 to the threshold's input, selected by the
//! Evaluator code (`audio-feedback.md`, "Run 4 Capture"). Inputs A..D (the
//! channel 1 and 2 amplitudes) are not used by the codes observed.

use std::fmt;

/// The Evaluator's `defaultControlValue`: no test passed.
const CONTROL_DEFAULT: u32 = 111;
/// Evaluator code for Band 80, Ch3/Ch4 pair, channel 4 larger.
const CONTROL_BAND80_CH4: u32 = 183;

/// Band amplitudes in volts: E, F = Ch3, Ch4 in Band 10; G, H = Ch3, Ch4 in Band 80.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandAmplitudes {
    pub e: f32,
    pub f: f32,
    pub g: f32,
    pub h: f32,
}

/// An Evaluator code whose `ExprTriggerAll` result is not recovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedControl(pub u32);

impl fmt::Display for UnsupportedControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ExprTriggerAll has no recovered result for code {}",
            self.0
        )
    }
}

impl std::error::Error for UnsupportedControl {}

/// The expression's result in volts: `E + F + G + H` for the default code, `H`
/// for 183. The sum is formed in double precision and rounded once.
pub fn trigger_all(control: u32, a: BandAmplitudes) -> Result<f32, UnsupportedControl> {
    match control {
        CONTROL_DEFAULT => {
            Ok((f64::from(a.e) + f64::from(a.f) + f64::from(a.g) + f64::from(a.h)) as f32)
        }
        CONTROL_BAND80_CH4 => Ok(a.h),
        other => Err(UnsupportedControl(other)),
    }
}
