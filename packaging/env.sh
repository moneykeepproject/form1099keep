# shellcheck shell=bash
# Shared setup for the packaging scripts (MoneyKeep's docs/specs/macos-release.md). Source it:
#   . "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
#
# Sets:
#   ROOT              the repository root
#   VERSION           the version in the root Cargo.toml ([workspace.package], else [package]):
#                     the version the release tag must carry, v<VERSION>
#   DIST              where release files go (default: $ROOT/dist, as cargo xtask installer)
#   CARGO_TARGET_DIR  cargo's target folder (default: $ROOT/target)
# Functions: warn, sha256.
# Written for macOS's own bash 3.2 as well as newer ones.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in one place: the root Cargo.toml's [workspace.package] or [package].
cargo_version() {
  local section="$1"
  awk -v want="$section" '
    /^\[/ { in_pkg = ($0 == want); next }
    in_pkg && $1 == "version" && $2 == "=" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
  return
}

VERSION="$(cargo_version '[workspace.package]')"
[[ -n "$VERSION" ]] || VERSION="$(cargo_version '[package]')"
if [[ -z "$VERSION" ]]; then
  echo "error: could not read the version ([workspace.package] or [package]) from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist}"
export DIST
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

# A GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
  return
}

# The SHA-256 of a file (just the hash).
sha256() {
  local file="$1"
  if command -v sha256sum >/dev/null; then sha256sum "$file" | cut -d' ' -f1; else shasum -a 256 "$file" | cut -d' ' -f1; fi
  return
}
