//! The original's decoded type-9 record (`ed_9.xml`, `session-storage.md`) read back as the
//! clone's stored channels, to hold `StoredSeries` to it.

use braintellect_linux::feedback::FFT_NORMAL_ORDER;
use braintellect_linux::storage::{Channel, Point, Series};

/// The numbers of every `<base:tag>` element in `text`, in order.
fn values(text: &str, tag: &str) -> Vec<f64> {
    let open = format!("<base:{tag}>");
    text.split(&open)
        .skip(1)
        .map(|t| t.split('<').next().expect("value").parse().expect("number"))
        .collect()
}

/// The value of attribute `name` in the start tag `tag`.
fn attribute<'a>(tag: &'a str, name: &str) -> &'a str {
    let open = format!("{name}=\"");
    tag.split_once(&open)
        .and_then(|(_, rest)| rest.split_once('"'))
        .unwrap_or_else(|| panic!("{name} in {tag}"))
        .0
}

/// Every series of the record as `(channel, series name, points)`, names unescaped. The
/// `FFT-Normal-Order` vector's points have `x` the bin index, as the clone stores them.
fn series(xml: &str) -> Vec<(String, String, Vec<Point>)> {
    let mut all = Vec::new();
    for channel in xml.split("<base:exerciseDataChannel ").skip(1) {
        let channel_name = attribute(channel.split('>').next().expect("tag"), "channelName");
        for s in channel.split("<base:exerciseDataSeries ").skip(1) {
            let name =
                attribute(s.split('>').next().expect("tag"), "seriesName").replace("&amp;", "&");
            let ys = values(s, "yValue");
            let points = if name == FFT_NORMAL_ORDER {
                ys.into_iter()
                    .enumerate()
                    .map(|(bin, y)| Point { x: bin as f64, y })
                    .collect()
            } else {
                values(s, "xValue")
                    .into_iter()
                    .zip(ys)
                    .map(|(x, y)| Point { x, y })
                    .collect()
            };
            all.push((channel_name.to_owned(), name, points));
        }
    }
    all
}

/// The record's series in the shape of `clone`: per clone channel, each clone series looked
/// up by channel and series name. Names compare without case: the original's design names
/// Channel4's 60 Hz series `60hz`, the others `60Hz`. A series the record lacks panics.
pub fn as_stored(xml: &str, clone: &[Channel]) -> Vec<Channel> {
    let record = series(xml);
    clone
        .iter()
        .map(|c| Channel {
            name: c.name.clone(),
            series: c
                .series
                .iter()
                .map(|s| {
                    let (_, _, points) = record
                        .iter()
                        .find(|(channel, name, _)| {
                            *channel == c.name && name.eq_ignore_ascii_case(&s.name)
                        })
                        .unwrap_or_else(|| panic!("{} {} not in the record", c.name, s.name));
                    Series {
                        name: s.name.clone(),
                        points: points.clone(),
                    }
                })
                .collect(),
        })
        .collect()
}

/// The largest `|dy|` of each clone series against the record, as `(channel, series, dy)`,
/// for the replays to print.
pub fn worst_by_series(expected: &[Channel], actual: &[Channel]) -> Vec<(String, String, f64)> {
    expected
        .iter()
        .zip(actual)
        .flat_map(|(e, a)| {
            e.series.iter().zip(&a.series).map(move |(es, as_)| {
                let worst = es
                    .points
                    .iter()
                    .zip(&as_.points)
                    .map(|(p, q)| (p.y - q.y).abs())
                    .fold(0.0, f64::max);
                (e.name.clone(), es.name.clone(), worst)
            })
        })
        .collect()
}
