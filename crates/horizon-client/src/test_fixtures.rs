//! The prepared vanilla data unit tests read, loaded once per test process.
//!
//! Decoding the block registry, baking every block model and reading the
//! movement tables takes seconds in a debug build, and a test that did it
//! for itself spent most of its time there. Tests only read these, so one
//! copy serves them all; each test still builds its own `World` over it.
use crate::{
    assets::{Assets, RenderAssets},
    motion::MotionResources,
};
use horizon_world::block_collision::BlockCollisionRegistry;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub(crate) struct Vanilla {
    pub assets: Assets,
    pub render: RenderAssets,
    pub collision: BlockCollisionRegistry<'static>,
    pub motion: MotionResources,
}

/// The prepared vanilla data at `HORIZON_TEST_RESOURCES`, or the checkout's
/// `.local/horizon-client/<version>`; `None` when neither has it (CI).
pub(crate) fn vanilla() -> Option<&'static Vanilla> {
    static VANILLA: OnceLock<Option<Vanilla>> = OnceLock::new();
    VANILLA
        .get_or_init(|| {
            let root = std::env::var_os("HORIZON_TEST_RESOURCES")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../.local/horizon-client")
                        .join(horizon_core::VANILLA_VERSION)
                });
            if !root.join("movement-properties.nbt").exists() {
                return None;
            }
            let assets = Assets::load(&root).unwrap();
            let render = assets.reload(&root).unwrap();
            let collision = crate::collisions::load(&root, assets.registry).unwrap();
            let motion = MotionResources::load(&root, &assets).unwrap();
            Some(Vanilla {
                assets,
                render,
                collision,
                motion,
            })
        })
        .as_ref()
}
