//! The clone against the original on a replayed stream (ticket #48).
//!
//! The original consumed a recorded stream served by `braintellect-linux emulate`; its hook
//! log is reduced by `scripts/replay-extract.py` to `meta.txt`, `eeg.txt`, `blocks.tsv`,
//! `amps.tsv` and `decisions.tsv`. This test feeds the clone the consumed EEG from the
//! original's logged graph-start sample, with the original's logged Evaluator codes,
//! counter clock reads and 16-tap-mean flags (as `parity_smart2_exercise.rs` does for run 8),
//! and reports where its decisions first differ from the original's on that replay.
//!
//! Ignored: the inputs are vendor-derived and live outside Git. Run with
//! `cargo test --test replay_s48 -- --ignored --nocapture`; `S48_REPLAY_DIR` overrides the
//! default directory (`tmp/s48/replay` in the harlequin workspace).

mod harness;

use std::fs;
use std::path::PathBuf;

use braintellect_linux::feedback::BLOCK_LEN;
use harness::fixtures::Run8Block;
use harness::run8::{replay_from, Replay};

fn dir() -> PathBuf {
    std::env::var_os("S48_REPLAY_DIR").map_or_else(
        || PathBuf::from("/home/neal/orca/workspaces/reverse-engineering/harlequin/tmp/s48/replay"),
        PathBuf::from,
    )
}

fn read(name: &str) -> String {
    let path = dir().join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `meta.txt` as (consumed volts logged before the first block, consumed index of `eeg.txt` line 0).
fn meta() -> (usize, usize) {
    let text = read("meta.txt");
    let value = |key: &str| -> usize {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("meta.txt: {key}<n>"))
    };
    (value("logged_count "), value("eeg_base "))
}

/// The consumed EEG, one four-channel sample per line.
fn eeg() -> Vec<[i32; 4]> {
    read("eeg.txt")
        .lines()
        .map(|line| {
            std::array::from_fn(|k| {
                let v = i32::from_str_radix(&line[6 * k..6 * k + 6], 16).expect("hex");
                (v << 8) >> 8
            })
        })
        .collect()
}

fn blocks() -> Vec<Run8Block> {
    read("blocks.tsv")
        .lines()
        .map(|line| {
            let c: Vec<&str> = line.split('\t').collect();
            let lap: i64 = c[3].parse().expect("lap");
            Run8Block {
                scalar_heads: std::array::from_fn(|k| c[0].as_bytes()[k] == b'1'),
                control: c[1].parse().expect("code"),
                read_ms: c[2].parse().expect("read"),
                lap_ms: (lap >= 0).then_some(lap),
            }
        })
        .collect()
}

/// The original's Trigger (value = block index) and Note decisions as (time, kind, value).
fn original_decisions() -> Vec<(i64, String, u32)> {
    read("decisions.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            let value = if c[1] == "Trigger" {
                1
            } else {
                c[2].parse().expect("note")
            };
            (c[0].parse().expect("time"), c[1].to_owned(), value)
        })
        .collect()
}

fn clone_decisions(run: &Replay) -> Vec<(i64, String, u32)> {
    let mut out = Vec::new();
    for (decision, block) in run.decisions.iter().zip(&run.blocks) {
        if decision.trigger {
            out.push((block.read_ms, "Trigger".to_owned(), 1));
        }
        if let Some(note) = decision.note_at_trigger {
            out.push((
                block.read_ms,
                "Note".to_owned(),
                u32::try_from(note).expect("MIDI notes are non-negative"),
            ));
        }
    }
    out
}

fn float(word: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(word, 16).expect("float32 bits"))
}

