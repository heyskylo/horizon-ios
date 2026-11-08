//! `EnvironmentAttributeSystem`: each attribute's value is its default run through a stack of
//! layers, in the order they were added: the dimension's attributes (constant), the biome's
//! (positional, mixed across nearby biomes for the visual attributes), timelines and the weather
//! (time-based). Also `WeatherAttributes`, `SpatialAttributeInterpolator` and `GaussianSampler`.
use crate::environment_attributes::{Attribute, AttributeMap, Entry, Lerp, Value};
use std::collections::HashMap;

/// `SpatialAttributeInterpolator`: biome attribute maps with their summed weights, in the order
/// they were first seen. The same map (by identity) only adds to its weight.
#[derive(Default)]
pub struct BiomeBlend<'a> {
    weights: Vec<(&'a AttributeMap, f64)>,
}

impl<'a> BiomeBlend<'a> {
    pub fn clear(&mut self) {
        self.weights.clear();
    }

    /// `SpatialAttributeInterpolator.accumulate`.
    pub fn accumulate(&mut self, weight: f64, attributes: &'a AttributeMap) {
        match self
            .weights
            .iter_mut()
            .find(|(m, _)| std::ptr::eq(*m, attributes))
        {
            Some((_, w)) => *w += weight,
            None => self.weights.push((attributes, weight)),
        }
    }

    /// `SpatialAttributeInterpolator.applyAttributeLayer`: each map applied to `base`, folded by
    /// the type's spatial lerp at `weight / weight so far`.
    pub fn apply(&self, attribute: &Attribute, base: &Value) -> Value {
        match self.weights.as_slice() {
            [] => base.clone(),
            [(map, _)] => map.apply(attribute, base),
            [(first, first_weight), rest @ ..] => {
                let mut result = first.apply(attribute, base);
                let mut accumulated = *first_weight;
                for (map, weight) in rest {
                    let value = map.apply(attribute, base);
                    accumulated += weight;
                    result = attribute.lerp(
                        Lerp::Spatial,
                        (weight / accumulated) as f32,
                        &result,
                        &value,
                    );
                }
                result
            }
        }
    }
}

/// `GaussianSampler.sample`: a 6x6x6 kernel around `position - 0.5`, z then x then y.
pub fn gaussian_sample<V>(
    position: [f64; 3],
    mut sampler: impl FnMut(i32, i32, i32) -> V,
    mut accumulate: impl FnMut(f64, V),
) {
    const KERNEL: [f64; 7] = [0.0, 1.0, 4.0, 6.0, 4.0, 1.0, 0.0];
    let p = position.map(|c| c - 0.5);
    let integral = p.map(|c| c.floor() as i32);
    let relative: [f64; 3] = std::array::from_fn(|i| p[i] - f64::from(integral[i]));
    // `Mth.lerp(double)`: `p0 + alpha * (p1 - p0)`.
    let weight =
        |axis: usize, i: usize| KERNEL[i + 1] + relative[axis] * (KERNEL[i] - KERNEL[i + 1]);
    for z in 0..6 {
        let (wz, sz) = (weight(2, z), integral[2] - 2 + z as i32);
        for x in 0..6 {
            let (wx, sx) = (weight(0, x), integral[0] - 2 + x as i32);
            for y in 0..6 {
                let (wy, sy) = (weight(1, y), integral[1] - 2 + y as i32);
                accumulate(wx * wy * wz, sampler(sx, sy, sz));
            }
        }
    }
}

/// What a query knows about where and when it is.
pub struct Conditions<'a, 'b> {
    /// `Level.getRainLevel(1)` and `getThunderLevel(1)`.
    pub rain: f32,
    pub thunder: f32,
    /// The biomes around the position, for spatially interpolated attributes.
    pub blend: Option<&'b BiomeBlend<'a>>,
    /// The biome's attributes at the position: the noise biome, or the block's biome for
    /// attributes with full-resolution biomes.
    pub biome: &'b dyn Fn(bool) -> Option<&'a AttributeMap>,
}

/// A time-based layer other than the weather, such as a timeline's track.
pub type TimeLayer = Box<dyn Fn(&Value) -> Value + Send + Sync>;

enum Layer {
    Constant(Entry),
    Biome,
    Time(TimeLayer),
    Weather {
        rain: Option<Entry>,
        thunder: Option<Entry>,
    },
}

