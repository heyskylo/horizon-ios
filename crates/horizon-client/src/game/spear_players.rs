//! A player's spear on the other players of a server: the charge
//! (`KineticWeapon.damageEntities`) and the jab (`PiercingWeapon.attack`)
//! stab every living entity along the view, players too. The stab is worked
//! out as the attacker in their turn (`Player.stabAttack`'s damage, magic
//! boost and pushes), lands on the victim like a player's melee blow once the
//! hub is back (`lan_pvp`), and the attacker's side of a landed stab
//! (`attackVisualEffects`, `itemAttackInteraction`, `damageStatsAndHearts`,
//! the exhaustion, and the hit's feedback) follows as the attacker again.
use super::{
    Game,
    lan::LanHost,
    lan_pvp::{Victim, pvp_scope},
};
use horizon_client::{health::HurtSource, spear::Kinetic, text::Text};
use horizon_core::geometry::{Aabb, Vec3};

/// Another player a spear can reach in the acting player's turn, taken from
/// the hub before it: `PiercingWeapon.canHitEntity` holds for them.
#[derive(Clone, Copy)]
pub(super) struct StabPlayer {
    pub(super) conn: u32,
    pub(super) entity_id: i32,
    pub(super) bounds: Aabb,
    /// `getKnownSpeed`: the movement their client reported, per tick.
    pub(super) movement: Vec3,
}

/// One stab on a player, as the attacker worked it out.
pub(super) struct PlayerStab {
    victim: u32,
    /// The damage after the enchantments (and the attack strength when the
    /// spear isn't in use); 0 when the stab deals none.
    amount: f32,
    deals_damage: bool,
    deals_knockback: bool,
    dismounts: bool,
    /// `magicBoost > 0`: the enchantments added damage.
    magic: bool,
    from: [f64; 3],
    /// `getKnockback`'s enchanted strength, and the attacker's yaw as a direction.
    knockback: (f32, f64, f64),
    disable_blocking: f32,
    item: Option<Text>,
    ignite: f32,
}

/// The stabs of one charge tick or one jab on players.
pub(super) struct StabBatch {
    /// The stabbing guest.
    attacker: u32,
    kinetic: Kinetic,
    /// A charge's tick (its hit feedback), else a jab (its hit sound).
    charge: bool,
    /// Whether a mob stabbed in the same tick gave the feedback already.
    felt: bool,
    stabs: Vec<PlayerStab>,
}

/// What a landed stab did to the victim.
struct Landed {
    hurt: bool,
    /// The stab's enchantments added damage (the magic crit).
    magic: bool,
    /// `stabAttack`'s result: hurt, pushed or dismounted.
    affected: bool,
    /// Health lost, for `damageStatsAndHearts`.
    actual: f32,
    id: i32,
    feet: [f64; 3],
    width: f64,
    height: f64,
}

/// A spear's push on a player as the native hooks name it.
const SPEAR_SOURCE: HurtSource<'static> = HurtSource {
    damage_type: "minecraft:spear",
    causing: 0,
    direct: 0,
};

