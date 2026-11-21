#!/usr/bin/env python3
"""Copy design/partials/*.html into every page between its partial markers.

Pages stay plain static HTML that open from disk (file://), while the shared
shell lives in one place. The Rust templates replace each marker pair with an
include of the same partial.

    <!-- @partial topbar -->  ...  <!-- @endpartial -->

Run from anywhere: python3 design/tools/sync_partials.py
Add --check to only report pages that are out of date (exit code 1).
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PARTIALS = ROOT / "partials"
MARKER = re.compile(r"([ \t]*)<!-- @partial (\w[\w-]*) -->.*?<!-- @endpartial -->", re.S)


def fill(match):
    indent, name = match.group(1), match.group(2)
    body = (PARTIALS / f"{name}.html").read_text().rstrip("\n")
    body = "\n".join(indent + line if line else line for line in body.splitlines())
    return f"{indent}<!-- @partial {name} -->\n{body}\n{indent}<!-- @endpartial -->"


def main():
    check = "--check" in sys.argv
    stale = []
    for page in sorted(ROOT.glob("*.html")):
        text = page.read_text()
        new = MARKER.sub(fill, text)
        if new != text:
            stale.append(page.name)
            if not check:
                page.write_text(new)
                print(f"updated {page.name}")
    if check and stale:
        print("out of date: " + ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
