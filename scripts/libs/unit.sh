# shellcheck shell=bash
# Which unit a command acts on, and what that unit declared.
#
# Source this, do not execute it; after libs/detect.sh where that is sourced
# too. Two commands take an optional unit name, cd into that unit, and dispatch
# on a fact the unit stated about itself -- scripts/run on how it runs,
# scripts/package on what it ships. scripts/release reads the same fact for
# every unit at once, which is why one accessor below takes a path instead.
# Resolving and reading live here once rather than in three scripts that would
# drift, which is the failure libs/detect.sh already exists to prevent.
#
# The values are not validated here. scripts/structure checks every declared
# value against a fixed enum on every commit, so a second copy of the enums
# would be a second answer to the same question.

# Every unit under apps/, one per line. A unit is a directory directly under
# apps/, which is the shape scripts/structure enforces.
unit_names() {
  local entry
  for entry in apps/*/; do
    [[ -d "$entry" ]] || continue
    basename "$entry"
  done
}

# The units a caller could have named, as the command line that names them.
# $1 is the script's own name, so the listing is a line to copy rather than a
# hint to translate.
unit_list() {
  local command="$1" name
  local -a names=()
  mapfile -t names < <(unit_names)

  if (( ${#names[@]} == 0 )); then
    echo "apps/ has no entries yet."
    return
  fi

  echo "Available:"
  for name in "${names[@]}"; do
    echo "  scripts/$command $name"
  done
}

# The unit this invocation acts on, printed on stdout. $1 is the name given on
# the command line and may be empty; $2 is the calling script's own name.
#
# Empty output means apps/ holds no units at all, which is not an error: a
# repository can have work to run before it has anything to deploy, and the
# caller falls back to the repository root. One unit needs no naming; several
# do, because picking one would act on the wrong program as often as the right
# one.
unit_resolve() {
  local requested="$1" command="$2"
  local -a names=()
  mapfile -t names < <(unit_names)

  if [[ -n "$requested" ]]; then
    if [[ ! -d "apps/$requested" ]]; then
      echo "No apps/$requested directory." >&2
      unit_list "$command" >&2
      return 1
    fi
    echo "$requested"
    return 0
  fi

  case "${#names[@]}" in
    0) return 0 ;;
    1) echo "${names[0]}" ;;
    *)
      echo "apps/ holds several units, so which one has to be said." >&2
      unit_list "$command" >&2
      return 1
      ;;
  esac
}

# What the unit in the current directory declared, as the variables the
# capability adapters read. Call after cd'ing into the unit.
#
# jq rather than a shell parse, for the reason scripts/structure gives: reading
# a structured format by hand is how a check comes to accept one of several
# valid spellings and call the rest wrong.
#
# unit_args and unit_entry are the two of the five no declaration holds:
# scripts/run fills them from the arguments after `--` and from --entry.
# Declared here with the rest so every adapter's context has one home and a
# caller with no unit leaves all of it empty.
#
# unit_entry is singular and unit_targets plural for a reason beyond arity:
# they are different vocabularies. A target is a platform a unit ships to, from
# .unit.json's ships.targets; an entry is one of the programs the unit's own
# language manifest already declares -- a [project.scripts] name, a bin key, a
# main package. Nothing declares the entry set here, because the manifest that
# defines it is the one the adapter reads anyway.
unit_run=""
unit_ships=""
unit_targets=()
# shellcheck disable=SC2034 # filled by scripts/run, read by the adapters
unit_args=()
# shellcheck disable=SC2034 # filled by scripts/run, read by the run adapters
unit_entry=""

# The path of the declaration for the unit at $1, on stdout, once it is known to
# be readable. Both readers below go through it, so a missing file and a missing
# jq are named the same way whichever one asked.
unit_declaration() {
  local file="${1%/}/.unit.json"

  if [[ ! -s "$file" ]]; then
    echo "No $file. A unit declares how it runs and what it ships, and" >&2
    echo "scripts/structure requires the file on every unit." >&2
    return 1
  fi

  if ! command -v jq > /dev/null 2>&1; then
    echo "jq is needed to read $file. scripts/doctor names it." >&2
    return 1
  fi

  echo "$file"
}

# The three are read by the run and package adapters in libs/detect.sh, which is
# a separate file, so nothing in this one uses them.
# shellcheck disable=SC2034
unit_read() {
  local file
  file="$(unit_declaration "$PWD")" || return 1

  unit_run="$(jq -r '.run' "$file")"
  unit_ships="$(jq -r '.ships.kind' "$file")"
  mapfile -t unit_targets < <(jq -r '.ships.targets // [] | .[]' "$file")
}

# What the unit at $1 ships, on stdout. The path form of the read above, for the
# one caller that walks every unit in a single process and so cannot cd into
# each: scripts/release, deciding whether a unit that produced no file broke its
# own declaration. A second hand-written path expression and jq query in that
# script is the drift this file exists to prevent.
unit_ships_of() {
  local file
  file="$(unit_declaration "$1")" || return 1

  jq -r '.ships.kind' "$file"
}
