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

The SMART-2 stage fixtures come from the run-6 baseline under `tmp/s43/baseline/`
(`decisions9.log`, `hook9s.log`, never committed; the last hook run in each file is
used): `smart2_trigger_blocks.tsv`, `smart2_note_blocks.tsv` and `smart2_decisions.tsv`
(the Expression inputs and the decisions the app made), `smart2_stages.tsv` (per
4-sample block, the Band 10/80 amplitudes, the Aggregator outputs and the
PeakFrequency bins), `smart2_stage_inputs.tsv` (the live Source output of the four
channels and the per-block 16-tap-mean alignment flags) and
`smart2_bandpass_stages.tsv` (one channel's band-pass FIR, amplitude and
PeakFrequency stages). `smart2_design.tsv` comes from the run-4 design XML
(`--src4`); the run-6 design is the same exercise.

The Source-chain fixture (`smart2_source_stages.tsv`) carries one channel's live
stage values from the run-5 capture under `tmp/s43/live/` (`hook6.log`, never
committed): for each sample the 25 Hz band-stop input and the band-stop, low-pass
and high-pass outputs, as the float32 bit patterns the app holds.

The whole-exercise fixtures (`run8_*`) come from the run-8 binary hook log
`tmp/s43/run8/run8.bin` (`--src8`, never committed; about four minutes of SMART-2
with the serial RX stream and every graph stage logged on one clock): the consumed
EEG as 24-bit hex frames (`run8_eeg_N.txt`), the per-block table (`run8_blocks_N.tsv`:
A..H flags, Evaluator code, eval clock, lap clock), the baseline Trigger and Note
decisions (`run8_decisions.tsv`) and the logged E..H band amplitudes of blocks 0..6599
(`run8_amplitudes_N.tsv`). Clocks are milliseconds from block 0.

The live-handshake fixtures (`handshake_live_a.txt`, `handshake_live_b.txt`) come from the
2026-11-07 live handshakes under `tmp/s58/` (`--src58`, never committed; `hs_A.txt` is the
first connection after the B-2 was switched on, `hs_B.txt` a reconnect), laid out like
`wire.txt` with the commands the clone writes (stop, a query pass, mode, injection, a second
query pass) and the 101 serial scrubbed.

The sensor-status fixtures (`sensor_inputs.tsv`, `sensor_levels.tsv`) come from the 2026-11-07
hook13 capture `tmp/s57/s57.bin` (`--src57`, never committed; run 8's no-wearer stream replayed into
the original on the Optimize tab): for a few of the 105 blocks, each channel's 512 float32 FFT
input, and the levels and fault flag the original's Head2D and evaluator produced from it.

