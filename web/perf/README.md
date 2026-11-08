# The performance web UI

`web/perf/` is the browser UI for `.hzperf` captures (docs/perf-platform.md §7). It is a Next.js
App Router app, self-hosted at `https://horizon-perf.umceko.com`, and it is built to be copied into
playhorizon.dev later: the UI lives in `app/perf/`.

The page reads a capture in the browser. `cargo xtask perf-wasm` builds `horizon-perf` for
`wasm32-unknown-unknown` with the `wasm` feature and writes `target/perf-wasm/{horizon_perf.wasm,horizon_perf.js}`;
the app's build copies those in. Every number on screen comes from a wasm call, so the UI and
`horizon-perf` cannot disagree.

## Running it

```sh
cargo xtask perf-wasm            # once, and after every change under crates/horizon-perf*/
cd web/perf
npm ci
npm run dev                      # http://localhost:3000/perf (the root redirects there)
```

If `cargo xtask` is unknown, the repository's `.cargo/config.toml` alias is not being read: run
it as `cargo run -p xtask -- perf-wasm` instead. The dev server needs the wasm to exist before it
starts, because `npm run dev` copies it in first.

`PERF_WASM_DIR` points the copy at another folder when the wasm was built elsewhere.

`npm run build` produces the deployable bundle under `.next/standalone`; `npm start` serves the
build. `npm run typecheck` runs TypeScript over the app and `npm test` its unit tests.

## What is here

| Path | Is |
|---|---|
| `app/perf/page.tsx` | the opening screen; `capture-view.tsx` holds the tabs of an open capture |
| `app/perf/health-panel.tsx` | the health bar and timelines (UI1) |
| `app/perf/metadata-panel.tsx` | platform, OS, CPU, build, flags, settings, world, game rules, mods, memory and network (UI1) |
| `app/perf/summary-panel.tsx` | `horizon-perf summary`'s text, verbatim |
| `app/perf/raw-panel.tsx` | the W1 skeleton's raw bridge: a query request and the wasm's reply |
| `lib/generated/horizon_perf.js` | the JS wrapper, copied from `crates/horizon-perf/web/`; not checked in |
| `lib/i18n.ts`, `lang/en_us.json` | translation-ready strings; add a language by adding a file and `setMessages` |
| `public/perf/horizon_perf.wasm` | the wasm, copied in; not checked in |
| `scripts/copy-wasm.mjs` | what `npm run wasm`, `dev`, `build` and `typecheck` run first |

The ABI (`hp_open`, `hp_query`, `hp_summary`, `hp_diff`, `hp_export`, `hp_close`, and the reply
buffer format) is documented at the top of `crates/horizon-perf/src/wasm.rs`. The wasm returns data,
ids and stable error codes; English sentences exist only in `hp_summary`.

## Checking the numbers

Open the **Query** tab and run the request shown there; its reply is the same JSON
`horizon-perf query` prints for the same capture and filters. For example, the health bar's series
are `query series h:tps h:mspt_avg …`, and the metadata panel reads `query meta`, `query world`,
`query net` and `query memory`.

The wasm side has an automated agreement test:

```sh
cargo test -p horizon-perf --features wasm --test wasm
```

It opens a fixture through the C ABI and asserts byte-identical replies to the CLI's `query`,
`summary` and `export`.

## Privacy and CSP

The page never uploads a capture. `next.config.mjs` sends a CSP whose `connect-src` is `'self'`
only, so the page cannot send anything to another origin.

## Deployment

`.github/workflows/perf-web.yml` builds the wasm, runs the agreement test, builds the app and uploads
`.next/standalone` as the `perf-web` artifact on every change to the crates it uses or this folder.
