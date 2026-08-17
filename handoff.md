# dexrs Handoff

## Current State

The repository exists at `/Users/daniel/Developer/DanielCarmingham/dexrs`.
It has no Rust crate scaffold yet.

Created files:

- `backlog.md`
- `AGENTS.md`
- `docs/superpowers/specs/2026-08-17-dexrs-design.md`
- `handoff.md`

`backlog.md` is intentionally used instead of dex for durable project tracking
because dexrs is replacing dex itself.

## User-Approved Direction

Build a Rust-based clone of dex focused on local CLI usage for agents. The
primary fix over current dex is thread-safe/concurrent-safe JSONL storage so two
simultaneous writes do not corrupt or lose updates.

The user approved the MVP scope:

- `init`
- `dir`
- `status`
- `create` / `add`
- `list` / `ls`
- `show`
- `start`
- `complete` / `done`
- `edit` / `update`
- `delete` / `rm` / `remove`

Also include a `dex` shim so normal agent instructions that call `dex` can use
the Rust implementation. The current real dex resolves to
`/Users/daniel/Library/pnpm/bin/dex`; do not alter it unless the user asks.

## Next Step

Ask the user to review
`docs/superpowers/specs/2026-08-17-dexrs-design.md`. Once approved, write the
implementation plan, then scaffold and implement the Rust CLI.

