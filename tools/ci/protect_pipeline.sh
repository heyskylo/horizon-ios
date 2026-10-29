#!/usr/bin/env bash
# Makes main's branch protection require the code owners' review (docs/security.md): a pull request that
# changes a file .github/CODEOWNERS lists waits for Ludovic's approval; every other one still merges as soon as
# `verify` passes (no approvals are required otherwise). Approvals are dismissed by a later push. Everything
# else in the protection (the required `verify` from GitHub Actions, admins included) is kept as it is.
#
# Ludovic is the only code owner, and GitHub never counts an author's approval of their own pull request, so his
# own pull requests to listed files could never merge. He is therefore the one account allowed to bypass the
# pull-request rules (bypass_pull_request_allowances). The alternative, not enforcing the protection on admins,
# would also let an admin merge without `verify`; here `verify` stays required for everyone, admins and Ludovic
# included (enforce_admins stays on), and only the review requirement has an exception, for him alone.
#
# Apply it only once agents open, push and merge pull requests as the horizon-agent GitHub App, not as ludovic111:
# while agents use his token, his bypass is theirs too, and their changes to listed files would skip his review.
#
#   tools/ci/protect_pipeline.sh            # print the protection it would set, change nothing
#   tools/ci/protect_pipeline.sh --apply    # set it (needs the maintainer's admin login)
set -euo pipefail
repo=${GITHUB_REPOSITORY:-horizon-rust/horizon}
api="repos/$repo/branches/main/protection"
body=$(gh api "$api" | jq '{
  required_status_checks: (.required_status_checks
    | {strict, checks: [.checks[] | {context, app_id}]}),
  enforce_admins: .enforce_admins.enabled,
  required_pull_request_reviews: {
    require_code_owner_reviews: true,
    required_approving_review_count: 0,
    dismiss_stale_reviews: true,
    require_last_push_approval: false,
    bypass_pull_request_allowances: {users: ["ludovic111"], teams: [], apps: []}
  },
  restrictions: null,
  required_linear_history: .required_linear_history.enabled,
  allow_force_pushes: .allow_force_pushes.enabled,
  allow_deletions: .allow_deletions.enabled,
  required_conversation_resolution: .required_conversation_resolution.enabled,
  lock_branch: .lock_branch.enabled,
  allow_fork_syncing: .allow_fork_syncing.enabled
}')
echo "$body"
if [[ "${1:-}" == --apply ]]; then
  gh api -X PUT "$api" --input - <<< "$body" > /dev/null
  set_now=$(gh api "$api" --jq '[.required_pull_request_reviews.require_code_owner_reviews,
    .enforce_admins.enabled, ([.required_pull_request_reviews.bypass_pull_request_allowances.users[]?.login]
    | index("ludovic111") != null), ([.required_status_checks.checks[].context] | index("verify") != null)]
    | map(tostring) | join(" ")')
  if [[ "$set_now" != "true true true true" ]]; then
    echo "GitHub didn't keep everything (code owners, admins enforced, Ludovic's bypass, verify): $set_now" >&2
    exit 1
  fi
  echo "main now requires code owners' reviews; Ludovic may bypass them; verify is required for everyone."
else
  echo "(not applied: run with --apply)"
fi
