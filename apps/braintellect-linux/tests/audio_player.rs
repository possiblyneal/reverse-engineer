//! The deterministic player: feedback decisions in, audio events and mixed PCM
//! out, on synthetic banks. Nothing here opens an audio device.

mod harness;

use braintellect_linux::audio::bank::Bank;
use braintellect_linux::audio::player::{AudioEvent, PlayError, Player};
use harness::synthetic_bank;

/// Notes 40, 41 and 84 at 8 kHz, four frames each.
fn bank() -> Bank {
    Bank::parse(&synthetic_bank::bank(&[
        (41, synthetic_bank::wav(8000, &[1, 2, 3, 4])),
        (40, synthetic_bank::wav(8000, &[10, 20, 30, 40])),
        (84, synthetic_bank::wav(8000, &[-5, -6, -7, -8])),
    ]))
    .expect("bank")
}

#[test]
fn a_block_without_a_trigger_is_silent() {
    let bank = bank();
    let mut player = Player::new(&bank, 8000, 1000);
    assert_eq!(player.feed(0, 1000, None).expect("feed"), None);
    assert!(player.events().is_empty());
    assert_eq!(player.mix(0, 16), vec![0; 16]);
}

#[test]
fn a_trigger_plays_the_notes_whole_sample_from_the_blocks_time() {
    let bank = bank();
    let mut player = Player::new(&bank, 8000, 1000);
    let event = player
        .feed(7, 1500, Some(40))
        .expect("feed")
        .expect("event");
    // 500 ms after the origin at 8 kHz is frame 4000.
    assert_eq!(
        event,
        AudioEvent {
            block: 7,
            time_ms: 1500,
            note: 40,
            sample_index: 4000,
            len: 4
        }
    );
    assert_eq!(player.events(), [event]);
    assert_eq!(player.mix(3998, 8), [0, 0, 10, 20, 30, 40, 0, 0]);
}

#[test]
fn overlapping_notes_mix() {
    let bank = bank();
    let mut player = Player::new(&bank, 8000, 0);
    player.feed(0, 0, Some(40)).expect("feed");
    player.feed(1, 0, Some(41)).expect("feed");
    assert_eq!(player.mix(0, 6), [11, 22, 33, 44, 0, 0]);
    assert_eq!(player.events().len(), 2);
}

#[test]
fn mixing_saturates_instead_of_wrapping() {
    let loud = synthetic_bank::bank(&[(40, synthetic_bank::wav(8000, &[30000, -30000]))]);
    let bank = Bank::parse(&loud).expect("bank");
    let mut player = Player::new(&bank, 8000, 0);
    player.feed(0, 0, Some(40)).expect("feed");
    player.feed(1, 0, Some(40)).expect("feed");
    assert_eq!(player.mix(0, 2), [i16::MAX, i16::MIN]);
}

#[test]
fn a_note_the_bank_lacks_is_an_error_and_plays_nothing() {
    let bank = bank();
    let mut player = Player::new(&bank, 8000, 0);
    assert!(matches!(
        player.feed(0, 0, Some(60)),
        Err(PlayError::UnknownNote(60))
    ));
    assert!(player.events().is_empty());
}

#[test]
fn a_sample_is_resampled_to_the_output_rate() {
    let bank = bank();
    let mut player = Player::new(&bank, 16000, 0);
    let event = player.feed(0, 0, Some(40)).expect("feed").expect("event");
    assert_eq!(event.len, 8);
    assert_eq!(player.mix(0, 9), [10, 15, 20, 25, 30, 35, 40, 40, 0]);
}

/// The original's `+0x2cf` check is the design's `playOnChange` (false in
/// SMART-2), so a note equal to the last one sounds again
/// (`audio-feedback.md`, "Playback gating").
#[test]
fn a_repeated_note_plays_again() {
    let bank = bank();
    let mut player = Player::new(&bank, 8000, 0);
    player.feed(0, 0, Some(40)).expect("feed").expect("first");
    player
        .feed(1, 500, Some(40))
        .expect("feed")
        .expect("repeat");
    assert_eq!(player.events().len(), 2);
}
