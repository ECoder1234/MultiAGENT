#!/usr/bin/env python3
"""Generate Flatpak Cargo source entries from Cargo.lock."""

from __future__ import annotations

import json
import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
CARGO_LOCK = ROOT / "Cargo.lock"
OUTPUT = ROOT / "packaging" / "flatpak" / "cargo-sources.json"
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"


def lock_value(block: str, key: str) -> str | None:
    match = re.search(rf'^{re.escape(key)} = "([^"]+)"$', block, re.MULTILINE)
    return match.group(1) if match else None


def main() -> int:
    text = CARGO_LOCK.read_text(encoding="utf-8")
    packages: list[tuple[str, str, str]] = []
    seen: set[tuple[str, str, str]] = set()

    for block in text.split("[[package]]")[1:]:
        name = lock_value(block, "name")
        version = lock_value(block, "version")
        source = lock_value(block, "source")
        checksum = lock_value(block, "checksum")

        if source is None:
            continue
        if source != CRATES_IO:
            print(f"unsupported Cargo source for {name} {version}: {source}", file=sys.stderr)
            return 1
        if not name or not version or not checksum:
            print(f"incomplete crates.io package entry in Cargo.lock:\n{block}", file=sys.stderr)
            return 1

        key = (name, version, checksum)
        if key in seen:
            continue
        seen.add(key)
        packages.append(key)

    sources: list[dict[str, object]] = []
    for name, version, checksum in sorted(packages):
        vendor_dest = f"cargo/vendor/{name}-{version}"
        sources.append(
            {
                "type": "archive",
                "archive-type": "tar-gzip",
                "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
                "sha256": checksum,
                "dest": vendor_dest,
                "strip-components": 1,
            }
        )
        sources.append(
            {
                "type": "inline",
                "contents": json.dumps({"package": checksum, "files": {}}),
                "dest": vendor_dest,
                "dest-filename": ".cargo-checksum.json",
            }
        )

    OUTPUT.write_text(json.dumps(sources, indent=2) + "\n", encoding="utf-8")
    print(
        f"wrote {len(sources)} Cargo source entries for {len(packages)} crates "
        f"to {OUTPUT.relative_to(ROOT)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
