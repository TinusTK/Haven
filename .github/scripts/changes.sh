#!/bin/bash
# Says which builds a run needs. Prints the files the push (or pull
# request) changed, one per line, or ALL when every build should run:
#   - "[ci all]" in a pushed commit's message (or the pull request's title)
#   - started by hand (Run workflow) or by anything other than a push or
#     pull request
#   - a new branch, a force push, or more files than GitHub lists (300),
#     where the changed files can't be known for sure
# Used by the "changes" job in build.yml and iphone.yml.
set -euo pipefail

event="$GITHUB_EVENT_PATH"
all() { echo ALL; exit 0; }

case "$GITHUB_EVENT_NAME" in
  push)
    messages="$(jq -r '.commits[]?.message' "$event")"
    grep -qiF '[ci all]' <<<"$messages" && all
    before="$(jq -r .before "$event")"
    after="$(jq -r .after "$event")"
    ;;
  pull_request)
    title="$(jq -r '.pull_request.title' "$event")"
    grep -qiF '[ci all]' <<<"$title" && all
    before="$(jq -r .pull_request.base.sha "$event")"
    after="$(jq -r .pull_request.head.sha "$event")"
    ;;
  *) all ;;
esac

[[ "$before" =~ ^0*$ ]] && all
files="$(gh api "repos/$GITHUB_REPOSITORY/compare/$before...$after" --jq '.files[].filename' 2>/dev/null)" || all
[ -n "$files" ] || all
[ "$(grep -c . <<<"$files")" -lt 300 ] || all
echo "$files"
