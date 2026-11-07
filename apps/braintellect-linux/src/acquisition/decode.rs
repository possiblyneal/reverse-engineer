//! Sample decoding and ADC->volts conversion for the B-2 stream.
//!
//! Every constant and formula here is recovered and cross-verified from the
//! original `braintellect.exe` and recorded in
//! `docs/reverse-engineering/braintellect2/b2-protocol.md` (sections "Inbound
//! sample stream" and "ADC -> volts conversion"). Call sites are cited by the
//! VA of the instruction that establishes them.
//!
//! Bug deliberately not reproduced: the original assumes a valid calibration
//! state and dereferences through it, so a bad gain faults rather than being
//! rejected (see the "validation-induced null cascade" pattern in the device
//! notes). [`ChannelCalibration::new`] validates its inputs and returns a
//! [`CalibrationError`] instead.

/// Device family reported by the controller, which fixes the reference voltage
/// `D1` and ADC full-scale count `D2` (`setDeviceFamily`, VA `0x557b50`). The
/// B-2 reports 12 (110 reply, pair 1 second byte; `info::SystemInfo1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceFamily {
    /// 16-bit, Vref 2.4 V.
    F11,
    /// 24-bit, Vref 2.4 V.
    F12,
    /// 24-bit, Vref 4.0 V.
    F99,
}

impl DeviceFamily {
    /// The family for a device-reported code: 11, 12 or 99 (`setDeviceFamily`
    /// rejects any other).
    ///
    /// # Errors
    /// The code itself when it is none of those.
    pub fn from_code(code: u8) -> Result<Self, u8> {
        match code {
            11 => Ok(Self::F11),
            12 => Ok(Self::F12),
            99 => Ok(Self::F99),
            other => Err(other),
        }
    }

    /// The family number the app stores (`deviceFamily`: 11, 12 or 99).
    #[must_use]
    pub fn code(self) -> u32 {
        match self {
            Self::F11 => 11,
            Self::F12 => 12,
            Self::F99 => 99,
        }
    }

    /// Reference voltage `D1`, in volts (`.rdata` `2.4`/`4.0`).
    #[must_use]
    pub fn vref(self) -> f64 {
        match self {
            Self::F11 | Self::F12 => 2.4,
            Self::F99 => 4.0,
        }
    }

    /// ADC full-scale count `D2 = 2^N - 1` (`pow` at `0x1073896`, minus `1.0`).
    #[must_use]
    pub fn full_scale(self) -> f64 {
        match self {
            Self::F11 => 32_767.0,                // 2^15 - 1
            Self::F12 | Self::F99 => 8_388_607.0, // 2^23 - 1
        }
    }

    /// Sample width in bits.
    #[must_use]
    pub fn sample_bits(self) -> u8 {
        match self {
            Self::F11 => 16,
            Self::F12 | Self::F99 => 24,
        }
    }
}

/// Lowest channel gain (PGA) the original accepts, `1.0` (`0x18954dc` is the max).
pub const CHANNEL_GAIN_MIN: f64 = 1.0;
/// Highest channel gain (PGA) the original accepts, `24.0` (`.rdata` `0x18954dc`).
pub const CHANNEL_GAIN_MAX: f64 = 24.0;

/// Why a [`ChannelCalibration`] was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationError {
    /// `channel_gain` outside `[CHANNEL_GAIN_MIN, CHANNEL_GAIN_MAX]` (or NaN).
    ChannelGainOutOfRange,
    /// `sensor_gain` not a positive number. The static `1000.0` in
    /// `setSensorGain` is not a floor: the live app runs with 10
    /// (`b2-protocol.md`, sensorGain), so only a zero/negative/NaN gain, which
    /// would break the divisor, is rejected.
    SensorGainNotPositive,
}

/// Per-channel calibration: the PGA gain `G_c` and sensor gain `G_s` the
/// controller reports, from which the counts-per-volt divisor and the
/// peak-to-peak input range are recomputed (recompute at VA `0x5558c0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelCalibration {
    channel_gain: f64,
    sensor_gain: f64,
}

impl ChannelCalibration {
    /// Build a calibration, rejecting out-of-range gains the original would
    /// instead carry into a faulting dereference.
    ///
    /// # Errors
    /// Returns [`CalibrationError`] when either gain is outside the range the
    /// original enforces in its setters.
    pub fn new(channel_gain: f64, sensor_gain: f64) -> Result<Self, CalibrationError> {
        if !(CHANNEL_GAIN_MIN..=CHANNEL_GAIN_MAX).contains(&channel_gain) {
            return Err(CalibrationError::ChannelGainOutOfRange);
        }
        if sensor_gain.is_nan() || sensor_gain <= 0.0 {
            return Err(CalibrationError::SensorGainNotPositive);
        }
        Ok(Self {
            channel_gain,
            sensor_gain,
        })
    }

    /// Counts-per-volt divisor: `G_c * D2 * G_s / D1` (recompute VA `0x5558ea`).
    #[must_use]
    pub fn divisor(self, family: DeviceFamily) -> f64 {
        self.channel_gain * family.full_scale() * self.sensor_gain / family.vref()
    }

    /// Peak-to-peak input range in volts: `2 * D1 / (G_c * G_s)` (VA `0x555900`).
    #[must_use]
    pub fn volts_pp(self, family: DeviceFamily) -> f64 {
        2.0 * family.vref() / (self.channel_gain * self.sensor_gain)
    }

