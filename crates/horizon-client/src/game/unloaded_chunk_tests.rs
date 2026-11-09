use super::touching_unloaded_chunk;
use horizon_client::pose::Dimensions;

/// `Entity.touchingUnloadedChunk` (#11312): the box inflated by one block
/// is checked chunk by chunk; a player in the middle of a loaded chunk
/// touches none, one within a block of its edge touches the neighbour.
#[test]
fn a_box_within_a_block_of_a_chunk_edge_touches_the_next_chunk() {
    let body = Dimensions {
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
    };
    let only_origin = |x: i32, z: i32| (x, z) == (0, 0);
    assert!(!touching_unloaded_chunk(
        [8.0, 64.0, 8.0],
        body,
        only_origin
    ));
    // 15.5 + 0.3 + 1 reaches 16.8, in chunk 1.
    assert!(touching_unloaded_chunk(
        [15.5, 64.0, 8.0],
        body,
        only_origin
    ));
    // 14.0 + 1.3 = 15.3: Mth.ceil gives 16, the first block of chunk 1.
    assert!(touching_unloaded_chunk(
        [14.0, 64.0, 8.0],
        body,
        only_origin
    ));
    // 13.5 + 1.3 = 14.8: ceil 15, still chunk 0.
    assert!(!touching_unloaded_chunk(
        [13.5, 64.0, 8.0],
        body,
        only_origin
    ));
    // Everything loaded: nothing is touched anywhere.
    assert!(!touching_unloaded_chunk(
        [15.9, 64.0, -0.5],
        body,
        |_, _| true
    ));
}
