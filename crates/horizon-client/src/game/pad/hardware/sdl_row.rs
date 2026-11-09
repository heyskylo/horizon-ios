//! What gilrs drops when it reads an SDL row: `~`, which says an axis runs the other way.
use gilrs::{Gamepad, MappingSource};

/// `[left x, left y, right x, right y]`, turned over.
type Flips = [bool; 4];

/// Every Linux row in gilrs 0.11.2's copy of the database with a `~` on a stick axis, by the
/// id a row starts with.
const FLIPPED_ROWS: [(&str, Flips); 9] = [
    // Cyber Gadget GameCube Controller
    (
        "03000000260900008888000000010000",
        [false, false, false, true],
    ),
    // Mayflash Adapter
    (
        "030000008f0e00001330000010010000",
        [false, false, true, false],
    ),
    // Microsoft Dual Strike
    (
        "030000005e0400002800000000010000",
        [false, false, false, true],
    ),
    // Nintendo GameCube Adapter
    (
        "03000000790000004618000010010000",
        [false, false, true, true],
    ),
    // Nintendo Switch 2 Pro Controller
    (
        "030000007e0500006920000011010000",
        [false, true, false, true],
    ),
    // Nintendo Switch Left Joy-Con
    (
        "050000007e0500000620000001800000",
        [false, true, false, false],
    ),
    // Nintendo Switch Right Joy-Con
    (
        "050000007e0500000720000001800000",
        [true, false, false, false],
    ),
    // NSO GameCube Controller
    (
        "030000007e0500007320000011010000",
        [false, true, false, true],
    ),
    // PS SS N64 Adapter
    (
        "03000000250900000017000010010000",
        [false, false, true, false],
    ),
];

/// The sticks as the row meant them.
pub(super) fn sticks(pad: &Gamepad, left: [f32; 2], right: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    if !cfg!(target_os = "linux") || pad.mapping_source() != MappingSource::SdlMappings {
        return (left, right);
    }
    let id: String = pad
        .uuid()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    apply(flips_of(&id), left, right)
}

fn flips_of(id: &str) -> Flips {
    FLIPPED_ROWS
        .iter()
        .find(|(row, _)| *row == id)
        .map_or([false; 4], |(_, flips)| *flips)
}

fn apply(flips: Flips, left: [f32; 2], right: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    let turn = |value: f32, flip: bool| if flip { -value } else { value };
    (
        [turn(left[0], flips[0]), turn(left[1], flips[1])],
        [turn(right[0], flips[2]), turn(right[1], flips[3])],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_switch_2_pro_pushed_forward_reads_forward() {
        // Nintendo Switch 2 Pro Controller
        let flips = flips_of("030000007e0500006920000011010000");
        assert_eq!(
            apply(flips, [0.5, -1.0], [0.25, 1.0]),
            ([0.5, 1.0], [0.25, -1.0])
        );
    }

    #[test]
    fn a_row_without_a_tilde_is_left_alone() {
        let flips = flips_of("050000005e040000130b000022050000");
        assert_eq!(
            apply(flips, [0.5, -1.0], [0.25, 1.0]),
            ([0.5, -1.0], [0.25, 1.0])
        );
    }
}
