//! Resource packs' Polytone post chains (#11486, wiki "Shaders"): vanilla `post_effect` chains a
//! pack runs over the world while their `activation_condition` holds, with uniform blocks from
//! expressions (`expression_uniforms`), the `PolyGlobals` block, and targets of their own
//! (`post_targets`) kept between frames. `post_chain` runs them; this reads what they add.
use crate::environment::Frame;
use crate::post_chain::{Extras, InternalTarget};
use horizon_polytone::expr::Program;
use serde_json::Value as Json;
use std::path::Path;

/// A `polytone/post_chains/<path>.json`.
struct Chain {
    /// The `post_effect` chain it runs.
    effect: String,
    activation: Option<Program>,
    /// Each expression uniform: its block's name and expression.
    uniforms: Vec<(String, Program)>,
}

/// The packs' post chains and targets.
pub struct PolytonePost {
    chains: Vec<Chain>,
    targets: std::collections::BTreeMap<String, InternalTarget>,
    configs: horizon_polytone::pack::Configs,
}

/// What `PolyGlobals` holds (wiki "Extra Shader Uniforms").
#[derive(Clone, Copy, Debug, Default)]
pub struct PolyGlobals {
    /// The world's projection (reverse-Z, as 26.3's and Horizon's are), and its model-view (the
    /// camera's turn).
    pub projection: glam::Mat4,
    pub model_view: glam::Mat4,
    /// The sun's angle in radians, 0 at the horizon.
    pub sun_angle: f32,
    /// The time of day in ticks (0 to 24000), and the game ticks this frame advanced.
    pub day_time: f32,
    pub delta_time: f32,
    /// The player's feet: `PolyPlayerBlockPos`, and `PolyPlayerOffset` as vanilla's
    /// `CameraOffset` is (the block position less the position, so `polyPlayerPos()` is the
    /// block position less the offset).
    pub player: [f64; 3],
}

impl PolyGlobals {
    /// The std140 block: `mat4, mat4, float, float, float, ivec3, vec3`.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(176);
        for m in [self.projection, self.model_view] {
            out.extend(m.to_cols_array().iter().flat_map(|v| v.to_le_bytes()));
        }
        for v in [self.sun_angle, self.day_time, self.delta_time, 0.0] {
            out.extend(v.to_le_bytes());
        }
        let block = self.player.map(f64::floor);
        for v in block {
            out.extend((v as i32).to_le_bytes());
        }
        out.extend(0i32.to_le_bytes());
        for i in 0..3 {
            out.extend(((block[i] - self.player[i]) as f32).to_le_bytes());
        }
        out.extend(0f32.to_le_bytes());
        out
    }
}

/// Every `.json` under `assets/<namespace>/polytone/<kind>`, by its implicit id.
fn files(root: &Path, kind: &str) -> Vec<(String, Json)> {
    let mut out = Vec::new();
    let namespaces = std::fs::read_dir(root.join("assets"))
        .into_iter()
        .flatten()
        .flatten();
    for namespace in namespaces {
        let mut stack = vec![namespace.path().join("polytone").join(kind)];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                let Some(id) = horizon_polytone::implicit_id(&relative, kind) else {
                    continue;
                };
                match horizon_world::json_file::read::<Json>(&path) {
                    Ok(json) => out.push((id, json)),
                    Err(e) => log::warn!("{relative}: {e}"),
                }
            }
        }
    }
    out
}

fn compile(where_: &str, src: &Json) -> Option<Program> {
    let text = match src {
        Json::String(s) => s.clone(),
        Json::Number(n) => n.to_string(),
        Json::Bool(b) => b.to_string(),
        _ => return None,
    };
    match Program::compile(&text) {
        Ok(p) => Some(p),
        Err(e) => {
            log::warn!("{where_}: {e}");
            None
        }
    }
}

