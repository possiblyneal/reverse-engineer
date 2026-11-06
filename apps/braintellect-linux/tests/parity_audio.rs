//! Parity of the audio layer on the run-8 exercise: the Trigger and note
//! decisions the clone's feedback graph makes from the decoded EEG, played
//! through the player, start exactly the notes the baseline sounded (415 of
//! them over about four minutes): same block, MIDI note, start sample and length.
//!
//! The bank is synthetic and shaped like bank 1002 (96 kHz mono, 0.8 s per note);
//! the vendor samples are never in Git, so the test asserts which note starts
//! when and for how long, not the sample values.

mod harness;

use braintellect_linux::audio::player::Player;

use harness::audio::{self, RATE_HZ};
use harness::run8::replay;
use harness::{assertions, fixtures};

#[test]
fn the_player_starts_the_notes_the_baseline_sounded() {
    let run = replay();
    let bank = audio::bank_1002_shaped();
    let mut player = Player::new(&bank, RATE_HZ, run.blocks[0].read_ms);
    for (block, (decision, timing)) in run.decisions.iter().zip(&run.blocks).enumerate() {
        player
            .feed(block, timing.read_ms, decision.note_at_trigger)
            .expect("every note the graph selects is in the bank");
    }
    let expected = audio::run8_events();
    assert_eq!(expected.len(), 415);
    assertions::audio_events(&expected, player.events()).expect("audio parity");
}

/// The baseline's own gate: notes are at least 500 ms apart (the counter's
/// interval), so with 0.8 s notes at most two ring at once, and they mix.
#[test]
fn consecutive_baseline_notes_are_at_least_one_counter_interval_apart() {
    let events = audio::run8_events();
    assert!(events
        .windows(2)
        .all(|w| w[1].time_ms - w[0].time_ms >= 500));
    assert!(fixtures::run8_blocks().len() > events.len());
}
