//! A guest's STOP_DESTROY_BLOCK (`ServerPlayerGameMode.handleBlockBreakAction`).
use super::*;

/// What `ServerPlayerGameMode.handleBlockBreakAction`'s STOP_DESTROY_BLOCK branch does with a
/// guest's STOP.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum StopDig {
    /// A block the last START did not name: the STOP is not this dig's.
    Other,
    /// Already broken, by the guest's own tick: only the crack goes.
    Gone,
    /// The dig the START counts is at least 70 % done: the block breaks outright.
    Break,
    /// Less than that: the server finishes it alone from that same START.
    Delay(u64),
}

/// The STOP branch's decision: a STOP is measured from the START that named its block
/// (`destroyPos`, which an abort or a STOP from out of reach leaves alone), whose ticks times
/// `getDestroyProgress` decide between the break and the delayed dig (#4124).
pub(super) fn stop_dig(
    play: &GuestPlay,
    pos: [i32; 3],
    air: bool,
    ticks: u64,
    per_tick: f32,
) -> StopDig {
    match play.dig_start_for(pos) {
        None => StopDig::Other,
        Some(_) if air => StopDig::Gone,
        Some(start) if per_tick * (ticks - start + 1) as f32 >= 0.7 => StopDig::Break,
        Some(start) => StopDig::Delay(start),
    }
}
