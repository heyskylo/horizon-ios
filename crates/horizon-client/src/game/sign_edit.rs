//! The sign editing screen (`SignEditScreen` / `HangingSignEditScreen`):
//! opened when a sign is placed, four lines limited to the sign's line
//! width, Up/Down/Enter move between lines, Escape or Done closes.
use super::{
    Game,
    commands::{Target, execute::Source},
    lan::LanHost,
};
use horizon_client::{
    containers::BlockEntity,
    hud::{HudBatch, rgba},
    items::ItemStack,
    signs::{ClickAction, DYE_TEXT, click_action, editor_sprite},
    ui::EditBox,
};
use horizon_protocol::packets::play::{self, Clientbound as C, Serverbound as S};
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{KeyCode, PhysicalKey},
};

pub(super) struct SignEditor {
    pub pos: (i32, i32, i32),
    pub front: bool,
    pub line: usize,
    /// The line being edited (`AbstractSignEditScreen`'s text field).
    edit: EditBox,
    /// `AbstractSignEditScreen.messages`: the side's lines as edited, sent
    /// when the editor closes, even if the sign is gone by then.
    lines: [String; 4],
}

fn sign_box() -> EditBox {
    let mut edit = EditBox::new(0.0, 0.0, 90.0, 9.0);
    edit.bordered = false;
    edit.max_length = 384;
    edit.focused = true;
    edit
}

/// The Done button's text, `CommonComponents.GUI_DONE`, in the selected language.
fn done_label() -> String {
    super::screen_info::tr("gui.done", &[])
}
/// AbstractSignEditScreen.init's Done button: `width / 2 - 100`,
/// `height / 4 + 144`, int GUI pixels.
fn done_button(w: f32, h: f32) -> (f32, f32) {
    (
        (w.ceil() / 2.0).floor() - 100.0,
        (h.ceil() / 4.0).floor() + 144.0,
    )
}

