#!/usr/bin/env python3
"""Extract the clone's replay inputs from a hook log of the original fed by `emulate`.

Reads a run-8 style binary hook log (`hook11.js` output, written while the original
consumed a stream replayed by `braintellect-linux emulate`; see
`docs/reverse-engineering/braintellect2/replay-rig.md`) and writes, next to it or to
`--out`, the files `tests/replay_s48.rs` reads:

- `meta.txt`: `logged_count <consumed ch0 volts logged before the first block closed>`,
  `eeg_base <consumed sample index of the first line of eeg.txt>` and
  `run8_band_ranks <A..H>`. The count is not the graph start: in run 8 it was 25912 where the
  graph's first sample was 25600 (the app converts samples ahead of the graph).
- Blocks: the complete logged prefix, every block up to the first without a clock read, at most
  30001 (the exercise), printed as `blocks <n> of 30001`. A replay of a live capture stops short
  of the exercise end when the capture runs out (`replay-rig.md`, #33).
- `eeg.txt`: the consumed EEG, one sample per line as four 24-bit hex values (channels 0..3),
  from `eeg_base` through `n` blocks after the logged count (or the last consumed sample).
- `blocks.tsv`: per block `flags<TAB>Evaluator code<TAB>read ms<TAB>lap ms`, clocks in ms from
  block 0. `flags` holds the 16-tap-mean flag of all 18 graph-call objects, in rank order (the
  rank of the object's address among the 18). Which rank is which band differs per process, so
  the test locates the A..H ranks from the logged amplitudes. Logs from before #53 have no
  A..D amplitudes; then run 8's (`run8_band_ranks`) stand in for A..D.
- `amps.tsv`: per block the logged A..H band amplitudes as 32 float32 hex words (A..D then
  E..H, four samples each; the `run8_amplitudes_*.tsv` fixtures hold E..H only), or `-` where
  the log has none. A..D are logged from `hook12.js` on (#53); `recs[0:4]` of the tag-11 record.
- `evaluator.tsv` (only when the log has the Evaluator hooks of #53): per block the latest
  value of the Evaluator's series after the block as six float32 hex words: Balance Ch1/2
  Band 10, Balance Ch1/2 Band 80, Balance Ch3/4 Band 10, Balance Ch3/4 Band 80, Coherence
  Ch3/4 Band 10, Coherence Ch3/4 Band 80.
- `decisions.tsv`: the original's Trigger and Note decisions (as `run8_decisions.tsv`).

Nothing written here is committed: the output stays under the repo's `tmp/`.
Usage: python3 -I replay-extract.py <run8.bin> [--out DIR]
"""
import argparse
import importlib.util
import pathlib
import struct

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("make_fixtures", HERE / "make-fixtures.py")
mf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mf)

EEG_BASE = 0


def consumed_samples(frames, volts):
    """The 4-channel raw sample for each consumed ch0 volt, matched forward into the frames."""
    # candidate stream: slots 0..4 then 4..8 of every frame, in order
    seq = []
    for f in frames:
        seq.append((f[0], [f[0], f[1], f[2], f[3]]))
        seq.append((f[4], [f[4], f[5], f[6], f[7]]))
    out, pos = [], 0
    for v in volts:
        raw = round(v * mf.RUN8_DIV)
        q = pos
        while q < min(len(seq), pos + 400) and seq[q][0] != raw:
            q += 1
        if q >= min(len(seq), pos + 400):
            raise SystemExit("consumed sample has no received counterpart")
        out.append(seq[q][1])
        pos = q + 1
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log", type=pathlib.Path)
    ap.add_argument("--out", type=pathlib.Path)
    args = ap.parse_args()
    out = args.out or args.log.parent / "replay"
    out.mkdir(parents=True, exist_ok=True)

    records = list(mf.run8_records(args.log))
    frames, volts = mf.run8_frames(records)
    logged_count = 0  # consumed volts logged before the first block closed
    for tag, _ in records:
        if tag == 2:
            break
        if tag == 12:
            logged_count += 1
    samples = consumed_samples(frames, volts)
    blocks = mf.run8_blocks(records)[:mf.RUN8_BLOCKS]
    blocks = blocks[:next((b for b, blk in enumerate(blocks) if blk["clock"] is None), len(blocks))]
    if not blocks:
        raise SystemExit("no complete block logged")
    keys = sorted({k for blk in blocks for k in blk["calls"]})
    if len(keys) != 18:
        raise SystemExit("expected 18 graph-call objects")

    first = EEG_BASE
    last = logged_count + len(blocks) * 4
    window = samples[first:last]
    lines = ["".join("%06x" % (x & 0xFFFFFF) for x in sample) + "\n" for sample in window]
    (out / "eeg.txt").write_text("".join(lines))
    (out / "meta.txt").write_text(f"logged_count {logged_count}\neeg_base {EEG_BASE}\n"
        f"run8_band_ranks {' '.join(map(str, mf.RUN8_BAND_RANKS))}\n")

    t0 = blocks[0]["clock"]
    rows, decs = [], []
    for b, blk in enumerate(blocks):
        flags = "".join("1" if blk["calls"].get(k) else "0" for k in keys)
        now = blk["clock"] - t0
        lap = blk["lap"] - t0 if blk["lap"] is not None else -1
        rows.append(f"{flags}\t{blk['code']}\t{now}\t{lap}\n")
        if blk["lap"] is not None:
            decs += [f"{now}\tTrigger\t{b}\n", f"{now}\tNote\t{blk['note']}\n"]
    def words(values):
        return " ".join("%08x" % struct.unpack("<I", struct.pack("<f", x))[0] for x in values)

    amps = ["-\n" if blk["amps"] is None else words(blk["amps_ad"] + blk["amps"]) + "\n"
            for blk in blocks]
    (out / "amps.tsv").write_text("".join(amps))
    series = mf.run8_evaluator_series(records)
    if series:
        order = ["Bal1n2B10", "Bal1n2B80", "Bal3n4B10", "Bal3n4B80", "Coh3n4B10", "Coh3n4B80"]
        evals = ["-\n" if any(b not in series[n] for n in order) else
                words([series[n][b] for n in order]) + "\n" for b in range(len(blocks))]
        (out / "evaluator.tsv").write_text("".join(evals))
    (out / "blocks.tsv").write_text("".join(rows))
    (out / "decisions.tsv").write_text("".join(decs))
    print(f"logged_count {logged_count}, frames {len(frames)}, consumed {len(volts)}, "
          f"blocks {len(blocks)} of {mf.RUN8_BLOCKS}, eeg samples {len(window)}, fires {len(decs) // 2}")


if __name__ == "__main__":
    main()
