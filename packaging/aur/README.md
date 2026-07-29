# AUR package (`opsh`)

PKGBUILD lives here and is published by `.github/workflows/aur-publish.yml` on `v*` tags.

## One-time setup

1. Create an [AUR account](https://aur.archlinux.org/register) and add an SSH public key under **My Account → SSH Public Key**.
2. Create the empty AUR package repo once (from a machine with the AUR SSH key):

```bash
git clone ssh://aur@aur.archlinux.org/opsh.git
cd opsh
# leave empty; the workflow will push PKGBUILD + .SRCINFO
```

3. In the GitHub repo **Settings → Secrets and variables → Actions**, add:

| Secret | Value |
|---|---|
| `AUR_USERNAME` | Your AUR username (git commit author name) |
| `AUR_EMAIL` | Email on your AUR account |
| `AUR_SSH_PRIVATE_KEY` | Private key matching the public key on AUR |

4. Push a version tag (`v0.1.5`, …). The workflow downloads the GitHub archive, fills `sha256sums`, and deploys to the AUR.

You can also run **Actions → aur-publish → Run workflow** and pass a tag manually.
