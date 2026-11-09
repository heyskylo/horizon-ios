//! The plugin channels Horizon handles itself, before the mods.

/// Whether a guest's plugin channel message is for one of Horizon's own handlers in
/// `lan_guest_packet` (its `CustomPayload` arms), not for the mods.
pub(super) fn horizon_channel(channel: &str) -> bool {
    [
        horizon_protocol::quiet::CHANNEL,
        horizon_protocol::mod_net::CHANNEL,
        horizon_client::lod_net::CHANNEL,
        horizon_client::lan::recipes::TRANSFER_CHANNEL,
    ]
    .contains(&channel)
}