/// The first block whose logged E..H amplitudes the clone does not reproduce bit for bit.
fn first_amplitude_mismatch(run: &Replay) -> Option<String> {
    for (b, line) in read("amps.tsv").lines().take(run.blocks.len()).enumerate() {
        if line == "-" {
            continue;
        }
        let logged: Vec<f32> = line.split(' ').map(float).collect();
        for band in 0..4 {
            let at = run.band_index[4 + band];
            for n in 0..BLOCK_LEN {
                let got = run.decisions[b].samples[n].bands[at];
                let want = logged[band * BLOCK_LEN + n];
                if got.to_bits() != want.to_bits() {
                    return Some(format!(
                        "block {b} sample {n} band {}: clone {got:e}, original {want:e}",
                        ["E", "F", "G", "H"][band]
                    ));
                }
            }
        }
    }
    None
}

/// How many of the first blocks' logged E..H amplitudes (16 per block) the clone reproduces bit for bit.
fn amplitude_matches(run: &Replay) -> usize {
    let mut n = 0;
    for (b, line) in read("amps.tsv").lines().take(run.blocks.len()).enumerate() {
        let logged: Vec<f32> = line.split(' ').map(float).collect();
        for band in 0..4 {
            let at = run.band_index[4 + band];
            for k in 0..BLOCK_LEN {
                let got = run.decisions[b].samples[k].bands[at];
                n += usize::from(got.to_bits() == logged[band * BLOCK_LEN + k].to_bits());
            }
        }
    }
    n
}

/// The replay with `graph_start` as the graph's first consumed sample, over the first `limit` blocks.
fn replay_at(
    eeg: &[[i32; 4]],
    eeg_base: usize,
    blocks: &[Run8Block],
    graph_start: usize,
    limit: usize,
) -> Replay {
    let from = graph_start - harness::run8::PREROLL - eeg_base;
    let frames: Vec<[i32; 8]> = eeg[from..]
        .chunks_exact(2)
        .map(|pair| std::array::from_fn(|k| pair[k / 4][k % 4]))
        .collect();
    replay_from(
        &frames,
        graph_start,
        blocks.iter().take(limit).cloned().collect(),
    )
}

#[test]
#[ignore = "reads the s48 replay extract outside Git"]
fn clone_decisions_on_the_replayed_stream_against_the_originals() {
    let (logged_count, eeg_base) = meta();
    let eeg = eeg();
    let blocks = blocks();
    // The app converts samples ahead of the graph, so the logged count is an upper bound on
    // the graph start (run 8: count 25912, start 25600). Locate it from the original's logged
    // E..H amplitudes: the start under which the clone reproduces the most of blocks 0..7.
    let mut best: Vec<(usize, usize)> = (logged_count - 1900..=logged_count + 400)
        .map(|s| {
            (
                amplitude_matches(&replay_at(&eeg, eeg_base, &blocks, s, 8)),
                s,
            )
        })
        .collect();
    best.sort_unstable_by(|a, b| b.cmp(a));
    println!(
        "logged count {logged_count}; best (matching values of 128, start): {:?}",
        &best[..3]
    );
    let graph_start = best[0].1;
    let run = replay_at(&eeg, eeg_base, &blocks, graph_start, usize::MAX);
    println!("graph start {graph_start}, blocks {}", run.blocks.len());
    println!(
        "first logged E..H amplitude mismatch: {}",
        first_amplitude_mismatch(&run).unwrap_or_else(|| "none (bit for bit)".into())
    );
    let want = original_decisions();
    let got = clone_decisions(&run);
    println!(
        "original decisions {}, clone decisions {}",
        want.len(),
        got.len()
    );
    let first = want
        .iter()
        .zip(&got)
        .position(|(w, g)| w != g)
        .or_else(|| (want.len() != got.len()).then_some(want.len().min(got.len())));
    println!(
        "decisions that differ position by position: {} of {}",
        want.iter().zip(&got).filter(|(w, g)| w != g).count(),
        want.len().min(got.len())
    );
    match first {
        None => println!("decisions identical (kind, value, time)"),
        Some(i) => println!(
            "first divergence at decision {i}: original {:?}, clone {:?}",
            want.get(i),
            got.get(i)
        ),
    }
    assert!(
        first.is_none(),
        "the clone's decisions differ from the original's on the replay"
    );
}
