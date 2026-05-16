#!/usr/bin/env python3
"""Add direct HTTPS screenshot URLs to MultiAGENT AppStream metadata."""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import urllib.error
import urllib.request
import xml.dom.minidom
import xml.etree.ElementTree as ET


ROOT = pathlib.Path(__file__).resolve().parents[1]
METAINFO = ROOT / "packaging" / "shared" / "dev.multiagent.multiagent.metainfo.xml"
DEFAULT_CAPTION = "Coordinate coding agents, sessions, and diffs in MultiAGENT"


def parse_image_size(value: str | None) -> tuple[str, str] | None:
    if value is None:
        return None
    match = re.fullmatch(r"([1-9][0-9]*)x([1-9][0-9]*)", value.strip())
    if match is None:
        raise argparse.ArgumentTypeError("size must look like WIDTHxHEIGHT, for example 1200x800")
    return match.group(1), match.group(2)


def validate_url(url: str, check_remote: bool) -> None:
    if not url.startswith("https://"):
        raise ValueError(f"screenshot URL must start with https://: {url}")
    if not re.search(r"\.(png|jpg|jpeg|webp)(\?.*)?$", url, re.IGNORECASE):
        raise ValueError(f"screenshot URL must point directly to an image file: {url}")
    if not check_remote:
        return
    request = urllib.request.Request(url, method="HEAD")
    try:
        with urllib.request.urlopen(request, timeout=12) as response:
            content_type = response.headers.get("content-type", "")
    except urllib.error.HTTPError as err:
        if err.code not in {405, 403}:
            raise ValueError(f"screenshot URL is not reachable: {url} ({err})") from err
        request = urllib.request.Request(url, method="GET")
        with urllib.request.urlopen(request, timeout=12) as response:
            content_type = response.headers.get("content-type", "")
    if "image/" not in content_type.lower():
        raise ValueError(f"screenshot URL did not return an image content type: {url} ({content_type})")


def indent_xml(element: ET.Element, level: int = 0) -> None:
    pad = "\n" + level * "  "
    child_pad = "\n" + (level + 1) * "  "
    if len(element):
        if not element.text or not element.text.strip():
            element.text = child_pad
        for child in element:
            indent_xml(child, level + 1)
        if not child.tail or not child.tail.strip():
            child.tail = pad
    if level and (not element.tail or not element.tail.strip()):
        element.tail = pad


def write_xml(tree: ET.ElementTree) -> None:
    indent_xml(tree.getroot())
    raw = ET.tostring(tree.getroot(), encoding="unicode")
    pretty = xml.dom.minidom.parseString(raw).toprettyxml(indent="  ")
    lines = [line for line in pretty.splitlines() if line.strip()]
    if lines and lines[0].startswith("<?xml"):
        lines[0] = '<?xml version="1.0" encoding="UTF-8"?>'
    METAINFO.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("urls", nargs="+", help="direct HTTPS screenshot image URL")
    parser.add_argument(
        "--caption",
        default=DEFAULT_CAPTION,
        help=f"caption to apply to each screenshot; default: {DEFAULT_CAPTION!r}",
    )
    parser.add_argument(
        "--size",
        type=parse_image_size,
        default=None,
        help="optional source image size as WIDTHxHEIGHT",
    )
    parser.add_argument(
        "--skip-remote-check",
        action="store_true",
        help="skip HEAD/GET checks for URLs that are not public yet",
    )
    args = parser.parse_args()

    try:
        for url in args.urls:
            validate_url(url, check_remote=not args.skip_remote_check)
    except ValueError as err:
        print(err, file=sys.stderr)
        return 1

    tree = ET.parse(METAINFO)
    root = tree.getroot()
    existing = root.find("screenshots")
    if existing is not None:
        root.remove(existing)

    screenshots = ET.Element("screenshots")
    for index, url in enumerate(args.urls):
        screenshot = ET.SubElement(
            screenshots,
            "screenshot",
            {"type": "default"} if index == 0 else {},
        )
        image_attrs = {"type": "source"}
        if args.size is not None:
            image_attrs["width"], image_attrs["height"] = args.size
        image = ET.SubElement(screenshot, "image", image_attrs)
        image.text = url
        caption = ET.SubElement(screenshot, "caption")
        caption.text = args.caption

    releases = root.find("releases")
    insert_at = list(root).index(releases) if releases is not None else len(list(root))
    root.insert(insert_at, screenshots)
    write_xml(tree)
    print(f"wrote {len(args.urls)} screenshot URL(s) to {METAINFO.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
