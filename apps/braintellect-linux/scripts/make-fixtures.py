#!/usr/bin/env python3
"""Regenerate the scrubbed parity fixtures from the raw no-wearer captures.

Reads the run-2 capture under `tmp/s26/run2/` (never committed) and writes the
minimal, scrubbed subset the parity-replay harness needs to
`apps/braintellect-linux/tests/fixtures/`.

Scrubbing: the B-2 serial number (the 101 reply's device id, read from the
capture, never hardcoded here) is replaced by a fixed placeholder; the wire
protocol carries no checksum, so nothing is re-derived. The Bluetooth MAC, host
IDs, machine/profile/user names, session/client ids and timestamps never enter
the fixtures: only the wire bytes, a short stream window, decoded-sample
expectations, control decisions, and a handful of record fields are kept.

Usage: python3 -I make-fixtures.py [--src DIR] [--out DIR]
"""
import argparse
import pathlib
import re
import xml.etree.ElementTree as ET

STREAM_FRAMES = 40
FRAME_LEN = 32
MARKER = bytes.fromhex("acbccc03")
SERIAL_PLACEHOLDER = b"SCRUBBEDSN00"  # 12 bytes, same length as the device id
HEADER_KEYS = [
    "version", "design", "montage", "device", "samplesPerSecond",
    "samplesPerPacket", "numberChannels", "deviceGain", "sensorGain",
    "deviceFamily", "deviceModel", "interferenceFilter",
    "deviceHighPassCutoff", "deviceLowPassCutoff", "dataStorageStyle",
]
NS = {"b": "http://optimizer.bst.com/model/base"}
VREF = {11: 2.4, 12: 2.4, 99: 4.0}
FULL_SCALE = {11: 2**15 - 1, 12: 2**23 - 1, 99: 2**23 - 1}


def with_separators(serial: bytes) -> bytes:
    """Lay 12 id bytes out as the reply does: 2-byte pairs each followed by 03."""
    return b"".join(serial[i:i + 2] + b"\x03" for i in range(0, 12, 2))


def read_trace(path):
    """Yield (layer, direction, hex) for each TX/RX line of the Frida trace."""
    for line in path.read_text().splitlines():
        parts = line.split(" ")
        if len(parts) >= 6 and parts[2] in ("TX", "RX"):
            yield parts[2], parts[3] + (" " + parts[4] if parts[3] == "win32" else ""), parts[-1]


def wire_events(trace):
    """BT_Device-level TX/RX up to and including the start command."""
    events = []
    for direction, layer, data in trace:
        if layer == "BT_Device::write" or layer == "BT_Device::readAsync":
            events.append((direction, bytes.fromhex(data)))
            if direction == "TX" and events[-1][1].startswith(b"!*#48,1,"):
                return events
    raise SystemExit("start command not found in trace")


def scrub_serial(events):
    """Replace the 101 reply's device id (the serial number) everywhere."""
    reply = next(
        data for (d, data), (pd, prev) in zip(events[1:], events)
        if d == "RX" and pd == "TX" and prev.startswith(b"!*#101,")
    )
    if reply[8] != 0x65:
        raise SystemExit("101 reply layout changed")
    found = reply[10:28]
    serial = b"".join(found[i:i + 2] for i in range(0, 18, 3))
    scrubbed = [(d, data.replace(found, with_separators(SERIAL_PLACEHOLDER)))
                for d, data in events]
    if serial in b"".join(data for _, data in scrubbed):
        raise SystemExit("serial survived scrubbing")
    return scrubbed


def stream_chunks(trace):
    """RX chunks of the first STREAM_FRAMES frames after the start command."""
    chunks, started, budget = [], False, STREAM_FRAMES * FRAME_LEN
    for direction, layer, data in trace:
        if layer == "BT_Device::write" and bytes.fromhex(data).startswith(b"!*#48,1,"):
            started = True
        if started and layer == "win32 GetOverlappedResult":
            chunk = bytes.fromhex(data)[:budget]
            chunks.append(chunk)
            budget -= len(chunk)
            if budget == 0:
                return chunks
    raise SystemExit("stream window not found")


