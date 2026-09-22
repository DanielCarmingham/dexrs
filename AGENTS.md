# Agent Instructions

## Project Purpose

dexrs is a Rust replacement for the local dex CLI workflow. The first milestone
is a source-installable MVP that can be called as either `dexrs` or `dex`, uses
dex-compatible JSONL task storage, and fixes concurrent writer corruption.

## Task Tracking

Do not use dex as the durable source of truth for this repository by default.
This project is implementing dex behavior, including storage and command
semantics, so dogfooding dex here can create confusing failures while the CLI is
incomplete.

Use `backlog.md` for project task tracking unless the user explicitly asks to
use dex for a specific operation.

## Current Design

Read the MVP spec before implementation:

- `docs/superpowers/specs/2026-08-17-dexrs-design.md`

Key requirements:

- Preserve `.dex/tasks.jsonl` compatibility with the current dex task shape.
- Resolve stores like dex: `DEX_STORAGE_PATH`, then `<git-root>/.dex`, then
  `~/.config/dex/local`.
- Wrap every mutating command in an exclusive cross-process locked transaction.
- Write JSONL via temp file, fsync, atomic rename, and directory sync where
  supported.
- Install a canonical `dexrs` binary and a compatibility `dex` binary that runs
  the same CLI code.
- Do not modify, uninstall, or move the user's existing npm/pnpm dex
  installation unless explicitly asked.

## Initial Scope

Implement these local commands first:

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

Defer MCP, GitHub sync, Shortcut sync, archive/import/export commands, shell
completions, `plan`, and full config management.

## Style

Keep output simple, clean, and agent-friendly. Use dextui as UX inspiration for
status language and quiet terminal presentation, but do not build a TUI for the
MVP.


## Releasing

The crate is published as **`dex-cli`** (the `dexrs` name on crates.io belongs
to an unrelated Dexcom library); the binaries it installs are still `dexrs` and
`dex`. A release goes out on three channels: crates.io, a GitHub Release with
prebuilt binaries for four targets, and a Homebrew formula pushed to
`DanielCarmingham/homebrew-tap`. Only the first is manual.

```bash
# 1. Bump the version in Cargo.toml and rename CHANGELOG.md's [Unreleased]
#    heading to [<version>] - <YYYY-MM-DD>; commit both together. The tag will
#    point at this commit, and dist builds the release notes from it.
# 2. Verify:
cargo test
cargo clippy --all-targets -- -D warnings
dist plan
dist generate --check        # release.yml still matches dist-workspace.toml
dist plan --output-format=json \
  | python3 -c "import json,sys; print(json.load(sys.stdin).get('announcement_changelog') or 'NO RELEASE NOTES FOUND')"
cargo publish --dry-run
# 3. crates.io first, because it is the irreversible one.
cargo publish
# 4. Tag the exact commit that was published; the push triggers release.yml.
git tag v<version>
git push origin main v<version>
gh run watch --repo DanielCarmingham/dexrs
```

`release.yml` is generated from `dist-workspace.toml` by `dist generate`;
never hand-edit it. Cargo refuses to publish from a dirty tree, so the bump
commit must land before any dry run.
