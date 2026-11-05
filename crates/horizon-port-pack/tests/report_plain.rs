//! The coverage report in plain words: the headline above the numbers, a sentence for each
//! reason and for "partial", all derived from the report's own rows (`report.rs`).
use horizon_port_pack::{
    inventory::Record,
    report::{Row, Status, Summary, build, markdown, summary},
};
use std::collections::BTreeMap;

fn record(key: &str, class: &str, tier: &str, depends: &[&str]) -> Record {
    Record {
        key: key.into(),
        class: class.into(),
        tier: tier.into(),
        license: "MIT".into(),
        depends: depends.iter().map(|d| (*d).to_owned()).collect(),
        ..Record::default()
    }
}

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
    }
}

#[test]
fn headline_counts_the_reports_own_rows_and_tiers() {
    let rows = [
        row("content", "data", Status::Ported),
        row("content", "data", Status::Partial),
        row("content", "data", Status::Partial),
        row("content", "data", Status::NotYet),
        row("content", "machines", Status::NotYet),
        row("content", "deep", Status::NotYet),
        row("integration", "data", Status::NotYet),
        row("unknown", "none", Status::NotYet),
        row("library", "none", Status::NotNeeded),
        // Not mods: a resource pack is no mod, whatever it is called in the report.
        row("resourcepack", "data", Status::Partial),
        row("shaderpack", "data", Status::NotYet),
    ];
    let s = summary(&rows);
    assert_eq!(
        s,
        Summary {
            needing: 8,
            carried: 3,
            partial: 2,
            ready: 1,
            blocked: 2,
            later: 2
        }
    );
    let lines = s.lines();
    assert!(lines[0].starts_with("3 of 8 mods that need porting come over"));
    assert!(lines[1].starts_with("Not yet: 1 are content mods Port can carry over, 2 need mod support Horizon does not have yet"), "{}", lines[1]);
    assert!(lines[1].contains("2 are not sorted yet"), "{}", lines[1]);
    assert!(lines[2].starts_with("Partial means"));
    assert_eq!(lines.len(), 3);
}

