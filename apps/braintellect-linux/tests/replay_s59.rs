//! Holds the clone's BAGSD report (every `00-90`, `LMH` and `LineNoise` series and
//! `FFT-Normal-Order` per channel) to the
//! original's stored type-9 record of the ticket-59 capture (exercise 147), within the record
//! epsilon, and each logged `DSPFFT` block to the original's output bit for bit.
//!
//! The original, fed run 8's no-wearer stream by `emulate`, stored an exercise and ran its
//! BAGSD design (`replay-rig.md`, ticket 59). `scripts/fft-extract.py` reduces its hook log to
//! `volts.tsv` (the consumed volts, as `replay_s55`) and `fft_in_<c>.txt` / `fft_out_<c>.txt`
//! (each BAGSD `DSPFFT` call's input block and output, one line per call); `ed_9.xml` is the decoded type-9 record.
//! Both are vendor-derived and stay under `tmp/`. Run with
//! `cargo test --release --test replay_s59 -- --ignored --nocapture`; `S59_REPLAY_DIR`
//! overrides the default directory (`tmp/s59/replay` at the repository root).

mod harness;

use braintellect_linux::acquisition::decode::{ChannelCalibration, DeviceFamily};
use braintellect_linux::feedback::{
    DspFft, Interference1, Interference2, SourceDesign, SourceFilters, StoredSeries, CHANNELS,
    RATE_HZ,
};
use braintellect_linux::storage::Channel;
use harness::{assertions, inputs, record};

/// Consumed sample the exercise's graph starts at, and its length: 240 040 ms. The 45 Hz
/// low-pass is switched in at the same sample on every channel.
const GRAPH_START: usize = 107_196;
const SAMPLES: usize = 120_020;
fn read(name: &str) -> String {
    inputs::read("S59_REPLAY_DIR", "tmp/s59/replay", name)
}

#[test]
#[ignore = "inputs live outside Git (tmp/s59/replay)"]
fn the_bagsd_report_matches_the_stored_record() {
    let design = SourceDesign {
        interference1: Interference1::Off,
        interference2: Interference2::Hz25,
        low_pass_hz: 150.0,
        high_pass_hz: 1.5,
    };
    let mut source: [SourceFilters; CHANNELS] =
        std::array::from_fn(|_| SourceFilters::new(&design, RATE_HZ).expect("design"));
    let counts_per_volt = ChannelCalibration::new(12.0, 10.0)
        .expect("baseline calibration")
        .divisor(DeviceFamily::F12);
    let mut stored = StoredSeries::new(counts_per_volt, RATE_HZ);
    for (s, line) in read("volts.tsv")
        .lines()
        .take(GRAPH_START + SAMPLES)
        .enumerate()
    {
        let volts: Vec<f64> = line
            .split('\t')
            .map(|t| f64::from_bits(u64::from_str_radix(t, 16).expect("float64 bits")))
            .collect();
        let filtered: [f64; CHANNELS] = std::array::from_fn(|c| {
            if s == GRAPH_START {
                source[c].set_interference1(Interference1::LowPass45, RATE_HZ);
            }
            source[c].push(volts[c])
        });
        if s >= GRAPH_START {
            stored.push(&filtered);
        }
    }
    let actual: Vec<Channel> = stored
        .series()
        .into_iter()
        .enumerate()
        .map(|(c, series)| Channel {
            name: format!("Channel{}", c + 1),
            series,
        })
        .collect();
    let expected = record::as_stored(&read("ed_9.xml"), &actual);
    for (channel, series, worst) in record::worst_by_series(&expected, &actual) {
        println!("{channel}\t{series}\t{worst:e}");
    }
    println!("{SAMPLES} samples");
    assertions::stored_series(&expected, &actual).expect("BAGSD report parity");
}

/// One line of logged `float32` values.
fn floats(line: &str) -> Vec<f32> {
    line.split_whitespace()
        .map(|v| v.parse::<f64>().expect("number") as f32)
        .collect()
}

#[test]
#[ignore = "inputs live outside Git (tmp/s59/replay)"]
fn every_logged_dspfft_block_is_bit_exact() {
    let fft = DspFft::new();
    for c in 0..CHANNELS {
        let inputs = read(&format!("fft_in_{c}.txt"));
        let outputs = read(&format!("fft_out_{c}.txt"));
        assert_eq!(
            inputs.lines().count(),
            outputs.lines().count(),
            "channel {c}"
        );
        for (call, (input, output)) in inputs.lines().zip(outputs.lines()).enumerate() {
            let mut block = [0.0f32; 512];
            let input = floats(input);
            block[..input.len()].copy_from_slice(&input);
            let got = fft.process(&block).map(f32::to_bits);
            let want: Vec<u32> = floats(output).iter().map(|v| v.to_bits()).collect();
            assert_eq!(got[..], want[..], "channel {c} call {call}");
        }
    }
}
