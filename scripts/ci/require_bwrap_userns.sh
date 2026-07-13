#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Bubblewrap execution proof requires a Linux runner" >&2
  exit 1
fi

restriction=/proc/sys/kernel/apparmor_restrict_unprivileged_userns
if [[ -e "$restriction" ]] && [[ "$(<"$restriction")" != 0 ]]; then
  echo 0 | sudo tee "$restriction" >/dev/null
fi

bwrap_path="$(nix develop .#ci -c bash -lc 'command -v bwrap')"
case "$bwrap_path" in
  /nix/store/*/bin/bwrap) ;;
  *)
    echo "Bubblewrap must resolve from the pinned Nix store: $bwrap_path" >&2
    exit 1
    ;;
esac

"$bwrap_path" --version
"$bwrap_path" \
  --die-with-parent \
  --new-session \
  --unshare-all \
  --ro-bind / / \
  --proc /proc \
  --dev /dev \
  /bin/true

if [[ -n "${GITHUB_PATH:-}" ]]; then
  dirname "$bwrap_path" >>"$GITHUB_PATH"
fi
