# AUR packaging (`opsh`)

Same setup as optionMusic: one SSH key secret, username hardcoded in CI.

Published: https://aur.archlinux.org/packages/opsh

## Install

```bash
yay -S opsh
# or
paru -S opsh
```

## Automatic publish

Every **`v*` tag** (and manual **Actions → release**) runs [`.github/workflows/release.yml`](../../.github/workflows/release.yml):

1. Publishes crates.io (`CARGO_REGISTRY_TOKEN`)
2. Bumps `packaging/aur/PKGBUILD` + `.SRCINFO`
3. Pushes the package to the AUR (`AUR_SSH_PRIVATE_KEY`)

### One-time setup

Reuse the same AUR key as optionMusic:

```bash
gh secret set AUR_SSH_PRIVATE_KEY < ~/.ssh/aur_synara
```

Public key must already be on the AUR account (it is, if optionMusic publishes).

### Day-to-day

```bash
git tag -a v0.1.6 -m "opsh 0.1.6"
git push origin v0.1.6
# → Actions publishes crates.io + AUR
```

## Local publish (fallback)

```bash
./packaging/aur/publish.sh           # push current packaging/
./packaging/aur/publish.sh 0.1.6     # bump + push
```

Uses `~/aur/opsh` and `~/.ssh/aur_synara` (override with `AUR_SSH_KEY=` / `AUR_DIR=`).
