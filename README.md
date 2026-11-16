# odiva

A desk companion that talks like a chat app and dispatches coding harnesses.

The window is a plain messenger: messages, files, quick replies. It has no
concept of tasks, progress or harnesses — everything it shows, the agent chose to
say. That is the whole design; see `docs/000-overview.md`.

## Layout

```
crates/
├── odiva-proto/   the seam: messages, presence, a `CoreHandle` trait. No deps.
├── odiva/         the chat window (iced). Holds no truth, only layout.
├── odiva-core/    the core: the ACP client, and a harness that does as it is told
└── ericed/        vendored from ../sofaia — widgets, palette, windowed list
```

`docs/`:

| | |
| --- | --- |
| [000](docs/000-overview.md) | the three faces, three invariants, what is out of scope |
| [001](docs/001-protocol.md) | the window↔core protocol, and why it has no task in it |
| [002](docs/002-ui.md) | the window: rules, palette, the layout ledger, iced's traps |
| [003](docs/003-plan.md) | milestones, and what is actually built |
| [004](docs/004-memory.md) | markdown + tantivy, tool-first retrieval, no embeddings |
| [005](docs/005-execution.md) | one middleman, N ACP workers, the task ledger |
| [006](docs/006-core.md) | the core: agent = context + tools, lanes, `say`, budgets |

## Build

```sh
cargo test --workspace            # protocol, layout, and the ericed examples
cargo test -p odiva --test render # rewrite target/ui-*.png for review
cargo run -p odiva                # open the window, connected to the core
```

The window owns no conversation: `ODIVA_HOME` says where the core keeps its lanes
(defaults to `./.odiva`), and `ODIVA_HARNESS` names the harness program it may
start (defaults to `dsh`).

The screenshots in `target/` are the design review loop: each one is a script of
`CoreMessage`s replayed into a fresh window, so a picture that cannot be produced
means the protocol stopped being enough.

Both executables write `$ODIVA_HOME/odiva.log` (`./.odiva/odiva.log` by default):
what was asked, whether she spoke, what the model said when she did not, and how
tasks ended. A window started from a shortcut has no terminal, and silence on
screen means two different things until someone writes it down.

## State

The window is connected to the core. `main` builds the engine and spawns it;
`App::subscription` turns its `CoreMessage`s into `Message::CoreReported`, so the
only thing that can change the transcript is still `App::apply`. The protocol is
down to three variants: a message either arrived whole or did not arrive.

`crates/odiva-core` has the engine, the talk lane, `say`, and the task ledger. A
turn still needs a harness on `PATH` and a model credential; without them the
window opens and attaches, and a turn fails with a sentence on stderr. The design
is [006](docs/006-core.md).

Dependencies are fetched with escalation: the sandboxed toolchain cannot reach
the TLS credential store, so `cargo add` and `cargo fetch` need it once. After
that the tree builds with `--offline`.