/// `EnvironmentAttributeSystem.Builder`.
#[derive(Default)]
pub struct Builder {
    layers: HashMap<&'static str, (&'static Attribute, Vec<Layer>)>,
}

impl Builder {
    fn push(&mut self, attribute: &'static Attribute, layer: Layer) {
        self.layers
            .entry(attribute.id)
            .or_insert((attribute, Vec::new()))
            .1
            .push(layer);
    }

    /// `addConstantLayer(EnvironmentAttributeMap)`: the dimension type's attributes.
    pub fn constant_layer(&mut self, map: &AttributeMap) -> &mut Self {
        for attribute in map.attributes() {
            if let Some(entry) = map.get(attribute) {
                self.push(attribute, Layer::Constant(entry.clone()));
            }
        }
        self
    }

    /// `addBiomeLayer`: a positional layer for every attribute some biome has.
    pub fn biome_layer<'m>(
        &mut self,
        biomes: impl IntoIterator<Item = &'m AttributeMap>,
    ) -> &mut Self {
        let mut seen: Vec<&'static str> = Vec::new();
        for map in biomes {
            for attribute in map.attributes() {
                if !seen.contains(&attribute.id) {
                    seen.push(attribute.id);
                    self.push(attribute, Layer::Biome);
                }
            }
        }
        self
    }

    /// `addTimeBasedLayer`, for a timeline's track.
    pub fn time_layer(&mut self, attribute: &'static Attribute, layer: TimeLayer) -> &mut Self {
        self.push(attribute, Layer::Time(layer));
        self
    }

    /// `WeatherAttributes.addBuiltinLayers`: a weather layer for every attribute rain or
    /// thunder changes.
    pub fn weather_layers(&mut self) -> &mut Self {
        let (rain, thunder) = weather_maps();
        let attributes: Vec<_> = rain.attributes().chain(thunder.attributes()).collect();
        let mut seen: Vec<&'static str> = Vec::new();
        for attribute in attributes {
            if !seen.contains(&attribute.id) {
                seen.push(attribute.id);
                self.weather_layer(
                    attribute,
                    rain.get(attribute).cloned(),
                    thunder.get(attribute).cloned(),
                );
            }
        }
        self
    }

    /// `WeatherAttributes.addLayer`: the attribute's rain and thunder entries, applied by
    /// [`weather`] when the layer runs.
    fn weather_layer(
        &mut self,
        attribute: &'static Attribute,
        rain: Option<Entry>,
        thunder: Option<Entry>,
    ) {
        self.push(attribute, Layer::Weather { rain, thunder });
    }

    /// `bakeLayerSampler`: leading constant layers fold into the base value.
    pub fn build(self) -> System {
        let samplers = self
            .layers
            .into_iter()
            .filter_map(|(id, (attribute, layers))| {
                let mut base = attribute.default_value()?;
                let mut layers = layers.into_iter().peekable();
                while let Some(Layer::Constant(entry)) = layers.peek() {
                    base = entry.apply(&base);
                    layers.next();
                }
                let layers: Vec<Layer> = layers.collect();
                let positional = layers.iter().any(|l| matches!(l, Layer::Biome));
                Some((
                    id,
                    Sampler {
                        attribute,
                        base,
                        layers,
                        positional,
                    },
                ))
            })
            .collect();
        System { samplers }
    }
}

struct Sampler {
    attribute: &'static Attribute,
    base: Value,
    layers: Vec<Layer>,
    positional: bool,
}

impl Sampler {
    /// `ValueSampler.computeValuePositional` / `computeValueNotPositional`, then
    /// `sanitizeValue`.
    fn compute(&self, conditions: &Conditions, positional: bool) -> Value {
        let attribute = self.attribute;
        let mut result = self.base.clone();
        for layer in &self.layers {
            result = match layer {
                Layer::Constant(entry) => entry.apply(&result),
                Layer::Time(f) => f(&result),
                Layer::Biome if !positional => result,
                // `addBiomeLayerForAttribute`'s positional layer.
                Layer::Biome => match conditions.blend {
                    Some(blend) if attribute.spatially_interpolated() => {
                        blend.apply(attribute, &result)
                    }
                    _ => match (conditions.biome)(attribute.full_resolution_biomes()) {
                        Some(map) => map.apply(attribute, &result),
                        None => result,
                    },
                },
                Layer::Weather { rain, thunder } => {
                    weather(attribute, result, rain, thunder, conditions)
                }
            };
        }
        attribute.sanitize(result)
    }
}

