# shellcheck shell=bash
# Which manifests Dependabot is watching, and which tracked ones it is not.
# Sourced by scripts/doctor, which reports the gap as a passing row carrying
# the advisory rather than as a finding. The entries are derived per
# repository by the /repo-builder flows, so a manifest added since the last
# update flow owes one: a line for the operator to read, never a red gate on a
# repository for gaining a module. Nor is it not-applicable, which says the
# check did not apply; here it applied and found something, and the detail is
# what separates the two passing rows. A workspace member is still named here:
# the exception for it is the advisory's, in scripts/doctor.
#
# The derivation rule in apps/repo-builder/src/references/lifecycle.md excludes
# the payload tree at apps/github-repository-template/src/base-repo/, and this
# file does not. It is mirrored into that payload, so an exclusion naming its
# own path would ship to every generated repository as a rule about a
# directory none of them has. The two agree while the payload ships no
# manifest; one added there is reported here and skipped by the derivation.
#
# Sourced, never executed: no shebang, no executable bit, a .sh extension so
# linters recognize it. The same rule libs/detect.sh follows, for the same
# reason.
#
# This is not in libs/detect.sh. That module owns language-specific decisions
# and is dispatched by language_capabilities, which answers whether a language
# is present from its *root* manifest. The question here is a different one --
# where every manifest sits, including the nested ones a root workspace file
# points at -- and the answer is a path rather than a yes.
#
# The gap this closes is silent. Dependabot's security half reads the
# dependency graph and needs no entry, so a dependency with an advisory still
# gets a pull request; a dependency with nothing yet wrong with it never does,
# and ages until something is. Nothing else reports it: the file is valid YAML,
# the config's own check run is green -- it validates the schema and never opens
# the directories the entries name -- and the only visible symptom is a bump
# that never arrives.
#
# Both ways of being wrong are quiet, so the parsing below is written against
# both. Reading an entry that is not there reports a gap that does not exist,
# and an operator who is told twice to derive an entry already present learns to
# skip the line. Missing an entry that is there is worse: it reports the
# manifest watched, which is the silence this module exists to break.

# The manifest filenames Dependabot reads, and the package-ecosystem each one
# earns: dependabot_ecosystem_of <basename>, empty for anything else.
#
# Deliberately the same six languages the rest of the template detects, plus
# docker, and the same set the mapping table in
# apps/repo-builder/src/references/lifecycle.md states. Anything else recognized
# here and nowhere else is a shape no other check can see, so a manifest outside
# this list is reported by a /repo-builder flow on its own gate rather than
# mapped here.
#
# docker is the one entry that is not a language, and the layout is why: it
# gives base-image pins a place of their own in deploy/containerfile/, and
# scripts/package reads the Containerfile a quadlet .build names, so the shape
# is one the template already sees. Without an entry those FROM lines are pins
# nothing ever bumps. Dependabot's docker fetcher matches /dockerfile|
# containerfile/i against every file in the entry's directory (docker/lib/
# dependabot/docker/file_fetcher.rb in dependabot-core), so one entry per
# directory covers every such file in it. The names mapped are the
# conventional ones -- Dockerfile, Containerfile, and either with a `.suffix`
# or a `name.` prefix -- rather than that whole regex. A name that only
# contains the word, such as docker-notes.md or my-dockerfile-guide.txt, earns
# nothing; Dockerfile.<anything> does, and Dependabot reads it too. An ignore
# file named for its Containerfile is not an image pin and maps to nothing.
#
# go.work and settings.gradle.kts are absent on purpose. They are workspace
# files: they say where the modules are, and the modules are what Dependabot
# reads. Mapping one would derive an entry at the workspace root, and what
# becomes of such an entry is the service's own discovery behaviour rather than
# a fixed rule -- go.work discovery was merged into dependabot-core in
# dependabot/dependabot-core#14909 on 2026-05-05, a date on the engine rather
# than on its rollout. Mapping manifests and nothing else is the rule that does
# not have to track which ecosystem gained it when.
#
# pyproject.toml earns pip here and uv where a uv.lock is tracked beside it,
# which dependabot_unwatched_manifests decides because it holds the tracked
# list. pip's updater does not move uv.lock, so a pip entry over a uv project
# opens bumps whose lockfile still pins the old version.
dependabot_ecosystem_of() {
  # Lowercased for docker alone: Dependabot matches those names in any case,
  # while every language manifest below is read under its exact name.
  case "${1,,}" in
    *.dockerignore | *.containerignore) return 0 ;;
    dockerfile | containerfile | dockerfile.* | containerfile.* \
      | *.dockerfile | *.containerfile)
      printf 'docker\n'
      return 0
      ;;
  esac

  case "$1" in
    package.json) printf 'npm\n' ;;
    pyproject.toml) printf 'pip\n' ;;
    go.mod) printf 'gomod\n' ;;
    Cargo.toml) printf 'cargo\n' ;;
    build.gradle.kts) printf 'gradle\n' ;;
    Package.swift) printf 'swift\n' ;;
    *) return 0 ;;
  esac
}

