# horizon-mod-registry

The service behind the registry's submission form (`playhorizon.dev/mods/submit`). A creator signs in with GitHub and
sends a file with its page; the service runs the file's own checks (`horizon_mods::index::inspect`, the signature, the
entry's fields), uploads it to the `staging` release of
[horizon-rust/horizon-mods](https://github.com/horizon-rust/horizon-mods) as `<sha256>.<extension>`, and opens a pull
request adding or updating `mods/<id>.toml`. That repository's CI checks it again, with a trial run of a mod in the
sandbox; reviewers decide, and merging publishes. Formats and rules: [docs/api/mod-index.md](../../docs/api/mod-index.md).

Signed-in players also rate published projects, 1 to 5 stars: one rating per GitHub account and project, which they can
change, never their own project's. That is the only data it keeps, in one SQLite file (`REGISTRY_DB`); it only ever
shows totals, never who rated. A session is a signed cookie holding the GitHub login and id, and the user's OAuth token
is used once, to read them, and dropped. The sign-in asks for no scope.

| Route | |
| --- | --- |
| `GET /api/mods/auth/login` | Starts the GitHub sign-in; `?back=/mods/<id>` (or `/mods`) comes back to that page |
| `GET /api/mods/auth/callback` | GitHub's redirect: sets the session, back to `/mods/submit` or the `back` page |
| `POST /api/mods/auth/logout` | Ends it |
| `GET /api/mods/me` | `{login, csrf}`, or 401 |
| `POST /api/mods/submit` | Multipart: `entry` (JSON: `id kind name summary description authors license source icon key signature version game changelog`) and `file`; `{pull_request}` or `{error}` |
| `GET /api/mods/ratings` | `{projects: {<id>: {average, count}}}` for every rated project; cached a minute |
| `GET /api/mods/<id>/rating` | The signed-in account's own `{stars}` (null when it hasn't rated), or 401 |
| `POST /api/mods/<id>/rating` | JSON `{stars}`, 1 to 5: sets or changes it; `{stars, average, count}` or `{error}` |
| `GET /healthz` | `ok` |

A submission or a rating must come from `REGISTRY_PUBLIC_URL`'s origin with the session's `X-CSRF-Token`. Six
submissions per account and sixty in all per hour; thirty ratings per account and 600 in all. A project's owners can't
rate it, and only projects on the registry's `main` can be rated. Only an entry's `owners` publish its updates; a key change, an icon in the repository or anything else
is a pull request by hand.

## Running it

```sh
cargo test                     # against a mock GitHub (tests/submit.rs)
REGISTRY_PUBLIC_URL=http://localhost:4173 REGISTRY_SESSION_KEY=... GITHUB_CLIENT_ID=... GITHUB_CLIENT_SECRET=... \
  GITHUB_BOT_TOKEN=... REGISTRY_REPO=you/horizon-mods-test REGISTRY_DB=/tmp/registry.sqlite3 cargo run
```

| Variable | |
| --- | --- |
| `REGISTRY_PUBLIC_URL` | The site, `https://playhorizon.dev`: the OAuth callback is `<it>/api/mods/auth/callback` |
| `REGISTRY_SESSION_KEY` | 32 characters or more, random: signs sessions |
| `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` | The OAuth app |
| `GITHUB_BOT_TOKEN` | Fine-grained, the registry repository only: Contents and Pull requests read and write |
| `REGISTRY_REPO` | Default `horizon-rust/horizon-mods` |
| `REGISTRY_DB` | The ratings' SQLite file: default `/data/registry.sqlite3`, the volume |
| `PORT` | Default 8080 |

On Railway it is the service `horizon-mod-registry` of the project `horizon-site`, next to the website: built from
`main` with this `Dockerfile` from the repository root, health check `/healthz`, no public domain, and the volume
`registry-data` at `/data` for the ratings. The
website's Caddy sends `/api/mods/*` to it over Railway's private network (`REGISTRY_UPSTREAM` on the website service:
`http://horizon-mod-registry.railway.internal:8080`). `railway.json` lists the same settings; Railway reads them from
the service, not from the file.
