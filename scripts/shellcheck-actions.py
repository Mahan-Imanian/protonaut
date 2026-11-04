#!/usr/bin/env python3
"""Runs shellcheck on every `run: |` block of the composite actions, which
actionlint does not inspect. Expressions (${{ ... }}) are replaced with a
placeholder first. Standard library only; needs shellcheck on PATH."""
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
ACTIONS = ["action.yml", "comment/action.yml"]


def blocks(path: pathlib.Path):
    lines = path.read_text().split("\n")
    i = 0
    while i < len(lines):
        m = re.match(r"^(\s*)run: \|\s*$", lines[i])
        if not m:
            i += 1
            continue
        indent = len(m.group(1)) + 2
        start = i + 1
        body = []
        i += 1
        while i < len(lines) and (not lines[i].strip() or len(lines[i]) - len(lines[i].lstrip()) >= indent):
            body.append(lines[i][indent:])
            i += 1
        yield start, re.sub(r"\$\{\{[^}]*\}\}", "X", "\n".join(body))


def main() -> int:
    failed = False
    with tempfile.TemporaryDirectory() as tmp:
        for rel in ACTIONS:
            for line, script in blocks(ROOT / rel):
                f = pathlib.Path(tmp) / f"{rel.replace('/', '_')}_{line}.sh"
                f.write_text("#!/bin/bash\n" + script + "\n")
                r = subprocess.run(["shellcheck", "-S", "warning", str(f)], capture_output=True, text=True)
                if r.returncode != 0:
                    failed = True
                    print(f"{rel}: run block starting at line {line}\n{r.stdout}")
    if not failed:
        print("shellcheck: composite action scripts ok")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
