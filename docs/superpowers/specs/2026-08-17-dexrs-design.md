# dexrs MVP Design

## Goal

Build a Rust implementation of the local dex CLI workflow that agents can call
as a drop-in replacement for the existing `dex` command, while fixing JSONL
corruption and lost updates caused by concurrent writers.

## Scope

The first version is a local CLI-compatible MVP. It implements the commands
agents use most often:

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

The first version intentionally defers MCP, GitHub sync, Shortcut sync,
archive/import/export commands, shell completions, `plan`, and full config
management. The data model preserves metadata fields so later versions can add
those features without rewriting existing stores.

## Compatibility

dexrs stores tasks in the same JSONL shape as dex:

- Store directory: `.dex`
- Task file: `.dex/tasks.jsonl`
- One task object per line
- Stable task ordering by `id`
- Compatible fields: `id`, `parent_id`, `name`, `description`, `priority`,
  `completed`, `result`, `metadata`, `created_at`, `updated_at`, `started_at`,
  `completed_at`, `blockedBy`, `blocks`, and `children`

The parser accepts the current dex format and should preserve metadata it does
not understand by storing it as structured JSON.

## Store Discovery

Storage path resolution follows dex:

1. `DEX_STORAGE_PATH`, when set
2. `<git-root>/.dex`, when the current directory is inside a Git repository
3. `~/.config/dex/local`, when outside a Git repository

`dexrs dir` prints the resolved store directory. `dexrs init` creates the store
directory and an empty `tasks.jsonl` when needed.

## Concurrency Model

Every mutating command runs inside a locked transaction:

1. Open or create `.dex/tasks.lock`
2. Acquire an exclusive cross-process lock
3. Read and validate `tasks.jsonl`
4. Apply the command mutation in memory
5. Validate task relationships
6. Write the full JSONL payload to a temporary file in `.dex`
7. Flush and sync the temporary file
8. Atomically rename the temporary file over `tasks.jsonl`
9. Sync the store directory where supported
10. Release the lock

The lock protects the full read-modify-write cycle, not only the final write.
This prevents both malformed JSONL and lost updates when two agents write at
the same time.

Read commands normally read without taking the exclusive lock. If a read sees a
parse error that looks transient, it may retry briefly before reporting data
corruption. The atomic rename means readers should either see the old complete
file or the new complete file on normal filesystems.

## CLI UX

Output should be simple and scriptable, with the quiet terminal style already
used by dextui and dex:

- Status icons: `[ ]` todo, `[>]` in progress, `[x]` done
- Blocked indicator: `[B: <id>]` or `[B: <count>]`
- Tree connectors for hierarchical list output
- `--json` for commands where agents need structured output
- Color enabled only for TTY output and disabled by `NO_COLOR`

The command text should remain close enough to dex that existing agent
instructions keep working. Error messages should be concise, state the failed
validation, and include the command shape needed to fix it.

## `dex` Shim

The crate installs the canonical binary as `dexrs`. It also provides a `dex`
compatibility executable that runs the same CLI code.

The shim can be implemented as either:

- A second Cargo binary named `dex` that calls the shared CLI entrypoint
- A small installed wrapper that execs `dexrs`

The first implementation should prefer the second Cargo binary because it is
portable through `cargo install --path .` and does not rely on shell script line
endings or platform-specific install hooks.

Users can choose whether the Rust `dex` shim wins by PATH order. On this
machine the existing dex is currently installed at
`/Users/daniel/Library/pnpm/bin/dex`; if `~/.cargo/bin` is earlier on PATH after
installing dexrs, the Rust shim will be used automatically. Otherwise the npm
dex can be renamed, moved, uninstalled, or called by its absolute path when
needed. dexrs should not attempt to modify the user's existing dex installation.

## Data Model

Tasks are modeled as Rust structs with serde derives. Known metadata fields can
be typed later, but the MVP should represent `metadata` as
`Option<serde_json::Value>` or a shallow struct with flattened unknown data so
existing stores round-trip safely.

Task ids use dex's eight-character lowercase alphanumeric style. Creation must
check for collisions inside the locked transaction before committing the new
task.

Relationship invariants:

- A task's `parent_id` must reference an existing task when non-null
- Parent `children` arrays must match child `parent_id` values
- `blockedBy` and `blocks` must be bidirectional
- Blocking cycles are rejected
- Completing a task with incomplete children fails unless `--force` is passed

## Testing

The MVP needs tests before it is considered usable as the agent-facing `dex`
command:

- JSONL fixture tests for parsing current dex task files
- Store discovery tests for git repo, non-repo, and `DEX_STORAGE_PATH`
- CLI tests for each implemented command
- Golden output tests for list/show/status basics
- Concurrent writer regression test that spawns many processes creating tasks
  in the same store and verifies valid JSONL with no lost task ids
- Transaction tests proving failed validation leaves the old store unchanged

## Build And Release Shape

The project should start as a normal Rust binary crate using edition 2024.
Likely dependencies:

- `clap` for argument parsing
- `serde` and `serde_json` for data compatibility
- `anyhow` and `thiserror` for errors
- `chrono` or `time` for timestamps
- `fs4` or `fd-lock` for cross-process locking
- `tempfile` for write staging and tests
- `assert_cmd` and `predicates` for CLI tests

The first milestone is source installable with `cargo install --path .`, placing
both `dexrs` and `dex` in Cargo's bin directory.

