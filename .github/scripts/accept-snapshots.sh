#!/usr/bin/env bash
# Accepts the snapshots and visual baselines a CI run wrote for review:
# downloads every OS's `snapshots-*` artifact and moves each `<file>.new` and
# `<name>.new.png` in it over its baseline. Look at them before committing.
# Usage: accept-snapshots.sh <run id>
set -euo pipefail

run=${1:?usage: accept-snapshots.sh <run id>}
root=$(git rev-parse --show-toplevel)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

gh run download "$run" --pattern 'snapshots-*' --dir "$tmp"
for archive in "$tmp"/*/snapshots.tar; do
  [ -e "$archive" ] || { echo "run $run has no snapshots to review"; exit 1; }
  tar -xf "$archive" -C "$root"
  tar -tf "$archive" | while read -r file; do
    case "$file" in
      *.diff.png) rm -f "$root/$file" ;;
      *.new.png) mv "$root/$file" "$root/${file%.new.png}.png" && echo "accepted ${file%.new.png}.png" ;;
      *.new) mv "$root/$file" "$root/${file%.new}" && echo "accepted ${file%.new}" ;;
    esac
  done
done
