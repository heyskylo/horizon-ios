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
        waiting: 0,
        old_layout: false,
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
        datapacks: 0,
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

#[test]
fn row_explanations_keep_the_coverage_reports_english_and_dependency_counts() {
    use horizon_launcher::modpacks::row_plain;
    use horizon_port_pack::inventory::Record;
    i18n::set(Language::English);
    let records: Vec<_> = [
        ("base", "content", "machines", vec![]),
        ("logic", "content", "logic", vec![]),
        ("one", "integration", "logic", vec!["base"]),
        ("two", "content", "deep", vec!["base", "logic"]),
        ("unknown", "unknown", "none", vec![]),
        ("data", "content", "data", vec![]),
        ("client", "client-ui", "client", vec![]),
        ("library", "library", "none", vec![]),
        ("performance", "performance", "none", vec![]),
        ("other", "other", "none", vec![]),
    ]
    .into_iter()
    .map(|(key, class, tier, depends)| Record {
        key: key.into(),
        class: class.into(),
        tier: tier.into(),
        depends: depends.into_iter().map(str::to_owned).collect(),
        ..Default::default()
    })
    .collect();
    let missing = std::env::temp_dir().join(format!("horizon-row-english-{}", std::process::id()));
    let rows = report::build(&records, &missing);
    for row in &rows {
        assert_eq!(row_plain(row), row.plain_text(), "{}", row.key);
    }
    assert!(row_plain(&rows[2]).ends_with("1 mod it depends on that is not ported yet."));
    assert!(row_plain(&rows[3]).ends_with("2 mods it depends on that are not ported yet."));
    // Reports from before the new count field, and future sentences, still show their text.
    let old: Row = serde_json::from_str(
        r#"{"key":"old","name":"","class":"content",
        "tier":"logic","license":"MIT","license_kind":"permissive","status":"not-yet",
        "reason":"detail","plain":"An explanation from another build."}"#,
    )
    .unwrap();
    assert_eq!(row_plain(&old), "An explanation from another build.");
    assert!(row_plain(&row("library", "none", Status::NotNeeded)).is_empty());
}
