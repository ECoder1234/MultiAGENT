# MultiAGENT Flatpak

This directory contains the local Flatpak/Flathub packaging for
`dev.multiagent.multiagent`.

## Build

```bash
scripts/generate_flatpak_cargo_sources.py
flatpak-builder --user --install-deps-from=flathub --force-clean build-dir packaging/flatpak/dev.multiagent.multiagent.yml
```

Use `--user` for local testing so the build installs SDKs, runtimes, and test
apps into the user Flatpak installation rather than the system installation.
The checked-in manifest uses the public `multiagent-v0.1.3-flathub2` GitHub
source tag so the same file can be used for Flathub review.

## Flathub Lint

The manifest intentionally requests two permissions that require Flathub
exceptions:

- `finish-args-home-filesystem-access`
- `finish-args-flatpak-spawn-access`

Those keep the Flatpak useful as a coding workspace: project files remain in the
user's normal home checkouts, and account-bound coding-agent CLIs run from the
host environment instead of being bundled into the app. The requested exception
text for Flathub review is in `flathub-exceptions.json`; the compact local
linter allow-list is in `flathub-lint-user-exceptions.json`.

Run the linter with the local exception file:

```bash
flatpak run --command=flatpak-builder-lint org.flatpak.Builder \
  --exceptions \
  --user-exceptions packaging/flatpak/flathub-lint-user-exceptions.json \
  --exceptions-repo stable \
  manifest packaging/flatpak/dev.multiagent.multiagent.yml
```

The built repository linter also expects public AppStream screenshots mirrored
into the OSTree repo. The current AppStream metadata points at the public
GitHub-hosted screenshot asset. Rebuild screenshot metadata after replacing or
adding screenshots with:

```bash
scripts/add_flathub_screenshots.py \
  --size 1200x800 \
  https://raw.githubusercontent.com/ECoder1234/multiagent-flathub-assets/766e775808480a3c9f01238b7ff854c398ac329b/screenshots/dev.multiagent.multiagent-main.png
```

```bash
flatpak run org.flatpak.Builder \
  --user \
  --repo=repo \
  --compose-url-policy=full \
  --mirror-screenshots-url=https://dl.flathub.org/media \
  --force-clean build-dir packaging/flatpak/dev.multiagent.multiagent.yml
```

Run the Flathub submission preflight with:

```bash
scripts/validate_flathub_submission.sh
```
