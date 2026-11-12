use super::*;
use crate::spans;
use horizon_perf_format::v1::{TimelineChunk, WarningCode};
use timeline::Caps;

/// Runs `f` on a thread named `name`, whose spans the timeline files under that name.
fn on_thread(name: &str, f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

fn chunks_of<'a>(c: &'a Capture, thread: &str) -> Vec<&'a TimelineChunk> {
    c.timeline
        .iter()
        .filter(|chunk| text(c, chunk.thread) == thread)
        .collect()
}

fn timeline_options(caps: Caps) -> CaptureOptions {
    CaptureOptions {
        timeline: Some(caps),
        ..options(100.0)
    }
}

#[test]
fn a_known_span_sequence_reads_back_with_its_durations_and_depths() {
    let _timeline = timeline::test_lock();
    let scratch = Scratch::new("timeline-known");
    let mut perf = ServerPerf::new(0.0, 250.0);
    perf.start_capture(0.0, &scratch.0, timeline_options(Caps::default()))
        .unwrap();
    on_thread("tl-known", || {
        spans::time("tick", || {
            sleep_ms(5);
            spans::time("tick_mobs", || sleep_ms(10));
            spans::time("tick_blocks", || sleep_ms(6));
        });
    });
    for i in 0..40 {
        perf.tick(
            f64::from(i) * 0.1,
            10.0,
            None,
            WorldCounts::default,
            Vec::new,
        );
    }
    let saved = stop(&mut perf, 4.0, R::Normal).unwrap().unwrap();

    let c = read(&saved.path);
    assert!(c.start[0].options.as_ref().unwrap().timeline);
    assert!(c.warnings.is_empty());
    assert_eq!(
        c.stop[0].overhead.as_ref().unwrap().timeline_stopped_t,
        None
    );
    let chunks = chunks_of(&c, "tl-known");
    assert_eq!(
        chunks.len(),
        1,
        "one chunk for the one second they ended in"
    );
    let chunk = chunks[0];
    assert_eq!(chunk.side(), S::Server);
    let names: Vec<_> = chunk.name.iter().map(|&id| text(&c, id)).collect();
    assert_eq!(names, ["tick", "tick_mobs", "tick_blocks"]);
    assert_eq!(chunk.depth, [0, 1, 1]);
    let (tick, mobs, blocks) = (0, 1, 2);
    assert!(chunk.dur_us[mobs] >= 10_000 && chunk.dur_us[blocks] >= 6_000);
    assert!(chunk.dur_us[tick] >= chunk.dur_us[mobs] + chunk.dur_us[blocks] + 5_000);
    let end = |i: usize| chunk.start_us[i] + u64::from(chunk.dur_us[i]);
    assert!(chunk.start_us[mobs] >= chunk.start_us[tick] + 5_000);
    assert!(chunk.start_us[blocks] >= end(mobs));
    assert!(end(blocks) <= end(tick));
    assert!(
        !spans::timeline::on(),
        "stopping the capture stops the timeline"
    );
}

#[test]
fn the_time_cap_stops_the_timeline_and_says_when() {
    let _timeline = timeline::test_lock();
    let scratch = Scratch::new("timeline-cap");
    let mut perf = ServerPerf::new(0.0, 250.0);
    let caps = Caps {
        seconds: 3.0,
        ..Caps::default()
    };
    perf.start_capture(0.0, &scratch.0, timeline_options(caps))
        .unwrap();
    for i in 0..12 {
        let t = f64::from(i) * 0.5;
        on_thread("tl-cap", || spans::time("tick", || ()));
        perf.tick(t, 10.0, None, WorldCounts::default, Vec::new);
    }
    assert!(
        !spans::timeline::on(),
        "the cap stopped the timeline at t = 3"
    );
    let saved = stop(&mut perf, 6.0, R::Normal).unwrap().unwrap();

    let c = read(&saved.path);
    assert_eq!(
        c.stop[0].overhead.as_ref().unwrap().timeline_stopped_t,
        Some(3.0)
    );
    assert_eq!(c.warnings.len(), 1);
    assert_eq!(c.warnings[0].code(), WarningCode::TimelineCapped);
    assert_eq!(c.warnings[0].t, 3.0);
    assert_eq!(c.warnings[0].args[0], 3.0);
    assert!(c.warnings[0].args[1] > 0.0);
    let chunks = chunks_of(&c, "tl-cap");
    assert_eq!(chunks.len(), 3, "the drains at 1, 2 and 3 s, none after");
    let spans_kept: usize = chunks.iter().map(|chunk| chunk.name.len()).sum();
    assert_eq!(
        spans_kept, 7,
        "a span per half second, up to the one at 3 s"
    );
}

#[test]
fn a_second_capture_does_not_take_over_a_running_timeline() {
    let _timeline = timeline::test_lock();
    let scratch = Scratch::new("timeline-twice");
    let (mut first, mut second) = (ServerPerf::new(0.0, 250.0), ServerPerf::new(0.0, 250.0));
    first
        .start_capture(0.0, &scratch.0, timeline_options(Caps::default()))
        .unwrap();
    second
        .start_capture(0.0, &scratch.0, timeline_options(Caps::default()))
        .unwrap();
    let second = stop(&mut second, 1.0, R::Normal).unwrap().unwrap();
    assert!(
        spans::timeline::on(),
        "the second capture left the first one's timeline alone"
    );
    let first = stop(&mut first, 1.0, R::Normal).unwrap().unwrap();
    let on = |path: &Path| read(path).start[0].options.as_ref().unwrap().timeline;
    assert_eq!((on(&first.path), on(&second.path)), (true, false));
}
