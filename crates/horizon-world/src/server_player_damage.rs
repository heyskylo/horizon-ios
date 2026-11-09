//! Compose ordinary server-player inner damage with registered attributes,
//! synchronized health/absorption and stored hunger. Outer hurt gates stay with
//! the caller; equipment, effect, statistic and combat services stay explicit.
use crate::{
    attribute_health::HealthAttributes,
    attributes::entity::{EntityAttributes, LookupError},
    combat_rules::ArmorWorld,
    compression::invalid,
    damage_reduction::{self, Bypass, DamageReductionWorld},
    food::FoodData,
    living_damage::{self, DamageStat, DamageWorld},
    player_health::{PlayerAttributeHealth, PlayerFloatIds, PlayerHealthError},
    synced_floats::{DataError, Observer, SyncedFloats},
};
use std::{fmt, io};
type EnvError<E> = <E as ArmorWorld>::Error;
#[derive(Debug)]
pub enum ServerDamageError<E, A = LookupError> {
    Attribute(A),
    Data(DataError<E>),
    Environment(E),
}
impl<E: fmt::Display, A: fmt::Debug> fmt::Display for ServerDamageError<E, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attribute(e) => write!(f, "damage attribute lookup: {e:?}"),
            Self::Data(e) => e.fmt(f),
            Self::Environment(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static, A: fmt::Debug> std::error::Error for ServerDamageError<E, A> {}
fn health_error<E, A>(e: PlayerHealthError<E, A>) -> ServerDamageError<E, A> {
    match e {
        PlayerHealthError::Attribute(e) => ServerDamageError::Attribute(e),
        PlayerHealthError::Data(e) => ServerDamageError::Data(e),
    }
}
pub trait ServerDamageEnvironment<A: HealthAttributes = EntityAttributes>:
    ArmorWorld + Observer<Error = <Self as ArmorWorld>::Error>
{
    type Attacker: Copy;
    fn invulnerable(&mut self) -> Result<bool, EnvError<Self>>;
    fn bypasses(&mut self, kind: Bypass) -> Result<bool, EnvError<Self>>;
    /// Equipment breakage may update the live attribute map before armor and
    /// toughness are queried by the rest of this hit.
    fn hurt_armor(&mut self, attributes: &mut A, damage: f32) -> Result<(), EnvError<Self>>;
    fn has_resistance(&mut self) -> Result<bool, EnvError<Self>>;
    fn resistance_amplifier(&mut self) -> Result<i32, EnvError<Self>>;
    fn award_resisted(&mut self, amount: i32) -> Result<(), EnvError<Self>>;
    fn attacker(&mut self) -> Result<Option<Self::Attacker>, EnvError<Self>>;
    fn award_dealt_resisted(&mut self, amount: i32) -> Result<(), EnvError<Self>>;
    fn damage_protection(&mut self, level: Self::ServerLevel) -> Result<f32, EnvError<Self>>;
    fn award_self(&mut self, stat: DamageStat, amount: i32) -> Result<(), EnvError<Self>>;
    fn award_attacker(
        &mut self,
        attacker: Self::Attacker,
        stat: DamageStat,
        amount: i32,
    ) -> Result<(), EnvError<Self>>;
    fn source_food_exhaustion(&mut self) -> Result<f32, EnvError<Self>>;
    fn ability_invulnerable(&self) -> bool;
    fn level_is_client(&mut self) -> Result<bool, EnvError<Self>>;
    fn record_damage(&mut self, amount: f32) -> Result<(), EnvError<Self>>;
    fn damage_event(&mut self) -> Result<(), EnvError<Self>>;
    /// [`DamageWorld::damage_pre`]. The default does nothing.
    fn damage_pre(&mut self, _damage: &mut f32) -> Result<(), EnvError<Self>> {
        Ok(())
    }
    /// [`DamageWorld::damage_post`]. The default does nothing.
    fn damage_post(&mut self, _damage: f32) -> Result<(), EnvError<Self>> {
        Ok(())
    }
}
pub struct ServerPlayerDamage<'a, E, A: HealthAttributes = EntityAttributes> {
    attributes: &'a mut A,
    data: &'a mut SyncedFloats,
    food: &'a mut FoodData,
    ids: PlayerFloatIds,
    armor: A::Key,
    toughness: A::Key,
    environment: &'a mut E,
}
impl<'a, A: HealthAttributes, E: ServerDamageEnvironment<A>> ServerPlayerDamage<'a, E, A> {
    pub fn new(
        attributes: &'a mut A,
        data: &'a mut SyncedFloats,
        food: &'a mut FoodData,
        ids: PlayerFloatIds,
        environment: &'a mut E,
    ) -> io::Result<Self> {
        let armor = attributes
            .health_attribute_key("minecraft:armor")
            .ok_or_else(|| invalid("missing ARMOR definition"))?;
        let toughness = attributes
            .health_attribute_key("minecraft:armor_toughness")
            .ok_or_else(|| invalid("missing ARMOR_TOUGHNESS definition"))?;
        PlayerAttributeHealth::new(attributes, data, ids, false, environment)?;
        Ok(Self {
            attributes,
            data,
            food,
            ids,
            armor,
            toughness,
            environment,
        })
    }
    fn health_binding(&mut self) -> PlayerAttributeHealth<'_, E, A> {
        PlayerAttributeHealth::new(
            self.attributes,
            self.data,
            self.ids,
            false,
            self.environment,
        )
        .expect("validated immutable player float schema")
    }
    pub fn apply(&mut self, damage: f32) -> Result<(), ServerDamageError<EnvError<E>, A::Error>> {
        living_damage::actually_hurt(self, damage, true)
    }
    pub(crate) fn apply_after_immunity_check(
        &mut self,
        damage: f32,
    ) -> Result<(), ServerDamageError<EnvError<E>, A::Error>> {
        living_damage::actually_hurt_after_immunity_check(self, damage, true)
    }
}
impl<A: HealthAttributes, E: ServerDamageEnvironment<A>> ArmorWorld
    for ServerPlayerDamage<'_, E, A>
{
    type Error = ServerDamageError<EnvError<E>, A::Error>;
    type Weapon = E::Weapon;
    type ServerLevel = E::ServerLevel;
    fn weapon(&mut self) -> Result<Option<Self::Weapon>, Self::Error> {
        self.environment
            .weapon()
            .map_err(ServerDamageError::Environment)
    }
    fn server_level(&mut self) -> Result<Option<Self::ServerLevel>, Self::Error> {
        self.environment
            .server_level()
            .map_err(ServerDamageError::Environment)
    }
    fn modify_armor_effectiveness(
        &mut self,
        l: Self::ServerLevel,
        w: Self::Weapon,
        v: f32,
    ) -> Result<f32, Self::Error> {
        self.environment
            .modify_armor_effectiveness(l, w, v)
            .map_err(ServerDamageError::Environment)
    }
}
impl<A: HealthAttributes, E: ServerDamageEnvironment<A>> DamageReductionWorld
    for ServerPlayerDamage<'_, E, A>
{
    fn bypasses(&mut self, b: Bypass) -> Result<bool, Self::Error> {
        self.environment
            .bypasses(b)
            .map_err(ServerDamageError::Environment)
    }
    fn hurt_armor(&mut self, v: f32) -> Result<(), Self::Error> {
        self.environment
            .hurt_armor(self.attributes, v)
            .map_err(ServerDamageError::Environment)
    }
    fn armor_value(&mut self) -> Result<i32, Self::Error> {
        self.attributes
            .health_attribute_value(self.armor.clone())
            .map(damage_reduction::armor_from_attribute)
            .map_err(ServerDamageError::Attribute)
    }
    fn toughness_attribute(&mut self) -> Result<f64, Self::Error> {
        self.attributes
            .health_attribute_value(self.toughness.clone())
            .map_err(ServerDamageError::Attribute)
    }
    fn has_resistance(&mut self) -> Result<bool, Self::Error> {
        self.environment
            .has_resistance()
            .map_err(ServerDamageError::Environment)
    }
    fn resistance_amplifier(&mut self) -> Result<i32, Self::Error> {
        self.environment
            .resistance_amplifier()
            .map_err(ServerDamageError::Environment)
    }
    fn victim_is_server_player(&self) -> bool {
        true
    }
    fn award_resisted(&mut self, v: i32) -> Result<(), Self::Error> {
        self.environment
            .award_resisted(v)
            .map_err(ServerDamageError::Environment)
    }
    fn attacker_is_server_player(&mut self) -> Result<bool, Self::Error> {
        self.environment
            .attacker()
            .map(|v| v.is_some())
            .map_err(ServerDamageError::Environment)
    }
    fn award_dealt_resisted(&mut self, v: i32) -> Result<(), Self::Error> {
        self.environment
            .award_dealt_resisted(v)
            .map_err(ServerDamageError::Environment)
    }
    fn damage_protection(&mut self, l: Self::ServerLevel) -> Result<f32, Self::Error> {
        self.environment
            .damage_protection(l)
            .map_err(ServerDamageError::Environment)
    }
}
impl<A: HealthAttributes, E: ServerDamageEnvironment<A>> DamageWorld
    for ServerPlayerDamage<'_, E, A>
{
    type Error = ServerDamageError<EnvError<E>, A::Error>;
    type Attacker = E::Attacker;
    fn invulnerable(&mut self) -> Result<bool, Self::Error> {
        self.environment
            .invulnerable()
            .map_err(ServerDamageError::Environment)
    }
    fn after_armor(&mut self, v: f32) -> Result<f32, Self::Error> {
        damage_reduction::after_armor(self, v)
    }
    fn after_magic(&mut self, v: f32) -> Result<f32, Self::Error> {
        damage_reduction::after_magic(self, v)
    }
    fn absorption(&mut self) -> Result<f32, Self::Error> {
        self.health_binding().absorption().map_err(health_error)
    }
    fn set_absorption(&mut self, v: f32) -> Result<(), Self::Error> {
        self.health_binding()
            .set_absorption(v)
            .map_err(health_error)
    }
    fn server_player_attacker(&mut self) -> Result<Option<Self::Attacker>, Self::Error> {
        self.environment
            .attacker()
            .map_err(ServerDamageError::Environment)
    }
    fn award_attacker(
        &mut self,
        a: Self::Attacker,
        s: DamageStat,
        v: i32,
    ) -> Result<(), Self::Error> {
        self.environment
            .award_attacker(a, s, v)
            .map_err(ServerDamageError::Environment)
    }
    fn award_self(&mut self, s: DamageStat, v: i32) -> Result<(), Self::Error> {
        self.environment
            .award_self(s, v)
            .map_err(ServerDamageError::Environment)
    }
    fn source_food_exhaustion(&mut self) -> Result<f32, Self::Error> {
        self.environment
            .source_food_exhaustion()
            .map_err(ServerDamageError::Environment)
    }
    fn cause_food_exhaustion(&mut self, v: f32) -> Result<(), Self::Error> {
        if !self.environment.ability_invulnerable()
            && !self
                .environment
                .level_is_client()
                .map_err(ServerDamageError::Environment)?
        {
            self.food.add_exhaustion(v);
        }
        Ok(())
    }
    fn record_damage(&mut self, v: f32) -> Result<(), Self::Error> {
        self.environment
            .record_damage(v)
            .map_err(ServerDamageError::Environment)
    }
    fn health(&mut self) -> Result<f32, Self::Error> {
        self.data
            .get(self.ids.health)
            .map_err(|id| ServerDamageError::Data(DataError::UnknownId(id)))
    }
    fn set_health(&mut self, v: f32) -> Result<(), Self::Error> {
        self.health_binding().set_health(v).map_err(health_error)
    }
    fn damage_event(&mut self) -> Result<(), Self::Error> {
        self.environment
            .damage_event()
            .map_err(ServerDamageError::Environment)
    }
    fn damage_pre(&mut self, damage: &mut f32) -> Result<(), Self::Error> {
        self.environment
            .damage_pre(damage)
            .map_err(ServerDamageError::Environment)
    }
    fn damage_post(&mut self, damage: f32) -> Result<(), Self::Error> {
        self.environment
            .damage_post(damage)
            .map_err(ServerDamageError::Environment)
    }
}
