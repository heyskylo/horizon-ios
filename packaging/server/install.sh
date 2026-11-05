#!/bin/sh
# Installs horizon-server (docs/server.md) on Linux or macOS: the current Horizon Nightly for this computer (the
# only build Horizon ships; GitHub's latest release while there is none), downloaded from GitHub Releases and
# checked against the release's signed SHA256SUMS when OpenSSL 3 is installed (docs/release-flow.md, "Signed
# checksums"), else against the SHA-256 GitHub publishes for it. On Linux that is the server build (horizon-server-linux-x86_64.tar.gz), which needs neither ALSA nor udev.
#
#   curl -fsSL https://raw.githubusercontent.com/horizon-rust/horizon/main/packaging/server/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --version v0.5.0 --dir /opt/horizon --bin /usr/local/bin
#   sh install.sh --archive horizon-server-linux-x86_64.tar.gz --version mybuild   (an archive you have)
#
# It needs curl, tar or unzip, and sha256sum or shasum (and OpenSSL 3 for the signature: macOS's LibreSSL is not
# enough, and there the GitHub SHA-256 is all that is checked); it never asks for sudo (pick folders you can write).
# Releases go to <dir>/releases/<version>, <dir>/current points at the one in use, and <bin>/horizon-server
# runs it. The game files you prepare from your own Minecraft stay in <dir>/game-files across updates.
# Nothing of Mojang's is downloaded.
set -eu

REPO=horizon-rust/horizon
VERSION=latest
DIR="${XDG_DATA_HOME:-$HOME/.local/share}/horizon"
BIN="$HOME/.local/bin"
ARCHIVE=
# Horizon's release public keys, base64, space-separated: copy of crates/horizon-release-sign/release-keys.txt
# (rotation adds a key there first; keep the two in step).
RELEASE_KEYS="MU4oiIQcCxkZvbwSjjXs7SySCuM6ywRQ3nGaJYZG0gw="

die() { echo "horizon install: $*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || die "--version needs a tag"; VERSION=$2; shift 2 ;;
    --dir) [ $# -ge 2 ] || die "--dir needs a folder"; DIR=$2; shift 2 ;;
    --bin) [ $# -ge 2 ] || die "--bin needs a folder"; BIN=$2; shift 2 ;;
    --archive) [ $# -ge 2 ] || die "--archive needs a file"; ARCHIVE=$2; shift 2 ;;
    -h|--help) sed -n '2,14p' "$0" 2>/dev/null || true; exit 0 ;;
    *) die "unknown option $1" ;;
  esac
done

# The archives to look for, in order: on Linux the server build, else (releases before it) the full game.
MACHINE=$(uname -m)
# A shell under Rosetta 2 says x86_64 on Apple Silicon: the native build runs faster there.
[ "$(uname -s)" = Darwin ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null)" = 1 ] && MACHINE=arm64
case "$(uname -s)-$MACHINE" in
  Linux-x86_64) ASSETS="horizon-server-linux-x86_64.tar.gz horizon-linux-x86_64.tar.gz" ;;
  Darwin-arm64) ASSETS=horizon-macos-arm64.zip ;;
  # Intel Macs: the Nightlies built since #1161.
  Darwin-x86_64) ASSETS=horizon-macos-x64.zip ;;
  *) die "no Horizon release for $(uname -s) $(uname -m) yet: build it from source (docs/server.md)" ;;
esac

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
if [ -n "$ARCHIVE" ]; then
  # An archive already here (a build of your own, or one fetched by hand): checking it is up to you.
  [ -f "$ARCHIVE" ] || die "no archive $ARCHIVE"
  if [ "$VERSION" = latest ]; then TAG=local; else TAG=$VERSION; fi
  ASSET=$(basename "$ARCHIVE")
  cp "$ARCHIVE" "$TMP/$ASSET"
