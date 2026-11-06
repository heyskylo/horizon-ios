# Modern sign literal strings (H238)

Frozen source: `41f822b659789780f58d32c63ddfb6887ea1b8bf`. Own actual Game versus fresh pinned Java 26.3, using `tools/test_sign_literal_parity.py`. The initial source read precedes a single same-level clone. No observation ticks are added.

The 53 complete-record contract includes loaded oak-sign blocks and block entities, four exact String messages, literal-type predicates, a real structured red/bold back-side component, a non-sign chest with stone2, and actual relative-clock delta0. It compares complete feedback, styles and callbacks without masks or tolerances. Four separately declared raw traces retain both full sign bodies and absolute startup clocks; these are not claimed equal.

Untouched Game: **44/53**; fixed Game: **53/53**, with the same program and byte-identical fresh Java reference. All nine residuals are below; every admission passes. The resource-free real `SignData::decode`/`encode` regression also fails before the fix: it reads `quoted text` and `literal object` instead of the original literal strings.

## Exact untouched residuals

| Label | Java expected | Untouched Game actual | Callback expected / actual |
|---|---|---|---|
| source-before-messages | `2, 200, 2 has the following block data: ["plain text", '"quoted text"', '{"text":"literal object","bold":true}', ""]` | `2, 200, 2 has the following block data: ["plain text", "quoted text", "literal object", ""]` | `[[True, 4]]` / `[[True, 4]]` |
| source-before-line1 | `2, 200, 2 has the following block data: '"quoted text"'` | `2, 200, 2 has the following block data: "quoted text"` | `[[True, 13]]` / `[[True, 11]]` |
| source-before-line2 | `2, 200, 2 has the following block data: '{"text":"literal object","bold":true}'` | `2, 200, 2 has the following block data: "literal object"` | `[[True, 37]]` / `[[True, 14]]` |
| source-before-literal-types | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| clone-after-messages | `4, 200, 2 has the following block data: ["plain text", '"quoted text"', '{"text":"literal object","bold":true}', ""]` | `4, 200, 2 has the following block data: ["plain text", "quoted text", "literal object", ""]` | `[[True, 4]]` / `[[True, 4]]` |
| clone-after-line1 | `4, 200, 2 has the following block data: '"quoted text"'` | `4, 200, 2 has the following block data: "quoted text"` | `[[True, 13]]` / `[[True, 11]]` |
| clone-after-line2 | `4, 200, 2 has the following block data: '{"text":"literal object","bold":true}'` | `4, 200, 2 has the following block data: "literal object"` | `[[True, 37]]` / `[[True, 14]]` |
| clone-after-literal-types | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| source-after-messages | `2, 200, 2 has the following block data: ["plain text", '"quoted text"', '{"text":"literal object","bold":true}', ""]` | `2, 200, 2 has the following block data: ["plain text", "quoted text", "literal object", ""]` | `[[True, 4]]` / `[[True, 4]]` |

## Source and scope

Pinned `SignText.CODEC` uses `ComponentSerialization.CODEC`; its String branch constructs `Component.literal`. `signs.rs::component_text` is only called by the modern `SignData::decode` path. The fix keeps `Tag::String` literal and leaves compound/list decoding, styling and click handling unchanged. Existing structured-component tests remain in place. This does not establish legacy migration, rendered output, arbitrary rich components or broader clone behavior.

## Reproduction and evidence

```sh
python3 tools/test_sign_literal_parity.py --reference-only --work-dir /tmp/sign-java/run
python3 tools/test_sign_literal_parity.py --binary /absolute/path/to/horizon-client --reference /tmp/sign-java/run/vanilla.jsonl --work-dir /tmp/sign-game/run
```

Each output directory must be fresh. Runs retain `script.jsonl`, `declaration.json`, complete `vanilla.jsonl`/`rust.jsonl`, `strict-report.json`, and complete Java/Game logs with raw trace records. Private own evidence and immutable binaries remain under `.local/h238/`; no Mojang resources are published.

