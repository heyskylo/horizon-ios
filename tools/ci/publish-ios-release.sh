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
if gh release view "$tag" --json isDraft,targetCommitish,assets > release-state.json; then
  target=$(python3 -c 'import json; print(json.load(open("release-state.json"))["targetCommitish"])')
  if [[ "$target" != "$GITHUB_SHA" ]]; then
    echo '::error::Release tag points to a different commit; refusing to modify it.'
    exit 1
  fi
  if [[ "$(python3 -c 'import json; print(str(json.load(open("release-state.json"))["isDraft"]).lower())')" != true ]]; then
    python3 -c 'import json; print("\n".join(a["name"] for a in json.load(open("release-state.json"))["assets"]))' > release-assets.txt
    grep -Fx -- "$ipa_name" release-assets.txt >/dev/null
    grep -Fx -- "$checksum_name" release-assets.txt >/dev/null
    echo 'This build is already published; leaving its assets unchanged.'
    exit 0
  fi
else
  cat > release-notes.md <<EOF
🤖 Agent: Automated unsigned iOS build from GitHub Actions.

Horizon $VERSION, build $BUILD, from commit $GITHUB_SHA.

Download the unsigned IPA below and sign it with your own signing service or certificate before installing it on an iPhone or iPad. No signing certificate, provisioning profile, account data, or prepared Minecraft assets are included.

The adjacent .sha256 file verifies the downloaded IPA. This is a prerelease build.
EOF
  gh release create "$tag" --target "$GITHUB_SHA" --draft --prerelease \
    --title "Horizon iOS $VERSION — build $BUILD" --notes-file release-notes.md
fi
# Resume partial draft uploads without replacing an already published build.
gh release upload "$tag" "$IPA" "$CHECKSUM" --clobber
# Keep prereleases out of the stable 'latest' channel.
gh release edit "$tag" --draft=false --prerelease --latest=false
