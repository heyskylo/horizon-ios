# Controllers

Horizon plays with PlayStation (DualShock 4, DualSense), Xbox (360, One,
Series) and Nintendo Switch Pro controllers, and on the Steam Deck with its
built-in controls. Keyboard and mouse keep working exactly as before: with no
controller connected, or with controllers turned off, nothing changes.

## What vanilla has

Nothing. Minecraft Java 26.3 never calls GLFW's gamepad or joystick API, has
no controller options and ships no button glyphs. Its only non-mouse UI input
is keyboard focus navigation (`InputType.KEYBOARD_ARROW` / `KEYBOARD_TAB`:
arrows and Tab move a focus ring between widgets). So controller support is
Horizon's own feature. Its default buttons are Horizon's own; analog walking and toggle sneak follow
Bedrock Edition on consoles.

## Input layer

[gilrs](https://crates.io/crates/gilrs) reads the pads: evdev on Linux (with
libudev for hot-plugging), IOKit on macOS, Windows.Gaming.Input on Windows.
Its SDL mapping database normalizes every pad to one positional layout
(South/East/West/North face buttons, bumpers, triggers, sticks, D-pad,
Select/Start). Horizon also injects a few SDL rows gilrs' bundled DB still
misses (notably the 8BitDo Ultimate 2 on Linux/Bluetooth, whose right stick
is on ABS_Z/ABS_RX without that row), and corrects that pad by vendor/product
when no SDL row matched. No second windowing or event stack: gilrs is polled
once per loop iteration next to winit's events.

- The pure logic (mapping table, deadzones and curves, button edges, the
  settings file, glyph choice) lives in `crates/horizon-client-input/src/gamepad.rs`
  and is unit-tested. The game owns a thin gilrs backend and applies actions.
- Controller actions go through the same path a keyboard key takes after
  rebinding: an action names a vanilla key mapping (`key.jump`, `key.attack`,
  ...), and the game handles it as that mapping's default key or mouse button.
  Toggle sneak/sprint/attack/use, screens releasing held keys, and the
  multiplayer client all behave as they do on the keyboard.
