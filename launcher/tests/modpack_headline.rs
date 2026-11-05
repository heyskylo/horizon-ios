//! The Modpacks page's headline: the same plain lines `coverage.md` opens with, in the
//! launcher's language (English here).
use horizon_launcher::{
    i18n::{self, Language},
    modpacks::Imported,
};
use horizon_port_pack::{
    fetch::Fetched,
    import::Pack,
    report::{self, Row, Status},
};
use std::path::PathBuf;

fn row(class: &str, tier: &str, status: Status) -> Row {
    Row {
        key: format!("{class}-{tier}-{}", status.label()),
        name: String::new(),
        class: class.into(),
        tier: tier.into(),
        license: "MIT".into(),
        license_kind: "permissive".into(),
        status,
        reason: String::new(),
        plain: String::new(),
    }
}

fn pack_with(rows: Vec<Row>) -> Imported {
    Imported {
        slug: "pack".into(),
        work: PathBuf::new(),
        pack: Pack::default(),
        rows,
        fetched: Fetched::default(),
        mods: Vec::new(),
        left_off: Vec::new(),
        portable: 0,
        resourcepacks: 0,
        indexed_resources: 0,
    }
}

#[test]
fn the_headline_says_what_coverage_md_says() {
    i18n::set(Language::English);
    let mixes = [
        vec![],
        vec![row("library", "none", Status::NotNeeded)],
        vec![
            row("content", "data", Status::Partial),
            row("content", "data", Status::Partial),
        ],
        vec![
            row("content", "machines", Status::NotYet),
            row("content", "data", Status::NotYet),
        ],
        vec![
            row("content", "data", Status::Ported),
            row("content", "data", Status::Partial),
            row("content", "deep", Status::NotYet),
            row("unknown", "none", Status::NotYet),
            row("integration", "data", Status::NotYet),
            row("library", "none", Status::NotNeeded),
            row("resourcepack", "data", Status::Partial),
        ],
    ];
    for rows in mixes {
        let expected = report::summary(&rows).lines();
        assert_eq!(pack_with(rows).headline(), expected);
    }
    // An empty pack says nothing; a mixed one starts with the plain count.
    assert!(pack_with(Vec::new()).headline().is_empty());
    let one = pack_with(vec![row("content", "data", Status::Ported)]).headline();
    assert!(
        one[0].starts_with("1 of 1 mods that need porting come over"),
        "{one:?}"
    );
}
