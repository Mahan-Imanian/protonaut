#!/usr/bin/env python3
"""Runs every fixture through `steamos-preflight test` and compares the
reports with tests/e2e/expectations.toml. Standard library only.

    scripts/e2e.py --bin target/release/steamos-preflight --runtime ge-proton11-12

Exit status 0 when every expectation holds, 1 otherwise.
"""
import argparse
import json
import pathlib
import shutil
import subprocess
import sys
import time
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
FRAME_CHECKS = {"SP-DISP-002", "SP-DISP-003"}


def find_leaks(results: pathlib.Path, paths: list[str]) -> list[str]:
    """Artifacts are uploaded publicly, so no host path may appear in them,
    in Linux form or in the Z:\\ forms Wine prints."""
    needles = set()
    for p in paths:
        if len(p) > 1:
            needles |= {p, "Z:" + p.replace("/", "\\"), "Z:" + p.replace("/", "\\\\")}
    leaks = []
    for f in results.rglob("*"):
        if f.is_file() and f.suffix in {".json", ".md", ".html", ".xml", ".log", ".txt"}:
            text = f.read_text(errors="replace")
            leaks += [f"{f.relative_to(results)}" for n in needles if n in text]
    return sorted(set(leaks))


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--bin", required=True)
    p.add_argument("--runtime", action="append", required=True)
    p.add_argument("--work", default="e2e-work")
    p.add_argument("--observe-secs", default="10")
    p.add_argument("--only", action="append", help="run only these fixtures")
    a = p.parse_args()

    expectations = tomllib.loads((ROOT / "tests/e2e/expectations.toml").read_text())["fixture"]
    work = pathlib.Path(a.work).resolve()
    failures = []
    summary = []
    for name, exp in expectations.items():
        if a.only and name not in a.only:
            continue
        build = work / name / "build"
        results = work / name / "results"
        shutil.rmtree(work / name, ignore_errors=True)
        build.mkdir(parents=True)
        shutil.copy(ROOT / "fixtures/bin" / f"{name}.exe", build / "Game.exe")
        cmd = [a.bin, "test", str(build), "--exe", "Game.exe", "--observe-secs", a.observe_secs, "--results", str(results)]
        for rt in a.runtime:
            cmd += ["--runtime", rt]
        started = time.monotonic()
        proc = subprocess.run(cmd, capture_output=True, text=True)
        elapsed = time.monotonic() - started
        report_path = results / "report.json"
        if not report_path.is_file():
            failures.append(f"{name}: no report (exit {proc.returncode})\n{proc.stderr[-2000:]}")
            continue
        report = json.loads(report_path.read_text())
        want_code = 0 if exp["verdict"] == "pass" else 1
        if proc.returncode != want_code:
            failures.append(f"{name}: exit code {proc.returncode}, expected {want_code}")
        if report["summary"]["verdict"] != exp["verdict"]:
            failures.append(f"{name}: verdict {report['summary']['verdict']}, expected {exp['verdict']}")
        capture = {r["runtime"]: r.get("capture_verified") for r in report["runs"]}
        for check_id, want in exp["checks"].items():
            rows = [c for c in report["checks"] if c["id"] == check_id]
            if not rows:
                failures.append(f"{name}: {check_id} missing from report")
                continue
            for c in rows:
                expected = want
                if check_id in FRAME_CHECKS and c.get("runtime") and capture.get(c["runtime"]) is not True:
                    expected = "NOT_TESTED"
                if c["status"] != expected:
                    failures.append(f"{name}: {check_id} [{c.get('runtime', 'static')}] is {c['status']}, expected {expected}: {c['summary']}")
        leaks = find_leaks(results, [str(work), str(pathlib.Path.home())])
        failures.extend(f"{name}: host path leaked into {leak}" for leak in leaks)
        runs = ", ".join(f"{r['runtime']}={r['outcome']}/capture:{r.get('capture_verified')}" for r in report["runs"])
        summary.append(f"{name:<18} exit={proc.returncode} verdict={report['summary']['verdict']:<4} {elapsed:5.1f}s  {runs}")

    print("\n".join(summary))
    if failures:
        print("\nFAILED EXPECTATIONS:")
        print("\n".join(failures))
        return 1
    print(f"\nall {len(summary)} fixtures matched their expectations")
    return 0


if __name__ == "__main__":
    sys.exit(main())
