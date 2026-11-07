//! Handshake state: ClientIntentionPacket.
use crate::{
    buf::{StrN, UShort, VarInt},
    packet, packet_enum,
};

/// ClientIntent ids.
pub const STATUS: i32 = 1;
pub const LOGIN: i32 = 2;
pub const TRANSFER: i32 = 3;

packet!(Intention {
    protocol: i32 => VarInt,
    host: String => StrN<255>,
    port: u16 => UShort,
    intent: i32 => VarInt,
});

packet_enum!(Serverbound, crate::ids::handshake::serverbound { Intention(Intention) = INTENTION });

/// The host limit a server behind a proxy reads the handshake with: `Short.MAX_VALUE`, as Paper does with proxy
/// forwarding on, for the data a proxy puts after the host (Floodgate's). [`Intention`]'s own codec has vanilla's
/// 255.
pub const PROXY_HOST_LIMIT: usize = 32767;

/// The handshake with its host read up to `host_limit` characters; anything but an intention, or bytes left
/// over, is an error.
pub fn decode_intention(id: i32, payload: &[u8], host_limit: usize) -> crate::Result<Intention> {
    if id != crate::ids::handshake::serverbound::INTENTION {
        return Err(crate::Error::new(format!(
            "expected a handshake, got packet {id}"
        )));
    }
    let mut r = crate::buf::Reader::new(payload);
    let intention = Intention {
        protocol: r.varint()?,
        host: r.string(host_limit)?,
        port: r.u16()?,
        intent: r.varint()?,
    };
    if r.remaining() != 0 {
        return Err(crate::Error::new(format!(
            "packet {id} (Serverbound) was larger than expected: {} extra bytes",
            r.remaining()
        )));
    }
    Ok(intention)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intention_bytes() {
        let p = Serverbound::Intention(Intention {
            protocol: 777,
            host: "localhost".into(),
            port: 25565,
            intent: LOGIN,
        });
        let body = p.encode();
        assert_eq!(body[0], 0);
        assert_eq!(&body[1..3], &[0x89, 0x06]);
        assert_eq!(&body[body.len() - 3..], &[0x63, 0xdd, 2]);
        crate::packets::round_trip(p, Serverbound::encode, Serverbound::decode);
    }

    #[test]
    fn a_proxy_host_may_be_longer_than_vanillas() {
        let long = "a".repeat(PROXY_HOST_LIMIT);
        let p = Intention {
            protocol: 777,
            host: long,
            port: 25565,
            intent: LOGIN,
        };
        let body = Serverbound::Intention(p.clone()).encode();
        let (id, payload) = (i32::from(body[0]), &body[1..]);
        assert!(
            Serverbound::decode(id, payload).is_err(),
            "vanilla's limit is 255"
        );
        assert_eq!(decode_intention(id, payload, PROXY_HOST_LIMIT).unwrap(), p);
        assert!(decode_intention(id, payload, 255).is_err());
        let over = Serverbound::Intention(Intention {
            host: "a".repeat(PROXY_HOST_LIMIT + 1),
            ..p
        })
        .encode();
        assert!(decode_intention(id, &over[1..], PROXY_HOST_LIMIT).is_err());
    }
}
