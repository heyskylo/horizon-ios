# Terminal execute fork errors

Pinned Java 26.3 `CommandSourceStack.handleError` suppresses failure delivery when
its fork flag is true, independently of source count. A fork can yield one
source. Native run_execute already tracks that state and handles ordinary run
errors with it; terminal conditions now receive the same flag.

Only terminal error delivery is guarded. Existing callback delivery, store
values, count accumulation and positive feedback remain in place. No-fork
errors still propagate through the existing handler. No parser, predicate,
scoreboard policy, function compiler or callback-result behavior is redesigned.

## Frozen actual Game proof

Base: `41f822b659789780f58d32c63ddfb6887ea1b8bf`. The untouched executable is
our immutable own H137 pre-gameplay-edit build of this identical base, verified
by its original manifest/hash; no pending production is imported. The fixed
executable uses this same base.

```sh
python3 tools/test_terminal_execute_fork_parity.py \
  --binary /absolute/path/to/horizon-client --work-dir /fresh/output
```

The 135-record program compares complete feedback/styles/callbacks and stored
values. It admits two UUID-bound NoAI/NoGravity pigs, Health 10, position/motion,
loaded stone/air, actual two-source and empty selections, and relative world
clock 0. Cases cover one-source as false/unless, a sorted mixed success/failure
fork, at, a missing objective, visible no-fork and constant-position failures,
empty selection, positive messages, and nonterminal/nested controls. Every case
resets result/success to -99 and branch to -77 before the command and reads them
back. Fresh Java uses owned pinned jars and `_JAVA_OPTIONS=-Xmx1G`.

Untouched: **129/135**, all admissions passing. Six command records have seven
extra failure messages; every callback and result/success/branch read already
matches. Fixed: **135/135**. Same program and exact fresh reference bytes are
required for the pair. These are current measurements, not historical draft counts.

- Program SHA256: `ce1dc941e55ab3080328e74274edddffe2a2f6c45474c1f1ef09f90e330db8ef`
- Reference SHA256: `e6a5d9e64b75432b24bbea132ab8a398406bdc75bb6d55110541e04c41bae4f6`

The actual Game regression
`game::commands::execute::terminal_fork_tests::terminal_conditions_keep_the_fork_state_and_callbacks`
uses real registered actors and command parsing, with existing chat/result
capture. It first admits positive and no-fork controls, then fails old on a
one-source fork's Test failed message after its callback and stores pass. The identical fixed
regression passes 1/1. It also protects missing-objective, multiple-source, empty-selection
and clock behavior. Set HORIZON_TEST_RESOURCES and run it explicitly with
`--ignored`; it requires prepared resources and an isolated game directory.

The first unit draft compared a Score reference with an integer and failed to
compile. Its build log is retained as a rejected test prerequisite; only the
corrected identical old/fixed assertion is runtime proof.

## Raw diagnostics and limits

Two whole actor bodies and two absolute clock reads are predeclared separate
raw traces retained in java.log/rust.log. Both bodies retain only the existing extra native movement_speed attribute
(base 0.25, versus Java attributes[]). Absolute start/end values are old Java 305,
fixed Java 317, native 0 in both; relative deltas remain exactly 0. All four raw
records remain unequal in both runs and are preserved without normalization. No whole-NBT or
absolute-clock equality claim. All 135 canonical records are strict, with no
field masks, error normalization or numeric tolerances.

Runtime coverage is the listed command forms in one loaded Overworld. This
neither repairs nor claims other condition evaluators, team matching/H271,
selectors, generic modifier errors, function compilation or other dimensions.
The existing fork computation and return/count paths are retained.