impl Game<'_> {
    /// `SignBlockEntity.getSlotPlayerIsFacing`: whether the player stands in
    /// front of the sign's text (within 90 degrees of its facing).
    fn facing_sign_front(&self, pos: (i32, i32, i32)) -> bool {
        let Some(player) = self.player.as_ref() else {
            return true;
        };
        let id = self.world.block(pos.0, pos.1, pos.2);
        let Some(state) = self.pack.assets.registry.state(id) else {
            return true;
        };
        let form = self.res.facts().get(id).sign_form();
        let prop = |key: &str| state.properties.get(key).map(String::as_str);
        // getYRotationDegrees: a wall sign's `Direction.toYRot`, a standing or
        // ceiling sign's rotation segment.
        let y_rot = if form.is_some_and(|f| f.wall) {
            match prop("facing") {
                Some("west") => 90.0,
                Some("north") => 180.0,
                Some("east") => 270.0,
                _ => 0.0,
            }
        } else {
            prop("rotation")
                .and_then(|r| r.parse::<f32>().ok())
                .unwrap_or(0.0)
                * 22.5
        };
        // getSignHitboxCenterPosition: only a wall sign's board is off-centre,
        // against the wall it hangs on.
        let centre = if form.is_some_and(|f| f.wall && !f.hanging) {
            match y_rot as i32 {
                90 => (0.9375, 0.5),
                180 => (0.5, 0.9375),
                270 => (0.0625, 0.5),
                _ => (0.5, 0.0625),
            }
        } else {
            (0.5, 0.5)
        };
        let xd = player.position.x - (f64::from(pos.0) + centre.0);
        let zd = player.position.z - (f64::from(pos.2) + centre.1);
        let player_y_rot = (zd.atan2(xd).to_degrees() - 90.0) as f32;
        // Mth.degreesDifferenceAbs.
        let diff = (player_y_rot - y_rot).rem_euclid(360.0);
        let diff = if diff >= 180.0 { 360.0 - diff } else { diff };
        diff <= 90.0
    }
    /// `SignBlockEntity.canExecuteClickCommands` for the side the acting
    /// player faces: a waxed sign with a `run_command` click event on it.
    pub(super) fn sign_can_execute_click_commands(&self, pos: (i32, i32, i32)) -> bool {
        // Only a waxed sign can; skip the facing maths for everything else.
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get(&pos) else {
            return false;
        };
        sign.waxed && sign.can_execute_click_commands(self.facing_sign_front(pos))
    }
    /// `WallHangingSignBlock.shouldTryToChainAnotherHangingSign` and
    /// `CeilingHangingSignBlock.shouldTryToChainAnotherHangingSign`: a hanging
    /// sign item clicked on a hanging sign's free side chains another sign onto
    /// it, unless the side the player faces carries click commands the click
    /// runs instead (`canExecuteClickCommands`).
    pub(super) fn chains_hanging_sign(
        &self,
        pos: [i32; 3],
        face: [i32; 3],
        held: Option<&ItemStack>,
    ) -> bool {
        // `itemStack.getItem() instanceof HangingSignItem`, by the item's class
        // and not its name.
        let hanging_sign = held.is_some_and(|s| {
            self.res
                .behaviours()
                .item_binding(&s.item)
                .is_some_and(|i| i.kind == horizon_world::level::ItemKind::HangingSign)
        });
        let key = (pos[0], pos[1], pos[2]);
        if !hanging_sign {
            return false;
        }
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let Some(form) = self
            .res
            .facts()
            .get(state)
            .sign_form()
            .filter(|f| f.hanging)
        else {
            return false;
        };
        let free_side = if form.wall {
            // `isHittingEditableSide`: a wall hanging sign's board lies in its
            // FACING's plane, so its two faces are the editable text and only
            // its edges chain.
            let Some(facing) = self
                .pack
                .assets
                .registry
                .state(state)
                .and_then(|s| s.properties.get("facing"))
            else {
                return false;
            };
            let axis = match &**facing {
                "east" | "west" => 0,
                "up" | "down" => 1,
                _ => 2,
            };
            face.iter().position(|c| *c != 0) != Some(axis)
        } else {
            // `hitResult.getDirection().equals(Direction.DOWN)`.
            face == [0, -1, 0]
        };
        free_side && !self.sign_can_execute_click_commands(key)
    }
    /// `SignBlockEntity.createCommandSourceStack`: the acting player runs the
    /// command from the middle of the sign's block, rotation zero, at the
    /// game master level, with `CommandSource.NULL` (no feedback, no failure
    /// messages).
    fn sign_command_source(&self, pos: [i32; 3]) -> Source {
        Source {
            entity: self.player.is_some().then_some(Target::Player),
            pos: pos.map(|v| f64::from(v) + 0.5),
            rot: [0.0, 0.0],
            eyes: false,
            silent: true,
            store: None,
            dimension: self.dim,
            level: None,
            block: None,
            cart: None,
        }
    }
    /// Run a sign's `run_command` line as the prefixed command vanilla runs
    /// it as: a leading slash is allowed, errors are not reported (the NULL
    /// source).
    fn run_sign_command(&mut self, pos: [i32; 3], command: &str) {
        let source = self.sign_command_source(pos);
        let line = command.strip_prefix('/').unwrap_or(command);
        self.exec = Default::default();
        let permission = std::mem::replace(&mut self.ui.registries.permission, 2);
        let _ = self.execute_line(line, source);
        self.ui.registries.permission = permission;
        // InventoryChangeTrigger fires as slots change, not on the next tick.
        if self.player.is_some() {
            self.check_inventory_trigger();
        }
    }
    /// `SignBlockEntity.executeClickCommandsIfPresent`: every line of the
    /// side whose style has a `run_command`, `show_dialog` or `custom` click
    /// event runs it as the acting player when the sign allows op features;
    /// otherwise the player is told click actions are disabled. Whether any
    /// line had one.
    fn execute_sign_click_commands(&mut self, pos: [i32; 3], front: bool) -> bool {
        let key = (pos[0], pos[1], pos[2]);
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get(&key) else {
            return false;
        };
        let allow_op_features = sign.allow_op_features;
        let side = if front { &sign.front } else { &sign.back };
        // The side as it was when the loop began: a command may change it.
        let events: Vec<_> = side.click.iter().flatten().cloned().collect();
        let mut any_click_command = false;
        for event in &events {
            match click_action(event) {
                Some(ClickAction::RunCommand(command)) => {
                    if allow_op_features {
                        self.run_sign_command(pos, &command);
                    }
                    any_click_command = true;
                }
                Some(ClickAction::ShowDialog(dialog)) => {
                    if allow_op_features {
                        self.show_click_dialog(dialog);
                    }
                    any_click_command = true;
                }
                // MinecraftServer.handleCustomClickAction only logs it.
                Some(ClickAction::Custom { id, payload }) => {
                    if allow_op_features {
                        log::debug!("Received custom click action {id} with payload {payload:?}");
                    }
                    any_click_command = true;
                }
                None => {}
            }
        }
        if !allow_op_features && any_click_command {
            self.sign_click_actions_disabled();
        }
        any_click_command
    }
    /// `Player.sendOverlayMessage(CLICK_ACTIONS_DISABLED)`: red text on the
    /// action bar. A guest gets a packet, so its own client translates it.
    fn sign_click_actions_disabled(&mut self) {
        const KEY: &str = "sign.click_actions_disabled";
        if self.lan_acting.is_some() {
            let message = horizon_client::text::Text::translate(KEY, &[])
                .with_color(horizon_client::text::RED);
            self.guest_packets.push(C::SystemChat(play::SystemChat {
                content: horizon_client::lan::data::component_nbt(&message.component()),
                overlay: true,
            }));
        } else {
            self.message = Some((
                self.translate(KEY, "Click actions are disabled for this sign"),
                std::time::Instant::now(),
            ));
        }
    }
    /// `SignBlock.useWithoutItem` (the block's `OpenMenu` effect): the side
    /// the player faces runs its click commands first; then a waxed sign
    /// plays the failed-interaction sound, a sign that ran one does nothing
    /// more, and otherwise a player who may build edits the side they face.
    pub(super) fn use_sign_empty_hand(&mut self, pos: [i32; 3]) {
        let key = (pos[0], pos[1], pos[2]);
        if !matches!(
            self.world.block_entities.get(&key),
            Some(BlockEntity::Sign(_))
        ) {
            return;
        }
        let front = self.facing_sign_front(key);
        let executed = self.execute_sign_click_commands(pos, front);
        // The commands may have changed the sign, or removed it.
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get(&key) else {
            return;
        };
        if sign.waxed {
            // `getSignInteractionFailedSoundEvent`: a hanging sign has its own.
            let sound = if sign.hanging {
                "minecraft:block.hanging_sign.waxed_interact_fail"
            } else {
                "minecraft:block.sign.waxed_interact_fail"
            };
            self.audio
                .play(sound, Some(pos.map(|v| f64::from(v) + 0.5)), 1.0, 1.0);
        } else if !executed
            && !self.other_player_is_editing_sign(key)
            && self.may_build()
            && self.sign_side_has_editable_text(key, front)
        {
            self.open_text_edit(key, front);
        }
    }
    /// `SignBlock.useItemOn` with a `SignApplicator` in hand (dyes, ink sacs,
    /// honeycomb): the item changes the side the player faces and is used up.
    /// Whether it did; a sign it does not apply to goes on to the empty hand.
    pub(super) fn use_sign_applicator(&mut self, pos: [i32; 3], held: Option<&ItemStack>) -> bool {
        let Some(stack) = held else {
            return false;
        };
        let key = (pos[0], pos[1], pos[2]);
        let waxed = match self.world.block_entities.get(&key) {
            Some(BlockEntity::Sign(sign)) => sign.waxed,
            _ => return false,
        };
        if waxed || !self.may_build() {
            return false;
        }
        let item = &*stack.item;
        let dye = if item.ends_with("_dye") {
            self.res
                .items()
                .get(&stack.item)
                .and_then(|i| i.dye.clone())
        } else {
            None
        };
        let colour = dye.and_then(|d| DYE_TEXT.iter().position(|(n, _)| *n == d));
        if !matches!(
            item,
            "minecraft:honeycomb" | "minecraft:glow_ink_sac" | "minecraft:ink_sac"
        ) && colour.is_none()
        {
            return false;
        }
        // `SignBlock.useItemOn`: a sign another player's editor is on takes no
        // dye, ink sac or honeycomb.
        if self.other_player_is_editing_sign(key) {
            return false;
        }
        let front = self.facing_sign_front(key);
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get_mut(&key) else {
            return false;
        };
        let centre = pos.map(|v| f64::from(v) + 0.5);
        let sound = if item == "minecraft:honeycomb" {
            // HoneycombItem.tryApplyToSign: waxes any sign, empty or not.
            sign.waxed = true;
            "minecraft:item.honeycomb.wax_on"
        } else {
            let side = if front {
                &mut sign.front
            } else {
                &mut sign.back
            };
            // SignApplicator.canApplyToSign: the side must have text.
            if side.is_empty() {
                return false;
            }
            match colour {
                Some(c) if side.color != c as u8 => {
                    side.color = c as u8;
                    "minecraft:item.dye.use"
                }
                None if item == "minecraft:glow_ink_sac" && !side.glowing => {
                    side.glowing = true;
                    "minecraft:item.glow_ink_sac.use"
                }
                None if item == "minecraft:ink_sac" && side.glowing => {
                    side.glowing = false;
                    "minecraft:item.ink_sac.use"
                }
                _ => return false,
            }
        };
        if item == "minecraft:honeycomb" {
            self.level_event_at(3003, pos, 0);
        }
        self.audio.play(sound, Some(centre), 1.0, 1.0);
        self.mark_block_entity_updated(key);
        self.execute_sign_click_commands(pos, front);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        self.post_block_game_event(
            "minecraft:block_change",
            pos,
            self.sculk_actor(),
            Some(state),
        );
        self.award_stat("used", &stack.item, 1);
        self.consume_held();
        self.trigger_block("minecraft:item_used_on_block", pos, stack);
        true
    }
    /// `SignBlock.openTextEdit`: the sign is this player's to edit while its
    /// editor is open (`setAllowedPlayerEditor`), and it opens.
    pub(super) fn open_text_edit(&mut self, pos: (i32, i32, i32), front: bool) {
        // `Minecraft.setScreen` drops the editor already open first, and that
        // sign is nobody's again.
        self.close_sign_editor();
        let who = self.sign_editor_uuid();
        self.world.block_entities.set_sign_editor(pos, Some(who));
        self.open_sign_editor(pos, front);
    }
    /// `Player.getUUID` of the acting player: the one a sign remembers as its
    /// editor.
    fn sign_editor_uuid(&mut self) -> u128 {
        horizon_client::lan::playerdata::ints_uuid(self.acting_player_uuid())
    }
    /// `SignBlock.otherPlayerIsEditingSign`: the sign at `key` belongs to
    /// someone else's editor right now.
    fn other_player_is_editing_sign(&mut self, key: (i32, i32, i32)) -> bool {
        match self.world.block_entities.sign_editor(&key) {
            Some(holder) => holder != self.sign_editor_uuid(),
            None => false,
        }
    }
    /// `SignText.hasEditableText` of the side the player faces: if every line is
    /// plain text, the editor rewrites nothing a player would miss.
    fn sign_side_has_editable_text(&self, key: (i32, i32, i32), front: bool) -> bool {
        match self.world.block_entities.get(&key) {
            Some(BlockEntity::Sign(sign)) => {
                let side = if front { &sign.front } else { &sign.back };
                side.has_editable_text()
            }
            _ => false,
        }
    }
    /// `SignBlockEntity.tick` (`clearInvalidPlayerWhoMayEdit`): a sign keeps its
    /// editor only while that player is here and within the block interaction
    /// range plus four blocks of it.
    pub(super) fn tick_sign_editors(&mut self) {
        let held: Vec<(i32, i32, i32)> =
            self.world.block_entities.sign_editors().copied().collect();
        for pos in held {
            match self.world.block_entities.sign_editor(&pos) {
                Some(who) if self.sign_editor_in_range(who, pos) => {}
                Some(_) => self.world.block_entities.set_sign_editor(pos, None),
                // A sign that is gone, or one no editor holds.
                None => self.world.block_entities.forget_sign_editor(pos),
            }
        }
    }
    /// `SignBlockEntity.playerIsTooFarAwayToEdit`: a player who left is no
    /// player at all (`Level.getPlayerByUUID` finds nobody).
    fn sign_editor_in_range(&self, who: u128, pos: (i32, i32, i32)) -> bool {
        let at = [pos.0, pos.1, pos.2];
        let me = self
            .level
            .player_uuid
            .map(horizon_client::lan::playerdata::ints_uuid);
        if me == Some(who) && self.player.is_some() {
            return self.eye_position().is_some_and(|eye| {
                horizon_client::lan::guest::within_block_range(eye, self.reach() + 4.0, at)
            });
        }
        self.lan.as_ref().is_some_and(|host| {
            host.hub
                .guests
                .iter()
                .any(|g| g.uuid == who && g.within_block_range(at, 4.0))
        })
    }
    /// `SignBlock.openTextEdit` for the front side after placement.
    pub(super) fn open_sign_editor(&mut self, pos: (i32, i32, i32), front: bool) {
        // Minecraft.setScreen: the editor already open is removed first.
        self.close_sign_editor();
        let lines = match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Sign(sign)) if front => sign.front.lines.clone(),
            Some(BlockEntity::Sign(sign)) => sign.back.lines.clone(),
            _ => Default::default(),
        };
        self.sign_edit = Some(SignEditor {
            pos,
            front,
            line: 0,
            edit: sign_box(),
            lines,
        });
        self.sign_load();
        self.keys.clear();
        self.right = false;
        self.capture(false);
        if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) {
            self.native_player_open_sign(player, &name, [pos.0, pos.1, pos.2], front);
        }
    }
    /// `AbstractSignEditScreen.tick`: the editor closes (and `removed` sends
    /// the text) once its sign is removed or the player's eyes are further
    /// than 4 blocks beyond the block interaction range
    /// (`SignBlockEntity.playerIsTooFarAwayToEdit`).
    pub(super) fn tick_sign_editor(&mut self) {
        let Some(edit) = &self.sign_edit else {
            return;
        };
        let (x, y, z) = edit.pos;
        let near = self.eye_position().is_some_and(|eye| {
            horizon_client::lan::guest::within_block_range(eye, self.reach() + 4.0, [x, y, z])
        });
        let sign = matches!(
            self.world.block_entities.get(&edit.pos),
            Some(BlockEntity::Sign(_))
        );
        if !sign || !near {
            self.close_sign_editor();
        }
    }
    /// `AbstractSignEditScreen.removed`: a server's client sends what it
    /// wrote (ServerboundSignUpdatePacket); the window's own world keeps it
    /// and sends it to its guests (`sendBlockUpdated`).
    pub(super) fn close_sign_editor(&mut self) {
        if let Some(edit) = self.sign_edit.take() {
            if self.remote.is_some() {
                self.send(S::SignUpdate(play::SignUpdate {
                    pos: [edit.pos.0, edit.pos.1, edit.pos.2],
                    lines: edit.lines,
                    front: edit.front,
                }));
            } else {
                self.mark_block_entity_updated(edit.pos);
            }
            // `SignBlockEntity.updateSignText`'s `setAllowedPlayerEditor(null)`:
            // the editor that closes gives the sign up.
            self.world.block_entities.set_sign_editor(edit.pos, None);
        }
        self.capture(true);
    }
    /// `ServerPlayer.openTextEdit` for the guest whose action opened a sign:
    /// the block, then the editor, on its client. The sign already remembers
    /// the guest as its editor (`SignBlock.openTextEdit`).
    pub(super) fn lan_open_sign_editor(&mut self, host: &mut LanHost, conn: u32, edit: SignEditor) {
        let pos = [edit.pos.0, edit.pos.1, edit.pos.2];
        let state = self.world.block(pos[0], pos[1], pos[2]).0 as i32;
        host.hub
            .send_to(conn, &C::BlockUpdate(play::BlockUpdate { pos, state }));
        host.hub.send_to(
            conn,
            &C::OpenSignEditor(play::OpenSignEditor {
                pos,
                front: edit.front,
            }),
        );
    }
    /// `ServerGamePacketListenerImpl.handleSignUpdate`: the lines, formatting
    /// codes stripped, go on the sign whose editor this player is and that is
    /// not waxed (`SignBlockEntity.updateSignText`); the sign then releases
    /// them.
    pub(super) fn lan_sign_update(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        mut p: play::SignUpdate,
    ) {
        let Some(g) = host.hub.guest(conn) else {
            return;
        };
        let (uuid, name) = (g.uuid, g.name.clone());
        let key = (p.pos[0], p.pos[1], p.pos[2]);
        let lines = p.lines.join(" ");
        if !self.chat_filter_text(
            host,
            conn,
            &[&lines],
            horizon_client::lan::hub::chat_filter::Place::Sign,
        ) {
            // The sign stays as it was: the editor closes, as after a sign the server refused.
            self.world.block_entities.set_sign_editor(key, None);
            // The player's client shows the text it sent: the sign as it is, again.
            self.mark_block_entity_updated(key);
            return;
        }
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get(&key) else {
            return;
        };
        if sign.waxed || sign.allowed_editor != Some(uuid) {
            log::warn!("{name} just tried to change non-editable sign");
            return;
        }
        // Everything vanilla checks has passed: the mods hear what is about to
        // go on the sign, with the player's state swapped in, and may rewrite
        // or refuse it.
        if let Some((uuid, name, dimension)) = host
            .hub
            .guest(conn)
            .map(|g| (g.uuid, g.name.clone(), g.dimension.clone()))
        {
            let mut lines = p.lines.to_vec();
            let mut cancel = false;
            self.as_guest_messaged(host, conn, |game| {
                game.native_sign_changed(
                    uuid,
                    &name,
                    &dimension,
                    p.pos,
                    p.front,
                    &mut lines,
                    &mut cancel,
                );
            });
            if cancel {
                // The sign keeps its text: the editor closes, and the player's
                // client, which shows what it sent, gets the sign as it is.
                self.world.block_entities.set_sign_editor(key, None);
                self.mark_block_entity_updated(key);
                return;
            }
            lines.resize(4, String::new());
            for (line, new) in p.lines.iter_mut().zip(lines) {
                *line = new;
            }
        }
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get_mut(&key) else {
            return;
        };
        // A hook may have broken the sign.
        if sign.waxed {
            self.world.block_entities.set_sign_editor(key, None);
            return;
        }
        let side = if p.front {
            &mut sign.front
        } else {
            &mut sign.back
        };
        for (line, new) in side.lines.iter_mut().zip(&p.lines) {
            *line = horizon_client::clipboard::strip_formatting(new);
        }
        // `SignBlockEntity.updateSignText` releases the sign, then sends it.
        self.world.block_entities.set_sign_editor(key, None);
        self.mark_block_entity_updated(key);
    }
    /// `ClientPacketListener.handleOpenSignEditor`: the editor on a sign this
    /// client has.
    pub(super) fn remote_open_sign_editor(&mut self, p: play::OpenSignEditor) {
        let key = (p.pos[0], p.pos[1], p.pos[2]);
        if matches!(
            self.world.block_entities.get(&key),
            Some(BlockEntity::Sign(_))
        ) {
            self.open_sign_editor(key, p.front);
        } else {
            log::warn!(
                "ignoring openTextEdit on a block that is no sign at {:?}",
                p.pos
            );
        }
    }
    pub(super) fn sign_edit_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        if let Some(shortcut) = self.edit_shortcut(event) {
            self.sign_shortcut(shortcut);
            return;
        }
        let key = match event.physical_key {
            PhysicalKey::Code(k) => Some(k),
            _ => None,
        };
        self.sign_input(key, event.text.as_deref());
    }
    /// A key and the text it typed on the sign being edited (the keyboard,
    /// or a controller's on-screen keyboard).
    pub(super) fn sign_input(&mut self, key: Option<KeyCode>, typed: Option<&str>) {
        if self.sign_edit.is_none() {
            return;
        }
        match key {
            Some(KeyCode::Escape) => self.close_sign_editor(),
            Some(KeyCode::ArrowUp) => self.sign_move_line(3),
            Some(KeyCode::ArrowDown | KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.sign_move_line(1)
            }
            Some(
                key @ (KeyCode::ArrowLeft
                | KeyCode::ArrowRight
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Backspace
                | KeyCode::Delete),
            ) => {
                let (ctrl, shift) = (self.has_control_down(), self.has_shift_down());
                self.sign_change(|edit| {
                    edit.key(key, ctrl, shift);
                });
            }
            _ => {
                let Some(typed) = typed else {
                    return;
                };
                self.sign_change(|edit| edit.insert_text(typed));
            }
        }
    }
    /// Up or down a line: the box writes the line it leaves and loads the next.
    fn sign_move_line(&mut self, by: usize) {
        self.sign_write();
        if let Some(edit) = self.sign_edit.as_mut() {
            edit.line = (edit.line + by) & 3;
        }
        self.sign_load();
    }
    /// The edited line's text.
    fn sign_current(&self) -> Option<String> {
        let edit = self.sign_edit.as_ref()?;
        let BlockEntity::Sign(sign) = self.world.block_entities.get(&edit.pos)? else {
            return None;
        };
        let side = if edit.front { &sign.front } else { &sign.back };
        Some(side.lines[edit.line].clone())
    }
    /// `EditBox.setValue` from the line now being edited.
    fn sign_load(&mut self) {
        let Some(text) = self.sign_current() else {
            return;
        };
        if let Some(edit) = self.sign_edit.as_mut() {
            edit.edit.set_value(&text);
        }
    }
    /// The box's text onto the editor's lines and the sign
    /// (`AbstractSignEditScreen.setMessage`), when it still fits the line.
    fn sign_write(&mut self) {
        let Some(edit) = self.sign_edit.as_ref() else {
            return;
        };
        let (pos, front, line, value) = (edit.pos, edit.front, edit.line, edit.edit.value.clone());
        if !self.sign_fits(&value) {
            return;
        }
        if let Some(edit) = self.sign_edit.as_mut() {
            edit.lines[line].clone_from(&value);
        }
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get_mut(&pos) else {
            return;
        };
        let side = if front {
            &mut sign.front
        } else {
            &mut sign.back
        };
        side.lines[line] = value;
    }
    /// TextFieldHelper: `font.width(s) <= maxTextLineWidth`.
    fn sign_fits(&self, value: &str) -> bool {
        let Some(edit) = self.sign_edit.as_ref() else {
            return false;
        };
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get(&edit.pos) else {
            return false;
        };
        HudBatch::text_width(&self.gui.atlas, value) <= sign.max_line_width()
    }
    /// Edit the line, then keep the result only when it still fits.
    fn sign_change(&mut self, f: impl FnOnce(&mut EditBox)) {
        let before = self
            .sign_edit
            .as_ref()
            .map(|e| (e.edit.value.clone(), e.edit.cursor, e.edit.highlight));
        if let Some(edit) = self.sign_edit.as_mut() {
            f(&mut edit.edit);
            let clean = horizon_client::clipboard::strip_formatting(&edit.edit.value);
            if clean != edit.edit.value {
                let n = horizon_client::ui::char_len(&clean);
                edit.edit.value = clean;
                edit.edit.cursor = edit.edit.cursor.min(n);
                edit.edit.highlight = edit.edit.highlight.min(n);
            }
        }
        self.sign_keep(before);
    }
    /// Write the box when the line still fits; otherwise put the old text back.
    fn sign_keep(&mut self, before: Option<(String, usize, usize)>) {
        let value = self
            .sign_edit
            .as_ref()
            .map(|e| e.edit.value.clone())
            .unwrap_or_default();
        if self.sign_fits(&value) {
            self.sign_write();
        } else if let (Some(edit), Some((value, cursor, highlight))) =
            (self.sign_edit.as_mut(), before)
        {
            edit.edit.value = value;
            edit.edit.cursor = cursor;
            edit.edit.highlight = highlight;
        }
    }
    /// `TextFieldHelper.keyPressed`'s shortcuts on the edited line. Paste
    /// reads `getClipboardContents` before `insertText`. A paste that would
    /// pass the sign's width is refused whole.
    fn sign_shortcut(&mut self, shortcut: horizon_client::clipboard::EditShortcut) {
        if shortcut == horizon_client::clipboard::EditShortcut::Paste {
            let raw = horizon_client::clipboard::Clipboard::get(&mut self.clipboard);
            let text = horizon_client::clipboard::helper_clipboard_contents(&raw);
            self.sign_change(|edit| edit.insert_text(&text));
            return;
        }
        let before = self
            .sign_edit
            .as_ref()
            .map(|e| (e.edit.value.clone(), e.edit.cursor, e.edit.highlight));
        if let Some(edit) = self.sign_edit.as_mut() {
            edit.edit.shortcut(shortcut, &mut self.clipboard);
            let clean = horizon_client::clipboard::strip_formatting(&edit.edit.value);
            if clean != edit.edit.value {
                let n = horizon_client::ui::char_len(&clean);
                edit.edit.value = clean;
                edit.edit.cursor = edit.edit.cursor.min(n);
                edit.edit.highlight = edit.edit.highlight.min(n);
            }
        }
        self.sign_keep(before);
    }
    /// Done button (`this.width / 2 - 100, this.height / 4 + 144`, 200x20),
    /// or a click on a line (`EditBox.onClick`).
    pub(super) fn sign_edit_click(&mut self, size: [u32; 2]) {
        let b = HudBatch::new(size);
        let (x, y) = done_button(b.width, b.height);
        let (mx, my) = self.cursor;
        if mx >= x && mx < x + 200.0 && my >= y && my < y + 20.0 {
            self.audio.play_button_click();
            self.close_sign_editor();
            return;
        }
        let Some((cx, top, line_height, max_width, lines)) = self.sign_lines(b.width) else {
            return;
        };
        let shift = self.has_shift_down();
        for i in 0..lines.len() {
            let ly = top + i as f32 * line_height;
            let left = cx - max_width / 2.0;
            if mx < left || mx >= left + max_width || my < ly || my >= ly + line_height {
                continue;
            }
            let current = self.sign_edit.as_ref().map(|e| e.line);
            if current != Some(i) {
                self.sign_move_line((i + 4 - current.unwrap_or(0)) & 3);
            }
            let value = self
                .sign_edit
                .as_ref()
                .map(|e| e.edit.value.clone())
                .unwrap_or_default();
            let lw = HudBatch::text_width(&self.gui.atlas, &value);
            let text_x = cx - (lw as i32 / 2) as f32;
            if let Some(edit) = self.sign_edit.as_mut() {
                edit.edit.x = text_x;
                edit.edit.y = ly;
                edit.edit.width = max_width;
                edit.edit.click(&self.gui.atlas, mx, shift, false);
            }
            self.sign_write();
            return;
        }
    }
    /// Centre x, the first line's y, the line height, the width limit and the four lines.
    fn sign_lines(&self, width: f32) -> Option<(f32, f32, f32, f32, Vec<String>)> {
        let edit = self.sign_edit.as_ref()?;
        let BlockEntity::Sign(sign) = self.world.block_entities.get(&edit.pos)? else {
            return None;
        };
        let text = if edit.front { &sign.front } else { &sign.back };
        let line_height = sign.line_height();
        let y = if sign.hanging { 125.0 } else { 90.0 };
        let top = y - (4.0 * line_height / 2.0).floor();
        Some((
            width / 2.0,
            top,
            line_height,
            sign.max_line_width(),
            text.lines.to_vec(),
        ))
    }
    pub(super) fn draw_sign_editor(&self, batch: &mut HudBatch) {
        let Some(edit) = &self.sign_edit else { return };
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get(&edit.pos) else {
            return;
        };
        self.render_transparent_background("sign_editor", batch);
        let atlas = &self.gui.atlas;
        let (w, h) = (batch.width, batch.height);
        let (key, fallback) = if sign.hanging {
            ("hanging_sign.edit", "Edit Hanging Sign Message")
        } else {
            ("sign.edit", "Edit Sign Message")
        };
        let title = self.translate(key, fallback);
        // SignEditScreen / HangingSignEditScreen: their title.
        horizon_client::widgets::begin_screen(|| {
            horizon_client::widgets::ScreenInfo::new("sign_edit", title.clone())
        });
        let tw = HudBatch::text_width(atlas, &title);
        batch.text(atlas, &title, (w - tw) / 2.0, 40.0, u32::MAX, true);
        let facts = self.res.facts();
        let block = facts.get(self.world.block(edit.pos.0, edit.pos.1, edit.pos.2));
        let wood = facts.wood_type(&block.name).unwrap_or("oak");
        let cx = w / 2.0;
        let y = if sign.hanging {
            // HangingSignEditScreen: 16x16 texture scaled 4.5 at (0, -13).
            let s = 4.5;
            batch.sprite_scaled(
                atlas,
                &editor_sprite("hanging_signs", wood),
                cx - 8.0 * s,
                125.0 - 13.0 - 8.0 * s,
                16.0 * s,
                16.0 * s,
                u32::MAX,
            );
            125.0
        } else {
            // SignEditScreen: 24x26 texture scaled 3.9 at (0, 27); wall signs show 12 rows.
            let s = 3.9;
            let wall = block.sign_form().is_some_and(|f| f.wall);
            let rows = if wall { 12.0 } else { 26.0 };
            if let Some(mut r) = atlas.sprites.get(&editor_sprite("signs", wood)).copied() {
                r.v1 = r.v0 + (r.v1 - r.v0) * rows / 26.0;
                batch.rect_uv(
                    cx - 12.0 * s,
                    90.0 + 27.0 - 13.0 * s,
                    24.0 * s,
                    rows * s,
                    r,
                    u32::MAX,
                );
            }
            90.0
        };
        let text = if edit.front { &sign.front } else { &sign.back };
        let color = if text.glowing {
            text.text_color()
        } else {
            text.dark_color()
        };
        let color = rgba((color >> 16) as u8, (color >> 8) as u8, color as u8, 255);
        let line_height = sign.line_height();
        let midpoint = (4.0 * line_height / 2.0).floor();
        for (i, line) in text.lines.iter().enumerate() {
            let shown = if i == edit.line {
                edit.edit.value.as_str()
            } else {
                line.as_str()
            };
            let lw = HudBatch::text_width(atlas, shown);
            let x = cx - (lw as i32 / 2) as f32;
            let ly = y + i as f32 * line_height - midpoint;
            batch.text(atlas, shown, x, ly, color, false);
            if i != edit.line {
                continue;
            }
            let value = &edit.edit.value;
            if edit.edit.cursor != edit.edit.highlight {
                let (a, b) = (
                    edit.edit.cursor.min(edit.edit.highlight),
                    edit.edit.cursor.max(edit.edit.highlight),
                );
                let pre: String = value.chars().take(a).collect();
                let sel: String = value.chars().skip(a).take(b - a).collect();
                batch.text_highlight(
                    atlas,
                    (
                        x + HudBatch::text_width(atlas, &pre),
                        ly,
                        HudBatch::text_width(atlas, &sel),
                        line_height,
                    ),
                    true,
                );
            }
            let pre: String = value.chars().take(edit.edit.cursor).collect();
            let caret_x = x + HudBatch::text_width(atlas, &pre);
            if edit.edit.cursor_visible() {
                batch.fill(atlas, caret_x, ly, 1.0, line_height, color);
            }
            horizon_client::preedit::report_caret(caret_x, ly, line_height);
        }
        let (bx, by) = done_button(w, h);
        horizon_client::widgets::declare_action(
            horizon_client::widgets::Action::Done,
            [bx, by, 200.0, 20.0],
            true,
        );
        horizon_client::widgets::narratable(|| {
            horizon_client::widgets::Narratable::button(done_label(), [bx, by, 200.0, 20.0], true)
        });
        batch.sprite_scaled(
            atlas,
            "gui/sprites/widget/button",
            bx,
            by,
            200.0,
            20.0,
            u32::MAX,
        );
        let done = done_label();
        let tw = HudBatch::text_width(atlas, &done);
        batch.text(
            atlas,
            &done,
            bx + 100.0 - (tw / 2.0).floor(),
            by + 6.0,
            u32::MAX,
            true,
        );
    }
}

