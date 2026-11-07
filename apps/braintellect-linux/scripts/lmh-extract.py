#!/usr/bin/env python3
"""Extract the stored LMH series of one original exercise for `tests/replay_s62.rs`.

Reads a decoded type-9 `exerciseBAGSData` XML (see
`docs/reverse-engineering/braintellect2/session-storage.md`) and writes, as TSV to `--out`
(default `tmp/s62/lmh.tsv` at the repository root), a `duration` row with the exercise length
in seconds, then one row per point of the `High` and `Low` series of Channel1..4:
`channel  band  x_ms  y`, `y` as stored.

Nothing written here is committed: the output stays under the repo's `tmp/`.
Usage: python3 -I lmh-extract.py <t9.xml> <duration_s> [--out FILE]
"""
import argparse
import pathlib
import re
import xml.etree.ElementTree as ET


def local(tag):
    return tag.rsplit("}", 1)[-1]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("xml", type=pathlib.Path)
    ap.add_argument("duration_s", type=int)
    ap.add_argument("--out", type=pathlib.Path,
                    default=pathlib.Path(__file__).resolve().parents[3] / "tmp/s62/lmh.tsv")
    args = ap.parse_args()
    rows = [f"duration\t{args.duration_s}\n"]
    for channel in ET.parse(args.xml).getroot().iter():
        if local(channel.tag) != "exerciseDataChannel" or channel.get("dataType") != "LMH":
            continue
        number = re.fullmatch(r"Channel(\d)", channel.get("channelName"))[1]
        for series in channel.iter():
            if local(series.tag) != "exerciseDataSeries":
                continue
            band = series.get("seriesName").split(" ", 1)[0]
            if band not in ("High", "Low"):
                continue
            for item in series.iter():
                if local(item.tag) == "dataItem":
                    v = {local(c.tag): c.text.strip() for c in item}
                    rows.append(f"{number}\t{band}\t{v['xValue']}\t{v['yValue']}\n")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("".join(rows))
    print(f"rows {len(rows) - 1}")


if __name__ == "__main__":
    main()
