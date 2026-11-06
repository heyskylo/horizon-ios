# Logical Redstone Archive catalogue

`entries.jsonl` is a catalogue of every entry of the
[Logical Redstone Archive](https://github.com/mattbatwings/Logical-Redstone-Archive) at commit `da4dd51`,
made by **maybeachromium** (GitHub [SolsticeSpectrum](https://github.com/SolsticeSpectrum)) and shared in
[#10103](https://github.com/horizon-rust/horizon/issues/10103) under the WTFPL ([LICENSE](LICENSE)).
Its `entries/<CODE>.json` files are kept unchanged here, one entry per line, sorted by code.

Each entry gives a contraption's schematic path in the archive, its input and output positions (schematic
coordinates: x east, y up, z south, bit 0 first), their encoding (`binary`, or one 0-15 signal level per
position), what it computes, and how it was checked: `status` is `verified`, `partial`, `unverified` or
`no_schematic`, and `verified` lists the checks run in MCHPRS (cases, exhaustive or not, the switches held on).

`tools/test_redstone_catalog_parity.py` runs these contraptions in vanilla and in Horizon and compares them
tick by tick. The schematics themselves have no licence: the harness downloads them into `.local/lra-catalog/`
at run time, pinned to `da4dd51` and checked by sha256. Never commit them.
