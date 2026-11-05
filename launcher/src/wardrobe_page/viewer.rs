//! The Wardrobe's player in 3D: the account's skin wearing its items (or the one a card's hover
//! tries on), drawn on the CPU by `horizon_cosmetics::preview` into an image, again only when
//! `horizon_cosmetics::viewer::Stage` says so. Dragging turns it, the wheel zooms, items
//! animate while the window is in front (not under reduced motion; `HORIZON_LAUNCHER_MOTION=always`
//! keeps them moving behind other windows), and an emote plays once
//! (`Viewer::play`).
//!
//! A view of its own: a new picture builds it alone, and GPUI Fast draws the page around it from
//! the last frame. The page hands it a new outfit after its own frame (`Context::defer`), so the
//! page never reads it while it renders and never depends on it. The pointer is the page's
//! (`frame`): GPUI Fast hit-tests the page's own elements, and hands each event on.
use crate::art;
use gpui::{
    canvas, div, img, prelude::*, px, App, Context, Div, Entity, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderImage, ScrollDelta, ScrollEvent, Stateful,
    Subscription, Window,
};
use horizon_cosmetics::preview::{self, Camera};
use horizon_cosmetics::viewer::{BodyMotion, Shot, Stage};
use horizon_cosmetics::{Item, Part, Texture};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The view's size, in points.
pub(super) const W: f32 = 240.;
pub(super) const H: f32 = 330.;
/// Pixels of a trackpad's scroll that make one wheel step.
const SCROLL_STEP: f32 = 40.;

/// What the player wears in the view.
#[derive(Clone)]
pub(super) struct Outfit {
    pub skin: Arc<Texture>,
    pub slim: bool,
    pub items: Vec<Arc<Item>>,
}

impl PartialEq for Outfit {
    /// The same files: items and skins are shared, never changed in place.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.skin, &other.skin)
            && self.slim == other.slim
            && self.items.len() == other.items.len()
            && (self.items.iter().zip(&other.items)).all(|(a, b)| Arc::ptr_eq(a, b))
    }
}

impl Outfit {
    fn animates(&self) -> bool {
        self.items.iter().any(|i| !i.animations.is_empty())
    }
}

pub(super) struct Viewer {
    outfit: Option<(Outfit, Part)>,
    stage: Stage,
    image: Option<Arc<RenderImage>>,
    /// Pictures no longer shown, still in the window's sprite atlas until dropped there.
    replaced: Vec<Arc<RenderImage>>,
    /// Where the pointer was at the last step of a drag.
    drag: Option<Point<Pixels>>,
    /// A timer is on its way to draw again.
    waking: bool,
    activation: Option<Subscription>,
}

impl Viewer {
    pub(super) fn new() -> Self {
        Self {
            outfit: None,
            stage: Stage::default(),
            image: None,
            replaced: Vec::new(),
            drag: None,
            waking: false,
            activation: None,
        }
    }

    /// Shows `outfit`, if it is a new one; `motion` is whether animations may play.
    pub(super) fn dress(&mut self, outfit: Outfit, motion: bool, cx: &mut Context<Self>) {
        if self.outfit.as_ref().is_some_and(|(o, _)| *o == outfit) {
            return;
        }
        self.stage.dress(motion && outfit.animates());
        let body = preview::player_model(outfit.slim);
        self.outfit = Some((outfit, body));
        cx.notify();
    }

    /// Back to where the view started.
    pub(super) fn reset(&mut self, cx: &mut Context<Self>) {
        if self.stage.moved() {
            self.stage.set_camera(Camera::HOME);
            cx.notify();
        }
    }

    /// Plays an emote once on the player model, its items following their bones. The emote
    /// format isn't in `horizon_cosmetics` yet: its player implements `BodyMotion` and calls
    /// this (docs/cosmetics.md, "The live view").
    #[allow(dead_code)]
    pub(super) fn play(&mut self, motion: Arc<dyn BodyMotion>, cx: &mut Context<Self>) {
        self.stage.play(motion, Instant::now());
        cx.notify();
    }

    fn move_camera(&mut self, camera: Camera, cx: &mut Context<Self>) -> bool {
        if camera == self.stage.camera() {
            return false;
        }
        self.stage.set_camera(camera);
        cx.notify();
        true
    }

