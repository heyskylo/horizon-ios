#!/bin/bash
# PURPOSE: proves packaging/server/install.sh checks the release signature (docs/release-flow.md, "Signed checksums").
# WHY: the first install used to trust only the SHA-256 GitHub publishes (docs/security.md).
# REFS: tools/ci/sign_release.sh (signs the fake releases with a throwaway key), crates/horizon-release-sign.
# LOGS: prints one ok/FAIL line per case; exit 1 when any case fails.
# READS-BY: tests/test_install_sh_signature.py (unittest wrapper), by hand: bash tests/install_sh_signature.sh
# ENDS-WHEN: it ends by itself; leaves nothing outside its own mktemp folder.
# Needs OpenSSL 3 (to sign the throwaway release); skips (exit 77) without it. curl is replaced by a fake that
# serves local files, so nothing touches the network.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
openssl version | grep -Eq '^OpenSSL [3-9]\.' || { echo "skip: OpenSSL 3 needed"; exit 77; }
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
fails=0
check() {  # name, expected exit (0 or 1), output grep (may be empty), then the case's exit and output files
  local name=$1 want=$2 pat=$3 rc=$4 out=$5
  local exit_ok=no
  if [ "$want" = 0 ]; then [ "$rc" = 0 ] && exit_ok=yes; else [ "$rc" != 0 ] && exit_ok=yes; fi
  if [ "$exit_ok" = yes ] && { [ -z "$pat" ] || grep -q -- "$pat" "$out"; }; then
    echo "ok   $name"
  else echo "FAIL $name (exit $rc, wanted $want, pattern '$pat')"; sed 's/^/     /' "$out"; fails=$((fails+1)); fi
}

# A fake release: an archive holding horizon-server, signed for TAG with a throwaway key.
make_release() {  # dir tag
  local d=$1 tag=$2
  mkdir -p "$d/src/horizon-server-x" "$d/files"
  printf '#!/bin/sh\necho fake\n' > "$d/src/horizon-server-x/horizon-server"; chmod +x "$d/src/horizon-server-x/horizon-server"
  tar -czf "$d/files/horizon-server-linux-x86_64.tar.gz" -C "$d/src" horizon-server-x
  EPHEMERAL=yes bash "$root/tools/ci/sign_release.sh" "$tag" "$d/files/horizon-server-linux-x86_64.tar.gz" > /dev/null
  json "$d" "$tag" yes
}
json() {  # dir tag with-sums: GitHub's release JSON, as install.sh reads it
  local d=$1 tag=$2 sums=$3 sha
  sha=$(sha256sum "$d/files/horizon-server-linux-x86_64.tar.gz" | cut -d' ' -f1)
  {
    printf '{\n  "tag_name": "%s",\n  "name": "%s",\n  "assets": [\n' "$tag" "$tag"
    printf '    {\n      "name": "horizon-server-linux-x86_64.tar.gz",\n      "digest": "sha256:%s",\n      "browser_download_url": "https://dl.test/horizon-server-linux-x86_64.tar.gz"\n    }' "$sha"
    if [ "$sums" = yes ]; then for f in SHA256SUMS SHA256SUMS.sig; do
      printf ',\n    {\n      "name": "%s",\n      "browser_download_url": "https://dl.test/%s"\n    }' "$f" "$f"; done; fi
    printf '\n  ]\n}\n'
  } > "$d/release.json"
}
# Tools install.sh may use, without openssl (the "no OpenSSL" case).
mkdir "$W/nossl"
for t in sh bash cat awk sed grep cut head find mkdir mktemp rm mv ln chmod cp basename dirname tar gzip sha256sum ls uname printf; do
  p=$(command -v "$t") && ln -s "$p" "$W/nossl/$t"
done
# The fake curl: the API address answers with release.json, any other address with the file of that name.
mkdir "$W/fake"
cat > "$W/fake/curl" <<'CURL'
#!/bin/sh
out=; url=
while [ $# -gt 0 ]; do case "$1" in -o) out=$2; shift 2 ;; -*) shift ;; *) url=$1; shift ;; esac; done
case "$url" in
  https://api.github.com/*) src=$REL/release.json ;;
  *) src=$REL/files/$(basename "$url") ;;
esac
[ -f "$src" ] || exit 22
if [ -n "$out" ]; then cp "$src" "$out"; else cat "$src"; fi
CURL
chmod +x "$W/fake/curl"

# install.sh with the throwaway public key in place of the release key.
with_key() {  # dir -> prints path of the patched copy
  local d=$1
  sed "s|^RELEASE_KEYS=.*|RELEASE_KEYS=\"$(cat "$d/files/ephemeral.pub")\"|" "$root/packaging/server/install.sh" > "$d/install.sh"
  grep -q "$(cat "$d/files/ephemeral.pub")" "$d/install.sh" || { echo "FAIL could not set the test key"; exit 1; }
}
run() {  # dir, path-dir, ...install args: runs the patched install.sh with fake curl
  local d=$1 p=$2; shift 2
  REL=$d PATH="$W/fake:$p" HOME="$W/home" sh "$d/install.sh" --dir "$d/inst" --bin "$d/bin" "$@" > "$d/out" 2>&1
  echo $? > "$d/rc"
}
case_run() { run "$@"; }
verdict() { check "$1" "$2" "$3" "$(cat "$4/rc")" "$4/out"; }
# sed -i differs between GNU and BSD (macOS) sed: edit through a copy instead.
edit() {  # sed-expression file
  sed "$1" "$2" > "$2.tmp" && mv "$2.tmp" "$2"
}
mkdir -p "$W/home"
P=/usr/bin:/bin

