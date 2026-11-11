# Agent control: driving the launcher and the game

A player's own AI agent (Claude Code, Codex, Cursor, any MCP client) or a script can do
everything a player does with the mouse and keyboard, in the launcher and in the game. This page
covers developers: the surfaces, the full inventory, and the rules every new command follows.
Players read [Control Horizon with your AI agent](https://playhorizon.dev/docs/agents)
(playhorizon.dev/docs/agents).

The command lists are generated, so they can't drift:

- [launcher-commands.md](launcher-commands.md): every `horizon-launcher` command, its arguments
  and its MCP tool. It comes from the CLI's clap definition (`cli_tools::markdown`), and the
  launcher test `the_launcher_reference_is_up_to_date` fails when it is stale or a tool is missing.
- [horizon-ctl-commands.md](horizon-ctl-commands.md): every `horizon-ctl` command, its MCP tool
  and its arguments. It comes from the command table (`horizon-ctl help --markdown`), and the
  test `the_generated_reference_is_up_to_date` fails when it is stale.

## Surfaces

| Surface | Where | Transport |
| --- | --- | --- |
| Game control socket | `horizon-client --control` / `--control-socket PATH`, or Agent Control on in Horizon Settings > AI Agent (`crates/horizon-client/src/control.rs`; commands in `src/game/control.rs`, `agent.rs`, `ui_control.rs`, `saves_control.rs` and friends) | text line in, one JSON line out; Unix socket, or localhost TCP and a token where there is none |
| `horizon-ctl` | `crates/horizon-client/src/bin/horizon-ctl/` | CLI over the socket; one table of commands (`commands.rs`) gives the help, the argument checks, the MCP tools and the reference |
| `horizon-ctl mcp` | same | MCP over stdio, one tool per command of the table |
| Launcher CLI | `launcher/src/cli.rs` | `horizon-launcher <command> [--json]` |
| Launcher MCP | `launcher/src/mcp.rs`, `cli_tools.rs`, `game_tools.rs` | `horizon-launcher mcp` (stdio): every CLI command as a tool, the session tools, and the game's tools as `game_*` |
| Connecting | `launcher/src/agents.rs`, `agents_page.rs`; `src/game/agent_settings.rs` | `horizon-launcher agent-config`; Settings > AI in the launcher; Horizon Settings > AI Agent in the game |
| No AI | `crates/horizon-core/src/no_ai.rs` | `noAi:true` in `<game home>/horizon-extras.txt`, read by every surface |

## One MCP server for everything

An agent adds one server, `horizon-launcher mcp`. It holds:

- **The launcher's CLI, generated.** `cli_tools.rs` walks the clap definition. Each command is a
  tool named after its words (`mods search` is `mods_search`). Its arguments are the command's,
  in snake_case, with clap's help, choices and defaults. A call runs that command with `--json`
  as a child process and returns what it printed. A new CLI command is a new tool with no extra
  code.
- **Session tools**, which keep something running between calls:
  - `play` (the game, under control: `world`, `creative`, `join`, `offscreen`, `arg`);
  - `server`, `server_command`, `server_log`, `server_stop`;
  - `account_sign_in`, `account_list`.
- **The game's tools.** It runs the selected version's `horizon-ctl mcp` (shipped beside the
  game) as a child. It lists that server's tools as `game_<name>`, all but `launch`, and passes
  calls and results through, images included. `play` starts the game with a control endpoint in
  the launcher's data folder. A game the player started and turned Agent Control on in answers on
  the game's default endpoint ([Connecting](#connecting)). Each call goes to the first of the two
  where a game answers, the launcher's first, so a game from `play` wins over one from the window.
  While no game answers on either, `game_*` calls say to call `play` or to turn Agent Control on.

The default endpoint is worked out in the launcher's own process, so `$HORIZON_CONTROL`, and on
Linux `$XDG_RUNTIME_DIR`, must be the same for `horizon-launcher mcp` as for the game.

Why one server rather than two: the game's tools stay defined once, in horizon-ctl's table. The
agent's client needs one entry and one approval, whichever way the game was started. The tools
are always those of the version the launcher selected, even for a game of another version: a
command that game does not know answers with an error, and `game_hello` tells which game it is.
`horizon-ctl mcp` stays available on its own, for a game started by hand or by a script.
`horizon-ctl mcp --paced` stops a single-player world between tool calls: it opens the pause menu
after each action and closes it before the next, so the world moves only while an action (or
`wait`) runs, not while the agent thinks. Reading tools (`state`, `inventory`, `entities`,
`events`...) leave it paused; a pause a person opened, another menu, a LAN or a server world are
left alone.

## Protocol revisions

All three servers share one implementation, `crates/horizon-mcp`, so they advertise and answer
the same revisions. There are two of them.

- `2026-07-28` is stateless: there is no handshake, and every request carries its version and
  its capabilities in `_meta` (`io.modelcontextprotocol/protocolVersion`,
  `io.modelcontextprotocol/clientCapabilities`). Every result says `resultType: "complete"`,
  `tools/list` carries `ttlMs` and `cacheScope`, and each result names the server in
  `_meta["io.modelcontextprotocol/serverInfo"]`. `server/discover` reports the versions,
  capabilities and identity. `ping` no longer exists in this revision.
- `2025-06-18` is the handshake revision, kept because the launcher's `game_*` tools talk to
  whichever `horizon-ctl` the player has installed: an older one does not know the stateless
  revision. A request with no `_meta` is answered in that revision's shapes, and `initialize`
  still negotiates it.

A server that serves both eras decides from the request itself, not from what came before it.
A version the server does not serve is refused with `UnsupportedProtocolVersionError` (`-32022`)
listing the ones it does, and a modern request missing its capabilities is malformed (`-32602`).
A client that speaks both probes with `server/discover` first on stdio, then falls back to
`initialize` when the answer is not a modern one.

## Inventory

What an agent can reach, by page and screen. CLI is `horizon-launcher` for the launcher and `horizon-ctl` for the game. MCP is
`horizon-launcher mcp` (the game's tools as `game_*`) and `horizon-ctl mcp`. **part** means only
through a generic command (`ui` and `click` by widget, or a raw socket line that horizon-ctl
passes on without checking it).

### Launcher

| Capability | Window | CLI | MCP | Notes |
| --- | --- | --- | --- | --- |
| Releases and installed versions | Play, version picker | yes | yes | `versions [--installed]` |
| Install, remove a version | Play | yes | yes | `remove` needs `confirm` in MCP |
| Pick the version that plays | Play | yes | yes | `settings selected-version TAG` |
| Prepare resources | Play | yes | yes |  |
| Start the game | Play | yes | yes | `play --world --creative --join --arg --account`; MCP `play` adds `offscreen` |
| Join a friend by invite code | Play | yes | yes | `play --join CODE` |
| Dedicated server here: start, console, log, stop | Servers | yes | yes | `server`; MCP `server`, `server_command`, `server_log`, `server_stop` |
| Hosted servers: list, create, set up, run a command, forget | Servers | yes | yes | `servers ...`; `servers_forget` needs `confirm` |
| Accounts: list, sign in, sign out, select, offline | Settings | yes | yes | names and UUIDs only; sign-in is a device code |
| Game folder, server folder, install new versions, opening animation | Settings | yes | yes | `settings [KEY VALUE]` |
| Extra game arguments | Settings | yes | yes | `game-args` |
| Applications menu entry (Linux) | Settings | yes | yes | `desktop-entry` |
| Updates: check and install | Settings | yes | yes | `update` |
| Patch notes, links | Patch notes, footer | yes | yes |  |
| Game log, crash reports | Settings | yes | yes | `logs [--crash]` |
| Data collection: status, allow, deny, show, delete, send, bug, benchmark upload | Settings > Data | yes | yes | `data_delete` needs `confirm` |
| Mod browser: search, check, install, update, remove, list (mods, resource packs, modpacks) | Mods | yes | yes | `mods ...`; `mods_remove` needs `confirm` |
| Modpacks: import, port, play, remove, list | Modpacks | yes | yes | `modpack ...`; `modpack_remove` needs `confirm` |
| Launcher benchmark | (none) | yes | yes |  |
| The game it started | (the game) | (horizon-ctl) | yes | `game_*` |
| Connect an AI agent | Settings > AI | yes | (none) | `agent-config [--client] [--test]`; the agent is already connected, so there is no tool |
| No AI | Settings > AI | yes | (none) | `no-ai [on\|off]`; the player's switch, never an agent's |

### Game

| Capability | Screen | CLI | MCP | Notes |
| --- | --- | --- | --- | --- |
| Start, wait until ready, save, quit, log | (all) | yes | yes | `launch`, `wait-ready`, `save`, `quit`, `logs` |
| Every widget of every screen, by id; click, set, type | (all) | yes | yes | `ui`, `click ID`, `set ID VALUE`, `type`; `click-at`, `drag-at`, `scroll-at` by pixel |
| Title menu, pause menu | Title, Pause | yes | yes | `ui`/`click`, `pause`, `resume` |
| Worlds: list | Select World | yes | yes | `worlds` |
| Worlds: create, open | Create New World | yes | yes | `open WORLD [SEED] [creative]` |
| Worlds: rename, back up | Edit World | yes | yes | `world-rename`, `world-backup` |
| Worlds: delete | Select World | yes | yes | `world-delete FOLDER --confirm` |
| Servers list | Multiplayer | yes | yes | `servers` (names and addresses; reads servers.dat) |
| Join a server or an invite code, leave | Multiplayer | yes | yes | `connect`, `disconnect` |
| Open to LAN | Pause > Open to LAN | yes | yes | `lan open\|close\|status` |
| Play with Friends: host, stop, status | Pause | yes | yes | `friends host\|stop\|status` |
| Vanilla options, every page | Options | yes | yes | `options-get`, `options-set` (all of options.txt, typed) |
| Horizon Settings: Extras, PvP & HUD, AI Agent; the graphics screen (Video Settings); Controller (Controls) | Horizon Settings, Video Settings, Controls | yes | yes | `options horizon\|graphics\|extras\|agent...`, then `ui`/`click`/`set` |
| Agent control on or off, status, connection details | Horizon Settings > AI Agent | yes | yes | `options horizon`, the page's widgets (`ai-agent`) |
| Key binds | Controls > Key Binds | yes | yes | `options-set key_key.jump key.keyboard.j` |
| Resource packs | Options > Resource Packs | yes | yes | `options packs`, then `ui` rows and `click`; `options-set resourcePacks` |
| Chat and commands | Chat | yes | yes | `chat`, `command`, `messages`, `chat-log` |
| Inventory, containers, stations | Inventory, Chest... | yes | yes | `inventory`, `screen`, `click-slot`, `container X Y Z` |
| Crafting, recipe book | Crafting | yes | yes | the grid by `click-slot`; the recipe book by `ui`/`click` |
| Trading, riding, shearing (right-click an entity) | Merchant, Horse | yes | yes | `interact-entity ID` |
| Horse, llama inventory | Horse | yes | yes | `mount-menu` |
| Books, lecterns, signs | Book, Sign | yes | yes | `book [next\|back\|take\|done]`, `signedit X Y Z` |
| Move, look, find a path | (playing) | yes | yes | `move`, `goto`, `look`, `look-at`, `turn`, `jump` |
| Mine, place, use, attack, hotbar, drop | (playing) | yes | yes | `break`, `place`, `interact`, `use-item`, `attack`, `select`, `drop` |
| Raw keys and mouse buttons | (all) | yes | yes | `key`, `mouse`, `release` |
| Controllers | (all) | yes | yes | `pad` |
| Respawn | Death | yes | yes | `respawn` |
| Screenshots | (all) | yes | yes | PNG of the next frame, `--offscreen` too |
| F3 data | F3 | yes | yes | `key f3`; `status` (renderer, chunks, memory) |
| Waits and events | (all) | yes | yes | `wait`, `wait-for`, `events` |
| Effects, experience, enchantments, potions, fill | (cheats) | yes | yes | `effect`, `effects`, `xp`, `enchant`, `potion`, `fill` |
| Benchmarks and frame stats | (all) | yes | yes | `bench`, `stats`, `fly` |
| Performance captures | `/profiler` | yes | yes | `profile [client\|server] start\|stop\|status\|mark`, `trace`; the file is read with `horizon-perf` or `horizon-perf mcp` ([perf-platform.md](perf-platform.md)), and its slow ring (the last 256 slow frames or ticks) rides in it; none of them is logged |

## No AI

One switch hides and turns off every AI feature: `noAi:true` in `<game home>/horizon-extras.txt`.
The game home is `HORIZON_HOME`, else the data dir's `horizon`. The game reads its extras from
its working directory, which is that folder. `horizon_core::no_ai` reads and writes the switch:

- `is_on(home)` and `check(home)` read it;
- `set(home, on)` keeps the file's other lines, and writes no file to turn off a switch that was
  never on;
- `REFUSAL` is the one line every surface prints.

The switch is in three places: Settings > AI in the launcher, Horizon Settings in the game
(`Item::NoAi`), and `horizon-launcher no-ai on|off`. When it is on:

| Surface | What happens |
| --- | --- |
| Game | opens no control socket, even with `--control` (it logs the refusal); Horizon Settings hides AI Agent...; turning it on in a running game closes the socket at once |
| `horizon-ctl` | every command refuses: exit status 4, `{"error": REFUSAL, "kind": "denied"}` |
| `horizon-ctl mcp` | refuses to start (exit 4, REFUSAL on stderr); a session already running refuses each call |
| `horizon-launcher mcp` | refuses to start; a running session answers each `tools/call` with error -32000 and REFUSAL |
| Launcher CLI | `mcp` and `agent-config` are hidden from the help and refuse |
| Launcher window | Settings > AI shows only the No AI switch, without Connect an AI agent |
| Companion (#1068) | uses the same switch (see horizon-parity companion-notes.md) |

Every other launcher command stays usable: versions, install, remove, prepare, play, server,
notes, links, account, settings, game-args, logs, update, desktop-entry, data,
modpack, mods, servers, no-ai and benchmark. None of them drives the game for an agent. The
launcher itself, packagers, the docs and players' scripts need them, and `no-ai` has to stay to
turn the switch off.

## Connecting

- **Launcher, Settings > AI > Connect an AI agent:**
  - a client picker (Claude Code, Codex, Cursor, any MCP client) with its config and a Copy button;
  - for clients with a CLI, the add command, shown and run only on **Add to ...**:
    `claude mcp add --scope user horizon -- <exe> mcp`, `codex mcp add horizon -- <exe> mcp`;
  - **Test the connection** (`agents::self_test`) starts `<exe> mcp`, lists its tools and says
    how many answered.
  - `horizon-launcher agent-config [--client ID] [--json] [--test]` prints the same thing.
- **Game, Horizon Settings > AI Agent:**
  - **Agent Control** (`agentControl` in horizon-extras.txt, off by default) opens the default
    endpoint (`$HORIZON_CONTROL`, else `horizon-control.sock` in the runtime directory, or
    `horizon-control.json` in the temporary directory on Windows) in a game the player started.
    `horizon-launcher mcp` reaches it as it is, with no second server. Turning it off closes the
    endpoint and stops its listener.
  - The page shows whether a client is connected (`Server::clients`), when the last command came,
    and the socket's name.
  - **Copy Connection Details** copies the socket path, a `horizon-ctl --socket ... state` line
    and an `mcpServers` config for this game's `horizon-ctl mcp`. That config points into the
    version's folder, so it stops working once the launcher removes that version; an agent that
    already has `horizon-launcher mcp` does not need it.
- **The default:** a game an agent starts (`play` in MCP, `horizon-ctl launch`, `--control`) is
  under control. A game the player starts from the window is not, until they turn Agent Control
  on. An open socket lets any program of that user drive the game, so it opens only when an agent
  asked for the game, or when the player chose it.
- **The window an agent opens** comes up without the focus (on Windows and macOS; X11 focuses it
  anyway): the person keeps their keyboard, and the game keeps its pointer grab to itself, as
  `--offscreen` does, so the system pointer is never taken, hidden or moved. Once the person
  clicks into the window it is an ordinary one, and that click takes the pointer.
- **Headless:** `horizon-ctl launch --headless` (`horizon-client --headless --control`) starts a
  game with no display, no window and no GPU: it runs where no display server is (an SSH session,
  a server) and runs its own loop instead of winit's (`game/headless.rs`), one frame a tick. It
  plays through these commands only; commands that need the screen's pixels (`screenshot`, clicks
  at GUI points) answer with an error ([#4951](https://github.com/horizon-rust/horizon/issues/4951)).

## Design rules for a new command

### One vocabulary

- **A command has one name**, kebab-case, the same on the socket, in `horizon-ctl` and in the
  launcher CLI. Its MCP tool is the name with `_` for `-` (`click-slot` is `click_slot`). In the
  launcher, a subcommand's words are joined (`mods search` is `mods_search`). The launcher's MCP
  offers every game tool as `game_<name>`, so none collides with its own (`logs` is on both
  sides). A launcher tool whose name starts with `game_` (`game_args`) is matched first.
- **Arguments:** positional words, then `--flags`.
  - The game: the `horizon-ctl` table (`Param`) is the source of truth. It generates the MCP
    `inputSchema` and the reference. A game command is not done until it is in that table.
  - The launcher: the clap definition is the source of truth. Give every argument a help line;
    it becomes the tool's description.
- **Replies are one JSON object.** A query returns its data; an action returns
  `{"ok": true, ...}` plus what changed (`clicked`, `screen`, `drops`...).
- **Errors** are `{"error": "<sentence an agent can act on>", "kind": "<reason>"}`. The kinds:
  - `usage`: a bad or missing argument;
  - `unknown`: no such command, widget, option or entity;
  - `state`: the game is not in a state that allows it (no world, wrong screen, disabled widget);
  - `busy`: another action or a benchmark is running;
  - `timeout`;
  - `connection`: the game can't be reached;
  - `denied`: No AI is on (exit status 4);
  - `api`: `--api LINE` named a protocol line the game's `hello` does not report (exit status 5).

  The message names what to do next ("no world open; use: open WORLD").
- **Shapes are shared:**
  - a position is `[x, y, z]`;
  - a block is `{"pos", "block", "state", "loaded"}`;
  - an item stack is `{"item", "count", "damage"?, "components"?}`;
  - a slot is a stack plus `"slot": ID` (the slot ids of `click-slot`);
  - bounds are `[x, y, w, h]` in GUI pixels, the coordinates of `click-at`.

### Safety

- **Local only.** The socket is a Unix socket in the user's runtime directory, or a TCP port on
  127.0.0.1 behind a 128-bit token in a file only the user can read. It is never `0.0.0.0` and
  never a network listener. Both MCP servers use stdio.
- **Confirm what can't be undone.** A command that deletes the player's things requires
  `confirm` and carries the MCP `destructiveHint`. In the game these are `world-delete`, `fill`
  and `setblock` (the table's `DESTRUCTIVE`); in the launcher, `remove`, `modpack remove`,
  `mods remove`, `servers forget`, `account sign-out` and `data delete` (`cli_tools::DESTRUCTIVE`).
  Without `confirm: true` the call is refused and the message says to ask the player. In the game
  CLI the flag is `--confirm`. The launcher CLI asks nothing: a person typing `remove` meant it.
- **Ask before reaching out.** A command that sends data or reaches other people carries
  `openWorldHint`, and its description says "only when the player asks". In the game these are
  `connect`, `lan`, `friends`, `chat` and `command`; in the launcher, `data allow`, `data send`,
  `data bug`, `data benchmark` and `servers create`. `play`'s `join` argument says the same.
  Telemetry stays opt-in, and only `data allow` turns it on.
- **No secrets.** No surface returns tokens, session files, keychain entries or invite secrets
  the player didn't ask to share:
  - accounts are names and UUIDs;
  - sign-in is a device code the player enters;
  - `--session-file` is read once and deleted;
  - `servers` reads names and addresses only.
- **Vanilla behaviour untouched.** Control code reads state and synthesizes the input events a
  player produces; registering widgets for the tree draws nothing. A change to game files runs
  `tools/parity.py --changed origin/main`.

### The widget tree

`ui` returns every interactive element the last frame drew, on every screen:

```json
{"screen": "options", "gui_size": [427, 240], "frame": 3061,
 "widgets": [
   {"id": "fov", "kind": "slider", "label": "FOV: 70", "bounds": [40, 50, 150, 20], "enabled": true, "value": 0.5},
   {"id": "done", "kind": "button", "label": "Done", "bounds": [...], "enabled": true}]}
```

- **Ids are stable and readable.** An id is the label up to its `:`, as a slug (`FOV: 70` is
  `fov`, `AI Agent...` is `ai-agent`). A repeated slug gets `-2`, `-3`; unlabelled elements are
  `<kind>-<n>`. A slider's id doesn't change with its value. Slots are `slot-<ID>` with their
  stack.
- **Kinds:** `button`, `slider` (`value` 0..1), `field`, `row` (a world, server, pack or
  language), `tab` (`selected`), `slot`.
- **Actions:**
  - `click ID [right|middle] [shift]` clicks where the mouse would.
  - `set ID VALUE` moves a slider.
  - `type` types into the focused field.

  After a click, the next `ui`, `click` or `set` waits for a redrawn frame. Right after
  `launch`, they and `screen` and `click-button` wait for the first one.
- **No new code paths in screens.** The tree is built from what the draw functions already
  register for controller navigation (`widgets.rs`).

### Waits and events

- `wait-for screen NAME|widget ID|loaded|chat TEXT|event KIND [--timeout S] [--since SEQ]`
  replies when the condition holds, or with `kind: timeout`.
- `events [--since SEQ]` returns what happened since SEQ: screens, chat, deaths, respawns, items,
  damage (`hurt`, with the damage type, the amount and what dealt it), `heal`, `food`, dimensions
  and worlds (the newest 256). `hurt` and `death` carry the player's position: a death drops the
  inventory there. Take `last` before an action, and wait with `--since`, so the action's event
  can't slip in before the wait (`wait-for event hurt` wakes an agent when it is attacked).

### Input provenance

Every input event carries who caused it: `user` (this computer's keyboard, mouse or pad),
`agent` (the control socket: `horizon-ctl`, an MCP client) or `game` (input the game made up
itself, such as a pad menu's click). This is how an agent tells its own actions from a person
who took over mid-test.

- While a screenshot sequence, a `HORIZON_TICK_SCRIPT` scenario or a `record-replay` runs, the
  keyboard, mouse and pad at this computer don't reach the game, and what they do isn't
  recorded: a stray key or mouse move can't spoil the run. The run's own input (the socket, the
  replay) still plays. Clicking into the window or away from it takes effect when the run ends,
  and the game's pointer grab stays inside the game until then, so the system pointer is never
  taken or moved; closing the window still ends the run. A key held as the run starts stays
  down until it is pressed again. Outside these runs nothing is held back; a game no one at
  the computer should touch runs `--offscreen` or `--headless`.
- `state` and `status` have an `input` section: `last_origin`, per-origin `counts`,
  `user_since_agent` and `last_user_age_s`.
- `input-log [--since SEQ] [--limit N] [--origin ORIGIN]` lists the newest 512 input events:
  `seq`, `tick`, `origin`, and the key/button/motion/pad event or control-socket line.
- Polls (`state`, `events`, `wait-for`, `input-log`, `ui`...) and session commands
  (`record-*`, `repro-export`, `quit`...) are not input: they do not fill the ring or count as an
  agent's keystroke. They still count as an agent watching, so a person's next key is noticed.
- When a person's input arrives while an agent was driving, the game logs one line and pushes a
  `user-input` event: `wait-for event user-input --timeout 600` makes a script notice at once
  instead of blaming the person's keystroke on itself. The agent counts as driving while a
  client is connected, a deferred action runs, or a `wait-for` waits, not only for five seconds
  after its last line.
- Things that are not input (frames, resizes) are not events. Typed text is never kept: a
  `chat`, `command`, `type`, `signedit`, `connect`, `book` or `publish` line is recorded as
  `<name> <redacted>`. OS-level tools that inject keys (xdotool, computer-use) look like a
  person (`user`); only the agent's own socket is `agent`.

### Repro recordings

A recording is one folder, a bundle is that folder plus the logs in one zip:

- `record-start [NAME] [--no-world]` saves the world, freezes the server, copies the world into
  `recordings/<stamp>-NAME/world/` and starts writing: `inputs.jsonl` (the provenance events
  above, with ticks), `state.jsonl` (the full `state_value()` every second, entities every ten),
  `events.jsonl` and `session.json`. A `--no-world` recording keeps only the input and state,
  for a server session. The deprecated in-frame mode (`--no-server-thread`) is refused.
- A recording cannot cross worlds: quit-to-title stops it. It also stops itself at 1,000,000
  input events or two hours, and is finished and bundled when the game exits. The newest ten
  recordings and ten bundles are kept; older ones are pruned.
- `record-stop [--export]` closes it; `repro-export [NAME] [PATH]` writes
  `repro/<stamp>-NAME.zip` with the recording, `logs/latest.log`, a crash report from this
  recording's session, a reduced `status.json` and a README. The export scrubs the user folder,
  IPs, UUIDs, the world and server names and the other players' names; `status.json` loses the
  server, name tags, sidebar and clipboard (an invite code lives there). Nothing is uploaded
  automatically: the player attaches the zip to the bug report.
- A player without `horizon-ctl` uses the debug keys: **F3+R** starts a recording and F3+R again
  stops it and writes the bundle (the chat shows the path); **F3+E** writes the bundle of the
  newest recording. The keys are off until **Recording shortcuts** is turned on (AI Agent page);
  the page also has the two buttons and a live recording/replay line.
- `/profiler start` and `/profiler stop` in chat start and save a capture and print its file name; add `client` or `server` for one side. `/profiler status` shows whether one is running. In the official build the saved line has `[Open]` and `[Share]` (`/profiler open [id]`, `/profiler share [id]`): Open shows the capture in the viewer from your own computer, Share uploads it and prints the link, the only upload. The local `.hzperf` and private `.hzperf.share.json` owner receipt stay in reports/. Nothing uploads unless you share, and none of this depends on the telemetry switches.
  See [graphics.md](graphics.md) for what the capture holds.
- `record-replay NAME [--stop] [--in-place] [--keep-world] [--timeout S]` is the best effort
  replay: it copies the recording's world into `saves/`, opens it, restores the recorded player
  position, rotation and velocity, and feeds the recorded keys, mouse, wheel, look, pad events
  and input-socket lines back at their original ticks. **Only input and movement commands run**
  (`key`, `mouse`, `click*`, `look`, `turn`, `goto`, `pad`, `wait`...), never world, network or
  account ones (`world-delete`, `connect`, `chat`, `command`...). It ends at the recording's
  `end_tick`, releases every key and button it held, and removes its `saves/` copy (keep it with
  `--keep-world`). The timeout counts from the command, not from when the world finished
  loading. It is not frame-perfect and only makes sense on the same build and machine;
  `state.jsonl` is the oracle for where the re-run should have ended. A server recording replays
  only `--in-place`, into the same world.

### Repeatable screenshots

`HORIZON_STILL=1` (a dev switch, off by default) makes a frozen scene draw the same frame in every
run, pixel for pixel, on the same build, GPU and driver: a change's before and after are then
compared by a pixel diff, not by eye, where two runs otherwise differ by a shade or two over much
of the frame ([#8750](https://github.com/horizon-rust/horizon/issues/8750)).

- The world starts frozen (`/tick freeze`) before its first tick, on the server and the client:
  how many ticks run while the world loads differs between runs, and the torch flicker and the
  particles move on each.
- What the frame shows of time follows the world's ticks, not the client's or the wall clock's:
  - animated textures, the enchantment glint, the world border's scroll, the darkness effect's
    pulse, the dragon's healing beam and the player model's idle swing (third person, the
    inventory);
  - the HUD: the hearts' jiggle and blink, chat fading, titles, the action bar, the held item's
    name, the locator bar and the tab list (a title shown while frozen fades in on the next
    `/tick step`);
  - a shader pack's clock (Iris's `frameTimeCounter`, `frameCounter` and `frameTime`, a Horizon
    pack's `time`, `frame_index` and `delta_time`, with smoothed values at their targets) and the
    randoms behind custom uniforms, cave mood (`playerMood`), obfuscated text and a clock or
    compass that spins;
  - the camera's partial tick, held at 1 like the level's while frozen.
- While frozen, the client's own entity effects (endermen's and blazes' particles) and its
  countdowns (effects, a totem's pop, the hotbar's, the border's size change) wait for the world.
- `/tick step N` moves the scene on by N ticks (flames, water, particles). What the server changes
  during a step reaches the client between its ticks whenever it arrives, so particles that
  follow can differ from run to run. Wait a second or more after a step before a capture (`wait
  40`): the server sends its time once a second, which can leave the client a tick ahead until the
  next, and the last tick still eases for a client tick. `/tick unfreeze` plays on as without the
  switch.
- While frozen, rain and thunder sit at their levels (1 while it rains or thunders, else 0), and
  the rain fog too, as a shader pack's smoothed values do: `/weather rain` shows full rain without
  a step. A step or `/tick unfreeze` fades them as without the switch.
- The server's own randoms and entity ids are its own: a summoned mob's facing and variant, and
  mob AI during a step, can differ when other server work drew on them first, and ids follow the
  order the server loads and creates entities in, which loading changes from run to run. What an
  id seeds can then differ: a minecart's offset against z-fighting, a mob's client random. Give
  facings and variants in the command (`{Rotation:[0f,0f]}`) for a repeatable scene. An end
  crystal's spin and bob start from its position instead, so they are the same in every run.
- A script should wait until the world has loaded before it changes it: a command that acts on
  what is loaded (`kill @e`, `fill`) finds more or less of it otherwise.
- The player still ticks while frozen, as in vanilla: stand the player on a block (it falls
  otherwise), and after a turn or a move wait a few seconds (`wait 60`) before a capture, or the
  hand, which eases toward the view each tick, is a hair off. What the player does goes on too:
  using an item, riding, running out of air.
- A shader pack's own buffers, kept from frame to frame, take `horizon-ctl screenshot-sequence
  FRAMES STEP_MS PATH`: the pack starts over (its buffers, images, shadow maps, smoothed values and
  custom uniforms, or a Horizon pack's `history` and eased values) and draws FRAMES frames on a set
  clock, frame n at n frames and (n - 1) × STEP_MS milliseconds (0 since the frame before on frame
  1, STEP_MS after; a decimal, `16.666667` for 60 fps). The last is written as `screenshot PATH`
  writes it (with its `-hdr.png` under HDR output), and the reply, once it is, names the pack and
  the last frame's clock. Without a pack it draws the frames all the same. It needs the world
  frozen, far terrain (LOD Distance), TAA, frame generation and the shadow cache off and chunk fade
  0, and says so otherwise. While it runs, commands that would change the scene are refused, a
  change to the camera, the world, the settings, the pack or the window's size ends it with an
  error, `screenshot-sequence cancel` stops it and `quit` quits.
- Not covered: temporal AA and frame generation (they build on earlier frames: turn them off),
  custom entity model animations, native mods, and what eases on the wall clock for a moment
  (toasts, the boss bar's fill, chunks fading in, the spyglass): wait for them to settle. Frame
  times are real ones: not for timing.

## Tests

- **Unit tests:**
  - horizon-ctl: the table, the schemas, the reference, the No AI refusal, the destructive and
    network annotations;
  - the launcher: generated tools, `confirm`, the reference, the agent configs;
  - `horizon_core::no_ai`;
  - `options_ui`: the AI Agent page, and that No AI hides it;
  - `input_trace`: origins, the ring, the intrusion rule, `input-log` and the recording writer;
  - `repro`: the format's round trip, the folder slug and the world copy.
- **`tools/agent_control_e2e.py`** drives a real game, offscreen on macOS and on a virtual screen
  on Linux, through the CLI and MCP:
  - `survival`, `ui`, `options`, `events`, `consume`, `pick`, `suggestions`;
  - `manage`: worlds, servers, effects, books, LAN, the options pages and `confirm`;
  - `no-ai`: every surface refuses;
  - `setting`: Agent Control on opens the endpoint, the AI Agent page, Copy, and off closes it;
  - `repro`: origins in `state`/`input-log`, record-start/stop, the bundle's entries, and a
    replay ending near the recorded state.
- **`tools/launcher_mcp_e2e.py`** drives `horizon-launcher mcp`: the generated tools, `play` and
  the `game_*` tools, `confirm`, `agent-config` and `--test`, and No AI.

## Later

These are tracked in FEATURES.md (LATER):

- Horizon's own option files by key (`horizon-graphics.txt`, controller, extras) next to
  `options-get` and `options-set`.
- Adding and removing saved servers by command.
- `subscribe`: events as a stream on one connection.
- Launcher window screenshots in this page's PRs.
- No AI changed in the launcher while a game is open: the game rewrites its extras when it saves.
- A confirmation question in the launcher CLI for destructive commands.
