//! Damage ownership, origin and damage-type predicates.
use super::{DamageKind, NamedDamage, Species, Who};

/// A damage source: its type, the attacking entity and where it came from.
#[derive(Clone, Copy, Debug)]
pub struct Damage {
    pub kind: DamageKind,
    pub attacker: Option<Who>,
    pub from: Option<[f64; 3]>,
    pub attacker_species: Option<Species>,
    /// UUID captured from the causing entity before deferred damage is applied.
    pub attacker_uuid: Option<[i32; 4]>,
}
impl Damage {
    pub fn with_attacker_uuid(mut self, uuid: Option<[i32; 4]>) -> Self {
        self.attacker_uuid = uuid;
        self
    }

    pub fn fall() -> Self {
        Self {
            kind: DamageKind::Fall,
            attacker: None,
            from: None,
            attacker_species: None,
            attacker_uuid: None,
        }
    }
    pub fn of(kind: DamageKind) -> Self {
        Self {
            kind,
            attacker: None,
            from: None,
            attacker_species: None,
            attacker_uuid: None,
        }
    }
    pub fn player(from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::PlayerAttack,
            attacker: Some(Who::Player),
            from: Some(from),
            attacker_species: None,
            attacker_uuid: None,
        }
    }
    /// `DamageSources.mace`: a player's smash attack.
    pub fn mace(from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::MaceSmash,
            attacker: Some(Who::Player),
            from: Some(from),
            attacker_species: None,
            attacker_uuid: None,
        }
    }
    /// `DamageSources.spear`: a player's stab with a spear.
    pub fn spear(from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::Spear,
            attacker: Some(Who::Player),
            from: Some(from),
            attacker_species: None,
            attacker_uuid: None,
        }
    }
    /// `DamageSources.indirectMagic(potion, owner)`: `attacker` is the owner
    /// (none for a dispenser's potion), `from` the potion's position, which
    /// the knockback pushes away from.
    pub fn indirect_magic(attacker: Option<Who>, from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::IndirectMagic,
            attacker,
            from: Some(from),
            attacker_species: None,
            attacker_uuid: None,
        }
    }
    /// `DamageSources.fireworks(rocket, owner)`: `attacker` is the owner
    /// (none for a rocket nobody fired), `from` the rocket's position, which
    /// the knockback pushes away from (`calculateHorizontalHurtKnockbackDirection`).
    pub fn fireworks(attacker: Option<Who>, from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::Fireworks,
            attacker,
            from: Some(from),
            attacker_species: None,
            attacker_uuid: None,
        }
    }
    pub fn mob(id: u32, from: [f64; 3]) -> Self {
        Self {
            kind: DamageKind::MobAttack,
            attacker: Some(Who::Mob(id)),
            from: Some(from),
            attacker_species: None,
            attacker_uuid: None,
        }
    }
    pub fn with_species(mut self, species: Species) -> Self {
        self.attacker_species = Some(species);
        self
    }
    /// DamageTypeTags.PANIC_CAUSES
    pub fn causes_panic(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::PANIC_CAUSES);
        }
        !matches!(
            self.kind,
            DamageKind::Fall
                | DamageKind::Drown
                | DamageKind::DryOut
                | DamageKind::Generic
                | DamageKind::InWall
                | DamageKind::SweetBerryBush
                | DamageKind::OutOfWorld
                | DamageKind::FallingBlock
                | DamageKind::FallingAnvil
                | DamageKind::Cramming
                | DamageKind::GenericKill
                | DamageKind::OutsideBorder
                | DamageKind::Spit
                | DamageKind::Stalagmite
                | DamageKind::Starve
                | DamageKind::Campfire
        )
    }
    /// DamageTypeTags.BYPASSES_ARMOR
    pub fn bypasses_armor(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::BYPASSES_ARMOR);
        }
        matches!(
            self.kind,
            DamageKind::Fall
                | DamageKind::FlyIntoWall
                | DamageKind::OnFire
                | DamageKind::Drown
                | DamageKind::Magic
                | DamageKind::IndirectMagic
                | DamageKind::InWall
                | DamageKind::Generic
                | DamageKind::SonicBoom
                | DamageKind::Freeze
                | DamageKind::OutOfWorld
                | DamageKind::Cramming
                | DamageKind::GenericKill
                | DamageKind::OutsideBorder
                | DamageKind::Stalagmite
                | DamageKind::Starve
                | DamageKind::Wither
        )
    }
    pub fn is_fire(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::IS_FIRE);
        }
        matches!(
            self.kind,
            DamageKind::OnFire
                | DamageKind::InFire
                | DamageKind::Campfire
                | DamageKind::HotFloor
                | DamageKind::Lava
                | DamageKind::Fireball
                | DamageKind::SulfurCubeHot
        )
    }
    /// DamageTypeTags.NO_KNOCKBACK
    pub fn no_knockback(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::NO_KNOCKBACK);
        }
        matches!(
            self.kind,
            DamageKind::Explosion
                | DamageKind::Fall
                | DamageKind::FlyIntoWall
                | DamageKind::OnFire
                | DamageKind::InFire
                | DamageKind::Campfire
                | DamageKind::HotFloor
                | DamageKind::Lava
                | DamageKind::Drown
                | DamageKind::DryOut
                | DamageKind::Magic
                | DamageKind::Cactus
                | DamageKind::SweetBerryBush
                | DamageKind::Generic
                | DamageKind::InWall
                | DamageKind::Freeze
                | DamageKind::OutOfWorld
                | DamageKind::LightningBolt
                | DamageKind::Cramming
                | DamageKind::GenericKill
                | DamageKind::OutsideBorder
                | DamageKind::Stalagmite
                | DamageKind::Starve
                | DamageKind::SulfurCubeHot
                | DamageKind::Wither
                | DamageKind::Spear
        )
    }
    pub fn is_projectile(&self) -> bool {
        if let DamageKind::Named(named) = self.kind {
            return named.has(NamedDamage::IS_PROJECTILE);
        }
        matches!(
            self.kind,
            DamageKind::Arrow
                | DamageKind::Thrown
                | DamageKind::Fireball
                | DamageKind::WindCharge
                | DamageKind::WitherSkull
        )
    }
}
