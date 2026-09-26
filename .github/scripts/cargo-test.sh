#!/usr/bin/env bash
# Runs the workspace tests (or the packages and features given after the
# label) and, if they fail, posts the failure report as an annotation.
# Usage: cargo-test.sh <label> [cargo test selection…]
set -uo pipefail

log="$RUNNER_TEMP/cargo-test-$1.log"
selection=("${@:2}")
[ ${#selection[@]} -gt 0 ] || selection=(--workspace)
cargo test "${selection[@]}" --no-fail-fast 2>&1 | tee "$log"
status=${PIPESTATUS[0]}

if [ "$status" -ne 0 ]; then
  # Every "failures:" report (the runner prints one per failing test
  # binary), or the tail of the log if the tests crashed before reporting.
  report=$(sed -n '/^failures:$/,/^test result:/p' "$log")
  [ -n "$report" ] || report=$(tail -n 60 "$log")
  report=${report//'%'/'%25'}
  report=${report//$'\r'/'%0D'}
  report=${report//$'\n'/'%0A'}
  echo "::error title=cargo test ($1) failed::$report"
fi
exit "$status"
