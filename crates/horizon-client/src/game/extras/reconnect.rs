//! Auto-reconnect (`autoReconnect`, docs/extras.md): the connection's start, join and end reach
//! the tracker (`horizon_client::reconnect`), and the Disconnected screen counts down to the next
//! attempt. The engine also reads the last server from it (`server_address`, `server_is_lan`).
use crate::game::Game;
use horizon_client::{multiplayer, reconnect::Retry, title::Mode};

impl Game<'_> {
    /// Auto-reconnect: a connection to `address` as `name` starts.
    pub(in crate::game) fn extras_connecting(&mut self, address: &str, name: &str, lan: bool) {
        self.extras.reconnect.connecting(address, name, lan);
    }
    /// The server this game connected to last.
    pub(in crate::game) fn server_address(&self) -> Option<&str> {
        self.extras.reconnect.address()
    }
    /// The server this game connected to last was found on the local network.
    pub(in crate::game) fn server_is_lan(&self) -> bool {
        self.extras.reconnect.lan()
    }
    /// Auto-reconnect: the server let the player in.
    pub(in crate::game) fn extras_joined(&mut self) {
        self.extras.reconnect.joined(std::time::Instant::now());
    }
    /// Auto-reconnect: the player left the server on purpose.
    pub(in crate::game) fn extras_left(&mut self) {
        self.extras.reconnect.reset();
    }
    /// Auto-reconnect's next attempt after a disconnect, when it is on and
    /// the reason allows one.
    pub(in crate::game) fn reconnect_after(
        &mut self,
        reason: &str,
        key: Option<&str>,
        from_server: bool,
    ) -> Option<Retry> {
        if !self.settings.extras.auto_reconnect {
            self.extras.reconnect.reset();
            return None;
        }
        let now = std::time::Instant::now();
        self.extras.reconnect.ended(reason, key, from_server, now)
    }
    /// Show the attempt's countdown on the Disconnected screen.
    pub(in crate::game) fn offer_reconnect(&mut self, retry: Option<Retry>) {
        let Some(retry) = retry else {
            return;
        };
        if let Some(title) = self.title.as_mut()
            && let Mode::Multiplayer(multiplayer::Screen::Disconnected { retry: slot, .. }) =
                &mut title.mode
        {
            log::info!(
                "Horizon: reconnecting to {} in {} s",
                horizon_client::net::shown_address(&retry.address),
                retry.seconds_left(std::time::Instant::now())
            );
            *slot = Some(retry);
        }
    }
    /// Auto-reconnect: connect again once the countdown is over. An
    /// online-mode server gets the signed-in account again: `connect_server`
    /// takes it from `auth::current()`, held in memory for the whole run
    /// (the launcher's session file is gone after the first read).
    pub(in crate::game) fn extras_retry(&mut self) -> horizon_client::Result<()> {
        if let Some(retry) = self.title.as_mut().and_then(|t| t.due_retry()) {
            self.connect_server_with_options(
                &retry.address,
                &retry.name,
                None,
                retry.lan,
                self.realm_session,
            )?;
        }
        Ok(())
    }
}
