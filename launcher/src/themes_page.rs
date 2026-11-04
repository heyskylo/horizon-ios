//! Settings' accent colour and Home background rows (`horizon_launcher::themes`).
use super::*;
use crate::glass_ui as ui;
use crate::servers_page::Field;
use gpui::{
    canvas, fill, point, size, Bounds, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point,
};
use horizon_launcher::themes::{self, Accent};
use std::path::PathBuf;

/// Settings' two sliders of the glass look (the mockup's `Slider`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum LookKnob {
    /// How much white the plates take: 0 to 16 %.
    Glass,
    /// How much they blur the scene: 0 to 48 px.
    Blur,
}

impl LookKnob {
    fn max(self) -> f32 {
        match self {
            LookKnob::Glass => 16.,
            LookKnob::Blur => 48.,
        }
    }
}

/// Where each slider's track is, and the one being dragged.
#[derive(Default)]
pub(super) struct LookSliders {
    areas: [std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>; 2],
    dragging: Option<LookKnob>,
}

/// A slider's track (`w-52`), in points.
const TRACK: f32 = 208.;

/// Each accent's swatch, in points.
const SWATCH: f32 = 26.;
/// The hue strip beside the swatches, in points: 0 to 360 degrees across it.
const STRIP: f32 = 120.;

impl Launcher {
    /// The theme Settings ask for, under the window's own light or dark: the player's look,
    /// under the launcher theme's sky if they picked one.
    pub(super) fn settings_theme(&self, window: &Window) -> Theme {
        let theme = Theme::for_appearance(
            self.settings.appearance,
            self.settings.accent,
            window.appearance(),
        )
        .with_glass(self.settings.glass_tint(), self.settings.glass_blur());
        match themes::theme(self.settings.theme.as_deref()) {
            Some(picked) => theme.with_sky(picked.sky),
            None => theme,
        }
    }

