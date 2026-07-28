# Changelog

All notable changes to opsh are documented here.

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
- Local persistent history and `$SHELL` execution for external commands.
