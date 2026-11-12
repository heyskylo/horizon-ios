//! `Mob::seen`: the per-tick snapshot goals read (`game` writes one per mob).

use super::*;

impl Mob {
    /// What other mobs see of this one this tick (`_player` is unused: it keyed the villager
    /// reputation that nothing read). Inline: the game's snapshot of every mob writes each one in
    /// its slot, not through a copy.
    #[inline]
    pub fn seen(&self, _player: Option<[i32; 4]>) -> Seen {
        let m = self;
        let villager = m.species() == Species::Villager;
        Seen {
            in_water: self.fluids.in_water,
            playing_dead: axolotl::is_playing_dead(self),
            last_damage: self.last_damage.map(|(d, _)| d),
            id: m.id,
            species: m.species(),
            position: m.position,
            bounds: m.bounds(),
            hitbox: m.hitbox(),
            eye_y: m.eye_y(),
            alive: m.alive(),
            baby: m.is_baby(),
            // A villager is "in love" when it is willing to breed (`canBreed`).
            in_love: if villager {
                villager::can_breed(m)
            } else {
                m.extra.in_love > 0
            },
            tame: m.extra.tame,
            bred: m.extra.bred,
            owner: m.extra.owner,
            hostile: m.is_enemy(),
            target: m.ai.target,
            health: m.health,
            variant: m.extra.variant,
            size: m.extra.size,
            color: m.extra.color,
            job: villager.then_some(m.extra.home).flatten(),
            bed: villager.then_some(m.extra.bed).flatten(),
            yaw: m.yaw,
            velocity: m.velocity,
            known_speed: m.position.subtract(m.previous),
            leash: m.extra.leash,
            caravan: caravan::head_of(m),
            school: (m.extra.leader, m.extra.followers),
            section: (m.extra.section, m.extra.section_seq),
            delay: m.extra.ints[5],
            hurt_by: m.ai.last_hurt_by.map(|w| (w, m.ai.last_hurt_by_time)),
            invisible: m.invisible
                || m.extra
                    .effects
                    .iter()
                    .any(|e| e.0 == "minecraft:invisibility"),
            panicking: m.ai.panicking,
            active_raid: raider::has_active_raid(m),
            opened_chest: m.extra.opened_chest,
            passenger: m.extra.mount.is_some(),
            vehicle: m.extra.mount,
            pushable: m.pushable(),
            sleeping: villager::sleeping(m)
                || (m.species() == Species::Fox && fox::flag(m, fox::SLEEPING)),
            cold_strider: strider::suffocating(m),
            category: m.category(),
            potential_job: villager_brain::potential_job_site(m),
            meeting: villager.then_some(m.extra.meeting).flatten(),
            profession: m.extra.variant2,
            xp: villager::xp(m),
            villager: villager.then(|| Arc::new(villager::seen(m))),
            stats: [
                f64::from(m.attrs.max_health),
                m.extra.vector[0],
                m.attrs.speed,
            ],
            sitting: m.extra.sitting_pose,
            silent: m.silent,
            lying: m.species() == Species::Cat && (m.extra.flag || m.extra.relax_one),
            ridden: m.rider.is_some() || m.extra.rider.is_some() || m.extra.mount.is_some(),
            is_vehicle: equine::is_vehicle(m),
            still: m.species() == Species::HappyGhast && happy_ghast::on_still_timeout(m),
            carrying: m.rider.is_some() || m.extra.rider.is_some(),
            knockback_resistance: m.attrs.knockback_resistance,
            effects: m
                .extra
                .effects
                .iter()
                .map(|e| e.0.trim_start_matches("minecraft:"))
                .collect(),
            hunted_recently: m.species() == Species::Piglin && m.extra.ints[0] > 0,
            hurt_recently: m.species() == Species::Piglin && m.extra.piglin.hurt,
            huntable: m.species() == Species::Hoglin && !m.is_baby() && !m.extra.cannot_be_hunted,
        }
    }
}
