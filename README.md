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
