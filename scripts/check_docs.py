#!/usr/bin/env python3
"""Check local links/assets in project Markdown without fetching remote URLs."""
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit


def check_files(root, files):
    root = root.resolve()
    errors = []
    count = 0
    for file in files:
        file = file.resolve()
        text = file.read_text(encoding="utf-8")
        # Examples inside fences are not links in the rendered document.
        text = re.sub(r"(?ms)^\s*(```|~~~).*?^\s*\1\s*$", "", text)
        links = re.findall(r"!?\[[^\]\n]*\]\(([^\n)]+)\)", text)
        links += re.findall(r'<(?:img|a)\b[^>]*\b(?:src|href)=[\"\']([^\"\']+)', text, re.I)
        for raw in links:
            target = raw.strip()
            target = target[1:target.index(">")] if target.startswith("<") and ">" in target else target.split(' "', 1)[0]
            url = urlsplit(target)
            if url.scheme or url.netloc or not url.path:
                continue
            count += 1
            resolved = (root / unquote(url.path.lstrip("/")) if url.path.startswith("/") else file.parent / unquote(url.path)).resolve()
            if root != resolved and root not in resolved.parents:
                errors.append(f"{file.relative_to(root)}: link leaves repository: {target}")
            elif not resolved.exists():
                errors.append(f"{file.relative_to(root)}: missing local link/asset: {target}")
    return count, errors


def main():
    root = Path(__file__).resolve().parent.parent
    names = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.md"], cwd=root).decode().split("\0")
    files = [root / name for name in sorted(set(names)) if name and not name.startswith("scripts/notices-upstream/")]
    count, errors = check_files(root, files)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"PASS: {len(files)} Markdown files, {count} local links/assets; no remote URLs fetched")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
