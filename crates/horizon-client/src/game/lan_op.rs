//! `horizon-ctl lan op`: a connected guest's operator level, which remote profiling checks.
use super::Game;
use horizon_client::Result;

impl Game<'_> {
    /// `horizon-ctl lan op NAME LEVEL`; `None` for any other command.
    pub(super) fn lan_op_control(&mut self, words: &[&str]) -> Result<Option<serde_json::Value>> {
        use serde_json::json;
        if words.first() != Some(&"lan") || words.get(1) != Some(&"op") {
            return Ok(None);
        }
        let usage = "lan op NAME LEVEL (0 to 4)";
        let (Some(name), Some(level)) = (
            words.get(2),
            words
                .get(3)
                .and_then(|l| l.parse::<u8>().ok())
                .filter(|l| *l <= 4),
        ) else {
            return Err(usage.into());
        };
        let name = (*name).to_owned();
        let found = match &self.integrated {
            Some(server) => server
                .call(move |g| g.lan_set_op(&name, level))
                .ok_or(super::integrated::NO_ANSWER)?,
            None => self.lan_set_op(&name, level),
        };
        if found {
            Ok(Some(json!({ "ok": true })))
        } else {
            Err("no guest of that name is connected".into())
        }
    }

    /// `horizon-ctl lan op`: the connected guest `name` gets operator level `level`; false when none is called that.
    pub(super) fn lan_set_op(&mut self, name: &str, level: u8) -> bool {
        let Some(host) = self.lan.as_mut() else {
            return false;
        };
        let Some(uuid) = host
            .hub
            .guests
            .iter()
            .find(|g| g.name == name)
            .map(|g| g.uuid)
        else {
            return false;
        };
        host.hub.ops.insert(uuid, level);
        host.hub.resend_commands();
        true
    }
}
