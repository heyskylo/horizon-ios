//! Existing pure bed geometry shared by gameplay and the reusable Mob layer.
use horizon_core::geometry::Vec3;

/// The part of a bed a sleeper lies on: `AbstractBedBlock.getSleepHeight`
/// answers with the shape of the block it is given, and `StrawBedBlock`
/// first walks a head over to its foot, so a straw bed is slept on at its
/// foot's shape while the head's own (the pillow's) is taller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepOn {
    /// The head itself: `BedBlock` and every other bed.
    Head,
    /// The foot the straw bed's head lies against.
    Foot,
}

/// `StrawBedBlock.getSleepHeight` then `AbstractBedBlock.getSleepHeight`: the
/// part whose shape the sleeper lies on, or no part at all. A straw bed's
/// head takes its foot's block, and only a foot of the same block will do:
/// with the foot broken, or replaced by something else, the height is empty
/// (`OptionalDouble.empty`) and `LivingEntity.startSleeping` returns false,
/// so nobody lies in the half that is left.
pub fn slept_on(
    bed: &str,
    part: Option<&str>,
    foot: Option<(&str, Option<&str>)>,
) -> Option<SleepOn> {
    if bed != "minecraft:straw_bed" || part != Some("head") {
        return Some(SleepOn::Head);
    }
    match foot {
        Some(("minecraft:straw_bed", Some("foot"))) => Some(SleepOn::Foot),
        _ => None,
    }
}

/// `VoxelShape.max(Axis.Y)` of the boxes the pack gives a block, in the
/// block's own coordinates: the top of the bed, `1.0` for a full cube, and
/// `None` for a shape with nothing in it (`VoxelShape.isEmpty`).
pub fn shape_top(boxes: &[[f64; 6]], base_y: f64) -> Option<f64> {
    boxes
        .iter()
        .map(|b| b[4] - base_y)
        .fold(None, |top: Option<f64>, y| {
            Some(top.map_or(y, |top| top.max(y)))
        })
}

/// `LivingEntity.setPosToBed`: the middle of the sleeper's block, `0.125`
/// above the top of the shape, which `getSleepHeight` measured from that
/// block's own y.
pub fn pos_to_bed(head: [i32; 3], height: f64) -> Vec3 {
    Vec3::new(
        f64::from(head[0]) + 0.5,
        f64::from(head[1]) + height + 0.125,
        f64::from(head[2]) + 0.5,
    )
}

#[cfg(test)]
mod tests {
    use super::{SleepOn, pos_to_bed, shape_top, slept_on};

    /// `StrawBedBlock.getSleepHeight`: a straw bed's head is slept on at its
    /// foot, and one whose foot is gone - or is not a straw bed's foot any
    /// more - has no height, so `startSleeping` gives up on it.
    #[test]
    fn a_straw_beds_head_is_slept_on_at_its_foot() {
        let straw = "minecraft:straw_bed";
        assert_eq!(slept_on(straw, Some("head"), None), None, "missing foot");
        assert_eq!(
            slept_on(straw, Some("head"), Some((straw, None))),
            None,
            "missing foot part"
        );
        assert_eq!(
            slept_on(straw, Some("head"), Some((straw, Some("foot")))),
            Some(SleepOn::Foot)
        );
        assert_eq!(
            slept_on(straw, Some("head"), Some((straw, Some("head")))),
            None,
            "a head facing another head"
        );
        assert_eq!(
            slept_on(straw, Some("head"), Some(("minecraft:oak_planks", None))),
            None,
            "the foot was replaced"
        );
        assert_eq!(
            slept_on(
                straw,
                Some("head"),
                Some(("minecraft:red_bed", Some("foot")))
            ),
            None,
            "the foot is another bed"
        );
        assert_eq!(
            slept_on(straw, Some("foot"), None),
            Some(SleepOn::Head),
            "a straw bed's foot sleeps on its own shape"
        );
        assert_eq!(
            slept_on("minecraft:red_bed", Some("head"), None),
            Some(SleepOn::Head),
            "an ordinary bed's head is its own shape"
        );
    }

    /// `AbstractBedBlock.getSleepHeight`: the tallest box of the shape, from
    /// the block's own y; a shape with nothing in it has no height.
    #[test]
    fn the_height_is_the_top_of_the_shape() {
        // The boxes are the pack's, in world space, as `outline_boxes` gives them.
        let bed = [[10.0, 70.0, 5.0, 11.0, 70.5625, 6.0]];
        assert_eq!(shape_top(&bed, 70.0), Some(0.5625));
        let pillow = [[10.0, 70.0, 5.0, 11.0, 70.6875, 6.0]];
        assert_eq!(shape_top(&pillow, 70.0), Some(0.6875));
        let two = [
            [10.0, 70.0, 5.0, 11.0, 70.5625, 6.0],
            [10.0, 70.0, 5.0, 10.5, 70.25, 5.5],
        ];
        assert_eq!(shape_top(&two, 70.0), Some(0.5625), "the tallest box");
        assert_eq!(shape_top(&[], 70.0), None, "a bed with no shape");
    }

    /// `setPosToBed`: `0.125` above the height, in the middle of the block.
    /// A straw bed's head slept on a foot `7/16` tall at y 70 puts the player
    /// at 70.5625, where the head's own `9/16` (or the flat 0.5625 every bed
    /// was given) had them at 70.6875.
    #[test]
    fn the_sleeper_lies_an_eighth_above_the_shape() {
        let at = pos_to_bed([10, 70, 5], 0.4375);
        assert_eq!((at.x, at.y, at.z), (10.5, 70.5625, 5.5));
        assert_eq!(pos_to_bed([10, 70, 5], 0.5625).y, 70.6875);
    }
}
