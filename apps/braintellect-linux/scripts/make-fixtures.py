#!/usr/bin/env python3
"""Regenerate the scrubbed parity fixtures from the raw no-wearer captures.

Reads the run-2 capture under `tmp/s26/run2/`, the run-3 SMART-2 decision
capture under `tmp/s26/run3/` and the run-4 upstream-stage capture under
`tmp/s26/run4/` (never committed) and writes the minimal, scrubbed subset the
parity-replay harness needs to `apps/braintellect-linux/tests/fixtures/`.

Scrubbing: the B-2 serial number (the 101 reply's device id, read from the
capture, never hardcoded here) is replaced by a fixed placeholder; the wire
protocol carries no checksum, so nothing is re-derived. The Bluetooth MAC, host
IDs, machine/profile/user names, session/client ids and timestamps never enter
the fixtures: only the wire bytes, a short stream window, decoded-sample
expectations, control decisions, and a handful of record fields are kept.

The run-4 stage fixtures (`smart2_design.tsv`, `smart2_eeg.tsv`,
`smart2_stages.tsv`) carry the design properties of the upstream elements, the
decoded no-wearer channel input aligned to the logged stage values, and those
values: the Band 10/80 amplitudes, the Aggregator outputs and the PeakFrequency
bins, per 4-sample block (`audio-feedback.md`, "Alignment").

Usage: python3 -I make-fixtures.py [--src DIR] [--src3 DIR] [--src4 DIR] [--out DIR]
"""
import argparse
import pathlib
import re
import xml.etree.ElementTree as ET

STREAM_FRAMES = 40
FEEDBACK_BLOCKS = 2000  # 4 samples each (16 s); the threshold window is steady throughout
FEEDBACK_EEG_MS = 5000  # EEG window length, from the first threshold sample
SMART2_TRIGGER_BLOCKS = 1500  # ExprTriggerAll inputs; the threshold window is steady well before the end
SMART2_NOTE_BLOCKS = 15200  # past the 15000-sample note tracker window and the Evaluator's 111 -> 183 switch
SMART2_STAGE_BLOCKS = 400  # logged stage values: 1600 samples, past the 600-sample start-up; kept under the 500 KB file limit
# Channel sample (counting both samples of every frame, from the start command) of
# the logged stage values' first sample: `2 * 34000 + 3659` found by sliding-DFT
# against the logged PeakFrequency (`audio-feedback.md`, "Alignment (observed)"),
# plus 15 samples, the offset that minimises the Band 10/80 error once the 150 Hz
# low-pass FIR (about 11.5 samples of group delay) is in the chain.
SMART2_FIRST_SAMPLE = 2 * 34000 + 3659 + 15
PEAK_BIN_HZ = 500 / 512  # PeakFrequency resolution: sample rate / FFT order 512
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
    """Independent (Python) implementation of the documented ADC->volts formula.

    Each row holds 8 values in channel order: channel 0's two samples (oldest
    first), then channel 1's, and so on.
    """
    family = int(header["deviceFamily"])
    divisor = (float(header["deviceGain"]) * FULL_SCALE[family]
               * float(header["sensorGain"]) / VREF[family])
    stream = b"".join(chunks)
    rows = []
    for off in range(0, len(stream), FRAME_LEN):
        frame = stream[off:off + FRAME_LEN]
        assert frame[:4] == MARKER and frame[6:8] == b"\x03\x03", "frame desync"
        counter = int.from_bytes(frame[4:6], "big")
        payload = [i24(frame[8 + 3 * k:11 + 3 * k]) / divisor * 1e6 for k in range(8)]
        # Channel c reads payload slots c and 4 + c (b2-protocol.md, "Unresolved").
        volts = [payload[c + 4 * n] for c in range(4) for n in range(2)]
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


def feedback(src):
    """Scrubbed EEG window and the aligned feedback decisions from run 3.

    Times are milliseconds from the first threshold sample (hook clock), never
    absolute. `blocks`: per 4-sample block the counter's wall-clock reading, the
    smoothed threshold inputs and the observed pass flags. `decisions`: counter
    fires (`Trigger`, value = block index) and the note input the sound element
    received (`Note`).
    """
    blocks, cur, t0, counter_count, prev, fires = [], [], None, 0, 0, []
    notes = []
    for line in (src / "decisions.log").read_text(errors="replace").splitlines():
        parts = line.split(" ")
        if len(parts) < 3 or not parts[0].isdigit():
            continue
        ts, kind = int(parts[0]), parts[1]
        if kind == "TH":
            t0 = ts if t0 is None else t0
            fields = dict(p.split("=") for p in parts[2:] if "=" in p)
            if "pass" not in fields:  # a line torn by the two hooks' concurrent writers
                continue
            cur.append((float(fields["x"]), int(fields["pass"])))
        elif kind == "CTR" and t0 is not None:
            count = int(parts[4].split("=")[1])
            passes = sum(p for _, p in cur)
            if count != prev + passes:
                fires.append((len(blocks), ts - t0))
            prev = count
            blocks.append((ts - t0, cur))
            cur = []
        elif kind == "SE_NOTEIDX" and t0 is not None:
            notes.append((ts - t0, int(parts[3].split("=")[1])))
    blocks = blocks[:FEEDBACK_BLOCKS]
    end = blocks[-1][0]
    decisions = sorted(
        [(t, "Trigger", b) for b, t in fires if b < len(blocks)]
        + [(t, "Note", n) for t, n in notes if t <= end]
    )
    return t0, blocks, decisions


