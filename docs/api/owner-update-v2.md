# What the registry's `owner-update` workflow must check (trailer v2)

The registry repository (horizon-rust/horizon-mods) merges an owner's update on its own when the pull request carries
an `owner-update` trailer that the submission service (`crates/horizon-mod-registry`, `update_trailer`) signed with
`REGISTRY_UPDATE_KEY`. Its workflow and `scripts/owner_update.py` live in that repository, not here. Since owner IDs,
the trailer is v2, and the workflow must check all of the following before it merges. Any check that fails leaves the
pull request to a reviewer (remove the label, comment why); it never merges partly.

## The trailer

The last HTML comment of the pull request body:

```text
<!-- owner-update
format: v2
id: <project id>
version: <version>
sha256: <hex>
file: <id>-<version>.<extension>
submitter: <login>
submitter_id: <numeric GitHub ID>
hmac: <hex>
-->
```

`hmac` is HMAC-SHA256 with `REGISTRY_UPDATE_KEY`, lowercase hex, over exactly these bytes (`\n` between fields, no
trailing newline):

```text
owner-update-v2\n<id>\n<version>\n<sha256>\n<file>\n<submitter>\n<submitter_id>
```

## The checks

1. **Format.** `format: v2` is present and every field above is there once. Refuse `v1` trailers (no `format` line)
   once the service is deployed: a v1 trailer names the submitter by login only.
2. **HMAC.** Recompute it over the v2 message and compare in constant time (`hmac.compare_digest`).
3. **The submitter is the pull request's.** `submitter_id` equals the numeric ID in the pull request head commit's
   `Co-authored-by: <login> <<id>+<login>@users.noreply.github.com>` line, which the service writes. Compare the IDs,
   not the logins.
4. **The submitter owns the project, by ID.** Read `mods/<id>.toml` **on main** (not the pull request's copy):
   `submitter_id` must be in its `owner_ids`. An entry without `owner_ids` never merges on its own (the service
   doesn't sign those, so a trailer on one is forged or stale).
5. **Nothing else changes.** The pull request touches only `mods/<id>.toml`. Its new entry equals main's except
   for one added `[[versions]]` entry, first, whose `version`, `sha256` and `file` are the trailer's. `owners`,
   `owner_ids`, `key`, `kind`, `repo` and the page fields are identical (the service already only signs such updates;
   check again, since the pull request branch can be pushed to after it opened).
6. **The file.** `staging` holds `<sha256>.<extension>` and its SHA-256 is `sha256`; the usual CI checks
   (`cargo horizon-mod registry check`, the trial run, a signature when the entry has a `key`) pass at the head
   that is merged.
7. **The head merged is the head checked.** Merge with the head SHA the checks ran on (`gh pr merge --match-head-commit
   <sha>`), so a push between check and merge can't slip in.

## Today

The registry repository has no `owner-update` workflow yet (checked at horizon-mods `e53ad82`): every owner update
waits for a reviewer, whatever its trailer says. Nothing merges on its own until a workflow doing every check above is
written there. Until then the v2 trailer and `owner_ids` only make the review's notes and the service's refusals
decide by ID.

## Order of deployment

1. In the registry repository: update `scripts/owner_update.py` and the workflow to the checks above, accepting v2
   only; deploy the CI's `cargo horizon-mod` from a Horizon commit that knows `owner_ids` (older ones refuse the field).
2. Once a maintainer has read `tools/registry_backfill_owner_ids.py`, run it on a checkout (dry run, then `--write`)
   and merge its pull request by hand.
3. Deploy the submission service with this change. Until step 1 is live, its v2 trailers fail verification and the
   updates simply wait for a reviewer.
