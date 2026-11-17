//! Where the workspace's `fake_harness` binary is.
//!
//! `CARGO_BIN_EXE_*` only exists for the package that owns the binary, and the
//! fake peer belongs to `odiva-harness`. So the tests find it where cargo put it,
//! and say so plainly when it is missing.

use std::path::{Path, PathBuf};

pub fn fake_harness() -> PathBuf {
    let name = if cfg!(windows) {
        "fake_harness.exe"
    } else {
        "fake_harness"
    };

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };

    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));

    let path = target.join(profile).join(name);

    assert!(
        path.is_file(),
        "{} 不存在：用 `cargo test --workspace` 跑，它会先建 odiva-harness 的 fake_harness",
        path.display()
    );

    path
}
