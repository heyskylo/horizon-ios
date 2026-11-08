//! Consumable component timing and ordered particle/sound emission.
use horizon_core::random::RandomSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseAnimation {
    None,
    Eat,
    Drink,
    Block,
    Bow,
    Trident,
    Crossbow,
    Spyglass,
    TootHorn,
    Brush,
    Bundle,
    Spear,
}

#[derive(Clone, Debug)]
pub struct Consumable<S, E = ()> {
    pub seconds: f32,
    pub animation: UseAnimation,
    pub sound: S,
    pub particles: bool,
    pub effects: Vec<E>,
}
impl<S: PartialEq, E: PartialEq> PartialEq for Consumable<S, E> {
    fn eq(&self, other: &Self) -> bool {
        let bits = |v: f32| if v.is_nan() { 0x7fc00000 } else { v.to_bits() };
        bits(self.seconds) == bits(other.seconds)
            && self.animation == other.animation
            && self.sound == other.sound
            && self.particles == other.particles
            && self.effects == other.effects
    }
}

pub trait EmissionWorld {
    type Error;
    type Stack: Clone;
    type Sound: Clone;
    type Random: Copy;
    type Source: RandomSource;
    /// This is the supplied random source, which may differ from the actor's
    /// direct field used inside spawn_item_particles.
    fn random_source(&mut self, random: Self::Random) -> &mut Self::Source;
    fn spawn_item_particles(&mut self, stack: Self::Stack, count: i32) -> Result<(), Self::Error>;
    /// Native class/interface membership, not a virtual actor call.
    fn overrides_consume_sound(&self) -> bool;
    fn consume_sound(&mut self, stack: Self::Stack) -> Result<Self::Sound, Self::Error>;
    fn play_sound(
        &mut self,
        sound: Self::Sound,
        volume: f32,
        pitch: f32,
    ) -> Result<(), Self::Error>;
}
impl<S, E> Consumable<S, E> {
    pub fn ticks(&self) -> i32 {
        (self.seconds * 20.0) as i32
    }
    pub fn should_emit(&self, remaining: i32) -> bool {
        let elapsed = self.ticks().wrapping_sub(remaining);
        let threshold = (self.ticks() as f32 * 0.21875) as i32;
        elapsed > threshold && remaining % 4 == 0
    }
}
impl<S: Clone, E> Consumable<S, E> {
    pub fn emit<W: EmissionWorld<Sound = S>>(
        &self,
        world: &mut W,
        random: W::Random,
        stack: W::Stack,
        count: i32,
    ) -> Result<(), W::Error> {
        let source = world.random_source(random);
        let eat_volume = if source.next_boolean() { 0.5 } else { 1.0 };
        let eat_pitch = source.triangle_float(1.0, 0.2);
        let drink_pitch = 0.9f32 + source.next_float() * (1.0f32 - 0.9f32);
        let (volume, pitch) = if self.animation == UseAnimation::Drink {
            (0.5, drink_pitch)
        } else {
            (eat_volume, eat_pitch)
        };
        if self.particles {
            world.spawn_item_particles(stack.clone(), count)?;
        }
        let sound = if world.overrides_consume_sound() {
            world.consume_sound(stack)?
        } else {
            self.sound.clone()
        };
        world.play_sound(sound, volume, pitch)
    }
}

use crate::{food_properties::FoodProperties, item_stack_refs::StackRef};

/// Actor and component services used around the concrete emission and stack
/// kernels. Listener enumeration must retain the component map's native order.
pub trait ConsumptionWorld<I, K, V, E>: EmissionWorld<Stack = StackRef<I, K, V>> {
    type Level: Copy;
    type Hand;
    fn food(&self, stack: &StackRef<I, K, V>) -> Option<FoodProperties>;
    fn is_player(&self) -> bool;
    fn can_eat(&mut self, always: bool) -> Result<bool, Self::Error>;
    fn start_using(&mut self, hand: Self::Hand) -> Result<(), Self::Error>;
    fn level(&mut self) -> Result<Self::Level, Self::Error>;
    fn get_random(&mut self) -> Result<Self::Random, Self::Error>;
    fn is_server_player(&self) -> bool;
    fn award_item_used(&mut self, item: I) -> Result<(), Self::Error>;
    fn consume_criterion(&mut self, stack: StackRef<I, K, V>) -> Result<(), Self::Error>;
    fn consume_listeners(
        &mut self,
        level: Self::Level,
        stack: StackRef<I, K, V>,
        component: &Consumable<Self::Sound, E>,
    ) -> Result<(), Self::Error>;
    fn is_client(&mut self, level: Self::Level) -> Result<bool, Self::Error>;
    fn apply_effect(
        &mut self,
        effect: &E,
        level: Self::Level,
        stack: StackRef<I, K, V>,
    ) -> Result<bool, Self::Error>;
    fn consume_event(&mut self, drink: bool) -> Result<(), Self::Error>;
    fn infinite_materials(&mut self) -> Result<bool, Self::Error>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsumeResult<T> {
    Fail,
    /// Native CONSUME uses no swing; timed use has no transformed stack.
    Consume {
        transformed: Option<T>,
    },
}
impl<S: Clone, E> Consumable<S, E> {
    pub fn can_consume<I, K, V, W: ConsumptionWorld<I, K, V, E, Sound = S>>(
        &self,
        world: &mut W,
        stack: &StackRef<I, K, V>,
    ) -> Result<bool, W::Error> {
        let food = world.food(stack);
        if let Some(food) = food
            && world.is_player()
        {
            return world.can_eat(food.can_always_eat);
        }
        Ok(true)
    }
    pub fn start<
        I: Copy + Eq,
        K: Clone + Eq,
        V: Clone + PartialEq,
        W: ConsumptionWorld<I, K, V, E, Sound = S>,
    >(
        &self,
        world: &mut W,
        stack: StackRef<I, K, V>,
        hand: W::Hand,
    ) -> Result<ConsumeResult<StackRef<I, K, V>>, W::Error> {
        if !self.can_consume(world, &stack)? {
            return Ok(ConsumeResult::Fail);
        }
        if self.ticks() > 0 {
            world.start_using(hand)?;
            Ok(ConsumeResult::Consume { transformed: None })
        } else {
            let level = world.level()?;
            let stack = self.on_consume(world, level, stack)?;
            Ok(ConsumeResult::Consume {
                transformed: Some(stack),
            })
        }
    }
    pub fn on_consume<
        I: Copy + Eq,
        K: Clone + Eq,
        V: Clone + PartialEq,
        W: ConsumptionWorld<I, K, V, E, Sound = S>,
    >(
        &self,
        world: &mut W,
        level: W::Level,
        stack: StackRef<I, K, V>,
    ) -> Result<StackRef<I, K, V>, W::Error> {
        let random = world.get_random()?;
        self.emit(world, random, stack.clone(), 16)?;
        if world.is_server_player() {
            let item = stack.read().item();
            world.award_item_used(item)?;
            world.consume_criterion(stack.clone())?;
        }
        world.consume_listeners(level, stack.clone(), self)?;
        if !world.is_client(level)? {
            for effect in &self.effects {
                let _ = world.apply_effect(effect, level, stack.clone())?;
            }
        }
        world.consume_event(self.animation == UseAnimation::Drink)?;
        if !world.infinite_materials()? {
            stack.write().shrink(1);
        }
        Ok(stack)
    }
}
