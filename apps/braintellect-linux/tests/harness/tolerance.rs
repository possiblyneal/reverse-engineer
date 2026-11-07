//! Tolerances, defined once and cited by every layer's tests.

/// Largest accepted difference between a clone-decoded sample and the baseline,
/// in microvolts. One ADC count at the baseline gain is about 0.0057 uV, so this
/// accepts only floating-point noise, never a one-count error.
pub const SAMPLE_EPSILON_UV: f64 = 1e-6;

/// Largest accepted difference between a clone decision time and the baseline,
/// in milliseconds. The baseline control log is stamped to whole seconds.
pub const TIMING_WINDOW_MS: u64 = 1_000;

/// Largest accepted difference between a numeric stored-record field and the
/// baseline value (the app stores single-precision floats as text).
pub const RECORD_EPSILON: f64 = 1e-6;

/// Largest accepted difference between a clone sensor level and the original's,
/// in dB. The original runs the FFT and the logarithm in single precision.
pub const LEVEL_EPSILON_DB: f32 = 1e-3;
