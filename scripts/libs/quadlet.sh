# shellcheck shell=bash
# Validation of a quadlet unit's deploy/quadlet/ files. Sourced by scripts/package,
# which is the ship-fact dispatch and holds nothing about quadlet itself.
#
# Sourced, never executed: no shebang, no executable bit, a .sh extension so
# linters recognize it. The same rule libs/detect.sh follows, for the same
# reason.

# Every assignment of one key in a systemd unit file, one per line. systemd
# takes the last assignment of most keys, but `ImageTag` repeats where the
# option does -- `podman build` takes several `--tag` -- so all of them are
# returned and each caller decides which it means.
quadlet_values() {
  sed -n -E "s/^[[:space:]]*$2[[:space:]]*=[[:space:]]*(.*[^[:space:]])[[:space:]]*\$/\\1/p" "$1"
}

# A quadlet unit ships unit files, not an image. systemd and podman build it on
# the deploy host when the unit starts, or pull it from a registry something
# else publishes to, so packaging one validates the unit files and produces
# nothing. Nothing here shells out to a container runtime, which is why
# the full gate still passes on a machine with no podman installed and why
# scripts/doctor requires none.
#
# What it catches is the mismatch that would otherwise surface as a service that
# fails to start on the deploy host: a Containerfile that is not where the build
# says it is, or a container asking for a local image no build here produces.
#
# A .build is optional: a .container may run an image pulled by a fully
# qualified registry reference, whose first path component is a host
# (`ghcr.io/...`, `registry:5000/...`). `localhost/...` and a bare name resolve
# to an image on the deploy host, so one of those no .build here produces is
# still the mismatch above. A registry on the deploy host itself
# (`localhost:5000/...`) is still a registry, something pushes to it, so it
# passes. A `.image` unit, quadlet's own way to pull, is not accepted: no unit
# has needed one, and `Image=foo.image` is refused like any other bare name.
#
# quadlet_registry_image <image>: whether the reference names a registry host,
# the part before the first / carrying a . or a : and not being localhost.
quadlet_registry_image() {
  local host="${1%%/*}"
  [[ "$1" == */* && "$host" != localhost && "$host" == *[.:]* ]]
}

# quadlet_validate <dir> <label>: the directory holding the unit files, and the
# name the messages report the unit under. They are listed with compgen rather
# than a glob so that no match is no file, whatever the caller's nullglob.
quadlet_validate() {
  local dir="$1" label="$2" file value status=0
  local -a builds=() containers=() produced=()

  mapfile -t builds < <(compgen -G "$dir/*.build" || true)
  mapfile -t containers < <(compgen -G "$dir/*.container" || true)

  if (( ${#containers[@]} == 0 )); then
    echo "$label declares ships: quadlet but $dir/ holds no .container unit." >&2
    return 1
  fi

  for file in "${builds[@]}"; do
    # The image names this build produces, plus the build unit's own file name:
    # a .container may ask for either, and quadlet resolves the second to the
    # first itself.
    mapfile -t -O "${#produced[@]}" produced < <(quadlet_values "$file" ImageTag)
    produced+=("$(basename "$file")")

    if [[ -z "$(quadlet_values "$file" ImageTag)" ]]; then
      echo "$file names no ImageTag, so nothing can depend on what it builds." >&2
      status=1
    fi

    value="$(quadlet_values "$file" File | tail -n 1)"
    if [[ -z "$value" ]]; then
      # podman reads the Containerfile out of the working directory when the
      # build names no File, so this is a legal unit rather than a broken one.
      if [[ -z "$(quadlet_values "$file" SetWorkingDirectory | tail -n 1)" ]]; then
        echo "$file names neither File nor SetWorkingDirectory, so podman has no Containerfile to build." >&2
        status=1
      fi
    elif [[ "$value" != - && "$value" != http://* && "$value" != https://* ]]; then
      # A relative File is relative to the unit file, which is what podman
      # resolves it against.
      [[ "$value" == /* ]] || value="$dir/$value"
      if [[ ! -e "$value" ]]; then
        echo "$file names File=$value, which does not exist." >&2
        status=1
      fi
    fi
  done

  for file in "${containers[@]}"; do
    value="$(quadlet_values "$file" Image | tail -n 1)"
    if [[ -z "$value" ]]; then
      echo "$file names no Image, so there is nothing for it to run." >&2
      status=1
      continue
    fi
    if ! grep -qxF -- "$value" <<< "$(printf '%s\n' "${produced[@]}")" \
      && ! quadlet_registry_image "$value"; then
      echo "$file asks for Image=$value, which no .build unit here produces and which is not a fully qualified registry reference." >&2
      echo "An image is built here or pulled by a fully qualified registry reference (host/path, the host carrying a . or a :); a .image unit is not accepted. Built here: ${produced[*]:-nothing}" >&2
      status=1
    fi
  done

  (( status == 0 )) && echo "$label ships a quadlet: validated $dir/, nothing built."
  return "$status"
}
