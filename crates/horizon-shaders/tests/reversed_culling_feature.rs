//! Original feature-negotiation control; no external shader implementation.
use horizon_shaders::{Macros, Properties};

#[test]
fn requested_reversed_culling_loads_and_packs_take_their_base_path() {
    let properties = Properties::parse(
        "iris.features.optional=REVERSED_CULLING\niris.features.required=REVERSED_CULLING\n",
        &Macros::standard(),
    );
    let notes = Macros::feature_notes(&properties);
    assert!(
        notes.is_empty(),
        "supported reversed culling must not be rejected: {notes:?}"
    );
    // Horizon defines no `IRIS_FEATURE_*` (spec OQ-2), so a pack that only asks for reversed
    // culling behind that macro keeps its other value; `shadow.culling=reversed` written
    // plainly still applies.
    let selected = Properties::parse(
        "#ifdef IRIS_FEATURE_REVERSED_CULLING\nshadow.culling=reversed\n#else\nshadow.culling=true\n#endif\n",
        &Macros::standard(),
    );
    assert_eq!(selected.get("shadow.culling"), Some("true"));
}
