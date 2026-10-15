# shellcheck shell=bash
# Assertion counting and fixture primitives shared by scripts/tests/*-test.
#
# Sourced, never executed: no shebang, no executable bit, a .sh extension so
# linters recognize it. The same rule libs/detect.sh follows, for the same
# reason -- pre-commit reads a shebang only on an executable file, so a file
# carrying one without the bit is skipped in silence.
#
# What is here is what every suite would otherwise write identically: a scratch
# repository with the scripts copied in, a unit declaration, a command stubbed
# on PATH, and the counting. A fixture() that knows each suite's world is still
# not here -- it would take a parameter per suite and be read in three places
# to understand one. Each suite composes its world from these.
#
# Callers use `set -uo pipefail` without -e: a failing assertion is recorded and
# the remaining cases still run, so one break does not hide the rest.
#
# No suite changes directory; a case addresses its fixture by path. Only a case
# about scripts/run lets a stub read its stdin, since run is the one capability
# that keeps the stdin it was given.

# Every suite here scaffolds throwaway Git repositories, and a fixture inherits
# the operator's global config unless told not to. One setting breaks them
# outright: a machine-wide core.hooksPath, set for an unrelated purpose such as
# an editor's Git integration or another agent's attribution hook, redirects
# every fixture's hooks away from .git/hooks. `pre-commit install` still reports
# success and the hook then never fires, so a suite asserting that a hook blocks
# a commit watches the commit succeed and reports the operator's config as a
# defect in this repository.
#
# Neutralize it by isolating the fixtures from user and system config entirely
# rather than by unsetting the one key. A fixture reading anything from outside
# the repository is the bug in general, and the empty-string override for this
# key specifically is a trap: `git config core.hooksPath ""` satisfies
# pre-commit's refusal check and installs, and the hook still does not run.
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_SYSTEM=/dev/null

# The same isolation for the two variables a real run reads from the ambient
# environment. An exported GOWORK points go_each at the modules of whatever
# workspace the developer is in, whose mains then build into the fixture's
# dist/. CI_DRY_RUN is exported by every invocation from scripts/ci, so a case
# asserting that something really ran or really deleted a file would pass by
# never running anything; a case wanting the dry run sets it per invocation.
export GOWORK=off
unset CI_DRY_RUN

harness_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
harness_path="$PATH"

# Where mktemp puts fixtures, resolved through symlinks so the cleanup guard
# below compares like with like: on macOS TMPDIR is /var/folders/..., which is
# a symlink into /private/var, and pwd -P reports the latter.
harness_tmp="$(cd "${TMPDIR:-/tmp}" && pwd -P)"

passed=0
failed=0
skipped=0
not_applicable_count=0
current=""

# The scratch directory the current case is using, and empty between cases.
work=""

# A fresh scratch repository: a mktemp directory, git-initialised with an
# identity so a fixture can commit, the scripts tree copied in, and an empty
# bin/ at the head of PATH for stub. Removes the previous one first, so a suite
# calls this per case and never cleanup.
scratch_repo() {
  cleanup
  work="$(mktemp -d)"
  work="$(cd "$work" && pwd -P)"
  git -C "$work" init -q
  git -C "$work" config user.email test@example.com
  git -C "$work" config user.name test
  git -C "$work" config commit.gpgsign false
  cp -r "$harness_root/scripts" "$work/"
  mkdir "$work/bin"
  PATH="$work/bin:$harness_path"
}

