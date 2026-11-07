#!/usr/bin/env python3
"""Extract the all-channel acquisition inputs from a hook log of the original fed by `emulate`.

Reads a `hook13.js` binary log (tag 12: consumed volts of channel 0, tag 13: channels 1 to 3,
tag 1: the serial RX bytes; written while the original consumed a stream replayed by
`braintellect-linux emulate`, see `docs/reverse-engineering/braintellect2/replay-rig.md`) and
writes, to `--out` (default `tmp/s55/replay/` at the repository root), the files
`tests/replay_s55.rs` reads:

- `frames.txt`: the frames the original consumed, one 32-byte frame per line as hex (frames it
  received but never consumed, a counter jump, drop out).
- `volts.tsv`: per consumed sample, the original's logged volts of channels 0 to 3 as four
  float64 bit patterns in hex, as `tests/fixtures/consumed_volts.tsv`.

Nothing written here is committed: the output stays under the repo's `tmp/`.
Usage: python3 -I consumed-extract.py <s55.bin> [--out DIR]
"""
import argparse
import importlib.util
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("make_fixtures", HERE / "make-fixtures.py")
mf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mf)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log", type=pathlib.Path)
    ap.add_argument("--out", type=pathlib.Path,
                    default=pathlib.Path(__file__).resolve().parents[3] / "tmp/s55/replay")
    args = ap.parse_args()
    frames, rows = mf.s55_consumed(args.log)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "frames.txt").write_text("".join(f.hex() + "\n" for f in frames))
    (args.out / "volts.tsv").write_text("".join(mf.s55_row(r) for r in rows))
    print(f"frames {len(frames)} samples {len(rows)}")


if __name__ == "__main__":
    main()
