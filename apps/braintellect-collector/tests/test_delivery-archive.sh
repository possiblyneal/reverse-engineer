#!/usr/bin/env bash
# Build verification for the owner delivery archive: scripts/build-delivery must
# produce a ZIP holding the collector and the owner instructions, and nothing
# else -- no tests, no fixtures, no exports. Runs without the VM.
set -euo pipefail
cd "$(dirname "$0")/.."
repo_root=$(git rev-parse --show-toplevel)

out=$(mktemp -d "$repo_root/tmp/delivery.XXXXXX")
trap 'rm -rf "$out"' EXIT

fail=0
check() { # name, command...
    local name=$1
    shift
    if "$@"; then printf 'PASS: %s\n' "$name"; else printf 'FAIL: %s\n' "$name"; fail=1; fi
}

built=$(scripts/build-delivery "$out/delivery.zip")
check "build-delivery prints the archive path" test "$built" = "$out/delivery.zip"
check "archive is valid" unzip -tq "$out/delivery.zip"

members=$(unzip -Z1 "$out/delivery.zip" | sort)
expected=$'BST-collect.ps1\nINSTRUCTIONS-for-tablet-owner.txt'
check "archive holds only the collector and the owner instructions" test "$members" = "$expected"

exit "$fail"
