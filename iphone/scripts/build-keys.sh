#!/bin/bash
# Writes BuildKeys.swift: the app keys built into Haven (Trakt, Simkl,
# OMDb), taken from the environment, as the Rust build does with
# option_env!. A key that is not set is left empty and Haven says it is
# missing. There is never a built-in TMDB key: people add their own.
#
# On this Mac:  set -a; . ../Local/build-keys.env; set +a  before building.
# In CI:        the same HAVEN_* variables come from the repository secrets.
#
# The values are never printed.
set -euo pipefail

out="$1"
mkdir -p "$(dirname "$out")"

# Swift string literal: only letters, digits and - _ . are kept, so a key
# can never break out of the quotes.
literal() {
  printf '"%s"' "$(printf '%s' "${1:-}" | tr -cd 'A-Za-z0-9._-')"
}

tmp="$out.tmp"
{
  echo "// Made by scripts/build-keys.sh at build time. Not kept in git."
  echo "enum BuildKeys {"
  echo "    static let traktClientID = $(literal "${HAVEN_TRAKT_CLIENT_ID:-}")"
  echo "    static let traktClientSecret = $(literal "${HAVEN_TRAKT_CLIENT_SECRET:-}")"
  echo "    static let simklClientID = $(literal "${HAVEN_SIMKL_CLIENT_ID:-}")"
  echo "    static let omdbAPIKey = $(literal "${HAVEN_OMDB_API_KEY:-}")"
  echo "}"
} > "$tmp"

# Only touch the file when it changed, so Swift does not rebuild for nothing.
if [ -f "$out" ] && cmp -s "$tmp" "$out"; then
  rm -f "$tmp"
else
  mv "$tmp" "$out"
fi

set_keys=0
for v in HAVEN_TRAKT_CLIENT_ID HAVEN_SIMKL_CLIENT_ID HAVEN_OMDB_API_KEY; do
  [ -n "${!v:-}" ] && set_keys=$((set_keys + 1))
done
echo "Build keys: $set_keys of 3 app keys set (values not shown)"