The all-channel acquisition fixtures (`consumed_stream.txt`, `consumed_volts.tsv`) come from the
ticket-55 replay log `tmp/s55/s55.bin` (`--src55`, never committed; `hook13.js` output of the
original fed run 8's no-wearer stream by `emulate`): the first `S55_FRAMES` frames the original
consumed, and its logged volts for channels 0 to 3 of each consumed sample as float64 bit patterns.

`circle_balance_design.tsv` is the Balance Meter wiring of the decrypted
`B-2-Circle-Balance-fromBAGSD-v5.1.1` design XML (`--src64`, `tmp/s64/design171.xml`, never
committed): one row per chart input, traced back to its LMH band, time window and channels.

`circle_balance_lmh.tsv` and `circle_balance_chart.tsv` are the Balance Meter's input and output
for the no-wearer exercise 146 (240 s): the stored `High`/`Low` LMH series (`--lmh62`, the
`lmh-extract.py` output of its type-9 XML, never committed) and the eight values the original's
`CircleGraphChart` inputs received when its report ran over that exercise (`--src62`, the
`tmp/s62/s62-report.bin` hook log of `0x5c4550`, never committed), as float32 bit patterns.

Usage: python3 -I make-fixtures.py [--src DIR] [--src3 DIR] [--src4 DIR] [--src5 DIR] [--src8 FILE] [--src57 FILE] [--src58 DIR] [--src55 FILE] [--src64 FILE] [--src62 FILE] [--lmh62 FILE] [--baseline DIR] [--out DIR]
"""
import argparse
import collections
import mmap
import pathlib
import re
import struct
import xml.etree.ElementTree as ET

STREAM_FRAMES = 40
FEEDBACK_BLOCKS = 2000  # 4 samples each (16 s); the threshold window is steady throughout
FEEDBACK_EEG_MS = 5000  # EEG window length, from the first threshold sample
SMART2_TRIGGER_BLOCKS = 1500  # ExprTriggerAll inputs; the threshold window is steady well before the end
SMART2_NOTE_BLOCKS = 15200  # past the 15000-sample note tracker window and the Evaluator's 111 -> 183 switch
SMART2_STAGE_BLOCKS = 400  # logged stage values: 1600 samples, past the 600-sample start-up; kept under the 500 KB file limit
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


def live_handshake(path):
    """The clone's handshake as `wire.txt` lines, from `<tag> <code> <reply hex>` lines."""
    replies = {}
    for line in path.read_text().splitlines():
        _, code, hexed = line.split(" ")
        replies.setdefault(int(code), []).append(bytes.fromhex(hexed))
    plain = ["!*#48,0,0,0,0,0,0", "!*#40,1,0,0,0,0,0", "!*#46,0,0,0,0,0,0"]
    lines = []

    def tx(text):
        lines.append("TX " + (text + "\r\n").encode().hex())

    def query_pass(n):
        for code in [101, 110, 112, *range(131, 139)]:
            tx(f"!*#{code},0,0,0,0,0,0")
            reply = replies[code][n]
            if code == 101:
                found = reply[10:28]
                reply = reply.replace(found, with_separators(SERIAL_PLACEHOLDER))
            lines.append("RX " + reply.hex())

    tx(plain[0])
    query_pass(0)
    tx(plain[1])
    tx(plain[2])
    query_pass(1)
    return "\n".join(lines) + "\n"


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


def smart2(lines):
    """Baseline upstream-stage fixtures: ExprTriggerAll and ExprNoteSelection inputs
    with the decisions the app produced from them.

    The two expression elements are told apart by their input count (9 =
    `ExprTriggerAll`: Band 10/80 amplitudes A..H and the Evaluator code J; 5 =
    `ExprNoteSelection`: PeakFrequency A..D and J). Heap addresses and absolute
    timestamps are dropped; times are milliseconds from the first threshold sample.
    Returns (trigger blocks, note blocks, decisions) as fixture text.
    """
    amps, peaks, notes, blocks, selected = [], [], [], [], []
    passes, t0, prev, fires = [], None, 0, []
    for line in lines:
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
        raise SystemExit("baseline stage logs are not aligned block for block")

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


def local(tag):
    return tag.rsplit("}", 1)[-1]


def design_rows(src, header):
    """The design properties of the upstream elements, as tab-separated rows.

    Element names (`BPF Ch1 Band 10`, `Coherence Ch3n4 Band 10`, `PeakFreq Ch1`) give
    channels and band label;
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
        elif m := re.fullmatch(r"Coherence Ch(\d)n(\d) (Band \d+)", name):
            rows.append(("coherence", m[1], m[2], m[3], props["lowFrequency"],
                         props["highFrequency"], props["windowType"], props["filterOrder"],
                         props["processingStyle"]))
        elif m := re.fullmatch(r"PeakFreq Ch(\d)", name):
            rows.append(("peak", m[1], props["fftOrder"], props["lowFrequency"],
                         props["highFrequency"], props["windowType"]))
        elif re.fullmatch(r"Aggregator Ch\d Band \d+", name):
            aggregators.add((props["flowStyle"], props["averageEpoch"]))
    if len(aggregators) != 1:
        raise SystemExit("Aggregators differ in style or epoch")
    rows.append(("aggregator", *aggregators.pop()))
    return rows


def circle_balance_rows(path):
    """One row per Balance Meter input of the decrypted Circle-Balance design XML.

    Traces each `CircleGraphChart` input back through its `ExpressionTransform`, the two
    `AggregatorFilter`/`TimeFilter`/`BAGSDataTypeBandFilter` chains, to the `Source` outputs:
    chart, graph title, range axis, LMH band, time anchor, begin and end (s), the channels
    feeding `A` and `B`, and the expression. Refuses any other chain shape.
    """
    root = ET.fromstring(path.read_text())

    def keyed(node):
        # `flowEngine.display.graphTitle.Chart1` -> `graphTitle.Chart1`
        return {p.get("key").split(".", 2)[2]: (p.text or "").strip()
                for p in node.iter() if local(p.tag) == "property"}

    impl, props, feeds, charts = {}, {}, {}, []
    for e in root.iter():
        if local(e.tag) == "element":
            name = e.get("name")
            impl[name] = next(c.text for c in e if local(c.tag) == "implClass").rsplit(".", 1)[1]
            props[name] = keyed(e)
    for c in root.iter():
        if local(c.tag) != "connection":
            continue
        f = {local(x.tag): (x.text or "").strip() for x in c}
        feeds[(f["destElementName"], f["destPipeName"])] = (f["sourceElementName"], f["sourcePipeName"])
        if impl[f["destElementName"]] == "CircleGraphChart":
            charts.append((f["destElementName"], f["destPipeName"], keyed(c)["rangeAxis"],
                           f["sourceElementName"]))

    def chain(aggregator):
        """The band, time window and Source output (channel) behind one aggregator."""
        time_filter = feeds[(aggregator, "InSignal")][0]
        band_filter = feeds[(time_filter, "InSignal")][0]
        source, out = feeds[(band_filter, "InBAGSD")]
        kinds = [impl[n] for n in (aggregator, time_filter, band_filter, source)]
        if kinds != ["AggregatorFilter", "TimeFilter", "BAGSDataTypeBandFilter", "Source"]:
            raise SystemExit(f"unexpected chain behind {aggregator}: {kinds}")
        if props[aggregator]["flowStyle"] != "absolute" or props[band_filter]["BAGSDataType"] != "LMH":
            raise SystemExit(f"unexpected aggregator or data type behind {aggregator}")
        t = props[time_filter]
        window = (t["exerciseTimeAnchorPoint"], t["beginTime"], t["endTime"])
        return props[band_filter]["BAGSDataTypeBand"], window, out.removeprefix("Out")

    rows = []
    for chart, pipe, axis, expression in sorted(charts):
        if impl[expression] != "ExpressionTransform":
            raise SystemExit(f"{chart} {pipe} is not fed by an expression")
        (band, window, a), (band_b, window_b, b) = (
            chain(feeds[(expression, f"InSignal{v}")][0]) for v in "AB")
        if (band, window) != (band_b, window_b):
            raise SystemExit(f"{expression} pairs different bands or windows")
        rows.append((chart, props[chart][f"graphTitle.{pipe}"], axis, band, *window, a, b,
                     props[expression]["expression"]))
    return rows


def circle_balance_chart(path):
    """The float32 bit patterns of the eight chart-input values of the `0x5c4550` hook log.

    Tag 31 is one call (`ts, this, count, scale, values`), tag 32 the input's running sum and
    count after it (`ts, this, sum, count`). Refuses any call that is not one value at scale 1
    whose sum is that value.
    """
    data = path.read_bytes()
    words = struct.unpack(f"<{len(data) // 8}d", data)
    out, pending, i = [], None, 0
    while i < len(words):
        tag, length = int(words[i]), int(words[i + 1])
        rec = words[i + 2:i + length]
        i += length
        if tag == 31:
            if (rec[2], rec[3]) != (1.0, 1.0):
                raise SystemExit(f"chart input call with count {rec[2]}, scale {rec[3]}")
            pending = (rec[1], rec[4])
        elif tag == 32:
            if pending is None or (rec[1], rec[2], rec[3]) != (pending[0], pending[1], 1):
                raise SystemExit("chart input sum is not its one value")
            out.append(struct.unpack("<I", struct.pack("<f", pending[1]))[0])
            pending = None
    return "".join(f"{bits:08x}\n" for bits in out)


def parse_bpf(hook_text):
    """The live band-pass and PeakFrequency log lines of a hook9 run.

    Returns `(edges, blocks, peak_out, peak_first)`: each `BandPass` object's
    (low, high) edges in Hz; its per-block `(in, out, amp)` little-endian hex
    (4 samples each); each PeakFrequency object's frequency and amplitude bits by
    call index; and the PeakFrequency objects whose window opens on a given sample.
    """
    edges, blocks, peak_out, peak_first = {}, {}, {}, {}
    for line in hook_text.splitlines():
        rest = line.partition(" ")[2]
        m = re.match(r"OBJ BPF (\S+) (\w+)$", rest)
        if m:
            raw = bytes.fromhex(m.group(2))
            edges[m.group(1)] = (struct.unpack_from("<d", raw, 0x20)[0], struct.unpack_from("<d", raw, 0x28)[0])
            continue
        m = re.match(r"BPF 0x66ea81 this=(\S+) n=4 in=(\w{32}) out=(\w{32}) amp=(\w{32})$", rest)
        if m:
            blocks.setdefault(m.group(1), []).append(m.groups()[1:])
            continue
        m = re.match(r"PK this=(\S+) c=(\d+) N=512 last4=\w+ last8=\w+ o4=(\w{8})\w{8} o5=(\w{8})\w{8}(?: win=(\w{8}))?", rest)
        if m:
            peak_out.setdefault(m.group(1), {})[int(m.group(2))] = m.group(3, 4)
            if m.group(5):
                peak_first.setdefault(le_bits(m.group(5))[0], []).append(m.group(1))
    return edges, blocks, peak_out, peak_first


def le_bits(le_hex):
    """Little-endian float32 hex words as big-endian bit-pattern hex words."""
    return [f"{int.from_bytes(bytes.fromhex(le_hex[i:i + 8]), 'little'):08x}" for i in range(0, len(le_hex), 8)]


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def mean_simd(p):
    """`ippsFIR_32f`'s 16-tap sum when the output buffer is 16-byte aligned (see `band_pass.rs`)."""
    total = f32(f32(p[0] + p[1]) + f32(p[2] + p[3]))
    for k in range(4, 16, 4):
        total = f32(total + p[k + 3])
        total = f32(total + p[k + 2])
        total = f32(total + f32(p[k] + p[k + 1]))
    return total


def mean_sequential(p):
    total = 0.0
    for v in p:
        total = f32(total + v)
    return total


def scalar_heads(high_hz, blocks):
    """Per block, whether the 16-tap mean ran its scalar head (destination 8 bytes past
    a 16-byte boundary: the first two outputs sum in order) instead of the SIMD order.

    The app's choice follows heap addresses and is not logged, so it is recovered from
    the logged amplitude: the sliding maximum of the logged FIR output, averaged under
    both sum orders. A block both orders explain is recorded as `0`. A block neither
    explains stops the run.
    """
    window = max(int(500 / (2 * high_hz)), 4)
    maxima, means, seen, flags = collections.deque(), collections.deque(), 0, []
    for _, out, amp in blocks:
        ys = struct.unpack("<4f", bytes.fromhex(out))
        want = [int.from_bytes(bytes.fromhex(amp)[i:i + 4], "little") for i in range(0, 16, 4)]
        simd, head = [], []
        for y in ys:
            y = abs(y)
            while maxima and maxima[-1][1] <= y:
                maxima.pop()
            maxima.append((seen, y))
            while maxima[0][0] + window <= seen:
                maxima.popleft()
            seen += 1
            means.append(maxima[0][1])
            if len(means) > 16:
                means.popleft()
            p = [f32(v * 0.125) for v in [0.0] * (16 - len(means)) + list(means)]
            simd.append(struct.unpack("<I", struct.pack("<f", mean_simd(p)))[0])
            head.append(struct.unpack("<I", struct.pack("<f", mean_sequential(p)))[0])
        simd_ok = simd == want
        head_ok = head[:2] == want[:2] and simd[2:] == want[2:]
        if not (simd_ok or head_ok):
            raise SystemExit("a logged amplitude fits neither 16-tap sum order")
        flags.append(0 if simd_ok else 1)
    return flags


# `ExprTriggerAll` input slots A..H -> (channel, band) of the BandPass behind each.
SLOT_CHANNEL = (0, 1, 0, 1, 2, 3, 2, 3)
SLOT_BAND = ("band10", "band10", "band80", "band80", "band10", "band10", "band80", "band80")
# The graph's band-pass order: Band 10 of Ch1..4, then Band 80 of Ch1..4.
DESIGN_ORDER = (0, 1, 4, 5, 2, 3, 6, 7)


def smart2_stages(lines, hook_text):
    """Baseline stage values per block and the live Source output that feeds them.

    `smart2_stages.tsv`, one row per 4-sample block: the Evaluator code, the 8
    band-pass amplitudes A..H in volts (4 samples each; A, B = Ch1, Ch2 Band 10,
    C, D = Ch1, Ch2 Band 80, E, F = Ch3, Ch4 Band 10, G, H = Ch3, Ch4 Band 80),
    the 8 Aggregator outputs in the same order, and the 4 PeakFrequency bins (hex,
    one digit per sample).

    `smart2_stage_inputs.tsv`, one row per block of SMART2_TRIGGER_BLOCKS: each
    channel's 4 Source-output samples (float32 bits; the input of both its
    band-passes and its PeakFrequency), then one flag per band-pass in the graph's
    order (Band 10 of Ch1..4, Band 80 of Ch1..4): `1` when that block's 16-tap mean ran
    its scalar head (`scalar_heads`).

    The B-2 stream the app consumed is not recoverable from the serial log (the app
    drops frames under the hooks), so the replay starts at the Source output.
    """
    amps, aggs, peaks = [], {}, []
    for line in lines:
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
        raise SystemExit("baseline stage logs are shorter than the stage span")
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

    edges, blocks, _, _ = parse_bpf(hook_text)
    objects = {}
    for obj, hz in edges.items():
        if obj not in blocks:
            continue
        for slot in range(8):
            if len(blocks[obj]) >= 1 and all(
                    f32(amps[0][slot][i]) == struct.unpack("<4f", bytes.fromhex(blocks[obj][0][2]))[i]
                    for i in range(4)):
                objects[slot] = obj
    if sorted(objects) != list(range(8)):
        raise SystemExit("the 8 amplitude band-passes are not all in the hook log")
    heads = {slot: scalar_heads(edges[objects[slot]][1], blocks[objects[slot]]) for slot in range(8)}
    input_rows = []
    for b in range(SMART2_TRIGGER_BLOCKS):
        samples = []
        for ch in range(4):
            slots = [s for s in range(8) if SLOT_CHANNEL[s] == ch]
            ins = {le_bits(blocks[objects[s]][b][0]).__str__() for s in slots}
            if len(ins) != 1:
                raise SystemExit("a channel's band-passes do not share one input")
            samples += le_bits(blocks[objects[slots[0]]][b][0])
        flags = "".join(str(heads[s][b]) for s in DESIGN_ORDER)
        input_rows.append("\t".join([*samples, flags]) + "\n")
    return "".join(stage_rows), "".join(input_rows)


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


# Return sites of the filter apply (`0x651490`) inside `processData` `0x557f00`, in
# chain order, for the 45 Hz interference low-pass, the 25 Hz band-stop, the 150 Hz
# low-pass and the 1.5 Hz high-pass.
SOURCE_SITES = ("0x557fb5", "0x557fe6", "0x558011", "0x55803c")


def source_stages(src):
    """`smart2_source_stages.tsv`: channel 0's Source-chain input and the four stage outputs."""
    stages = {site: [] for site in SOURCE_SITES}
    for line in (src / "hook6.log").read_text().splitlines():
        m = re.match(r"\d+ SRC (0x\w+) this=\S+ n=1 in=(\w{8}) out=(\w{8}) ", line)
        if m and m.group(1) in stages:
            stages[m.group(1)].append((m.group(2), m.group(3)))
    n = len(stages[SOURCE_SITES[0]])
    assert n and all(len(v) == n for v in stages.values()), "source-chain capture is incomplete"

    def bits(le_hex):
        return f"{int.from_bytes(bytes.fromhex(le_hex), 'little'):08x}"

    return "".join(
        "\t".join(bits(h) for h in (
            stages[SOURCE_SITES[0]][i][0], *(stages[s][i][1] for s in SOURCE_SITES))) + "\n"
        for i in range(n))


BANDPASS_SAMPLES = 6000  # live samples kept (cold start included); keeps the file under 500 KB
BAND_EDGES = {(1.0, 3.0): "band10", (23.0, 36.0): "band80"}  # Hz, from the BandPass object


def bandpass_stages(hook_text):
    """`smart2_bandpass_stages.tsv`: one channel's live BandPass and PeakFrequency stages.

    Per sample, as float32 bit patterns: the shared input (the Source output), the FIR
    output and `OutAmplitude` of Band 10 and of Band 80, then the PeakFrequency frequency
    and amplitude; then, per Band 10 and Band 80, whether the sample's block ran the
    16-tap mean's scalar head (`scalar_heads`). The channel is the one whose BandPass
    objects and PeakFrequency window open with the same first sample (the first such
    channel in the log).
    """
    edges, blocks, peak_out, peak_first = parse_bpf(hook_text)
    chosen = {}
    for obj, hz in edges.items():
        if obj in blocks and hz in BAND_EDGES:
            chosen.setdefault(le_bits(blocks[obj][0][0])[0], {})[BAND_EDGES[hz]] = obj
    candidates = [
        (b, peak_first[first][0]) for first, b in chosen.items()
        if len(b) == 2 and len(set(peak_first.get(first, []))) == 1
    ]
    assert candidates, "no channel has both bands and a PeakFrequency window"
    bands, pk = candidates[0]
    heads = {band: scalar_heads(edges[bands[band]][1], blocks[bands[band]]) for band in bands}
    rows = []
    for i in range(BANDPASS_SAMPLES):
        k, j = divmod(i, 4)
        row = [le_bits(blocks[bands["band10"]][k][0])[j]]
        for band in ("band10", "band80"):
            row += [le_bits(blocks[bands[band]][k][1])[j], le_bits(blocks[bands[band]][k][2])[j]]
        row += [f"{int.from_bytes(bytes.fromhex(h), 'little'):08x}" for h in peak_out[pk][i + 1]]
        row += [str(heads["band10"][k]), str(heads["band80"][k])]
        rows.append("\t".join(row) + "\n")
    return "".join(rows)


def new_run(path, marker):
    """The text of the last hook run in an append-only hook log (NUL bytes dropped)."""
    data = path.read_bytes().replace(b"\0", b"")
    return data[data.rfind(marker):].decode(errors="replace")


RUN8_DIV = 12 * (2**23 - 1) * 10 / 2.4  # raw counts per volt, as the app converts them
RUN8_GRAPH_START = 25600  # consumed-sample index the graph started at (found by replay)
RUN8_PREROLL = 4000  # consumed samples kept before the graph start, to warm the Source chain
RUN8_BLOCKS = 30001
RUN8_AMP_BLOCKS = 6600  # = RUN8_AMPLITUDE_BLOCKS in tests/harness/fixtures.rs; logged E..H amplitudes kept: blocks 0..6599, past the 6409..6477 burst
RUN8_AMP_FILE_BLOCKS = 3300  # amplitude blocks per file (144 bytes each); keeps every file under 500 KB
RUN8_FILE_LIMIT = 9000  # EEG frames or block rows per file; keeps every file under 500 KB
# Band A..H -> rank of the band-pass object's address among the 18 graph-call
# objects (ascending). A..H: Ch1 B10, Ch2 B10, Ch1 B80, Ch2 B80, Ch3 B10, Ch4 B10, Ch3 B80, Ch4 B80.
RUN8_BAND_RANKS = (14, 3, 2, 8, 9, 16, 12, 0)


RUN8_TAGS = frozenset((1, 2, 3, 4, 5, 7, 11, 12, 13, 20, 21))  # serial RX, block, clocks, graph calls, note, amplitudes, ch0 volts, ch1..3 volts, Evaluator series input and key


def run8_records(path):
    """Yield (tag, doubles) for each RUN8_TAGS record of the run-8 binary hook log."""
    with open(path, "rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as m:
        i, n = 0, len(m) // 8
        while i + 3 <= n:
            tag, length = (int(x) for x in struct.unpack_from("<2d", m, 8 * i))
            if tag in RUN8_TAGS:
                yield tag, struct.unpack_from("<%dd" % length, m, 8 * i)
            i += length


def run8_frames(records, raw=None):
    """Received frames and the volts the app consumed, from the serial RX records.

    A frame is `AC BC CC 03`, a 16-bit counter and 8 x 24-bit samples; `03 03` in
    the counter position is accepted only when it continues the counter.
    Returns (frames as 8 raw ints, consumed ch0 volts in order). `raw`, if given, collects
    each frame's 32 bytes.
    """
    buf, frames, volts = bytearray(), [], []
    prev = None
    for tag, v in records:
        if tag == 12:
            volts.append(v[4])
        elif tag == 1:
            buf += bytes(int(x) for x in v[4:4 + int(v[3])])
            while True:
                j = buf.find(MARKER)
                if j < 0:
                    del buf[:-3]
                    break
                del buf[:j]
                if len(buf) < FRAME_LEN:
                    break
                counter = (buf[4] << 8) | buf[5]
                if buf[4:6] == b"\x03\x03" and counter != ((prev or 0) + 1) & 0xFFFF:
                    del buf[:FRAME_LEN]
                    continue
                prev = counter
                frames.append([i24(buf[8 + 3 * k:11 + 3 * k]) for k in range(8)])
                if raw is not None:
                    raw.append(bytes(buf[:FRAME_LEN]))
                del buf[:FRAME_LEN]
    return frames, volts


def run8_consumed_indices(frames, volts):
    """Indices of the received frames holding the samples the app consumed, in order.

    Each consumed ch0 volt is matched forward (within 400 samples) to the received
    ch0 slot 0 / slot 4 sequence; frames the app never consumed (counter jumps) drop out.
    """
    cand = [raw for f in frames for raw in (f[0], f[4])]
    pos, used = 0, set()
    for v in volts:
        raw = round(v * RUN8_DIV)
        q = pos
        while q < min(len(cand), pos + 400) and cand[q] != raw:
            q += 1
        if q >= min(len(cand), pos + 400):
            raise SystemExit("consumed sample has no received counterpart")
        used.add(q // 2)
        pos = q + 1
    return sorted(used)


def run8_consumed_frames(frames, volts):
    """The received frames holding the samples the app consumed, in order."""
    return [frames[k] for k in run8_consumed_indices(frames, volts)]


S55_FRAMES = 150  # consumed frames kept in the committed all-channel acquisition fixture


def s55_consumed(path):
    """The frames the original consumed and its logged volts for channels 0 to 3.

    `path` is a `hook13.js` log (tag 12: channel 0, tag 13: channels 1 to 3) of the original
    fed a no-wearer stream by `emulate`. Returns (consumed frames as 32 raw bytes, volts rows
    of [ch0, ch1, ch2, ch3] per consumed sample), both cut to the samples every channel logged.
    """
    records = list(run8_records(path))
    raw, volts0 = [], []
    frames, volts0 = run8_frames(records, raw)
    volts = [volts0] + [[] for _ in range(3)]
    for tag, v in records:
        if tag == 13:
            volts[int(v[3])].append(v[5])
    used = run8_consumed_indices(frames, volts0)
    n = min(len(c) for c in volts) // 2 * 2
    n = min(n, 2 * len(used))
    return [raw[k] for k in used[:n // 2]], [[volts[c][i] for c in range(4)] for i in range(n)]


def s55_row(row):
    """One consumed sample: the four logged volts as float64 bit patterns."""
    return "\t".join("%016x" % struct.unpack("<Q", struct.pack("<d", x))[0] for x in row) + "\n"


def run8_blocks(records):
    """Per graph block: the 16-tap-mean flags, Evaluator code, clocks, note and amplitudes.

    A block is closed by its tag-2 record. Flags come from the logged `dst` of the
    4-sample band-pass calls (tag 5), the Evaluator code from the last tag-11
    record, the clock from the tag-3 timer read and the lap read from tag 4.
    """
    blocks, cur = [], {"calls": {}, "lap": None, "clock": None, "code": None, "note": None, "amps": None, "amps_ad": None}
    for tag, v in records:
        if tag == 5 and int(v[6]) == 4:
            cur["calls"][int(v[3])] = int(v[5]) % 16 == 8
        elif tag == 3:
            cur["clock"] = int(v[4]) + int(v[5])
        elif tag == 4:
            cur["lap"] = int(v[4])
        elif tag == 7:
            cur["note"] = cur["note"] if cur["note"] is not None else int(v[4])
        elif tag == 11:
            p, recs = 4, []
            for _ in range(int(v[3])):
                c = int(v[p])
                recs.append(v[p + 1:p + 1 + c])
                p += 1 + c
            cur["code"] = int(recs[-1][0])
            if len(recs) == 9 and cur["amps"] is None:
                cur["amps"] = [x for r in recs[4:8] for x in r]
                cur["amps_ad"] = [x for r in recs[0:4] for x in r]
        elif tag == 2:
            blocks.append(cur)
            cur = {"calls": {}, "lap": None, "clock": None, "code": None, "note": None, "amps": None, "amps_ad": None}
    return blocks


def run8_evaluator_series(records):
    """The Evaluator's series inputs by name, as `{name: {block: latest float32 value}}`.

    Tag 21 names each series object (`Bal1n2B10`, `Coh3n4B80`, ...); tag 20 is one
    `setInput` per block (4 samples), the series' latest value after it. The block is the
    count before the call over 4.
    """
    names, out = {}, {}
    for tag, v in records:  # the key dump may follow the series' first input
        if tag == 21:
            key = bytes(int(x) for x in v[39:87]).split(b"\0")[0].decode("latin-1")
            kind = "Bal" if key.startswith("lance:") else "Coh"
            channels, band = key.split(":")[-2:]
            names[int(v[3])] = f"{kind}{channels.replace('Channels ', '')}{band.replace('Band ', 'B')}"
    for tag, v in records:
        if tag == 20:
            out.setdefault(names[int(v[3])], {})[int(v[4]) // 4] = v[6]
    return out


def run8(path):
    """Run-8 fixtures: the consumed EEG, the block table, decisions, logged amplitudes.

    Returns a dict of file name to text. Clocks are milliseconds from block 0's
    timer read, so no wall-clock time enters the fixtures. Raw serial bytes stay
    out: only the consumed 24-bit samples are kept, which carry no identifiers.
    """
    records = list(run8_records(path))
    frames, volts = run8_frames(records)
    consumed = run8_consumed_frames(frames, volts)
    blocks = run8_blocks(records)
    if len(blocks) < RUN8_BLOCKS or any(b["clock"] is None for b in blocks[:RUN8_BLOCKS]):
        raise SystemExit("run 8 block log is incomplete")
    blocks = blocks[:RUN8_BLOCKS]
    keys = sorted({k for blk in blocks for k in blk["calls"]})
    if len(keys) != 18:
        raise SystemExit("expected 18 graph-call objects")
    out = {}
    first = (RUN8_GRAPH_START - RUN8_PREROLL) // 2
    eeg = consumed[first:RUN8_GRAPH_START // 2 + (RUN8_BLOCKS * 4 + 1) // 2 + 1]
    for part, lo in enumerate(range(0, len(eeg), RUN8_FILE_LIMIT)):
        out[f"run8_eeg_{part}.txt"] = "".join(
            "".join("%06x" % (x & 0xFFFFFF) for x in f) + "\n"
            for f in eeg[lo:lo + RUN8_FILE_LIMIT])
    t0 = blocks[0]["clock"]
    rows, decs = [], []
    for b, blk in enumerate(blocks):
        flags = "".join("1" if blk["calls"].get(keys[r]) else "0" for r in RUN8_BAND_RANKS)
        now = blk["clock"] - t0
        lap = blk["lap"] - t0 if blk["lap"] is not None else -1
        rows.append(f"{flags}\t{blk['code']}\t{now}\t{lap}\n")
        if blk["lap"] is not None:
            decs += [f"{now}\tTrigger\t{b}\n", f"{now}\tNote\t{blk['note']}\n"]
    for part, lo in enumerate(range(0, len(rows), RUN8_FILE_LIMIT * 2)):
        out[f"run8_blocks_{part}.tsv"] = "".join(rows[lo:lo + RUN8_FILE_LIMIT * 2])
    out["run8_decisions.tsv"] = "".join(decs)
    amps = [" ".join("%08x" % struct.unpack("<I", struct.pack("<f", x))[0] for x in blk["amps"]) + "\n"
            for blk in blocks[:RUN8_AMP_BLOCKS]]
    for part, lo in enumerate(range(0, len(amps), RUN8_AMP_FILE_BLOCKS)):
        out[f"run8_amplitudes_{part}.tsv"] = "".join(amps[lo:lo + RUN8_AMP_FILE_BLOCKS])
    return out


SENSOR_BLOCKS = (0, 1, 2, 3, 15, 16, 17, 20, 31, 58, 77)  # the line-noise decision, the fault blocks and their neighbours


def sensor_blocks(path):
    """Tags 30 (FFT input), 32 (Head2D levels) and 35 (evaluator fault flag), four channels per block."""
    import array

    raw = pathlib.Path(path).read_bytes()
    a = array.array("d")
    a.frombytes(raw[:len(raw) // 8 * 8])
    ffts, heads, flags = [], [], []  # flags: per block, the fault flag after its last evaluator call
    i = 0
    while i + 3 <= len(a):
        tag, size = int(a[i]), int(a[i + 1])
        if size < 3 or i + size > len(a):
            break
        r = a[i:i + size]
        if tag == 30:
            ffts.append((list(r[12:12 + 512]), list(r[12 + 512:12 + 768])))
        elif tag == 32:
            if len(heads) % 4 == 0:
                flags.append(None)
            heads.append((list(r[21:21 + 256]), r[9], r[12], r[13], r[14]))
        elif tag == 35 and flags:
            flags[-1] = int(r[4])
        i += size
    blocks = len(heads) // 4
    assert blocks == len(ffts) // 4 and None not in flags, "capture is not whole blocks"
    inputs, levels = [], []
    for b in SENSOR_BLOCKS:
        for c in range(4):
            samples, spectrum = ffts[4 * b + c]
            head = heads[4 * b + c]
            assert head[0] == spectrum, "Head2D spectrum is the FFT output"
            inputs.append(f"{b}\t{c}\t" + " ".join(f"{x:.9g}" for x in samples) + "\n")
            levels.append(f"{b}\t{c}\t" + "\t".join(f"{x:.9g}" for x in head[1:]) + f"\t{flags[b]}\n")
    return {"sensor_inputs.tsv": "".join(inputs), "sensor_levels.tsv": "".join(levels)}


def main():
    root =pathlib.Path(__file__).resolve().parents[3]
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", type=pathlib.Path, default=root / "tmp/s26/run2")
    ap.add_argument("--src3", type=pathlib.Path, default=root / "tmp/s26/run3")
    ap.add_argument("--src4", type=pathlib.Path, default=root / "tmp/s26/run4")
    ap.add_argument("--src5", type=pathlib.Path, default=root / "tmp/s43/live")
    ap.add_argument("--src8", type=pathlib.Path, default=root / "tmp/s43/run8/run8.bin")
    ap.add_argument("--src57", type=pathlib.Path, default=root / "tmp/s57/s57.bin")
    ap.add_argument("--src58", type=pathlib.Path, default=root / "tmp/s58")
    ap.add_argument("--src55", type=pathlib.Path, default=root / "tmp/s55/s55.bin")
    ap.add_argument("--src64", type=pathlib.Path, default=root / "tmp/s64/design171.xml")
    ap.add_argument("--src62", type=pathlib.Path, default=root / "tmp/s62/s62-report.bin")
    ap.add_argument("--lmh62", type=pathlib.Path, default=root / "tmp/s62/lmh146.tsv")
    ap.add_argument("--baseline", type=pathlib.Path, default=root / "tmp/s43/baseline")
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
    for tag in "ab":
        (args.out / f"handshake_live_{tag}.txt").write_text(
            live_handshake(args.src58 / f"hs_{tag.upper()}.txt"))
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

    decision_lines = new_run(args.baseline / "decisions9.log", b"hook5b loaded").splitlines()
    hook_text = new_run(args.baseline / "hook9s.log", b"hook6 loaded base")
    trigger, note, decs4 = smart2(decision_lines)
    (args.out / "smart2_trigger_blocks.tsv").write_text(trigger)
    (args.out / "smart2_note_blocks.tsv").write_text(note)
    (args.out / "smart2_decisions.tsv").write_text(decs4)

    header4 = dict(re.findall(
        r"<base:(\w+)>([^<]*)</base:\1>", (args.src4 / "130-t0.xml").read_text()))
    stages, stage_inputs = smart2_stages(decision_lines, hook_text)
    (args.out / "smart2_stages.tsv").write_text(stages)
    (args.out / "smart2_stage_inputs.tsv").write_text(stage_inputs)
    (args.out / "smart2_source_stages.tsv").write_text(source_stages(args.src5))
    (args.out / "smart2_bandpass_stages.tsv").write_text(bandpass_stages(hook_text))
    (args.out / "smart2_design.tsv").write_text("".join(
        "\t".join(r) + "\n" for r in design_rows(args.src4, header4)))
    (args.out / "circle_balance_design.tsv").write_text("".join(
        "\t".join(r) + "\n" for r in circle_balance_rows(args.src64)))

    (args.out / "circle_balance_lmh.tsv").write_text(args.lmh62.read_text())
    (args.out / "circle_balance_chart.tsv").write_text(circle_balance_chart(args.src62))

    for name, text in run8(args.src8).items():
        (args.out / name).write_text(text)
    for name, text in sensor_blocks(args.src57).items():
        (args.out / name).write_text(text)

    frames55, rows55 = s55_consumed(args.src55)
    if len(frames55) < S55_FRAMES:
        raise SystemExit("ticket-55 replay log holds too few consumed frames")
    counters = [(f[4] << 8) | f[5] for f in frames55[:S55_FRAMES]]
    if any(b != (a + 1) & 0xFFFF for a, b in zip(counters, counters[1:])):
        raise SystemExit("ticket-55 replay dropped a frame inside the committed window")
    (args.out / "consumed_stream.txt").write_text("".join(f.hex() + "\n" for f in frames55[:S55_FRAMES]))
    (args.out / "consumed_volts.tsv").write_text("".join(s55_row(r) for r in rows55[:2 * S55_FRAMES]))


if __name__ == "__main__":
    main()
