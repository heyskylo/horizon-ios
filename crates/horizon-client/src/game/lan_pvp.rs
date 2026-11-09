//! Players hitting players on a server (`Player.attack` on a player). The hit
//! is worked out as the attacker (weapon, attack strength, critical hit,
//! enchantments, a mace's smash) and lands on the victim like any player
//! attack (blocking, armour, knockback), when the `pvp` game rule allows it
//! (`ServerPlayer.canHarmPlayer`, `isPvpAllowed`). A player's arrow on
//! another player lands the same way.
use super::{Game, lan::LanHost, lan_mobs::guest_bounds, mace::Spared};
use horizon_client::enchant::EffectContext;

use horizon_client::{
    Result,
    health::HurtSource,
    mobs::Who,
    projectiles::{Hit, Projectile, ProjectileKind},
    text::Text,
};
use horizon_core::geometry::{Aabb, Vec3};

/// The player projectiles that meet other players: arrows (their hurt), and
/// snowballs, eggs and mods' thrown types, which break on them
/// (`Player.hurtServer` takes no 0 damage). Pearls, potions and wind charges
/// fly past players as before.
pub(super) fn meets_players(kind: &ProjectileKind) -> bool {
    matches!(
        kind,
        ProjectileKind::Arrow { .. }
            | ProjectileKind::Snowball
            | ProjectileKind::Egg
            | ProjectileKind::Thrown(_)
    )
}

/// The player-versus-player hit being landed, which `native_player_hurt_by_player`
/// hears at the victim's `hurtServer` (`native_hooks::hurt_server`).
#[derive(Clone, Copy)]
pub(super) struct PvpHit {
    pub(super) attacker: u128,
    pub(super) attacker_entity: i32,
    pub(super) victim: u128,
    pub(super) victim_entity: i32,
    /// `melee`, `sweep` or the projectile's entity type.
    pub(super) source: &'static str,
}

thread_local! {
    static PVP_HIT: std::cell::Cell<Option<PvpHit>> = const { std::cell::Cell::new(None) };
}

/// The hit being landed on the player `victim_entity`, if one is.
pub(super) fn pvp_hit(victim_entity: i32) -> Option<PvpHit> {
    PVP_HIT
        .get()
        .filter(|hit| hit.victim_entity == victim_entity)
}

/// Names `hit` as the one being landed until it drops.
pub(super) struct PvpScope(Option<PvpHit>);

impl Drop for PvpScope {
    fn drop(&mut self) {
        PVP_HIT.set(self.0);
    }
}

pub(super) fn pvp_scope(hit: Option<PvpHit>) -> PvpScope {
    PvpScope(PVP_HIT.replace(hit))
}

/// Who a player's attack lands on: a player by connection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Victim {
    Guest(u32),
}

impl Victim {
    fn conn(self) -> Option<u32> {
        let Victim::Guest(conn) = self;
        Some(conn)
    }
}

/// The attacker's side of the hit.
pub(super) struct Strike {
    /// The attacking guest, kept after their turn through `as_guest` ends.
    attacker: Option<u32>,
    amount: f32,
    scale: f32,
    crit: bool,
    /// `Player.attack`'s `magicBoost > 0`: the enchantments added damage.
    magic: bool,
    from: [f64; 3],
    /// `getSecondsToDisableBlocking` of the attacker's weapon.
    disable_blocking: f32,
    sweep: Option<[f64; 3]>,
    /// `causeExtraKnockback`: its strength and direction (the attacker's
    /// yaw), and whether the hit is a sprinting knockback attack.
    knockback: (f32, f64, f64),
    knockback_attack: bool,
    /// The attacker's team, for the victim's `Player.canHarmPlayer`.
    team: Option<horizon_client::scoreboard::Team>,
    /// The attacker's main-hand item with a custom name, for the death
    /// message's `.item` form.
    item: Option<horizon_client::text::Text>,
    /// The weapon's `minecraft:ignite` (Fire Aspect) on the victim, in
    /// seconds (`EnchantmentHelper.doPostAttackEffects`).
    ignite: f32,
}

/// A player's blow on another player as the native hooks name it: the hit's
/// own push and its damage name no attacker yet, so the extra and the sweep's
/// pushes don't either.
const PVP_SOURCE: HurtSource<'static> = HurtSource {
    damage_type: "minecraft:player_attack",
    causing: 0,
    direct: 0,
};

