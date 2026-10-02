# Self-hosted Flatpak repository

QRnew publishes its own Flatpak repository to GitHub Pages rather than going to
Flathub. Users get one-click installation and ordinary `flatpak update`; we keep
the release process.

## Why not Flathub

Flathub's generative-AI policy, in force since 29 May 2026, requires submitters
to disclose AI-generated code, documentation and packaging and its approximate
extent. Reviewers may then reject a submission at their discretion, including
without further review. AI tools may not open the submission pull request or
write its commit messages or replies.

It is a disclosure rule rather than the outright ban the press coverage
described, so the door is not shut — but for a young app with a largely
AI-written codebase the odds are poor, and this repository does the same job.

Two things would have to change to submit later:

- **The build cannot use the network.** `--share=network` in the manifest would
  become a generated `cargo-sources.json` listing every crate, regenerated
  whenever the Blitz pin moves.
- **The source would be a tagged archive** with a checksum, not a git branch.

## One-time setup

### 1. The signing key

```sh
just flatpak-keygen
```

It prints a fingerprint and a private key block. Add both under **Settings →
Secrets and variables → Actions**:

| Secret | Value |
| --- | --- |
| `FLATPAK_GPG_KEY_ID` | the fingerprint |
| `FLATPAK_GPG_PRIVATE_KEY` | the whole `-----BEGIN PGP PRIVATE KEY BLOCK-----` block |

Keep a copy somewhere that is not GitHub. Losing the key means existing installs
stop trusting their updates, and every user has to remove and re-add the remote.

### 2. GitHub Pages

**Settings → Pages → Build and deployment → Source: GitHub Actions.** Not
"Deploy from a branch" — the workflow uploads the repository as a Pages artifact.

After that, every push to `main` and every `v*` tag republishes the repository.

## Testing locally

```sh
just flatpak-build              # build and install from the last commit
flatpak run dev.lhdjung.QRnew
just flatpak-clean              # remove it again
```

The manifest uses a git source, so `flatpak-build` builds the last commit and
not the working tree. Commit first.

`just flatpak-bundle` writes `QRnew.flatpak`, the single-file bundle that the
release also carries.

## What the sandbox allows

Four permissions: `wayland`, `fallback-x11` with `ipc`, and `dri`. No filesystem
access — saving and opening both go through the file portal, which hands back
one file at a time. No network, matching a dependency tree built without
`blitz-net`.

## Notes

**The repository is rebuilt from scratch on each release.** Clients only need
the newest commit, so updates work, but old versions are not kept and no static
deltas are generated. Caching `repo/` between runs would change both.

**`runtime-version` lives in the manifest** and the workflow reads it from there,
so bumping it is a one-line change.
