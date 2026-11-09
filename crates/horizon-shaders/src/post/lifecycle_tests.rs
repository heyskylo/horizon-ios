use super::*;
#[test]
fn setup_and_begin_compute_load_before_prepare_even_with_only_a_suffix() {
    let compute = "#version 430\nlayout(local_size_x=1) in; void main(){}";
    let zip = crate::pack::tests::zip(&[
        ("shaders/setup.csh", compute),
        ("shaders/begin_a.csh", compute),
        ("shaders/prepare.csh", compute),
    ]);
    let pack = Pack::from_zip_bytes("lifecycle.zip", zip).unwrap();
    let post = load_post(
        &pack,
        "",
        &Properties::default(),
        &Macros::standard(),
        &GameInputs::default(),
    );
    assert!(post.notes.is_empty(), "{:?}", post.notes);
    assert_eq!(
        post.programs
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["setup", "begin", "prepare"]
    );
}
