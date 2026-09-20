# dexrs

Rust implementation of the local dex CLI workflow. The MVP stores tasks in
dex-compatible `.dex/tasks.jsonl` files and wraps every mutation in a locked,
atomic JSONL transaction.

## Install From Source

```bash
cargo install --path .
```

This installs two binaries:

- `dexrs`: canonical binary
- `dex`: compatibility binary that runs the same CLI code

Which `dex` runs depends on PATH order. This project does not modify, move, or
uninstall any existing npm/pnpm dex installation.

## Implemented Commands

The store format, config files, and the flags below match dex v0.16, so the
two tools can share a `.dex/tasks.jsonl` file and a `dex.toml`.

Global options: `--config <path>`, `--storage-path <path>`, `--version`.

- `init`, `-y`, `--config-dir` (writes the default `~/.config/dex/dex.toml`)
- `dir`, `--global`
- `config <key>[=<value>]`, `--unset`, `--list`, `--global`, `--local`
- `status` (also the default when no command is given)
- `create` / `add` `"name"` or `-n`, `-d`, `-p`, `--parent`, `--blocked-by`
- `list` / `ls` `[id|search]`, `--all`, `--completed`, `--in-progress`,
  `--blocked`, `--ready`, `--issue <n>`, `--commit <sha>`, `--archived`,
  `--flat`, `--json`
- `show <id>...`, `--full`, `--expand`, `--json`
- `start <id>`, `--force`
- `complete` / `done` `<id> --result "..."`, `--commit <sha>`, `--no-commit`,
  `--force`
- `edit` / `update` `<id>`, `-n`, `-d`, `-p`, `--parent`, `--add-blocker`,
  `--remove-blocker`, `--commit`
- `delete` / `rm` / `remove` `<id>`, `--force`
- `plan <file>`, `-p`, `--parent`
- `archive <id>` or `--completed` or `--older-than 30d`, with `--except`
  and `--dry-run`; archived tasks go to `archive.jsonl` in the original's
  format and are visible via `list --archived` and `show`

- `completion <bash|zsh|fish|...>`, named after the invoked binary

Not implemented: `mcp`, `sync`, `import`, and `export`.

## Store Resolution

Same precedence as original dex: `--storage-path`, then `storage.file.path`
from config, then `DEX_STORAGE_PATH`, then either the centralized project
directory under the dex home (`storage.file.mode = "centralized"`) or
`<git-root>/.dex`, falling back to `<dex home>/local` outside a repository.
The dex home is `DEX_HOME`, else `$XDG_CONFIG_HOME/dex`, else `~/.config/dex`.

## Testing Against the Original

Set `DEX_REFERENCE_BIN` to the original dex executable to make the test suite
also verify that a store written by dexrs is accepted by it:

```bash
DEX_REFERENCE_BIN="$(command -v dex)" cargo test
```
