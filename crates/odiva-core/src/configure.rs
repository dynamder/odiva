//! What the environment says about the harness to start.
//!
//! `ODIVA_HARNESS` names the program, `ODIVA_HARNESS_ARGS` its arguments (space
//! separated). The default is dsh in its ACP profile — the only mode that speaks
//! the protocol on stdio. `ODIVA_DSH_HOME` moves dsh's own home when the default
//! one is not writable; `ODIVA_DSH_PERMISSION_MODE` overrides the sandbox
//! boundary it starts with.

use std::env;

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
