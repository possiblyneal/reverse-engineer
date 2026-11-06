#!/usr/bin/env python3
"""Extract the clone's replay inputs from a hook log of the original fed by `emulate`.

Reads a run-8 style binary hook log (`hook11.js` output, written while the original
consumed a stream replayed by `braintellect-linux emulate`; see
`docs/reverse-engineering/braintellect2/replay-rig.md`) and writes, next to it or to
`--out`, the files `tests/replay_s48.rs` reads:

- `meta.txt`: `logged_count <consumed ch0 volts logged before the first block closed>` and
  `eeg_base <consumed sample index of the first line of eeg.txt>`. The count is not the graph
  start: in run 8 it was 25912 where the graph's first sample was 25600 (the app converts
  samples ahead of the graph).
- `eeg.txt`: the consumed EEG, one sample per line as four 24-bit hex values (channels 0..3), from `eeg_base` through the end of the exercise
  (30001 blocks after the logged count).
- `blocks.tsv`: per block `flags<TAB>Evaluator code<TAB>read ms<TAB>lap ms` (as the
  `run8_blocks_*.tsv` fixtures), clocks in ms from block 0.
- `amps.tsv`: per block the logged E..H band amplitudes as 16 float32 hex words (as the
  `run8_amplitudes_*.tsv` fixtures), or `-` where the log has none.
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

BLOCKS = mf.RUN8_BLOCKS
EEG_BASE = 20000


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
    blocks = mf.run8_blocks(records)
    if len(blocks) < BLOCKS or any(b["clock"] is None for b in blocks[:BLOCKS]):
        raise SystemExit("block log is incomplete")
    blocks = blocks[:BLOCKS]
    keys = sorted({k for blk in blocks for k in blk["calls"]})
    if len(keys) != 18:
        raise SystemExit("expected 18 graph-call objects")

    first = EEG_BASE
    last = logged_count + BLOCKS * 4
    window = samples[first:last]
    lines = ["".join("%06x" % (x & 0xFFFFFF) for x in sample) + "\n" for sample in window]
    (out / "eeg.txt").write_text("".join(lines))
    (out / "meta.txt").write_text(f"logged_count {logged_count}\neeg_base {EEG_BASE}\n")

    t0 = blocks[0]["clock"]
    rows, decs = [], []
    for b, blk in enumerate(blocks):
        flags = "".join("1" if blk["calls"].get(keys[r]) else "0" for r in mf.RUN8_BAND_RANKS)
        now = blk["clock"] - t0
        lap = blk["lap"] - t0 if blk["lap"] is not None else -1
        rows.append(f"{flags}\t{blk['code']}\t{now}\t{lap}\n")
        if blk["lap"] is not None:
            decs += [f"{now}\tTrigger\t{b}\n", f"{now}\tNote\t{blk['note']}\n"]
    amps = ["-\n" if blk["amps"] is None else
            " ".join("%08x" % struct.unpack("<I", struct.pack("<f", x))[0] for x in blk["amps"]) + "\n"
            for blk in blocks]
    (out / "amps.tsv").write_text("".join(amps))
    (out / "blocks.tsv").write_text("".join(rows))
    (out / "decisions.tsv").write_text("".join(decs))
    print(f"logged_count {logged_count}, frames {len(frames)}, consumed {len(volts)}, "
          f"blocks {len(blocks)}, fires {len(decs) // 2}")


if __name__ == "__main__":
    main()
