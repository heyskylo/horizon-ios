//! Blurring the screenshots on screen (the mockup's `PrivacyToggle`, `launcher-views.tsx`): the
//! eye button beside Overview's and the Screenshots tab's pictures, which draws them as
//! `blur-2xl` would. GPUI has no image filter, so each picture is made once, small and blurred,
//! in the background, and drawn stretched. For the session only, like the mockup's.
use super::*;
use crate::glass_ui::{self as ui, white};
use gpui::RenderImage;
use horizon_theme::glass::{fill, radius};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The blurred copy is made this wide (and 16:9 tall), its blur this many of its pixels: about
/// the mockup's 40 px over its tiles (163 to 335 px wide).
const SMALL: (u32, u32) = (96, 54);
const SIGMA: f32 = 15.;
/// Then smoothed up to this, so the stretched picture shows no texels.
const SHOWN: (u32, u32) = (288, 162);

#[derive(Default)]
pub(super) struct ShotBlur {
    /// Whether the screenshots are blurred.
    pub(super) hidden: bool,
    /// Each picture's blurred copy (None: it couldn't be read).
    made: HashMap<PathBuf, Option<Arc<RenderImage>>>,
    making: HashSet<PathBuf>,
}

impl Launcher {
    /// The eye button (`ICON_BUTTON`, 32 px): blurs the screenshots, or shows them again.
    pub(super) fn shots_privacy(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let hidden = self.shot_blur.hidden;
        ui::icon_button(
            "shots-privacy",
            if hidden { "view-off-slash" } else { "view" },
            32.,
            17.,
            radius::CONTROL,
            hidden,
            &self.theme,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.shot_blur.hidden = !this.shot_blur.hidden;
            cx.notify();
        }))
    }

    /// Screenshot `path` filling its tile, round at `radius::ROW`: blurred while they are
    /// hidden (a plain fill until its copy is made).
    pub(super) fn shot_picture(&mut self, path: &Path, cx: &mut Context<Self>) -> AnyElement {
        if !self.shot_blur.hidden {
            return gpui::img(path.to_path_buf())
                .size_full()
                .rounded(px(radius::ROW))
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element();
        }
        match self.shot_blur.made.get(path) {
            Some(Some(image)) => gpui::img(image.clone())
                .size_full()
                .rounded(px(radius::ROW))
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element(),
            made => {
                if made.is_none() && self.shot_blur.making.insert(path.to_path_buf()) {
                    let read = path.to_path_buf();
                    let task = cx
                        .background_executor()
                        .spawn(async move { blurred(&read) });
                    let path = path.to_path_buf();
                    cx.spawn(async move |this, cx| {
                        let image = task.await;
                        let _ = this.update(cx, |this, cx| {
                            this.shot_blur.making.remove(&path);
                            this.shot_blur.made.insert(path, image);
                            cx.notify();
                        });
                    })
                    .detach();
                }
                div()
                    .size_full()
                    .rounded(px(radius::ROW))
                    .bg(white(fill::HOVER))
                    .into_any_element()
            }
        }
    }
}

/// The picture at `path`, small, blurred and smoothed up, as GPUI draws it (BGRA, premultiplied).
fn blurred(path: &Path) -> Option<Arc<RenderImage>> {
    let picture = image::open(path).ok()?;
    let small = picture
        .resize_to_fill(SMALL.0, SMALL.1, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Some(Arc::new(RenderImage::new([image::Frame::new(gpu_order(
        soften(&small),
    ))])))
}

/// `small` blurred as `blur-2xl` would at its size, then smoothed up to [`SHOWN`].
fn soften(small: &image::RgbaImage) -> image::RgbaImage {
    let soft = image::imageops::blur(small, SIGMA);
    image::imageops::resize(
        &soft,
        SHOWN.0,
        SHOWN.1,
        image::imageops::FilterType::Triangle,
    )
}

/// RGBA to GPUI's BGRA with premultiplied alpha.
fn gpu_order(mut image: image::RgbaImage) -> image::RgbaImage {
    for p in image.pixels_mut() {
        let [r, g, b, a] = p.0;
        let pre = |c: u8| (c as u32 * a as u32 / 255) as u8;
        p.0 = [pre(b), pre(g), pre(r), a];
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blurred_copy_keeps_the_colours_but_not_the_edges() {
        // Left half black, right half white: blurred, the middle turns grey, the far sides
        // keep most of their colour.
        let small = image::RgbaImage::from_fn(SMALL.0, SMALL.1, |x, _| {
            let v = if x < SMALL.0 / 2 { 0 } else { 255 };
            image::Rgba([v, v, v, 255])
        });
        let soft = soften(&small);
        assert_eq!(soft.dimensions(), SHOWN);
        let at = |x: u32| soft.get_pixel(x, SHOWN.1 / 2).0[0] as i32;
        assert!((at(SHOWN.0 / 2) - 128).abs() < 30, "{}", at(SHOWN.0 / 2));
        assert!(
            at(0) < 60 && at(SHOWN.0 - 1) > 195,
            "{} {}",
            at(0),
            at(SHOWN.0 - 1)
        );
    }
}
