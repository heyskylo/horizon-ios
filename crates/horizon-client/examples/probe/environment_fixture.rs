//! Environment attributes for differential tests against vanilla
//! (tools/test_environment_render_parity.py): reads "dimension ticks rain
//! thunder biome" lines and prints the sampled sky state as JSON lines.
//!
//! usage: environment_fixture RESOURCES INPUT OUTPUT
use horizon_client::{
    biome::Biomes,
    environment::{Environment, Sample},
};
use std::{collections::HashMap, fmt::Write as _, fs, path::Path};

pub fn main() -> horizon_client::Result<()> {
    let args: Vec<String> = crate::args().collect();
    let root = Path::new(
        args.get(1)
            .ok_or("usage: environment_fixture RESOURCES INPUT OUTPUT")?,
    );
    let input = fs::read_to_string(args.get(2).ok_or("missing INPUT")?)?;
    let biomes = Biomes::load(root)?;
    let mut environments: HashMap<String, Environment> = HashMap::new();
    let mut out = String::new();
    for line in input.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        let dimension = f[0];
        let environment = match environments.entry(dimension.to_owned()) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(Environment::load_dimension(root, dimension)?)
            }
        };
        let info = biomes.get(biomes.id(&format!("minecraft:{}", f[4])));
        // The weather layers only exist where the dimension can have weather.
        let sky = environment.sample(&Sample {
            day_time: f[1].parse()?,
            rain: f[2].parse()?,
            thunder: f[3].parse()?,
            blend: None,
            biome: Some(&info.attributes),
            ..Sample::default()
        });
        let v3 = |c: [f32; 3]| format!("[{:?},{:?},{:?}]", c[0], c[1], c[2]);
        let v4 = |c: [f32; 4]| format!("[{:?},{:?},{:?},{:?}]", c[0], c[1], c[2], c[3]);
        let mut row = format!("{{\"case\":\"{line}\"");
        let mut put = |name: &str, value: String| {
            let _ = write!(row, ",\"minecraft:{name}\":{value}");
        };
        put("visual/sky_color", v3(sky.sky_color));
        put("visual/fog_color", v3(sky.fog_color));
        put("visual/cloud_color", v4(sky.cloud_color));
        put("visual/sunrise_sunset_color", v4(sky.sunrise_color));
        put(
            "visual/sky_light_factor",
            format!("{:?}", sky.sky_light_factor),
        );
        put("visual/sky_light_color", v3(sky.sky_light_color));
        put("visual/block_light_tint", v3(sky.block_light_tint));
        put("visual/ambient_light_color", v3(sky.ambient_light_color));
        put("visual/sun_angle", format!("{:?}", sky.sun_angle));
        put("visual/moon_angle", format!("{:?}", sky.moon_angle));
        put("visual/star_angle", format!("{:?}", sky.star_angle));
        put(
            "visual/star_brightness",
            format!("{:?}", sky.star_brightness),
        );
        put(
            "gameplay/sky_light_level",
            format!("{:?}", sky.sky_light_level * 15.0),
        );
        put("visual/fog_start_distance", format!("{:?}", sky.fog_start));
        put("visual/fog_end_distance", format!("{:?}", sky.fog_end));
        put(
            "visual/cloud_fog_end_distance",
            format!("{:?}", sky.cloud_fog_end),
        );
        put("visual/cloud_height", format!("{:?}", sky.cloud_height));
        out.push_str(&row);
        out.push_str("}\n");
    }
    fs::write(args.get(3).ok_or("missing OUTPUT")?, out)?;
    Ok(())
}
