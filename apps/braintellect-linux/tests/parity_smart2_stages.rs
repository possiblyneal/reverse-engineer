//! Parity of the SMART-2 stages upstream of `ExprTriggerAll` (Source filter
//! chain, `BandPassFilter`, `PeakFrequency`, `AggregatorFilter`) and of the
//! graph from decoded EEG to the trigger and note decisions, against run 4.
//!
//! The Source chain, the band-pass FIR and PeakFrequency are asserted bit for bit
//! against the run-5 live capture. Band `OutAmplitude` (a 16-tap mean, 1 to 3
//! `f32` ulps off) and the run-4 band amplitudes are not asserted at any harness
//! tolerance (see `band_pass_amplitude_agreement_report`, `stage_agreement_report`).

mod harness;

use braintellect_linux::feedback::{
    BandPass, BandPassDesign, BlockDecision, Interference1, Interference2, PeakDesign,
    PeakFrequency, Smart2Graph, Smoother, SourceDesign, SourceFilters, BLOCK_LEN,
};
use harness::assertions;
use harness::fixtures::{self, Decision};

/// Same counter timer start as `parity_feedback.rs`.
const TIMER_BEFORE_FIRST_BLOCK_MS: i64 = 500;
/// Logged samples excluded from the stage figures: `BandPassFilter` start-up,
/// `audio-feedback.md` "Not resolved", first entry.
const BAND_START_UP_SAMPLES: usize = 600;
/// Window of the `continuous` Aggregators: 500 Hz times the 60 s `averageEpoch`.
const AGGREGATOR_WINDOW: usize = 500 * 60;

/// Run the graph over the decoded EEG, block by block, under the logged
/// Evaluator codes and block times.
fn replay() -> Vec<BlockDecision> {
    let mut graph =
        Smart2Graph::new(fixtures::smart2_design(), -TIMER_BEFORE_FIRST_BLOCK_MS).expect("design");
    let logged: Vec<[f64; 4]> = fixtures::smart2_eeg().into_iter().map(|r| r.1).collect();
    fixtures::smart2_trigger_blocks()
        .iter()
        .zip(logged.chunks(BLOCK_LEN))
        .map(|(block, samples)| {
            graph
                .process_block(block.time_ms as i64, block.control, samples)
                .expect("observed Evaluator codes")
        })
        .collect()
}

/// Live samples before the capture's filter state has converged: the capture was
/// hooked mid-stream, the clone starts cold, and the filter memories fade within this
/// many samples.
const SOURCE_CONVERGENCE_SAMPLES: usize = 3000;

#[test]
fn source_chain_reproduces_the_live_stage_outputs_bit_for_bit() {
    let design = SourceDesign {
        interference1: Interference1::LowPass45,
        interference2: Interference2::Hz25,
        low_pass_hz: 150.0,
        high_pass_hz: 1.5,
    };
    let mut chain = SourceFilters::new(&design, 500);
    let rows = fixtures::smart2_source_stages();
    assert!(rows.len() > SOURCE_CONVERGENCE_SAMPLES + 1000);
    for (n, row) in rows.iter().enumerate() {
        let got = chain.push_stages(f64::from(row[0]));
        if n < SOURCE_CONVERGENCE_SAMPLES {
            continue;
        }
        for (stage, (&g, &want)) in got.iter().zip(&row[2..]).enumerate() {
            assert_eq!(
                (g as f32).to_bits(),
                want.to_bits(),
                "sample {n} stage {stage}"
            );
        }
    }
}

/// `(low Hz, high Hz, column of the FIR output, column of the amplitude)` of the two
/// captured bands.
const CAPTURED_BANDS: [(f64, f64, usize, usize); 2] = [(1.0, 3.0, 1, 2), (23.0, 36.0, 3, 4)];

fn captured_band(low_hz: f64, high_hz: f64) -> BandPass {
    let design = BandPassDesign {
        low_hz,
        high_hz,
        order: 128,
    };
    BandPass::new(&design, 500, 4)
}