impl Game<'_> {
    /// The players the guest `actor` could stab now: in their level, placed,
    /// alive and no spectators (`canBeHitByProjectile`), allowed by
    /// `canHarmPlayer` (the `pvp` game rule and the victim's team), and not
    /// on the attacker's vehicle.
    pub(super) fn stab_players_for(&self, host: &LanHost, actor: u32) -> Vec<StabPlayer> {
        let board = &self.score.board;
        let Some(attacker) = host.hub.guest(actor).filter(|_| self.ui.rules.bool("pvp")) else {
            return Vec::new();
        };
        let own_team = board.players_team(&attacker.name);
        let own_vehicle = attacker.vehicle;
        host.hub
            .guests
            .iter()
            .filter(|g| {
                g.conn != actor
                    && host.hub.here(g)
                    && g.placed
                    && g.loaded
                    && !g.dead
                    && !g.spectator()
                    && (own_vehicle.is_none() || g.vehicle != own_vehicle)
                    && horizon_client::scoreboard::teams_allow_damage(
                        board.players_team(&g.name),
                        own_team,
                    )
            })
            .map(|g| {
                let [x, y, z] = g.known_movement;
                StabPlayer {
                    conn: g.conn,
                    entity_id: g.entity_id,
                    bounds: g.bounds(),
                    movement: Vec3::new(x, y, z),
                }
            })
            .collect()
    }

    /// The attacker's side of `Player.stabAttack` on the player `target`
    /// (an index in `stab_players`), queued to land once the hub is back.
    pub(super) fn queue_player_stab(
        &mut self,
        target: usize,
        base_damage: f32,
        deals_damage: bool,
        deals_knockback: bool,
        dismounts: bool,
    ) -> Option<PlayerStab> {
        let victim = self.stab_players.get(target)?.conn;
        let from = self
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z])?;
        let magic_boost = self.enchant_damage_bonus("minecraft:player");
        let (base_damage, magic_boost) = if self.using.is_none() {
            let scale = self.attack_strength_scale();
            (
                base_damage * (0.2 + scale * scale * 0.8),
                magic_boost * scale,
            )
        } else {
            (base_damage, magic_boost)
        };
        let amount = if deals_damage {
            base_damage + magic_boost
        } else {
            0.0
        };
        let (xd, zd) = self.extra_knockback_direction();
        let strength = if deals_knockback {
            // causeExtraKnockback: the attacker stops sprinting.
            let (strength, _) = self.extra_knockback("minecraft:player", 0.0);
            self.stop_sprint_after_extra_knockback();
            strength
        } else {
            0.0
        };
        // LivingEntity.getSecondsToDisableBlocking: none while an item is in use.
        let disable_blocking = if self.using.is_some() {
            0.0
        } else {
            self.attack_item_info()
                .map_or(0.0, |i| i.disable_blocking_seconds)
        };
        Some(PlayerStab {
            victim,
            amount,
            deals_damage,
            deals_knockback,
            dismounts,
            magic: magic_boost > 0.0,
            from,
            knockback: (strength, xd, zd),
            disable_blocking,
            item: self.main_hand_custom_name(),
            ignite: self.weapon_ignite_seconds(),
        })
    }

    /// Keeps a guest's charge tick's or jab's stabs on players: they land
    /// once the guest's turn ends and the hub is back (`land_player_stabs`).
    pub(super) fn keep_player_stabs(
        &mut self,
        kinetic: Kinetic,
        charge: bool,
        felt: bool,
        stabs: Vec<PlayerStab>,
    ) {
        let Some(attacker) = self.lan_acting.filter(|_| !stabs.is_empty()) else {
            return;
        };
        self.player_stabs.push(StabBatch {
            attacker,
            kinetic,
            charge,
            felt,
            stabs,
        });
    }

    /// The queued stabs on players landing, each then followed by the
    /// attacker's side of a stab that affected its victim.
    pub(super) fn land_player_stabs(&mut self, host: &mut LanHost) {
        for batch in std::mem::take(&mut self.player_stabs) {
            let landed: Vec<Landed> = (batch.stabs.iter())
                .filter_map(|s| self.land_player_stab(host, batch.attacker, s))
                .collect();
            self.as_guest(host, batch.attacker, |g| {
                g.after_player_stabs(&batch, &landed)
            });
        }
    }

    /// `stabAttack`'s hurt, pushes and dismount on the victim, then
    /// `awardKillScore` and the victim's thorns. None when the victim has gone.
    fn land_player_stab(
        &mut self,
        host: &mut LanHost,
        attacker: u32,
        s: &PlayerStab,
    ) -> Option<Landed> {
        let victim = Victim::Guest(s.victim);
        let g = host.hub.guest(s.victim).filter(|g| !g.dead)?;
        let id = g.entity_id;
        let (attacker_id, attacker_name) = host
            .hub
            .guest(attacker)
            .map(|a| (a.entity_id, self.player_text(&a.name, a.uuid)))?;
        let hit = self.pvp_hit_of(host, Some(attacker), victim, "melee");
        let land = |g: &mut Self| {
            let _attacker = g.knockback_scope(attacker_id);
            g.combat.player_attacker = Some((s.from, s.disable_blocking));
            g.note_player_attacker(
                "minecraft:spear",
                attacker_id as u32,
                attacker_name.clone(),
                s.item.clone(),
            );
            let health = |g: &Self| g.player.as_ref().map_or(0.0, |p| p.health.health());
            let old_health = health(g);
            let hurt = s.deals_damage && {
                let _hit = pvp_scope(hit);
                g.mob_hits_player(s.amount, s.from, "minecraft:spear")
            };
            g.combat.player_attacker = None;
            // causeExtraKnockback: a push of 0.4, then the weapon's Knockback,
            // whether or not the hurt went through.
            let (strength, xd, zd) = s.knockback;
            if s.deals_knockback {
                g.knock_back_player(0.4, xd, zd, SPEAR_SOURCE);
                if strength > 0.0 {
                    g.knock_back_player(f64::from(strength), xd, zd, SPEAR_SOURCE);
                }
            }
            if hurt
                && s.ignite > 0.0
                && let Some(p) = g.player.as_mut()
            {
                let _ = p.ignite_for_ticks((s.ignite * 20.0) as i32);
            }
            let dismounted = s.dismounts && g.riding.is_some();
            if dismounted {
                g.stop_riding();
            }
            let thorns = if hurt { g.thorns_damage() } else { Vec::new() };
            let (feet, width, height) = g.player.as_ref().map_or(([0.0; 3], 0.6, 1.8), |p| {
                (
                    [p.position.x, p.position.y, p.position.z],
                    f64::from(p.dimensions.width),
                    f64::from(p.dimensions.height),
                )
            });
            let landed = Landed {
                hurt,
                magic: s.magic,
                affected: hurt || s.deals_knockback || dismounted,
                actual: old_health - health(g),
                id,
                feet,
                width,
                height,
            };
            (landed, thorns)
        };
        let (landed, thorns) = self.guest_struck_by(host, s.victim, land)?;
        self.pvp_aftermath(host, victim, Some(attacker), landed.hurt, thorns);
        Some(landed)
    }

    /// The attacker's side of the stabs that affected their victims
    /// (`stabAttack` past its hurt), then the charge's hit feedback
    /// (`broadcastEntityEvent(player, 2)`) or the jab's hit sound when no
    /// mob's stab gave it already.
    fn after_player_stabs(&mut self, batch: &StabBatch, landed: &[Landed]) {
        let mut affected = false;
        for l in landed.iter().filter(|l| l.affected) {
            affected = true;
            // attackVisualEffects: no attack sound, the enchantments' magic crit.
            if l.magic {
                self.critical_hit(self.lan_acting, l.id, l.feet, l.width, l.height, true);
            }
            // itemAttackInteraction: hurtEnemy (statistic, wear), and the
            // post-attack enchantments when the target was hurt.
            if let Some(stack) = self.held().cloned() {
                self.award_stat("used", &stack.item, 1);
                if l.hurt {
                    self.post_attack_bursts(&stack, "minecraft:player");
                }
                self.weapon_wear(&stack);
            }
            self.damage_stats_and_hearts(l.actual, l.feet, l.height);
            if let Some(p) = self.player.as_mut()
                && !self.creative
            {
                p.cause_food_exhaustion(0.1);
            }
        }
        if !affected || batch.felt {
            return;
        }
        if batch.charge {
            self.spear_hit_feedback(&batch.kinetic);
        } else {
            self.play_at_feet(batch.kinetic.hit_sound);
        }
    }
}
