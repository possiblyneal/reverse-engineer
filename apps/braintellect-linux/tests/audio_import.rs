//! Importing the original's instrument banks into the user's data directory, and
//! the sink's behavior without a usable ALSA. Nothing here opens a device.

use std::fs;
use std::path::PathBuf;

use braintellect_linux::audio::import::{bank_path, import_banks, ImportError};
use braintellect_linux::audio::sink::{AlsaSink, SinkError};
use braintellect_linux::storage;

/// A fresh, empty directory under cargo's per-run scratch directory.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("audio-import-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

#[test]
fn every_instrument_dll_is_copied_and_nothing_else() {
    let lib = scratch("lib");
    for (name, body) in [
        ("dll1001.BT.dll", &b"one"[..]),
        ("dll1002.BT.dll", b"two"),
        ("dll1003.BT.dll", b"three"),
        ("braintellect.dll", b"other"),
        ("dllX.BT.dll", b"not a number"),
        ("dll1004.BT.dll.bak", b"backup"),
    ] {
        fs::write(lib.join(name), body).expect("write");
    }
    let dest = scratch("dest").join("instruments");
    let copied = import_banks(&lib, &dest).expect("import");
    assert_eq!(
        copied,
        [
            dest.join("dll1001.BT.dll"),
            dest.join("dll1002.BT.dll"),
            dest.join("dll1003.BT.dll")
        ]
    );
    assert_eq!(fs::read(bank_path(&dest, 1002)).expect("read"), b"two");
    assert_eq!(fs::read_dir(&dest).expect("dir").count(), 3);
}

#[test]
fn importing_twice_overwrites_with_the_current_files() {
    let lib = scratch("lib2");
    let dest = scratch("dest2");
    fs::write(lib.join("dll1002.BT.dll"), b"old").expect("write");
    import_banks(&lib, &dest).expect("first");
    fs::write(lib.join("dll1002.BT.dll"), b"new").expect("write");
    import_banks(&lib, &dest).expect("second");
    assert_eq!(fs::read(bank_path(&dest, 1002)).expect("read"), b"new");
}

#[test]
fn a_directory_without_banks_or_a_missing_one_is_an_error() {
    let dest = scratch("dest3");
    assert!(matches!(
        import_banks(&scratch("empty"), &dest),
        Err(ImportError::NoBanks)
    ));
    let missing = scratch("parent").join("nope");
    assert!(matches!(
        import_banks(&missing, &dest),
        Err(ImportError::Io(_))
    ));
}

#[test]
fn the_instruments_directory_sits_in_the_storage_data_directory() {
    let data = storage::resolve_data_dir(Some("/x/data".into()), Some("/home/u".into()));
    assert_eq!(
        data.map(|d| d.join("instruments")),
        Some("/x/data/braintellect-linux/instruments".into())
    );
}

#[test]
fn importing_a_directory_onto_itself_is_refused_and_leaves_the_banks_intact() {
    let dir = scratch("same");
    fs::write(dir.join("dll1002.BT.dll"), b"bank bytes").expect("write");
    assert!(matches!(
        import_banks(&dir, &dir),
        Err(ImportError::SameDirectory)
    ));
    // Also through a different spelling of the same directory.
    let alias = dir.join(".").join("..").join("audio-import-same");
    assert!(matches!(
        import_banks(&dir, &alias),
        Err(ImportError::SameDirectory)
    ));
    assert_eq!(
        fs::read(bank_path(&dir, 1002)).expect("read"),
        b"bank bytes"
    );
}

#[test]
fn a_missing_alsa_library_is_an_error_not_a_crash() {
    let result = AlsaSink::open_from("libno-such-alsa.so.9", "default", 96_000);
    assert!(matches!(result, Err(SinkError::LibraryMissing(_))));
}
