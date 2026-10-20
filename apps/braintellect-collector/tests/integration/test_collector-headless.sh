#!/usr/bin/env bash
# Integration test: one headless collector invocation against the synthetic
# BRAINtellect install in the Windows VM, then inspection of the ZIP it returns.
#
# The seam is the collector's own contract: BST_NO_UI=1, a zero exit, one ZIP.
# Needs the VM container `windows` on server with C:\braintest seeded
# (person=3, session=4, flowdesign=2, montagesystem=3) and an admin PowerShell
# window focused on its desktop, which is where the keystrokes land. The guest
# is driven through the QEMU monitor, so it needs no guest SSH.
# shellcheck disable=SC2329,SC2029 # has/has_re run through check; $run_host expands client-side on purpose
set -euo pipefail
cd "$(dirname "$0")/../.."
repo_root=$(git rev-parse --show-toplevel)

SERVER=neal@10.10.10.10
SHARE_HOST=/home/neal/windows-data/bsttest # guest: \\host.lan\data\bsttest
run="r$(date +%s)"                         # fresh folder: the guest's SMB cache serves stale overwrites
run_host="$SHARE_HOST/$run"
out=$(mktemp -d "$repo_root/tmp/collector.XXXXXX")
trap 'rm -rf "$out"; ssh "$SERVER" "rm -rf $run_host"' EXIT

fail=0
check() { # name, command...
    local name=$1
    shift
    if "$@"; then printf 'PASS: %s\n' "$name"; else printf 'FAIL: %s\n' "$name"; fail=1; fi
}
has() { grep -qF -- "$2" "$1"; }      # file, fixed text
has_re() { grep -qE -- "$2" "$1"; }   # file, regex

# Type a command into the focused guest window through the QEMU monitor.
type_in_guest() {
    local text=$1 i c key keys=()
    for ((i = 0; i < ${#text}; i++)); do
        c=${text:i:1}
        case $c in
            ' ') key=spc ;;
            \\) key=backslash ;;
            ".") key="dot" ;;
            '-') key=minus ;;
            [a-z0-9]) key=$c ;;
            *) echo "type_in_guest: unmapped character '$c'" >&2; return 1 ;;
        esac
        keys+=("sendkey $key")
    done
    keys+=("sendkey ret")
    printf '%s\n' "${keys[@]}" |
        ssh "$SERVER" "podman exec -i windows sh -c 'nc -U -w3 /run/shm/monitor.sock >/dev/null'"
}

ssh "$SERVER" "mkdir -p $run_host"
scp -q src/BST-collect.ps1 tests/integration/guest-run.ps1 "$SERVER:$run_host/"
type_in_guest "powershell -noprofile -executionpolicy bypass -file \\\\host.lan\\data\\bsttest\\$run\\guest-run.ps1"

for _ in $(seq 1 180); do
    ssh "$SERVER" "grep -q DONE $run_host/guest.log" 2>/dev/null && break
    sleep 5
done
scp -q "$SERVER:$run_host/*" "$out/"
cat "$out/guest.log"

# --- ok case: the whole collection ---
check "headless run exits zero" has "$out/guest.log" 'ok exit=0'
check "ZIP is a valid archive" unzip -tq "$out/ok.zip"
mkdir "$out/ok"
unzip -qq -o "$out/ok.zip" -d "$out/ok" 2>/dev/null || true # .NET 4.5 writes backslash separators; unzip warns, then extracts
status=$out/ok/STATUS.txt
for stage in '0. System information' '1. Install folder' '2. Database' '3. Registry'; do
    check "stage ok: $stage" has "$status" "[OK] $stage"
done
sql=$out/ok/database/BST-full-export.sql
check "SQL holds person rows" has "$sql" "INSERT INTO \`person\` VALUES (1,'Jane','Doe','reacts fast'),(2,'John','Roe','slow'),(3,'Jae','Kim','tremor noted');"
# shellcheck disable=SC2016 # the backticks are literal SQL quoting
check "SQL holds session rows" has "$sql" 'INSERT INTO `session` VALUES (10,1,'"'complete'),(11,2,'incomplete'),(12,2,'complete'),(13,3,'complete');"
check "SQL holds design rows" has "$sql" "INSERT INTO \`flowdesign\` VALUES (1,'Cycle A'"
iconv -f UTF-16 -t UTF-8 "$out/ok/registry/HKEY_CURRENT_USER_Software_BST_BRAINtellect2.reg" >"$out/ok/license.reg" # reg.exe writes UTF-16
check "registry export has the license key" has "$out/ok/license.reg" '"license.expiry"="2099-12-31"'
info=$out/ok/SYSTEM-INFO.txt
check "SYSTEM-INFO: OS" has "$info" 'OS (Caption):'
check "SYSTEM-INFO: hardware" has "$info" 'Manufacturer:'
check "SYSTEM-INFO: PowerShell version" has_re "$info" 'PowerShell: +[0-9]+\.[0-9]+'
check "SYSTEM-INFO: .NET release" has_re "$info" '\.NET Release: +[0-9]+'
check "SYSTEM-INFO: CIM" has_re "$info" 'CIM cmdlet: +available'
check "SYSTEM-INFO: reg.exe" has_re "$info" 'reg.exe: +[A-Za-z]:[\\]'
check "SYSTEM-INFO: ZIP API" has_re "$info" 'ZIP API: +available'
check "SYSTEM-INFO: MySQL engine version" has_re "$info" 'MySQL engine: +.*mysqld.exe \(version [0-9]+\.[0-9]+'

# --- fail case: the failure reaches the caller and the export ---
check "missing database exits non-zero" has_re "$out/guest.log" 'fail exit=[1-9]'
mkdir "$out/fail"
unzip -qq -o "$out/fail.zip" -d "$out/fail" 2>/dev/null || true
check "failed stage is recorded" has "$out/fail/STATUS.txt" '[FAIL] 2. Database'

exit "$fail"