# Writes apps/<name>/.unit.json: the run fact, the ship kind, and any targets
# after it. Defaults to a unit that neither runs nor ships, which is the legal
# declaration a fixture exercising some other rule owes.
declare_unit() {
  local name="$1" run="${2:-none}" kind="${3:-none}" targets=""
  if (( $# > 3 )); then
    targets="$(printf ', "%s"' "${@:4}")"
    targets=", \"targets\": [${targets:2}]"
  fi
  mkdir -p "$work/apps/$name"
  printf '{"schema_version": 1, "run": "%s", "ships": {"kind": "%s"%s}}\n' \
    "$run" "$kind" "$targets" > "$work/apps/$name/.unit.json"
}

# Writes a well-formed quadlet pair and its Containerfile under
# apps/<name>/deploy/quadlet/, for a case to then break in exactly one way.
quadlet_pair() {
  local dir="$work/apps/$1/deploy/quadlet"
  mkdir -p "$dir"
  printf '[Build]\nImageTag=localhost/%s:latest\nFile=./Containerfile\n' "$1" > "$dir/$1.build"
  printf '[Container]\nImage=localhost/%s:latest\n' "$1" > "$dir/$1.container"
  printf 'FROM scratch\n' > "$dir/Containerfile"
}

# Puts <command> on the scratch repository's PATH with the body read from
# stdin, shebang included: `stub gh <<'STUB' ... STUB`.
stub() {
  [[ -n "$work" ]] || { echo "stub: no scratch repository; call scratch_repo first" >&2; return 1; }
  cat > "$work/bin/$1"
  chmod +x "$work/bin/$1"
}

# A PATH holding only the named tools, for a case asserting what a command does
# without one: `PATH="$(minimal_path bash git)" scripts/structure`. Each tool
# the harness PATH has is linked into a fresh directory under the scratch
# repository; one it lacks is left out, so the case sees the absence it would
# on a host without it.
minimal_path() {
  [[ -n "$work" ]] || { echo "minimal_path: no scratch repository; call scratch_repo first" >&2; return 1; }
  local dir="$work/minimal" tool
  mkdir -p "$dir"
  for tool in "$@"; do
    command -v "$tool" >/dev/null 2>&1 && ln -sf "$(command -v "$tool")" "$dir/$tool"
  done
  echo "$dir"
}

# The fixture most suites need: a fresh scratch repository and the case's
# name. A suite whose cases need more defines its own fixture after sourcing
# this file, which replaces this one.
fixture() {
  scratch_repo
  current="$1"
}

# Guarded on being under the temp directory as well as on being set: this runs
# from an EXIT trap, where an rm -rf reached by an unexpected path has nothing
# left to stop it. Leaves the directory first for a suite that cd'd into it.
cleanup() {
  if [[ -n "$work" && "$work" == "$harness_tmp"/* ]]; then
    [[ "$PWD" == "$work" || "$PWD" == "$work"/* ]] && cd /
    rm -rf "$work"
  fi
  work=""
  PATH="$harness_path"
}
trap cleanup EXIT

fail() {
  echo "  FAIL  $current: $1"
  failed=$((failed + 1))
}

pass() {
  passed=$((passed + 1))
}

# A case the host cannot run. Counted, so a suite that skipped everything is
# told apart from one that verified everything.
skip() {
  echo "  skip  ${current:+$current: }$1"
  skipped=$((skipped + 1))
}

# A suite with no case to run on this host, as opposed to a case it could not
# run. The subject ships outside this repository, so its absence is the Result
# state not-applicable rather than unavailable: session-start-test is the one
# suite that has it, when the operator's global hook is not installed.
#
# Declared rather than ridden on an empty tally. Nothing counted used to exit
# 0, which made a deliberate no-op read exactly like a suite that returned
# early by mistake -- a host gate taken on the wrong condition, a loop over a
# list that came back empty, an early return left by a refactor. Now the suite
# says which one it is, and a suite that says nothing fails.
not_applicable() {
  echo "  n/a   ${current:+$current: }$1"
  not_applicable_count=$((not_applicable_count + 1))
}

# Prints the tally and returns the suite's exit status, so a caller ends with
# `report` as its last line and the script's status is the suite's result. The
# four counts are the interface; the exit status reads them as Result states. A
# suite that verified nothing is green only where it declared itself
# not-applicable: passing nothing beside a skip is unavailable, and counting
# nothing at all is a suite that never reached its cases.
report() {
  echo
  echo "$passed passed, $failed failed, $skipped skipped, $not_applicable_count not-applicable"
  (( failed == 0 )) && (( passed > 0 || not_applicable_count > 0 ))
}
