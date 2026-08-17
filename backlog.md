# dexrs Backlog

This project intentionally avoids using dex as the source of truth for its own
work tracking. dexrs replaces dex behavior, including storage and command
semantics, so dogfooding dex here can hide or create failures while the CLI is
incomplete.

## Active

- [ ] Task 5: Implement core task relationship validation

## Backlog

- [ ] Implement core task relationship validation
- [ ] Implement `create` / `add`
- [ ] Implement `list` / `ls`
- [ ] Implement `show`
- [ ] Implement `start`
- [ ] Implement `complete` / `done`
- [ ] Implement `edit` / `update`
- [ ] Implement `delete` / `rm` / `remove`
- [ ] Add the `dex` compatibility shim
- [ ] Add CLI golden tests against dex-compatible fixtures
- [ ] Add concurrent writer regression tests

## Deferred

- [ ] MCP server support
- [ ] GitHub sync
- [ ] Shortcut sync
- [ ] Archive/import/export commands
- [ ] Shell completions
- [ ] `dex plan`
- [ ] `dex config`

## Done

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
