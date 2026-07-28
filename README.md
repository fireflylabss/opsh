# ◆ opsh

**opsh** — a small, local-first shell written in Rust.

It keeps the interface deliberately quiet: a color-aware prompt, useful local history and a compact set of built-ins. Everything else is passed to your configured system shell, so familiar pipes, redirects, environment variables and scripts keep working.

```text
◆ opsh  local shell
  type help for built-ins · Ctrl+D to exit

◆ ~/AEFireflyLabs/opsh
› pwd
/home/firefly/AEFireflyLabs/opsh
```

## Install

Requires Rust 1.85+.

```bash
cargo install --path .
opsh
```

Or run from the checkout:

```bash
cargo run --release
```

## Usage

```bash
opsh
opsh -c 'echo hello | tr a-z A-Z'
opsh --help
```

### Built-ins

| Command | Description |
|---|---|
| `cd [DIR]` | Change the current directory; defaults to `HOME` |
| `pwd` | Print the current directory |
| `history` | List commands saved locally |
| `status` | Show the previous command status |
| `which CMD` | Locate a built-in or executable |
| `mkdir DIR...` | Create directories |
| `mkcd DIR` | Create and enter a directory |
| `touch FILE...` | Create files when absent |
| `open PATH` | Open with the desktop default application |
| `set NAME VALUE` / `unset NAME` | Change the shell environment |
| `source FILE` / `. FILE` | Run local opsh commands |
| `clear` | Clear the interactive screen |
| `about` | Show version and project information |
| `help` | Show built-ins |
| `exit [N]` | Leave with optional status code |

External commands run through `$SHELL`, falling back to `/bin/sh`. This means shell syntax such as `|`, `>`, `&&`, `$(...)` and `$VARIABLE` has the expected behavior.

## Local state

History is stored at `$XDG_STATE_HOME/opsh/history`, or `~/.local/state/opsh/history` when XDG state is not configured. Set `OPSH_HISTORY` to use a different path. Colors automatically stay out of redirected output and can be disabled with `NO_COLOR=1`.

No daemon, account, telemetry or cloud service is involved.

## Development

```bash
cargo fmt --check
cargo test
cargo build --release
```

## License

[Apache-2.0](LICENSE)