def i24(b):
    v = (b[0] << 16) | (b[1] << 8) | b[2]
    return v - (1 << 24) if v & 0x800000 else v


def expected_microvolts(chunks, header):
    """Independent (Python) implementation of the documented ADC->volts formula."""
    family = int(header["deviceFamily"])
    divisor = (float(header["deviceGain"]) * FULL_SCALE[family]
               * float(header["sensorGain"]) / VREF[family])
    stream = b"".join(chunks)
    rows = []
    for off in range(0, len(stream), FRAME_LEN):
        frame = stream[off:off + FRAME_LEN]
        assert frame[:4] == MARKER and frame[6:8] == b"\x03\x03", "frame desync"
        counter = int.from_bytes(frame[4:6], "big")
        volts = [i24(frame[8 + 3 * k:11 + 3 * k]) / divisor * 1e6 for k in range(8)]
        rows.append((counter, volts))
    return rows


def decisions(t5):
    out = []
    for m in ET.fromstring(t5).findall("b:controlMessage", NS):
        mm, ss = m.findtext("b:time", namespaces=NS).split(":")
        nums = re.findall(r"\d+", m.findtext("b:message", namespaces=NS))
        out.append((
            (int(mm) * 60 + int(ss)) * 1000,
            m.findtext("b:type", namespaces=NS), nums[0], nums[1],
        ))
    return out


def record(t0, t9, rows):
    header = dict(re.findall(r"<base:(\w+)>([^<]*)</base:\1>", t0))
    out = {f"header.{k}": header[k] for k in HEADER_KEYS}
    out["exercise.duration"] = re.search(r"duration:\s*(\d+)", rows).group(1)
    channels = ET.fromstring(t9).findall(".//b:exerciseDataChannel", NS)
    out["series.channelCount"] = str(len(channels))
    first = channels[0].find(".//b:exerciseDataSeries", NS)
    prefix = f"series.{channels[0].get('channelName')}.{first.get('seriesName')}"
    for i, item in enumerate(first.findall(".//b:dataItem", NS)[:3]):
        out[f"{prefix}.{i}.x"] = item.findtext("b:xValue", namespaces=NS)
        out[f"{prefix}.{i}.y"] = item.findtext("b:yValue", namespaces=NS)
    return out, header


def main():
    root = pathlib.Path(__file__).resolve().parents[3]
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", type=pathlib.Path, default=root / "tmp/s26/run2")
    ap.add_argument(
        "--out", type=pathlib.Path,
        default=root / "apps/braintellect-linux/tests/fixtures",
    )
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    trace = list(read_trace(args.src / "trace.log"))
    events = scrub_serial(wire_events(trace))
    chunks = stream_chunks(trace)
    rec, header = record(
        (args.src / "128-t0.xml").read_text(),
        (args.src / "128-t9.xml").read_text(),
        (args.src / "rows.txt").read_text(errors="replace"),
    )

    (args.out / "wire.txt").write_text(
        "".join(f"{d} {data.hex()}\n" for d, data in events))
    (args.out / "stream.txt").write_text(
        "".join(f"{c.hex()}\n" for c in chunks))
    (args.out / "microvolts.tsv").write_text("".join(
        f"{c}\t" + "\t".join(f"{v:.9f}" for v in vs) + "\n"
        for c, vs in expected_microvolts(chunks, header)))
    (args.out / "decisions.tsv").write_text("".join(
        f"{t}\t{k}\t{o}\t{n}\n"
        for t, k, o, n in decisions((args.src / "128-t5.xml").read_text())))
    (args.out / "record.tsv").write_text(
        "".join(f"{k}\t{v}\n" for k, v in rec.items()))


if __name__ == "__main__":
    main()
