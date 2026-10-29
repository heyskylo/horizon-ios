//! Pack-provided PNG inputs, including stage-specific overrides of color-buffer samplers.
use horizon_shaders::{ComputeGroup, Group, TextureDefinition};
pub(super) struct Texture {
    pub definition: TextureDefinition,
    pub view: wgpu::TextureView,
    pub linear: bool,
    pub clamp: bool,
}
pub(super) struct Textures(pub Vec<Texture>);
fn stage_matches(stage: &str, group: Group) -> bool {
    match stage {
        "gbuffers" => matches!(group, Group::Gbuffers | Group::Translucent | Group::Shadow),
        "shadowcomp" => group == Group::ShadowComp,
        "prepare" => group == Group::Prepare,
        "deferred" => group == Group::Deferred,
        "composite" => matches!(group, Group::Composite | Group::Final),
        "final" => group == Group::Final,
        _ => false,
    }
}
fn compute_stage_matches(stage: &str, group: ComputeGroup) -> bool {
    match group {
        ComputeGroup::Setup => stage == "setup",
        ComputeGroup::Begin => stage == "begin",
        ComputeGroup::ShadowComp => stage == "shadowcomp",
        ComputeGroup::Prepare => stage == "prepare",
        ComputeGroup::Deferred => stage == "deferred",
        ComputeGroup::Composite => stage == "composite",
        ComputeGroup::Final => matches!(stage, "composite" | "final"),
    }
}

/// The shader type must match the actual view and its scalar sample type. A PNG or
/// framebuffer view is two-dimensional even when the shader declares a volume sampler.
pub(super) fn sampler_matches(
    ty: &str,
    sample_type: wgpu::TextureSampleType,
    dimension: wgpu::TextureViewDimension,
) -> bool {
    let (scalar, shape) = if let Some(shape) = ty.strip_prefix("usampler") {
        (wgpu::TextureSampleType::Uint, shape)
    } else if let Some(shape) = ty.strip_prefix("isampler") {
        (wgpu::TextureSampleType::Sint, shape)
    } else if let Some(shape) = ty.strip_prefix("sampler") {
        if shape == "2DShadow" {
            return dimension == wgpu::TextureViewDimension::D2
                && sample_type == wgpu::TextureSampleType::Depth;
        }
        (sample_type, shape)
    } else {
        return false;
    };
    let expected_dimension = match shape {
        "1D" => wgpu::TextureViewDimension::D1,
        "2D" => wgpu::TextureViewDimension::D2,
        "3D" => wgpu::TextureViewDimension::D3,
        _ => return false,
    };
    let scalar_matches = if ty.starts_with("sampler") {
        matches!(sample_type, wgpu::TextureSampleType::Float { .. })
    } else {
        sample_type == scalar
    };
    dimension == expected_dimension && scalar_matches
}

fn lookup<'a>(
    definitions: impl Iterator<Item = (usize, &'a TextureDefinition)>,
    name: &str,
    matches: impl Fn(&str) -> bool,
) -> Option<usize> {
    definitions
        .filter(|(_, definition)| {
            same_sampler(&definition.name, name) && definition.stage.as_deref().is_none_or(&matches)
        })
        .max_by_key(|(_, definition)| usize::from(definition.stage.is_some()))
        .map(|(i, _)| i)
}

