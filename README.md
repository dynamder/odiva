# odiva

A desk companion that talks like a chat app and dispatches coding harnesses.

The window is a plain messenger: messages, files, quick replies, and — when the agent needs a
decision — one blocking question card instead of the composer. It has no concept of tasks,
progress or harnesses; everything it shows, the agent chose to say. That is the whole design;
see `docs/000-overview.md`.

## Layout

```
crates/
├── odiva-proto/     the seam: messages, presence, a blocking question, a `CoreHandle` trait
├── odiva-harness/   ACP: processes, sessions, `Run`, escalation. The only place ACP types live
├── odiva/           the chat window (iced). Holds no truth, only layout
├── odiva-core/      the core: the `Odiva` actor, context lines, the ledger, the tools
└── ericed/          vendored from ../sofaia — widgets, palette, windowed list
```

`docs/`:

| | |
| --- | --- |
| [000](docs/000-overview.md) | the three faces, three invariants, what is out of scope |
| [001](docs/001-protocol.md) | the window↔core protocol: messages, the question card, and why it has no task in it |
| [002](docs/002-ui.md) | the window: rules, palette, the layout ledger, iced's traps |
| [003](docs/003-plan.md) | migration phases P0–P6, what is actually built, what is blocked |
| [004](docs/004-memory.md) | markdown + tantivy, tool-first retrieval, no embeddings |
| [005](docs/005-execution.md) | harness processes, runs, quotas, pause/resume, the task ledger |
| [006](docs/006-core.md) | outside facts: ACP, harness products, yoagent |
| [007](docs/007-core.md) | the core: states, context lines, four rosters, cancellation, questions, escalation |

## Build

```sh
cargo test --workspace            # protocol, layout, and the ericed examples
cargo test -p odiva --test render # rewrite target/ui-*.png for review
cargo run -p odiva                # open the window, connected to the core
```

The window owns no conversation: `ODIVA_HOME` says where the core keeps its lines
(defaults to `./.odiva`). The harness it starts is `ODIVA_HARNESS` + `ODIVA_HARNESS_ARGS`
(defaults to `dsh --profile acp`), with `ODIVA_DSH_HOME` / `ODIVA_DSH_PERMISSION_MODE`
for dsh itself.

Before trusting the wiring, ask it one question that costs nothing:

```sh
cargo run -p odiva-core --example doctor            # handshake + session/new only
cargo run -p odiva-core --example doctor -- D:\work
```

The screenshots in `target/` are the design review loop: each one is a script of
`CoreMessage`s replayed into a fresh window, so a picture that cannot be produced
means the protocol stopped being enough.

Both executables write `$ODIVA_HOME/odiva.log` (`./.odiva/odiva.log` by default):
what was asked, whether she spoke, what the model said when she did not, and how
tasks ended. A window started from a shortcut has no terminal, and silence on
screen means two different things until someone writes it down.

The same file carries whatever iced has to say about the window itself — the one
worth knowing is "More than 3 consecutive RedrawRequested events produced layout
invalidation", which means a frame never settled. `ODIVA_LOG=debug` adds a line
per message with the four numbers that decide what the transcript shows, so a
list that moves on its own says which number moved:

```sh
ODIVA_LOG=debug cargo run -p odiva
```

## State

The window is connected to the core. `main` starts the runtime and the `Odiva` actor;
`App::subscription` turns its `CoreMessage`s into `Message::CoreReported`, so the
only thing that can change the transcript is still `App::apply`.

The migration in [003](docs/003-plan.md) is done through P5: the ACP client is its own crate,
lines are `yoagent::Session` files with a separate transcript, one `Odiva` actor owns the state
machine (idle/busy, four tool rosters, questions, escalations) and a `Records` actor owns the
ledger. What is left is P6: the example and this file, and the small operational gaps listed as
待定 in 003.

A turn still needs a harness on `PATH` and a model credential; without them the
window opens and attaches, and a turn fails with a sentence on stderr.

Dependencies are fetched with escalation: the sandboxed toolchain cannot reach
the TLS credential store, so `cargo add` and `cargo fetch` need it once. After
that the tree builds with `--offline`.

## License

Dual-licensed, at your option, under either

- the [Apache License, Version 2.0](LICENSE-APACHE), or
- the [MIT license](LICENSE-MIT).

`crates/ericed` is vendored from the same author's `sofaia` and carries the same
terms.
