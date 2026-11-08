<!-- Opened by an AI agent? First line: "> 🤖 Opened by <your GitHub login>'s AI agent (<tool, model>), not by <your GitHub login> personally." Use your own login, not someone else's. -->

## What and why

<!-- What this changes, and why. Link the issue it fixes ("Fixes #123"). -->

## Kind

<!-- Tick one: it sets the pull request's label (CONTRIBUTING.md "Pull requests"). -->

- [ ] parity: Horizon now does what vanilla 26.3 does
- [ ] bug: a crash or a breakage
- [ ] perf: faster, or less memory
- [ ] feature: something new for players (listed under New in the release notes)
- [ ] extra: Horizon-only, off by default
- [ ] tooling: tools, CI, tests, harnesses
- [ ] docs

## Layer

<!-- engine, platform or Horizon (docs/layers.md, "Where a new feature goes"). Reviewers check:
     - the new code sits in a crate or module of that layer;
     - no new upward dependency (CI's layer check), and no engine module calling up into platform or Horizon code;
     - Horizon features in the game are behind `horizon-extras`, and platform features that change the vanilla
       look are off by default;
     - no new built-in server address outside the Horizon layer. -->

## Vanilla reference

<!-- The vanilla classes and methods this follows (tools/rea_vanilla.py), and the harness that checks it.
     "Horizon-only" for a feature vanilla doesn't have; "none" for tooling or docs. -->

## Other projects' code

<!-- "none", or for each project you read or copied from: its link at the commit, its license (only MIT,
     BSD-2-Clause, BSD-3-Clause, Apache-2.0, ISC, zlib, 0BSD, MIT-0, Unlicense or CC0-1.0), and what you took.
     CONTRIBUTING.md "Only original code". Shader packs: also the sections of docs/cleanroom/shader-packs-spec.md,
     the documentation pages and the packs you used. -->

## Self-check

<!-- Paste the block `python3 tools/pr_ready.py` prints (`--base upstream/main` from a fork). -->

## Screenshots

<!-- For anything players see: before and after. Delete this section otherwise. -->

<!-- Please tick "Allow edits by maintainers": we push small fixups ourselves instead of a round trip.
     See CONTRIBUTING.md. -->
