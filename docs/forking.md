# Forking: Horizon's extras and the vanilla game

Horizon is the vanilla game plus a few things of its own: cosmetics, Play with Friends, the
official server in the list, its settings pages. In the code they sit behind one Cargo feature,
`horizon-extras`, on by default, so a fork can build without them or delete them. Players always
get the full Horizon: there is no setting, flag or variable that turns the extras off.
Optimizations are not extras; they are part of the game in every build.

## Building without Horizon's extras

```sh
cargo build --release -p horizon-client --no-default-features --features audio,controllers
```

Best effort: CI builds and tests the default only. The server build (`--no-default-features`,
docs/server.md) compiles the same gates, so the code keeps compiling, but nobody plays it as a
client before a release. [horizon-rust/horizon-plain](https://github.com/horizon-rust/horizon-plain)
builds it from each official release tag, unsigned.

What changes in that build:

| Extra | Where it lives | The gate |
|---|---|---|
| Cosmetics, emotes and the emote wheel, "Show me as a Horizon player" | `game/cosmetics.rs` (+ `cosmetics/fx.rs`, `cosmetics/emotes.rs`, `cosmetics/emote_wheel.rs`), crate `horizon-cosmetics` | `game::cosmetics::enabled()`: the service is never asked, nothing is drawn, B opens no wheel |
| The official server added to the server list | `multiplayer.rs` (`add_official_server`) | their call at startup (`game.rs`, `fn run`) |
| Horizon Settings (Extras, PvP & HUD, Splitscreen, Studio, AI Agent, Discord) and Mods (with the registry browser) | `options_ui.rs` (`Page::Horizon` and below), `extras_ui.rs`, `extras.rs`, `pvp.rs`, `game/extras.rs`, `game/tweaks.rs`, `extras_hud.rs` | the Options screen shows vanilla's greyed Telemetry Data instead (`options_ui::HORIZON_EXTRAS`, set in `entry.rs`) |
| Discord Rich Presence | `discord_presence.rs`, `game/discord.rs` | the `discord` feature, which `horizon-extras` turns on |
| Play with Friends | `game/friends.rs`, `friends_screen.rs`, crate `horizon-friends` | the `friends` feature (`game/friends_off.rs` stands in) |
| The anonymous "I'm playing" ping | `heartbeat.rs` | the `heartbeat` feature |

Credits & Attribution then says where the official build is (`horizon.plain.official`).

`grep -rn horizon-extras crates/` lists every gate. Apart from the three features above, the gates
are `cfg!(feature = "horizon-extras")`: both sides compile in every build, so no stub has to follow
the code it replaces. To remove an extra for good, delete its files from the table and the lines its
gate guards.

## Not behind the feature

- The launcher (`launcher/`) holds the Wardrobe, the Shop, the featured servers page, its themes,
  the telemetry consent and updates. A build of the game alone has none of them; without the
  launcher's consent the game keeps no crash or performance report (docs/telemetry.md).
- The graphics screen's fullbright, dynamic lights and hiding block-break particles: renderer
  options, off by default (docs/extras.md). A `horizon-extras.txt` left by an official install is
  still read.
- The mod runtime: nothing installs a mod without the launcher, so it stays idle.
- The `horizon` brand reply to a Horizon server, for its protocol extensions (`net.rs`).
- Agent control (`--control`, `horizon-ctl`), controllers, HDR, shader packs.
