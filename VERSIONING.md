# Versioning

This project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html) with an explicit **release channel** suffix, and [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## Surface

opsh is a single surface (CLI `opsh`). Changelog headings use the channel suffix, e.g. `## v0.1.10-stable · DD/MM/YYYY`.

`Cargo.toml` / git tags keep the numeric version (`0.1.10`, `v0.1.10`) — the channel lives in the changelog (and packaging notes).

## Release channels (`x.y.z-<channel>`)

| Channel | Tag example | Meaning |
|---------|-------------|---------|
| **alpha** | `0.1.11-alpha` | Extremely early. Features incomplete; bugs are expected and common. |
| **beta** | `0.1.11-beta` | Feature set nearly complete, but still rough — bugs and hard edges remain. |
| **stable** | `0.1.10-stable` | Production-ready: finished for that version, few or no known bugs. |

Do **not** label something `stable` unless it is actually release-ready. Prefer **beta** while a large rewrite is settling; use **alpha** only for brand-new / half-built surfaces.

Alpha/beta cuts are normally changelog + local/dev artifacts — not GitHub Release / AUR — unless explicitly promoted to **stable**.
