//! Bridges the baseline's flat stored-record fixture (`key -> value`) and the
//! clone's [`SessionRecord`], so storage is compared through the public API.

use std::collections::BTreeMap;

use braintellect_linux::storage::{Channel, Header, Point, Series, SessionRecord};

use super::fixtures::StoredRecord;

fn text(r: &StoredRecord, key: &str) -> String {
    r.0[key].clone()
}

/// Build the session the baseline recorded. The fixture keeps only the first
/// series of the first channel (three points) and the channel count, so the
/// other channels are named `Channel<n>` and carry no series.
pub fn from_baseline(r: &StoredRecord) -> SessionRecord {
    let header = Header {
        version: text(r, "header.version"),
        design: text(r, "header.design"),
        montage: text(r, "header.montage"),
        device: text(r, "header.device"),
        samples_per_second: r.number("header.samplesPerSecond") as u32,
        samples_per_packet: r.number("header.samplesPerPacket") as u32,
        number_channels: r.number("header.numberChannels") as u32,
        device_gain: r.number("header.deviceGain"),
        sensor_gain: r.number("header.sensorGain"),
        device_family: r.number("header.deviceFamily") as u32,
        device_model: text(r, "header.deviceModel"),
        interference_filter: text(r, "header.interferenceFilter"),
        device_high_pass_cutoff: r.number("header.deviceHighPassCutoff"),
        device_low_pass_cutoff: r.number("header.deviceLowPassCutoff"),
        data_storage_style: text(r, "header.dataStorageStyle"),
    };
    let series_prefix =
        r.0.keys()
            .find_map(|k| k.strip_suffix(".0.x"))
            .expect("a baseline series");
    let mut parts = series_prefix.splitn(3, '.');
    assert_eq!(parts.next(), Some("series"));
    let (channel, series) = (parts.next().unwrap(), parts.next().unwrap());
    let points: Vec<Point> = (0..)
        .map(|i| {
            (
                format!("{series_prefix}.{i}.x"),
                format!("{series_prefix}.{i}.y"),
            )
        })
        .take_while(|(x, _)| r.0.contains_key(x))
        .map(|(x, y)| Point {
            x: r.number(&x),
            y: r.number(&y),
        })
        .collect();
    let channels = (1..=r.number("series.channelCount") as usize)
        .map(|n| {
            let name = format!("Channel{n}");
            let series = if name == channel {
                vec![Series {
                    name: series.to_owned(),
                    points: points.clone(),
                }]
            } else {
                Vec::new()
            };
            Channel { name, series }
        })
        .collect();
    SessionRecord {
        header,
        duration_s: r.number("exercise.duration") as u32,
        started_at: 0,
        capture: None,
        channels,
    }
}

/// Flatten a stored session to the baseline's observable field names. Series
/// points are limited to the first series of the first channel, the same
/// window the fixture keeps.
pub fn to_stored_record(s: &SessionRecord) -> StoredRecord {
    let h = &s.header;
    let mut m: BTreeMap<String, String> = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        m.insert(k.to_owned(), v);
    };
    put("header.version", h.version.clone());
    put("header.design", h.design.clone());
    put("header.montage", h.montage.clone());
    put("header.device", h.device.clone());
    put("header.samplesPerSecond", h.samples_per_second.to_string());
    put("header.samplesPerPacket", h.samples_per_packet.to_string());
    put("header.numberChannels", h.number_channels.to_string());
    put("header.deviceGain", h.device_gain.to_string());
    put("header.sensorGain", h.sensor_gain.to_string());
    put("header.deviceFamily", h.device_family.to_string());
    put("header.deviceModel", h.device_model.clone());
    put("header.interferenceFilter", h.interference_filter.clone());
    put(
        "header.deviceHighPassCutoff",
        h.device_high_pass_cutoff.to_string(),
    );
    put(
        "header.deviceLowPassCutoff",
        h.device_low_pass_cutoff.to_string(),
    );
    put("header.dataStorageStyle", h.data_storage_style.clone());
    put("exercise.duration", s.duration_s.to_string());
    put("series.channelCount", s.channels.len().to_string());
    if let Some((channel, series)) = s
        .channels
        .first()
        .and_then(|c| c.series.first().map(|s| (c, s)))
    {
        for (i, p) in series.points.iter().take(3).enumerate() {
            let prefix = format!("series.{}.{}.{i}", channel.name, series.name);
            put(&format!("{prefix}.x"), p.x.to_string());
            put(&format!("{prefix}.y"), p.y.to_string());
        }
    }
    StoredRecord(m)
}
