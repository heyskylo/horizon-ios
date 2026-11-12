//! Login state: offline/online hello, encryption request, compression,
//! login plugin queries and the finished game profile.
use crate::{
    buf::{Bool, Bytes, Ident, Opt, RestN, Str, StrN, Uuid, VarInt},
    packet, packet_enum,
    packets::Empty,
};

packet!(
    /// com.mojang.authlib.properties.Property.
    Property {
        name: String => StrN<64>,
        value: String => Str,
        signature: Option<String> => Opt<StrN<1024>>,
    }
);
packet!(
    /// ByteBufCodecs.GAME_PROFILE.
    GameProfile {
        uuid: u128 => Uuid,
        name: String => StrN<16>,
        properties: Vec<Property> => crate::buf::List<Property>,
    }
);
packet!(Disconnect { reason_json: String => StrN<262144> });
packet!(Hello {
    server_id: String => StrN<20>,
    public_key: Vec<u8> => Bytes,
    challenge: Vec<u8> => Bytes,
    should_authenticate: bool => Bool,
});
packet!(LoginFinished { profile: GameProfile => GameProfile, session: u128 => Uuid });
packet!(Compression { threshold: i32 => VarInt });
/// `ClientboundCustomQueryPacket.MAX_PAYLOAD_SIZE` and `ServerboundCustomQueryAnswerPacket.MAX_PAYLOAD_SIZE`.
pub const MAX_QUERY_PAYLOAD: usize = 1_048_576;
packet!(CustomQuery {
    transaction: i32 => VarInt,
    channel: String => Ident,
    data: Vec<u8> => RestN<MAX_QUERY_PAYLOAD>,
});
packet!(CookieRequest { key: String => Ident });

packet_enum!(Clientbound, crate::ids::login::clientbound {
    Disconnect(Disconnect) = LOGIN_DISCONNECT,
    Hello(Hello) = HELLO,
    LoginFinished(LoginFinished) = LOGIN_FINISHED,
    Compression(Compression) = LOGIN_COMPRESSION,
    CustomQuery(CustomQuery) = CUSTOM_QUERY,
    CookieRequest(CookieRequest) = COOKIE_REQUEST,
});

packet!(ServerHello { name: String => StrN<16>, uuid: u128 => Uuid });
packet!(Key { key: Vec<u8> => Bytes, challenge: Vec<u8> => Bytes });
packet!(CustomQueryAnswer {
    transaction: i32 => VarInt,
    payload: Option<Vec<u8>> => Opt<RestN<MAX_QUERY_PAYLOAD>>,
});
packet!(CookieResponse { key: String => Ident, payload: Option<Vec<u8>> => Opt<Bytes> });

packet_enum!(Serverbound, crate::ids::login::serverbound {
    Hello(ServerHello) = HELLO,
    Key(Key) = KEY,
    CustomQueryAnswer(CustomQueryAnswer) = CUSTOM_QUERY_ANSWER,
    LoginAcknowledged(Empty) = LOGIN_ACKNOWLEDGED,
    CookieResponse(CookieResponse) = COOKIE_RESPONSE,
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::round_trip;

    #[test]
    fn login_round_trips() {
        let profile = GameProfile {
            uuid: crate::crypto::offline_uuid("Horizon"),
            name: "Horizon".into(),
            properties: vec![Property {
                name: "textures".into(),
                value: "e30=".into(),
                signature: None,
            }],
        };
        for p in [
            Clientbound::Disconnect(Disconnect {
                reason_json: "{\"text\":\"bye\"}".into(),
            }),
            Clientbound::Hello(Hello {
                server_id: String::new(),
                public_key: vec![1, 2],
                challenge: vec![3; 4],
                should_authenticate: true,
            }),
            Clientbound::LoginFinished(LoginFinished {
                profile,
                session: 7,
            }),
            Clientbound::Compression(Compression { threshold: 256 }),
            Clientbound::CustomQuery(CustomQuery {
                transaction: 3,
                channel: "velocity:player_info".into(),
                data: vec![1],
            }),
            Clientbound::CookieRequest(CookieRequest { key: "a:b".into() }),
        ] {
            round_trip(p, Clientbound::encode, Clientbound::decode);
        }
        for p in [
            Serverbound::Hello(ServerHello {
                name: "Horizon".into(),
                uuid: 1,
            }),
            Serverbound::Key(Key {
                key: vec![9; 128],
                challenge: vec![8; 128],
            }),
            Serverbound::CustomQueryAnswer(CustomQueryAnswer {
                transaction: 3,
                payload: None,
            }),
            Serverbound::LoginAcknowledged(Empty),
            Serverbound::CookieResponse(CookieResponse {
                key: "a:b".into(),
                payload: Some(vec![1]),
            }),
        ] {
            round_trip(p, Serverbound::encode, Serverbound::decode);
        }
    }

    #[test]
    fn rejects_trailing_bytes_and_long_names() {
        let mut body = Serverbound::LoginAcknowledged(Empty).encode();
        body.push(0);
        assert!(Serverbound::decode(3, &body[1..]).is_err());
        let long = Serverbound::Hello(ServerHello {
            name: "x".repeat(17),
            uuid: 0,
        })
        .encode();
        assert!(Serverbound::decode(0, &long[1..]).is_err());
    }

    /// `ClientboundCustomQueryPacket` and `ServerboundCustomQueryAnswerPacket` read what is left
    /// of the packet after their fixed fields as the payload, and refuse more than 1048576 bytes
    /// ("Payload may not be larger than 1048576 bytes"); exactly that many is read.
    #[test]
    fn a_login_query_payload_is_at_most_a_mebibyte() {
        const MAX: usize = 1_048_576;
        let query = |n: usize| {
            Clientbound::CustomQuery(CustomQuery {
                transaction: 3,
                channel: "velocity:player_info".into(),
                data: vec![7; n],
            })
            .encode()
        };
        let answer = |n: usize| {
            Serverbound::CustomQueryAnswer(CustomQueryAnswer {
                transaction: 3,
                payload: Some(vec![7; n]),
            })
            .encode()
        };
        let query_id = crate::ids::login::clientbound::CUSTOM_QUERY;
        let answer_id = crate::ids::login::serverbound::CUSTOM_QUERY_ANSWER;
        for n in [0, 1, MAX] {
            match Clientbound::decode(query_id, &query(n)[1..]).unwrap() {
                Clientbound::CustomQuery(q) => assert_eq!(q.data.len(), n),
                other => panic!("{other:?}"),
            }
            match Serverbound::decode(answer_id, &answer(n)[1..]).unwrap() {
                Serverbound::CustomQueryAnswer(a) => {
                    assert_eq!(a.payload.map(|p| p.len()), Some(n));
                }
                other => panic!("{other:?}"),
            }
        }
        // (Not `unwrap_err`: it would print the whole payload if one were read.)
        let Err(too_big) = Clientbound::decode(query_id, &query(MAX + 1)[1..]) else {
            panic!("a query payload one byte over the cap was read");
        };
        assert!(too_big.0.contains("1048576"), "{too_big:?}");
        let Err(too_big) = Serverbound::decode(answer_id, &answer(MAX + 1)[1..]) else {
            panic!("an answer payload one byte over the cap was read");
        };
        assert!(too_big.0.contains("1048576"), "{too_big:?}");
        // An answer with no payload is still read.
        let none = Serverbound::CustomQueryAnswer(CustomQueryAnswer {
            transaction: 3,
            payload: None,
        })
        .encode();
        assert!(Serverbound::decode(answer_id, &none[1..]).is_ok());
    }
}
