//! The past-sessions list: what the window shows for each stored session, from
//! [`Summary`] rows. Pure, so it is tested without a window or a database.

use crate::feedback::circle_balance::{
    CircleBalanceDesign, Graph, LmhBand, Meter, GRAPH_MAX_RANGE,
};
use crate::feedback::{Response, BANDS_00_90};
use crate::storage::Summary;

use super::state::{clock, CHANNEL_LABELS};

/// One past session as the window lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: i64,
    /// Local start time and length, with "test" when marked.
    pub title: String,
    pub test: bool,
    /// The sensor headings, then one row per band: its pass band and each sensor's mean.
    pub sensors: Vec<String>,
    pub bands: Vec<(String, Vec<String>)>,
    /// The session's [`balance`] table.
    pub balance: Vec<(String, Vec<String>)>,
}

/// The column headings of a [`balance`] table.
pub const BALANCE_COLUMNS: [&str; 4] = ["Before High", "Before Low", "After High", "After Low"];

/// How to read a Balance Meter cell.
pub const BALANCE_NOTE: &str =
    "Each value is how far the meter leans, as a share of its full scale \
     (4, shown as 100%), toward the stronger sensor: right is FP2 or T4, left is FP1 or T3.";

/// The Balance Meter of a session from its Circle-Balance report (in
/// [`CircleBalanceDesign`] input order): one row per meter with its sensor pair, then a
/// cell per [`BALANCE_COLUMNS`], `—` where the session was too short for the window.
/// Empty when no value could be computed.
pub fn balance(report: &[Option<f32>]) -> Vec<(String, Vec<String>)> {
    if report.iter().all(Option::is_none) {
        return Vec::new();
    }
    let design = CircleBalanceDesign::circle_balance();
    let cells = [
        (Graph::Before, LmhBand::High),
        (Graph::Before, LmhBand::Low),
        (Graph::After, LmhBand::High),
        (Graph::After, LmhBand::Low),
    ];
    [
        (Meter::Emotional, "Emotional State"),
        (Meter::Activation, "Activation State"),
    ]
    .into_iter()
    .map(|(meter, name)| {
        let (a, b) = design
            .inputs
            .iter()
            .find(|input| input.meter == meter)
            .expect("the design reads each meter")
            .channels;
        let row = cells
            .iter()
            .map(|&(graph, band)| {
                design
                    .inputs
                    .iter()
                    .zip(report)
                    .find(|(i, _)| i.meter == meter && i.graph == graph && i.band == band)
                    .and_then(|(_, value)| *value)
                    .map_or_else(|| "—".to_owned(), signed)
            })
            .collect();
        let sensor = |channel: usize| CHANNEL_LABELS[channel - 1];
        (format!("{name} ({}/{})", sensor(a), sensor(b)), row)
    })
    .collect()
}

/// A Balance Meter value as a whole percentage of [`GRAPH_MAX_RANGE`], clipped at 100% as the
/// chart clips, with the side it favours: positive is the right sensor.
fn signed(value: f32) -> String {
    let percent = (value.abs() / GRAPH_MAX_RANGE * 100.0).min(100.0).round();
    if percent == 0.0 {
        "0% even".to_owned()
    } else if value > 0.0 {
        format!("{percent}% right")
    } else {
        format!("{percent}% left")
    }
}

/// The rows to list, newest first as given, leaving out test sessions unless `show_tests`.
/// `local_time` formats a start time in seconds since the Unix epoch.
pub fn rows(
    sessions: &[Summary],
    show_tests: bool,
    local_time: impl Fn(i64) -> String,
) -> Vec<Row> {
    sessions
        .iter()
        .filter(|s| show_tests || !s.test)
        .map(|s| {
            let mut sensors: Vec<String> = Vec::new();
            let mut bands: Vec<(String, Vec<String>)> = Vec::new();
            for b in &s.bands {
                let sensor = channel_label(&b.channel);
                if !sensors.contains(&sensor) {
                    sensors.push(sensor);
                }
                let band = band_label(&b.series);
                let mean = format!("{:.2}", b.mean);
                match bands.iter_mut().find(|(name, _)| *name == band) {
                    Some((_, means)) => means.push(mean),
                    None => bands.push((band, vec![mean])),
                }
            }
            let mark = if s.test { " · test" } else { "" };
            Row {
                id: s.id,
                title: format!(
                    "{} · {}{mark}",
                    local_time(s.started_at),
                    clock(s.duration_s)
                ),
                test: s.test,
                sensors,
                bands,
                balance: balance(&s.balance),
            }
        })
        .collect()
}

/// A stored channel name (`Channel1`) as its montage site (`FP1`).
fn channel_label(name: &str) -> String {
    name.strip_prefix("Channel")
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| CHANNEL_LABELS.get(n.wrapping_sub(1)))
        .map_or_else(|| name.to_owned(), |l| (*l).to_owned())
}

/// A stored band series name (`Band-10`) as its pass band (`1–3 Hz`).
fn band_label(series: &str) -> String {
    match BANDS_00_90.iter().find(|(name, _)| *name == series) {
        Some((_, Response::LowPass(high))) => format!("below {high} Hz"),
        Some((_, Response::BandPass(low, high))) => format!("{low}–{high} Hz"),
        Some((_, Response::HighPass(low))) => format!("above {low} Hz"),
        None => series.to_owned(),
    }
}

/// `seconds` since the Unix epoch as local `YYYY-MM-DD HH:MM`.
pub fn local_time(seconds: i64) -> String {
    let t = seconds as libc::time_t;
    // SAFETY: `localtime_r` writes only into `tm`, which outlives the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return format!("{seconds}");
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}
