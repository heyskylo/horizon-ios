//! Who a level event broadcast leaves out.

/// Who `ServerLevel.levelEvent` leaves out of the broadcast.
///
/// `LevelAccessor.levelEvent(int, BlockPos, int)` passes a null source, so the
/// acting player is included. `ComposterBlock.useItemOn` (and the hopper input
/// container) fill that way: event 1500, which the client turns into
/// `handleFill` (the fill particles and `playLocalSound`). A call that names
/// the player still excludes them. `collateral_break` is a later 2001 from
/// `destroyBlock` with no player, after the breaker's own `playerWillDestroy`.
pub(super) fn level_event_except(
    id: i32,
    acting: Option<u32>,
    collateral_break: bool,
) -> Option<u32> {
    if matches!(
        id,
        2015 | 1500
            | 1501
            | 1503
            | 1505
            | 1026
            | 1027
            | 1029
            | 1030
            | 1045
            | 1052
            | 2019
            | 2020
            | 3000
            | 3008
    ) || collateral_break
    {
        None
    } else {
        acting
    }
}