fn same_sampler(a: &str, b: &str) -> bool {
    a == b
        || horizon_shaders::buffer_index(a)
            .zip(horizon_shaders::buffer_index(b))
            .is_some_and(|(a, b)| a == b)
}
impl Textures {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        definitions: Vec<TextureDefinition>,
    ) -> Result<Self, String> {
        definitions
            .into_iter()
            .map(|definition| {
                let (width, height, rgba) = crate::hud::decode_bytes(&definition.bytes)
                    .map_err(|e| format!("{}: {e}", definition.path))?;
                if width > device.limits().max_texture_dimension_2d
                    || height > device.limits().max_texture_dimension_2d
                {
                    return Err(format!(
                        "{}: PNG exceeds device dimensions",
                        definition.path
                    ));
                }
                let metadata = definition
                    .metadata
                    .as_deref()
                    .map(serde_json::from_str::<serde_json::Value>)
                    .transpose()
                    .map_err(|e| format!("{}.mcmeta: {e}", definition.path))?;
                let linear = metadata
                    .as_ref()
                    .and_then(|m| m["texture"]["blur"].as_bool())
                    .unwrap_or(false);
                let clamp = metadata
                    .as_ref()
                    .and_then(|m| m["texture"]["clamp"].as_bool())
                    .unwrap_or(false);
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(&definition.path),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(height),
                    },
                    texture.size(),
                );
                Ok(Texture {
                    definition,
                    view: texture.create_view(&Default::default()),
                    linear,
                    clamp,
                })
            })
            .collect::<Result<Vec<_>, String>>()
            .map(Self)
    }
    pub fn sampler(&self, name: &str, group: Group) -> Option<usize> {
        lookup(
            self.0.iter().enumerate().map(|(i, t)| (i, &t.definition)),
            name,
            |stage| stage_matches(stage, group),
        )
    }
    pub fn compute_sampler(&self, name: &str, group: ComputeGroup) -> Option<usize> {
        lookup(
            self.0.iter().enumerate().map(|(i, t)| (i, &t.definition)),
            name,
            |stage| compute_stage_matches(stage, group),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compute_texture_overrides_keep_setup_begin_and_prepare_separate() {
        let definitions: Vec<_> = [None, Some("prepare"), Some("begin"), Some("setup")]
            .into_iter()
            .map(|stage| TextureDefinition {
                name: "gaux1".into(),
                stage: stage.map(str::to_owned),
                path: "test.png".into(),
                bytes: vec![],
                metadata: None,
            })
            .collect();
        let find = |group| {
            lookup(definitions.iter().enumerate(), "colortex4", |stage| {
                compute_stage_matches(stage, group)
            })
        };
        assert_eq!(find(ComputeGroup::Setup), Some(3));
        assert_eq!(find(ComputeGroup::Begin), Some(2));
        assert_eq!(find(ComputeGroup::Prepare), Some(1));
        assert_eq!(find(ComputeGroup::Composite), Some(0));
        assert!(compute_stage_matches("composite", ComputeGroup::Final));
        assert!(!compute_stage_matches("prepare", ComputeGroup::Setup));
        assert!(!compute_stage_matches("prepare", ComputeGroup::Begin));
    }
    #[test]
    fn sampler_type_requires_texture_shape_and_scalar_type() {
        use wgpu::{TextureSampleType as S, TextureViewDimension as D};
        let float = S::Float { filterable: true };
        assert!(sampler_matches("sampler2D", float, D::D2));
        assert!(sampler_matches("sampler3D", float, D::D3));
        assert!(!sampler_matches("sampler3D", float, D::D2));
        assert!(!sampler_matches("sampler1D", float, D::D2));
        assert!(!sampler_matches("usampler3D", float, D::D3));
        assert!(sampler_matches("usampler3D", S::Uint, D::D3));
        assert!(!sampler_matches("isampler3D", S::Uint, D::D3));
        assert!(!sampler_matches("sampler3D", S::Uint, D::D3));
        assert!(sampler_matches("sampler2DShadow", S::Depth, D::D2));
        assert!(!sampler_matches("sampler2DShadow", float, D::D2));
        assert!(!sampler_matches("sampler2DShadow", S::Depth, D::D3));
    }
    #[test]
    fn overrides_follow_stage_and_legacy_buffer_aliases() {
        assert!(same_sampler("gaux4", "colortex7"));
        assert!(!same_sampler("noisetex", "colortex0"));
        assert!(stage_matches("gbuffers", Group::Shadow));
        assert!(stage_matches("composite", Group::Final));
        assert!(!stage_matches("deferred", Group::Final));
    }
}
