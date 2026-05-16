#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID="dev.multiagent.multiagent"
MANIFEST="packaging/flatpak/${APP_ID}.yml"
LOCAL_EXCEPTIONS="packaging/flatpak/flathub-lint-user-exceptions.json"

cd "$ROOT"

appstreamcli validate packaging/shared/${APP_ID}.metainfo.xml
desktop-file-validate \
  packaging/shared/${APP_ID}.desktop \
  packaging/shared/${APP_ID}.OpenCode.desktop

if command -v flatpak-builder >/dev/null 2>&1; then
  flatpak-builder --show-manifest "$MANIFEST" >/dev/null
elif flatpak info --user org.flatpak.Builder >/dev/null 2>&1 || flatpak info org.flatpak.Builder >/dev/null 2>&1; then
  flatpak run org.flatpak.Builder --show-manifest "$MANIFEST" >/dev/null
else
  echo "flatpak-builder not found; skipped manifest parse check" >&2
fi

if command -v flatpak-builder-lint >/dev/null 2>&1; then
  flatpak-builder-lint \
    --exceptions \
    --user-exceptions "$LOCAL_EXCEPTIONS" \
    --exceptions-repo stable \
    manifest "$MANIFEST"
elif flatpak info --user org.flatpak.Builder >/dev/null 2>&1 || flatpak info org.flatpak.Builder >/dev/null 2>&1; then
  flatpak run --command=flatpak-builder-lint org.flatpak.Builder \
    --exceptions \
    --user-exceptions "$LOCAL_EXCEPTIONS" \
    --exceptions-repo stable \
    manifest "$MANIFEST"
else
  echo "flatpak-builder-lint not found; skipped Flathub manifest linter" >&2
fi
