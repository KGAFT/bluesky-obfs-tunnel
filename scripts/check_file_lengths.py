#!/usr/bin/env python3
"""Require maintained source files to stay within 300 total lines."""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAX_LINES = 300
SOURCE_SUFFIXES = {".rs", ".py", ".sh"}
SOURCE_DIRS = ("src", "tests", "examples", "benches", "scripts")


def main():
    files = [path for path in ROOT.iterdir() if path.is_file()]
    for directory in SOURCE_DIRS:
        files.extend(path for path in (ROOT / directory).rglob("*") if path.is_file())
    counts = []
    for path in sorted(files):
        if path.suffix not in SOURCE_SUFFIXES:
            continue
        with path.open(encoding="utf-8") as source:
            counts.append((path.relative_to(ROOT), sum(1 for _ in source)))

    oversized = [(path, count) for path, count in counts if count > MAX_LINES]
    for path, count in oversized:
        print(f"{path}: {count} lines (limit: {MAX_LINES})", file=sys.stderr)
    if oversized:
        return 1

    largest = max((count for _, count in counts), default=0)
    print(f"Checked {len(counts)} source files; largest: {largest}/{MAX_LINES} lines.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