    /// Settings' launcher theme: Horizon's own sky or First Light, free for everyone. Picking one
    /// brings its accent too; the accent row can change it after.
    pub(super) fn launcher_theme_row(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let current = themes::theme(self.settings.theme.as_deref()).map(|th| th.id);
        let mut options: Vec<(Option<&'static str>, &'static str)> = vec![(None, "Horizon")];
        options.extend(themes::THEMES.iter().map(|th| (Some(th.id), th.name)));
        let this = cx.entity().downgrade();
        let picker = ui::segments("launcher-theme", &options, current, &t, move |id, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.settings.theme = id.map(str::to_owned);
                this.settings.accent = match themes::theme(id) {
                    Some(picked) => Accent::custom(picked.accent_hue),
                    None => Accent::VIOLET,
                };
                this.save_settings();
                this.restyle = true;
                cx.notify();
            });
        });
        ui::setting_row(
            tr("Launcher theme"),
            Some(
                div()
                    .child(tr(
                        "Free for everyone. First Light paints the sky just before sunrise behind the light appearance, with a blue accent.",
                    ))
                    .into_any_element(),
            ),
            Some(picker.into_any_element()),
        )
    }

    /// The picture behind Home, if the player keeps one.
    pub(super) fn home_background(&self) -> Option<PathBuf> {
        themes::background(&backend::data_dir(), self.settings.background.as_deref())
    }

    pub(super) fn accent_row(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let mut swatches = div().flex().gap(px(6.));
        for accent in Accent::ALL {
            let face = Theme::for_appearance(
                if t.dark {
                    backend::Appearance::Dark
                } else {
                    backend::Appearance::Light
                },
                accent,
                gpui::WindowAppearance::Dark,
            )
            .face;
            let chosen = self.settings.accent == accent;
            swatches = swatches.child(
                keyboard(
                    div().id(SharedString::from(format!("accent-{}", accent.name()))),
                    &t,
                )
                .size(px(SWATCH))
                .bg(face)
                .border_2()
                .border_color(if chosen { t.ink } else { t.outline })
                .cursor_pointer()
                .when(chosen, |d| {
                    d.flex()
                        .items_center()
                        .justify_center()
                        .child(div().size(px(8.)).bg(t.on_face))
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.settings.accent = accent;
                    this.save_settings();
                    this.theme = this.settings_theme(window);
                    cx.notify();
                })),
            );
        }
        index_row(
            0,
            tr("Accent colour"),
            Some(
                div()
                    .child(tr(
                        "Horizon's violet by default. Every colour keeps text as easy to read.",
                    ))
                    .into_any_element(),
            ),
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(swatches)
                    .child(self.hue_strip(&t, cx))
                    .into_any_element(),
            ),
            &t,
        )
    }

    /// The glass look's accent row (the mockup's): its seven swatches, the any-colour swatch
    /// (a click shows the hue strip under them, the browser's colour picker in the mockup) and
    /// the hex field, which takes the colour as soon as it is one.
    pub(super) fn glass_accent_row(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        self.sync_accent_hex(cx);
        let current = self.settings.accent;
        let custom = !Accent::ALL.contains(&current);
        let swatch = |id: SharedString, on: bool| {
            keyboard(
                div()
                    .id(id)
                    .relative()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(36.))
                    .rounded(px(10.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(ui::white(0.18)))
                    .when(!on, |d| d.hover(|s| s.bg(ui::white(0.08)))),
                &t,
            )
        };
        let chip = |rgb: u32| div().size(px(24.)).rounded(px(7.)).bg(gpui::rgb(rgb));
        let mut row = div().flex().flex_wrap().items_center().gap(px(4.));
        for (accent, rgb) in Accent::ALL.into_iter().zip(themes::SWATCHES) {
            row = row.child(
                swatch(
                    SharedString::from(format!("accent-{}", accent.name())),
                    current == accent,
                )
                .child(chip(rgb))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.settings.accent = accent;
                    this.save_settings();
                    this.restyle = true;
                    cx.notify();
                })),
            );
        }
        row = row
            .child(
                swatch("accent-any".into(), custom)
                    .child(if custom {
                        chip(current.rgb()).into_any_element()
                    } else {
                        ui::icon("color-picker", 18., ui::white(0.7)).into_any_element()
                    })
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.accent_strip = !this.accent_strip;
                        cx.notify();
                    })),
            )
            .child(self.field(Field::AccentHex, "#6b4bff", 96., cx).ml(px(4.)));
        let control = div()
            .flex()
            .flex_col()
            .items_end()
            .gap(px(10.))
            .child(row)
            .when(self.accent_strip, |d| {
                d.child(
                    self.hue_strip(&t, cx)
                        .w(px(212.))
                        .h(px(24.))
                        .rounded(px(6.))
                        .overflow_hidden(),
                )
            });
        ui::setting_row(
            tr("Accent colour"),
            Some(
                div()
                    .child(tr(
                        "Colours the buttons, the switches and the sky. Pick one, or type any hex colour.",
                    ))
                    .into_any_element(),
            ),
            Some(control.into_any_element()),
        )
    }

    /// The Glass or Blur row: the mockup's slider (a 4 px track filled in the soft accent, a
    /// 16 px white knob) and its value. Dragging changes the look at once and saves on release.
    pub(super) fn look_slider_row(&mut self, knob: LookKnob, cx: &mut Context<Self>) -> Div {
        let (title, about, value, shown) = match knob {
            LookKnob::Glass => {
                let v = (self.settings.glass_tint() * 100.).round();
                (
                    "Glass",
                    "How light the frosted panels and menus are.",
                    v,
                    trf("{value}%", &[("value", &v.to_string())]),
                )
            }
            LookKnob::Blur => {
                let v = self.settings.glass_blur().round();
                (
                    "Blur",
                    "How frosted the panels are.",
                    v,
                    trf("{value} px", &[("value", &v.to_string())]),
                )
            }
        };
        let f = (value / knob.max()).clamp(0., 1.);
        let area = self.look_sliders.areas[knob as usize].clone();
        let track = div()
            .id(SharedString::from(format!("look-slider-{knob:?}")))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .w(px(TRACK))
            .h(px(24.))
            .cursor_pointer()
            .child(
                canvas(move |bounds, _, _| area.set(Some(bounds)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .h(px(4.))
                    .w_full()
                    .rounded_full()
                    .overflow_hidden()
                    .bg(ui::white(0.12))
                    .child(
                        div()
                            .h_full()
                            .w(relative(f))
                            .rounded_full()
                            .bg(ui::color(ui::look().accent_soft)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(f * (TRACK - 16.)))
                    .size(px(16.))
                    .rounded_full()
                    .bg(ui::paper()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.look_sliders.dragging = Some(knob);
                    this.pick_look(knob, ev.position, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                if ev.pressed_button == Some(MouseButton::Left)
                    && this.look_sliders.dragging == Some(knob)
                {
                    this.pick_look(knob, ev.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.finish_look(cx)),
            )
            // A drag released off the track saves too.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.finish_look(cx)),
            );
        let control = div().flex().items_center().gap(px(16.)).child(track).child(
            ui::text(13., FontWeight::NORMAL, 0.6)
                .w(px(48.))
                .text_right()
                .child(shown),
        );
        ui::setting_row(
            tr(title),
            Some(div().child(tr(about)).into_any_element()),
            Some(control.into_any_element()),
        )
    }

    /// The value under the pointer, in whole steps, as the look; saved when the drag ends.
    fn pick_look(&mut self, knob: LookKnob, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(area) = self.look_sliders.areas[knob as usize].get() else {
            return;
        };
        let width: f32 = area.size.width.into();
        if width <= 0. {
            return;
        }
        let across: f32 = (at.x - area.origin.x).into();
        let value = Some(((across / width).clamp(0., 1.) * knob.max()).round());
        let slot = match knob {
            LookKnob::Glass => &mut self.settings.glass,
            LookKnob::Blur => &mut self.settings.blur,
        };
        if *slot != value {
            *slot = value;
            self.restyle = true;
            cx.notify();
        }
    }

    fn finish_look(&mut self, cx: &mut Context<Self>) {
        if self.look_sliders.dragging.take().is_some() {
            self.save_settings();
            cx.notify();
        }
    }

    /// Reset to defaults: the card's options go back to how Horizon ships (the mockup's
    /// accent, glass, blur, language, while playing and opening; here the appearance and Home's
    /// background too, being in the same card).
    pub(super) fn reset_look_row(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        ui::setting_row(
            tr("Reset the launcher's look"),
            Some(
                div()
                    .child(tr(
                        "Accent colour, glass, blur, language and the options above go back to how Horizon ships.",
                    ))
                    .into_any_element(),
            ),
            Some(
                ui::quiet_icon(
                    "look-reset",
                    Some("refresh"),
                    tr("Reset to defaults"),
                    40.,
                    &t,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    let s = &mut this.settings;
                    s.appearance = Default::default();
                    s.accent = Accent::VIOLET;
                    s.theme = None;
                    s.glass = None;
                    s.blur = None;
                    s.background = None;
                    s.language = None;
                    s.while_playing = Default::default();
                    s.skip_opening = false;
                    this.save_settings();
                    i18n::set(this.settings.language());
                    titlebar::apply_appearance(window, this.settings.appearance);
                    this.accent_strip = false;
                    this.background_error = None;
                    this.theme = this.settings_theme(window);
                    crate::glass_toast::say(tr("Launcher settings are back to their defaults"));
                    cx.notify();
                }))
                .into_any_element(),
            ),
        )
    }

    /// The hex field shows the accent, except while typed in: then each whole colour typed is
    /// the accent at once, as in the mockup.
    fn sync_accent_hex(&mut self, cx: &mut Context<Self>) {
        let shown = self.settings.accent.hex();
        if !self.servers.has_focus(Field::AccentHex) {
            if self.servers.value(Field::AccentHex) != shown {
                self.servers.text(Field::AccentHex).set(shown);
            }
            return;
        }
        let typed = self.servers.value(Field::AccentHex).trim();
        let hex = format!("#{}", typed.trim_start_matches('#'));
        if let Some(accent) = Accent::parse(&hex).filter(|a| *a != self.settings.accent) {
            self.settings.accent = accent;
            self.save_settings();
            self.restyle = true;
            cx.notify();
        }
    }

    /// A strip of every hue: a click or a drag picks any of them, the swatches being shortcuts.
    fn hue_strip(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let current = self.settings.accent.hue();
        let area = self.hue_area.clone();
        let ink = t.ink;
        let outline = t.outline;
        // Hues per painted quad: 60 of them, fine enough at this width.
        const STEPS: usize = 60;
        div()
            .relative()
            .flex_none()
            .w(px(STRIP))
            .h(px(SWATCH))
            .border_2()
            .border_color(outline)
            .cursor_pointer()
            .child(
                canvas(
                    move |bounds, _, _| {
                        area.set(Some(bounds));
                        bounds
                    },
                    move |bounds, _, window, _| {
                        let width: f32 = bounds.size.width.into();
                        let step = width / STEPS as f32;
                        for i in 0..STEPS {
                            let left = px(step * i as f32);
                            let colour: Hsla =
                                gpui::rgb(themes::hue_colour(i as f32 * 360. / STEPS as f32))
                                    .into();
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.left() + left, bounds.top()),
                                    size(px(step), bounds.size.height),
                                ),
                                colour,
                            ));
                        }
                        if let Some(hue) = current {
                            let at = px(width * hue / 360.);
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.left() + at - px(1.5), bounds.top()),
                                    size(px(3.), bounds.size.height),
                                ),
                                ink,
                            ));
                        }
                    },
                )
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    this.pick_hue(ev.position, window, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    this.pick_hue(ev.position, window, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_hue();
                    cx.notify();
                }),
            )
            // `on_mouse_up` fires only while the strip is hovered, so a drag released off it would
            // keep its accent in memory and never write it.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_hue();
                    cx.notify();
                }),
            )
    }

    /// A release ends the drag and writes `settings.json` once.
    fn finish_hue(&mut self) {
        if self.hue_dragging {
            self.hue_dragging = false;
            self.save_settings();
        }
    }

    /// The hue under the pointer, as the accent. Saving waits until the mouse comes up, so a drag
    /// writes `settings.json` once.
    fn pick_hue(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(area) = self.hue_area.get() else {
            return;
        };
        let width: f32 = area.size.width.into();
        if width <= 0. {
            return;
        }
        let across: f32 = (at.x - area.origin.x).into();
        self.settings.accent = Accent::custom((across / width).clamp(0., 1.) * 360.);
        self.hue_dragging = true;
        self.theme = self.settings_theme(window);
        cx.notify();
    }

    fn choose_background(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("Choose").to_string().into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.take_background(&path);
                cx.notify();
            });
        })
        .detach();
    }

    fn take_background(&mut self, path: &std::path::Path) {
        match themes::keep_background(&backend::data_dir(), path) {
            Ok(name) => {
                self.settings.background = Some(name);
                self.save_settings();
                self.background_error = None;
            }
            Err(e) => self.background_error = Some(e),
        }
    }

    pub(super) fn home_background_row(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let custom = self.home_background().is_some();
        let mut actions = div().flex().gap(px(4.)).child(
            button(
                "background-choose",
                "Choose a picture…",
                Kind::Secondary,
                &t,
            )
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.choose_background(cx);
                cx.notify();
            })),
        );
        if custom {
            actions = actions.child(
                button("background-reset", "Landscape", Kind::Ghost, &t).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.settings.background = None;
                        this.save_settings();
                        cx.notify();
                    },
                )),
            );
        }
        let said = if custom {
            tr("Your picture, behind Home, cropped to fill it.")
        } else {
            tr("Horizon's landscape. Pick a PNG, JPEG or WebP picture of your own.")
        };
        let detail = div().flex().flex_col().child(said.to_string()).children(
            self.background_error
                .clone()
                .map(|e| div().text_color(t.danger).child(e)),
        );
        index_row(
            0,
            tr("Home background"),
            Some(detail.into_any_element()),
            Some(actions.into_any_element()),
            &t,
        )
    }
}
