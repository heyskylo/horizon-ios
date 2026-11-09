//! Players coming and going: joining (P1's world through memory, or P1's server) as a profile
//! it picks (`split_profiles`), dropping out.
use super::*;
use horizon_client::{
    controller_notice::{Notice, NoticeId, Priority},
    gamepad::PadState,
    options_ui::Page,
};

/// Whether `game` shows its profile page now.
fn on_profiles(game: &Game<'_>) -> bool {
    game.screen == crate::game::Screen::Options && game.ui.options.page() == Page::SplitProfiles
}

/// An offline profile on an online-mode server: the session service knows no such player.
const ONLINE_ONLY: &str = "horizon.splitscreen.online_only";
/// An offline profile on a server outside this computer and its local network.
const LOCAL_ONLY: &str = "horizon.splitscreen.local_only";

/// A loopback, private (RFC 1918, IPv6 unique local) or link-local address: this computer or
/// its local network.
fn local_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => local_ip(IpAddr::V4(v4)),
            None => {
                let first = v6.segments()[0];
                v6.is_loopback() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
            }
        },
    }
}

/// Whether the server at `address` is on this computer or its local network, where an offline
/// profile may join P1 (the project's rule: everyone who plays elsewhere owns the game). It
/// resolves as the guest's connection will (the SRV redirect, then every address of its host)
/// and needs all of them local; a friend's invite or an address that doesn't resolve isn't.
fn local_server(address: &str) -> bool {
    use std::net::ToSocketAddrs;
    if address.starts_with(horizon_client::net::FRIEND) {
        return false;
    }
    let Some((host, port)) = horizon_protocol::servers::parse_address(address) else {
        return false;
    };
    let (host, port) = horizon_protocol::server_redirect(&host, port);
    match (host.as_str(), port).to_socket_addrs() {
        Ok(addresses) => {
            let addresses: Vec<_> = addresses.collect();
            !addresses.is_empty() && addresses.iter().all(|a| local_ip(a.ip()))
        }
        Err(_) => false,
    }
}

/// Why a player's game left its server, when the server said (its Disconnected or error screen).
fn turned_away(game: &Game<'_>) -> Option<String> {
    use horizon_client::{multiplayer::Screen, title::Mode};
    let why = match &game.title.as_ref()?.mode {
        Mode::Multiplayer(Screen::Disconnected { reason, .. }) => reason.clone(),
        Mode::Error { message, .. } => message.clone(),
        _ => return None,
    };
    // An online-mode server: the other players have no Minecraft account of their own here.
    Some(if why.contains("verify username") {
        horizon_client::lang::tr("horizon.splitscreen.own_account", &[])
    } else {
        why
    })
}

/// The `Game` `start` makes (~67 KB, by value) into a box, in a frame of its own that is gone
/// once it is: the copies stay out of the caller's frame, deep in the event loop.
#[inline(never)]
fn boxed(start: impl FnOnce() -> Result<Option<Game<'static>>>) -> Result<Box<Game<'static>>> {
    match start() {
        Ok(Some(game)) => Ok(Box::new(game)),
        Ok(None) => Err("the player's game did not start".into()),
        Err(e) => Err(e),
    }
}

/// P1's server lets a splitscreen player in (#6399: everyone who plays elsewhere owns the
/// game): `account`, its launcher Microsoft session (whether that has expired), else an offline
/// profile, `unavailable` when that name is a launcher account that couldn't be handed over.
/// An offline profile joins only a server on this computer or its local network
/// (`local`, resolved only for it) in offline mode; a Microsoft account any server while its
/// session holds. The refusal is its translation key.
fn server_gate(
    account: Option<bool>,
    unavailable: bool,
    local: impl FnOnce() -> bool,
    online: bool,
) -> std::result::Result<(), &'static str> {
    match account {
        Some(false) => Ok(()),
        Some(true) => Err("horizon.splitscreen.account_expired"),
        None if !local() => Err(LOCAL_ONLY),
        None if !online => Ok(()),
        None if unavailable => Err("horizon.splitscreen.account_unavailable"),
        None => Err(ONLINE_ONLY),
    }
}

