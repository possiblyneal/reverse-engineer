//! The audio layer's behavior on synthetic banks: bank parsing and the note map,
//! the player's gating and mixing, the importer and the sink's failure modes.
//! Nothing here opens an audio device or touches vendor bytes.

mod harness;

use braintellect_linux::audio::bank::{Bank, BankError};
use harness::synthetic_bank;

/// Three notes with distinct, recognizable samples (rate 8 kHz, 4 frames).
fn three_notes() -> Vec<(u8, Vec<u8>)> {
    vec![
        (41, synthetic_bank::wav(8000, &[1, 2, 3, 4])),
        (40, synthetic_bank::wav(8000, &[10, 20, 30, 40])),
        (84, synthetic_bank::wav(8000, &[-5, -6, -7, -8])),
    ]
}

#[test]
fn the_bank_maps_each_note_to_its_own_sample_whatever_the_blob_order() {
    let bank = Bank::parse(&synthetic_bank::bank(&three_notes())).expect("bank");
    assert_eq!(bank.notes().collect::<Vec<_>>(), [40, 41, 84]);
    let sample = bank.sample(40).expect("note 40");
    assert_eq!(sample.rate_hz, 8000);
    assert_eq!(sample.pcm, [10, 20, 30, 40]);
    assert_eq!(bank.sample(41).expect("note 41").pcm, [1, 2, 3, 4]);
    assert_eq!(bank.sample(84).expect("note 84").pcm, [-5, -6, -7, -8]);
    assert!(bank.sample(42).is_none());
}

#[test]
fn zero_padding_around_the_resource_is_not_mistaken_for_the_tree() {
    let mut bytes = synthetic_bank::bank(&three_notes());
    bytes.extend([0u8; 4096]);
    let bank = Bank::parse(&bytes).expect("bank");
    assert_eq!(bank.notes().collect::<Vec<_>>(), [40, 41, 84]);
}

#[test]
fn bytes_without_a_sample_resource_are_an_error_not_a_crash() {
    assert!(matches!(
        Bank::parse(&[0u8; 4096]),
        Err(BankError::NoResource)
    ));
    assert!(matches!(Bank::parse(&[]), Err(BankError::NoResource)));
}

#[test]
fn a_bank_cut_inside_its_samples_is_an_error() {
    let bytes = synthetic_bank::bank(&three_notes());
    for cut in [20, 100, 160] {
        assert!(Bank::parse(&bytes[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn a_sample_that_is_not_16_bit_mono_pcm_is_refused() {
    let mut file = synthetic_bank::wav(8000, &[1, 2]);
    file[22] = 2; // two channels
    let bytes = synthetic_bank::bank(&[(40, file)]);
    assert!(matches!(Bank::parse(&bytes), Err(BankError::Wave(_))));
}

#[test]
fn a_missing_bank_file_is_an_error() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-bank.dll");
    assert!(matches!(Bank::load(&path), Err(BankError::Io(_))));
}

/// Local only: the original's bank 1002, named by `BT_BANK_1002_DLL` (the
/// vendor file is never in Git). Run with `cargo test -- --ignored`.
#[test]
#[ignore = "needs the original's dll1002.BT.dll in BT_BANK_1002_DLL"]
fn the_real_bank_1002_holds_45_notes_of_0_8_s_at_96_khz() {
    let path = std::env::var_os("BT_BANK_1002_DLL").expect("BT_BANK_1002_DLL");
    let bank = Bank::load(std::path::Path::new(&path)).expect("bank 1002");
    assert_eq!(
        bank.notes().collect::<Vec<_>>(),
        (40..=84).collect::<Vec<_>>()
    );
    for note in 40..=84 {
        let sample = bank.sample(note).expect("note");
        assert_eq!(
            (sample.rate_hz, sample.pcm.len()),
            (96_000, 76_800),
            "note {note}"
        );
    }
}
