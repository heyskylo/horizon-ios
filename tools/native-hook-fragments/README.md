# Native hook API fragments

A hook PR adds a uniquely named JSON file here instead of editing the API number,
the API histories or `native-hooks.json`. Helper-only additions use an empty
`hooks` array and list new Host helpers in `helpers` as catalog entries. Example:

```json
{
  "hooks": [
    {"name": "example", "target": "horizon_client::game::native_hooks::Game<'_>::native_example"}
  ],
  "rust_summary": "an example hook (`native_example`).",
  "doc_summary": "an example hook (`native_example`). Bundles need \"hooks\": @API:example@.",
  "comments": ["crates/horizon-client/src/game/example.rs"]
}
```

A hook may also carry the catalog's optional `neoforge`, `side` and
`cancellable` keys; the tool copies them into `native-hooks.json`.

Name this file `example.json`. The tool replaces `@API:example@` in the
listed comment files and in `native_hooks.rs`; omit `comments` if unnecessary.
Summaries may contain newlines. New catalog contracts start at version 1;
this is separate from the global API number. A hook in a pending fragment is
not API yet; do not depend on it until it has a number.

After hook PRs merge, run `python3 tools/stamp_native_hooks.py` on current
main and open a PR with the result. The tool assigns one API number per
fragment in first-parent merge order. By default it consumes all pending
fragments; running it again does nothing. `--check` exits nonzero when any
are pending.

To register reviewed fragments while unrelated contracts are still being
settled, select their names without `.json`:

```sh
python3 tools/stamp_native_hooks.py --only gui_render_post client_services
```

Only the selected fragments get numbers, still in merge order regardless of
argument order. All others and their markers remain pending; a later stamp
continues from the current API number. Missing names (including already
stamped ones) fail before writing anything. `--check --only ...` checks the
selection without changing files. Selection does not check dependencies:
include any pending contracts the selected APIs require.

For local testing only, run the tool on a hook branch without committing
the result. If another stamp PR merges first, regenerate from current main.