F32_DIGITS = "%.9g"  # enough digits to round-trip a single-precision float


def expression_records(line):
    """The input arrays of an ExpressionTransform log line: one list per record."""
    return [
        [float(x) for x in re.search(r"\[(.*?)\]", rec).group(1).split(",")]
        for rec in line.split("|")[1:]
    ]


def smart2(src):
    """Run-4 upstream-stage fixtures: ExprTriggerAll and ExprNoteSelection inputs
    with the decisions the app produced from them.

    The two expression elements are told apart by their input count (9 =
    `ExprTriggerAll`: Band 10/80 amplitudes A..H and the Evaluator code J; 5 =
    `ExprNoteSelection`: PeakFrequency A..D and J). Heap addresses and absolute
    timestamps are dropped; times are milliseconds from the first threshold sample.
    Returns (trigger blocks, note blocks, decisions) as fixture text.
    """
    amps, peaks, notes, blocks, selected = [], [], [], [], []
    passes, t0, prev, fires = [], None, 0, []
    for line in (src / "decisions5.log").read_text(errors="replace").splitlines():
        parts = line.split(" ", 3)
        if len(parts) < 3 or not parts[0].isdigit():
            continue
        ts, kind = int(parts[0]), parts[1]
        if kind == "EX" and parts[3].startswith("n=9 "):
            amps.append(expression_records(line))
        elif kind == "EX" and parts[3].startswith("n=5 "):
            peaks.append(expression_records(line))
        elif kind == "NP.out":
            notes.append(int(line.rsplit("note=", 1)[1]))
        elif kind == "TH":
            t0 = ts if t0 is None else t0
            fields = dict(p.split("=") for p in line.split(" ")[3:] if "=" in p)
            passes.append(int(fields["pass"]))
        elif kind == "CTR" and t0 is not None:
            count = int(line.split("count=")[1].split(" ")[0])
            if len(passes) != 4:
                raise SystemExit("threshold block is not 4 samples")
            if count != prev + sum(passes):
                fires.append((len(blocks), ts - t0))
            prev = count
            blocks.append((ts - t0, passes))
            passes = []
        elif kind == "SE_NOTEIDX" and t0 is not None:
            selected.append((ts - t0, int(line.split(" in=")[1].split(" ")[0])))
    if not len(amps) == len(peaks) == len(notes) == len(blocks):
        raise SystemExit("run-4 stage logs are not aligned block for block")

    def control(records):
        j = records[-1]
        if len(set(j)) != 1:
            raise SystemExit("Evaluator code changed inside a block")
        return int(j[0])

    trigger_rows = []
    for (time_ms, flags), rec in zip(blocks[:SMART2_TRIGGER_BLOCKS], amps):
        values = [F32_DIGITS % v for r in rec[4:8] for v in r]
        trigger_rows.append(
            f"{time_ms}\t{control(rec)}\t" + "\t".join(values) + "\t"
            + "\t".join(map(str, flags)) + "\n")

    def hexbins(record):
        bins = [v / PEAK_BIN_HZ for v in record]
        if any(b != int(b) or not 0 <= b <= 15 for b in bins):
            raise SystemExit("PeakFrequency is not a bin of the 512-point FFT")
        return "".join("%x" % int(b) for b in bins)

    note_rows = [
        f"{control(rec)}\t" + "\t".join(hexbins(r) for r in rec[:4]) + f"\t{note}\n"
        for rec, note in zip(peaks[:SMART2_NOTE_BLOCKS], notes)
    ]

    end = blocks[SMART2_TRIGGER_BLOCKS - 1][0]
    decisions = sorted(
        [(t, "Trigger", b) for b, t in fires if b < SMART2_TRIGGER_BLOCKS]
        + [(t, "Note", n) for t, n in selected if t <= end])
    decision_rows = [f"{t}\t{k}\t{v}\n" for t, k, v in decisions]
    return "".join(trigger_rows), "".join(note_rows), "".join(decision_rows)