#[cfg(test)]
#[path = "sign_edit_tests.rs"]
mod sign_edit_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn done_button_sits_on_whole_gui_pixels() {
        // `width / 2 - 100, height / 4 + 144` in ints: 334 high is 83 + 144.
        assert_eq!(super::done_button(427.0, 334.0), (113.0, 227.0));
        assert_eq!(super::done_button(320.0, 240.0), (60.0, 204.0));
    }

    /// The Done button said "Done" in every language (#1927).
    #[test]
    fn done_button_reads_the_selected_language() {
        use horizon_client::lang::{Language, ResourceSource, scoped};
        struct Vanilla;
        impl ResourceSource for Vanilla {
            fn read(&self, path: &str) -> Option<Vec<u8>> {
                match path {
                    "assets/minecraft/lang/en_us.json" => Some(br#"{"gui.done": "Done"}"#.to_vec()),
                    "assets/minecraft/lang/de_de.json" => {
                        Some(br#"{"gui.done": "Fertig"}"#.to_vec())
                    }
                    _ => None,
                }
            }
        }
        for (code, want) in [("de_de", "Fertig"), ("en_us", "Done")] {
            let (label, missed) = scoped(Language::load(&[&Vanilla], code), super::done_label);
            assert_eq!(label, want, "{code}");
            assert!(missed.is_empty(), "{code}: {missed:?}");
        }
    }
}
