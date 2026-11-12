//! Session-storage layer replayed through the parity harness: the baseline
//! record is saved to the clone's SQLite store, read back through the public
//! API, and compared to the baseline record fixture.

mod harness;

use std::path::PathBuf;

use braintellect_linux::storage::{resolve_path, Error, Store, SCHEMA_VERSION};
use harness::{assertions, fixtures, session};

/// A fresh, empty database path under cargo's per-run scratch directory.
fn db_path(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(format!("storage-{name}.db"));
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn saved_session_reads_back_as_the_baseline_record() {
    let baseline = fixtures::stored_record();
    let store = Store::open(&db_path("parity")).expect("open");
    let id = store
        .save(&session::from_baseline(&baseline))
        .expect("save");

    let loaded = store.load(id).expect("load");
    assertions::stored_record(&baseline, &session::to_stored_record(&loaded))
        .expect("stored record parity");
}

#[test]
fn record_persists_across_reopen_in_one_database() {
    let path = db_path("reopen");
    let baseline = fixtures::stored_record();
    let first = {
        let store = Store::open(&path).expect("open");
        store
            .save(&session::from_baseline(&baseline))
            .expect("save")
    };

    let store = Store::open(&path).expect("reopen");
    let second = store
        .save(&session::from_baseline(&baseline))
        .expect("save");
    assert_ne!(first, second, "sessions get distinct ids");
    assert_eq!(store.list().expect("list"), vec![first, second]);
    let loaded = store.load(first).expect("load after reopen");
    assertions::stored_record(&baseline, &session::to_stored_record(&loaded))
        .expect("stored record parity after reopen");
}

#[test]
fn loading_an_unknown_session_is_an_error() {
    let store = Store::open(&db_path("unknown")).expect("open");
    assert!(matches!(store.load(99), Err(Error::NotFound(99))));
}

#[test]
fn database_from_a_different_schema_version_is_refused() {
    let path = db_path("version");
    drop(Store::open(&path).expect("open"));
    let other = SCHEMA_VERSION + 1;
    rusqlite::Connection::open(&path)
        .expect("raw open")
        .pragma_update(None, "user_version", other)
        .expect("bump version");
    assert!(matches!(
        Store::open(&path),
        Err(Error::SchemaVersion { found, .. }) if found == other
    ));
}

#[test]
fn location_follows_xdg_with_home_fallback() {
    assert_eq!(
        resolve_path(Some("/x/data".into()), Some("/home/u".into())),
        Some(PathBuf::from("/x/data/braintellect-linux/sessions.db"))
    );
    assert_eq!(
        resolve_path(None, Some("/home/u".into())),
        Some(PathBuf::from(
            "/home/u/.local/share/braintellect-linux/sessions.db"
        ))
    );
    assert_eq!(resolve_path(None, None), None);
    // The XDG spec says a relative path in `XDG_DATA_HOME` is invalid and ignored.
    assert_eq!(
        resolve_path(Some("rel/data".into()), Some("/home/u".into())),
        Some(PathBuf::from(
            "/home/u/.local/share/braintellect-linux/sessions.db"
        ))
    );
}

/// A baseline-header session started at `started_at` whose two channels carry
/// `Band-10` and `Band-80` series, and an `LMH` series and an FFT the summary leaves out.
fn banded(started_at: i64) -> braintellect_linux::storage::SessionRecord {
    use braintellect_linux::storage::{Channel, Point, Series};
    let series = |name: &str, ys: &[f64]| Series {
        name: name.to_owned(),
        points: ys
            .iter()
            .enumerate()
            .map(|(i, &y)| Point { x: i as f64, y })
            .collect(),
    };
    let mut record = session::from_baseline(&fixtures::stored_record());
    record.started_at = started_at;
    record.channels = (1..=2)
        .map(|n| Channel {
            name: format!("Channel{n}"),
            series: vec![
                series("Band-10", &[n as f64, 3.0 * n as f64]),
                series("Band-80", &[10.0, 20.0]),
                series("High 1&2", &[7.0]),
                series("FFT-Normal-Order", &[99.0]),
            ],
        })
        .collect();
    record
}

#[test]
fn summaries_list_newest_first_with_band_means() {
    use braintellect_linux::storage::BandMean;
    let store = Store::open(&db_path("summaries")).expect("open");
    let older = store.save(&banded(1_000)).expect("save");
    let newer = store.save(&banded(2_000)).expect("save");
    assert_eq!(store.load(newer).expect("load").started_at, 2_000);

    let summaries = store.summaries().expect("summaries");
    let ids: Vec<i64> = summaries.iter().map(|s| s.id).collect();
    assert_eq!(ids, vec![newer, older]);
    let mean = |channel: &str, series: &str, mean: f64| BandMean {
        channel: channel.to_owned(),
        series: series.to_owned(),
        mean,
    };
    assert_eq!(
        summaries[0].bands,
        vec![
            mean("Channel1", "Band-10", 2.0),
            mean("Channel1", "Band-80", 15.0),
            mean("Channel2", "Band-10", 4.0),
            mean("Channel2", "Band-80", 15.0),
        ]
    );
    assert!(!summaries[0].test);
}

#[test]
fn a_session_marked_as_a_test_can_be_unmarked() {
    let store = Store::open(&db_path("test-mark")).expect("open");
    let id = store.save(&banded(1_000)).expect("save");
    store.set_test(id, true).expect("mark");
    assert!(store.summaries().expect("summaries")[0].test);
    store.set_test(id, false).expect("unmark");
    assert!(!store.summaries().expect("summaries")[0].test);
    assert!(matches!(store.set_test(99, true), Err(Error::NotFound(99))));
}

#[test]
fn a_session_keeps_its_capture_file() {
    let store = Store::open(&db_path("capture")).expect("open");
    let mut record = banded(1_000);
    record.capture = Some("captures/b2-1000.rx".to_owned());
    let id = store.save(&record).expect("save");
    assert_eq!(store.load(id).expect("load"), record);
}

/// The Balance Meter of a summary comes from the stored `High` and `Low` series of
/// channels 1 to 4, each pair's `A` the odd channel and `B` the even one.
#[test]
fn summaries_carry_the_balance_meter_of_the_lmh_series() {
    use braintellect_linux::feedback::circle_balance::balance_meter;
    use braintellect_linux::storage::{Channel, Point, Series};
    let flat = |name: String, y: f64| Series {
        name,
        points: (1..=48)
            .map(|i| Point {
                x: f64::from(i) * 5000.0,
                y,
            })
            .collect(),
    };
    let mut record = banded(1_000);
    record.duration_s = 240;
    record.channels = (1..=4)
        .map(|n| {
            let pair = if n <= 2 { "1&2" } else { "3&4" };
            Channel {
                name: format!("Channel{n}"),
                series: vec![
                    flat(format!("High {pair}"), f64::from(n)),
                    flat(format!("Mid {pair}"), 100.0),
                    flat(format!("Low {pair}"), 10.0 * f64::from(n)),
                ],
            }
        })
        .collect();
    let store = Store::open(&db_path("balance")).expect("open");
    store.save(&record).expect("save");

    let (high, low) = (balance_meter(3.0, 4.0), balance_meter(30.0, 40.0));
    let (emotional_high, emotional_low) = (balance_meter(1.0, 2.0), balance_meter(10.0, 20.0));
    assert_eq!(
        store.summaries().expect("summaries")[0].balance,
        [
            low,
            high,
            low,
            high,
            emotional_low,
            emotional_high,
            emotional_low,
            emotional_high
        ]
        .map(Some)
        .to_vec()
    );
}
