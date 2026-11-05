#!/usr/bin/env python3
"""Checks that every relative link and image in the Markdown docs points at
a file or directory that exists, and that every `protonaut <command>`
mentioned in a shell block is a real subcommand. Standard library only."""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
COMMANDS = {"init", "inspect", "test", "proton", "report", "diagnose", "setup", "schema", "--version", "--help", "help"}
LINK = re.compile(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")


def docs():
    for p in [*ROOT.glob("*.md"), *ROOT.glob("docs/**/*.md"), *ROOT.glob("fixtures/*.md"), *ROOT.glob(".github/**/*.md")]:
        if p.is_file():
            yield p


def main() -> int:
    errors = []
    for doc in docs():
        text = doc.read_text()
        for target in LINK.findall(text):
            if re.match(r"^[a-z]+:", target) or target.startswith("#"):
                continue
            path = target.split("#", 1)[0]
            if path and not (doc.parent / path).exists():
                errors.append(f"{doc.relative_to(ROOT)}: broken link {target}")
        for block in re.findall(r"```(?:bash|sh|console)\n(.*?)```", text, re.S):
            for line in block.splitlines():
                m = re.match(r"\s*(?:sudo\s+)?protonaut\s+(\S+)", line)
                if m and m.group(1) not in COMMANDS:
                    errors.append(f"{doc.relative_to(ROOT)}: unknown command `protonaut {m.group(1)}`")
    if errors:
        print("\n".join(errors))
        return 1
    print("documentation links and commands ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
