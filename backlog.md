# dexrs Backlog

This project intentionally avoids using dex as the source of truth for its own
work tracking. dexrs replaces dex behavior, including storage and command
semantics, so dogfooding dex here can hide or create failures while the CLI is
incomplete.

## Active

No active implementation task.

## Backlog


## Deferred

Nothing. Full parity with dex v0.16 is the goal.

## Done

- [x] Sync foundation: sync and archive config keys, metadata comment
      formats for issues and stories, sync-state file, three-level depth
      limit, auto-archive on write
- [x] GitHub sync service with create/update, labels, skip-unchanged,
      pull-from-remote, and not-closing reasons, tested against a mock API
- [x] Shortcut sync service with stories, subtasks, blocker links, workflow
      states, and the label cache, tested against a mock API
- [x] `sync`, `import`, and `export` commands, auto-sync after mutations,
      and closing remote issues on delete
- [x] `mcp` stdio server with create_task, update_task, and list_tasks
- [x] `doctor` with `--fix`, `help`, `version`, and unknown-command
      suggestions

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
- [x] Honor dex config files, `--config`, `--storage-path`, centralized mode;
      add `config`, `dir --global`, `--version`, and original `init` semantics
- [x] Match original `show` layout and enriched `--json` shape
- [x] Match mutation output wording and task cards, blocker warning, parent
      hint, and the `--commit`/`--no-commit` requirement for linked leaf tasks

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
