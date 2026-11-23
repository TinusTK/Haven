#!/bin/bash
# Checks the App Store build holds no add-on code (the Google Play build
# has it, for the Stremio sign-in, and is not checked):
#
#   app/store-check.sh <program> [target]
#
# Fails if haven-addons is among the store build's dependencies, or if the
# program holds the word "add-on" anywhere a person could read it.
set -euo pipefail

program="$1"
target="${2:-}"
cd "$(dirname "$0")"

target_flag=()
if [ -n "$target" ]; then target_flag=(--target "$target"); fi
if cargo tree -p haven-app --no-default-features --features store -e normal \
    ${target_flag[@]+"${target_flag[@]}"} | grep -q "haven-addons"; then
  echo "store check: haven-addons is in the store build"; exit 1
fi

found="$(strings -a "$program" | grep -i "add-on" | head -5 || true)"
if [ -n "$found" ]; then
  echo "store check: the store build mentions add-ons:"; echo "$found"; exit 1
fi
echo "store check: no add-on code or wording in $program"
