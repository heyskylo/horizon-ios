use crate::auth;
use serde_json::Value;

/// authlib's `FriendsService.ResultCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultCode {
    Success,
    Error,
    ServiceNotAvailable,
    TooManyRequests,
    Forbidden,
    UpgradeNeeded,
    ConnectionIssue,
    TemporaryUnavailable,
    UnknownProfile,
    Unauthorized,
    GenericError,
}

/// `FriendDto`: one friend or one pending request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Friend {
    pub id: u128,
    pub name: String,
}

/// authlib's `FriendData`: the three lists the service answers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FriendData {
    pub friends: Vec<Friend>,
    pub incoming: Vec<Friend>,
    pub outgoing: Vec<Friend>,
}

/// authlib's `PresenceStatus`, as `updatePresence` stores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceStatus {
    Online,
    PlayingOffline,
    PlayingRealms,
    PlayingServer,
    PlayingHostedServer,
    Offline,
}
impl PresenceStatus {
    /// `PresenceStatus.name()`: the string `updatePresence` sends and
    /// `PresenceStatusDto.status` carries.
    pub(super) fn wire(self) -> &'static str {
        match self {
            Self::Online => "ONLINE",
            Self::PlayingOffline => "PLAYING_OFFLINE",
            Self::PlayingRealms => "PLAYING_REALMS",
            Self::PlayingServer => "PLAYING_SERVER",
            Self::PlayingHostedServer => "PLAYING_HOSTED_SERVER",
            Self::Offline => "OFFLINE",
        }
    }
}

/// `PresenceStatusDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presence {
    pub id: u128,
    pub status: PresenceStatus,
}

/// authlib's `FriendsListResponse` (`getFriends`/`updateFriends` body).
pub fn parse_friends(body: &str) -> Option<FriendData> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(FriendData {
        friends: parse_friend_list(v.get("friends")),
        incoming: parse_friend_list(v.get("incomingRequests")),
        outgoing: parse_friend_list(v.get("outgoingRequests")),
    })
}

fn parse_friend_list(value: Option<&Value>) -> Vec<Friend> {
    value
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|friend| {
            Some(Friend {
                id: auth::parse_uuid(friend.get("profileId")?.as_str()?)?,
                name: friend.get("name")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

/// authlib's `PresenceResponse`.
pub fn parse_presence(body: &str) -> Option<Vec<Presence>> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(presence_of(&v))
}

/// `PresenceResponse.presence`: a missing list reads as empty, as Gson's null does.
pub(super) fn presence_of(v: &Value) -> Vec<Presence> {
    v.get("presence")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let status = entry.get("status")?.as_str()?;
            let status = match status {
                "ONLINE" => PresenceStatus::Online,
                "PLAYING_OFFLINE" => PresenceStatus::PlayingOffline,
                "PLAYING_REALMS" => PresenceStatus::PlayingRealms,
                "PLAYING_SERVER" => PresenceStatus::PlayingServer,
                "PLAYING_HOSTED_SERVER" => PresenceStatus::PlayingHostedServer,
                _ => PresenceStatus::Offline,
            };
            Some(Presence {
                id: auth::parse_uuid(entry.get("profileId")?.as_str()?)?,
                status,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_friends_answer_reads_its_three_lists() {
        let data = parse_friends(
            r#"{"friends":[{"profileId":"0123456789abcdef0123456789abcdef","name":"Alex"}],
                "incomingRequests":[],"outgoingRequests":[{"profileId":"fedcba9876543210fedcba9876543210","name":"Steve"}]}"#,
        )
        .unwrap();
        assert_eq!(
            data.friends,
            vec![Friend {
                id: 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
                name: "Alex".into()
            }]
        );
        assert!(data.incoming.is_empty());
        assert_eq!(data.outgoing[0].name, "Steve");
        // The service's answer to an account with no friends, as the live check
        // captured it: the three lists plus an `empty` flag the parser ignores.
        let empty = parse_friends(
            r#"{"empty":true,"friends":[],"incomingRequests":[],"outgoingRequests":[]}"#,
        )
        .unwrap();
        assert_eq!(empty, FriendData::default());
    }

    #[test]
    fn a_presence_answer_reads_each_status() {
        let presence = parse_presence(
            r#"{"presence":[
                {"profileId":"0123456789abcdef0123456789abcdef","pmid":"00000000000000000000000000000000","status":"PLAYING_REALMS","lastUpdated":"2026-11-08T12:00:00Z"},
                {"profileId":"fedcba9876543210fedcba9876543210","pmid":"00000000000000000000000000000000","status":"OFFLINE","lastUpdated":"2026-11-08T12:00:00Z"}]}"#,
        )
        .unwrap();
        assert_eq!(presence[0].status, PresenceStatus::PlayingRealms);
        assert_eq!(presence[1].status, PresenceStatus::Offline);
        // `PresenceResponse` without its list is empty, not missing.
        assert_eq!(parse_presence("{}"), Some(Vec::new()));
        assert_eq!(parse_presence("not json"), None);
    }
}
