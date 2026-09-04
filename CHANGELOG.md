# Changelog

All notable changes to opsh are documented here.

## v0.1.11-stable · 04/09/2026

`doctor` command, family SDK path, and agent docs. This version was made for CLI with a stable release channel on 04/09/2026 (v0.1.11-stable).

- New `opsh doctor [--json]`: checks `~/.option/opsh` state dir, `history`/`rc` paths, and the POSIX shell (`$OPSH_SHELL` / `$SHELL` / `/bin/sh`).
- Depend on `optionSDK` via the local family path (`../optionSDK`) instead of crates.io only, matching `optionUtils`.
- Add `VERSIONING.md` (single CLI surface) and `AGENTS.md` (build/test/install checklist).

## v0.1.10-stable · 03/08/2026

Shared SDK 0.1.3 release alignment. This version was made for CLI with a stable release channel on 03/08/2026 (v0.1.10-stable).

- Adopt the canonical `optionSDK` 0.1.3 contract for shared paths and atomic persistence helpers.
- Keep the `opsh` binary and its existing local-first history/rc behavior unchanged.

## 0.1.9 — 2026-08-02

### Added

- Depend on published **optionSDK** 0.1.2 for shared `~/.option/opsh/` paths, marks, and `NO_COLOR` helpers.

### Changed

- History and rc resolution go through `option_sdk::App::OPSH` (with the same legacy XDG migrate via `migrate_file`).

## 0.1.8 — 2026-07-31

### Added

- Incremental history search with **Ctrl+R** (documented; powered by rustyline).
- History expansion: `!!` (last command) and `!N` (entry N).
- `history N`, `history clear`, and `history QUERY` for trim / clear / substring search.
- Prompt placeholders `{git}` (branch, `*` when dirty) and `{elapsed}` (quiet below 10ms).

### Changed

- Default prompt templates now include `{git}` and `{elapsed}`.

## 0.1.7 — 2026-07-29

### Changed

- Config and history now live under **`~/.option/opsh/`** (`rc`, `history`). Legacy XDG paths are migrated automatically. `OPSH_RC` / `OPSH_HISTORY` still override.

## 0.1.6 — 2026-07-29

### Added

- Customizable prompt via `OPSH_PROMPT` placeholders (`{mark}`, `{cwd}`, `{cwd:full}`, `{status}`, `{prompt}`, `{stack}`).
- `OPSH_PROMPT_STYLE=single|double` and directory-stack depth in the prompt (`{stack}`).
- Banner control with `OPSH_BANNER` / `opsh -q`.
- Palette knobs: `OPSH_COLOR_OK`, `OPSH_COLOR_ERR`, `OPSH_COLOR_PATH`, `OPSH_COLOR_MARK`, `OPSH_COLOR_ACCENT`.
- Dim history hints in the line editor.
- `config` built-in to inspect active UI / path settings.

## 0.1.5 — 2026-07-29

### Added

- Interactive startup file at `$OPSH_RC` or `~/.option/opsh/rc`.
- `alias` / `unalias` built-ins, with tab completion and `which` support.
- GitHub Actions workflow to publish the AUR package on `v*` tags.

## 0.1.4 — 2026-07-29

### Added

- In-process `&&`, `||` and `;` chains so `cd` persists across segments.
- Tab completion for executables on `PATH` (alongside built-ins).
- Quote-aware argument splitting for built-ins (`mkdir "my dir"`).
- Packaging metadata for crates.io and an AUR `PKGBUILD` under `packaging/aur/`.

### Changed

- Pipes, redirects, substitutions and background jobs still use `/bin/sh` (or `$OPSH_SHELL` / a non-fish `$SHELL`); chain operators no longer force a subshell.

## 0.1.3 — 2026-07-29

### Added

- `pushd`, `popd` and `dirs` for an in-session directory stack.
- `path` and `get` for compact environment inspection.
- `repeat N COMMAND` and `time COMMAND` for repeated and timed commands.
- Interactive line editing with history recall (↑/↓), Ctrl+C handling and tab completion for built-ins and paths (`rustyline`).
- GitHub Actions CI for format, tests and release builds.
- `OPSH_SHELL` to choose the POSIX shell used for external commands.

### Changed

- External and compound commands no longer run through fish: only `/bin/sh`, `$OPSH_SHELL`, or a non-fish `$SHELL`, so fish built-ins stay out of opsh.
- Built-in lines that contain shell operators (`&&`, `|`, `;`, redirects, …) are handed to that command shell instead of being truncated by the built-in matcher.
- `repeat` and `time` can wrap built-ins as well as external commands.
- History is written atomically (temp file + rename).
- `source` rejects nesting deeper than 32 levels.
- `which` only reports executables on Unix.
- `opsh --help` lists every built-in.

## 0.1.1 — 2026-07-28

### Added

- Color-aware prompt with blue path, green success marker and red failure status.
- Colored built-in headings, status indicators and action feedback.
- `status`, `which`, `mkdir`, `mkcd`, `touch`, `open`, `set`, `unset`, `source` and `about` built-ins.
- `cd -` to return to the previous working directory.
- `NO_COLOR` and `TERM=dumb` support for plain output.

### Changed

- Interactive errors now remain in the shell and set the next prompt to failure state.
- Help output documents every built-in in the compact opsh style.

## 0.1.0 — 2026-07-28

### Added

- Initial Rust shell with `cd`, `pwd`, `history`, `clear`, `help` and `exit`.
- Local persistent history and shell execution for external commands.
