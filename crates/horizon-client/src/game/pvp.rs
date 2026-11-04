//! The PvP & HUD page's options in the game (`horizon_client::pvp`, `docs/pvp.md`). Each
//! does nothing while it is off.
use super::Game;
use horizon_client::{
    hud::{HudBatch, rgba},
    pvp::{Colour, CrosshairCache},
};

#[derive(Default)]
pub(super) struct PvpState {
    crosshair: CrosshairCache,
}

impl Game<'_> {
    /// Whether the custom crosshair replaces vanilla's sprite.
    pub(super) fn custom_crosshair(&self) -> bool {
        self.settings.extras.pvp.crosshair.custom
    }
    /// The custom crosshair, centred where vanilla's 15x15 sprite is (`Hud.extractCrosshair`):
    /// its outline in black, then the shape, inverting what is under it when its colour is
    /// `Inverted`. The runs are cached, so a frame only adds their quads.
    pub(super) fn draw_custom_crosshair(&mut self, batch: &mut HudBatch, w: i32, h: i32) {
        let mut c = self.settings.extras.pvp.crosshair;
        if let Some(hover) = c.hover
            && self.crosshair_living() == Some(true)
        {
            c.colour = hover;
        }
        let (cx, cy) = ((w - 15) / 2 + 7, (h - 15) / 2 + 7);
        let atlas = &self.gui.atlas;
        let (shape, rim) = self.pvp.crosshair.runs(&c);
        let fill = |batch: &mut HudBatch, &(x0, y, x1): &(i32, i32, i32), colour: u32| {
            let (x, y) = ((cx + x0) as f32, (cy + y) as f32);
            batch.fill(atlas, x, y, (x1 - x0) as f32, 1.0, colour);
        };
        for run in rim {
            fill(batch, run, rgba(0, 0, 0, 255));
        }
        let start = batch.vertices.len();
        let [a, r, g, b] = c.colour.argb().to_be_bytes();
        for run in shape {
            fill(batch, run, rgba(r, g, b, a));
        }
        if c.colour == Colour::Inverted {
            batch.invert_since(start);
        }
    }
}