- Movement is analog: the left stick's direction and tilt feed the player's
  move vector directly (a half-tilted stick walks slowly, like Bedrock),
  instead of being rounded to WASD. The forward/back/left/right flags other
  code reads (sprint start, boats, minecarts, the server's input packet) are
  set when the stick passes a threshold. On a server (not this game's own
  server thread) the stick moves as those keys do, in 8 directions at full
  speed, as the server only hears the keys: an anticheat predicting from them
  pulls back a player moving at the stick's own angle and speed.
- The pad last touched is the active one. Several pads are not merged.
- `horizon-ctl pad ...` injects a virtual pad (buttons, sticks, triggers) into
  a running game, for tests without hardware.

## Default mapping

| Action | Xbox | PlayStation | Mapping |
|---|---|---|---|
| Move | Left stick | Left stick | analog `key.forward/back/left/right` |
| Look | Right stick | Right stick | camera |
| Jump / fly up | A | ✕ | `key.jump` |
| Drop item | D-pad down | D-pad down | `key.drop` |
| Swap offhand | X | □ | `key.swapOffhand` |
| Inventory | Y | △ | `key.inventory` |
| Hotbar left / right | LB / RB | L1 / R1 | Horizon `pad.hotbarPrevious/Next` |
| Use / place | LT | L2 | `key.use` |
| Attack / break | RT | R2 | `key.attack` |
| Sprint | Left stick click | L3 | `key.sprint` |
| Sneak (toggles) | Right stick click | R3 | `key.sneak` |
| Change camera | View | Create / Share | `key.togglePerspective` |
| Pick block | D-pad left | D-pad left | `key.pickItem` |
| Chat | D-pad up | D-pad up | `key.chat` |
| Pause / back | Menu | Options | Escape (fixed) |

Advancements and the player list have no button of their own, and are
bindable. A settings file from before these defaults keeps its buttons where
they were.

Also bindable, with no default button: fullscreen, player social, quick actions, the save and load
toolbar activators and hotbar slots 1 to 9. They act in play only: in a menu or a prompt the button
does what that screen says. A toolbar activator does nothing by itself: a creative player holds it and
presses a hotbar slot's button, as with the keys, and a spectator's hotbar buttons drive the spectator
menu.

The bumpers go through what the wheel goes through (`Game::hotbar_step`), one notch a press, RB down:
with no screen, prompt or bed in the way, a native mod can take it (`native_wheel`, then
`native_mouse_scrolling`), then a spectator's menu or flying speed takes it, else the hotbar turns.

Drop follows Q: a tap throws one item. Held, it repeats at the repeat delay and interval of Controller
Settings > General, and from 1 second on (`DROP_STACK_HOLD`) each repeat throws what's left of the
stack, as a pad has no Control to hold. Nothing repeats while a menu is open.

The Guide / PS / Steam button is left to the system. Sneak on a stick click
toggles by default (holding a stick in while steering the camera is
awkward, which is why Bedrock does the same) and acts like vanilla's toggle
crouch: a screen stands you up while it is open and closing it crouches you
again. The controls options can make it hold-to-sneak.

A Nintendo pad's face buttons can go by their places, as the table and SDL's
layout have them (B, at the bottom, jumps and confirms), or by their letters,
as a Switch's own menus and keyboard do: A (right) jumps and confirms, B goes
back and drops, X swaps the offhand, Y opens the inventory, and the binds list
and the on-screen keyboard's hints show those letters (splitscreen's join and
lost-controller prompts, which a controller nobody holds yet answers, stay by
places). Controller Settings > Device > Nintendo Buttons picks one: By
Position or By Letter, saved as `nintendoLetters:true|false` in
`horizon-controller.txt`. By position unless changed (a build for the Switch
itself goes by letters). The row is always there, like the DualSense rows
beside it, and changes nothing until a Nintendo controller is the one
playing (the glyph choice alone doesn't turn it on). Bindings name the part, so a rebinding means the same on
either.

## Menus and inventories

### Adding a screen

Draw the screen's Done, Back, Cancel or Apply with `ui::action_button` and the
`widgets::Action` it performs, or call `widgets::declare_action` if the screen
draws its own buttons. That is all: nothing else names the screen.

Every control also needs a focus target, or the pad can't reach it. `ui::button` and its kin register
themselves, as does `settings_shell::foot_button`. `graphics_style::button` and `screen_draw::button` only
paint, so anything drawn with them, or by hand, needs its own
`widgets::record` or `widgets::target_as`. A focus screen with any target has no crosshair, so register all
of it. The `every_screen_has_targets` case of `tools/pad_menu_e2e.py` fails on a focus screen that
registers nothing, unless its allowlist names it.

| Action | Pad button | With the button disabled |
|---|---|---|
| Back, Cancel, Done (the first the screen has that can be pressed, in that order) | B | B falls back to Escape |
| Apply | X | no glyph, X does nothing |
| Options | X | no glyph, X does nothing |
| Exit | moves the focus there for A to confirm: B on a screen with no Back, Cancel or Done, else Y | no glyph |

It works the same on a focus screen and a pointer screen (Statistics, Advancements, packs, books).
The hint is the glyph on the declared button, in that player's pad's style and view; nothing draws
in a corner. It shows while the pad drives; the mouse or a focus key hides it, and a button too small
for it, or running off a split view, goes without it and keeps its pad button. What a screen declared
stops counting as soon as another screen is drawn over it (`widgets::begin_screen`: a popup's buried
buttons can't be pressed) or takes its place. While a key row waits for its key nothing is declared and only B and Start act:
they are Escape, which unbinds. A row on Controller Settings > Bindings takes the next button pressed
as its binding, and Start cancels.

Don't declare a button Escape deliberately ignores: Optimize World's and a connection's Cancel have
no pad button, only A on them, and show in `undeclared` for that reason.

A focused text field shows A at its right end. `game/pad_actions.rs` holds this table. A screen that declares nothing keeps
B as Escape. `horizon-ctl pad status` lists `undeclared`: a button labelled
Done, Back or Cancel on the current screen that didn't declare its job.
`tools/pad_menu_e2e.py` fails on one in the settings screens. `glyphs` lists what is drawn: each
button, its `sprite` (null when it is the label on a key) and the `rect` it covers.

A label wider than its button slides inside it, as vanilla's do
(`AbstractWidget.extractScrollingStringOverContents`); one under a glyph slides beside it.

Vanilla's screens are all mouse-driven, and Horizon draws them 1:1. A pad
drives them two ways, each on the screens it suits:

- **Focus**, in menus: a console-style highlight that the D-pad and the left
  stick move between buttons, sliders, switches, text fields, list rows and
  tabs.
- **Pointer**, in inventories and containers, and on the screens whose
  clickable parts register no focus target (the advancements map, the
  statistics lists, the pack rows, a book's page text): a crosshair the left
  stick moves, centred on the point it clicks. Letting go of the stick glides
  it onto the nearest slot or control, and the D-pad hops it one slot or
  control at a time.

Controller Settings also lets each player adjust pointer speed, right-stick scroll speed, and the delay and
interval of held navigation repeats. These preferences are saved alongside
that player's options and bindings.

Both modes feed the screens the same pointer events a mouse sends: in focus
mode the (hidden) pointer sits on the focused widget. So every screen keeps
its mouse behaviour (drags, sliders, scrollbars, hover highlights) with no
controller code of its own, but focus only lands on what a screen registers,
and a focus screen with any target has no crosshair: a control with no target
on it is out of the pad's reach. Moving the real mouse hands control back at once.

### Focus navigation

A screen registers what it draws as focus targets with their kind
(`widgets::target_as`): classic buttons, sliders, edit
boxes and list rows (worlds, servers, languages, superflat layers and presets, biomes), icon buttons (the title's,
the pause menu's and Realms'), inventory and container slots, creative and recipe-book
tabs, and the stations' own buttons (trades, loom patterns, beacon effects,
enchantments, stonecutter recipes, the recipe book's recipes, pages and
filter). Not every control registers yet (see "Adding a screen"). A screen that registers nothing falls back to the pointer.

The step itself is **vanilla's arrow-key navigation**
(`ContainerEventHandler.nextFocusPathInDirection`, 26.3), applied to every
target of the screen at once: from a strip one pixel past the focused edge,
the nearest target that overlaps it on the other axis wins (ties go to the
leftmost or topmost); without one, the target whose facing edge's centre is
nearest the focused edge's centre (`nextFocusPathVaguelyInDirection`). That
gives vanilla-like jumps in containers too: hotbar to inventory, the grid to
the crafting result, armour to the player's grid.

| Input | Focus mode |
|---|---|
| D-pad, left stick | move focus (held: repeats after 0.35 s, every 0.09 s; the stick steps past 0.55 and lets go under 0.35) |
| A | press the focused widget: a button when A is let go, if the focus is still on it; anything else when A goes down (held: drag); on a text field, type with the on-screen keyboard. With nothing focused yet, A only lands the focus, as the D-pad does |
| B, Menu | back (Escape); on the title's main menu, B moves the focus to Quit Game, for A to confirm |
| X | right click (inventories: take half, place one); on the title's main menu and the pause menu, Options |
| Y | quick move (shift-click) in inventories; on the pause menu, moves the focus to the exit button, for A to confirm |
| View | throw one item from the focused slot (Q) |
| Left stick click | throw the focused stack (Ctrl+Q) |
| LB / RB | previous / next tab (Create World, Player Social, the graphics settings' sidebar) |
| Right stick | scroll the list, wherever the focus is: the focus stays on its row (when the row leaves the view, on the nearest one still in view), no value changes, and nothing scrolls under the on-screen keyboard |

- **Tooltips**: an options page's tooltip hangs from the focused widget's bottom left while the pad's focus drives
  the page, since the pad's hidden pointer sits mid-widget. The mouse and the keyboard keep vanilla's positioners.
- **Declared buttons**: a button that says what it's for gets its controller
  button and a glyph on it while the pad drives; see "Adding a screen".
- **Sliders**: A takes hold of a focused slider, shown by a white outline; left
  and right then move the value (one pixel per press like vanilla's arrow keys,
  faster while held). A or B lets go and keeps the value, and up or down lets go
  and moves on. Until then left and right move the focus, so passing over a
  slider doesn't change it.
- **The server list**: the bumpers move the selected server up or down, as its arrows do.
- **Lists** follow focus: a step out of a list that can still scroll that way
  scrolls it and lands on the next row once it is drawn, so every row can be
  reached; the buttons below or above come after its last or first row.
- **Inventories**: the carried stack is drawn on the focused slot (vanilla
  draws it under the pointer). A takes or places, X splits or places one, Y
  quick-moves, View and the left stick click throw.
- **Where focus lands** when a screen opens: the held hotbar slot in an
  inventory, else the first button; on a prompt, see "Prompts".

It looks as vanilla's own keyboard focus does: buttons, sliders and slots
show their highlighted sprite (`isHoveredOrFocused`, drawn because the hidden
pointer is over them), and list rows and text fields get vanilla's white
one-pixel outline (`AbstractSelectionList`'s focused selection, `EditBox`'s
highlighted border).

### Keyboard

Vanilla's own keyboard focus navigation (`Screen.keyPressed`, 26.3) uses the
same focus, so it works with no controller and with controllers turned off
(`game/key_focus.rs` decides, `game/pad_menu.rs` applies it, and
`game/startup_menu.rs` on the title shown while resources load). It matters
most for the narrator: the focused widget is the narration's focused one.

| Key | Does |
|---|---|
| Tab / Shift+Tab | next / previous widget in tab order, wrapping (`TabNavigation`) |
| Arrows | the widget that way (`ArrowNavigation`, the step above; past a list's last visible row the list scrolls) |
| Enter, Space, keypad Enter | press the focused button, cycle button, checkbox or tab (`AbstractButton.keyPressed`: a click on it, with its sound) |

- **Sliders** (`AbstractSliderButton`): focused with Tab or a click, left
  and right change the value (one pixel per press, `1 / (width - 8)`);
  focused with an arrow they move the focus until Enter lets them change
  it. Enter toggles that. Up and down always move the focus.
- **Text fields** keep left and right for their caret (`EditBox.keyPressed`),
  and Enter and Space for themselves; Tab, up and down leave them. Focus
  landing on a field focuses it with the caret at the end; leaving it
  unfocuses it (title screens).
- **Lists**: a list's rows are one Tab stop (Tab enters on its first row,
  Shift+Tab on its last, the next Tab leaves); up and down move between rows.
  On the world list focus selects the row and Enter joins it, as vanilla;
  on other lists Enter clicks the row (selects it, or joins the selected
  server).
- With nothing focused, Enter and Space go to the screen (the world list's
  Enter joins the selected world) and an arrow starts from the screen's
  edge behind it. A click focuses what it lands on (`mouseClicked`), so the
  next Tab or arrow goes on from there.
- A new screen opened by a key starts on its first widget in tab order
  (`Screen.setInitialFocus`); one opened from the game has no focus.
- The keyboard doesn't reach inventory slots or the creative and recipe-book
  tabs (vanilla's screens don't focus them): in container screens only
  their buttons (the recipe book button, trades, beacon effects).
- Screens that keep the keys: chat (Tab completes commands), signs, books
  being written, the creative, recipe-book and mod searches, the anvil's
  name, command blocks, dialogs, a key or button being bound on Controls,
  the credits, the on-screen keyboard.

It looks as vanilla's keyboard focus does: the focused widget draws its
highlighted sprite, and rows and fields the white outline (as above). As
for the pad, the screens draw their highlight from the pointer, so the focus
puts Horizon's pointer (not the OS one, which stays where the mouse left it
and stays visible) on the focused widget, and Enter is a click there. Moving
the mouse hands the pointer back; a click or a scroll first puts it back
where the mouse is.

Approximations:

- **Tab order** is reading order (top to bottom, then left to right), not
  vanilla's add order (`children()` by `getTabOrderGroup`). Horizon has no
  widget tree, and a screen's buttons and other targets are registered
  separately, so their draw order isn't one sequence; vanilla's layouts add
  widgets row by row, which reading order follows.
- Tab visits the visible widgets only (a scrolled-away options row is
  reached with the arrows, which scroll); a list is entered on its first or
  last visible row, not its selected one.
- A focused slider whose arrows don't change it yet shows the hovered look,
  not vanilla's highlighted track.
- The widget under the real mouse isn't highlighted while the keyboard has
  the pointer (vanilla highlights both).
- The narration reads a slider's `canChangeValue` from the input type, so
  Enter sets the input type to match.

### Pointer

- Left stick moves the crosshair with acceleration. Let go and it glides
  onto the nearest slot or control; a short flick toward a neighbouring slot
  lands on that slot rather than back where it started. The rules and their
  numbers live in one place (`horizon_client_input::snap`).
- D-pad hops to the nearest target that way (a slot, a button). Held, it stops
  at the edge of the slots it is on; a fresh press crosses.
- A = left click, X = right click, Y = quick move, B = back; View and the left
  stick click throw, as in focus mode.
- LB / RB switch tabs (creative, Statistics, Advancements); with the recipe book
  open they turn its pages, stopping at the ends, and LT / RT change its
  category. Right stick scrolls.
- Sliders: hold A on the slider and move the stick, or nudge with the D-pad.
- **The recipe book's categories**: RT changes to the next category as it goes down. LT changes to the previous one
  as it goes down on a furnace's, smoker's or blast furnace's book; on the crafting book (the inventory's and the
  crafting table's), where LT + A crafts, it changes it as it's let go instead, however long it was held, unless A
  was pressed meanwhile.
- **Direct crafting**: LT + A on a recipe crafts it once into the inventory.
  With both still held past the repeat delay it keeps crafting until either is
  let go, the items run out or the inventory is full. It's the recipe book's
  own placing and a quick move of the result, one request at a time, each
  waiting for the server's answer. On any server but single player's own, placements
  go at most one every 250 ms (`PLACE_GAP`), under Paper's default limit of 5 a second
  over 4 seconds; on a server that needs **Quick Craft on Servers** (below) every request
  also waits a tick (`REQUEST_GAP`). A quick tap
  makes one, whichever button comes up first, and each LT + A on the recipe while one
  is still in flight makes one more. An LT + A it refuses is plain A. The inventory's guide says so
  with the pad's own glyphs, on a recipe, while the pad is in use, nothing is in the hand and it's allowed there.
  Letting go of either button, any other press, the pad going away or focus
  leaving the window all end it one way: no new craft starts, the one in flight
  finishes, and what's in the grid goes back, so the grid ends empty unless
  something can't fit. If the screen itself has gone, the game returns the grid
  as it does on closing any screen. A result only moves with room for all of
  it, as the client last heard the inventory, so a full inventory stops the craft with the
  ingredients back in the inventory. A pickup the client hasn't heard of yet can take
  that room first, and then the server's quick move drops the rest, as a vanilla
  shift-click on the result does. What's in the grid goes back first, and
  what a recipe leaves behind (bottles, buckets) follows the result, or stays
  in the grid if there's no room. A stack in the hand refuses it. It's on by itself
  only in single player, a splitscreen guest's join of that world, and on a LAN world
  joined from the server list's LAN entries (and that world's reconnects). Everywhere else,
  a server joined by address included, even one on this computer or the local network
  (it can be a proxy, a tunnel or a relay), and Play with Friends, it's off until **Quick
  Craft on Servers** is on in Controller Settings > General, as a server's rules may count
  it as a macro.
- **When direct crafting stops short**, the craft ends and nothing says so but the grid, in view on the open
  screen. A result that isn't the recipe's (item and count, as a server with recipes of its own can number the
  book differently) stops it with nothing moved, as a click on the recipe would leave it. Two seconds with no
  answer from the server stops it too, and nothing more goes out, as a quick move can't return what a late answer
  has yet to place: what the server placed stays in the grid. Either way, closing the screen returns the grid, and
  so does the next LT + A before it places anything.

The cursor is drawn only while the pad is the last-used input.

### Coming back to a screen

The pad remembers its focus on each screen it leaves and lands there again
on return (Options, after its Done), as vanilla keeps a screen's focused
widget. Moving the pad's focus drops the highlight a click gave a button, so
only one widget ever shows as focused.

The focus means the widget, not the place: the rows under it can have moved, or a resize or a new
GUI scale moved everything. So it is found again by its label (`widgets::slug`) when exactly one
widget has that label, and otherwise by its rectangle.

### Prompts

A prompt is a server dialog, the code of conduct, a link or command confirmation or a server pack
prompt (`Game::prompt_open`), over gameplay or over another screen. The pad drives it by focus, as a
menu, and nothing it presses reaches the game or the screen behind:

- Of the buttons only A, B, Start and the D-pad do anything.
- The focus starts on what answers no (its enabled Cancel or Back), so a press meant for the game
  can't accept anything. A dialog with text to fill in starts on its first input instead, and a prompt
  with neither starts on its first widget in tab order, a dialog's warning icon last.
- B and Start answer as Escape, the top prompt first: a command or link confirmation or a pack prompt
  says no, a dialog closes only if it allows that, and the code of conduct stays, since it has no
  answer by Escape.
- The right stick and a step past a list's edge scroll only the prompt, as the mouse wheel does.
- A prompt coming or going closes the on-screen keyboard and starts the focus again on the new frame.
  What a prompt covers stops counting (`widgets::cover_frame`): a buried button has no pad button.
- Once a prompt closes, a keyboard it took from chat or a sign opens again, and on a crosshair screen
  the crosshair glides back to where it was.

### Text fields

A on a focused text field (A in pointer mode clicks into it) opens the
**on-screen keyboard**: a QWERTY grid and a symbols grid in Horizon's design
language, at the bottom of the screen (at the top when the field is in the
lower half, as chat is). D-pad or stick to move, A types, X deletes, Y is a
space, LT is Shift (twice: caps lock), RB switches to symbols and accents,
Menu is Enter, B closes it. It opens by itself when the pad opens chat or a
sign, but not over the chat a sleeper gets (the game opens that one): there
it waits for A on the chat box. It types into chat (local and server), signs, books being written, the title screens' fields,
every options page's fields (searches, the packs', the language list's,
splitscreen profiles), the creative and recipe-book searches, the mod browser's search, the anvil, the
LAN screen's port and its game rules, a server dialog's inputs, command blocks, the game master blocks'
edit screens, Player Social's search and Debug Options' search. The one exception is a native mod's
own screen (`Screen::Mod`), which takes its input through the screen hooks: the keyboard types nothing
there.