    /// Convert a raw ADC sample to volts: `raw / divisor`, equivalently
    /// `raw * D1 / (G_c * D2 * G_s)` (`_convertValue`, VA `0x555930`).
    #[must_use]
    pub fn to_volts(self, raw_adc: i32, family: DeviceFamily) -> f64 {
        f64::from(raw_adc) / self.divisor(family)
    }
}

/// Decode a 24-bit big-endian signed sample from the three magnitude bytes of a
/// channel's 4-byte group (`processData_v5`, VA `0x556c46`: `movsx` high byte,
/// then two `movzx`/`shl 8`/`or`). Byte `[+0]` of the group is status/lead-off
/// and is not passed here.
#[must_use]
pub fn decode_sample_i24_be(bytes: [u8; 3]) -> i32 {
    let raw = (i32::from(bytes[0]) << 16) | (i32::from(bytes[1]) << 8) | i32::from(bytes[2]);
    // Sign-extend bit 23 via a left-then-arithmetic-right shift.
    (raw << 8) >> 8
}

/// Decode a lead-off (LOD) reading: `0x3fff - x`, a 14-bit value, taken when a
/// channel's type field equals `1` (`processData_v5`, VA `0x556c2c`).
///
/// Provisional pending the T2 capture: the disassembly at `0x556c2c` shows the
/// `0x3fff - x` subtraction, but the input width of `x` (the `& 0x3fff` mask
/// here) is assumed 14-bit, not yet confirmed. Unused until lead-off is wired
/// into the frame walk, at which point the mask is verified against `0x556c2c`.
#[must_use]
pub fn lead_off_reading(raw14: u16) -> i32 {
    0x3fff - i32::from(raw14 & 0x3fff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn i24_sign_extension() {
        assert_eq!(decode_sample_i24_be([0x00, 0x00, 0x00]), 0);
        assert_eq!(decode_sample_i24_be([0x00, 0x00, 0x01]), 1);
        assert_eq!(decode_sample_i24_be([0xFF, 0xFF, 0xFF]), -1);
        assert_eq!(decode_sample_i24_be([0x7F, 0xFF, 0xFF]), 8_388_607);
        assert_eq!(decode_sample_i24_be([0x80, 0x00, 0x00]), -8_388_608);
        assert_eq!(decode_sample_i24_be([0x01, 0x00, 0x00]), 65_536);
    }

    #[test]
    fn lead_off_transform() {
        assert_eq!(lead_off_reading(0x3fff), 0);
        assert_eq!(lead_off_reading(0), 0x3fff);
        assert_eq!(lead_off_reading(0x2000), 0x1fff);
        // Bits above the 14-bit field are ignored.
        assert_eq!(lead_off_reading(0xffff), 0);
    }

    #[test]
    fn family_constants() {
        assert!(close(DeviceFamily::F11.full_scale(), 32_767.0));
        assert!(close(DeviceFamily::F12.full_scale(), 8_388_607.0));
        assert!(close(DeviceFamily::F99.full_scale(), 8_388_607.0));
        assert!(close(DeviceFamily::F11.vref(), 2.4));
        assert!(close(DeviceFamily::F99.vref(), 4.0));
        assert_eq!(DeviceFamily::F12.sample_bits(), 24);
        assert_eq!(DeviceFamily::F11.sample_bits(), 16);
    }

    #[test]
    fn calibration_rejects_bad_gains() {
        assert_eq!(
            ChannelCalibration::new(0.5, 1000.0),
            Err(CalibrationError::ChannelGainOutOfRange)
        );
        assert_eq!(
            ChannelCalibration::new(25.0, 1000.0),
            Err(CalibrationError::ChannelGainOutOfRange)
        );
        assert_eq!(
            ChannelCalibration::new(f64::NAN, 1000.0),
            Err(CalibrationError::ChannelGainOutOfRange)
        );
        assert_eq!(
            ChannelCalibration::new(1.0, 0.0),
            Err(CalibrationError::SensorGainNotPositive)
        );
        assert_eq!(
            ChannelCalibration::new(1.0, f64::NAN),
            Err(CalibrationError::SensorGainNotPositive)
        );
        // The baseline runs with sensorGain 10.
        assert!(ChannelCalibration::new(12.0, 10.0).is_ok());
        assert!(ChannelCalibration::new(1.0, 1000.0).is_ok());
        assert!(ChannelCalibration::new(24.0, 50_000.0).is_ok());
    }

    #[test]
    fn full_scale_positive_sample_is_half_pp() {
        // At the positive full-scale count, volts must equal voltsPP / 2.
        let cal = ChannelCalibration::new(1.0, 1000.0).unwrap();
        for family in [DeviceFamily::F12, DeviceFamily::F99] {
            let full = family.full_scale() as i32;
            assert!(close(
                cal.to_volts(full, family),
                cal.volts_pp(family) / 2.0
            ));
        }
    }

    #[test]
    fn volts_pp_matches_recovered_values() {
        // G_c = 1, G_s = 1000: voltsPP = 2*Vref/(1*1000).
        let cal = ChannelCalibration::new(1.0, 1000.0).unwrap();
        assert!(close(cal.volts_pp(DeviceFamily::F12), 0.0048));
        assert!(close(cal.volts_pp(DeviceFamily::F99), 0.008));
    }
}