else
  command -v curl >/dev/null || die "curl is needed"
  if command -v sha256sum >/dev/null; then SHA="sha256sum"
  elif command -v shasum >/dev/null; then SHA="shasum -a 256"
  else die "sha256sum or shasum is needed"; fi

  api() { curl -fsSL -H "Accept: application/vnd.github+json" "https://api.github.com/repos/$REPO/releases/$1"; }
  if [ "$VERSION" = latest ]; then
    JSON=$(api tags/nightly) || JSON=$(api latest) || die "no release on GitHub"
  else
    JSON=$(api "tags/$VERSION") || die "no release $VERSION on GitHub"
  fi

  TAG=$(printf '%s\n' "$JSON" | sed -n 's/^  "tag_name": "\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$TAG" ] || die "could not read the release's tag"
  RAWTAG=$TAG
  # A Nightly installs under its date and commit, as `horizon-server update` names it:
  # "Nightly 2026-10-28 · 0c02067" is nightly-2026-10-28-0c02067. Its signature is checked for exactly that
  # version, so the title must name it (as horizon_release_sign::nightly_title_version reads it, strictly).
  NAME=
  case "$TAG" in
    nightly|nightly-*)
      NAME=$(printf '%s\n' "$JSON" | sed -n 's/^  "name": "Nightly \([0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}\) · \([0-9a-f]\{7,40\}\)"[,}]*$/nightly-\1-\2/p' | head -n 1)
      [ -n "$NAME" ] || die "this Nightly's title doesn't name its date and commit as Horizon signs them, so it can't be checked: not installed"
      TAG=$NAME ;;
  esac
  # A release asset's download address, from its block in the release's JSON.
  asset_url() {
    printf '%s\n' "$JSON" | awk -v asset="$1" '
      /"name": / { line = $0; sub(/.*"name": "/, "", line); sub(/".*/, "", line); current = line }
      current == asset && /"browser_download_url": / { line = $0; sub(/.*"browser_download_url": "/, "", line); sub(/".*/, "", line); print line; exit }'
  }
  # The first asset the release has: its download address and digest, from its block in the release's JSON.
  for ASSET in $ASSETS; do
    FIELDS=$(printf '%s\n' "$JSON" | awk -v asset="$ASSET" '
      /"name": / { line = $0; sub(/.*"name": "/, "", line); sub(/".*/, "", line); current = line }
      current == asset && /"digest": / { line = $0; sub(/.*"digest": "sha256:/, "", line); sub(/".*/, "", line); digest = line }
      current == asset && /"browser_download_url": / { line = $0; sub(/.*"browser_download_url": "/, "", line); sub(/".*/, "", line); url = line }
      END { print url; print digest }')
    URL=$(printf '%s\n' "$FIELDS" | sed -n 1p)
    DIGEST=$(printf '%s\n' "$FIELDS" | sed -n 2p)
    [ -z "$URL" ] || break
  done
  [ -n "$URL" ] || die "release $TAG has none of $ASSETS"

  # The signature (docs/release-flow.md, "Signed checksums"): SHA256SUMS.sig holds one base64 Ed25519 signature per
  # line over "horizon release <tag>\n" + SHA256SUMS. A Nightly is checked against SHA256SUMS.v2.sig instead, signed
  # for its own version (nightly-<date>-<commit>, from its title), never the channel's SHA256SUMS.sig: an older
  # Nightly can't be passed off as the current one (docs/security.md, "Nightly signatures").
  # It needs OpenSSL 3 (LibreSSL, which macOS ships, and OpenSSL 1.x cannot verify Ed25519 this way); without it
  # the install says so and keeps the GitHub SHA-256 check below.
  EXPECT=$DIGEST
  if [ -n "$NAME" ]; then SIGTAG=$NAME; SIGFILE=SHA256SUMS.v2.sig; else SIGTAG=$RAWTAG; SIGFILE=SHA256SUMS.sig; fi
  if ! command -v openssl >/dev/null; then
    echo "Note: OpenSSL isn't installed, so the release signature was not checked; checking GitHub's SHA-256 only."
  elif ! openssl version | grep -Eq '^OpenSSL ([3-9]|[1-9][0-9])\.'; then
    echo "Note: $(openssl version | cut -d' ' -f1-2) can't check Ed25519 signatures (OpenSSL 3 is needed), so the release signature was not checked; checking GitHub's SHA-256 only."
  else
    SUMS_URL=$(asset_url SHA256SUMS)
    SIG_URL=$(asset_url "$SIGFILE")
    if [ -n "$NAME" ] && { [ -z "$SUMS_URL" ] || [ -z "$SIG_URL" ]; }; then
      die "$TAG has no $SIGFILE (its signature for this exact build): it was published before Nightlies were signed for their own version, so it can't be checked. Install the current Nightly (--version latest) instead."
    elif [ -z "$SUMS_URL" ] || [ -z "$SIG_URL" ]; then
      echo "Note: release $TAG has no signed checksums (it was published before signing began), so the signature was not checked; checking GitHub's SHA-256 only."
    else
      curl -fsSL -o "$TMP/SHA256SUMS" "$SUMS_URL" || die "could not download SHA256SUMS"
      curl -fsSL -o "$TMP/SHA256SUMS.sig" "$SIG_URL" || die "could not download $SIGFILE"
      { printf 'horizon release %s\n' "$SIGTAG"; cat "$TMP/SHA256SUMS"; } > "$TMP/message"
      SIGNED=
      for KEY in $RELEASE_KEYS; do
        # The raw 32-byte key in an Ed25519 SubjectPublicKeyInfo, the form OpenSSL reads.
        { printf '\060\052\060\005\006\003\053\145\160\003\041\000'; printf '%s' "$KEY" | openssl base64 -d -A; } > "$TMP/key.der"
        while IFS= read -r LINE || [ -n "$LINE" ]; do
          [ -n "$LINE" ] || continue
          printf '%s' "$LINE" | openssl base64 -d -A > "$TMP/signature" 2>/dev/null || continue
          if openssl pkeyutl -verify -rawin -pubin -keyform DER -inkey "$TMP/key.der" -in "$TMP/message" -sigfile "$TMP/signature" >/dev/null 2>&1; then SIGNED=yes; fi
        done < "$TMP/SHA256SUMS.sig"
      done
      [ -n "$SIGNED" ] || die "$TAG's SHA256SUMS signature doesn't match Horizon's release key: not installed. The download may have been tampered with; please report it on Discord or GitHub."
      EXPECT=$(awk -v asset="$ASSET" '$2 == asset || $2 == "*" asset { print $1; exit }' "$TMP/SHA256SUMS")
      [ -n "$EXPECT" ] || die "the signed SHA256SUMS doesn't list $ASSET: not installed"
      echo "Release signature OK (Horizon release key)."
    fi
  fi
  [ -n "$EXPECT" ] || die "release $TAG publishes no SHA-256 for $ASSET"

  echo "Downloading Horizon $TAG ($ASSET)..."
  curl -fL --progress-bar -o "$TMP/$ASSET" "$URL"
  GOT=$($SHA "$TMP/$ASSET" | cut -d' ' -f1)
  [ "$GOT" = "$EXPECT" ] || die "$ASSET doesn't match its SHA-256 (got $GOT, expected $EXPECT): not installed"
fi

mkdir -p "$TMP/x"
case "$ASSET" in
  *.tar.gz) tar -xzf "$TMP/$ASSET" -C "$TMP/x" ;;
  *.zip) command -v unzip >/dev/null || die "unzip is needed"; unzip -q "$TMP/$ASSET" -d "$TMP/x" ;;