impl PolytonePost {
    /// The post chains and targets of the packs merged at `root`; `None` without chains.
    pub fn read(root: &Path, configs: horizon_polytone::pack::Configs) -> Option<Self> {
        let chains: Vec<Chain> = files(root, "post_chains")
            .into_iter()
            .filter_map(|(id, json)| {
                let effect = json["post_chain"].as_str()?;
                let effect = if effect.contains(':') {
                    effect.to_owned()
                } else {
                    format!("minecraft:{effect}")
                };
                let activation = json
                    .get("activation_condition")
                    .and_then(|c| compile(&id, c));
                let uniforms = json["expression_uniforms"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(name, src)| Some((name.clone(), compile(&id, src)?)))
                    .collect();
                Some(Chain {
                    effect,
                    activation,
                    uniforms,
                })
            })
            .collect();
        if chains.is_empty() {
            return None;
        }
        let targets = files(root, "post_targets")
            .into_iter()
            .map(|(id, json)| {
                let size = |key: &str| json[key].as_u64().filter(|n| *n > 0).map(|n| n as u32);
                let target = InternalTarget {
                    width: size("width"),
                    height: size("height"),
                    persistent: true,
                    clear: [0.0; 4],
                };
                (id, target)
            })
            .collect();
        Some(Self {
            chains,
            targets,
            configs,
        })
    }

    /// The chains to run this frame (those whose condition holds), and what they add.
    pub fn frame(&self, frame: &Frame, globals: &PolyGlobals) -> (Vec<String>, Extras) {
        let mut effects = Vec::new();
        let mut extras = Extras {
            targets: self.targets.clone(),
            blocks: Default::default(),
        };
        extras
            .blocks
            .insert("PolyGlobals".to_owned(), globals.bytes());
        for chain in &self.chains {
            let on = chain.activation.as_ref().is_none_or(|c| {
                crate::environment::eval_frame(c, frame, &self.configs).is_some_and(|v| v.truthy())
            });
            if !on {
                continue;
            }
            for (name, program) in &chain.uniforms {
                let value = crate::environment::eval_frame(program, frame, &self.configs)
                    .map_or(0.0, |v| v.num()) as f32;
                let mut block = value.to_le_bytes().to_vec();
                block.resize(16, 0);
                extras.blocks.insert(name.clone(), block);
            }
            effects.push(chain.effect.clone());
        }
        (effects, extras)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poly_globals_follow_std140() {
        let g = PolyGlobals {
            projection: glam::Mat4::from_diagonal(glam::Vec4::splat(2.0)),
            sun_angle: 1.5,
            day_time: 6000.0,
            delta_time: 0.5,
            player: [10.25, -3.5, 7.0],
            ..Default::default()
        };
        let b = g.bytes();
        let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let i = |at: usize| i32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        assert_eq!(b.len(), 176);
        assert_eq!(f(0), 2.0);
        assert_eq!((f(128), f(132), f(136)), (1.5, 6000.0, 0.5));
        // ivec3 at 144, vec3 at 160 (std140 aligns both to 16). The offset is the block position
        // less the position, as vanilla's CameraOffset: the block position less it is the position.
        assert_eq!((i(144), i(148), i(152)), (10, -4, 7));
        assert_eq!((f(160), f(164), f(168)), (-0.25, -0.5, 0.0));
    }

    #[test]
    fn chains_run_while_their_condition_holds_with_their_uniforms() {
        let root =
            std::env::temp_dir().join(format!("horizon-polytone-post-{}", std::process::id()));
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "assets/p/polytone/post_chains/glare.json",
            r#"{"activation_condition": "g.rain() < 0.5", "post_chain": "p:glare",
                "expression_uniforms": {"Weather": "1 - g.rain()"}}"#,
        );
        write(
            "assets/p/polytone/post_targets/history.json",
            r#"{"width": 1, "height": 1}"#,
        );
        let post = PolytonePost::read(&root, Default::default()).unwrap();
        let mut frame = Frame {
            rain: 0.25,
            ..Frame::default()
        };
        let (effects, extras) = post.frame(&frame, &PolyGlobals::default());
        assert_eq!(effects, ["p:glare"]);
        // `g.rain` is the mean of rain (0.25) and thunder (0): 0.125.
        assert_eq!(
            f32::from_le_bytes(extras.blocks["Weather"][..4].try_into().unwrap()),
            0.875
        );
        assert!(extras.targets["p:history"].persistent);
        frame.rain = 1.0;
        assert!(post.frame(&frame, &PolyGlobals::default()).0.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
