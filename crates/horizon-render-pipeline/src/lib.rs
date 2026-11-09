//! The external shader pipeline the renderer runs (docs/layers.md, layer plan M6): what a shader
//! pack hands the renderer once its own format is read. The renderer (`horizon-client-render`,
//! engine) builds its targets and passes from these types and does nothing with them on
//! vanilla's path; the pack formats that fill them are platform crates (`horizon-shaders` reads
//! `shaders/` packs). No wgpu and no pack file here.
//!
//! - [`config`]: the colortex buffers and the shadow map: formats, clearing, mipmaps, settings.

pub mod config;

pub use config::{
    BUFFERS, Buffer, Config, Format, HALF_LIVES, Kind, SHADOW_COLORS, Shadow, buffer_index,
    shadow_color,
};
