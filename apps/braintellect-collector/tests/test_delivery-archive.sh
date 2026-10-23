#!/usr/bin/env bash
# Build verification for the owner delivery archives. Both archives hold only the
# collector variant and matching owner instructions; no tests, fixtures, or exports.
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
check "normal archive is valid" unzip -tq "$out/delivery.zip"
members=$(unzip -Z1 "$out/delivery.zip" | sort)
expected=$'Collect-BRAINtellect2-Data.cmd\nINSTRUCTIONS-for-tablet-owner.txt'
check "normal archive holds only its collector and instructions" test "$members" = "$expected"

built=$(scripts/build-delivery --retry "$out/retry.zip")
check "retry build prints the archive path" test "$built" = "$out/retry.zip"
check "retry archive is valid" unzip -tq "$out/retry.zip"
members=$(unzip -Z1 "$out/retry.zip" | sort)
expected=$'Collect-BRAINtellect2-Data-Retry.cmd\nINSTRUCTIONS-for-tablet-owner-retry.txt'
check "retry archive holds only its collector and instructions" test "$members" = "$expected"
unzip -p "$out/retry.zip" Collect-BRAINtellect2-Data-Retry.cmd >"$out/retry.cmd"
check "retry collector has retry mode enabled" grep -qF 'set "BST_RETRY_ONLY=1"' "$out/retry.cmd"
check "retry collector preserves CRLF" python3 -I - "$out/retry.cmd" <<'PY'
from pathlib import Path
script = Path(__import__('sys').argv[1]).read_bytes()
assert b'\r\n' in script
assert b'\n' not in script.replace(b'\r\n', b'')
PY

exit "$fail"
