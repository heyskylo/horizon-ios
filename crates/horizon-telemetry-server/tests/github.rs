//! A website bug report as the issue GitHub gets: the template, and people's text made inert.
use horizon_telemetry_server::github::{issue, neutralise};
use serde_json::{Map, Value, json};

#[test]
fn people_text_mentions_and_references_nothing() {
    let text =
        neutralise("@octocat see #12 and https://github.com/a/b/issues/3 <img src=x> `x` Fixes it");
    assert!(!text.contains("@o") && !text.contains("#1") && !text.contains("github.com/"));
    assert!(!text.contains('<') && !text.contains('`'));
    assert!(text.contains("F\u{200b}ixes"));
    // Words that only start like a keyword stay as they are.
    assert_eq!(neutralise("closet fixture"), "closet fixture");
}

#[test]
fn gh_references_link_nothing_either() {
    // GitHub links GH-12 (either case) to issue 12, like #12.
    assert_eq!(neutralise("see GH-12"), "see GH\u{200b}-12");
    assert_eq!(neutralise("gh-7, (Gh-8)"), "gh\u{200b}-7, (Gh\u{200b}-8)");
    // Only at the start of a word and before a number: other words are left alone.
    assert_eq!(
        neutralise("high-5 GH-pages laugh-1 aGH-3"),
        "high-5 GH-pages laugh-1 aGH-3"
    );
}

#[test]
fn a_report_reads_as_the_template() {
    let fields: Map<String, Value> = serde_json::from_value(json!({
        "title": "Water is black\non AMD", "did": "Opened a world", "happened": "Black water",
        "expected": "Blue water", "system": "Windows 11, RX 6600", "name": "steve_"
    }))
    .unwrap();
    let issue = issue(&fields);
    assert_eq!(issue.title, "Water is black on AMD");
    assert!(
        issue
            .body
            .starts_with("> **What I did:**\n> Opened a world\n> **What happened:**")
    );
    assert!(
        issue
            .body
            .contains("> **Horizon version:** not given\n> **OS, CPU, GPU:** Windows 11, RX 6600")
    );
    assert!(
        issue
            .body
            .contains("Reported on playhorizon.dev by **steve**")
    );
    assert_eq!(issue.labels, ["bug", "from-website"]);
}
