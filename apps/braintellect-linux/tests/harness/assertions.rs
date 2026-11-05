//! One assertion per layer. Each compares observable behavior only and returns
//! a [`Mismatch`] describing the first difference.

use super::fixtures::{Decision, StoredRecord};
use super::tolerance::{RECORD_EPSILON, SAMPLE_EPSILON_UV, TIMING_WINDOW_MS};

/// The first difference between clone output and the baseline.
#[derive(Debug)]
pub struct Mismatch(pub String);

pub type Parity = Result<(), Mismatch>;

fn mismatch(msg: String) -> Parity {
    Err(Mismatch(msg))
}

/// Decoded microvolts match within [`SAMPLE_EPSILON_UV`].
pub fn microvolts(expected: &[f64], actual: &[f64]) -> Parity {
    if expected.len() != actual.len() {
        return mismatch(format!(
            "sample count: expected {}, got {}",
            expected.len(),
            actual.len()
        ));
    }
    for (i, (e, a)) in expected.iter().zip(actual).enumerate() {
        if (e - a).abs() > SAMPLE_EPSILON_UV {
            return mismatch(format!("sample {i}: expected {e} uV, got {a} uV"));
        }
    }
    Ok(())
}

/// Commands written to the controller match byte for byte, in order.
pub fn command_bytes(expected: &[Vec<u8>], actual: &[Vec<u8>]) -> Parity {
    if expected.len() != actual.len() {
        return mismatch(format!(
            "command count: expected {}, got {}",
            expected.len(),
            actual.len()
        ));
    }
    for (i, (e, a)) in expected.iter().zip(actual).enumerate() {
        if e != a {
            return mismatch(format!("command {i}: expected {e:02x?}, got {a:02x?}"));
        }
    }
    Ok(())
}

/// Note/trigger decisions match in kind and values, with times within
/// [`TIMING_WINDOW_MS`].
pub fn decisions(expected: &[Decision], actual: &[Decision]) -> Parity {
    if expected.len() != actual.len() {
        return mismatch(format!(
            "decision count: expected {}, got {}",
            expected.len(),
            actual.len()
        ));
    }
    for (i, (e, a)) in expected.iter().zip(actual).enumerate() {
        if (e.kind.as_str(), e.old, e.new) != (a.kind.as_str(), a.old, a.new) {
            return mismatch(format!("decision {i}: expected {e:?}, got {a:?}"));
        }
        if e.time_ms.abs_diff(a.time_ms) > TIMING_WINDOW_MS {
            return mismatch(format!(
                "decision {i} time: expected {} ms, got {} ms",
                e.time_ms, a.time_ms
            ));
        }
    }
    Ok(())
}

/// Stored record has the same fields; numeric values match within
/// [`RECORD_EPSILON`], all others exactly.
pub fn stored_record(expected: &StoredRecord, actual: &StoredRecord) -> Parity {
    let keys = |r: &StoredRecord| r.0.keys().cloned().collect::<Vec<_>>();
    if keys(expected) != keys(actual) {
        return mismatch(format!(
            "record fields: expected {:?}, got {:?}",
            keys(expected),
            keys(actual)
        ));
    }
    for (key, e) in &expected.0 {
        let a = &actual.0[key];
        let equal = match (e.parse::<f64>(), a.parse::<f64>()) {
            (Ok(e), Ok(a)) => (e - a).abs() <= RECORD_EPSILON,
            _ => e == a,
        };
        if !equal {
            return mismatch(format!("record {key}: expected {e}, got {a}"));
        }
    }
    Ok(())
}
