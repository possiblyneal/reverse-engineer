#!/usr/bin/env python3
"""Reduce a ticket-61 hook log (`g/s61.js`) of the original on a worn B-2 capture to the files
`tests/replay_s61.rs` reads.

The log (`s61.bin`) was written while the original, fed the capture by `braintellect-linux
emulate`, ran its Optimize-tab sensor status (`docs/reverse-engineering/braintellect2/
sensor-status.md`, "Worn replays"). Tags: 12 consumed sample `[12, 6, ts, channel, count, uV]`,
32 Head2D level update (four per block), 41 overlay state `[41, n, ts, state]`.

Writes to `--out` (default the log's directory):

- `worn.tsv`: per block, the state the overlay shows when the next block arrives (the last
  state set so far, "Adjust Fit", "Ready" or "Not Available") and each channel's line-noise
  level and 12, 40 and 70 Hz band levels as `float32` bit patterns in hex.
- `skipped.txt`: the capture's stream frames the original never consumed, one frame index
  (counted from the first frame after the 22 handshake replies) per line. The original's host
  skips received frames; the capture has no counter gap or failed validation there, so the
  skips are found by matching the consumed channel-0 samples to the capture's frames.

Nothing written here is committed: the output stays under the repo's `tmp/`.
Usage: python3 -I worn-sensor-extract.py <s61.bin> <capture.rx> [--out DIR]
"""
import argparse
import array
import pathlib
import statistics
import struct

HANDSHAKE_LEN = 22 * 56
MARKER = bytes.fromhex("acbccc03")
# A consumed sample is the capture's count times a constant, to within a few counts.
COUNT_TOLERANCE = 40
STATES = ["Not Available", "Adjust Fit", "Ready"]


def records(path):
    raw = path.read_bytes()
    a = array.array("d")
    a.frombytes(raw[: len(raw) // 8 * 8])
    i = 0
    while i + 3 <= len(a):
        tag, size = int(a[i]), int(a[i + 1])
        if size < 3 or i + size > len(a):
            break
        yield tag, a[i : i + size]
        i += size


def stream_frames(capture):
    data = capture.read_bytes()[HANDSHAKE_LEN:]
    frames, at = [], 0
    while True:
        start = data.find(MARKER, at)
        if start < 0 or start + 32 > len(data):
            return frames
        frame = data[start : start + 32]
        at = start + 4
        if frame[6:8] == b"\x03\x03" and all(frame[8 + 3 * k + 2] & 3 == 3 for k in range(8)):
            frames.append(frame)
            at = start + 32


def count(frame, n):
    """Channel 0's sample `n` (0 or 1) of `frame`, in ADC counts."""
    slot = 4 * n
    return int.from_bytes(frame[8 + 3 * slot : 11 + 3 * slot], "big", signed=True) >> 8


def skipped(consumed, frames):
    """Frame indices the consumed channel-0 samples (volts) leave out."""
    counts = [count(f, n) for f in frames for n in range(2)]
    pairs = [(v, c) for v, c in zip(consumed[:2000], counts) if abs(c) > 1000]
    scale = statistics.median(v / c for v, c in pairs)
    matches = lambda v, c: abs(v / scale - c) <= COUNT_TOLERANCE
    out, shift, j = [], 0, 0
    while j < len(consumed):
        if j + shift < len(counts) and matches(consumed[j], counts[j + shift]):
            j += 1
            continue
        for skip in range(2, 200, 2):
            if j + shift + skip < len(counts) and matches(consumed[j], counts[j + shift + skip]):
                break
        else:
            raise SystemExit(f"consumed sample {j} matches no frame after {shift} skipped samples")
        first = (j + shift) // 2
        out += range(first, first + skip // 2)
        shift += skip
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log", type=pathlib.Path)
    ap.add_argument("capture", type=pathlib.Path)
    ap.add_argument("--out", type=pathlib.Path)
    args = ap.parse_args()
    out = args.out or args.log.parent
    consumed, heads, state, blocks = [], [], None, []
    for tag, r in records(args.log):
        if tag == 12 and int(r[3]) == 0:
            consumed.append(r[5])
        elif tag == 41:
            state = STATES[int(r[4])]
        elif tag == 32:
            if len(heads) % 4 == 0:
                # The state now is the state when the previous block's interval ended.
                if blocks:
                    blocks[-1]["state"] = state
                blocks.append({"state": None, "levels": []})
            heads.append(r)
            blocks[-1]["levels"].append([r[9], *r[12:15]])
    if blocks[-1]["state"] is None:
        blocks[-1]["state"] = state
    blocks = [b for b in blocks if len(b["levels"]) == 4]
    f32 = lambda x: struct.unpack("<I", struct.pack("<f", x))[0]
    out.mkdir(parents=True, exist_ok=True)
    with (out / "worn.tsv").open("w") as f:
        for n, b in enumerate(blocks):
            hexes = [f"{f32(v):08x}" for ch in b["levels"] for v in ch]
            f.write("\t".join([str(n), b["state"] or "-", *hexes]) + "\n")
    skips = skipped(consumed, stream_frames(args.capture))
    (out / "skipped.txt").write_text("".join(f"{i}\n" for i in skips))
    print(f"blocks {len(blocks)} consumed {len(consumed)} skipped frames {len(skips)}")


if __name__ == "__main__":
    main()
