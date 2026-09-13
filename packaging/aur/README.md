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
2. Bumps `packaging/aur/PKGBUILD` + `.SRCINFO` in the runner's checkout
3. Pushes the package to the AUR (`AUR_SSH_PRIVATE_KEY`)
4. Opens a PR against `main` with the packaging bump
   ([`peter-evans/create-pull-request`](https://github.com/peter-evans/create-pull-request)).
   The workflow never pushes to `main` directly; if `main` already has the
   bump, no PR is opened.

The AUR publish uses the bumped files from the runner, so it does not depend
on the PR being merged. Merge the PR (or commit the bump before tagging, see
below) so `main` stays in sync with what is on the AUR.

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
# → Actions publishes crates.io + AUR, then opens a PR with the PKGBUILD bump
```

The bump needs the tag tarball's sha256, so it cannot be committed before the
tag exists — merging the automated PR is the normal way to land it. If the
workflow's PR step fails, run `./packaging/aur/bump.sh v0.1.6` locally and
commit `packaging/aur/PKGBUILD` + `.SRCINFO` by hand.

## Local publish (fallback)

```bash
./packaging/aur/publish.sh           # push current packaging/
./packaging/aur/publish.sh 0.1.6     # bump + push
```

Uses `~/aur/opsh` and `~/.ssh/aur_synara` (override with `AUR_SSH_KEY=` / `AUR_DIR=`).
