//! The feedback tones: the original banks plus the tones rendered from a
//! SoundFont. The SoundFont here is synthetic (looped sines); no third-party
//! sample bytes are in the tests.

mod harness;

use std::fs;
use std::path::PathBuf;

use braintellect_linux::audio::bank::Sample;
use braintellect_linux::audio::soundfont::{SoundFont, SoundFontError};
use braintellect_linux::audio::tone::{
    import_soundfont, load_tone, Availability, Tone, ToneImportError, DEFAULT_TONE, TONES,
};
use braintellect_linux::audio::InstrumentError;
use braintellect_linux::live::OUTPUT_RATE_HZ;
use harness::synthetic_bank;
use harness::synthetic_soundfont::{soundfont, soundfont_after_unused_samples, Preset, Zone};

const NOTES: std::ops::RangeInclusive<i32> = 40..=76;
/// A rendered note's relative pitch bound, outside the parity contract (not a `tolerance.rs` item).
const PITCH_RELATIVE_ERROR: f64 = 0.003;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("audio-tone-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn zone(keys: (u8, u8), rate_hz: u32, period: u32, header_pitch: u8) -> Zone {
    Zone {
        keys,
        rate_hz,
        period,
        header_pitch,
        root_override: None,
        fine_cents: None,
        loops: true,
    }
}

/// Harp, nylon guitar and xylophone presets that differ in rate, root key
/// source, key split, loop source and tuning, so each way the renderer finds the
/// pitch is exercised.
fn font() -> Vec<u8> {
    soundfont(&[
        Preset {
            program: 46,
            global_loop: true,
            zones: vec![
                Zone {
                    loops: false,
                    root_override: Some(57),
                    ..zone((0, 58), 44_100, 200, 60)
                },
                Zone {
                    loops: false,
                    ..zone((59, 127), 44_100, 100, 69)
                },
            ],
        },
        Preset {
            program: 24,
            global_loop: false,
            zones: vec![Zone {
                fine_cents: Some(-25),
                ..zone((0, 127), 22_050, 50, 69)
            }],
        },
        Preset {
            program: 13,
            global_loop: false,
            zones: vec![Zone {
                root_override: Some(70),
                ..zone((0, 127), 32_000, 64, 60)
            }],
        },
    ])
}

/// Hz of the first `frames` of `pcm` at `rate_hz`, by upward zero crossings with
/// linear interpolation.
fn fundamental_hz(pcm: &[i16], rate_hz: u32) -> f64 {
    let crossings: Vec<f64> = pcm
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] < 0 && w[1] >= 0)
        .map(|(i, w)| i as f64 + f64::from(-w[0]) / f64::from(i32::from(w[1]) - i32::from(w[0])))
        .collect();
    let periods = crossings.len() - 1;
    periods as f64 * f64::from(rate_hz) / (crossings[periods] - crossings[0])
}

fn expected_hz(zone_hz: f64, root: i32, cents: f64, note: i32) -> f64 {
    zone_hz * 2f64.powf((f64::from(note - root) + cents / 100.0) / 12.0)
}

fn tone(key: &str) -> &'static Tone {
    TONES.iter().find(|t| t.key == key).expect("tone")
}

fn assert_pitch(sample: &Sample, expected: f64, what: &str) {
    assert_eq!(sample.rate_hz, OUTPUT_RATE_HZ);
    // Leave out the closing fade, which ends in a silent run that is not a cycle.
    let steady = &sample.pcm[..sample.pcm.len() - 1000];
    let got = fundamental_hz(steady, sample.rate_hz);
    assert!(
        (got / expected - 1.0).abs() < PITCH_RELATIVE_ERROR,
        "{what}: {got} Hz, expected {expected} Hz"
    );
}

#[test]
fn harp_guitar_and_xylophone_play_every_note_40_to_76_at_the_right_pitch() {
    let dir = scratch("pitch");
    import_soundfont(&font(), &dir).expect("import");

    for note in NOTES {
        let harp = load_tone(&dir, tone("harp")).expect("harp");
        let (zone_hz, root) = if note <= 58 { (220.5, 57) } else { (441.0, 69) };
        assert_pitch(
            harp.sample(note).expect("harp note"),
            expected_hz(zone_hz, root, 0.0, note),
            &format!("harp {note}"),
        );

        let guitar = load_tone(&dir, tone("guitar")).expect("guitar");
        assert_pitch(
            guitar.sample(note).expect("guitar note"),
            expected_hz(441.0, 69, -25.0, note),
            &format!("guitar {note}"),
        );

        let xylophone = load_tone(&dir, tone("xylophone")).expect("xylophone");
        assert_pitch(
            xylophone.sample(note).expect("xylophone note"),
            expected_hz(500.0, 70, 0.0, note),
            &format!("xylophone {note}"),
        );
    }
}

#[test]
fn a_rendered_tone_holds_exactly_the_clamped_note_range_for_the_length_of_a_note() {
    let dir = scratch("range");
    import_soundfont(&font(), &dir).expect("import");
    let bank = load_tone(&dir, tone("harp")).expect("harp");

    assert_eq!(bank.notes().collect::<Vec<_>>(), NOTES.collect::<Vec<_>>());
    // 0.8 s, the length of the original's notes.
    assert_eq!(bank.sample(60).expect("note").pcm.len(), 76_800);
}