impl Split<'_> {
    /// The first seat free (2 to `MAX_PLAYERS`).
    pub(super) fn free_seat(&self) -> Option<usize> {
        (2..=MAX_PLAYERS).find(|n| self.guests.iter().all(|g| g.seat != *n))
    }

    /// The splitscreen folder in the game folder: `profiles/<name>/` holds each profile.
    fn folder() -> Result<PathBuf> {
        Ok(std::env::current_dir()?.join("splitscreen"))
    }

    /// Add a player driven by `pad`: it picks a profile first, unless `name` is given (tests).
    pub(super) fn join(&mut self, pad: Option<PadId>, name: Option<String>) -> Result<()> {
        let seat = self.free_seat().ok_or("every seat is taken")?;
        // The first other player: P1 moves to the compositor now (back with nobody joined, if
        // this one can't: `about_to_wait`).
        if self.surfaces.is_empty() {
            self.enable()?;
        }
        // Until it has a profile, a scratch options file: never P1's.
        let options = Self::folder()?.join(".joining/options.txt");
        OPTIONS.set(Some(options.clone()));
        horizon_hooks::engine::set_seat(seat - 1);
        let mut settings = self.host.settings.clone();
        if pad.is_some() {
            // The Controller setting is P1's own: a player who pressed a pad to join plays
            // with it even when P1 turned controllers off, or its pad would be dead on the
            // profile screen it joins into.
            settings.controller.enabled = true;
        }
        let game = self.start_guest(seat, &options, settings);
        OPTIONS.set(None);
        horizon_hooks::engine::set_seat(0);
        let mut game = game?;
        if let Some(pad) = pad {
            // The press that joined it doesn't act in its game.
            self.fresh.push(pad);
        }
        // Over its title screen: Cancel leaves (`pick_profiles`).
        game.open_options_at(true, Page::SplitProfiles);
        let i = self.guests.len() + 1;
        self.guests.push(Guest {
            game,
            name: None,
            seat,
            options,
            pad,
            window: 1,
            widgets: Default::default(),
            entered: false,
            bumpers: [false; 2],
        });
        self.planned_window(i, seat);
        if let Some(name) = name {
            self.enter(i - 1, name)?;
        }
        Ok(())
    }

    /// The names playing now: P1's and the joined players'.
    fn taken(&self) -> Vec<String> {
        std::iter::once(self.host.options.player_name())
            .chain(self.guests.iter().filter_map(|g| g.name.clone()))
            // On a server: everyone in its player list.
            .chain(self.host.remote_online_names().unwrap_or_default())
            .collect()
    }

    /// Whether `name` can play in P1's world, said on the profile page before it tries: P1's
    /// own world takes anyone; offline profiles require a local, offline-mode server, while
    /// Microsoft accounts require a valid session.
    fn can_join(&self, name: &str) -> std::result::Result<(), String> {
        if self.taken().iter().any(|n| n.eq_ignore_ascii_case(name)) {
            return Err(format!("{name} is already playing."));
        }
        let Some(address) = self.p1_server() else {
            return Ok(());
        };
        let account = self
            .accounts
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name));
        let unavailable = launcher::unavailable()
            .iter()
            .any(|a| a.eq_ignore_ascii_case(name));
        server_gate(
            account.map(|a| a.expired()),
            unavailable,
            || local_server(&address),
            self.host.online_server(),
        )
        .map_err(|key| horizon_client::lang::tr(key, &[&name]))
    }

    /// A server's resource packs: the others answer as P1 did (asked once, by P1). A player
    /// joining after P1 answered is answered at once; one asked before waits for P1's answer.
    pub(super) fn follow_pack_answer(&mut self) {
        let Some(yes) = self.host.pack_answer() else {
            return;
        };
        for i in 0..self.guests.len() {
            if !self.guests[i].game.pack_prompt_open() {
                continue;
            }
            self.turn(i + 1);
            if let Err(e) = self.guests[i].game.answer_pack_prompt(yes) {
                log::warn!(
                    "Horizon: splitscreen: P{}'s server packs: {e}",
                    self.guests[i].seat
                );
            }
        }
        self.turn(0);
    }

    /// The profile pages just opened, filled: the list and who plays now.
    pub(super) fn sync_profiles(&mut self) {
        let taken = self.taken();
        for g in self.guests.iter_mut().filter(|g| on_profiles(&g.game)) {
            g.game.ui.options.split_profiles.taken = taken.clone();
        }
        let open = |g: &Guest<'_>| on_profiles(&g.game) && !g.game.ui.options.split_profiles.filled;
        if !self.guests.iter().any(open) {
            return;
        }
        let (names, accounts) = self.profile_names().unwrap_or_else(|e| {
            log::warn!("Horizon: splitscreen: the profiles: {e}");
            Default::default()
        });
        for g in self.guests.iter_mut().filter(|g| open(g)) {
            let page = &mut g.game.ui.options.split_profiles;
            page.filled = true;
            page.names = names.clone();
            if let Some(me) = g.name.as_ref().filter(|me| !names.contains(me)) {
                page.names.insert(0, me.clone());
            }
            page.taken = taken.clone();
            page.accounts = accounts.clone();
            page.offline_accounts = launcher::unavailable();
            page.offline_accounts.extend(
                self.accounts
                    .iter()
                    .filter(|a| a.expired())
                    .map(|a| a.name.clone()),
            );
            page.current = g.name.clone();
        }
    }

    /// The launcher's other Microsoft accounts, then this instance's offline profiles.
    fn profile_names(&self) -> Result<(Vec<String>, Vec<String>)> {
        let profiles = Self::folder()?.join("profiles");
        let mut offline = horizon_client::split_profiles::store::names(&profiles)?;
        let mut accounts: Vec<String> = self.accounts.iter().map(|a| a.name.clone()).collect();
        accounts.extend(launcher::unavailable());
        offline.retain(|n| !accounts.iter().any(|a| a.eq_ignore_ascii_case(n)));
        offline.sort_by_key(|n| n.to_lowercase());
        let mut names = accounts.clone();
        names.extend(offline);
        Ok((names, accounts))
    }

    /// Guest `i`, playing, switches to profile `name` where it plays: out of the world as its
    /// old profile, in as the new one.
    fn switch(&mut self, i: usize, name: String) -> Result<()> {
        self.turn(i + 1);
        let left = self.guests[i].game.leave_server(None);
        self.turn(0);
        let old = self.guests[i].name.take();
        if let (Some(server), Some(old)) = (&self.host.integrated, &old) {
            server.leave_split(old);
        }
        left?;
        self.enter(i, name)
    }

    /// Guest `i` plays as profile `name`: its options, then into P1's world.
    pub(super) fn enter(&mut self, i: usize, name: String) -> Result<()> {
        self.can_join(&name)?;
        // The page's names are player names; a test seat's (P2) isn't one, but no name leaves
        // the profiles folder.
        if !folder_name(&name) {
            return Err(format!("{name:?} can't be a profile.").into());
        }
        let profiles = Self::folder()?.join("profiles");
        // A Microsoft account signs in as itself (its own connections and skin).
        let account = self
            .accounts
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(&name))
            .cloned();
        self.guests[i].game.split_auth = account.clone();
        if account.is_none() && horizon_client::split_profiles::valid_name(&name) {
            horizon_client::split_profiles::store::restore(&profiles, &name)?;
        }
        let profile = profiles.join(&name);
        let options = profile.join("options.txt");
        let made = if options.exists() {
            None
        } else {
            std::fs::create_dir_all(&profile)?;
            let mut new = self.host.settings.clone();
            // The Controller setting is P1's, as on the profile page (`join`).
            new.controller.enabled |= self.guests[i].pad.is_some();
            // On P1's seat, so the crates' settings stores write P1's values too.
            let turn = horizon_hooks::seat();
            horizon_hooks::engine::set_seat(0);
            new.save(&options);
            horizon_hooks::engine::set_seat(turn);
            Some(new)
        };
        // The crates' stores load into the guest's seat, before its turn starts below.
        let turn = horizon_hooks::seat();
        horizon_hooks::engine::set_seat(self.guests[i].seat - 1);
        let mut settings = match made {
            Some(new) => {
                horizon_client::settings::Settings::load_stores(&options);
                new
            }
            None => horizon_client::settings::Settings::load(&options),
        };
        horizon_hooks::engine::set_seat(turn);
        // A profile an earlier join saved with P1's off, or the player's pad would be dead.
        settings.controller.enabled |= self.guests[i].pad.is_some();
        // P1's own world through memory (`Ok`), else P1's server over the network (`Err`).
        let via = match &self.host.integrated {
            Some(server) => Ok(server
                .join_split(&name)
                .ok_or("P1's world does not let players in")?),
            None => Err(self.p1_server().ok_or("P1 is not in a world")?),
        };
        self.guests[i].options = options;
        self.turn(i + 1);
        let guest = &mut self.guests[i];
        // Its profile page closes as it goes in.
        guest.game.screen = crate::game::Screen::Playing;
        settings.extras.agent_control &= guest.game.options.control.is_some();
        guest.game.settings = settings;
        guest.game.options.player_name = Some(name.clone());
        guest.game.apply_settings();
        let joined = match via {
            Ok(pipe) => guest.game.connect_local(pipe, &name),
            // A server that refuses it (whitelist, an online-mode sign-in) disconnects it: it
            // picks again, told why (`drop_out`).
            Err(address) => guest.game.connect_server(&address, &name),
        };
        self.turn(0);
        if let Err(e) = joined {
            // Let in for nothing: the world pauses again, the name is free.
            if let Some(server) = &self.host.integrated {
                server.leave_split(&name);
            }
            return Err(e);
        }
        // The seat plays P1's world, so mods keep its data with P1's.
        let save = self
            .host
            .remote
            .as_ref()
            .and_then(|r| r.save_folder.clone());
        if let Some(r) = self.guests[i].game.remote.as_mut() {
            r.save_folder = save;
        }
        self.guests[i].name = Some(name);
        self.guests[i].entered = false;
        Ok(())
    }

    /// Players' profile pages: a pick goes in, one that can't says why there; a forgotten
    /// profile is hidden; a joining player who cancels (or closes the page) leaves.
    pub(super) fn pick_profiles(&mut self) -> bool {
        let mut changed = false;
        let mut i = 0;
        while i < self.guests.len() {
            if let Some(name) = self.guests[i]
                .game
                .ui
                .options
                .split_profiles
                .take_forgotten()
            {
                let signed_in = self
                    .accounts
                    .iter()
                    .any(|a| a.name.eq_ignore_ascii_case(&name));
                let signed_in = signed_in
                    || launcher::unavailable()
                        .iter()
                        .any(|a| a.eq_ignore_ascii_case(&name));
                let hidden = if signed_in
                    || self.taken().iter().any(|n| n.eq_ignore_ascii_case(&name))
                {
                    Err("This profile cannot be forgotten while it is playing or signed in.".into())
                } else {
                    Self::folder().and_then(|f| {
                        Ok(horizon_client::split_profiles::store::hide(
                            &f.join("profiles"),
                            &name,
                        )?)
                    })
                };
                match hidden {
                    Ok(()) => {
                        for guest in &mut self.guests {
                            guest.game.ui.options.split_profiles.filled = false;
                        }
                    }
                    Err(e) => self.refuse(i, e.to_string()),
                }
            }
            let joining = self.guests[i].name.is_none();
            match self.guests[i].game.ui.options.split_profiles.take_choice() {
                Some(Some(name)) => match (|| {
                    self.can_join(&name)?;
                    if joining {
                        self.enter(i, name)
                    } else {
                        self.switch(i, name)
                    }
                })() {
                    Ok(()) => changed = true,
                    Err(why) => self.refuse(i, why.to_string()),
                },
                _ if joining && !on_profiles(&self.guests[i].game) => {
                    self.remove_guest(i);
                    changed = true;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        changed
    }

    /// Guest `i`'s pick can't play: its profile page says why (opened again if it had left the
    /// world for it).
    fn refuse(&mut self, i: usize, why: String) {
        log::warn!("Horizon: splitscreen: P{}: {why}", self.guests[i].seat);
        let game = &mut self.guests[i].game;
        if !on_profiles(game) {
            game.open_options_at(true, Page::SplitProfiles);
        }
        game.ui.options.split_profiles.refused = Some(why);
    }

    /// A new player's game in its seat. Boxed as it comes out of `start_game`, and never
    /// inlined: a `Game` is ~67 KB, and copies of it in the event loop's frames overflowed
    /// Windows' 1 MB main stack.
    #[inline(never)]
    fn start_guest(
        &mut self,
        seat: usize,
        options_file: &Path,
        settings: horizon_client::settings::Settings,
    ) -> Result<Box<Game<'static>>> {
        // The players' games borrow P1's resources for the process's lifetime: one copy for all.
        if self.res.is_none() {
            self.res = Some(Box::leak(Box::new(self.host.res.clone())));
        }
        let res = self.res.ok_or("no resources")?;
        let host = &mut self.host;
        std::fs::create_dir_all(options_file.parent().ok_or("bad options path")?)?;
        let mut options = host.options.clone();
        options.world = None;
        options.server = None;
        // P1's frame-limited test runs end the process; a player's game doesn't.
        options.frames = None;
        options.world_frames = None;
        options.split_guest = true;
        options.offscreen = true;
        options.paced_by_split = true;
        // P1's socket (`--control` or Agent Control) + the seat: every game in this process
        // would otherwise take the one default endpoint, P1's.
        options.control = host.control.as_ref().map(|c| {
            let mut p = c.path().as_os_str().to_owned();
            p.push(seat.to_string());
            p.into()
        });
        let mut settings = settings;
        settings.extras.agent_control &= options.control.is_some();
        // `options.resources` is P1's overlay when a pack is on: never the vanilla root, as a
        // reload would rebuild that overlay out of itself.
        let vanilla_root = host.vanilla_root();
        let gui = UiResources::load(&options.resources, &vanilla_root)?;
        // P1's output, heard from this player's camera too (`Audio::share`).
        let audio = host.audio.share(seat as u8);
        let window = host.window.clone().ok_or("no window")?;
        let pack = host.pack.clone();
        let mut guest = boxed(|| {
            start_game(
                res,
                pack,
                gui,
                options,
                None,
                audio,
                vanilla_root,
                Instant::now(),
                None,
                Some(settings),
                None,
                None,
            )
        })?;
        guest.renderer = Some(build_renderer(
            window.clone(),
            true,
            &guest.pack,
            &guest.gui,
            host.renderer.as_ref(),
            None,
            host.settings.horizon.api,
            host.settings.horizon.translucent_batching,
            &host.settings.horizon,
            // The pack its own game loads, from its profile.
            crate::game::shader_packs::picked_pack_shape(&guest.settings),
            host.renderer
                .as_ref()
                .map(|r| r.display_reading())
                .unwrap_or_default(),
            (
                guest.settings.file.int("textureFiltering"),
                guest.settings.file.int("maxAnisotropyBit"),
            ),
        )?);
        guest.window = Some(window);
        guest.world_renderer_ready = true;
        guest.keyboard_mouse = false;
        guest.split_window = host.split_window;
        guest.apply_settings();
        // Its pad comes from the split (`route_pads`): it never opens the controllers itself.
        guest.pad.feed = Some(PadState::default());
        guest.ui.options.split_guest = true;
        Ok(guest)
    }

    /// Players who disconnected drop out; all of them once P1 has left its world. One the
    /// server turned away as it came in picks again, told why; one it sent away later (a kick)
    /// leaves, and P1 hears why. Whether any left.
    pub(super) fn drop_out(&mut self) -> bool {
        // Out of its world: on the title screen, or neither on its server thread nor a server's.
        let p1_left = self.host.title.is_some()
            || (self.host.integrated.is_none() && self.host.remote.is_none());
        let before = self.guests.len();
        if p1_left {
            self.host.split_hud.notice.clear();
        }
        for k in (0..self.guests.len()).rev() {
            let g = &mut self.guests[k];
            g.entered |= g.game.remote.is_some() && g.game.player.is_some();
            // In the world: a pad P1 gave up is this player's for good.
            if g.entered
                && let Some(id) = g.pad
            {
                self.p1_pad.joined(id);
            }
            let g = &self.guests[k];
            // A player picking a profile has no connection yet.
            let playing = g.name.is_none() || g.game.remote.is_some();
            let why = (!p1_left && g.game.error.is_none() && !playing)
                .then(|| turned_away(&g.game))
                .flatten();
            if let (true, Some(name), Some(why)) = (g.entered, &g.name, &why) {
                self.host.split_hud.notice.raise(Notice::new(
                    NoticeId::PlayerLeft,
                    Priority::Attention,
                    format!("{name} could not stay"),
                    why.clone(),
                ));
            } else if let Some(why) = why {
                let old = self.guests[k].name.take();
                if let (Some(server), Some(old)) = (&self.host.integrated, &old) {
                    server.leave_split(old);
                }
                self.refuse(k, why);
                continue;
            }
            if p1_left || !playing || g.game.error.is_some() {
                self.remove_guest(k);
            }
        }
        self.guests.len() != before
    }

    /// Guest `k` (player `k + 1`) leaves: keyboard and mouse stay with whoever had them (P1 if
    /// it was this one), and the server forgets the name (the world may pause again).
    pub(super) fn remove_guest(&mut self, k: usize) {
        // Its turn's buttons and options go back to their owners before the order shifts.
        self.turn(0);
        if self.focus == k + 1 {
            self.handoff(0);
            self.turn(0);
        }
        let guest = self.guests.remove(k);
        // Gone before it got into the world: a pad P1 gave up goes back to P1.
        if !guest.entered
            && let Some(id) = guest.pad
        {
            self.restore_given_up_pad(id);
        }
        if self.focus > k + 1 {
            self.focus -= 1;
        }
        if let (Some(server), Some(name)) = (&self.host.integrated, &guest.name) {
            server.leave_split(name);
        }
    }
}

/// A name that stays one folder inside `profiles`.
fn folder_name(name: &str) -> bool {
    !name.is_empty() && name != "." && !name.contains(['/', '\\']) && !name.contains("..")
}

#[cfg(test)]
mod tests {
    use super::{LOCAL_ONLY, ONLINE_ONLY, folder_name, local_server, server_gate};

    #[test]
    fn a_profile_name_never_leaves_the_profiles_folder() {
        assert!(folder_name("P2") && folder_name("Steve"));
        for bad in ["", ".", "..", "../x", "a/b", "a\\b"] {
            assert!(!folder_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn offline_guests_join_only_local_servers() {
        for local in [
            "127.0.0.1:25565",
            "localhost:25570",
            "192.168.1.20",
            "10.0.0.5:25566",
            "172.20.1.2",
            "169.254.3.4",
            "[::1]:25565",
            "[fd12:3456::1]:25565",
            "[fe80::1]:25565",
        ] {
            assert!(local_server(local), "{local} is local");
        }
        for elsewhere in [
            "8.8.8.8:25565",
            "172.32.0.1",
            "[2001:db8::1]:25565",
            "[::ffff:1.1.1.1]:25565",
            "friend:K7QM-3XPD",
            "",
        ] {
            assert!(!local_server(elsewhere), "{elsewhere} is not local");
        }
    }

    #[test]
    fn only_a_microsoft_session_lifts_the_local_server_rule() {
        const EXPIRED: &str = "horizon.splitscreen.account_expired";
        const UNAVAILABLE: &str = "horizon.splitscreen.account_unavailable";
        // (account: None offline, Some(expired)), unavailable, local, online => the answer.
        let cases: [(Option<bool>, bool, bool, bool, Result<(), &str>); 11] = [
            // An offline profile: home-network offline-mode servers only.
            (None, false, true, false, Ok(())),
            (None, false, false, false, Err(LOCAL_ONLY)),
            (None, false, false, true, Err(LOCAL_ONLY)),
            (None, false, true, true, Err(ONLINE_ONLY)),
            // A launcher account that couldn't be handed over plays as an offline profile.
            (None, true, false, false, Err(LOCAL_ONLY)),
            (None, true, true, true, Err(UNAVAILABLE)),
            (None, true, true, false, Ok(())),
            // Its own Microsoft session: any server, while it holds.
            (Some(false), false, false, false, Ok(())),
            (Some(false), false, false, true, Ok(())),
            (Some(true), false, false, true, Err(EXPIRED)),
            (Some(true), false, true, false, Err(EXPIRED)),
        ];
        for (account, unavailable, local, online, want) in cases {
            assert_eq!(
                server_gate(account, unavailable, || local, online),
                want,
                "{account:?} unavailable={unavailable} local={local} online={online}"
            );
        }
        // A Microsoft session never needs the address resolved.
        assert_eq!(
            server_gate(Some(false), false, || unreachable!(), true),
            Ok(())
        );
    }
}
