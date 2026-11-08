#!/usr/bin/env bash
# Signs one prebuilt native-mod build (#11859, crates/horizon-release-sign/src/native_build.rs) in
# horizon-mods' native-build workflow: writes <key16>-<target>.sig beside the archive, one base64 Ed25519
# signature per line over "horizon native build <key> <target>\n<sha256>  <archive>\n", for every key set in
# NATIVE_BUILD_SIGNING_KEY and NATIVE_BUILD_SIGNING_KEY_NEXT (base64 PKCS#8, the `native-builds` environment's
# secrets). The key and target are the workflow's own inputs, never read from the build: a build can only be
# signed as the set it was asked for. The job that runs this holds the private keys, so it runs nothing from the
# build but this file, which calls only sha256sum and OpenSSL.
# Each key's public half must be listed in native-build-keys.txt (beside this repository's copy of the script),
# else launchers would refuse the file: the script fails instead.
# EPHEMERAL=yes (dry runs, tests) signs with a throwaway key instead and writes its public key to ephemeral.pub.
#   tools/ci/sign_native_build.sh <key, 64 hex> <target> dist/<key16>-<target>.tar.gz
set -euo pipefail
key=$1 target=$2 archive=$3
openssl=${OPENSSL:-openssl}
keys_file=${NATIVE_BUILD_KEYS:-$(dirname "$0")/../../crates/horizon-release-sign/native-build-keys.txt}
[[ "$key" =~ ^[0-9a-f]{64}$ ]] || { echo "$key: not a build key (64 lowercase hex digits)" >&2; exit 1; }
[[ "$target" =~ ^[a-z0-9_]+(-[a-z0-9_]+)+$ ]] || { echo "$target: not a target triple" >&2; exit 1; }
dir=$(dirname "$archive")
name=$(basename "$archive")
[[ "$name" == "${key:0:16}-$target.tar.gz" ]] || { echo "$name: must be named ${key:0:16}-$target.tar.gz" >&2; exit 1; }
if command -v sha256sum > /dev/null; then sha=$(sha256sum < "$archive"); else sha=$(shasum -a 256 < "$archive"); fi
sha=${sha%% *}
umask 077
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf 'horizon native build %s %s\n%s  %s\n' "$key" "$target" "$sha" "$name" > "$work/message"
out="$dir/${key:0:16}-$target.sig"
rm -f "$out" "$dir/ephemeral.pub"

sign_with() {  # the key file in $1; $2 = "listed" to require its public key in the keys file
  "$openssl" pkey -inform DER -in "$1" -pubout -outform DER | tail -c 32 | base64 > "$work/public"
  if [[ "$2" == listed ]] && ! grep -qxF "$(cat "$work/public")" <(sed 's/[[:space:]]//g' "$keys_file"); then
    echo "a signing key's public half is not in $(basename "$keys_file"): launchers would refuse this build" >&2
    exit 1
  fi
  "$openssl" pkeyutl -sign -rawin -inkey "$1" -keyform DER -in "$work/message" -out "$work/signature"
  "$openssl" pkeyutl -verify -rawin -pubin -inkey <("$openssl" pkey -inform DER -in "$1" -pubout) \
    -in "$work/message" -sigfile "$work/signature" > /dev/null
  base64 < "$work/signature" | tr -d '\n' >> "$out"
  echo >> "$out"
  rm -f "$1" "$work/signature"
}

if [[ -n "${EPHEMERAL:-}" ]]; then
  "$openssl" genpkey -algorithm ed25519 -outform DER -out "$work/key"
  "$openssl" pkey -inform DER -in "$work/key" -pubout -outform DER | tail -c 32 | base64 > "$dir/ephemeral.pub"
  sign_with "$work/key" any
else
  for var in NATIVE_BUILD_SIGNING_KEY NATIVE_BUILD_SIGNING_KEY_NEXT; do
    if [[ -n "${!var:-}" ]]; then
      # From the environment into a private temporary file: never an argument, never printed.
      printf '%s' "${!var}" | base64 --decode > "$work/key"
      sign_with "$work/key" listed
    fi
  done
fi
[[ -s "$out" ]] || { echo "no signing key: NATIVE_BUILD_SIGNING_KEY is not set" >&2; exit 1; }
echo "$name: sha256 $sha, signed with $(wc -l < "$out" | tr -d ' ') key(s) into $(basename "$out")"