def stream_frames(src):
    """Every 32-byte stream frame received after the start command, in order."""
    data, started = bytearray(), False
    for line in (src / "trace5.log").read_text().splitlines():
        p = line.split(" ")
        if len(p) < 6:
            continue
        if p[2] == "TX" and p[3] == "BT_Device::write" and bytes.fromhex(p[-1]).startswith(b"!*#48,1,"):
            started = True
        if started and p[2] == "RX" and p[3] == "win32" and p[4] == "GetOverlappedResult":
            try:
                data += bytes.fromhex(p[-1])
            except ValueError:
                continue
    frames, off = [], data.find(MARKER)
    while 0 <= off and off + FRAME_LEN <= len(data):
        if data[off:off + 4] != MARKER:
            off = data.find(MARKER, off + 1)
            continue
        frames.append(bytes(data[off:off + FRAME_LEN]))
        off += FRAME_LEN
    return frames


def local(tag):
    return tag.rsplit("}", 1)[-1]


def design_rows(src, header):
    """The design properties of the upstream elements, as tab-separated rows.

    Element names (`BPF Ch1 Band 10`, `PeakFreq Ch1`) give channel and band label;
    the Source filter settings come from the session header and the app log.
    """
    root = ET.fromstring((src / "130-t1.xml").read_text())
    interference2 = re.findall(
        r"interferenceFilter2 -> (.*)", (src / "app.log").read_text(errors="replace"))[-1]
    rows = [
        ("rate", header["samplesPerSecond"]),
        ("source", "interferenceFilter2", interference2.strip()),
        ("source", "lowPassCutoff", header["deviceLowPassCutoff"]),
        ("source", "highPassCutoff", header["deviceHighPassCutoff"]),
    ]
    aggregators = set()
    for element in root.iter():
        if local(element.tag) != "element":
            continue
        name = element.get("name")
        props = {p.get("key").rsplit(".", 1)[1]: p.text
                 for p in element.iter() if local(p.tag) == "property"}
        if m := re.fullmatch(r"BPF Ch(\d) (Band \d+)", name):
            rows.append(("bandpass", m[1], m[2], props["lowFrequency"], props["highFrequency"],
                         props["windowType"], props["filterOrder"]))
        elif m := re.fullmatch(r"PeakFreq Ch(\d)", name):
            rows.append(("peak", m[1], props["fftOrder"], props["lowFrequency"],
                         props["highFrequency"], props["windowType"]))
        elif re.fullmatch(r"Aggregator Ch\d Band \d+", name):
            aggregators.add((props["flowStyle"], props["averageEpoch"]))
    if len(aggregators) != 1:
        raise SystemExit("Aggregators differ in style or epoch")
    rows.append(("aggregator", *aggregators.pop()))
    return rows


