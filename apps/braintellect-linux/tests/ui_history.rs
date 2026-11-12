//! The past-sessions list the window shows, from stored session summaries.

use braintellect_linux::storage::{BandMean, Summary};
use braintellect_linux::ui::history::{balance, rows};

fn summary(id: i64, test: bool) -> Summary {
    let band = |channel: &str, series: &str, mean: f64| BandMean {
        channel: channel.to_owned(),
        series: series.to_owned(),
        mean,
    };
    Summary {
        id,
        started_at: id * 100,
        duration_s: 240,
        test,
        bands: vec![
            band("Channel1", "Band-00", 9.0),
            band("Channel1", "Band-10", 1.234),
            band("Channel1", "Band-VHF", 5.0),
            band("Channel2", "Band-00", 8.0),
            band("Channel2", "Band-10", 2.0),
            band("Channel2", "Band-VHF", 6.789),
        ],
        balance: vec![None; 8],
    }
}

fn at(seconds: i64) -> String {
    format!("t{seconds}")
}

#[test]
fn test_sessions_are_hidden_unless_shown() {
    let sessions = [summary(2, true), summary(1, false)];
    let hidden: Vec<i64> = rows(&sessions, false, at).iter().map(|r| r.id).collect();
    assert_eq!(hidden, vec![1]);
    let shown: Vec<i64> = rows(&sessions, true, at).iter().map(|r| r.id).collect();
    assert_eq!(shown, vec![2, 1]);
}

#[test]
fn a_row_names_its_start_length_sensors_and_bands() {
    let row = &rows(&[summary(1, false), summary(2, true)], true, at)[0];
    assert_eq!(row.title, "t100 · 4:00");
    assert_eq!(row.sensors, vec!["FP1", "FP2"]);
    let band = |name: &str, means: [&str; 2]| (name.to_owned(), means.map(str::to_owned).to_vec());
    assert_eq!(
        row.bands,
        vec![
            band("below 1 Hz", ["9.00", "8.00"]),
            band("1–3 Hz", ["1.23", "2.00"]),
            band("above 62 Hz", ["5.00", "6.79"]),
        ]
    );
    assert!(
        row.balance.is_empty(),
        "a session too short has no Balance Meter"
    );
    assert_eq!(
        rows(&[summary(2, true)], true, at)[0].title,
        "t200 · 4:00 · test"
    );
}

/// The report comes in design order: Activation (T3/T4) then Emotional (FP1/FP2), each
/// Before then After, each Low then High.
#[test]
fn the_balance_meter_shows_each_meter_as_a_share_of_its_scale_toward_the_stronger_side() {
    let report = [
        Some(0.6),
        Some(-1.25),
        None,
        Some(0.0),
        Some(2.0),
        Some(5.0),
        Some(0.2),
        Some(-3.0),
    ];
    let cells = |values: [&str; 4]| values.map(str::to_owned).to_vec();
    assert_eq!(
        balance(&report),
        vec![
            (
                "Emotional State (FP1/FP2)".to_owned(),
                cells(["100% right", "50% right", "75% left", "5% right"]),
            ),
            (
                "Activation State (T3/T4)".to_owned(),
                cells(["31% left", "15% right", "0% even", "—"]),
            ),
        ]
    );
}
