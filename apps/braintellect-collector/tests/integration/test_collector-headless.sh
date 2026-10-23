#!/usr/bin/env bash
# Integration test: one headless collector invocation against the synthetic
# BRAINtellect install in the Windows VM, then inspection of the ZIP it returns.
#
# The seam is the collector's own contract: BST_NO_UI=1, a zero exit, one ZIP.
# Needs the VM container `windows` on server with C:\braintest seeded
# (person=3, session=4, flowdesign=2, montagesystem=3) and an admin PowerShell
# window focused on its desktop, which is where the keystrokes land. The guest
# is driven through the QEMU monitor, so it needs no guest SSH.
# shellcheck disable=SC2317,SC2329,SC2029 # has/has_re run through check; $run_host expands client-side on purpose
set -euo pipefail
cd "$(dirname "$0")/../.."
repo_root=$(git rev-parse --show-toplevel)

SERVER=neal@10.10.10.10
WINDOWS_CONTAINER=${WINDOWS_CONTAINER:-systemd-windows}
SHARE_HOST=/srv/local/windows/shared/bsttest # guest: \\host.lan\data\bsttest
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
        ssh "$SERVER" "podman exec -i $WINDOWS_CONTAINER sh -c 'nc -U -w3 /run/shm/monitor.sock >/dev/null'"
}

ssh "$SERVER" "mkdir -p $run_host"
scp -q src/Collect-BRAINtellect2-Data.cmd tests/integration/guest-run.ps1 "$SERVER:$run_host/"
type_in_guest "powershell -noprofile -executionpolicy bypass -file \\\\host.lan\\data\\bsttest\\$run\\guest-run.ps1"

finished=0
for _ in $(seq 1 360); do
    if ssh "$SERVER" "grep -q DONE $run_host/guest.log" 2>/dev/null; then finished=1; break; fi
    sleep 5
done
scp -q "$SERVER:$run_host/*" "$out/" || true # the log may not exist yet after a timeout
[ -f "$out/guest.log" ] && cat "$out/guest.log"
if [ "$finished" -ne 1 ]; then
    echo "FAIL: guest run did not finish within 15 minutes (a window left open hangs it)"
    exit 1
fi

# --- ok case: the whole collection ---
check "headless run exits zero" has "$out/guest.log" 'ok exit=0'
check "successful run removes its private copy" has "$out/guest.log" 'ok temp leftovers=0'
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
reg_to_utf8() { iconv -f UTF-16 -t UTF-8 "$1" >"$2"; } # reg.exe writes UTF-16
registryFile=$(find "$out/ok/registry" -type f -name '*.reg' | head -n 1)
check "registry export creates a file" test -n "$registryFile"
check "registry export converts to text" reg_to_utf8 "$registryFile" "$out/ok/license.reg"
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

# --- retry case: database/state capture succeeds without duplicate program files ---
check "retry run exits zero" has "$out/guest.log" 'retry exit=0'
check "retry run removes its private copy" has "$out/guest.log" 'retry temp leftovers=0'
check "retry ZIP is a valid archive" unzip -tq "$out/retry.zip"
mkdir "$out/retry"
unzip -qq -o "$out/retry.zip" -d "$out/retry" 2>/dev/null || true
check "retry keeps database" has "$out/retry/database/BST-data-retry.sql" "INSERT INTO \`person\` VALUES (1,'Jane'"
check "retry keeps application state" test -d "$out/retry/application-state"
check "retry excludes install copy" test ! -e "$out/retry/install"

# --- ui case: the default window mode ends by itself and delivers the same ZIP ---
check "window run exits zero" has "$out/guest.log" 'ui exit=0'
check "window run removes its private copy" has "$out/guest.log" 'ui temp leftovers=0'
check "window run ZIP is a valid archive" unzip -tq "$out/ui.zip"
mkdir "$out/ui"
unzip -qq -o "$out/ui.zip" -d "$out/ui" 2>/dev/null || true
for stage in '2. Database' '3. Registry'; do
    check "window run stage ok: $stage" has "$out/ui/STATUS.txt" "[OK] $stage"
done
check "window run keeps SQL rows" has "$out/ui/database/BST-full-export.sql" "INSERT INTO \`person\` VALUES (1,'Jane'"

# --- fail case: the failure reaches the caller and the export ---
check "missing database exits non-zero" has_re "$out/guest.log" 'fail exit=[1-9]'
check "failed run removes its private copy" has "$out/guest.log" 'fail temp leftovers=0'
mkdir "$out/fail"
unzip -qq -o "$out/fail.zip" -d "$out/fail" 2>/dev/null || true
check "failed stage is recorded" has "$out/fail/STATUS.txt" '[FAIL] 2. Database'
check "failed run still returns a valid ZIP" unzip -tq "$out/fail.zip"
check "failed run still writes SYSTEM-INFO" has_re "$out/fail/SYSTEM-INFO.txt" 'OS \(Caption\):'
check "failed database does not stop the registry stage" has "$out/fail/STATUS.txt" '[OK] 3. Registry'
check "failed database does not stop the install stage" has "$out/fail/STATUS.txt" '[OK] 1. Install folder(s)'
check "window run fails the caller too" has_re "$out/guest.log" 'uifail exit=[1-9]'
check "failed window run removes its private copy" has "$out/guest.log" 'uifail temp leftovers=0'

# --- retry fail case: the retry flow also continues after the database stage fails ---
check "retry with missing database exits non-zero" has_re "$out/guest.log" 'retryfail exit=[1-9]'
check "retry with missing database removes its private copy" has "$out/guest.log" 'retryfail temp leftovers=0'
check "retry failure ZIP is a valid archive" unzip -tq "$out/retryfail.zip"
mkdir "$out/retryfail"
unzip -qq -o "$out/retryfail.zip" -d "$out/retryfail" 2>/dev/null || true
check "retry failure records the database stage" has "$out/retryfail/STATUS.txt" '[FAIL] 2. Database'
check "retry failure still captures the registry" has "$out/retryfail/STATUS.txt" '[OK] 3. Registry'
check "retry failure still writes SYSTEM-INFO" test -s "$out/retryfail/SYSTEM-INFO.txt"

# --- relocated case: configuration points at a datadir and state outside the install ---
for flow in reloc relocretry; do
    check "$flow exits zero" has "$out/guest.log" "$flow exit=0"
    check "$flow ZIP is a valid archive" unzip -tq "$out/$flow.zip"
    mkdir "$out/$flow"
    unzip -qq -o "$out/$flow.zip" -d "$out/$flow" 2>/dev/null || true
    check "$flow database stage ok" has "$out/$flow/STATUS.txt" '[OK] 2. Database'
    check "$flow captures relocated datadir rows" grep -rqF "INSERT INTO \`person\` VALUES (1,'Jane'" "$out/$flow/database"
    check "$flow captures relocated state" grep -rqF 'relocated-state=1' "$out/$flow/application-state"
    check "$flow leaves unrelated MySQL out" test -z "$(grep -rlF 'unrelated-mysql=1' "$out/$flow" || true)"
done
check "relocated normal run keeps the install copy" test -d "$out/reloc/install"
check "relocated retry run excludes the install copy" test ! -e "$out/relocretry/install"

exit "$fail"
