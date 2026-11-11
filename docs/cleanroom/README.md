# Clean-room rewrite of shader pack support

> Published on 2026-11-08 as the record of the shader-pack clean room (#6739, #1555). The text is as written in
> October 2026, except that local paths, machine names and the names of private files are replaced by a
> description of what they were, and the last section was added. Nothing else was changed.

Horizon's first shader pack support was written from another loader's source code, under a licence we do not meet,
and it is being removed. This folder holds what is needed to bring the feature back without taking anyone's code: a
specification written from public documentation, and the rules that keep the people who implement it away from
everything else.

## Files

| File | What it is | Who writes it |
|---|---|---|
| [shader-packs-spec.md](shader-packs-spec.md) | The specification: pack layout, programs and their order, `shaders.properties`, the uniform/attribute/sampler interface, render targets, shadow map, settings UI, open questions, sources | spec team |
| [provenance-log.md](provenance-log.md) | Every source the spec team read for the current revision, with revisions and hashes, and every incident | spec team |
| `questions/Q-NNN.md` (created when needed) | One question from the implementer per file | implementer |
| `decisions.md` (created when needed) | The implementer's answer to each open question of the spec, with the test that settled it | implementer |

## Two teams, two sets of sources

Each team is a separate person or agent session. Nobody moves from the spec team to the implementation team or back.
An agent session that has read any forbidden material is disqualified for both roles.

| | Spec team | Implementation team |
|---|---|---|
| May read | Public documentation of the pack format for pack authors (OptiFine's `shaders.txt` / `shaders.properties` docs, the `shaders.properties` documentation site, ShaderDoc, the shaderLABS wiki, tutorials about writing packs); the GLSL and OpenGL specifications; packs' own `.properties` and `.lang` files and folder layout; Minecraft 26.3 | **This folder only** (the spec, its open questions, the answers to its questions); Minecraft 26.3 (the project's reference, as for every Horizon feature); wgpu and naga documentation; Horizon's code outside the removed shader feature |
| May use | web search and fetch for documentation pages; local copies of public packs | running shader packs inside Horizon as black boxes and looking at the result; the packs' own `.properties` and `.lang` files when a test needs them |
| Never | source or binaries of Iris, OptiFine, Oculus, Canvas, Sodium or any other loader or renderer; Horizon's removed shader code (`crates/horizon-shaderpack`, `java_pack.rs`, `gfx/java_stages.rs`, `gfx/java_terrain.rs`, `gfx/pack_stages.rs`, `shader_pack.rs`, `docs/shader-packs.md`, `docs/java-packs.md`), its git history and its PRs; "how X does it" answers built from code | everything in the spec team's "Never" column, **and** the documentation the spec team read (the spec is the only channel), old Horizon notes, reports or issues about the removed feature, and `git log` / `git show` / `git blame` of the removed paths |

The Khronos GLSL and OpenGL specifications are public standards the spec cites by section. Whether the implementation
team may read them directly (they are needed to write a GLSL front end) is Ludovic's call; the spec team's
recommendation is yes, because they describe the language, not any loader.

Running another loader to look at a pack's output is not part of this process. If a comparison with real output is
needed, the spec team records the observation (what was seen, with which pack and settings) in a new spec revision.

## Workflow

1. **Spec.** The spec team writes `shader-packs-spec.md`, tags each statement with its source, and lists in section 9
   every point the documentation leaves open or contradicts. Each revision updates the revision line at the top,
   `provenance-log.md`, and a short changelog at the end of the spec.
2. **Implementation.** The implementation team builds the feature in Horizon from the spec, in Rust on wgpu/naga,
   following Horizon's usual rules (small PRs, harnesses, both single-player paths, `pr_ready.py`). Every PR body says
   which spec revision it implements and links the decisions it relied on.
3. **Open questions.** For each OQ of section 9 the implementer decides from first principles and from tests (packs
   run in Horizon, vanilla 26.3, the GL conventions in spec 5.5), and records the decision and the test in
   `decisions.md`. A decision is never taken from another implementation, its code, its issue tracker or its changelog.
4. **Questions back to the spec team.** When the spec is unclear, the implementer writes `questions/Q-NNN.md`: the
   spec section, the question, and what was observed. It contains no code from anywhere else and no guess about how
   another loader does it. The spec team answers only with a new revision of the spec (a clarified section, a new OQ,
   or a recorded observation), with sources. Answers are never code, pseudo-code taken from an implementation, or
   "loader X does Y".
5. **Review.** Reviewers of the implementation PRs follow the same "never" list as the implementer. Reviews compare
   against the spec, vanilla and observed output only.

## Showing the result is independent

- **Provenance per section:** spec section 11 maps each section to its sources; `provenance-log.md` pins every source
  to a revision and hash. Anyone can check a statement against the public document it cites.
- **Input manifest:** each implementation PR lists what its author read: the spec revision, the questions and answers,
  `decisions.md` entries, the vanilla classes and the wgpu/naga pages it relied on.
- **Session records:** the orchestrator keeps the names of the agent sessions that did the spec and the
  implementation, and confirms that no implementation session was ever given a forbidden path, file or link.
- **Name tripwire:** CI or the reviewer greps the new code, comments, commits and PRs for internal names of other
  loaders (those listed in the maintainer's private agent rules, such as `AlphaTests`, `BlendModeOverride`, `ShadowMatrices`,
  `ShaderPackScreen`, JCPP). The interface names a pack uses (uniform names, keys, file names) are format facts from the
  spec and are expected.
- **Decisions on record:** every behaviour the documentation does not fix is traced to a `decisions.md` entry and its
  test, not to anyone's code.

## Current state

- Revision 1 of the spec: 2026-10-24.
- No implementation has started. The removal of the old feature (`shaders-remove-1`) is a separate task.

## Orchestrator decisions (2026-10-24)

- The implementation team **may read the Khronos GLSL and OpenGL specifications** directly. They are public language standards, not any loader.
- **OQ-2: Horizon does not define another loader's identity macros** (`IS_IRIS`, `IRIS_VERSION`, `IRIS_FEATURE_*`, or OptiFine's version macro if one names OptiFine itself). Horizon is not that loader, and pretending to be it is how the complaint started. Packs that branch on those macros get their non-Iris path. The format's documented `MC_*` macros are defined.
- The old Rust spike is no longer in this folder. It was moved out of every agent's reach, to a private archive outside every repository.
- The implementation starts only after the removal PR (shaders-remove-1) has merged, in a fresh worktree of that main.

## Since publication (2026-11-08)

- The implementation is `crates/horizon-shaders` ([docs/shaders.md](../shaders.md) covers Horizon's own WGSL packs).
  Its first slices were written by the implementation team above; later contributors did not have the spec, which
  was private until now.
- Not published: the implementer's question files and the implementation plan, which were working notes.
