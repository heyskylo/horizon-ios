//! Placing the local player in a world it opened: at its saved position once the chunks around it
//! are lit, or at the world spawn, then capturing the mouse.
use super::*;

impl Game<'_> {
    pub(super) fn spawn_player(&mut self) -> Result<()> {
        if self.player.is_some() || self.remote.is_some() {
            return Ok(());
        }
        let saved = self.level.position.map(|p| Vec3::new(p[0], p[1], p[2]));
        if self.spawn.is_none() {
            self.spawn = self
                .level
                .spawn
                .map(|s| Vec3::new(s[0] + 0.5, s[1], s[2] + 0.5));
        }
        let feet = if let Some(p) = saved {
            let (cx, cz) = ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4);
            self.streaming.center = (cx, cz);
            if !(-1..=1).all(|dz| (-1..=1).all(|dx| self.streaming.is_lit(cx + dx, cz + dz))) {
                return Ok(());
            }
            self.level.position = None;
            p
        } else if self.spawn.is_none() || self.player_spawn.is_some() {
            // A new world: its spawn, then the first player around it.
            if self.spawn.is_none() {
                let Some([x, y, z]) = self.find_world_spawn()? else {
                    return Ok(());
                };
                self.spawn = Some(Vec3::new(
                    f64::from(x) + 0.5,
                    f64::from(y),
                    f64::from(z) + 0.5,
                ));
                self.level.spawn = Some([f64::from(x), f64::from(y), f64::from(z)]);
                // The world's spawn from the start, not only once saved: a bed set
                // before the first save must not become it.
                self.level.world_spawn.get_or_insert([x, y, z]);
                // The "Bonus Chest" create-world option, placed once with the spawn.
                if horizon_client::save::WorldGen::read(&self.world_root)
                    .is_some_and(|g| g.bonus_chest)
                {
                    self.place_bonus_chest([x, y, z])?;
                }
            }
            let s = self
                .level
                .spawn
                .unwrap_or_default()
                .map(|v| v.floor() as i32);
            let Some(feet) = self.find_player_spawn(s)? else {
                return Ok(());
            };
            feet
        } else {
            let s = self.spawn.unwrap_or_default();
            let (cx, cz) = ((s.x.floor() as i32) >> 4, (s.z.floor() as i32) >> 4);
            self.streaming.center = (cx, cz);
            if !(-1..=1).all(|dz| (-1..=1).all(|dx| self.streaming.is_lit(cx + dx, cz + dz))) {
                return Ok(());
            }
            let column = [s.x.floor() as i32, s.y.floor() as i32, s.z.floor() as i32];
            if self.level.world_spawn == Some(column) {
                // TeleportTransition.missingRespawnBlock: PlayerSpawnFinder
                // around the world spawn, not its exact column; its candidates'
                // chunks load first.
                let Some(feet) = self.find_player_spawn(column)? else {
                    return Ok(());
                };
                feet
            } else {
                // A spawnpoint: the stored column, lifted onto its current surface.
                let y = self
                    .land_column(column[0], column[2])
                    .map_or(s.y, f64::from);
                Vec3::new(s.x, y.max(s.y), s.z)
            }
        };
        let mut player = PlayerMotion::new(self.motion, feet)?;
        player.yaw = self.yaw;
        let entering = self.restore_player_state;
        player.load_saved_state_once(&mut self.restore_player_state, &self.level)?;
        let spectator = self.mode == horizon_client::command::args::GameMode::Spectator;
        player.may_fly = self.creative || spectator;
        player.invulnerable = self.creative || spectator;
        player.spectator = spectator;
        if spectator {
            player.flying = true;
        } else if !player.may_fly {
            player.flying = false;
        }
        player.respiration = self.armor[0]
            .as_ref()
            .map_or(0, |s| s.enchantment_level("minecraft:respiration") as i32);
        player.pitch = self.pitch;
        player.glider = self.glider_worn();
        // A saved death already dropped inventory/XP and counted the death.
        if player.health.dead_or_dying() {
            self.screen = Screen::Dead;
            self.death_focus = None;
            self.ui.died_at = Some(Instant::now());
            self.ui.death_text = None;
            self.ui.death_confirm = None;
            self.capture(false);
        }
        self.previous_position = feet;
        if entering {
            // The world's first player: a server's client gets it with its
            // level (Minecraft.setLevel), which stops every sound.
            self.stop_sounds_for_new_level();
        }
        self.player = Some(player);
        self.input_player_replaced();
        self.sway = Default::default();
        self.apply_equipment();
        self.eye = 1.62;
        self.previous_eye = 1.62;
        self.clock.reset();
        self.capture_loaded_world();
        Ok(())
    }
}
