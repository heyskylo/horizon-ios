//! Block states past vanilla's 16-bit global palette (docs/modpacks.md):
//! content mods can take the registry past 65,536 states. Its own test binary,
//! since the palette width is set once for the whole process.

extern crate horizon_client_lib as horizon_client;
use horizon_client::chunk::{
    Chunk, Section, block_strategy, known_block_state, known_blocks_only, set_block_states,
};
use horizon_core::paletted::{PalettedContainer, Strategy, storage_len};

#[test]
fn sections_hold_states_past_sixteen_bits() {
    // Vanilla's width until the registry needs more (one test: the width is
    // global to the process).
    assert_eq!(block_strategy().global_bits, 16);
    set_block_states(35_723);
    assert_eq!(block_strategy().global_bits, 16);
    set_block_states(100_000);
    assert_eq!(block_strategy().global_bits, 17);

    // A few states past 65,536 stay in the section's own palette.
    let mut chunk = Chunk::empty(0, 0);
    chunk.set(1, 70, 2, 70_000);
    chunk.set(3, 70, 4, 99_999);
    assert_eq!(chunk.get(1, 70, 2), 70_000);
    assert_eq!(chunk.get(3, 70, 4), 99_999);
    assert_eq!(chunk.get(0, 70, 0), 0);

    // More than 256 distinct states take the global palette at 17 bits.
    let mut values = [0u32; 4096];
    for (i, v) in values.iter_mut().enumerate() {
        *v = 65_000 + (i as u32 % 1000) * 30;
    }
    let mut section = Section {
        blocks: Section::from_blocks(&values),
        ..Section::default()
    };
    let mut out = [0u32; 4096];
    section.unpack(&mut out);
    assert_eq!(out, values);
    section.set(17, 99_998);
    assert_eq!(section.get(17), 99_998);
    assert_eq!(section.get(18), values[18]);

    // What a server with mods we lack sends: ids past the registry read as air,
    // though they fit the width. 100,000 states: 99,999 is the last one.
    assert!(known_block_state(99_999));
    assert!(!known_block_state(100_000));
    let mut sent = [1u32; 4096];
    sent[0] = 100_000;
    sent[1] = 131_071;
    let known = known_blocks_only(PalettedContainer::from_values(block_strategy(), &sent));
    assert_eq!((known.get(0), known.get(1), known.get(2)), (0, 0, 1));
    // The same in a global palette, with more ids than a section's own holds.
    let mut wide: Vec<u32> = (0..4096).map(|i| 1000 + i).collect();
    wide[5] = 120_000;
    let known = known_blocks_only(PalettedContainer::from_values(block_strategy(), &wide));
    assert_eq!((known.get(5), known.get(6)), (0, 1006));
    // A palette entry no position uses doesn't stay either.
    let words = vec![0; storage_len(4, 4096)];
    let container =
        PalettedContainer::from_wire(block_strategy(), 4, vec![1, 100_000], words).unwrap();
    assert!(container.maybe_has(|id| id == 100_000));
    let known = known_blocks_only(container);
    assert!(!known.maybe_has(|id| id == 100_000));
    assert_eq!(known.get(0), 1);
    // A host wider than us: narrowed, and what doesn't fit is air.
    let wider = Strategy::blocks(20);
    let mut sent = [7u32; 4096];
    sent[3] = 900_000;
    let known = known_blocks_only(PalettedContainer::from_values(wider, &sent));
    assert_eq!(known.strategy().global_bits, block_strategy().global_bits);
    assert_eq!((known.get(3), known.get(4)), (0, 7));
    // Within vanilla's width too: with vanilla's registry, 40,000 is unknown.
    set_block_states(35_723);
    assert!(!known_block_state(35_723));
    let known = known_blocks_only(PalettedContainer::new(block_strategy(), 40_000));
    assert_eq!(known.get(0), 0);
}
