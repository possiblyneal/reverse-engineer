//! The clone against the original on all four channels over a whole replay (ticket #55).
//!
//! The original consumed run 8's no-wearer stream served by `braintellect-linux emulate`, with
//! `hook13.js` logging its converted volts for channels 0 to 3. `scripts/consumed-extract.py`
//! reduces that log to `frames.txt` (the frames the original consumed, one per line) and
//! `volts.tsv` (the logged volts per consumed sample, as `tests/fixtures/consumed_volts.tsv`).
//! This test decodes the frames and holds all four channels to the logged values bit for bit
//! over the whole range; a mismatch names the channel and its first sample.
//!
//! Ignored: the inputs stay outside Git. Run with
//! `cargo test --test replay_s55 -- --ignored --nocapture`; `S55_REPLAY_DIR` overrides the
//! default directory (`tmp/s55/replay` at the repository root).

mod harness;

use std::fs;
use std::path::PathBuf;

use harness::{assertions, channels, fixtures};

fn read(name: &str) -> String {
    let dir = std::env::var_os("S55_REPLAY_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/s55/replay"),
        PathBuf::from,
    );
    let path = dir.join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
#[ignore = "inputs live outside Git (tmp/s55/replay)"]
fn all_four_channels_match_the_original_over_the_whole_replay() {
    let frames: Vec<Vec<u8>> = read("frames.txt")
        .lines()
        .map(|l| {
            (0..l.len() / 2)
                .map(|i| u8::from_str_radix(&l[2 * i..2 * i + 2], 16).expect("hex"))
                .collect()
        })
        .collect();
    let expected = fixtures::consumed_microvolts_from(&read("volts.tsv"));
    let (actual, stats) = channels::decode(frames);
    println!("{} samples per channel, {stats:?}", expected[0].len());
    assertions::channel_microvolts(&expected, &actual).expect("all-channel microvolts parity");
}