    fn drag_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(from) = self.drag.replace(at) else {
            return;
        };
        let (dx, dy) = (f32::from(at.x - from.x), f32::from(at.y - from.y));
        self.move_camera(self.stage.camera().dragged(dx, dy), cx);
    }

    /// The wheel or a trackpad: closer or farther. False at the zoom's ends, where the page
    /// scrolls on.
    fn scroll(&mut self, delta: ScrollDelta, cx: &mut Context<Self>) -> bool {
        let steps = match delta {
            ScrollDelta::Lines(p) => p.y,
            ScrollDelta::Pixels(p) => f32::from(p.y) / SCROLL_STEP,
        };
        self.move_camera(self.stage.camera().zoomed(steps), cx)
    }

    /// The picture for `shot`, `scale` device pixels a point. It is drawn at the view's size in
    /// points, each pixel a block of device pixels as the launcher's pixel art is: a skin's pixel
    /// is 7 points, and a Retina screen costs no more than any other.
    fn draw(&mut self, shot: &Shot, scale: f32) {
        let Some((outfit, body)) = &self.outfit else {
            return;
        };
        let mut draws = vec![preview::Draw {
            poses: shot.body.clone(),
            ..preview::Draw::model(body, &outfit.skin)
        }];
        for item in &outfit.items {
            draws.extend(preview::Draw::item_posed(
                item, body, &shot.body, shot.ticks,
            ));
        }
        let (pw, ph) = (W as u32, H as u32);
        let picture = preview::render(&draws, pw, ph, &shot.camera);
        let texels = picture.rgba.as_chunks::<4>().0;
        let baked = art::bake(texels, pw as usize, ph as usize, 1., scale);
        self.replaced.extend(self.image.replace(baked));
    }

    /// Draws again in `after`, unless a timer is already on its way.
    fn wake(&mut self, after: Duration, cx: &mut Context<Self>) {
        if self.waking {
            return;
        }
        self.waking = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            let _ = this.update(cx, |this, cx| {
                this.waking = false;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.replaced.drain(..) {
            let _ = window.drop_image(image);
        }
        // Coming to the front starts the animations again; leaving stops them.
        if self.activation.is_none() {
            self.activation = Some(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        }
        let scale = window.scale_factor();
        // As the sky: `HORIZON_LAUNCHER_MOTION=always` keeps it moving in the background.
        let front = window.is_window_active() || crate::sky::motion() == crate::sky::Motion::Always;
        let frame = self.stage.frame(Instant::now(), scale, front);
        if let Some(shot) = &frame.shot {
            self.draw(shot, scale);
        }
        if let Some(after) = frame.wake {
            self.wake(after, cx);
        }
        div()
            .w(px(W))
            .h(px(H))
            .children(self.image.clone().map(|i| img(i).w(px(W)).h(px(H))))
    }
}

/// The view in the page, with the pointer: press and drag to turn (followed past the view's
/// edges while held, as a scrollbar's), the wheel to zoom. `dragging` is the page's
/// (`set_dragging` changes it), so that the page draws the grabbing hand.
pub(super) fn frame(
    viewer: Entity<Viewer>,
    dragging: bool,
    set_dragging: impl Fn(bool, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    let (down, wheel, held) = (viewer.clone(), viewer.clone(), viewer.clone());
    let set = set_dragging.clone();
    div()
        .id("wardrobe-viewer")
        .relative()
        .w(px(W))
        .h(px(H))
        .when(dragging, |d| d.cursor_grabbing())
        .when(!dragging, |d| d.cursor_grab())
        .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, _, cx| {
            down.update(cx, |v, _| v.drag = Some(ev.position));
            set(true, cx);
            cx.stop_propagation();
        })
        .on_scroll(move |ev: &ScrollEvent, _, cx| {
            if wheel.update(cx, |v, cx| v.scroll(ev.delta, cx)) {
                cx.stop_propagation();
            }
        })
        .child(viewer)
        .when(dragging, |d| {
            d.child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let (moves, ups) = (held.clone(), held);
                        let set = set_dragging.clone();
                        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                            if !phase.capture() {
                                return;
                            }
                            if ev.pressed_button == Some(MouseButton::Left) {
                                moves.update(cx, |v, cx| v.drag_to(ev.position, cx));
                            } else {
                                moves.update(cx, |v, _| v.drag = None);
                                set(false, cx);
                            }
                        });
                        window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                            if phase.capture() && ev.button == MouseButton::Left {
                                ups.update(cx, |v, _| v.drag = None);
                                set_dragging(false, cx);
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, animated: bool) -> Arc<Item> {
        // A one-box hat, a 32×32 texture of one grey.
        let png = {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(32, 32, image::Rgba([200; 4]))
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
        };
        let animations = if animated {
            r#", "animations": [{"part": "c", "motion": "spin", "axis": "y", "period": 40}]"#
        } else {
            ""
        };
        let file = format!(
            r#"{{"format": 1, "id": "{id}", "slot": "head", "attach": {{"offset": [0, -8, 0]}},
              "texture": {{"png": "{png}"}},
              "parts": [{{"name": "c", "cubes": [{{"origin": [-4, -4, -4], "size": [8, 4, 8], "uv": [0, 0]}}]}}]{animations}}}"#
        );
        Arc::new(Item::parse(file.as_bytes()).unwrap())
    }

    #[test]
    fn outfits_compare_by_the_files_they_share() {
        let skin = Arc::new(super::super::plain());
        let (cap, other) = (item("cap", false), item("cap", false));
        let outfit = |items: Vec<Arc<Item>>| Outfit {
            skin: skin.clone(),
            slim: false,
            items,
        };
        assert!(outfit(vec![cap.clone()]) == outfit(vec![cap.clone()]));
        // Another file read under the same id is another outfit (a new hash).
        assert!(outfit(vec![cap.clone()]) != outfit(vec![other]));
        assert!(outfit(vec![cap.clone()]) != outfit(Vec::new()));
        let slim = Outfit {
            slim: true,
            ..outfit(vec![cap])
        };
        assert!(slim != outfit(vec![item("cap", false)]));
    }

    #[test]
    fn only_items_with_animations_animate() {
        let skin = Arc::new(super::super::plain());
        let outfit = |items| Outfit {
            skin: skin.clone(),
            slim: false,
            items,
        };
        assert!(!outfit(vec![item("cap", false)]).animates());
        assert!(outfit(vec![item("cap", false), item("top", true)]).animates());
        assert!(!outfit(Vec::new()).animates());
    }
}