# The (ecosystem, directory) pairs the config watches, one per line, tab
# separated. A `directories` list contributes one pair per member.
#
# Parsed rather than grepped, for the reason libs/precommit.sh gives at length.
# Four shapes are all valid YAML for the same configuration and an edit or a
# formatter turns one into another, so a reader that understands only some of
# them is wrong about a file nobody has broken:
#
#   - `directory` and `directories`, singular and plural.
#   - a block list, a flow list, and a flow list wrapped across lines.
#   - `package-ecosystem` anywhere in its entry rather than first. Mapping keys
#     are unordered in YAML, so an entry that opens with `schedule:` is the same
#     entry -- and reading the key position rather than the entry boundary
#     attributes the second entry's directory to the first entry's ecosystem,
#     which reports a real gap as watched.
#   - a blank or comment line inside a block list, which separates members
#     rather than ending the list.
#
# A comment is stripped before the line is read, so the prose at the top of the
# file -- which names every ecosystem, including the ones no entry lists --
# cannot be mistaken for configuration.
dependabot_watched_pairs() {
  local config="$1"
  local -a lines=()
  local raw body content value flow indent i=0
  local ecosystem="" in_directories=0 directories_indent=0 updates_indent=-1

  [[ -s "$config" ]] || return 0

  mapfile -t lines < "$config"

  while (( i < ${#lines[@]} )); do
    raw="${lines[i]%%#*}"
    i=$(( i + 1 ))

    # A line holding nothing but whitespace carries no structure. It is skipped
    # rather than treated as the end of a block list, because a blank line
    # between two members is a formatting choice and dropping every member after
    # it reports their manifests unwatched.
    [[ -n "${raw//[[:space:]]/}" ]] || continue

    body="${raw#"${raw%%[![:space:]]*}"}"
    indent=$(( ${#raw} - ${#body} ))

    if [[ "$body" == -* ]]; then
      content="${body#-}"
      content="${content#"${content%%[![:space:]]*}"}"

      # A member of the `directories` list. YAML lets a block sequence sit at
      # its key's own indentation as well as past it, and several formatters
      # write it that way, so the test is `>=` rather than `>`.
      if (( in_directories )) && (( indent >= directories_indent )); then
        _dependabot_emit_pair "$ecosystem" "$content"
        continue
      fi

      # The first `-` in the file opens the first update entry, and its column
      # is where every later entry begins.
      (( updates_indent >= 0 )) || updates_indent=$indent

      # A `-` deeper than that is a member of some other sequence inside the
      # entry -- a `groups` pattern list, an `ignore` or an `allow` -- and it is
      # skipped rather than read as a boundary. Resetting the ecosystem on one
      # drops every key after it, so an entry writing `groups` before
      # `directory` reports its own manifest unwatched.
      if (( indent > updates_indent )); then
        continue
      fi

      # Otherwise it opens a new update entry. The ecosystem resets here rather
      # than at the key, which is what keeps one entry's ecosystem from
      # reaching the next entry's directory.
      in_directories=0
      ecosystem=""
      indent=$(( indent + 2 ))
      body="$content"
      [[ -n "$body" ]] || continue
    elif (( in_directories )) && (( indent <= directories_indent )); then
      in_directories=0
    fi

    case "$body" in
      package-ecosystem:*)
        ecosystem="${body#package-ecosystem:}"
        ecosystem="${ecosystem//[[:space:]]/}"
        ecosystem="${ecosystem//\"/}"
        ecosystem="${ecosystem//\'/}"
        ;;
      directory:*)
        _dependabot_emit_pair "$ecosystem" "${body#directory:}"
        ;;
      directories:*)
        value="${body#directories:}"
        value="${value//[[:space:]]/}"

        if [[ -z "$value" ]]; then
          in_directories=1
          directories_indent=$indent
          continue
        fi

        # A flow list, which a line-length formatter is free to wrap. Reading
        # only the first line of one yields half the members and reports the
        # rest of their manifests unwatched.
        flow="$value"
        while [[ "$flow" != *']'* ]] && (( i < ${#lines[@]} )); do
          value="${lines[i]%%#*}"
          i=$(( i + 1 ))
          flow+="${value//[[:space:]]/}"
        done
        _dependabot_emit_flow "$ecosystem" "$flow"
        ;;
    esac
  done

  return 0
}

_dependabot_emit_flow() {
  local ecosystem="$1" flow="$2" member
  local -a members=()

  flow="${flow#*[}"
  flow="${flow%%]*}"

  # read -ra rather than word splitting an unquoted expansion: a member is a
  # directory value and may hold a glob, which splitting would also expand
  # against the filesystem.
  IFS=, read -ra members <<< "$flow"

  for member in ${members[@]+"${members[@]}"}; do
    _dependabot_emit_pair "$ecosystem" "$member"
  done
}

_dependabot_emit_pair() {
  local ecosystem="$1" directory="$2"

  directory="${directory//[[:space:]]/}"
  directory="${directory//\"/}"
  directory="${directory//\'/}"

  [[ -n "$ecosystem" && -n "$directory" ]] || return 0
  printf '%s\t%s\n' "$ecosystem" "$directory"
}

# Whether an entry's directory value covers a manifest's directory.
#
# A directory value may be a glob -- /.github/actions/* is how the shipped
# github-actions entry reaches the composite actions -- so a literal comparison
# reads that entry as watching a directory named "*" and reports every action
# unwatched.
#
# `*` is held inside one path segment and `**` crosses them, which is the
# ordinary glob reading and the conservative one here: a pattern read too
# widely reports a manifest watched when nothing watches it, which is the
# silence this module exists to break, while a pattern read too narrowly costs
# a line of output an operator can check.
_dependabot_directory_matches() {
  local directory="$1" pattern="$2" regex

  if [[ "$pattern" != *'*'* && "$pattern" != *'?'* ]]; then
    [[ "$directory" == "$pattern" ]]
    return
  fi

  # Built one character at a time rather than by a chain of substitutions.
  # Escaping a brace through ${var//\}/\\}} is ambiguous to the parser and
  # silently appends a stray one, and a regex that is wrong by a character
  # matches nothing -- which reports every manifest under a glob entry
  # unwatched while every test using a literal directory still passes.
  local i=0 char
  regex=""
  while (( i < ${#pattern} )); do
    char="${pattern:i:1}"
    i=$(( i + 1 ))

    case "$char" in
      '*')
        if [[ "${pattern:i:1}" == '*' ]]; then
          i=$(( i + 1 ))
          regex+='.*'
        else
          regex+='[^/]*'
        fi
        ;;
      '?') regex+='[^/]' ;;
      [[:alnum:]/_-]) regex+="$char" ;;
      *) regex+="\\$char" ;;
    esac
  done

  [[ "$directory" =~ ^${regex}$ ]]
}

# Every tracked manifest with no entry watching it, one path per line. Non-zero
# when the tracked set could not be read at all, so a caller reports a check
# that did not run rather than a green one: an empty list is indistinguishable
# from a repository where every manifest is watched.
#
# Tracked files rather than a search of the working tree. What Dependabot reads
# is the pushed tree, so `git ls-files` is not an approximation of the right
# answer, it is the right answer -- and a pruned search is not: DETECT_PRUNE_DIRS
# was written for root-manifest detection and names no agent or tool directory,
# so a gitignored checkout of this same repository sitting in one contributes
# manifests that are not in the repository at all.
dependabot_unwatched_manifests() {
  local config="$1"
  local tracked manifest directory ecosystem pair watched_ecosystem watched_directory
  local -a pairs=()

  [[ -s "$config" ]] || return 0

  tracked="$(git ls-files)" || return 1

  mapfile -t pairs < <(dependabot_watched_pairs "$config")

  while IFS= read -r manifest; do
    [[ -n "$manifest" ]] || continue

    ecosystem="$(dependabot_ecosystem_of "${manifest##*/}")"
    [[ -n "$ecosystem" ]] || continue
    if [[ "$ecosystem" == pip ]] \
      && grep -qxF "${manifest%pyproject.toml}uv.lock" <<< "$tracked"; then
      ecosystem=uv
    fi

    directory="/${manifest%/*}"
    [[ "$manifest" == */* ]] || directory="/"

    for pair in ${pairs[@]+"${pairs[@]}"}; do
      IFS=$'\t' read -r watched_ecosystem watched_directory <<< "$pair"
      [[ "$watched_ecosystem" == "$ecosystem" ]] || continue
      if _dependabot_directory_matches "$directory" "$watched_directory"; then
        continue 2
      fi
    done

    printf '%s\n' "$manifest"
  done <<< "$tracked"

  return 0
}
