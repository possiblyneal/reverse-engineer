//! The ignored replays' inputs, which stay outside Git: a path from an environment
//! variable, else a default under the repository root's `tmp/`.

use std::fs;
use std::path::PathBuf;

/// `$var`, else `default` (a path such as `tmp/s55/replay`) under the repository root.
pub fn path(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var).map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(default)
        },
        PathBuf::from,
    )
}

/// The text of `name` in the directory [`path`] gives; panics naming the file.
pub fn read(var: &str, default: &str, name: &str) -> String {
    let file = path(var, default).join(name);
    fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()))
}
