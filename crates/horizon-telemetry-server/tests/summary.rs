//! `GET /v1/benchmarks/summary` against the shared fixtures (tests/fixtures/benchmarks/): the same uploads must
//! give the groups the website's `scripts/benchmarks.mjs` (horizon-rust/horizon-site) gives on its copy of them.
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use horizon_telemetry::Item;
use horizon_telemetry::batch::{self, Batch};
use horizon_telemetry::payload::{Payload, Source};
use horizon_telemetry_server::store::Store;
use horizon_telemetry_server::summary::{self, Upload};
use horizon_telemetry_server::{App, Config, router};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/benchmarks");

fn items() -> Vec<Value> {
    std::fs::read_to_string(format!("{FIXTURES}/items.jsonl"))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn expected() -> Value {
    serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}/expected.json")).unwrap())
        .unwrap()
}

/// The fixtures' short install names as the 32 hex digits real installs have.
fn install_id(name: &str) -> String {
    format!(
        "{:032x}",
        name.bytes().fold(0u128, |n, b| n * 257 + u128::from(b))
    )
}

#[test]
fn the_fixtures_give_the_websites_groups() {
    let uploads: Vec<Upload> = items()
        .into_iter()
        .map(|i| Upload {
            install: i["install_id"]
                .as_str()
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
            version: i["version"].as_str().unwrap_or_default().to_owned(),
            report: i["data"].clone(),
        })
        .collect();
    let s = summary::summary(&uploads, summary::MIN_INSTALLS, "2026-11-03T00:00:00Z");
    let want = expected();
    assert_eq!(s["format"], "horizon-benchmark-summary");
    assert_eq!(s["version"], 1);
    assert_eq!(s["min_installs"], want["min_installs"]);
    assert_eq!(s["groups"], want["groups"]);
}

#[test]
fn only_comparable_runs_count() {
    let reasons: Vec<&str> = items()
        .iter()
        .filter_map(|i| summary::rejection(&i["data"]))
        .collect();
    assert_eq!(
        reasons,
        [
            "VSync on or unknown",
            "offscreen",
            "frame rate capped",
            "not every scene",
            "not the standard flight",
            "not a version 1 benchmark report"
        ]
    );
}

#[test]
fn medians_and_rounding() {
    assert_eq!(summary::median(&[3.0, 1.0, 2.0]), Some(2.0));
    assert_eq!(summary::median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
    assert_eq!(summary::median(&[f64::NAN]), None);
    assert_eq!(summary::round("p95_ms", 3.46), 3.5);
    assert_eq!(summary::round("world_ms", 4512.4), 4512.0);
    assert_eq!(summary::round("avg_fps", 99.5), 100.0);
}

async fn get(app: &Arc<App>, uri: &str) -> (StatusCode, String, Option<String>) {
    let res = router(app.clone())
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let code = res.status();
    let origin = res
        .headers()
        .get("access-control-allow-origin")
        .map(|v| v.to_str().unwrap().to_owned());
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (code, String::from_utf8_lossy(&body).into_owned(), origin)
}

/// Uploaded through `POST /v1/batch` as the launcher sends them, then read back from the endpoint.
#[tokio::test]
async fn uploads_come_back_grouped_and_anonymous() {
    let day = Arc::new(AtomicU32::new(20_396));
    let clock = day.clone();
    let config = Config {
        requests_per_hour: 1e6,
        ..Config::default()
    };
    let app = App::with_clock(
        Store::open(":memory:").unwrap(),
        config,
        Box::new(move || clock.load(std::sync::atomic::Ordering::SeqCst)),
    );
    for i in items() {
        let Some(name) = i["install_id"].as_str() else {
            continue; // an item without an install can't be sent
        };
        let item = Item::new(
            Source::Game,
            i["version"].as_str().unwrap(),
            Payload::Benchmark(i["data"].clone()),
        );
        let body = batch::encode(&Batch::new(&install_id(name), vec![item]));
        let res = router(app.clone())
            .oneshot(
                Request::post("/v1/batch")
                    .header("content-encoding", "zstd")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT, "{name}");
    }

    let (code, body, origin) = get(&app, "/v1/benchmarks/summary").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(origin.as_deref(), Some("https://playhorizon.dev"));
    let s: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(s["groups"], expected()["groups"]);
    assert_eq!(s["groups"][0]["breakdowns"]["vram"][0]["installs"], 3);
    let generated = s["generated"].as_str().unwrap();
    assert!(
        generated.len() == 20 && generated.ends_with('Z'),
        "{generated}"
    );
    for secret in [
        install_id("a1").as_str(),
        "install_id",
        "secret-os-version",
        "secret-driver",
        "20394",
        "ram_mb",
        "vram_mb",
        "16376",
        "32768",
    ] {
        assert!(!body.contains(secret), "{secret} in {body}");
    }

    // Deleting one of the three installs takes the group below three: it goes at once.
    let res = router(app.clone())
        .oneshot(
            Request::delete(format!("/v1/installs/{}", install_id("a3")))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let (_, body, _) = get(&app, "/v1/benchmarks/summary").await;
    let s: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(s["groups"], serde_json::json!([]));
}