Enter (Menu, or the keyboard's Done key) closes the keyboard, and on the way:

- reaches the field as the key would in chat (it sends), on signs and books, on command blocks (their
  Done) and game master blocks, on the options pages and on the title's fields;
- does nothing more on the searches, the anvil, the LAN screen and a dialog's one-line input;
- on a dialog's input of many lines is a new line, and the keyboard stays.


On a Steam Deck under Steam (`SteamDeck=1`), A asks Steam for its own
keyboard instead (`steam://open/keyboard`), which types like a real keyboard;
Steam + X opens it at any time. "On-Screen Keyboard: Auto / Horizon / Steam"
(`keyboard:`) picks one.

### Screens covered

Both modes send the same pointer events, but neither reaches everything. Focus mode
registers targets on: the title menus (main, world list and its search, create
and edit world, delete, multiplayer list and its edit screens), every options
page (vanilla's and Horizon Settings with the pages it opens, key and
controller binds, language list), pause and Play with Friends, death, chat, signs and books (their buttons), the
survival and creative inventories, every container and station (chests,
furnaces, brewing, crafter, crafting table with its recipe book, anvil,
grindstone, smithing, stonecutter, loom, cartography, enchanting, beacon,
villager and wandering-trader trades, mount inventories), and the LAN /
world options. Where a screen draws something clickable no target names
(the advancements map, the statistics lists, the pack screen's rows, the
book's page text), the pad drives the pointer instead.

## Glyphs

A button is drawn as Horizon's own art where its family has a picture, and otherwise as the label
printed on the pad, in its colour, on a dark key (`pad_glyph.rs`; no Mojang assets, nothing from the
player's jar). Whatever draws or measures a glyph goes through that module.

| Family | Art | Text |
|---|---|---|
| Xbox, also Auto, Steam Deck and anything unknown | `xb_*`: a b x y, lb rb lt rt, view menu, l3 r3, up down left right | none |
| PlayStation | `psx_*`: cross circle square triangle, l1 r1 l2 r2, create options, l3 r3, up down left right | none |
| Nintendo | `nx_*`, by what the pad prints: b a y x at south, east, west and north, l r zl zr, minus plus, l3 r3, up down left right | none |

Every family has its own file for every button, so a family's art can change without touching another's.
Some are copies until theirs is drawn: Nintendo's letters and shoulders are Xbox's, and all three share the
same sticks, D-pad and menu pictures. Text remains for what runs inside a line of text, and for an atlas
without the sprite.

The art is in `crates/horizon-client/assets/horizon/textures/gui/controller/`, built into the binary, one
line of `pad_glyph::ART` per picture: a button with no line is text, and adding one is that line and its
file. A resource pack replaces a picture by bringing the same path,
`assets/horizon/textures/gui/controller/<file>.png`: the atlas packs other namespaces' `textures/gui/` as
`horizon:gui/controller/<file>` and adds Horizon's own only when that sprite is missing (`HudAtlas`). A
pack's picture of another size is drawn in the same place, and a pack can't add a picture for a button
with no line. An atlas without the sprite falls back to text.

The picture's texture is 32 pixels tall, drawn 16 GUI pixels tall with a clear edge of 2 all round, so a
glyph shows 12 tall, as a text glyph does: a square picture 12 wide, a bumper or trigger 20. A declared
button's label leaves room for it.

Art shows on declared buttons and a focused field's A, in notices, the gameplay guide and splitscreen's
strips. What runs inside a line of text stays text: the recipe tooltip, the on-screen keyboard's hint line and the buttons on
the Bindings page. Vanilla's own messages that name a key (tutorial hints) keep the keyboard's names.

The inventory crosshair is `crosshair_inv` in the same folder, drawn at 16 GUI pixels on the point it
clicks (`pad_cursor.rs`).

The family comes from the pad's USB ids (`GlyphStyle::of_device`) unless Controller Settings > Device
forces one: Sony 054c and a few licensed pads (HORIPAD 4 FPS and FPS Plus, Razer Raiju, Nacon Revolution
Pro) are PlayStation; Nintendo 057e and a few licensed pads (HORIPAD for Nintendo Switch, PowerA Wired
Plus, PDP Faceoff Wired Pro) are Nintendo; Microsoft, Valve's Steam Deck and Steam Input's virtual pad,
and everything else are Xbox.

## Options and remapping

Controller settings are Horizon's own: **Options → Controls → Controller
Settings...** (or **Options → Horizon Settings... → Controller Settings...**),
a screen in the graphics settings' style with a search field and General, Bindings and Device pages. Bindings
is a list like the key binds screen (pick an action, press a button; conflicts marked as vanilla does), and
Reset. General's Play group has the **Gameplay Guide** setting (Full, Basic or Off) and **Trigger Threshold**, how far a trigger is pulled before it counts as pressed:
0.1 to 0.9, by default 0.4, which also places a DualSense's catch (see "Adaptive triggers"). The screen
names the player whose profile it edits and the controller the game sees; Device also shows its vendor and
product ids, the detected glyph family and the override. Changes apply at once; **Undo** (X) is there only
while something differs from when the screen opened, and **Done** (B) leaves. They live in
`horizon-controller.txt` next to `options.txt`, like `horizon-graphics.txt`: `options.txt` stays
exactly vanilla's, so a vanilla client can share the game folder.

```
enabled:true
vibration:true
adaptiveTriggers:true
lightbar:health
lookSensitivity:0.5
invertX:false
invertY:false
moveDeadzone:0.15
lookDeadzone:0.12
triggerThreshold:0.4
toggleSneak:true
glyphs:auto
bind_key.jump:south
craftOnServers:false
gameplayGuide:full
bind_pad.hotbarNext:rb
bindLayout:1
```

Only bindings that differ from the defaults are written. A file without `bindLayout` comes from before the
current defaults and keeps its buttons where they were.

Deadzones are radial (the stick's length, so diagonals aren't clipped) and the
remaining range is rescaled to 0..1. Look uses a squared curve for precise
aim near the centre, up to 200°/s yaw and 140°/s pitch at the default
sensitivity, slower while scoping with a spyglass.

## Playing in the background

Play in Background (off by default, per player) lets the pad keep driving the game while its window
has no focus, and a game the pad is playing doesn't pause on losing focus. Every game on the PC reads
every pad: for a pad in one window and the keyboard in another, turn Controllers off in the keyboard's.

## Gameplay guide

While the pad plays, the HUD's top corners say what its buttons do now: break, use and pick block
at the top right when there is something to do them to, movement and the player's own actions at
the top left. The rows follow the bindings, the game mode, what the crosshair is on, the held
items, riding, swimming and flying, and an item in use; an action that would do nothing, or has no
button, has no row. The mouse or keyboard taking over hides them, as do a menu, F1, F3 and the
Gameplay Guide option. They read what a click would do from the client's own answers
(`remote_block_reacts`, `remote_mob_takes`), and a use only a fluid ray or the server can tell (an
empty bucket) shows nothing. The rows step around what the HUD drew (effect icons, boss bars, the
sidebar, chat, toasts, splitscreen's badge), and in a small view the last rows drop.

An inventory or creative screen the pad drives has rows of its own, from its bottom corners, under
the same option, and what's under the crosshair decides them: a slot, by whether it holds anything
and what's carried; a recipe; another of the book's controls. Bottom right, what one face button
does: Close, always the lowest row, then Take, Put Down, Quick Move, Place Recipe and so on. Bottom
left, the rest: LT + A where direct crafting is allowed, View and the left stick's drops, the recipe book's pages and categories, and creative's
tabs. The creative grid's items aren't slots, so creative has only Close and its tabs.

## Rumble

Vibration (on by default, off in the options): taking damage (stronger with
the damage), a block broken, an explosion nearby (scaled by distance), a totem
of undying. Short, low-intensity pulses, as on Bedrock. A DualSense gets them
shaped, through its own output report (below), on every OS. Other pads go through
gilrs force feedback: Linux (evdev force feedback, including Steam Input's
virtual pad) and Windows support it; macOS gilrs has no force feedback, so it
is silently skipped there.

## DualSense output

Gilrs only reads the pads. A DualSense (054c:0ce6, the Edge 054c:0df2) is also
written to directly, over HID: the `horizon-dualsense` crate builds its output
report and [hidapi](https://crates.io/crates/hidapi) (MIT, bundling the C
hidapi under its BSD option) sends it: hidraw on Linux, IOKit on macOS (opened
shared, so gilrs keeps reading the same pad), the HID API on Windows.

- **Report.** One 47-byte block carries everything: which parts to apply
  (three flag bytes), the two compatible-vibration motors, both adaptive
  triggers' effects, the lightbar colour and the player LEDs. Over USB it is
  report 0x02; over Bluetooth report 0x31, led by a sequence number and a tag
  and closed by a CRC-32 of the seed byte 0xA2 and the report. Unit tests pin
  the bytes to the references below (the CRC to zlib's, the LED reset to the
  kernel's).
- **Rumble.** Every report carries the motors: a report without the rumble
  flags ends the motors' emulation and hands the actuators back to the audio
  haptics (that is how SDL stops rumble, and how the HD haptics are re-armed),
  so a trigger change mid-rumble would otherwise cut it. A pad with update
  version 0x0224 or later (feature report 0x20), and every Edge, takes the
  newer emulation (flags 2, bit 2); older or unknown firmware the legacy one
  (flags 0, bit 0), at half strength as SDL does. The kernel takes the newer
  one from 0x0215; between the two, legacy works on every firmware. The first
  report after opening clears the motor and trigger attenuation another host
  (a PS5's intensity settings) may have left (flags 1, bit 6, byte 36 = 0).
- **Lights.** Before the first colour, a report of its own resets the lights
  (flags 1 bit 3, and the lightbar setup "light out"; the kernel does the
  same). Over Bluetooth the pad plays a connection animation that paints over
  the lightbar: like SDL, the colour waits until the pad's sensor clock
  (input report 0x31) passes 10 200 000.
- **Bluetooth.** Any output report, even reading a feature report, switches a
  pad on Bluetooth to its full input report (0x31) until it is turned off. On
  Linux `hid-playstation` has done that already and gilrs reads its events,
  so output goes ahead. Gilrs on macOS and Windows parses the simple report
  0x01 instead and would stop seeing the pad (SDL's
  `SDL_HINT_JOYSTICK_ENHANCED_REPORTS`: "break Windows DirectInput for other
  applications"), so there a pad on Bluetooth gets no output unless
  `HORIZON_DUALSENSE_BT=1`. Doing it for everyone means reading the pad's
  input ourselves over HID, as SDL does (a later PR). A pad on both cables is
  written over USB.
- **Writer thread.** Writes block (a Bluetooth write can take milliseconds),
  so a thread owns the pad: the game hands it the state to show and timed
  vibrations, and it sends only what changed, and everything again when the
  pad reconnects. It opens the pad once this window has the pad in hand (the
  pad is active and the window has focus), and looks again whenever gilrs
  sees a controller come or go.
- **Off-safe.** Quitting (and the thread ending any other way) frees the
  triggers and stops the motors before the pad is closed. Another game on the
  same pad is left alone: nothing is written while this window is unfocused.
- **Fallback.** If the pad can't be opened (Linux without access to its
  hidraw node), vibration silently stays with gilrs, as before.
- **Linux access.** `/dev/hidraw*` is root-only by default. Steam's udev rules
  (`60-steam-input.rules`, installed with Steam or the `steam-devices`
  package) already give the logged-in user the DualSense, over USB and
  Bluetooth. Without Steam, this rule does the same (the launcher could
  install it):

  ```
  # /etc/udev/rules.d/70-horizon-dualsense.rules
  KERNEL=="hidraw*", ATTRS{idVendor}=="054c", ATTRS{idProduct}=="0ce6|0df2", TAG+="uaccess"
  KERNEL=="hidraw*", KERNELS=="*054C:0CE6*|*054C:0DF2*", TAG+="uaccess"
  ```

  The kernel's own `hid-playstation` driver keeps working alongside: its
  reports set only the parts their flags name.
- **Trying it without the game:** `cargo run -p horizon-dualsense --example
  dualsense -- all` plays each output on a connected pad and reads back what
  the pad reports: the triggers' active effect, and the accelerometer's
  shake while the motors run (`watch` logs the shake while the game drives
  the pad).

### Adaptive triggers

On by default with a DualSense ("Adaptive Triggers" in the controller
options, `adaptiveTriggers:`). Each trigger feels the action bound to it, so
rebinding moves the feel along: with the default mapping that is L2 for
`key.use` and R2 for `key.attack`.

| While | Trigger | Effect (zones 0..9, strength 1..8) | Feel |
|---|---|---|---|
| Drawing a bow | use | feedback from zone 2, strength 1 rising to 6 with the draw (`BowItem.getPowerForTime`) | the string's tension builds; letting go frees the trigger at once |
| Charging a trident | use | the same, at 6 after the 10 ticks a throw needs | |
| Loading a crossbow | use | the same, at 6 when it is loaded | |
| Holding a loaded crossbow | use | weapon: from two zones before the press zone to the press zone, strength 8 | a gun trigger: a hard catch that breaks with a click as it fires |
| Gliding with a rocket, riding a pig or a strider with its stick | use | weapon, the same zones, strength 5 | a lighter catch: pulling through it is the boost |
| A shield in the use hand (raised or ready) | use | feedback from zone 2, strength 3 | its weight |
| That shield disabled by an axe | use | feedback from zone 1, strength 8 | the trigger locks until the shield recovers |
| Mining (survival) | attack | vibration from zone 1: amplitude 2 (dirt) to 5 (obsidian) of 8, 40 Hz on sand and dirt, 30 on stone, 20 on wood, deepslate and ores, 10 on obsidian | a grit that gets heavier and slower on harder blocks; none on blocks that break at once |

The press zone is where Horizon presses the trigger's action: the trigger
threshold (0.4 by default) times ten, kept between 3 and 8, so with the
defaults a crossbow catches from zone 2 and breaks at zone 4, as its bolt
leaves. This assumes the ten zones split the travel evenly, as dualsensectl
documents them (0 at rest, 9 fully pressed); it is not measured.

Everything else, and every menu, the pause screen, a window without focus,
another pad being the active one, and controllers turned off, leaves the
triggers free; quitting frees them too. `pad status` shows the effects in
use.

Why these values, from what shipped games do and what the community
documents (none of it measured on Horizon's pad yet):

- **Only the official modes.** Off 0x05, feedback 0x21, weapon 0x25 and
  vibration 0x26 are the modes Nielk1's notes call official and expected in
  future firmware; bow 0x22, galloping 0x23 and machine 0x27 "might be
  removed", and the low "simple" modes (0x01, 0x02) that pydualsense and
  DualSense-Windows send are leftovers. The bow mode's snap-back would suit a
  bow, but a firmware update could take it away.
- **Bows build tension.** Kena: Bridge of Spirits and Astro's Playroom use
  the triggers to make you feel the bow being drawn (PlayStation Blog's
  DualSense round-up, as reported by
  [GameSpot](https://www.gamespot.com/articles/sony-reveals-how-ps5s-dualsense-controller-works-with-ratchet-clank-returnal-scarlet-nexus-and-more/1100-6491469/)). Minecraft holds the draw for
  as long as you like, so the tension stops at 6 of 8 (dualsensectl: 1 is
  "almost no resistance", 8 the maximum), and starts after the first two
  zones so the press that starts the draw stays light.
- **A loaded crossbow is a gun trigger.** The developer note GameSpot quotes
  describes a bazooka whose resistance "will cause the trigger to catch at the
  input position just before the bullet is fired, and as you continue to pull
  the trigger, the bullet will fire and release the tension": the weapon
  effect does exactly that, and Horizon moves its breaking point onto the
  threshold so the click is the shot. DSX community presets use the same
  shape (DSX++'s example `Weapon {2, 6, 8}`; dualsensectl's
  `weapon 2 3 8` "resists strongly and lets go almost immediately").
  Returnal and Ratchet & Clank: Rift Apart use a half-press stop to split two
  actions on one trigger; Minecraft has one action per trigger, so Horizon
  doesn't.
- **Boosts click lighter.** A rocket or a pig's carrot fires on the pull, so
  it gets a catch that gives way rather than a wall that never does (strength
  5, below the crossbow's 8).
- **A disabled shield locks the trigger**, as Deathloop locks R2 halfway
  when a gun jams, "to give to the player an immediate feedback even before
  the animation plays out" (its director, Dinga Bakaba, in
  [Destructoid's report](https://www.destructoid.com/when-a-gun-jams-in-deathloop-the-ps5-controllers-triggers-will-lock-up/)):
  the shield can't be raised, and the finger feels why.
- **Mining is a texture.** The vibration mode buzzes the trigger at a given
  rate; reWASD's preset uses 30 Hz and its "rifle" 2..20 Hz. Softer blocks
  get a finer, faster grit, harder ones a slower, heavier chug, never above
  5 of 8, since the attack trigger is held for seconds.

### Vibration shapes and HD haptics

Each event plays a short shape rather than one flat block: a hit opens at
full strength for 50 ms and dies away in two steps, an explosion rolls on for
half a second, a totem swells, a broken block is a 35 ms tick. Over HID they
drive the motors' emulation (the compatible-vibration bytes). Measured on the
pad, that emulation has inertia: after a full-strength step it takes about
half a second to settle, so short shapes blur.

The DualSense has no motors, but two voice-coil actuators, and on USB it is
also a four-channel 48 kHz audio device whose channels 3 and 4 drive them
(1 and 2 are its speaker and headset). Played there as waveforms (an 80 Hz
tone for the heavy part, 200 Hz for the light one, 4 ms ramps between steps,
channels 1 and 2 silent), the same shapes come out crisp. Per OS:

| | USB | Bluetooth |
|---|---|---|
| macOS | the pad lists as a 4-channel CoreAudio output ("DualSense Wireless Controller", seen on the Mac mini by dualsense-1); cpal opens it by name | no audio device |
| Linux | snd-usb-audio exposes the card "Controller" with 4 channels; cpal opens it through ALSA (`plughw:CARD=Controller`), which fails while PipeWire or PulseAudio holds the card, as desktops do: then the motors' emulation stays | no audio device |
| Windows | a WASAPI endpoint named "Wireless Controller" (or "DualSense Wireless Controller"), stereo until its speaker setup is set to Quadraphonic in the Sound control panel (`mmsys.cpl`, Configure); until then Horizon finds no 4-channel output and keeps the motors | no audio device |

The audio haptics play only while the pad is out of its rumble emulation:
any report with the rumble flags (flags 0, bits 0 and 1) silences them until
a report without them (SDL: "Leaving emulated rumble bits off will restore
audio haptics"; nondebug/dualsense and
[dse-haptics](https://github.com/iranran666/dse-haptics) arm them with an
all-zero report 0x02). While HD haptics are open the motors stay at zero, so
every report Horizon sends, starting with the one at opening, keeps them
armed; that first report also clears the attenuation a PS5 may have left.
The frame layout `[0, 0, left, right]` at 48 kHz is the one
[KytyPS5 #843](https://github.com/KytyPS5/KytyPS5/pull/843) plays on
Windows.

Over Bluetooth the haptics travel as 8-bit 3 kHz samples in vendor output
report 0x32, reverse-engineered by
[SAxense](https://github.com/egormanga/SAxense) and since used by DSX 3.2
and [dualsense-bt-haptics](https://github.com/awalol/dualsense-bt-haptics);
it needs Bluetooth output, which only Linux gets (see above), and isn't
done: there the motors' emulation is used. HD haptics are opt-in for now
(`HORIZON_DUALSENSE_HAPTICS=1`) until they are confirmed on a real pad;
games started for tests (`HORIZON_NO_AUDIO`) never open them, and the pad's
audio device is only ever opened by name, never the system's default output.

### Lightbar and player LEDs

"Lightbar: Health / Dimension / Off" in the controller options
(`lightbar:`, health by default). Health runs from green at full through
amber at half to red; Dimension is blue in the Overworld, orange in the
Nether, violet in the End. Both at half brightness (the pad's full light is
glaring); menus keep the last colour. The player LEDs show player 1 (the
middle LED). Off leaves both to the pad. When Horizon lets go of the pad, a
lightbar or LEDs it changed go back to a host's defaults: blue (0, 0, 128)
and the middle LED, as Linux's driver gives a first pad (neither SDL nor
the kernel resets anything when it lets go; Horizon does, so the pad never
keeps a game's colour or a stiff trigger). The microphone's mute LED is
never touched (flags 1, bit 0 stays clear): whatever the system set
stays. Over Bluetooth the colour waits for the pad's connection
animation (see "DualSense output"). Verified against the references only.

Byte layout from public references, read for facts; no code is copied from
them (the kernel and dualsensectl are GPL):

| Reference | Read at | What it settles |
| --- | --- | --- |
| SDL [`SDL_hidapi_ps5.c`](https://github.com/libsdl-org/SDL/blob/6defbc0129b5ba90bab3f6c67bbc1e9329e24ff1/src/joystick/hidapi/SDL_hidapi_ps5.c) and [`SDL_hints.h`](https://github.com/libsdl-org/SDL/blob/6defbc0129b5ba90bab3f6c67bbc1e9329e24ff1/include/SDL3/SDL_hints.h) (zlib) | `6defbc0` | the 47-byte block, rumble flags and stop, 0x0224, legacy at half, LED reset and the Bluetooth animation clock, USB report length 48, the enhanced-mode switch |
| Linux [`hid-playstation.c`](https://github.com/torvalds/linux/blob/045958acaa75205b31578cb22c4345e28dd2e4bf/drivers/hid/hid-playstation.c) (GPL) | `045958a` | valid flags, the Bluetooth sequence and tag 0x10, CRC seed 0xA2, `dualsense_reset_leds`, `use_vibration_v2` from 2.21, player LED patterns, the default blue |
| [dualsensectl](https://github.com/nowrep/dualsensectl/blob/d3ae2fa272e1bf9a5c08a0ded3f374b40f7c73ac/main.c) (GPL) | `d3ae2fa` | trigger valid bits (flags 0 bits 2, 3), `reduce_motor_power` (byte 36) and its flag, the trigger effect packing |
| Nielk1's [TriggerEffectGenerator](https://gist.github.com/Nielk1/6d54cc2c00d2201ccb8c2720ad7538db/f73a299fdc52f43be84d39e62734192ad82bb393) (MIT) | rev. `f73a299` | the trigger modes (official 0x05, 0x21, 0x25, 0x26) and their zone packing |
| [pydualsense](https://github.com/flok/pydualsense/blob/01445f5ed6ba/pydualsense/pydualsense.py) (MIT) | `01445f5` | sets the attenuation flag with 0 on every report |
| [nondebug/dualsense](https://github.com/nondebug/dualsense) | `b87450e` | an all-zero report 0x02 leaves rumble emulation for the audio haptics |
| [KytyPS5 #843](https://github.com/KytyPS5/KytyPS5/pull/843), [dse-haptics](https://github.com/iranran666/dse-haptics), [SAxense](https://github.com/egormanga/SAxense) | read 2026-10-15 | the 4-channel layout, the Windows and Linux device names and setup, the Bluetooth haptics report |

Verified on hardware (USB, dualsense-1, Mac mini): the motors with the V2
flags (the IMU shook, and settled after a stop that then still sent the
flags), the pad reporting a feedback and a weapon trigger effect as active,
and the writer thread. Verified against the references only: everything
else, the flag-less stop, the attenuation and LED reset reports, and all of
Bluetooth.

## Steam Deck

- In Game Mode, or any game launched through Steam, Steam Input presents the
  Deck's controls as a virtual Xbox pad: Horizon sees it like any Xbox pad
  and uses Xbox-letter glyphs (the Deck's face buttons are A B X Y). Steam's
  default "Gamepad" layout works with no setup; the trackpads stay mouse.
- Started outside Steam on the Deck, the kernel's `hid-steam` driver exposes
  the controls as a gamepad too (while Steam isn't holding them), so it also
  works out of the box.
- Text entry uses Steam's keyboard (Steam + X).

## PR plan

The first round (input layer, menus, glyphs and options, rumble) shipped in
PRs #120, #129, #145 and #149. The second round, each shippable alone:

1. **Focus navigation** (this section above): focus targets and vanilla's
   arrow-key step, vanilla's focus highlight, the on-screen keyboard.
2. **Button prompts**: contextual hints with the pad's glyphs in menus and
   optionally in game, and rebinding from them.
3. **DualSense output** (above): the HID output layer and rumble through it
   first, then adaptive triggers, shaped vibration and the lightbar and
   player LEDs.
4. **Launcher**: the same focus navigation in the gpui launcher.

Each is tested with unit tests (focus steps, prompt contexts, report bytes),
the injected pad and vscreen screenshots; each PR says what still needs a
real controller.