/// The time-based layer `WeatherAttributes.addLayer` adds: rain counts only the part thunder
/// doesn't cover, and each blends in by the type's state-change lerp.
fn weather(
    attribute: &Attribute,
    mut result: Value,
    rain: &Option<Entry>,
    thunder: &Option<Entry>,
    conditions: &Conditions,
) -> Value {
    let thunder_level = conditions.thunder;
    let rain_level = conditions.rain - thunder_level;
    if let Some(entry) = rain.as_ref().filter(|_| rain_level > 0.0) {
        let rained = entry.apply(&result);
        result = attribute.lerp(Lerp::StateChange, rain_level, &result, &rained);
    }
    if let Some(entry) = thunder.as_ref().filter(|_| thunder_level > 0.0) {
        let stormed = entry.apply(&result);
        result = attribute.lerp(Lerp::StateChange, thunder_level, &result, &stormed);
    }
    result
}

/// `EnvironmentAttributeSystem`.
pub struct System {
    samplers: HashMap<&'static str, Sampler>,
}

impl System {
    /// `getValue(attribute, pos, biomeInterpolator)`: the default when no layer has the
    /// attribute (or its type isn't ported).
    pub fn value(&self, attribute: &Attribute, conditions: &Conditions) -> Option<Value> {
        match self.samplers.get(attribute.id) {
            Some(sampler) => Some(sampler.compute(conditions, sampler.positional)),
            None => attribute.default_value(),
        }
    }

    /// `getDimensionValue`: positional layers are skipped.
    pub fn dimension_value(&self, attribute: &Attribute, conditions: &Conditions) -> Option<Value> {
        match self.samplers.get(attribute.id) {
            Some(sampler) => Some(sampler.compute(conditions, false)),
            None => attribute.default_value(),
        }
    }
}

/// `ARGB.as8BitChannel` then `from8BitChannel`: a float color channel through a byte.
fn byte_channel(v: f32) -> f32 {
    (v * 255.0).floor() / 255.0
}

/// `WeatherAttributes.RAIN` and `THUNDER`.
fn weather_maps() -> (AttributeMap, AttributeMap) {
    use crate::environment_attributes::attribute;
    // `Timelines.NIGHT_SKY_LIGHT_COLOR`: `colorFromFloat(1, 0.48, 0.48, 1)`.
    let night = [byte_channel(0.48), byte_channel(0.48), 1.0];
    let map = |gray: (f32, f32), fog: f32, blue: f32, cloud: (f32, f32), alpha: f32| {
        let tint = [byte_channel(fog), byte_channel(fog), byte_channel(blue)];
        let entries = [
            (
                "visual/sky_color",
                Entry::BlendToGray {
                    brightness: gray.0,
                    factor: gray.1,
                },
            ),
            ("visual/fog_color", Entry::Multiply(Value::Rgb(tint))),
            (
                "visual/cloud_color",
                Entry::BlendToGray {
                    brightness: cloud.0,
                    factor: cloud.1,
                },
            ),
            (
                "gameplay/sky_light_level",
                Entry::AlphaBlendFloat { value: 4.0, alpha },
            ),
            (
                "visual/sky_light_color",
                Entry::AlphaBlendColor([night[0], night[1], night[2], byte_channel(alpha)]),
            ),
            (
                "visual/sky_light_factor",
                Entry::AlphaBlendFloat { value: 0.24, alpha },
            ),
            ("visual/star_brightness", Entry::Override(Value::Float(0.0))),
            (
                "visual/sunrise_sunset_color",
                Entry::Multiply(Value::Argb([tint[0], tint[1], tint[2], 1.0])),
            ),
            (
                "gameplay/bees_stay_in_hive",
                Entry::Override(Value::Bool(true)),
            ),
        ];
        AttributeMap::from_entries(
            entries
                .into_iter()
                .filter_map(|(id, e)| Some((attribute(id)?, e))),
        )
    };
    (
        map((0.6, 0.75), 0.5, 0.6, (0.24, 0.5), 0.3125),
        map((0.24, 0.94), 0.25, 0.3, (0.095, 0.94), 0.527_343_75),
    )
}