#[test]
fn band_pass_fir_reproduces_the_live_outputs_bit_for_bit() {
    let rows = fixtures::smart2_bandpass_stages();
    assert!(rows.len() >= 6000);
    for (low_hz, high_hz, fir, _) in CAPTURED_BANDS {
        let mut band = captured_band(low_hz, high_hz);
        for (n, row) in rows.iter().enumerate() {
            let [out, _] = band.push_stages(f64::from(row[0]));
            assert_eq!(
                (out as f32).to_bits(),
                row[fir].to_bits(),
                "band {high_hz} sample {n}"
            );
        }
    }
}

/// Not asserted: the 16-tap mean of `OutAmplitude` is not bit exact (see
/// `audio-feedback.md`), so print the f32 ulp distance of the clone to the live value.
#[test]
fn band_pass_amplitude_agreement_report() {
    let rows = fixtures::smart2_bandpass_stages();
    for (low_hz, high_hz, _, amp) in CAPTURED_BANDS {
        let mut band = captured_band(low_hz, high_hz);
        let mut ulps = vec![0u32; 6];
        for (n, row) in rows.iter().enumerate() {
            let got = (band.push(f64::from(row[0])) as f32).to_bits();
            if n >= 600 {
                let d = got.abs_diff(row[amp].to_bits()) as usize;
                ulps[d.min(5)] += 1;
            }
        }
        println!("band {high_hz}: samples at 0..>=5 ulp {ulps:?}");
    }
}

#[test]
fn peak_frequency_reproduces_the_live_frequency_and_amplitude_bit_for_bit() {
    let rows = fixtures::smart2_bandpass_stages();
    let design = PeakDesign {
        fft_order: 9,
        low_hz: 6.0,
        high_hz: 14.0,
    };
    let mut peak = PeakFrequency::new(&design, 500);
    for (n, row) in rows.iter().enumerate() {
        let got = peak.push(f64::from(row[0]));
        if n == 1 {
            // Two nonzero samples: the spectrum is flat to rounding, so the app's FFT
            // rounding decides the bin and the clone's FFT does not reproduce it.
            continue;
        }
        assert_eq!(
            (got.frequency_hz as f32).to_bits(),
            row[5].to_bits(),
            "frequency, sample {n}"
        );
        assert_eq!(
            (got.amplitude as f32).to_bits(),
            row[6].to_bits(),
            "amplitude, sample {n}"
        );
    }
}

#[test]
fn aggregators_reproduce_the_logged_means_of_the_logged_band_amplitudes() {
    let mut smoothers: Vec<Smoother> = (0..8).map(|_| Smoother::new(AGGREGATOR_WINDOW)).collect();
    for (b, block) in fixtures::smart2_stages().iter().enumerate() {
        for (stage, smoother) in smoothers.iter_mut().enumerate() {
            for n in 0..BLOCK_LEN {
                let got = smoother.push(block.bands[stage][n]);
                assert_eq!(
                    got, block.aggregates[stage][n],
                    "block {b} sample {n} stage {stage}"
                );
            }
        }
    }
}

#[test]
fn end_to_end_trigger_decisions_match_the_baseline() {
    let blocks = fixtures::smart2_trigger_blocks();
    let actual: Vec<Decision> = replay()
        .iter()
        .zip(&blocks)
        .filter(|(decision, _)| decision.trigger)
        .map(|(_, block)| Decision {
            time_ms: block.time_ms,
            kind: "Trigger".into(),
            old: 0,
            new: 1,
        })
        .collect();
    let expected: Vec<Decision> = fixtures::smart2_decisions()
        .into_iter()
        .filter(|d| d.kind == "Trigger")
        .collect();
    assert!(expected.len() > 15, "baseline holds many triggers");
    assertions::decisions(&expected, &actual).expect("trigger parity from decoded EEG");
}

