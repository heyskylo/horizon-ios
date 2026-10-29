#!/usr/bin/env bash
# Makes a release signing key (docs/security.md, "Signed updates"): the private key goes straight from
# horizon-release-sign into the `release` environment's secret through a pipe (never on disk, never printed,
# never in the Agents vault); the public key is printed for crates/horizon-release-sign/release-keys.txt.
# Needs a gh login that may write the repository's environment secrets (the maintainer's), and the `release`
# environment to exist.
#
#   tools/ci/new_release_key.sh                            # the first key: RELEASE_SIGNING_KEY
#   tools/ci/new_release_key.sh RELEASE_SIGNING_KEY_NEXT   # rotating: the next key, signed with alongside
set -euo pipefail
secret=${1:-RELEASE_SIGNING_KEY}
case "$secret" in RELEASE_SIGNING_KEY|RELEASE_SIGNING_KEY_NEXT) ;; *) echo "unknown secret $secret" >&2; exit 2 ;; esac
repo=${GITHUB_REPOSITORY:-horizon-rust/horizon}
cd "$(dirname "$0")/../.."
gh api "repos/$repo/environments/release" > /dev/null || {
  echo "The release environment doesn't exist yet: create it first (docs/security.md)." >&2; exit 1; }
public=$(mktemp)
trap 'rm -f "$public"' EXIT
cargo run --quiet --locked -p horizon-release-sign -- keygen --public-out "$public" \
  | gh secret set "$secret" --env release --repo "$repo"
echo "$secret is set in $repo's release environment. Its public key, for crates/horizon-release-sign/release-keys.txt:"
cat "$public"
