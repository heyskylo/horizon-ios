//! `horizon-telemetry-server`: serves on `PORT` (default 8080), keeps its data in
//! `HORIZON_TELEMETRY_DB` (default `telemetry.sqlite3`), writes the live count and the official
//! server's down every minute
//! and does its upkeep every ten minutes.
use std::net::SocketAddr;
use std::time::Duration;

use horizon_telemetry_server::{App, Config, router, store::Store};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let path = std::env::var("HORIZON_TELEMETRY_DB").unwrap_or_else(|_| "telemetry.sqlite3".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let store = Store::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let config = Config::from_env();
    if let Some(why) = config.client_ip_refusal() {
        eprintln!("horizon-telemetry-server: {why}");
        std::process::exit(1);
    }
    if let Some(warning) = config.client_ip_warning() {
        eprintln!("horizon-telemetry-server: WARNING: {warning}");
    }
    if std::env::var("HORIZON_TELEMETRY_AGGREGATE_DAYS").is_ok_and(|v| !v.trim().is_empty())
        && config.aggregate_days.is_none()
    {
        eprintln!(
            "horizon-telemetry-server: HORIZON_TELEMETRY_AGGREGATE_DAYS isn't a list of days from 1 to 366: \
             /v1/aggregate takes any value"
        );
    }
    let app = App::new(store, config);
    let upkeep = app.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(600));
        loop {
            every.tick().await;
            // Off the async workers: the upkeep rebuilds totals and deletes rows (SQLite, blocking).
            let worker = upkeep.clone();
            match tokio::task::spawn_blocking(move || worker.maintain()).await {
                Ok(Err(e)) => eprintln!("horizon-telemetry-server: upkeep failed: {e}"),
                Err(e) => eprintln!("horizon-telemetry-server: upkeep stopped: {e}"),
                Ok(Ok(_)) => {}
            }
            // The hub's reports GitHub refused before: their issues, off the async workers.
            let filing = upkeep.clone();
            let _ = tokio::task::spawn_blocking(move || filing.file_pending()).await;
        }
    });
    // The live count's history: one sample a minute (`history`).
    let recorder = app.clone();
    tokio::spawn(async move {
        // A few seconds into each minute, so each tick lands in a minute of its own.
        let into_minute = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() % 60);
        let first = tokio::time::Instant::now() + Duration::from_secs((65 - into_minute) % 60);
        let mut every = tokio::time::interval_at(first, Duration::from_secs(60));
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            every.tick().await;
            let worker = recorder.clone();
            if let Ok(Err(e)) = tokio::task::spawn_blocking(move || worker.record_live()).await {
                eprintln!("horizon-telemetry-server: could not record the live count: {e}");
            }
            // The official server's count: a status ping, off the async workers.
            let official = recorder.clone();
            if let Ok(Err(e)) =
                tokio::task::spawn_blocking(move || official.record_official()).await
            {
                eprintln!("horizon-telemetry-server: could not record the official server: {e}");
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port)))
        .await
        .unwrap_or_else(|e| panic!("port {port}: {e}"));
    eprintln!("horizon-telemetry-server: listening on {port}, data in {path}");
    axum::serve(
        listener,
        router(app).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .unwrap_or_else(|e| panic!("{e}"));
}
