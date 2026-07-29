# ◆ opsh

**opsh** — a small, local-first shell written in Rust.

It keeps the interface deliberately quiet: a color-aware prompt, local history with ↑/↓ recall, tab completion and a compact set of built-ins. Everything else is passed to your configured system shell, so familiar pipes, redirects, environment variables and scripts keep working.

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
| `pushd DIR` / `popd` / `dirs` | Navigate through a local directory stack |
| `history` | List commands saved locally |
| `status` | Show the previous command status |
| `which CMD` | Locate a built-in or executable |
| `path` / `get NAME` | Inspect `PATH` or one environment variable |
| `mkdir DIR...` | Create directories |
| `mkcd DIR` | Create and enter a directory |
| `touch FILE...` | Create files when absent |
| `open PATH` | Open with the desktop default application |
| `set NAME VALUE` / `unset NAME` | Change the shell environment |
| `alias` / `unalias` | List, define or remove aliases |
| `config` | Show active prompt / color / path knobs |
| `source FILE` / `. FILE` | Run local opsh commands |
| `repeat N COMMAND` / `time COMMAND` | Repeat or time an external command |
| `clear` | Clear the interactive screen |
| `about` | Show version and project information |
| `help` | Show built-ins |
| `exit [N]` | Leave with optional status code |

External commands run through `/bin/sh` by default (or `$OPSH_SHELL`, or a non-fish `$SHELL`). Fish is never used as the command shell. `&&`, `||` and `;` run inside opsh so `cd` persists; pipes, redirects and `$(...)` still go to that POSIX shell.

### Install from crates.io

```bash
cargo install opsh
```

Arch: install the AUR package `opsh` (CI publishes on each `v*` tag). Setup notes are in `packaging/aur/README.md`.

## Local state

History is stored at `~/.option/opsh/history`. Set `OPSH_HISTORY` to use a different path. Legacy XDG state files are migrated automatically.

Interactive sessions load `$OPSH_RC` when set, otherwise `~/.option/opsh/rc`. A missing rc file is ignored. Example:

```text
set EDITOR nvim
alias g=git
alias ll=ls -la
set OPSH_BANNER 0
set OPSH_PROMPT_STYLE single
set OPSH_COLOR_PATH 38;5;81
```

### Configuration

| Variable | Purpose |
|---|---|
| `OPSH_RC` | Startup file path |
| `OPSH_HISTORY` | History file path |
| `OPSH_SHELL` | POSIX shell for pipes / redirects (never fish) |
| `OPSH_BANNER` | `0` / `false` / `off` hides the startup banner (`opsh -q` also hides it) |
| `OPSH_PROMPT_STYLE` | `double` (default) or `single` |
| `OPSH_PROMPT` | Custom template; placeholders: `{mark}` `{cwd}` `{cwd:full}` `{status}` `{prompt}` `{stack}` |
| `OPSH_COLOR_OK` | Success color (default `38;5;114`) |
| `OPSH_COLOR_ERR` | Failure color (default `38;5;210`) |
| `OPSH_COLOR_PATH` | Path color (default `38;5;75`) |
| `OPSH_COLOR_MARK` | `›` color (default `38;5;221`) |
| `OPSH_COLOR_ACCENT` | Accent / built-in name color (default `38;5;183`) |

Color values accept `38;5;N`, plain ANSI codes, or `off`. Run `config` inside opsh to inspect the active values. Colors stay out of redirected output and can be disabled with `NO_COLOR=1`.

No daemon, account, telemetry or cloud service is involved.

## Development

```bash
cargo fmt --check
cargo test
cargo build --release
```

## License

[Apache-2.0](LICENSE)
