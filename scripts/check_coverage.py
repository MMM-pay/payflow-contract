#!/usr/bin/env python3
"""Measure and enforce per-contract coverage floors (#257).

This is the local equivalent of the `coverage` job in `.github/workflows/ci.yml`.
It reads the explicit per-crate floors from `coverage.toml`, runs each contract's
tests under `cargo llvm-cov -p <crate>` with LLVM branch counters enabled, writes
one HTML report per crate under `target/llvm-cov/html/<crate>`, prints a summary
table, and exits non-zero if any crate falls below any of its floors.

Why RUSTC_BOOTSTRAP=1: `-Z coverage-options=branch` is still unstable in rustc,
but it only affects LLVM coverage instrumentation. Setting the variable lets the
pinned stable toolchain emit branch counters without installing a second
nightly toolchain. No unstable language feature is used by the contracts.
"""

from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONFIG = ROOT / "coverage.toml"
METRICS = ("lines", "branches", "regions", "functions")


def run(command: list[str], env: dict[str, str]) -> int:
    print("+", " ".join(command), flush=True)
    return subprocess.run(command, cwd=ROOT, env=env, check=False).returncode


def main() -> int:
    config = tomllib.loads(CONFIG.read_text(encoding="utf-8"))
    ignore = config["ignore_filename_regex"]
    crates: dict[str, dict[str, float]] = config["crates"]

    env = os.environ.copy()
    # Only unlocks LLVM's branch coverage flag; see module docstring.
    env["RUSTC_BOOTSTRAP"] = "1"

    json_dir = ROOT / "target" / "llvm-cov-json"
    html_root = ROOT / "target" / "llvm-cov" / "html"
    json_dir.mkdir(parents=True, exist_ok=True)
    html_root.mkdir(parents=True, exist_ok=True)

    failures: list[str] = []
    rows = [
        "| Contract crate | Lines | Branches | Regions | Functions |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]

    for crate, floors in crates.items():
        json_path = json_dir / f"{crate}.json"
        json_path.unlink(missing_ok=True)

        status = run(
            [
                "cargo", "llvm-cov", "-p", crate, "--lib", "--branch",
                "--ignore-filename-regex", ignore,
                "--json", "--summary-only", "--output-path", str(json_path),
            ],
            env,
        )
        if status != 0:
            failures.append(f"{crate}: coverage run failed (exit {status})")
            continue

        try:
            totals = json.loads(json_path.read_text(encoding="utf-8"))["data"][0]["totals"]
        except (KeyError, IndexError, json.JSONDecodeError) as error:
            failures.append(f"{crate}: could not read coverage summary: {error}")
            continue

        cells = [f"`{crate}`"]
        for metric in METRICS:
            actual = float(totals[metric]["percent"])
            floor = float(floors[metric])
            cells.append(f"{actual:.2f}% / {floor:.0f}%")
            if actual + 1e-9 < floor:
                failures.append(
                    f"{crate}: {metric} {actual:.2f}% is below the {floor:.0f}% floor"
                )
            if metric == "branches" and totals[metric]["count"] == 0:
                failures.append(f"{crate}: no branch counters found; check instrumentation")

        rows.append("| " + " | ".join(cells) + " |")

        html_dir = html_root / crate
        status = run(
            [
                "cargo", "llvm-cov", "report", "-p", crate,
                "--ignore-filename-regex", ignore,
                "--html", "--output-dir", str(html_dir),
            ],
            env,
        )
        if status != 0:
            failures.append(f"{crate}: HTML report failed (exit {status})")

    summary = "\n".join(rows) + "\n"
    print(summary, flush=True)
    print(
        "Coverage floors are defined in coverage.toml; "
        "HTML reports are in target/llvm-cov/html/<crate>.",
        flush=True,
    )

    github_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_summary:
        with pathlib.Path(github_summary).open("a", encoding="utf-8") as handle:
            handle.write("## Per-contract coverage\n\n" + summary)

    if failures:
        print("COVERAGE GATE FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
