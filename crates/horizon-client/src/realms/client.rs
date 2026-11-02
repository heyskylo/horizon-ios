//! Minecraft Realms API client and data structures.
//! Communicates with the official Mojang Realms endpoints (`pc.realms.minecraft.net`).

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const DEFAULT_REALMS_HOST: &str = "https://pc.realms.minecraft.net";

/// A Minecraft Realm server representation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RealmServer {
    pub id: i64,
    #[serde(default)]
    pub remote_subscription_id: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub owner_uuid: Option<String>,
    pub name: String,
    #[serde(default)]
    pub motd: String,
    #[serde(default)]
    pub default_permission: String,
    pub state: String,
    #[serde(default)]
    pub days_left: i32,
    #[serde(default)]
    pub expired: bool,
    #[serde(default)]
    pub expired_trial: bool,
    #[serde(default)]
    pub world_type: String,
    #[serde(default)]
    pub active_slot: i32,
    #[serde(default)]
    pub minigame_name: Option<String>,
    #[serde(default)]
    pub minigame_id: Option<i32>,
    #[serde(default)]
    pub minigame_image: Option<String>,
}

impl RealmServer {
    pub fn is_open(&self) -> bool {
        self.state.eq_ignore_ascii_case("OPEN") && !self.expired
    }
}

/// A wrapper around server list response.
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct RealmServerList {
    pub servers: Vec<RealmServer>,
}

/// The connection address returned when joining a Realm.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RealmAddress {
    pub address: String,
    #[serde(default)]
    pub resource_pack_url: Option<String>,
    #[serde(default)]
    pub resource_pack_hash: Option<String>,
}

/// Client for making requests to the Realms API using the player's AuthSession.
pub struct RealmsClient<'a> {
    base_url: &'a str,
    player_name: &'a str,
    player_uuid: &'a str,
    access_token: &'a str,
}

impl<'a> RealmsClient<'a> {
    pub fn new(
        base_url: &'a str,
        player_name: &'a str,
        player_uuid: &'a str,
        access_token: &'a str,
    ) -> Self {
        Self {
            base_url,
            player_name,
            player_uuid,
            access_token,
        }
    }

    /// Default production client.
    pub fn default_client(
        player_name: &'a str,
        player_uuid: &'a str,
        access_token: &'a str,
    ) -> Self {
        Self::new(DEFAULT_REALMS_HOST, player_name, player_uuid, access_token)
    }

    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(10))
            .build()
    }

    /// Builds cookie string required by Realms API: `sid=token:<access_token>:<uuid>; user=<name>; version=26.3`.
    fn cookie_header(&self) -> String {
        let undashed_uuid = self.player_uuid.replace('-', "");
        format!(
            "sid=token:{}:{}; user={}; version=26.3",
            self.access_token, undashed_uuid, self.player_name
        )
    }

    /// Fetches the list of Realms available to the player.
    pub fn list_worlds(&self) -> Result<Vec<RealmServer>, String> {
        let url = format!("{}/worlds", self.base_url);
        let resp = match Self::agent()
            .get(&url)
            .set("Cookie", &self.cookie_header())
            .set("Authorization", &format!("Bearer {}", self.access_token))
            .set("User-Agent", "Horizon/26.3")
            .call()
        {
            Ok(resp) => resp,
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                let detail = if body.is_empty() {
                    format!("status code {code}")
                } else {
                    format!("{code}: {body}")
                };
                return Err(format!("Realms request failed: {url}: {detail}"));
            }
            Err(e) => return Err(format!("Realms network error: {e}")),
        };

        let body = resp
            .into_string()
            .map_err(|e| format!("Failed to read Realms response: {e}"))?;

        let list: RealmServerList = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse Realms server list: {e}"))?;

        Ok(list.servers)
    }

    /// Requests server address to connect to a Realm.
    pub fn join_world(&self, realm_id: i64) -> Result<RealmAddress, String> {
        let url = format!("{}/worlds/v1/{}/join/pc", self.base_url, realm_id);
        let resp = match Self::agent()
            .get(&url)
            .set("Cookie", &self.cookie_header())
            .set("Authorization", &format!("Bearer {}", self.access_token))
            .set("User-Agent", "Horizon/26.3")
            .call()
        {
            Ok(resp) => resp,
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                let detail = if body.is_empty() {
                    format!("status code {code}")
                } else {
                    format!("{code}: {body}")
                };
                return Err(format!("Realms join failed: {url}: {detail}"));
            }
            Err(e) => return Err(format!("Realms network error: {e}")),
        };

        let body = resp
            .into_string()
            .map_err(|e| format!("Failed to read Realms join response: {e}"))?;

        let addr: RealmAddress = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse Realms address: {e}"))?;

        Ok(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_realm_server_list() {
        let raw = r#"{
            "servers": [
                {
                    "id": 12345,
                    "name": "Test Realm",
                    "motd": "Welcome to my Realm!",
                    "state": "OPEN",
                    "daysLeft": 25,
                    "expired": false,
                    "worldType": "NORMAL",
                    "activeSlot": 1
                }
            ]
        }"#;

        let parsed: RealmServerList = serde_json::from_str(raw).expect("Failed to parse");
        assert_eq!(parsed.servers.len(), 1);
        let server = &parsed.servers[0];
        assert_eq!(server.id, 12345);
        assert_eq!(server.name, "Test Realm");
        assert_eq!(server.motd, "Welcome to my Realm!");
        assert!(server.is_open());
    }

    #[test]
    fn parse_realm_address() {
        let raw = r#"{
            "address": "192.168.1.100:25565"
        }"#;

        let parsed: RealmAddress = serde_json::from_str(raw).expect("Failed to parse address");
        assert_eq!(parsed.address, "192.168.1.100:25565");
    }
}
