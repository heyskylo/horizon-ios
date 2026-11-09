use super::horizon_channel;

#[test]
fn horizon_channels_reach_their_handlers_and_others_go_to_the_mods() {
    // The recipe viewer's transfer (`native_transfer_recipe`) was handed to the mods
    // before its own arm could run, so a viewer's "+" never reached the server.
    assert!(horizon_channel(
        horizon_client::lan::recipes::TRANSFER_CHANNEL
    ));
    assert!(horizon_channel(horizon_protocol::quiet::CHANNEL));
    assert!(horizon_channel(horizon_protocol::mod_net::CHANNEL));
    assert!(horizon_channel(horizon_client::lod_net::CHANNEL));
    assert!(!horizon_channel("mymod:hello"));
    assert!(!horizon_channel("minecraft:brand"));
}