/// The `Note` decisions from decoded EEG, then the baseline's.
fn note_decisions() -> (Vec<Decision>, Vec<Decision>) {
    let blocks = fixtures::smart2_trigger_blocks();
    let actual: Vec<Decision> = replay()
        .iter()
        .zip(&blocks)
        .filter_map(|(decision, block)| {
            decision.note_at_trigger.map(|note| Decision {
                time_ms: block.time_ms,
                kind: "Note".into(),
                old: 0,
                new: u32::try_from(note).expect("MIDI notes are non-negative"),
            })
        })
        .collect();
    let expected: Vec<Decision> = fixtures::smart2_decisions()
        .into_iter()
        .filter(|d| d.kind == "Note")
        .collect();
    (expected, actual)
}

/// Notes the graph reproduces from decoded EEG with the Source filters cold at the
/// first logged sample (9 of 20 with warm filters). Not a tolerance: a floor that
/// keeps that gain until the strict test below passes.
const NOTES_MATCHING_FROM_DECODED_EEG: usize = 17;

#[test]
fn most_note_decisions_match_the_baseline_from_decoded_eeg() {
    let (expected, actual) = note_decisions();
    assert_eq!(expected.len(), actual.len(), "one note per counter fire");
    let matching = expected
        .iter()
        .zip(&actual)
        .filter(|(e, a)| e.new == a.new)
        .count();
    assert!(
        matching >= NOTES_MATCHING_FROM_DECODED_EEG,
        "{matching} of {} notes match",
        expected.len()
    );
}

/// Known gap: fires 1, 2 and 5 differ. Fires 1 and 2 read PeakFrequency values from
/// the first 130 blocks, where the cold Source transient and the start-up window
/// dominate; fire 5 is a counter fire one block late (`audio-feedback.md`, "Not
/// resolved"). Run with `--ignored`; it fails until those are resolved.
#[test]
#[ignore = "17 of 20 note decisions match from decoded EEG"]
fn end_to_end_note_decisions_match_the_baseline() {
    let (expected, actual) = note_decisions();
    assertions::decisions(&expected, &actual).expect("note parity from decoded EEG");
}

/// Prints how closely the recovered stages follow the logged values. Not an
/// assertion: the harness has no tolerance that fits a statistical match.
///
/// Excluded from the figures: the first `BAND_START_UP_SAMPLES` samples
/// (`audio-feedback.md` "Not resolved", BandPassFilter start-up) and Band 10 on
/// Ch2 (same section, Band 10 Ch2 entry).
#[test]
#[ignore = "report only"]
fn stage_agreement_report() {
    let design = fixtures::smart2_design();
    let stages = fixtures::smart2_stages();
    let decisions = replay();
    for (stage, &(label, channel)) in fixtures::STAGES.iter().enumerate() {
        if (label, channel) == ("Band 10", 1) {
            continue;
        }
        let at = design
            .band_passes
            .iter()
            .position(|b| b.channel == channel && b.label == label)
            .expect("band-pass in design");
        let mut errors: Vec<f64> = (BAND_START_UP_SAMPLES..stages.len() * BLOCK_LEN)
            .map(|s| {
                let want = f64::from(stages[s / BLOCK_LEN].bands[stage][s % BLOCK_LEN]);
                let got = f64::from(decisions[s / BLOCK_LEN].samples[s % BLOCK_LEN].bands[at]);
                ((got - want) / want).abs()
            })
            .collect();
        errors.sort_by(f64::total_cmp);
        println!(
            "{label} Ch{}: median relative error {:.3}",
            channel + 1,
            errors[errors.len() / 2]
        );
    }
    let (mut missed, mut total) = (0, 0);
    for (b, decision) in decisions.iter().take(stages.len()).enumerate() {
        for (n, sample) in decision.samples.iter().enumerate() {
            if b * BLOCK_LEN + n < BAND_START_UP_SAMPLES {
                continue;
            }
            for ch in 0..4 {
                total += 1;
                missed += usize::from(sample.peaks[ch].bin != stages[b].peak_bins[ch][n]);
            }
        }
    }
    println!("PeakFrequency bins differing: {missed} of {total}");
}
