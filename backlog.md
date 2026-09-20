# dexrs Backlog

This project intentionally avoids using dex as the source of truth for its own
work tracking. dexrs replaces dex behavior, including storage and command
semantics, so dogfooding dex here can hide or create failures while the CLI is
incomplete.

## Active

No active implementation task.

## Backlog


## Deferred

- [ ] MCP server support
- [ ] GitHub sync
- [ ] Shortcut sync
- [ ] Import/export commands
- [ ] `dex config`

## Done

- [x] Write dex-compatible task records (`description` string, numeric
      `priority` defaulting to 1), verified against the real dex binary
- [x] Add `--result` / `-r`, `--commit`, and `--no-commit` to `complete`
- [x] Add `--parent` and `--blocked-by` to `create`; `--parent`,
      `--add-blocker`, `--remove-blocker`, and `--commit` to `edit`
- [x] Add `list` filters (`--all`, `--completed`, `--in-progress`,
      `--blocked`, `--ready`, `--flat`, positional id/search) and tree view
- [x] Add `show --full`, multiple ids to `show`, `start --force`,
      `delete --force`, and `status` as the default command
- [x] Match the original `status` dashboard and `--json` shape
- [x] Implement `plan <file>`
- [x] Implement `archive`, `list --archived`, and archived `show`
- [x] Add `list --issue` and `list --commit` lookups with the `[GH-n]` tag
- [x] Add `completion <shell>`

- [x] Write the dexrs MVP design spec
- [x] Write the dexrs MVP implementation plan
- [x] Scaffold the Rust CLI project
- [x] Implement project-local store discovery
- [x] Implement `init` and `dir`
- [x] Implement dex-compatible task data types and JSONL parsing
- [x] Add CLI golden tests against dex-compatible fixtures
- [x] Implement cross-process locked store transactions
- [x] Implement atomic JSONL writes
- [x] Add concurrent writer regression tests
- [x] Implement core task relationship validation
- [x] Implement `create` / `add`
- [x] Implement `start`
- [x] Implement `complete` / `done`
- [x] Implement `edit` / `update`
- [x] Implement `delete` / `rm` / `remove`
- [x] Implement `list` / `ls`
- [x] Implement `show`
- [x] Implement `status`
- [x] Add the `dex` compatibility shim
- [x] Add install documentation