- Program SHA256: `b54cfc22887b2dbf947f352fe13f813a95d1eda16fb9b6f2e21a3c60955eb821`
- Fresh Java reference SHA256: `e94d6e051fe155cbcd1406ecb9642a4b043acb94a0a6bd506bc86408fb811d16`
- Untouched Game binary SHA256: `e45a7e8023e21869d10c1b010f3c3617bb092f62e3b941a94bbd986619d1e131`

## Raw diagnostic limits

Untouched paired run absolute time: Java 297 at start/end, Game 0 at start/end. Fixed paired run: Java 317 at start/end, Game 0 at start/end. The separate exact relative-clock admission is 0 in both programs; no absolute values are normalized.

Both complete sign-body traces retain an unrelated compound display-order difference: Java shows `{color: "red", text: "Structured", bold: 1b}` while Game shows `{color: "red", bold: 1b, text: "Structured"}`. The literal messages in those bodies are corrected. The strict scalar/typed structured checks pass; no formatter change is included.

```text
/execute store result score start h238 run time query gametime 
JAVA: The game time is 317 tick(s) 
GAME: The game time is 0 tick(s)
/data get block 2 200 2 
JAVA: 2, 200, 2 has the following block data: {back_text: {has_glowing_text: 0b, color: "black", messages: [{color: "red", text: "Structured", bold: 1b}, "", "", ""]}, components: {}, is_waxed: 0b, x: 2, y: 200, z: 2, id: "minecraft:sign", front_text: {has_glowing_text: 0b, color: "black", messages: ["plain text", '"quoted text"', '{"text":"literal object","bold":true}', ""]}} 
GAME: 2, 200, 2 has the following block data: {back_text: {has_glowing_text: 0b, color: "black", messages: [{color: "red", bold: 1b, text: "Structured"}, "", "", ""]}, components: {}, is_waxed: 0b, x: 2, y: 200, z: 2, id: "minecraft:sign", front_text: {has_glowing_text: 0b, color: "black", messages: ["plain text", '"quoted text"', '{"text":"literal object","bold":true}', ""]}}
/data get block 4 200 2 
JAVA: 4, 200, 2 has the following block data: {back_text: {has_glowing_text: 0b, color: "black", messages: [{color: "red", text: "Structured", bold: 1b}, "", "", ""]}, components: {}, is_waxed: 0b, x: 4, y: 200, z: 2, id: "minecraft:sign", front_text: {has_glowing_text: 0b, color: "black", messages: ["plain text", '"quoted text"', '{"text":"literal object","bold":true}', ""]}} 
GAME: 4, 200, 2 has the following block data: {back_text: {has_glowing_text: 0b, color: "black", messages: [{color: "red", bold: 1b, text: "Structured"}, "", "", ""]}, components: {}, is_waxed: 0b, x: 4, y: 200, z: 2, id: "minecraft:sign", front_text: {has_glowing_text: 0b, color: "black", messages: ["plain text", '"quoted text"', '{"text":"literal object","bold":true}', ""]}}
/execute store result score end h238 run time query gametime 
JAVA: The game time is 317 tick(s) 
GAME: The game time is 0 tick(s)
```

## Local checks

Required `pr_ready --quick --verbose --base 41f822b659789780f58d32c63ddfb6887ea1b8bf` ran once on the final production change: **1546 Rust tests passed**, including the same decoder/save regression that failed old; rustfmt, clippy (including server features), doc tests and public-tree checks passed. Overall exit1 remains due to the three pre-existing macOS optional-release-archive Python subtests (`musl[@]` masks0/1, `arm64[@]` mask2: unbound Bash array); 708 Python tests ran with one skip. The broad 382 affected harnesses were listed for CI by `--quick`; only the scoped actual Game pair above was run here.
