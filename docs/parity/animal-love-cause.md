# Saved Animal LoveCause

Pinned Java 26.3 `Animal.addAdditionalSaveData` writes InLove and stores an
optional `EntityReference` as LoveCause. A null reference writes no key.
Horizon already parses the reference into `extra.love_cause`; its raw-backed
Animal writer now removes LoveCause when that actual modeled value is None.
Valid modeled UUID output, shared decoding, counters and other fields are unchanged.

## Frozen Game proof

Base: `398da2168910f1e234eb1d4cf5f37790037f2d32`, one persistent checkout.
The own untouched and fixed executables use this same base. No pending H091,
H109 or other production is imported.

```sh
python3 tools/test_animal_love_cause_parity.py \
  --binary /absolute/path/to/horizon-client --work-dir /fresh/output
```

The predeclared focused program contains 93 complete records and six independent
pigs: absent reference, valid four-int UUID, short/empty IntArrays, scalar and
compound. UUID/type/Health 10/Pos/Motion/NoAI/NoGravity/load/floor and exact relative
clock 0 are admitted; nonzero Int 123 InLove, names and tags stay strict. Each
reference read has fresh result/success sentinels and exact presence/type checks.
The valid UUID is a loaded preservation payload, not a live breeder witness.
Fresh Java uses the owner's pinned jars and `_JAVA_OPTIONS=-Xmx1G`.

Untouched: **78/93**, all admissions passing. The 15 differences are reads,
stores and presence of the four invalid inputs, not 15 independent defects.
Fixed: **93/93**. Same script and exact fresh reference bytes are required.

- Program SHA256: `4d9e65228d9deafabab3ca67db331b2f11cc4a94f0670efdf493e6a901b6196d`
- Reference SHA256: `9f1fe0178bbc76c24d65b4d11eacfadb40e0cc4dc0ce7e1da234158877a7dd72`

The resource-free real decode/save regression
`entity_nbt::animal_love_cause_tests::animal_save_uses_the_parsed_love_cause_instead_of_stale_raw`
fails old after confirming a malformed empty-array input parsed as None. It
covers raw-invalid/no-raw, modeled Some, valid loaded references, a cleared model
with valid stale raw data, InLove/name/tag preservation and unchanged non-Animal
raw behavior. The identical fixed regression passes in the required Rust suite. The five-int unit input preserves current
native parsing policy; it is not a claim that Java rejects it.

## Separate UUID admission diagnostic

The initial 106-record program/reference/raws remain unchanged. The public
`--uuid-length-diagnostic` mode reproduces exactly its operations and labels;
this includes the independently observed five-int input. Java accepts
`[I;57,1,2,3,4]` and saves `[I;57,1,2,3]`; the unchanged native decoder requires
exactly four ints and returns None. The focused six-scene program was declared
before its fresh Java and paired captures. No fields are masked or normalized.

Old full diagnostic: **89/106**, including the 15 assigned differences plus
`long-cause-read/result` (Java four-int output/result 4; native five-int
output/result 5). Fixed full diagnostic: **102/106**. Its four remaining rows are:

| Label | Java expected | Fixed native actual |
|---|---|---|
| long-cause-read | `[I;57,1,2,3]`, callback true/4 | missing LoveCause, callback false/0 |
| long-cause-result | score 4, callback true/4 | score 0, callback true/0 |
| long-cause-success | score 1, callback true/1 | score 0, callback true/0 |
| long-cause-absent | Test failed. Count: 1, callback false/0 | Test passed, callback true/1 |

The full styled records remain in the raw logs. The new writer reflects the
unchanged native decoder's None instead of retaining the five-int raw value;
it does not make that independently valid Java input decode correctly. This UUID-admission seam remains
separate; neither shared UUID decoding nor String acceptance is changed.

Whole bodies and absolute clock reads are predeclared separate raw traces in
`java.log` and `rust.log`; the six focused bodies differ on the existing extra native movement_speed
attribute (Java `attributes:[]`, native base 0.25). Before the fix, the four invalid
cases additionally retained their raw LoveCause; afterward only the attribute
residual remains. The full diagnostic's overlong body also differs on LoveCause.
Absolute start/end values: focused old Java 281/fixed Java 269 versus native 0;
full old Java 301/fixed Java 293 versus native 0; both relative deltas remain exactly 0.
No raw records were normalized. No whole-body, absolute-clock
or full 106 equality claim. Runtime coverage is pigs; other Animal writers share
the reviewed branch but are not runtime-tested here. The Zombie unit protects
unchanged native non-Animal behavior, not its Java parity. Breeding/attribution,
AI/love lifetime, String admission and generic raw cleanup remain outside scope.
