//! The clone against the original on a replayed stream (ticket #48).
//!
//! The original consumed a recorded stream served by `braintellect-linux emulate`; its hook
//! log is reduced by `scripts/replay-extract.py` to `meta.txt`, `eeg.txt`, `blocks.tsv`,
//! `amps.tsv`, `decisions.tsv` and, for logs with the Evaluator hooks (#53), `evaluator.tsv`.
//! This test feeds the clone the consumed EEG from the original's logged graph-start sample,
//! with the original's counter clock reads and 16-tap-mean flags (as
//! `parity_smart2_exercise.rs` does for run 8), and reports where its decisions first differ
//! from the original's on that replay. The graph start, the 45 Hz switch and the A..H band to
//! band-pass object ranks are not logged; all three are located from the original's logged
//! A..H amplitudes. The replay then runs twice: under the original's logged Evaluator codes
//! (the decisions and amplitudes are asserted) and under the clone's own Evaluator (its
//! Balance and Coherence values against `evaluator.tsv` bit for bit, and its code of every
//! block, are asserted).
//!
//! Ignored: the inputs are vendor-derived and live outside Git. Run with
//! `cargo test --test replay_s48 -- --ignored --nocapture`; `S48_REPLAY_DIR` overrides the
//! default directory (`tmp/s48/replay` at the repository root).

mod harness;

use braintellect_linux::feedback::{Mode, BLOCK_LEN};
use harness::fixtures::Run8Block;
use harness::run8::{deviation, replay_from, Codes, Replay};
use harness::{assertions, inputs};

const DIR_VAR: &str = "S48_REPLAY_DIR";
const DIR_DEFAULT: &str = "tmp/s48/replay";

fn read(name: &str) -> String {
    inputs::read(DIR_VAR, DIR_DEFAULT, name)
}

/// Graph-call objects whose 16-tap-mean flags `blocks.tsv` holds, in address-rank order.
const OBJECTS: usize = 18;
/// Blocks over which each A..H rank must reproduce the logged amplitudes bit for bit.
const RANK_BLOCKS: usize = 300;

struct Meta {
    /// Consumed volts logged before the first block closed.
    logged_count: usize,
    /// Consumed index of `eeg.txt` line 0.
    eeg_base: usize,
    /// Run 8's band A..H ranks, which seed the start search.
    run8_band_ranks: [usize; 8],
}

fn meta() -> Meta {
    let text = read("meta.txt");
    let values = |key: &str| -> Vec<usize> {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.split(' ').map(|n| n.parse().expect("number")).collect())
            .unwrap_or_else(|| panic!("meta.txt: {key}<n>"))
    };
    Meta {
        logged_count: values("logged_count ")[0],
        eeg_base: values("eeg_base ")[0],
        run8_band_ranks: values("run8_band_ranks ")
            .try_into()
            .expect("eight band ranks"),
    }
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

/// A logged block, with the 16-tap-mean flags of all graph-call objects by address rank.
struct LoggedBlock {
    flags: [bool; OBJECTS],
    block: Run8Block,
}

fn blocks() -> Vec<LoggedBlock> {
    read("blocks.tsv")
        .lines()
        .map(|line| {
            let c: Vec<&str> = line.split('\t').collect();
            let lap: i64 = c[3].parse().expect("lap");
            assert_eq!(
                c[0].len(),
                OBJECTS,
                "blocks.tsv: one flag per graph-call object"
            );
            LoggedBlock {
                flags: std::array::from_fn(|r| c[0].as_bytes()[r] == b'1'),
                block: Run8Block {
                    scalar_heads: [false; 8],
                    control: c[1].parse().expect("code"),
                    read_ms: c[2].parse().expect("read"),
                    lap_ms: (lap >= 0).then_some(lap),
                },
            }
        })
        .collect()
}

