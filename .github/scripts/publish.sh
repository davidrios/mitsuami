#!/usr/bin/env bash
# Publishes every workspace crate whose version isn't on crates.io yet, so
# a release can be resumed. crates.io rate-limits the creation of new
# crates, so a failed upload is retried after a pause.
set -uo pipefail

attempts=8
pause=660

for attempt in $(seq "$attempts"); do
  excludes=()
  pending=()
  while read -r name version; do
    if curl -sf -o /dev/null -A "mitsuami-release ($GITHUB_REPOSITORY)" \
      "https://crates.io/api/v1/crates/$name/$version"; then
      excludes+=(--exclude "$name")
    else
      pending+=("$name")
    fi
  done < <(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | "\(.name) \(.version)"' | tr -d '\r')

  if [ ${#pending[@]} -eq 0 ]; then
    echo "Every crate is published."
    exit 0
  fi
  echo "Attempt $attempt/$attempts: publishing ${pending[*]}"
  if cargo publish --workspace "${excludes[@]}"; then
    exit 0
  fi
  if [ "$attempt" -lt "$attempts" ]; then
    echo "::warning::Publishing stopped (attempt $attempt); retrying in $((pause / 60)) minutes"
    sleep "$pause"
  fi
done

echo "::error::Some crates are still unpublished after $attempts attempts"
exit 1
