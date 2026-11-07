#!/usr/bin/env python3
"""Extract a BAGSD run's consumed volts and `DSPFFT::process` calls from the original's hook log.

Reads the binary log of the ticket-59 hook (`hook13.js` plus the `DSPFFT::process` tags 30 and
31) written while the original, fed by `braintellect-linux emulate`, stored an exercise and then
ran its BAGSD design (`docs/reverse-engineering/braintellect2/replay-rig.md`). Tag 30 is
`[30, length, ts, this, in, scale, out, N, +0x50, +0x54, window, +0x3c, input at entry[N],
input at return[N], output[N/2]]`. It writes to `--out` (default `tmp/s59/replay/` at the
repository root) the files `tests/replay_s59.rs` reads:

- `volts.tsv`: per consumed sample, the logged volts of channels 0 to 3 as four float64 bit
  patterns in hex (as `consumed-extract.py`).
- `fft_in_<c>.txt` and `fft_out_<c>.txt`: per call of the BAGSD design's FFT object of
  channel `c`, in call order, the N input samples at entry and the N/2 output magnitudes, one
  call per line, each a `float32` value widened to a float64 `repr`. The BAGSD objects are the
  four that call last, in order of first appearance in the log.

Nothing written here is committed: the output stays under the repo's `tmp/`.
Usage: python3 -I fft-extract.py <s59.bin> [--out DIR]
"""
import argparse
import importlib.util
import mmap
import pathlib
import struct

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("make_fixtures", HERE / "make-fixtures.py")
mf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mf)

CHANNELS = 4


def fft_calls(path):
    """Per `DSPFFT` object (key: its address), its tag-30 records in log order."""
    calls = {}
    with open(path, "rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as m:
        i, n = 0, len(m) // 8
        while i + 3 <= n:
            tag, length = (int(x) for x in struct.unpack_from("<2d", m, 8 * i))
            if length < 3 or i + length > n:
                break
            if tag == 30:
                rec = struct.unpack_from("<%dd" % length, m, 8 * i)
                calls.setdefault(int(rec[3]), []).append(rec)
            i += length
    return calls


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log", type=pathlib.Path)
    ap.add_argument("--out", type=pathlib.Path,
                    default=pathlib.Path(__file__).resolve().parents[3] / "tmp/s59/replay")
    args = ap.parse_args()
    _, rows = mf.s55_consumed(args.log)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "volts.tsv").write_text("".join(mf.s55_row(r) for r in rows))
    calls = fft_calls(args.log)
    bagsd = list(calls)[-CHANNELS:]
    for c, this in enumerate(bagsd):
        with open(args.out / f"fft_in_{c}.txt", "w") as fin, open(args.out / f"fft_out_{c}.txt", "w") as fout:
            for rec in calls[this]:
                n = int(rec[7])
                fin.write(" ".join(repr(x) for x in rec[12:12 + n]) + "\n")
                fout.write(" ".join(repr(x) for x in rec[12 + 2 * n:12 + 2 * n + n // 2]) + "\n")
    print(f"samples {len(rows)} fft calls per channel {[len(calls[t]) for t in bagsd]}")


if __name__ == "__main__":
    main()