#[test]
fn headline_of_an_all_partial_pack_says_all_come_over_and_what_partial_means() {
    let rows = [
        row("content", "data", Status::Partial),
        row("content", "data", Status::Partial),
    ];
    let lines = summary(&rows).lines();
    assert!(lines[0].starts_with("2 of 2 mods"), "{lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("Not yet")), "{lines:?}");
    assert!(lines[1].starts_with("Partial means"), "{lines:?}");
}

#[test]
fn headline_of_an_all_not_yet_pack_says_none_come_over() {
    let rows = [
        row("content", "machines", Status::NotYet),
        row("content", "data", Status::NotYet),
    ];
    let lines = summary(&rows).lines();
    assert!(lines[0].starts_with("0 of 2 mods"), "{lines:?}");
    assert!(
        lines[1].starts_with("Not yet: 1 are content mods Port can carry over, 1 need mod support"),
        "{lines:?}"
    );
    assert_eq!(lines.len(), 2, "no partial, nothing to explain: {lines:?}");
}

#[test]
fn headline_of_an_empty_pack_or_one_that_needs_no_port_is_empty() {
    assert!(summary(&[]).lines().is_empty());
    let rows = [row("library", "none", Status::NotNeeded)];
    assert!(summary(&rows).lines().is_empty());
    assert!(!markdown("empty", &[]).contains("mods that need porting"));
}

#[test]
fn markdown_has_the_headline_above_the_tables_and_a_plain_column() {
    let rows = [row("content", "data", Status::Partial)];
    let md = markdown("pack", &rows);
    let headline = md.find("**1 of 1 mods that need porting").expect(&md);
    assert!(headline < md.find("| class |").unwrap(), "{md}");
    assert!(md.contains("| why | in plain words |"), "{md}");
}

#[test]
fn every_reason_has_a_plain_sentence_and_the_technical_reason_is_kept() {
    let work = std::env::temp_dir().join(format!("horizon-plain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let dir = work.join("mods/ported");
    std::fs::create_dir_all(dir.join("harness")).unwrap();
    std::fs::write(
        dir.join("carry-report.json"),
        r#"{"blocks":{"a":"minecraft:stone"},"items":0}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("harness/result.json"),
        r#"{"passed":true,"checks":{"blocks":{"passed":1,"failed":0}}}"#,
    )
    .unwrap();
    let mut with_jar = record("rundown", "content", "data", &[]);
    with_jar.jar = Some("rundown.jar".into());
    let records = [
        record("ported", "content", "data", &[]),
        with_jar,
        record("nojar", "content", "data", &[]),
        record("logic", "content", "logic", &[]),
        record("machine", "content", "machines", &[]),
        record("deep", "content", "deep", &["machine", "logic"]),
        record("client", "content", "client", &[]),
        record("odd", "content", "", &[]),
        record("addon", "integration", "logic", &["machine"]),
        record("mystery", "unknown", "none", &[]),
        record("lib", "library", "none", &[]),
        record("fast", "performance", "none", &[]),
        record("tool", "other", "none", &[]),
        record("hud", "client-ui", "none", &[]),
        record("nothing", "content", "none", &[]),
    ];
    let rows = build(&records, &work);
    let by: BTreeMap<&str, &Row> = rows.iter().map(|r| (r.key.as_str(), r)).collect();
    let said = |key: &str| by[key].plain.as_str();
    assert!(said("ported").starts_with("Its blocks and items came over and passed"));
    assert!(said("rundown").contains("jar is here, but it has not been carried over yet"));
    assert!(said("nojar").contains("once its jar is here"));
    assert!(said("logic").contains("when used, placed, broken or ticked"));
    assert!(said("machine").contains("block entities, menus"));
    assert!(said("deep").contains("mobs, custom rendering"));
    assert!(
        by["deep"]
            .plain_text()
            .ends_with("It also waits for 2 mods it depends on that are not ported yet."),
        "{}",
        said("deep")
    );
    assert!(said("client").contains("client interfaces"));
    assert!(said("odd").contains("has not worked out"));
    assert!(said("addon").starts_with("It joins other mods together"));
    assert!(
        by["addon"]
            .plain_text()
            .ends_with("waits for 1 mod it depends on that is not ported yet."),
        "{}",
        said("addon")
    );
    assert!(said("mystery").contains("cannot tell yet what this mod does"));
    assert!(said("lib").contains("library other mods build on"));
    assert!(said("fast").contains("makes the game faster"));
    assert!(said("tool").contains("Pack tooling"));
    assert!(said("hud").contains("Built into Horizon"));
    assert_eq!(said("nothing"), "Nothing to port.");
    // The plain words come beside the technical ones; nothing is replaced.
    assert!(by["machine"].reason.starts_with("needs mod API 0.4"));
    assert_eq!(
        by["mystery"].reason,
        "unclassified: its jar (mods.toml, file census) or a reviewed row decides"
    );
    assert_eq!(by["rundown"].reason, "data tier: carry-over not run yet");
    assert!(
        by["addon"]
            .reason
            .starts_with("after the mods it joins are ported")
    );
    // Every row says it in plain words, without the tier jargon.
    for r in &rows {
        assert!(!r.plain.is_empty(), "{}", r.key);
        assert!(!r.plain.contains("mod API 0."), "{}: {}", r.key, r.plain);
        assert!(!r.plain.contains("unclassified"), "{}: {}", r.key, r.plain);
    }
    std::fs::remove_dir_all(&work).unwrap();
}

#[test]
fn partial_and_resource_rows_are_explained() {
    let work = std::env::temp_dir().join(format!("horizon-plain-partial-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let dir = work.join("mods/half");
    std::fs::create_dir_all(dir.join("harness")).unwrap();
    std::fs::write(
        dir.join("carry-report.json"),
        r#"{"blocks":{"a":"minecraft:stone"},"items":0}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("harness/result.json"),
        r#"{"checks":{"blocks":{"passed":1,"failed":2}}}"#,
    )
    .unwrap();
    let mut pack = record("shaders", "other", "data", &[]);
    pack.path = "shaderpacks/a.zip".into();
    let mut resources = record("textures", "other", "data", &[]);
    resources.path = "resourcepacks/a.zip".into();
    let records = [record("half", "content", "data", &[]), pack, resources];
    let rows = build(&records, &work);
    let by: BTreeMap<&str, &Row> = rows.iter().map(|r| (r.key.as_str(), r)).collect();
    let partial = Status::Partial.meaning();
    assert!(partial.contains("something is left out"), "{partial}");
    assert_eq!(by["half"].status, Status::Partial);
    assert_eq!(by["half"].plain, partial);
    assert_eq!(
        by["shaderpacks/a.zip"].plain,
        "Shader packs are not converted yet."
    );
    assert_eq!(
        by["resourcepacks/a.zip"].plain,
        "Not downloaded yet; Port gets it."
    );
    std::fs::remove_dir_all(&work).unwrap();
}

#[test]
fn a_coverage_json_from_before_the_plain_column_still_reads() {
    let old = r#"{"key":"k","name":"n","class":"content","tier":"data","license":"MIT",
        "license_kind":"permissive","status":"partial","reason":"harness not run"}"#;
    let row: Row = serde_json::from_str(old).unwrap();
    assert_eq!(row.plain, "");
    assert_eq!(row.waiting, 0);
    assert_eq!(row.reason, "harness not run");
}

#[test]
fn dependency_counts_are_separate_from_the_translatable_sentence() {
    let records = [
        record("base", "content", "machines", &[]),
        record("addon", "integration", "logic", &["base"]),
    ];
    let rows = build(&records, &std::env::temp_dir());
    let addon = &rows[1];
    assert_eq!(
        addon.plain,
        "It joins other mods together, so it waits until those are ported."
    );
    assert_eq!(serde_json::to_value(addon).unwrap()["waiting"], 1);
    assert!(
        markdown("test", &rows)
            .contains("It also waits for 1 mod it depends on that is not ported yet.")
    );
}