/// The first `limit` blocks with band A..H's flags taken from object ranks `ranks`.
fn blocks_under(logged: &[LoggedBlock], ranks: [usize; 8], limit: usize) -> Vec<Run8Block> {
    logged
        .iter()
        .take(limit)
        .map(|l| Run8Block {
            scalar_heads: ranks.map(|r| l.flags[r]),
            ..l.block.clone()
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

/// Band letters in the order of `amps.tsv`.
const BANDS: [&str; 8] = ["A", "B", "C", "D", "E", "F", "G", "H"];

/// Each logged A..H amplitude beside the clone's, as `(block, sample, band, clone, original)`;
/// blocks the original logged no amplitudes for (`-`) are skipped.
fn amplitudes(run: &Replay) -> Vec<(usize, usize, usize, f32, f32)> {
    let mut out = Vec::new();
    for (b, line) in read("amps.tsv").lines().take(run.blocks.len()).enumerate() {
        if line == "-" {
            continue;
        }
        let logged: Vec<f32> = line.split(' ').map(float).collect();
        for band in 0..8 {
            let at = run.band_index[band];
            for n in 0..BLOCK_LEN {
                let got = run.decisions[b].samples[n].bands[at];
                out.push((b, n, band, got, logged[band * BLOCK_LEN + n]));
            }
        }
    }
    out
}

/// The first block whose logged A..H amplitudes the clone does not reproduce bit for bit.
fn first_amplitude_mismatch(run: &Replay) -> Option<String> {
    amplitudes(run)
        .into_iter()
        .find(|&(_, _, _, got, want)| got.to_bits() != want.to_bits())
        .map(|(b, n, band, got, want)| {
            format!(
                "block {b} sample {n} band {}: clone {got:e}, original {want:e}",
                BANDS[band]
            )
        })
}

/// How many of the first blocks' logged A..H amplitudes (32 per block) the clone reproduces bit for bit.
fn amplitude_matches(run: &Replay) -> usize {
    amplitudes(run)
        .iter()
        .filter(|&&(_, _, _, got, want)| got.to_bits() == want.to_bits())
        .count()
}

/// Per band A..H, the object rank whose flags make the clone reproduce every logged amplitude
/// of blocks `0..RANK_BLOCKS` bit for bit. Each band's amplitudes depend only on its own
/// band-pass object's flags, so one replay per rank (that rank for all eight bands) scores all
/// eight. Panics unless exactly one rank fully matches per band.
fn locate_ranks(
    eeg: &[[i32; 4]],
    meta: &Meta,
    logged: &[LoggedBlock],
    graph_start: usize,
    switch: usize,
) -> [usize; 8] {
    // [rank][band] = (matching, logged) values
    let scores: Vec<[(usize, usize); 8]> = (0..OBJECTS)
        .map(|r| {
            let blocks = blocks_under(logged, [r; 8], RANK_BLOCKS);
            let run = replay_at(
                eeg,
                meta.eeg_base,
                blocks,
                graph_start,
                switch,
                Codes::Logged,
            );
            let mut score = [(0, 0); 8];
            for (_, _, band, got, want) in amplitudes(&run) {
                score[band].0 += usize::from(got.to_bits() == want.to_bits());
                score[band].1 += 1;
            }
            score
        })
        .collect();
    std::array::from_fn(|band| {
        let full: Vec<usize> = (0..OBJECTS)
            .filter(|&r| scores[r][band].1 > 0 && scores[r][band].0 == scores[r][band].1)
            .collect();
        assert!(
            full.len() == 1,
            "band {}: {} ranks reproduce the logged amplitudes of blocks 0..{RANK_BLOCKS} bit for \
             bit, want exactly one; (matching, logged) by rank: {:?}",
            BANDS[band],
            full.len(),
            scores.iter().map(|s| s[band]).collect::<Vec<_>>()
        );
        full[0]
    })
}

/// The replay of `blocks` with `graph_start` as the graph's first consumed sample and the
/// 45 Hz low-pass switched in at consumed sample `interference_switch`.
fn replay_at(
    eeg: &[[i32; 4]],
    eeg_base: usize,
    blocks: Vec<Run8Block>,
    graph_start: usize,
    interference_switch: usize,
    codes: Codes,
) -> Replay {
    // The original ran Source from its first streamed sample, so the warm-up is everything
    // before the graph start.
    let frames: Vec<[i32; 8]> = eeg
        .chunks_exact(2)
        .map(|pair| std::array::from_fn(|k| pair[k / 4][k % 4]))
        .collect();
    replay_from(
        &frames,
        eeg_base,
        graph_start,
        interference_switch,
        blocks,
        Mode::Original,
        codes,
    )
}

/// The logged Evaluator series inputs per block (`evaluator.tsv`): the four Balance values,
/// then the two Coherence values; `None` where the log has none.
fn evaluator_series() -> Vec<Option<[f32; 6]>> {
    read("evaluator.tsv")
        .lines()
        .map(|line| {
            (line != "-").then(|| {
                let words: Vec<f32> = line.split(' ').map(float).collect();
                words.try_into().expect("six series")
            })
        })
        .collect()
}

#[test]
#[ignore = "reads the s48 replay extract outside Git"]
fn clone_decisions_on_the_replayed_stream_against_the_originals() {
    let meta = meta();
    let logged_count = meta.logged_count;
    let eeg = eeg();
    let logged = blocks();
    // The app converts samples ahead of the graph, so the logged count is an upper bound on
    // the graph start (run 8: count 25912, start 25600). Locate it from the original's logged
    // A..H amplitudes. The switch (`setInterferenceFilter`, run by the UI thread at Play) is not
    // logged either and lands at a different sample against the processing thread each run
    // (run 8: start + 1, s48: start + 4, s53: start), so it is located by the most bit-exact
    // values of blocks 0..7 once the start is known. Both searches run under run 8's band ranks:
    // a wrong rank costs at most one float32 ulp on a block's scalar-head samples, so the best
    // start and switch still stand out, and the A..H ranks located afterwards must then match
    // every logged amplitude, which a wrong start or switch would not.
    let seed = blocks_under(&logged, meta.run8_band_ranks, 8);
    // The start comes first, from block 0's first sample: the bands start cold there, so the
    // sample depends on the start alone (the switch lands at or after it). The right start leaves
    // only rank-order ulps; any other leaves a relative error near one.
    let mut best: Vec<(f64, usize)> = (logged_count - 1900..=logged_count + 400)
        .map(|s| {
            let run = replay_at(
                &eeg,
                meta.eeg_base,
                seed[..1].to_vec(),
                s,
                s + 4,
                Codes::Logged,
            );
            let error = amplitudes(&run)
                .iter()
                .filter(|a| a.1 == 0)
                .map(|&(_, _, _, got, want)| f64::from((got - want).abs() / want.abs()))
                .sum();
            (error, s)
        })
        .collect();
    best.sort_unstable_by(|a, b| a.partial_cmp(b).expect("finite"));
    println!(
        "logged count {logged_count}; best (summed relative error of block 0 sample 0, start): {:?}",
        &best[..3]
    );
    let graph_start = best[0].1;
    let mut switches: Vec<(usize, usize)> = (graph_start - 16..=graph_start + 16)
        .map(|s| {
            (
                amplitude_matches(&replay_at(
                    &eeg,
                    meta.eeg_base,
                    seed.clone(),
                    graph_start,
                    s,
                    Codes::Logged,
                )),
                s,
            )
        })
        .collect();
    switches.sort_unstable_by(|a, b| b.cmp(a));
    println!(
        "best (matching values of 256, switch): {:?}",
        &switches[..3]
    );
    let switch = switches[0].1;
    let ranks = locate_ranks(&eeg, &meta, &logged, graph_start, switch);
    println!(
        "A..H band-pass object ranks {ranks:?} (run 8's {:?})",
        meta.run8_band_ranks
    );
    let run = replay_at(
        &eeg,
        meta.eeg_base,
        blocks_under(&logged, ranks, usize::MAX),
        graph_start,
        switch,
        Codes::Logged,
    );
    println!(
        "graph start {graph_start}, switch {switch}, blocks {}",
        run.blocks.len()
    );
    let mismatch = first_amplitude_mismatch(&run);
    println!(
        "first logged A..H amplitude mismatch: {}",
        mismatch.as_deref().unwrap_or("none (bit for bit)")
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
    // The live sum-order policies against the logged order (`audio-feedback.md`, "Live sum
    // order").
    for (name, scalar_head) in [("all-SIMD", false), ("all-scalar-head", true)] {
        let policy: Vec<Run8Block> = blocks_under(&logged, ranks, usize::MAX)
            .into_iter()
            .map(|b| Run8Block {
                scalar_heads: [scalar_head; 8],
                ..b
            })
            .collect();
        let other = replay_at(
            &eeg,
            meta.eeg_base,
            policy,
            graph_start,
            switch,
            Codes::Logged,
        );
        println!("sum order {name}: {:?}", deviation(&run, &other));
    }
    assert_eq!(mismatch, None, "logged A..H amplitudes differ");
    assert!(
        first.is_none(),
        "the clone's decisions differ from the original's on the replay"
    );

    // The clone's own Evaluator, on the same replay.
    let evaluated = replay_at(
        &eeg,
        meta.eeg_base,
        blocks_under(&logged, ranks, usize::MAX),
        graph_start,
        switch,
        Codes::Evaluated,
    );
    assert_evaluator(&evaluated);
}

/// The clone's Evaluator against the original's: every logged Balance and Coherence value
/// bit for bit (when the log has them), and the code of every block.
fn assert_evaluator(run: &Replay) {
    if inputs::path(DIR_VAR, DIR_DEFAULT)
        .join("evaluator.tsv")
        .exists()
    {
        let series = evaluator_series();
        let (mut logged, mut clone) = (Vec::new(), Vec::new());
        for (decision, values) in run.decisions.iter().zip(&series) {
            let Some(values) = values else { continue };
            logged.extend(values);
            clone.extend(decision.balances.iter().chain(&decision.coherences));
        }
        // Six values per logged block: four Balance series, then two Coherence series.
        assertions::float32_bits("Evaluator value", &logged, &clone)
            .expect("Balance and Coherence values bit for bit");
        println!(
            "Balance and Coherence values bit for bit: {} blocks",
            logged.len() / 6
        );
        let coherent: Vec<usize> = (0..series.len())
            .filter(|&b| series[b].is_some_and(|s| s[4].abs() > 0.80 || s[5].abs() > 0.80))
            .collect();
        println!(
            "blocks with a Coherence value above the threshold: {} (first {:?})",
            coherent.len(),
            coherent.first()
        );
    }
    let codes = |control: &dyn Fn(usize) -> u32| (0..run.decisions.len()).map(control).collect();
    let logged: Vec<u32> = codes(&|b| run.blocks[b].control);
    let clone: Vec<u32> = codes(&|b| run.decisions[b].control);
    println!("codes: {:?}", code_runs(&logged));
    assertions::control_codes(&logged, &clone).expect("per-block Evaluator code parity");
}

/// `(first block, code)` of each run of equal codes.
fn code_runs(codes: &[u32]) -> Vec<(usize, u32)> {
    codes
        .iter()
        .enumerate()
        .filter(|&(b, &c)| b == 0 || codes[b - 1] != c)
        .map(|(b, &c)| (b, c))
        .collect()
}
