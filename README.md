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
├── ericed/        vendored from ../sofaia — widgets, palette, windowed list
└── odiva-core/    not started: agent loop, ACP client, memory
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

## Build

```sh
cargo test --workspace            # protocol, layout, and the ericed examples
cargo test -p odiva --test render # rewrite target/ui-*.png for review
cargo run -p odiva                # open the window (fixtures, no core yet)
```

The screenshots in `target/` are the design review loop: each one is a script of
`CoreMessage`s replayed into a fresh window, so a picture that cannot be produced
means the protocol stopped being enough.

## State

`crates/odiva` and `crates/odiva-proto` are real. `crates/odiva-core` does not
exist yet, so the window runs on fixtures — there is no agent behind it and
nothing talks to a harness.
