#!/usr/bin/env bash
# Measure the committed per-call resource budgets for every contract and print
# a reviewable table. The tests fail when CPU instructions, memory bytes,
# ledger write entries or write bytes grow beyond their snapshot by more than
# 5%, so this script exits non-zero on a regression.
set -euo pipefail

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

set +e
cargo test --all call_budgets -- --nocapture 2>&1 | tee "$log"
status=${PIPESTATUS[0]}
set -e

if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    echo "## Contract resource budgets"
    echo
    echo "Per-call CPU, memory and storage footprint. CI fails when any metric"
    echo "exceeds the committed snapshot by more than 5%."
    echo
    grep -E '^(###|\|)' "$log" || true
  } >> "$GITHUB_STEP_SUMMARY"
fi

exit "$status"
