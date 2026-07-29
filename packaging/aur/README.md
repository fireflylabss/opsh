# AUR package (`opsh`)

PKGBUILD lives here and is published by `.github/workflows/release.yml` on `v*` tags (together with crates.io).

## One-time setup

1. Create an [AUR account](https://aur.archlinux.org/register) and add an SSH public key under **My Account → SSH Public Key**.
2. Create the empty AUR package repo once (from a machine with the AUR SSH key):

```bash
ssh-keygen -t ed25519 -f ~/.ssh/aur -C "aur-opsh" -N ""
# paste ~/.ssh/aur.pub into https://aur.archlinux.org/account/ (SSH Public Key)

GIT_SSH_COMMAND='ssh -i ~/.ssh/aur' git clone ssh://aur@aur.archlinux.org/opsh.git
# empty repo is fine; CI pushes PKGBUILD + .SRCINFO
```

3. In the GitHub repo **Settings → Secrets and variables → Actions**, add:

| Secret | Value |
|---|---|
| `AUR_USERNAME` | Your AUR username |
| `AUR_EMAIL` | Email on your AUR account |
| `AUR_SSH_PRIVATE_KEY` | Contents of `~/.ssh/aur` (private key) |
| `CARGO_REGISTRY_TOKEN` | crates.io API token (already used for crates publish) |

4. Push a version tag (`v0.1.6`, …) or run **Actions → release → Run workflow**.

Until the three `AUR_*` secrets exist, the AUR job fails on purpose; crates.io can still publish.