/// A sweep attack's reach to the other players (`Player.doSweepAttack`),
/// worked out as the attacker.
#[derive(Clone)]
pub(super) struct Sweep {
    /// The attacking guest (none: the host).
    attacker: Option<u32>,
    from: [f64; 3],
    /// The damage after the enchantments and the attack strength.
    damage: f32,
    /// The push along the attacker's yaw.
    knockback: (f64, f64),
    team: Option<horizon_client::scoreboard::Team>,
    /// The target's box grown by (1, 0.25, 1).
    pub(super) hit_box: Aabb,
    /// The entity id of the player the sweep started from, which it spares.
    pub(super) skip: Option<i32>,
    /// The weapon's `minecraft:ignite` (Fire Aspect) on each swept player, in
    /// seconds (`doPostAttackEffects` after a sweep hit lands).
    ignite: f32,
}

impl Strike {
    /// Whether it has extra knockback, which stops the attacker's sprint
    /// once it lands (Player.causeExtraKnockback).
    pub(super) fn stops_sprint(&self) -> bool {
        self.knockback.0 > 0.0
    }
}

impl Game<'_> {
    /// Whether the player a hit landed on stands on the ground.
    pub(super) fn pvp_victim_on_ground(host: &LanHost, victim: Victim) -> bool {
        match victim {
            Victim::Guest(conn) => host
                .hub
                .guest(conn)
                .and_then(|g| g.motion.as_deref())
                .is_some_and(|p| p.ground.on_ground),
        }
    }

    /// The player `id` that guest `conn` attacks, if it is one.
    pub(super) fn pvp_victim(host: &LanHost, conn: u32, id: i32) -> Option<Victim> {
        host.hub
            .guests
            .iter()
            .find(|g| g.entity_id == id && g.conn != conn && host.hub.here(g) && !g.dead)
            .map(|g| Victim::Guest(g.conn))
    }

    /// `Player.attack`'s `causeFoodExhaustion(0.1F)`, charged to the attacker
    /// (the host, or a guest swapped in) only once the hit hurt the victim.
    pub(super) fn pvp_exhaustion(&mut self) {
        if let Some(p) = self.player.as_mut()
            && !self.creative
        {
            p.cause_food_exhaustion(0.1);
        }
    }

    /// The attacker's side of `Player.attack` (the host, or a guest swapped
    /// in): the damage, the charge reset and the swing.
    pub(super) fn pvp_strike(&mut self) -> Option<Strike> {
        let p = self.player.as_ref()?;
        let from = [p.position.x, p.position.y, p.position.z];
        let (amount, scale, crit, magic) = self.melee_damage("minecraft:player", true);
        let sweep = self.sweep_attack_position(scale, crit);
        let (strength, knockback_attack) = self.extra_knockback("minecraft:player", scale);
        let (xd, zd) = self.extra_knockback_direction();
        self.attack_ticks = 0;
        self.swing_arm();
        // LivingEntity.getSecondsToDisableBlocking: the held weapon's value,
        // none while another item is in use.
        let disable_blocking = if self.using.is_some() {
            0.0
        } else {
            self.held()
                .map_or(0.0, |stack| self.items().weapon_disable_blocking(&stack))
        };
        let ignite = self.weapon_ignite_seconds();
        Some(Strike {
            attacker: self.lan_acting,
            amount,
            scale,
            crit,
            magic,
            from,
            disable_blocking,
            sweep,
            knockback: (strength, xd, zd),
            knockback_attack,
            team: self.player_team().cloned(),
            item: self.main_hand_custom_name(),
            ignite,
        })
    }

    /// The held weapon's `minecraft:ignite` (Fire Aspect) seconds on a player
    /// victim (`EnchantmentHelper.doPostAttackEffects`).
    pub(super) fn weapon_ignite_seconds(&mut self) -> f32 {
        let Some(held) = self.held().cloned() else {
            return 0.0;
        };
        let ctx = EffectContext {
            this: Some("minecraft:player"),
            attacker: Some("minecraft:player"),
            direct_attacker: Some("minecraft:player"),
            is_direct: true,
            items: Some(world_items!(self)),
            ..Default::default()
        };
        let mut random = self.world.random.clone();
        world_stations!(self)
            .enchantments
            .ignite_seconds(held.enchantments(), &ctx, &mut random)
    }

    /// The guests by connection, with their entity ids.
    pub(super) fn lan_guest_entities(&self) -> Vec<(u32, u32)> {
        self.lan.as_ref().map_or_else(Vec::new, |host| {
            host.hub
                .guests
                .iter()
                .map(|g| (g.conn, g.entity_id as u32))
                .collect()
        })
    }

    /// The player a player's projectile hit, when it hit one.
    pub(super) fn projectile_pvp_victim(&self, p: &Projectile, hit: Hit) -> Option<Victim> {
        if p.owner != Some(Who::Player) || !meets_players(&p.kind) {
            return None;
        }
        let Hit::Entity { who, .. } = hit else {
            return None;
        };
        match who {
            Who::Player => None,
            Who::Mob(id) => self
                .lan
                .as_ref()?
                .hub
                .guests
                .iter()
                .find(|g| g.entity_id as u32 == id)
                .map(|g| Victim::Guest(g.conn)),
        }
    }

    /// A player's projectile on a player: the hit runs as the victim, whose
    /// hurt refuses it when the `pvp` game rule or their teams forbid it
    /// (`ServerPlayer.canHarmPlayer`; `AbstractArrow.onHitEntity` then turns
    /// the arrow back).
    pub(super) fn projectile_hits_player(
        &mut self,
        p: &Projectile,
        hit: Hit,
        victim: Victim,
    ) -> Result<()> {
        let Hit::Entity { at, .. } = hit else {
            return Ok(());
        };
        let Some(mut host) = self.lan.take() else {
            return self.projectile_hit(p, hit);
        };
        let team_of = |game: &Self, name: &str| game.score.board.players_team(name).cloned();
        let attacker = match p.guest {
            Some(conn) => host.hub.guest(conn).and_then(|g| team_of(self, &g.name)),
            None => self.player_team().cloned(),
        };
        let target = match victim {
            Victim::Guest(conn) => host.hub.guest(conn).and_then(|g| team_of(self, &g.name)),
        };
        let refused = !(self.ui.rules.bool("pvp")
            && horizon_client::scoreboard::teams_allow_damage(target.as_ref(), attacker.as_ref()));
        let on_player = Hit::Entity {
            who: Who::Player,
            at,
        };
        let shooter = self.player_attacker(&host, p.guest);
        let spectral = p
            .arrow
            .as_ref()
            .is_some_and(|a| a.spectral || &*a.pickup_item.item == "minecraft:spectral_arrow");
        let source = if spectral {
            "minecraft:spectral_arrow"
        } else {
            p.kind.id()
        };
        let hit = self.pvp_hit_of(&host, p.guest, victim, source);
        let zero = matches!(p.kind, ProjectileKind::Snowball | ProjectileKind::Egg);
        let result = match victim {
            Victim::Guest(conn) => self.guest_hit_by(&mut host, conn, |g| {
                g.combat.refuse_player_hit = refused;
                g.combat.projectile_attacker = shooter;
                let _hit = pvp_scope(hit);
                if zero && !refused {
                    g.pvp_zero_hurt(hit, "minecraft:thrown");
                }
                let r = g.projectile_hit(p, on_player);
                g.combat.refuse_player_hit = false;
                g.combat.projectile_attacker = None;
                r
            }),
        };
        self.lan = Some(host);
        result.unwrap_or(Ok(()))
    }

    /// The hit of `attacker` (a guest; none: this game's player) on `victim` as
    /// `native_player_hurt_by_player` names it, none when either has gone.
    pub(super) fn pvp_hit_of(
        &mut self,
        host: &LanHost,
        attacker: Option<u32>,
        victim: Victim,
        source: &'static str,
    ) -> Option<PvpHit> {
        let Victim::Guest(conn) = victim;
        let (victim, victim_entity) = host.hub.guest(conn).map(|g| (g.uuid, g.entity_id))?;
        let (attacker, attacker_entity) = match attacker {
            Some(conn) => host.hub.guest(conn).map(|g| (g.uuid, g.entity_id))?,
            None => (self.native_acting_id(), self.player.as_ref()?.entity_id),
        };
        Some(PvpHit {
            attacker,
            attacker_entity,
            victim,
            victim_entity,
            source,
        })
    }

    /// `Player.hurtServer` of a snowball's or egg's zero damage on the player in place: past
    /// its guards (not creative, a spectator or dead) the mods hear it, then it stops on zero.
    fn pvp_zero_hurt(&mut self, hit: Option<PvpHit>, damage_type: &str) {
        let Some(hit) = hit else {
            return;
        };
        if self.creative
            || self.spectator()
            || self
                .player
                .as_ref()
                .is_none_or(|p| p.health.dead_or_dying())
        {
            return;
        }
        let (mut amount, mut canceled) = (0.0, false);
        Self::native_player_hurt_by_player(
            hit.attacker,
            hit.attacker_entity,
            hit.victim,
            hit.victim_entity,
            hit.source,
            damage_type,
            &mut amount,
            &mut canceled,
        );
    }

    pub(super) fn player_attacker(&self, host: &LanHost, conn: Option<u32>) -> Option<(u32, Text)> {
        match conn {
            Some(conn) => {
                let g = host.hub.guest(conn)?;
                Some((g.entity_id as u32, self.player_text(&g.name, g.uuid)))
            }
            None => None,
        }
    }

    /// The hit landing on the victim with its extra knockback, then the
    /// attacker's attack sound. Whether the victim was hurt.
    pub(super) fn pvp_land(&mut self, host: &mut LanHost, victim: Victim, s: &Strike) -> bool {
        // ServerPlayer.canHarmPlayer: the pvp game rule, then the victim's
        // team (Player.canHarmPlayer: allies only with friendly fire on).
        let team = match victim {
            Victim::Guest(conn) => host
                .hub
                .guest(conn)
                .and_then(|g| self.score.board.players_team(&g.name)),
        };
        let harm = horizon_client::scoreboard::teams_allow_damage(team, s.team.as_ref());
        if s.knockback_attack {
            self.audio.play(
                "minecraft:entity.player.attack.knockback",
                Some(s.from),
                1.0,
                1.0,
            );
        }
        // Player.attack: the hurt (with its knockback away from the
        // attacker), then causeExtraKnockback along the attacker's yaw.
        // CombatTracker: the attacking player is the death message's killer.
        // Without it the message is built with one argument and a template
        // that names a killer ("%1$s was slain by %2$s") shows as written.
        let attacker_note = s.attacker.and_then(|conn| {
            host.hub
                .guest(conn)
                .map(|a| (a.entity_id, self.player_text(&a.name, a.uuid)))
        });
        let attacker_id = self.player.as_ref().map_or(0, |p| p.entity_id);
        let hit = self.pvp_hit_of(host, s.attacker, victim, "melee");
        let land = |g: &mut Self| {
            let _attacker =
                g.knockback_scope(attacker_note.as_ref().map_or(attacker_id, |(id, _)| *id));
            g.combat.player_attacker = Some((s.from, s.disable_blocking));
            if let Some((id, name)) = attacker_note.clone() {
                g.note_player_attacker("minecraft:player_attack", id as u32, name, s.item.clone());
            }
            let hurt = {
                let _hit = pvp_scope(hit);
                g.mob_hits_player(s.amount, s.from, "minecraft:player_attack")
            };
            g.combat.player_attacker = None;
            let (strength, xd, zd) = s.knockback;
            if hurt && strength > 0.0 {
                if g.legacy_rules() {
                    g.legacy_extra_knockback(f64::from(strength), (xd, zd));
                } else {
                    g.knock_back_player(f64::from(strength), xd, zd, PVP_SOURCE);
                }
            }
            // Entity.igniteForSeconds on the victim: the fire timer only
            // rises, and a fire-immune victim (no player is by type) is spared.
            if hurt
                && s.ignite > 0.0
                && let Some(p) = g.player.as_mut()
            {
                let _ = p.ignite_for_ticks((s.ignite * 20.0) as i32);
            }
            // The victim's thorns, dealt to the attacker below.
            let thorns = if hurt { g.thorns_damage() } else { Vec::new() };
            (hurt, thorns)
        };
        let (hurt, thorns) = if self.ui.rules.bool("pvp") && harm {
            match victim {
                Victim::Guest(conn) => self.guest_struck_by(host, conn, land).unwrap_or_default(),
            }
        } else {
            Default::default()
        };
        if hurt && (s.crit || s.magic) {
            let target = match victim {
                Victim::Guest(conn) => host
                    .hub
                    .guest(conn)
                    .and_then(|g| g.motion.as_deref().map(|p| (g.entity_id, p))),
            };
            if let Some((id, p)) = target {
                let d = p.dimensions;
                let (feet, width, height) = (
                    [p.position.x, p.position.y, p.position.z],
                    f64::from(d.width),
                    f64::from(d.height),
                );
                if s.crit {
                    self.critical_hit(s.attacker, id, feet, width, height, false);
                }
                if s.magic {
                    self.critical_hit(s.attacker, id, feet, width, height, true);
                }
            }
        }
        self.pvp_aftermath(host, victim, s.attacker, hurt, thorns);
        let sound = match (hurt, s.crit, s.scale > 0.9) {
            (false, ..) => "minecraft:entity.player.attack.nodamage",
            (true, true, _) => "minecraft:entity.player.attack.crit",
            (true, false, true) => "minecraft:entity.player.attack.strong",
            (true, false, false) => "minecraft:entity.player.attack.weak",
        };
        if let Some(at) = s.sweep.filter(|_| hurt) {
            // The sweep around the player that was hit, as its attacker.
            let (area, id) = match victim {
                Victim::Guest(conn) => {
                    let g = host.hub.guest(conn);
                    (g.map(guest_bounds), g.map_or(-1, |g| g.entity_id))
                }
            };
            let target = area.map(|area| (area, Spared::Player(id)));
            match s.attacker {
                None => {
                    // The host is checked out here: collect the sweep for the players.
                    self.combat.sweeps = Some(Vec::new());
                    self.sweep_attack(at, Some(s.from), target, s.scale);
                    for sweep in self.combat.sweeps.take().unwrap_or_default() {
                        self.sweep_players(host, &sweep);
                    }
                }
                Some(conn) => {
                    self.as_guest(host, conn, |g| {
                        g.sweep_attack(at, Some(s.from), target, s.scale)
                    });
                }
            }
        } else {
            self.audio.play(sound, Some(s.from), 1.0, 1.0);
        }
        hurt
    }

    /// What follows a player's blow on a player as the victim's hurt ends:
    /// `ServerPlayer.awardKillScore` when it killed them, then the victim's
    /// thorns dealt back to the attacker.
    pub(super) fn pvp_aftermath(
        &mut self,
        host: &mut LanHost,
        victim: Victim,
        attacker: Option<u32>,
        hurt: bool,
        thorns: Vec<f32>,
    ) {
        // ServerPlayer.awardKillScore: the blow that kills a player.
        if hurt {
            let died = match victim {
                // A totem that fired is applied on the next tick_totem, so the
                // victim is still at zero health but is not dying.
                Victim::Guest(conn) => host
                    .hub
                    .guest(conn)
                    .and_then(|g| g.motion.as_deref())
                    .is_some_and(|p| p.health.dead_or_dying() && !p.health.totem_used),
            };
            if died {
                match attacker {
                    Some(conn) => {
                        self.as_guest(host, conn, |g| g.award_custom_stat("player_kills", 1));
                    }
                    None => self.award_custom_stat("player_kills", 1),
                }
            }
        }
        let thorns_cause = self.player_attacker(host, victim.conn());
        for damage in thorns {
            match attacker {
                None => {
                    if let Some((id, name)) = &thorns_cause {
                        self.note_named_attacker("minecraft:thorns", *id, name.clone());
                    }
                    self.hurt_player_by("minecraft:thorns", damage);
                }
                Some(conn) => {
                    self.guest_hit_by(host, conn, |g| {
                        if let Some((id, name)) = &thorns_cause {
                            g.note_named_attacker("minecraft:thorns", *id, name.clone());
                        }
                        g.hurt_player_by("minecraft:thorns", damage)
                    });
                }
            }
        }
    }

    /// The sweep of the player in place, for the players it reaches: the
    /// damage (`1 + ratio * baseDamage`, plus the enchantments against a
    /// player, scaled by the attack strength) and who hits. The box is set
    /// by the caller.
    pub(super) fn player_sweep(&mut self, base: f32, scale: f32) -> Sweep {
        let from = self
            .player
            .as_ref()
            .map_or([0.0; 3], |p| [p.position.x, p.position.y, p.position.z]);
        let damage = self.enchanted_damage("minecraft:player", base) * scale;
        Sweep {
            attacker: self.lan_acting,
            from,
            damage,
            knockback: self.extra_knockback_direction(),
            team: self.player_team().cloned(),
            hit_box: Aabb::new(Vec3::ZERO, Vec3::ZERO),
            skip: None,
            ignite: self.weapon_ignite_seconds(),
        }
    }

    /// A sweep from a mob hit reaches the other players. Inside a guest's
    /// turn the host holds the other players (`as_guest`), so it waits for
    /// the turn to end.
    pub(super) fn sweep_other_players(&mut self, sweep: Sweep) {
        if let Some(collect) = self.combat.sweeps.as_mut() {
            collect.push(sweep);
        } else if self.lan_acting.is_some() {
            self.creatures.sweeps.push(sweep);
        } else if let Some(mut host) = self.lan.take() {
            self.sweep_players(&mut host, &sweep);
            self.lan = Some(host);
        }
    }

    /// The sweeps guests' turns put off.
    pub(super) fn flush_sweeps(&mut self) {
        for sweep in std::mem::take(&mut self.creatures.sweeps) {
            self.sweep_other_players(sweep);
        }
    }

    /// `Player.doSweepAttack` on the players: each other player in the box,
    /// closer than 3 blocks (`distanceToSqr < 9`) and not an ally
    /// (`isAlliedTo`), is hurt (`ServerPlayer.canHarmPlayer`: the `pvp` rule)
    /// and pushed 0.4 along the attacker's yaw.
    pub(super) fn sweep_players(&mut self, host: &mut LanHost, sweep: &Sweep) {
        let from = Vec3::new(sweep.from[0], sweep.from[1], sweep.from[2]);
        let mut victims = Vec::new();
        for g in &host.hub.guests {
            if host.hub.here(g)
                && g.loaded
                && !g.dead
                && !g.spectator()
                && sweep.attacker != Some(g.conn)
                && guest_bounds(g).intersects(sweep.hit_box)
                && Vec3::new(g.position[0], g.position[1], g.position[2]).distance_squared(from)
                    < 9.0
            {
                let team = self.score.board.players_team(&g.name).cloned();
                victims.push((Victim::Guest(g.conn), g.entity_id, team));
            }
        }
        let (xd, zd) = sweep.knockback;
        let attacker = self.player_attacker(host, sweep.attacker);
        let attacker_id = self.player.as_ref().map_or(0, |p| p.entity_id);
        let land = |g: &mut Self| {
            let _attacker =
                g.knockback_scope(attacker.as_ref().map_or(attacker_id, |(id, _)| *id as i32));
            if let Some((id, name)) = &attacker {
                g.note_named_attacker("minecraft:player_attack", *id, name.clone());
            }
            if g.mob_hits_player(sweep.damage, sweep.from, "minecraft:player_attack") {
                g.knock_back_player(f64::from(0.4_f32), xd, zd, PVP_SOURCE);
                // EnchantmentHelper.doPostAttackEffects, after the knockback.
                if sweep.ignite > 0.0
                    && let Some(p) = g.player.as_mut()
                {
                    let _ = p.ignite_for_ticks((sweep.ignite * 20.0) as i32);
                }
                // The swept player's thorns, dealt to the attacker below.
                return g.thorns_damage();
            }
            Vec::new()
        };
        for (victim, id, team) in victims {
            if sweep.skip == Some(id)
                || horizon_client::scoreboard::teams_allied(team.as_ref(), sweep.team.as_ref())
                || !self.ui.rules.bool("pvp")
            {
                continue;
            }
            let hit = self.pvp_hit_of(host, sweep.attacker, victim, "sweep");
            let Victim::Guest(conn) = victim;
            let thorns = self
                .guest_struck_by(host, conn, |g| {
                    let _hit = pvp_scope(hit);
                    land(g)
                })
                .unwrap_or_default();
            // doPostAttackEffects' victim side: its armour's thorns hurt the
            // attacker, as on a direct hit (pvp_land).
            let thorns_cause = self.player_attacker(host, victim.conn());
            for damage in thorns {
                match sweep.attacker {
                    None => {
                        if let Some((id, name)) = &thorns_cause {
                            self.note_named_attacker("minecraft:thorns", *id, name.clone());
                        }
                        self.hurt_player_by("minecraft:thorns", damage);
                    }
                    Some(conn) => {
                        self.guest_hit_by(host, conn, |g| {
                            if let Some((id, name)) = &thorns_cause {
                                g.note_named_attacker("minecraft:thorns", *id, name.clone());
                            }
                            g.hurt_player_by("minecraft:thorns", damage)
                        });
                    }
                }
            }
        }
    }
}