# 1. Good signature, a Nightly (signed for the channel "nightly" under the tag nightly-2026-11-03).
d=$W/good; make_release "$d" nightly; with_key "$d"
edit 's/"tag_name": "nightly"/"tag_name": "nightly-2026-11-03"/' "$d/release.json"
run "$d" "$P"; verdict "good signature installs (nightly channel)" 0 "Release signature OK" "$d"
[ -x "$d/inst/current/horizon-server" ] || { echo "FAIL nothing installed"; fails=$((fails+1)); }

# 2. Good signature, a normal tag.
d=$W/tag; make_release "$d" v1.2.3; with_key "$d"
run "$d" "$P" --version v1.2.3; verdict "good signature installs (release tag)" 0 "Release signature OK" "$d"

# 3. Tampered archive: GitHub's digest is made to agree with the swapped archive, only SHA256SUMS catches it.
d=$W/arc; make_release "$d" v1.2.3; with_key "$d"
printf 'x' >> "$d/files/horizon-server-linux-x86_64.tar.gz"; json "$d" v1.2.3 yes
run "$d" "$P" --version v1.2.3; verdict "tampered archive refused" 1 "doesn't match its SHA-256" "$d"
[ ! -e "$d/inst/current" ] || { echo "FAIL tampered archive was installed"; fails=$((fails+1)); }

# 4. Tampered SHA256SUMS (the archive's line swapped for the hash of the tampered archive).
d=$W/sums; make_release "$d" v1.2.3; with_key "$d"
printf 'x' >> "$d/files/horizon-server-linux-x86_64.tar.gz"
new=$(sha256sum "$d/files/horizon-server-linux-x86_64.tar.gz" | cut -d' ' -f1)
printf '%s  horizon-server-linux-x86_64.tar.gz\n' "$new" > "$d/files/SHA256SUMS"; json "$d" v1.2.3 yes
run "$d" "$P" --version v1.2.3; verdict "tampered SHA256SUMS refused" 1 "signature doesn't match" "$d"

# 5. A signature made for another tag does not pass (a release's list replayed under another tag).
d=$W/replay; make_release "$d" v1.2.3; with_key "$d"
edit 's/"tag_name": "v1.2.3"/"tag_name": "v9.9.9"/' "$d/release.json"
run "$d" "$P" --version v9.9.9; verdict "signature replayed under another tag refused" 1 "signature doesn't match" "$d"

# 6. Signed by a key that is not the release key.
d=$W/wrong; make_release "$d" v1.2.3; with_key "$d"
edit 's|^RELEASE_KEYS=.*|RELEASE_KEYS="MU4oiIQcCxkZvbwSjjXs7SySCuM6ywRQ3nGaJYZG0gw="|' "$d/install.sh"
run "$d" "$P" --version v1.2.3; verdict "wrong key refused" 1 "signature doesn't match" "$d"

# 7. No OpenSSL: says so, keeps the SHA-256 check.
d=$W/none; make_release "$d" v1.2.3; with_key "$d"
run "$d" "$W/nossl" --version v1.2.3; verdict "no OpenSSL: installs, says it was not checked" 0 "OpenSSL isn't installed" "$d"
printf 'x' >> "$d/files/horizon-server-linux-x86_64.tar.gz"
d2=$W/none2; cp -r "$d" "$d2"; rm -rf "$d2/inst" "$d2/bin"; edit 's/"digest": "sha256:[0-9a-f]*"/"digest": "sha256:0000"/' "$d2/release.json"
run "$d2" "$W/nossl" --version v1.2.3; verdict "no OpenSSL: a wrong GitHub SHA-256 still refused" 1 "doesn't match its SHA-256" "$d2"

# 8. Old OpenSSL and LibreSSL (macOS): says so, keeps the SHA-256 check.
for v in "OpenSSL 1.1.1w  11 Sep 2023" "LibreSSL 3.3.6"; do
  d=$W/old; rm -rf "$d"; make_release "$d" v1.2.3; with_key "$d"
  mkdir -p "$W/old-bin"; rm -f "$W/old-bin/openssl"
  printf '#!/bin/sh\necho "%s"\n' "$v" > "$W/old-bin/openssl"; chmod +x "$W/old-bin/openssl"
  REL=$d PATH="$W/old-bin:$W/fake:$W/nossl" HOME="$W/home" sh "$d/install.sh" --dir "$d/inst" --bin "$d/bin" --version v1.2.3 > "$d/out" 2>&1; echo $? > "$d/rc"
  verdict "${v%% *}: installs, says it can't check signatures" 0 "can't check Ed25519" "$d"
done

# 9. A release published before signing (no SHA256SUMS): says so, keeps the SHA-256 check.
d=$W/legacy; make_release "$d" v1.2.3; with_key "$d"; json "$d" v1.2.3 no
run "$d" "$P" --version v1.2.3; verdict "unsigned release: installs, says so" 0 "no signed checksums" "$d"

echo "$fails case(s) failed"; [ "$fails" = 0 ]
