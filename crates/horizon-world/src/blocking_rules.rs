//! Blocks-attacks scalar reduction, delay/cooldown and item-damage rules.
//! Direction calculation, component codecs, equipment mutation and sounds are
//! separate services; angles here are the native caller's radians.
pub trait TypeMatchWorld {
    type Error;
    type Set: Copy;
    fn source_in_set(&mut self, set: Self::Set) -> Result<bool, Self::Error>;
}
#[derive(Clone, Copy, Debug)]
pub struct DamageReduction<S> {
    pub horizontal_angle: f32,
    pub types: Option<S>,
    pub base: f32,
    pub factor: f32,
}
fn min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        f32::from_bits(a.to_bits() | b.to_bits())
    } else {
        a.min(b)
    }
}
fn clamp(value: f32, damage: f32) -> f32 {
    if value < 0.0 { 0.0 } else { min(value, damage) }
}
fn round(value: f32) -> i32 {
    (f64::from(value) + 0.5).floor() as i32
}
impl<S: Copy> DamageReduction<S> {
    pub fn resolve<W: TypeMatchWorld<Set = S>>(
        &self,
        world: &mut W,
        damage: f32,
        angle: f64,
    ) -> Result<f32, W::Error> {
        if angle > f64::from(0.017_453_292_f32 * self.horizontal_angle) {
            return Ok(0.0);
        }
        if let Some(set) = self.types
            && !world.source_in_set(set)?
        {
            return Ok(0.0);
        }
        Ok(clamp(self.base + self.factor * damage, damage))
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ItemDamageFunction {
    pub threshold: f32,
    pub base: f32,
    pub factor: f32,
}
impl ItemDamageFunction {
    pub fn apply(self, damage: f32) -> i32 {
        if damage < self.threshold {
            0
        } else {
            (self.base + self.factor * damage).floor() as i32
        }
    }
}
#[derive(Clone, Debug)]
pub struct BlockingRules<S> {
    pub delay_seconds: f32,
    pub disable_scale: f32,
    pub reductions: Vec<DamageReduction<S>>,
    pub item_damage: ItemDamageFunction,
}
impl<S: Copy> BlockingRules<S> {
    pub fn delay_ticks(&self) -> i32 {
        round(self.delay_seconds * 20.0)
    }
    pub fn disable_ticks(&self, seconds: f32) -> i32 {
        let duration = seconds * self.disable_scale;
        if duration > 0.0 {
            round(duration * 20.0)
        } else {
            0
        }
    }
    pub fn resolve<W: TypeMatchWorld<Set = S>>(
        &self,
        world: &mut W,
        damage: f32,
        angle: f64,
    ) -> Result<f32, W::Error> {
        let mut blocked = 0.0;
        for reduction in &self.reductions {
            blocked += reduction.resolve(world, damage, angle)?;
        }
        Ok(clamp(blocked, damage))
    }
}
