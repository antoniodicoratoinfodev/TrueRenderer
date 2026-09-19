#!/usr/bin/env python3
"""Check local Markdown links and anchors in maintained project documentation."""
from pathlib import Path
import re
import subprocess
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]


def prose(path):
    text = path.read_text(encoding="utf-8")
    return re.sub(r"(?ms)^```[^\n]*\n.*?^```[^\n]*$", "", text)


def anchors(path):
    text = prose(path)
    result = set(re.findall(r'<a\s+(?:id|name)="([^"]+)"', text))
    counts = {}
    for heading in re.findall(r"(?m)^#{1,6}\s+(.+?)\s*#*\s*$", text):
        heading = re.sub(r"\[([^]]+)\]\([^)]+\)", r"\1", heading)
        slug = re.sub(r"[^\w\- ]", "", heading.lower()).replace(" ", "-")
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        result.add(slug if count == 0 else f"{slug}-{count}")
    return result


def main():
    names = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "*.md"],
        cwd=ROOT,
    ).decode().split("\0")
    failures = []
    checked = 0
    cached_anchors = {}
    for name in sorted(set(names)):
        path = ROOT / name
        if not name or not path.is_file() or any(part in name for part in (
            "originale-v1.2", "third_party/", "dependency-notices/", "dependency-inventory-windows-notices/"
        )):
            continue  # Immutable historical source and upstream notices are not rewritten.
        for link in re.findall(r"\[[^\]\n]*\]\(([^)\s]+)\)", prose(path)):
            if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", link):
                continue
            filename, _, fragment = unquote(link).partition("#")
            target = (path.parent / filename).resolve() if filename else path
            checked += 1
            if not target.exists():
                failures.append(f"{name}: missing {link}")
            elif fragment and target.suffix == ".md":
                if target not in cached_anchors:
                    cached_anchors[target] = anchors(target)
                if fragment not in cached_anchors[target]:
                    failures.append(f"{name}: missing anchor {link}")
    print(f"Checked {checked} local links; {len(failures)} failures")
    if failures:
        raise SystemExit("\n".join(failures))


if __name__ == "__main__":
    main()
