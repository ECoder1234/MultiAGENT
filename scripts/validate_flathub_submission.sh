#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID="dev.multiagent.multiagent"
METAINFO="packaging/shared/${APP_ID}.metainfo.xml"
MANIFEST="packaging/flatpak/${APP_ID}.yml"
LOCAL_EXCEPTIONS="packaging/flatpak/flathub-lint-user-exceptions.json"

cd "$ROOT"

scripts/validate_flatpak_metadata.sh

python3 - <<'PY'
from __future__ import annotations

import sys
import xml.etree.ElementTree as ET

path = "packaging/shared/dev.multiagent.multiagent.metainfo.xml"
tree = ET.parse(path)
root = tree.getroot()
images = [
    (image.text or "").strip()
    for screenshot in root.findall("screenshots/screenshot")
    for image in screenshot.findall("image")
]

if not images:
    print(
        "Flathub submission is missing AppStream screenshots. "
        "Add direct HTTPS screenshot URLs to packaging/shared/"
        "dev.multiagent.multiagent.metainfo.xml before submission.",
        file=sys.stderr,
    )
    sys.exit(1)

bad = [url for url in images if not url.startswith("https://")]
if bad:
    print("All AppStream screenshot images must be direct HTTPS URLs:", file=sys.stderr)
    for url in bad:
        print(f"  {url}", file=sys.stderr)
    sys.exit(1)
PY

if command -v flatpak-builder-lint >/dev/null 2>&1; then
  flatpak-builder-lint \
    --exceptions \
    --user-exceptions "$LOCAL_EXCEPTIONS" \
    --exceptions-repo stable \
    manifest "$MANIFEST"
else
  flatpak run --command=flatpak-builder-lint org.flatpak.Builder \
    --exceptions \
    --user-exceptions "$LOCAL_EXCEPTIONS" \
    --exceptions-repo stable \
    manifest "$MANIFEST"
fi

echo "Flathub submission preflight passed."
