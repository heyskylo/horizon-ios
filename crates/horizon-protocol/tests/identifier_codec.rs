use horizon_protocol::{
    buf::{Codec, Ident, Reader, Str, Writer},
    packets::{configuration, play},
};

fn wire(value: &str) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.string(value);
    writer.data
}

#[test]
fn identifiers_normalize_default_namespace_without_restricting_empty_paths() {
    for (input, expected) in [
        ("stone", "minecraft:stone"),
        (":stone", "minecraft:stone"),
        ("", "minecraft:"),
        (":", "minecraft:"),
        ("mod:", "mod:"),
        ("mod:dir/file.-_09", "mod:dir/file.-_09"),
    ] {
        let bytes = wire(input);
        let mut reader = Reader::new(&bytes);
        assert_eq!(Ident::read(&mut reader).unwrap(), expected, "{input:?}");
        assert_eq!(reader.remaining(), 0);
    }
}

#[test]
fn identifier_validation_does_not_change_ordinary_packet_strings() {
    for input in [
        "STONE",
        "mod:UPPER",
        "a:b:c",
        "a+b:c",
        "a/b:c",
        "a b:c",
        "a:é",
    ] {
        let bytes = wire(input);
        assert!(Ident::read(&mut Reader::new(&bytes)).is_err(), "{input:?}");
        assert_eq!(Str::read(&mut Reader::new(&bytes)).unwrap(), input);
    }
}

#[test]
fn configuration_and_play_custom_payloads_share_identifier_decoding() {
    let mut body = wire("request_secret");
    body.extend_from_slice(&[0, 1, 255]);
    let cfg = configuration::Clientbound::decode(
        horizon_protocol::ids::configuration::clientbound::CUSTOM_PAYLOAD,
        &body,
    )
    .unwrap();
    let configuration::Clientbound::CustomPayload(payload) = cfg else {
        panic!("wrong packet")
    };
    assert_eq!(payload.channel, "minecraft:request_secret");
    assert_eq!(payload.data, [0, 1, 255]);
    let packet = play::Clientbound::decode(
        horizon_protocol::ids::play::clientbound::CUSTOM_PAYLOAD,
        &body,
    )
    .unwrap();
    let play::Clientbound::CustomPayload(payload) = packet else {
        panic!("wrong packet")
    };
    assert_eq!(payload.channel, "minecraft:request_secret");
    assert_eq!(payload.data, [0, 1, 255]);
    let invalid = wire("bad:CHANNEL");
    assert!(
        configuration::Clientbound::decode(
            horizon_protocol::ids::configuration::clientbound::CUSTOM_PAYLOAD,
            &invalid
        )
        .is_err()
    );
    assert!(
        play::Clientbound::decode(
            horizon_protocol::ids::play::clientbound::CUSTOM_PAYLOAD,
            &invalid
        )
        .is_err()
    );
}
