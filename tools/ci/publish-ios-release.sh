#!/usr/bin/env bash
# Called only by the trusted main-branch release job, after auditing its artifact.
set -euo pipefail
: "${GH_REPO:?}" "${GITHUB_SHA:?}" "${IPA:?}" "${CHECKSUM:?}" "${VERSION:?}" "${BUILD:?}"
tag="ios-build-${BUILD}"
ipa_name=$(basename "$IPA")
checksum_name=$(basename "$CHECKSUM")
# A pre-existing tag must not redirect this build to another source revision.
gh api "repos/$GH_REPO/git/matching-refs/tags/$tag" --jq '.[].ref' > release-tag-refs.txt
if grep -Fx -- "refs/tags/$tag" release-tag-refs.txt >/dev/null; then
  target=$(gh api "repos/$GH_REPO/commits/$tag" --jq .sha)
  if [[ "$target" != "$GITHUB_SHA" ]]; then
    echo '::error::Release tag points to a different commit; refusing to modify it.'
    exit 1
  fi
fi
# Keep notes/title current even when a prior attempt already published this tag.
{
  printf '%s\n\n' '🤖 Agent: Automated unsigned iOS build from GitHub Actions.'
  printf 'Horizon %s, build %s, from commit %s.\n\n' "$VERSION" "$BUILD" "$GITHUB_SHA"
  printf '%s\n\n' 'Download the unsigned IPA below and sign it with your own signing service or certificate before installing it on an iPhone or iPad. No signing certificate, provisioning profile, account data, or prepared Minecraft assets are included.'
  printf '%s\n' 'The adjacent .sha256 file verifies the downloaded IPA.'
} > release-notes.md
upload=true
: > obsolete-assets.txt
if gh release view "$tag" --json isDraft,targetCommitish,assets > release-state.json; then
  target=$(python3 -c 'import json; print(json.load(open("release-state.json"))["targetCommitish"])')
  if [[ "$target" != "$GITHUB_SHA" ]]; then
    echo '::error::Release tag points to a different commit; refusing to modify it.'
    exit 1
  fi
  python3 -c 'import json; print("\n".join(a["name"] for a in json.load(open("release-state.json"))["assets"]))' > release-assets.txt
  if [[ "$(python3 -c 'import json; print(str(json.load(open("release-state.json"))["isDraft"]).lower())')" != true ]] &&
      grep -Fx -- "$ipa_name" release-assets.txt >/dev/null &&
      grep -Fx -- "$checksum_name" release-assets.txt >/dev/null; then
    upload=false
  fi
  # Remove only obsolete Horizon IPA/checksum names for this exact build after
  # uploading its corrected pair. Unrelated attachments are never touched.
  python3 - "$ipa_name" "$checksum_name" "$BUILD" <<'PYASSETS' > obsolete-assets.txt
import json, re, sys
keep = set(sys.argv[1:3])
pattern = re.compile(r"Horizon-iOS-[0-9]+(?:\.[0-9]+){0,2}-build" + re.escape(sys.argv[3]) + r"-unsigned\.ipa(?:\.sha256)?")
for asset in json.load(open("release-state.json"))["assets"]:
    name = asset["name"]
    if name not in keep and pattern.fullmatch(name):
        print(name)
PYASSETS
else
  gh release create "$tag" --target "$GITHUB_SHA" --draft \
    --title "Horizon iOS $VERSION — build $BUILD" --notes-file release-notes.md
fi
if [[ "$upload" == true ]]; then
  gh release upload "$tag" "$IPA" "$CHECKSUM" --clobber
fi
while IFS= read -r obsolete; do
  [[ -z "$obsolete" ]] || gh release delete-asset "$tag" "$obsolete" --yes
done < obsolete-assets.txt
# An older run retried after a newer release must not take over Latest.
latest=true
if latest_tag=$(gh api "repos/$GH_REPO/releases/latest" --jq .tag_name 2>/dev/null); then
  latest=$(python3 - "$latest_tag" "$BUILD" <<'PYLATEST'
import re, sys
match = re.fullmatch(r"ios-build-([0-9]+)\.([0-9]+)", sys.argv[1])
current = tuple(map(int, sys.argv[2].split(".")))
print(str(not match or current >= tuple(map(int, match.groups()))).lower())
PYLATEST
  )
fi
gh release edit "$tag" --title "Horizon iOS $VERSION — build $BUILD" \
  --notes-file release-notes.md --draft=false --prerelease=false --latest="$latest"
