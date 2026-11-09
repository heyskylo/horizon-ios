use super::*;
#[test]
fn buffer_blends_use_colortex_numbers_and_preserve_explicit_off() {
    let props = Properties::parse(
        "blend.gbuffers_water=off\nblend.gbuffers_water.colortex13=ONE ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA\nblend.gbuffers_water.colortex2=off\n",
        &Macros::standard(),
    );
    let overrides = buffer_blends(&props, "gbuffers_water");
    assert_eq!(blend(&props, "gbuffers_water"), Some(None));
    assert_eq!(
        overrides,
        vec![
            (
                13,
                Some([
                    Factor::One,
                    Factor::OneMinusSrcAlpha,
                    Factor::One,
                    Factor::OneMinusSrcAlpha
                ])
            ),
            (2, None)
        ]
    );
}
