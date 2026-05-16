# MultiAGENT

> MultiAGENT is a Linux-first GTK4/libadwaita desktop workspace for running coding agents, reviewing diffs, managing project sessions, and working with local runtime profiles from one glassy interface.

## Based On / Credits

MultiAGENT is built on top of enzim-coder by enz1m, which serves as the source first iteration and main backbone of this project.

Source backbone: <https://github.com/enz1m/enzim-coder>

## Highlights

- Glassy dark interface with compact project, session, and active-agent panels.
- Multi-agent status panel with idle, running, and waiting indicators.
- Agent output tabs for primary, review, and background streams.
- Review tab for changed files, staged commits, pushes, and syntax-highlighted diffs.
- Ctrl+K command palette for fast navigation and settings access.
- Composer model selector, collaboration controls, file mentions, image attachments, and voice input.
- Session token and estimated cost counter in the app chrome.
- Settings panel with shortcuts, dark/light theme toggle, runtime configuration, and attribution.
- Local SQLite storage, local runtime profiles, Git worktrees, restore previews, and browser integration.

## Runtime Support

MultiAGENT can work with Codex and OpenCode runtimes. Settings also detects GitHub Copilot CLI so authentication can be prepared while full in-app adapter support evolves.

Install one or more supported CLIs:

```bash
npm i -g @openai/codex
curl -fsSL https://opencode.ai/install | bash
sudo snap install copilot --classic
```

## Remote Mode

Remote mode works over Discord DMs and Telegram bot chats. In the app, open Settings -> Remote, choose the provider, enter the bot token, then authenticate by sending the shown 6-digit code. The bottom-bar remote button toggles remote mode without blocking the local GUI.

## Chrome Bridge

The official Codex Chrome extension connects through the Chrome native messaging host named `com.openai.codexextension`. On Linux, open Settings -> Browser and use Enable Linux bridge to enable the bundled Chrome bridge configuration, install the native messaging manifest, and create MultiAGENT's native-host wrapper.

## Platform

- Linux desktop app
- Rust `1.92`
- GTK4 + libadwaita
- GTK `4.14+`
- No macOS-only APIs, Cocoa dependencies, or macOS window-control styling

## Development

System packages required:

- `gtk4`
- `libadwaita`
- `gtksourceview-5`
- `glib2`
- `pkg-config`
- C build toolchain

Check the workspace:

```bash
cargo check --workspace
```

Run the GTK app:

```bash
cargo run -p multiagent-gtk --release
```

Run with isolated app data:

```bash
MULTIAGENT_PROFILE_HOME_DIR=/path/to/testdir cargo run -p multiagent-gtk --release
```

Build the release binary used by packaging:

```bash
cargo build -p multiagent-gtk --release --locked
```

## Flatpak And Flathub

The Flatpak manifest is Linux-first and does not install system packages. It
targets the GNOME 50 runtime, builds Rust dependencies from a generated offline
Cargo source manifest, installs only into `/app`, and keeps runtime filesystem
access to the user's home directory instead of the whole host.

Refresh the Cargo source manifest after dependency changes:

```bash
scripts/generate_flatpak_cargo_sources.py
```

Validate metadata without installing anything:

```bash
scripts/validate_flatpak_metadata.sh
```

Run the Flathub manifest linter with the local exception request file:

```bash
flatpak run --command=flatpak-builder-lint org.flatpak.Builder \
  --exceptions \
  --user-exceptions packaging/flatpak/flathub-lint-user-exceptions.json \
  --exceptions-repo stable \
  manifest packaging/flatpak/dev.multiagent.multiagent.yml
```

Build and test locally with user-scoped Flatpak state:

```bash
flatpak-builder --user --install-deps-from=flathub --force-clean build-dir packaging/flatpak/dev.multiagent.multiagent.yml
flatpak-builder --user --install --force-clean build-dir packaging/flatpak/dev.multiagent.multiagent.yml
flatpak run dev.multiagent.multiagent
```

The Flatpak app ID is `dev.multiagent.multiagent` and assumes the project can
verify/control `multiagent.dev` for Flathub review. If that domain is not under
project control, change the app ID before submission rather than shipping an
unverifiable ID.

Flathub will also require a case-by-case linter exception for home-project
filesystem access and `flatpak-spawn` host CLI bridging. The exception request
text is kept in `packaging/flatpak/flathub-exceptions.json`.

Before opening the Flathub submission, publish current Linux window screenshots
on the project site and add them to the AppStream metadata so Flathub can mirror
them into the repository.

```bash
scripts/add_flathub_screenshots.py \
  --size 1200x800 \
  https://raw.githubusercontent.com/ECoder1234/multiagent-flathub-assets/766e775808480a3c9f01238b7ff854c398ac329b/screenshots/dev.multiagent.multiagent-main.png
```

Run the final submission preflight after the screenshot URLs are in place:

```bash
scripts/validate_flathub_submission.sh
```

Build the AppImage:

```bash
scripts/build_appimage.sh
```

## Project Layout

- `apps/gtk/` GTK app crate
- `crates/multiagent_core/` shared core logic
- `src/` shared app/service layer used by platform apps
- `packaging/` release packaging
- `icons/` bundled icon subset used by the resource file

## Status

MultiAGENT is in active iteration.
