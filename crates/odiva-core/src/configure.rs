//! What the environment says about the harness to start.
//!
//! `ODIVA_HARNESS` names the program, `ODIVA_HARNESS_ARGS` its arguments (space
//! separated). The default is dsh in its ACP profile — the only mode that speaks
//! the protocol on stdio. `ODIVA_DSH_HOME` moves dsh's own home when the default
//! one is not writable; `ODIVA_DSH_PERMISSION_MODE` overrides the sandbox
//! boundary it starts with.

use std::env;
use std::path::PathBuf;

use odiva_harness::Harness;

pub fn harness() -> Harness {
    let program = env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned());
    let args = env::var("ODIVA_HARNESS_ARGS").unwrap_or_else(|_| "--profile acp".to_owned());

    let mut harness = Harness::new(program);

    for arg in args.split_whitespace() {
        harness = harness.arg(arg);
    }

    harness = harness.env(
        "DSH_PERMISSION_MODE",
        env::var("ODIVA_DSH_PERMISSION_MODE").unwrap_or_else(|_| "workspace-write".to_owned()),
    );

    if let Ok(home) = env::var("ODIVA_DSH_HOME") {
        harness = harness.env("DSH_HOME", home);
    }

    harness
}

/// A startup warning when dsh's own home would stop it from booting.
///
/// dsh writes `profiles/<name>/cordis.yml` on every boot, so a read-only home
/// fails before the handshake — with nothing in our log to say why.
pub fn warning() -> Option<String> {
    let program = env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned());

    if !program.to_lowercase().contains("dsh") {
        return None;
    }

    let home = env::var_os("ODIVA_DSH_HOME")
        .or_else(|| env::var_os("DSH_HOME"))
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".dsh")))?;

    let target = if home.is_dir() {
        home.clone()
    } else {
        home.parent()?.to_path_buf()
    };

    let probe = target.join(".odiva-write-probe");

    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            None
        }
        Err(error) => Some(format!(
            "DSH_HOME {} 不可写（{error}）：dsh 启动会在 cordis.yml 上失败。把 ODIVA_DSH_HOME 指到一个可写目录",
            home.display()
        )),
    }
}