#[test]
fn a_tone_that_was_not_imported_is_unavailable_and_says_how_to_import_it() {
    let dir = scratch("missing");
    fs::write(
        dir.join("dll1002.BT.dll"),
        synthetic_bank::bank(&[(40, synthetic_bank::wav(8000, &[1, 2]))]),
    )
    .expect("write");

    assert_eq!(DEFAULT_TONE.availability(&dir), Availability::Ready);
    let Availability::Missing(why) = tone("harp").availability(&dir) else {
        panic!("harp was never imported");
    };
    assert!(why.contains("import-soundfont"), "{why}");
    assert!(matches!(
        load_tone(&dir, tone("harp")),
        Err(InstrumentError::Missing(_))
    ));
    // The original sibling banks are reported the same way.
    assert!(matches!(
        tone("piano").availability(&dir),
        Availability::Missing(_)
    ));
}

#[test]
fn importing_a_soundfont_makes_exactly_the_rendered_tones_available() {
    let dir = scratch("listing");
    import_soundfont(&font(), &dir).expect("import");

    let ready: Vec<&str> = TONES
        .iter()
        .filter(|t| t.availability(&dir) == Availability::Ready)
        .map(|t| t.key)
        .collect();
    assert_eq!(ready, ["harp", "guitar", "xylophone"]);
}

#[test]
fn a_soundfont_without_a_required_instrument_is_refused_and_writes_nothing() {
    let dir = scratch("incomplete");
    let font = soundfont(&[Preset {
        program: 46,
        global_loop: true,
        zones: vec![zone((0, 127), 44_100, 100, 69)],
    }]);

    assert!(matches!(
        import_soundfont(&font, &dir),
        Err(ToneImportError::Font(SoundFontError::MissingPreset(24)))
    ));
    assert_eq!(fs::read_dir(&dir).expect("dir").count(), 0);
}

#[test]
fn a_write_that_fails_leaves_no_tone_behind() {
    let dir = scratch("write-fails");
    // A directory where the guitar's file is written makes that write fail.
    fs::create_dir(dir.join("tone-guitar.bank.part")).expect("obstacle");

    assert!(matches!(
        import_soundfont(&font(), &dir),
        Err(ToneImportError::Io(_))
    ));
    let left: Vec<_> = fs::read_dir(&dir)
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(left, ["tone-guitar.bank.part"]);
}

#[test]
fn a_sample_index_above_32767_is_read() {
    let font = soundfont_after_unused_samples(
        &[Preset {
            program: 46,
            global_loop: true,
            zones: vec![zone((0, 127), 44_100, 100, 69)],
        }],
        32_768,
    );

    let zones = SoundFont::parse(&font)
        .expect("parse")
        .zones(46)
        .expect("zones");
    assert_eq!(zones.len(), 1);
}

#[test]
fn a_sample_without_a_rate_is_refused() {
    let font = soundfont(&[Preset {
        program: 46,
        global_loop: true,
        zones: vec![zone((0, 127), 0, 100, 69)],
    }]);

    assert!(matches!(
        SoundFont::parse(&font).expect("parse").zones(46),
        Err(SoundFontError::Malformed("a sample has no rate"))
    ));
}

#[test]
fn the_tone_list_is_the_original_banks_by_their_resource_names_then_the_recorded_ones() {
    let labels: Vec<(&str, &str)> = TONES.iter().map(|t| (t.key, t.label)).collect();
    assert_eq!(
        labels,
        [
            ("strings", "Acoustic strings (original default)"),
            ("piano", "Piano"),
            ("celeste", "Celeste"),
            ("harp", "Harp"),
            ("guitar", "Acoustic guitar"),
            ("xylophone", "Xylophone"),
        ]
    );
    assert_eq!(DEFAULT_TONE.key, "strings");
}

#[test]
fn a_preview_plays_its_notes_apart_and_each_whole() {
    use braintellect_linux::audio::bank::Bank;
    use braintellect_linux::audio::player::PlayError;
    use braintellect_linux::audio::tone::{preview, PREVIEW_GAP_MS, PREVIEW_NOTES};
    use std::collections::BTreeMap;

    let len = 1000;
    let samples: BTreeMap<i32, Sample> = PREVIEW_NOTES
        .iter()
        .map(|&n| {
            let pcm = vec![i16::try_from(n).expect("note"); len];
            (
                n,
                Sample {
                    rate_hz: OUTPUT_RATE_HZ,
                    pcm,
                },
            )
        })
        .collect();
    let pcm = preview(&Bank::from_samples(samples.clone())).expect("preview");

    let gap = usize::try_from(PREVIEW_GAP_MS).expect("gap") * OUTPUT_RATE_HZ as usize / 1000;
    assert_eq!(pcm.len(), 2 * gap + len);
    assert_eq!(pcm[0], i16::try_from(PREVIEW_NOTES[0]).expect("note"));
    assert_eq!(
        pcm[2 * gap + len - 1],
        i16::try_from(PREVIEW_NOTES[2]).expect("note")
    );

    let mut missing = samples;
    missing.remove(&PREVIEW_NOTES[1]);
    assert_eq!(
        preview(&Bank::from_samples(missing)),
        Err(PlayError::UnknownNote(PREVIEW_NOTES[1]))
    );
}
