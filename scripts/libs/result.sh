# shellcheck shell=bash
# How a check reports: the four Result states, the printed layout, the findings
# under a failed check, and the tally that becomes the process exit status.
# Sourced by libs/detect.sh, and so by every capability command, and directly
# by structure, github-parity, changelog-check, protect-branch, and
# repo-settings.
#
# Sourced, never executed: no shebang, no executable bit, a .sh extension so
# linters recognize it. The same rule libs/detect.sh follows, for the same
# reason.
#
# The layout is an interface. A suite that greps a column relies on it, and
# scripts/summarize is the one command that parses it:
#
#   <check>            <state>          <detail>
#                        <finding>
#
# The check name is padded to 18 columns and the state to 16, so a state is
# matched with `^<check> +<state>`; each finding sits on its own line under a
# FAIL, indented exactly 21 spaces; any other indented line is not a finding.
# A state is one of pass, not-applicable, unavailable, or FAIL and nothing
# else: a check that did not run is not a check that passed, so unavailable
# fails the tally the same as FAIL does. One more word is accepted in the
# state column and is not a state: would-run, the dry-run marker for a check
# that was wired and not executed. It counts as neither, and prints in the
# same layout so a dry run reads like the run it stands in for.

result_failed=0

# The layout alone: result_line <check> <word> <detail>. What result prints
# once it has judged the state, and what a table that is not a set of Results
# -- the capability probe, whose words are wired and absent -- prints so it
# reads in the same columns. The widths live here and nowhere else.
result_line() {
  printf '%-18s %-16s %s\n' "$1" "$2" "$3"
}

# One line: result <check> <state> <detail>.
result() {
  case "$2" in
    pass | not-applicable | would-run) ;;
    unavailable | FAIL) result_failed=1 ;;
    *)
      echo "result: '$2' is not a Result state (pass, not-applicable, unavailable, FAIL, or would-run in a dry run)" >&2
      return 1
      ;;
  esac
  result_line "$1" "$2" "$3"
}

# Findings for the check currently being run, printed under its result line by
# verdict. A finding recorded twice is reported once.
result_findings=()

record() {
  local finding
  for finding in ${result_findings[@]+"${result_findings[@]}"}; do
    [[ "$finding" == "$1" ]] && return 0
  done
  result_findings+=("$1")
}

# Closes the check: pass with the summary when nothing was recorded, FAIL with
# the findings listed when something was.
verdict() {
  if (( ${#result_findings[@]} == 0 )); then
    result "$1" pass "$2"
  else
    result "$1" FAIL "${#result_findings[@]} violation(s)"
    printf '                     %s\n' "${result_findings[@]}"
  fi
  result_findings=()
}

# The process exit status: non-zero once any check reported FAIL or unavailable.
# A command's last line, so its status is the tally.
tally() {
  return "$result_failed"
}
