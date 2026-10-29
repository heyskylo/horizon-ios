//! The use animation a `consumable` component names.

pub fn consumable_animation(v: &serde_json::Value) -> horizon_world::consumable::UseAnimation {
    use horizon_world::consumable::UseAnimation;
    match v
        .get("animation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("eat")
    {
        "eat" => UseAnimation::Eat,
        "drink" => UseAnimation::Drink,
        _ => UseAnimation::None,
    }
}
