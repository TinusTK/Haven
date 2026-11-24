#!/bin/bash
# Checks the App Store build of the Swift app holds no add-on code or
# wording:
#
#   iphone/scripts/store-check.sh <Haven.app>
#
# Looks at Haven's own program and every file in the app except the libmpv
# frameworks, for:
#   - "add-on" in any case: wording a person could read;
#   - "Addon" with a capital A: add-on code, because Swift keeps the names
#     of types and functions (StoredAddon, saveAddon, …) in the program.
# The lowercase "addons" table name is allowed: every build's database
# has that table, so a database (and a backup) is the same in each build.
set -euo pipefail

app="$1"
[ -d "$app" ] || { echo "store check: no app at $app"; exit 1; }

found=""
while IFS= read -r -d '' file; do
  hits="$( (strings -a "$file" 2>/dev/null || true) | grep -E -e '[Aa][Dd][Dd]-[Oo][Nn]' -e 'Addon' | head -5 || true)"
  if [ -n "$hits" ]; then
    found+="${file#"$app"/}:"$'\n'"$hits"$'\n'
  fi
done < <(find "$app" -type f -not -path "$app/Frameworks/*" -print0)

if [ -n "$found" ]; then
  echo "store check: the App Store build has add-on code or wording:"
  printf '%s' "$found"
  exit 1
fi
echo "store check: no add-on code or wording in $(basename "$app")"
