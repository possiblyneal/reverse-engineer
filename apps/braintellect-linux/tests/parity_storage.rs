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
}
