# AGENTS.md — opsh

## Product

**opsh** — small local-first shell written in Rust.

Binary: `opsh`. Everything else passes through to `/bin/sh` (or `$OPSH_SHELL`).

Local-first. No daemon. No network. History/rc via `optionSDK` (`~/.option/opsh/`).

## After every change

When you finish a task that touches code:

1. **Build** — always verify:

   ```bash
   export CARGO_TARGET_DIR="$(pwd)/target"
   cargo fmt --check
   cargo test
   cargo build --release
   ```

2. **Install to PATH** — refresh so `opsh` matches the tree:

   ```bash
   export CARGO_TARGET_DIR="$(pwd)/target"
   cargo install --path . --force --offline
   ```

Do **not** leave the user on a stale `~/.cargo/bin/opsh`.

## Sandbox / target dir

If the binary looks stale, check `CARGO_TARGET_DIR`. Prefer `export CARGO_TARGET_DIR="$(pwd)/target"`.

## Stack notes

- `optionSDK` via path `../optionSDK` (`App::OPSH`, `ensure()`, `migrate_file`, `color_enabled`)
- State: `~/.option/opsh/history`, `~/.option/opsh/rc` (`$OPSH_HISTORY` / `$OPSH_RC` override; legacy XDG migrated)
- External commands via POSIX shell, never fish; `&&/||/;` stay in-process so `cd` persists
- Prompt knobs: `OPSH_PROMPT`, `OPSH_PROMPT_STYLE`, colors `OPSH_COLOR_*`; respect `NO_COLOR` + `TERM=dumb`
- Interactive: `rustyline` (`↑/↓`, `Ctrl+R`, `!!`/`!N`, tab completion)

## Release channels

See [VERSIONING.md](VERSIONING.md). Single surface `opsh` — no `m` in tag. Do **not** label `stable` unless release-ready.

## Don’t

- Commit or push unless the user asks
- Force-push / skip hooks / amend pushed commits
- Add daemon/network/telemetry or break POSIX passthrough