def smart2_stages(src, header):
    """Run-4 stage values per block and the decoded EEG aligned to them.

    `smart2_stages.tsv`, one row per 4-sample block: the Evaluator code, the 8
    band-pass amplitudes A..H in volts (4 samples each; A, B = Ch1, Ch2 Band 10,
    C, D = Ch1, Ch2 Band 80, E, F = Ch3, Ch4 Band 10, G, H = Ch3, Ch4 Band 80),
    the 8 Aggregator outputs in the same order, and the 4 PeakFrequency bins (hex,
    one digit per sample). `smart2_eeg.tsv`: the first column is the sample's
    index relative to the first logged sample (negative: Source-filter history),
    then microvolts for channels 1..4.
    """
    amps, aggs, peaks = [], {}, []
    for line in (src / "decisions5.log").read_text(errors="replace").splitlines():
        parts = line.split(" ", 3)
        if len(parts) < 4 or parts[1] != "EX":
            continue
        if parts[3].startswith("n=9 "):
            amps.append(expression_records(line))
        elif parts[3].startswith("n=5 "):
            peaks.append(expression_records(line))
        elif parts[3].startswith("n=2 "):
            aggs.setdefault(parts[2], []).append(expression_records(line))
    count = SMART2_STAGE_BLOCKS
    if min(len(amps), len(peaks), *map(len, aggs.values())) < count:
        raise SystemExit("run-4 stage logs are shorter than the stage span")
    # Each Aggregator feeds one Balance expression; the Aggregator's first output
    # equals its band-pass input's first sample, which tells the inputs apart.
    first = [amps[0][i][0] for i in range(8)]
    by_input = {}
    for records in aggs.values():
        for slot in (0, 1):
            by_input[first.index(records[0][slot][0])] = (records, slot)
    if sorted(by_input) != list(range(8)):
        raise SystemExit("Aggregator outputs do not map onto the 8 band-pass inputs")

    def control(records):
        j = records[-1]
        if len(set(j)) != 1:
            raise SystemExit("Evaluator code changed inside a block")
        return int(j[0])

    stage_rows = []
    for b in range(count):
        band = [F32_DIGITS % v for rec in amps[b][:8] for v in rec]
        agg = [F32_DIGITS % v for i in range(8) for v in by_input[i][0][b][by_input[i][1]]]
        bins = [
            "".join("%x" % round(v / PEAK_BIN_HZ) for v in rec) for rec in peaks[b][:4]]
        stage_rows.append("\t".join([str(control(amps[b])), *band, *agg, *bins]) + "\n")

    uv = expected_microvolts([b"".join(stream_frames(src))], header)
    eeg_rows = []
    for s in range(SMART2_FIRST_SAMPLE, SMART2_FIRST_SAMPLE + 4 * SMART2_TRIGGER_BLOCKS):
        volts = uv[s // 2][1]
        eeg_rows.append(
            f"{s - SMART2_FIRST_SAMPLE}\t"
            + "\t".join("%.6f" % volts[2 * c + s % 2] for c in range(4)) + "\n")
    return "".join(stage_rows), "".join(eeg_rows)


def feedback_eeg(src, t0, header):
    """EEG frames (counter, 8 microvolt values in channel order) received within the window."""
    trace = (src / "trace.log").read_text().splitlines()
    data, stamps, started = bytearray(), [], False
    for line in trace:
        p = line.split(" ")
        if len(p) < 6:
            continue
        if p[2] == "TX" and p[3] == "BT_Device::write" and bytes.fromhex(p[-1]).startswith(b"!*#48,1,"):
            started = True
        if started and p[2] == "RX" and p[3] == "win32" and p[4] == "GetOverlappedResult":
            chunk = bytes.fromhex(p[-1])
            stamps += [int(p[0])] * len(chunk)
            data += chunk
    # Re-synchronise on the frame marker: chunking does not align with frames.
    rows, off = [], data.find(MARKER)
    while off >= 0 and off + FRAME_LEN <= len(data):
        ms = stamps[off] - t0
        if 0 <= ms < FEEDBACK_EEG_MS:
            rows.append((ms, data[off:off + FRAME_LEN]))
        off += FRAME_LEN
    chunks = [b"".join(f for _, f in rows)]
    uv = expected_microvolts(chunks, header)
    return [(ms, c, vs) for (ms, _), (c, vs) in zip(rows, uv)]


def main():
    root = pathlib.Path(__file__).resolve().parents[3]
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", type=pathlib.Path, default=root / "tmp/s26/run2")
    ap.add_argument("--src3", type=pathlib.Path, default=root / "tmp/s26/run3")
    ap.add_argument("--src4", type=pathlib.Path, default=root / "tmp/s26/run4")
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

    t0, blocks, decs = feedback(args.src3)
    header3 = dict(re.findall(
        r"<base:(\w+)>([^<]*)</base:\1>", (args.src3 / "129-t0.xml").read_text()))
    (args.out / "feedback_blocks.tsv").write_text("".join(
        f"{t}\t" + "\t".join(repr(v) for v, _ in vs) + "\t"
        + "\t".join(str(p) for _, p in vs) + "\n" for t, vs in blocks))
    (args.out / "feedback_decisions.tsv").write_text(
        "".join(f"{t}\t{k}\t{v}\n" for t, k, v in decs))
    (args.out / "feedback_eeg.tsv").write_text("".join(
        f"{ms}\t{c}\t" + "\t".join(f"{v:.9f}" for v in vs) + "\n"
        for ms, c, vs in feedback_eeg(args.src3, t0, header3)))

    trigger, note, decs4 = smart2(args.src4)
    (args.out / "smart2_trigger_blocks.tsv").write_text(trigger)
    (args.out / "smart2_note_blocks.tsv").write_text(note)
    (args.out / "smart2_decisions.tsv").write_text(decs4)

    header4 = dict(re.findall(
        r"<base:(\w+)>([^<]*)</base:\1>", (args.src4 / "130-t0.xml").read_text()))
    stages, eeg = smart2_stages(args.src4, header4)
    (args.out / "smart2_stages.tsv").write_text(stages)
    (args.out / "smart2_eeg.tsv").write_text(eeg)
    (args.out / "smart2_design.tsv").write_text("".join(
        "\t".join(r) + "\n" for r in design_rows(args.src4, header4)))


if __name__ == "__main__":
    main()