esac
SOURCE=$(dirname "$(find "$TMP/x" -name horizon-server -type f | head -n 1)")
[ -x "$SOURCE/horizon-server" ] || die "$ASSET holds no horizon-server (a release from before it existed?)"

RELEASE="$DIR/releases/$TAG"
mkdir -p "$DIR/releases" "$DIR/game-files/horizon-client" "$BIN"
rm -rf "$RELEASE"
mv "$SOURCE" "$RELEASE"
# Prepared game files live outside the release, so updates and rollbacks keep them.
mkdir -p "$RELEASE/.local"
ln -sfn "$DIR/game-files/horizon-client" "$RELEASE/.local/horizon-client"
ln -sfn "$RELEASE" "$DIR/current"
# Small launchers rather than links: the programs find their release folder from where they run.
for tool in horizon-server horizon-prepare; do
  printf '#!/bin/sh\nexec "%s/current/%s" "$@"\n' "$DIR" "$tool" > "$BIN/$tool"
  chmod +x "$BIN/$tool"
done

echo "Installed Horizon $TAG in $RELEASE; horizon-server and horizon-prepare are in $BIN."
# The full game (releases before the server build) links ALSA and udev; minimal Linux images often lack them.
if [ "$(uname -s)" = Linux ] && [ ! -e "$RELEASE/horizon-engine" ] && command -v ldd >/dev/null; then
  MISSING=$(ldd "$RELEASE/horizon-client" 2>/dev/null | awk '/not found/ { printf "%s ", $1 }')
  if [ -n "$MISSING" ]; then
    echo "The game needs libraries this system doesn't have: $MISSING"
    echo "Install them before starting a server:"
    echo "  Debian, Ubuntu: sudo apt install libasound2 libudev1   (Ubuntu 24.04 and later: libasound2t64)"
    echo "  Fedora, RHEL: sudo dnf install alsa-lib systemd-libs"
    echo "  Arch: sudo pacman -S alsa-lib systemd-libs"
    echo "  NixOS: enable programs.nix-ld with alsa-lib and udev in its libraries"
  fi
fi
case ":$PATH:" in
  *":$BIN:"*) ;;
  *) echo "Add $BIN to your PATH, for example: echo 'export PATH=\"$BIN:\$PATH\"' >> ~/.profile" ;;
esac
if [ -z "$(ls -A "$DIR/game-files/horizon-client" 2>/dev/null)" ]; then
  echo "Next, prepare the game files from your own Minecraft 26.3 (it needs Java 25 and the game installed once"
  echo "by the official launcher; see docs/server.md):"
  echo "  horizon-prepare --prepare --server-jar /path/to/server.jar"
fi
echo "Then set up a server: mkdir my-server && cd my-server && horizon-server init"
