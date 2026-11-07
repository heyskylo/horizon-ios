# Fully inline armor-trim holders

A valid fully inline armor trim was accepted by the command parser but lost from the actual modeled stack. `StackData.trim` required two names and the modeled-key filter excluded raw fallback. The saved encoder and both item-sync directions therefore had no direct holders to project. Named trim already worked.

Frozen base: `ac401818533eff43a518549ba21ce2ed4ee69c71`. Issue: [#11632](https://github.com/horizon-rust/horizon/issues/11632).

The bounded repair gives material and pattern typed named/direct holders, retaining palette/asset identifiers, descriptions through the existing text-tag representation, and the pattern decal. Existing saved/unsaved item codecs project that actual model. Protocol `Component::Trim` and its named byte encoding remain unchanged; `TrimDirect` uses existing Reader/Writer/NBT/holder conventions. The actual LAN send and item-sync receive paths carry direct values without registry IDs or raw-source fallback. Existing direct HashOps logic is reused, not replaced.

The representation reaches existing gear, loot projections, smithing's named construction and named-only rendering/tooltip consumers. Their adapters preserve named behavior. No inline rendering, tooltip or smithing support is claimed. No generic registry, component schema, numeric helper or effective-item policy changed. Current open/closed searches and actual10559/8718/1633/2950/4782 diffs were reviewed: renderer caching, older-version adapters, named network/loot trims and direct hashing are separate from this model bridge. Prototype-aware saved encoding and explicit-removal work remain independent.

## Actual command pair

The predeclared134-record program independently admits a real loaded chest and support, actual player UUID/type/Health/mode/position, three empty slots, accepted item commands and zero relative world ticks. It measures a diamond chestplate with fully inline material/pattern descriptions, a named gold/coast counterpart and a plain counterpart, each retaining `custom_data={keep:284}`. Complete typed saved stacks/components, field presence, feedback/styles and fresh result/success stores stay strict.

Fresh Java26.3 passes134. Pristine native is **121/134**; all13 differences are inline saved reads/presence. Named/plain/custom-data, setup, actual callbacks and clocks pass. All original residual expected/actual values are in [inline-trim-residuals.jsonl](inline-trim-residuals.jsonl). Both the first fixed and final-source captures pass134/134 with identical fresh Java reference bytes. The final source was rebuilt after the named-only borrow cleanup and stronger hash assertion, then the unchanged command and actual packet programs were repeated. No raw absolute-clock equality is claimed. Actual full parent body and absolute clocks are retained as predeclared traces.

```sh
python3 tools/test_inline_trim_holders_parity.py --reference-only --work-dir .local/trim-java
python3 tools/test_inline_trim_holders_parity.py --binary /path/to/game \
  --reference .local/trim-java/vanilla.jsonl --work-dir .local/trim-game
```

Script SHA256: `3822a250e6f3dfdcc9905738b100fc05f6d238499cbe4940e6ae578dc4c9d5ab`.

## Actual packet route

The existing LocatorBarClient joins actual dedicated Java/native servers and uses the official packet codec, requiring complete payload consumption. After two real join steps, actual UUID/type/Health/mode/position, empty inventory, loaded support and saved item/count/slot guards pass. Each real inventory command is followed by a receipt and one declared delivery tick; exact relative clocks are1/2/3. The official client's decoded typed holder/item/count/custom facts are compared. Every emitted packet, both directions, raw bytes, order, chat and clocks are retained locally. No full packet-family, all-neighbor-field, arbitrary timing or random equality is claimed.

Old native lacks the inline trim in the actual packet; named gold/coast and plain match. Both the first fixed and final binaries match all declared packet facts, with identical fresh Java fact-reference bytes. The complete relevant initial/three-case inventory packets and official decodes are published in [inline-trim-packets.jsonl](inline-trim-packets.jsonl). Complete captures, including other packet families and rejected attempts, remain in `.local/h284/`.

```sh
python3 tools/inline_trim_packet_capture.py --reference-only --binary /path/to/game \
  --world .local/trim-java/vanilla/universe/harness --work-dir .local/trim-wire-java
python3 tools/inline_trim_packet_capture.py --binary /path/to/game \
  --world .local/trim-java/vanilla/universe/harness --work-dir .local/trim-wire-game
```

The supplied world is copied into fresh isolated server directories. The program prepares and verifies its floor with actual commands. The binary is unused by `--reference-only`. Packet script SHA256: `0a669f6caeccfb7b0735c110b12709038f43693ae395c44d54640a1b6de6fa63`.

## Model and codec controls

The real item decode→model→save regression fails old at missing actual `trim` adoption and passes fixed. A new stack copies only the actual typed trim and explicit unrelated custom data, proving no raw-input echo. The actual item-sync bridge test crosses the stream, decodes into the model, clears cached wire state, saves and re-encodes. A protocol unit decodes the exact Slot payload from the admitted official inventory packet, checks every direct field and zero trailing bytes, then requires exact complete re-encoding. Existing named trim packet controls stay intact. Direct hashing delegates to the unchanged existing direct-holder hash path.

## Limits and retained attempts

Runtime coverage is one fully inline material+pattern with literal descriptions, named gold/coast and plain diamond chestplates. Mixed holders and malformed inputs are not a closure claim; rendering, smithing, tooltip, reopen, all item families and all registry contexts are excluded. Existing generic text representation is reused, not a new rich-text codec.

Rejected captures remain separate: the first invocation used Python3.9, then `--loaded-fixture` replaced the intended program; the next Java typed predicate incorrectly expected structured literal descriptions to retain their input compound instead of Java's canonical string. Packet v2 found that the copied oracle world did not retain its floor; v3 exposed a test-helper barrier serial restart matching stale setup state. Those actual partial traces are retained and have no qualified native defect counts. Final packet v4 uses one monotonic receipt stream and actual floor guards. One preliminary unit invocation selected zero tests; the fully qualified invocation subsequently ran and failed at actual model adoption. All logs, declarations, original/final binaries and raw proofs are protected in `.local/h284/`.

## Reference hashes and later source review

Command reference SHA256: `21ea821adf6670a2b3d424b24a801dbee4ac5a36183483ac5b0bb0431f97f282`. Packet fact reference SHA256: `b18d583a3060ad9c3a82323d9caa91210dd62db5d1a6b252792fff38e1f194b4`. Full wire records are not normalized or required to be byte-equal between servers. The existing item-wire probe still classifies direct trims with named trims as lossless, retaining its strict byte-reencoding requirement.

Read-only review of later main `c1102d9227d8170e1b6676d4495de5362dae41f0` found zero deltas in all touched source, instructions and relevant command/client/capture drivers. The pair remains frozen onac4018185; no foreign production was imported. Unknown phase start times remain null in the private manifest.

## Required local checks

`tools/pr_ready.py --quick --base ac401818533eff43a518549ba21ce2ed4ee69c71` ran2026-11-07T05:55:52Z–06:14:04Z. Public tree, rustfmt, Clippy,3,578 Rust tests, doc tests and27 content-root tests pass. Overall exit1:718 Python tests include three existing optional-release Bash-array subcase failures (`musl[@]`/`arm64[@]` unbound), with one skip. No release-script repair is included. Quick mode listed411 affected parity fixtures without running them; the focused command and live packet captures were run independently. The586-line scope warning includes the atomic model/protocol API change, mechanical existing consumers, units and executable fixtures.

Final binary SHA256: `05339b6d4b96f5a7c39b6e91512ad0db1c4c9b514b82e9caff10ff0a2c511c5b`. Final command capture2026-11-07T06:18:10Z–06:18:21Z and actual packet capture06:18:21Z–06:18:43Z both pass. The actual Java command/fact reference bytes are identical to their frozen old counterparts. All initial, first-fixed and final relevant inventory packets remain distinguished in the public JSONL; all other raw packets remain in the protected local archive.
