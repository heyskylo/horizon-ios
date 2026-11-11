//! The small option value labels (`generic`, `percent`, `on_off`, `chat_px`,
//! `chat_delay_label`) and `Options.chatDelay`'s slider mapping.
use super::*;

pub(crate) fn generic(caption: &str, value: Text) -> Text {
    Text::translate("options.generic_value", &[tr(caption), value])
}
pub(crate) fn on_off(v: bool) -> Text {
    tr(if v { "options.on" } else { "options.off" })
}
pub(crate) fn chat_px(caption: &str, px: f32) -> Text {
    Text::translate(
        "options.pixel_value",
        &[tr(caption), Text::literal((px as i32).to_string())],
    )
}
pub(crate) fn percent(caption: &str, value: f32) -> Text {
    Text::translate(
        "options.percent_value",
        &[
            tr(caption),
            Text::literal(((f64::from(value) * 100.0) as i32).to_string()),
        ],
    )
}
/// `Options.chatDelay`'s message: `options.chat.delay_none` at zero, else
/// `options.chat.delay` with the delay to one decimal (`%.1f` seconds).
pub(crate) fn chat_delay_label(s: &Settings) -> Text {
    let value = s.file.double("chatDelay") as f32;
    if value <= 0.0 {
        Text::translate("options.chat.delay_none", &[])
    } else {
        Text::translate_with_fallback(
            "options.chat.delay",
            "Chat Delay: %s second(s)",
            &[Text::literal(format!("{value:.1}"))],
        )
    }
}
/// `IntRange(0, 60).toSliderValue` on the stored seconds (`xmap(v / 10.0)`):
/// the tenth-of-a-second step over the slider's 60.
pub(crate) fn kind(value: f64) -> Kind {
    Kind::Slider(((value * 10.0).floor() / 60.0).clamp(0.0, 1.0) as f32)
}
/// `IntRange(0, 60).fromSliderValue` then `xmap`'s `/ 10.0`:
/// floor(map(v, 0, 1, 0, 61)), the last step capped at 60, in tenths.
pub(crate) fn from_slider(v: f32) -> f64 {
    f64::from((v.clamp(0.0, 1.0) * 61.0).floor().min(60.0) / 10.0)
}
/// `Options.chatDelay.set`: the slider's value as the stored seconds.
pub(crate) fn set(s: &mut Settings, v: f32) {
    s.file
        .set("chatDelay", crate::options_file::V::Double(from_slider(v)));
}
