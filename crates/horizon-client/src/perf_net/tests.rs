use super::*;

fn start(seconds: u32) -> Wanted {
    Wanted {
        seconds: Some(seconds),
        hz: None,
        over_ms: None,
        threads: Threads::Main,
        wall: false,
        samples: true,
        timeline: false,
    }
}

fn started(path: &str, seconds: Option<u32>) -> Reply {
    Reply::Started {
        path: path.into(),
        seconds,
        samples: true,
    }
}

fn finished(path: &str) -> Reply {
    Reply::Finished {
        path: path.into(),
        removed: Vec::new(),
    }
}

#[test]
fn a_start_request_is_the_bytes_the_wire_says() {
    let wanted = Wanted {
        seconds: Some(60),
        hz: Some(500),
        over_ms: Some(2.5),
        threads: Threads::All,
        wall: true,
        samples: false,
        timeline: true,
    };
    let mut expected = vec![0, 60];
    expected.extend([0xf4, 0x03]);
    expected.extend(2.5f32.to_be_bytes());
    expected.extend([1, 0b101]);
    assert_eq!(Request::Start(wanted).encode(), expected);
    assert_eq!(Request::decode(&expected).unwrap(), Request::Start(wanted));
    assert_eq!(Request::Stop.encode(), [1]);
    assert_eq!(Request::decode(&[1]).unwrap(), Request::Stop);
}

#[test]
fn unset_switches_stay_unset_across_the_wire() {
    let wanted = Wanted {
        seconds: None,
        ..start(1)
    };
    let back = Request::decode(&Request::Start(wanted).encode()).unwrap();
    assert_eq!(back, Request::Start(wanted));
}

#[test]
fn a_request_beyond_the_limits_of_profile_start_is_refused() {
    let with = |seconds: i32, hz: i32, over: f32, threads: u8, flags: u8| {
        let mut w = Writer::new();
        w.u8(0);
        w.varint(seconds);
        w.varint(hz);
        w.f32(over);
        w.u8(threads);
        w.u8(flags);
        w.data
    };
    assert!(Request::decode(&with(600, 10, 1.0, 0, 2)).is_ok());
    for bad in [
        with(601, 0, 0.0, 0, 2),
        with(-1, 0, 0.0, 0, 2),
        with(0, 9, 0.0, 0, 2),
        with(0, 1001, 0.0, 0, 2),
        with(0, 0, -1.0, 0, 2),
        with(0, 0, f32::NAN, 0, 2),
        with(0, 0, f32::INFINITY, 0, 2),
        with(0, 0, 0.0, 2, 2),
        with(0, 0, 0.0, 0, 8),
    ] {
        assert!(Request::decode(&bad).is_err(), "{bad:?}");
    }
    let mut trailing = Request::Stop.encode();
    trailing.push(0);
    assert!(Request::decode(&trailing).is_err());
    assert!(Request::decode(&[]).is_err());
    assert!(Request::decode(&[9]).is_err());
    assert!(Request::decode(&vec![1; MAX_PAYLOAD + 1]).is_err());
}

#[test]
fn replies_round_trip_and_refusals_are_distinct_bytes() {
    let replies = [
        started("reports/profile-20261107-143204Z.hzperf", Some(60)),
        Reply::Started {
            path: "reports/a.hzperf".into(),
            seconds: None,
            samples: false,
        },
        Reply::Finished {
            path: "reports/a.hzperf".into(),
            removed: vec!["reports/old.hzperf".into(), "reports/older.hzperf".into()],
        },
    ];
    for reply in replies {
        assert_eq!(Reply::decode(&reply.encode()).unwrap(), reply);
    }
    let bytes: Vec<Vec<u8>> = [
        Refusal::Denied,
        Refusal::AlreadyRunning,
        Refusal::NotRunning,
        Refusal::Failed,
    ]
    .map(|why| Reply::Refused(why).encode())
    .to_vec();
    assert_eq!(bytes, [[2, 0], [2, 1], [2, 2], [2, 3]]);
    assert_eq!(
        Reply::decode(&[2, 1]).unwrap(),
        Reply::Refused(Refusal::AlreadyRunning)
    );
    assert!(Reply::decode(&[2, 4]).is_err());
    assert!(Reply::decode(&[7]).is_err());
}

#[test]
fn the_server_judges_by_brand_then_level_then_what_runs() {
    let go = Request::Start(start(5));
    let cases = [
        ("vanilla", 4, &go, Held::Nothing, Verdict::Ignore),
        ("", 4, &Request::Stop, Held::Asker, Verdict::Ignore),
        (
            "horizon",
            3,
            &go,
            Held::Nothing,
            Verdict::Refuse(Refusal::Denied),
        ),
        (
            "horizon",
            2,
            &Request::Stop,
            Held::Asker,
            Verdict::Refuse(Refusal::Denied),
        ),
        (
            "horizon",
            0,
            &go,
            Held::Someone,
            Verdict::Refuse(Refusal::Denied),
        ),
        ("horizon", 4, &go, Held::Nothing, Verdict::Accept),
        (
            "horizon",
            4,
            &go,
            Held::Asker,
            Verdict::Refuse(Refusal::AlreadyRunning),
        ),
        (
            "horizon",
            4,
            &go,
            Held::Someone,
            Verdict::Refuse(Refusal::AlreadyRunning),
        ),
        ("horizon", 4, &Request::Stop, Held::Asker, Verdict::Accept),
        (
            "horizon",
            4,
            &Request::Stop,
            Held::Nothing,
            Verdict::Refuse(Refusal::NotRunning),
        ),
        (
            "horizon",
            4,
            &Request::Stop,
            Held::Someone,
            Verdict::Refuse(Refusal::NotRunning),
        ),
    ];
    for (brand, level, request, held, expected) in cases {
        assert_eq!(
            judge(brand, level, request, held),
            expected,
            "{brand:?} level {level} {request:?} {held:?}"
        );
    }
}

#[test]
fn only_a_horizon_brand_may_ask() {
    let now = Instant::now();
    for brand in ["vanilla", "", "horizon-studio", "Horizon"] {
        let mut link = Link::default();
        assert_eq!(link.ask(brand, now), Err(Unsent::NotHorizon), "{brand:?}");
        assert!(link.is_idle());
    }
    let mut link = Link::default();
    assert_eq!(link.ask("horizon", now), Ok(()));
    assert_eq!(link.ask("horizon", now), Err(Unsent::Busy));
}

#[test]
fn a_capture_goes_from_asked_to_running_to_finished() {
    let t0 = Instant::now();
    let mut link = Link::default();
    link.ask("horizon", t0).unwrap();
    assert!(!link.running());
    assert_eq!(link.stop(t0), Err(Unsent::NothingToStop));
    assert_eq!(
        link.answer(started("reports/p.hzperf", None), t0),
        Outcome::Started {
            path: "reports/p.hzperf".into(),
            seconds: None,
            samples: true
        }
    );
    assert!(link.running());
    assert_eq!(link.ask("horizon", t0), Err(Unsent::Busy));
    assert_eq!(
        link.answer(started("reports/p.hzperf", None), t0),
        Outcome::Stray
    );
    assert!(link.running(), "a stray reply leaves the link as it was");
    link.stop(t0).unwrap();
    assert_eq!(link.stop(t0), Err(Unsent::NothingToStop));
    assert!(link.running());
    assert!(matches!(
        link.answer(finished("reports/p.hzperf"), t0),
        Outcome::Finished { .. }
    ));
    assert!(link.is_idle());
}

#[test]
fn a_refusal_frees_the_link_and_a_window_end_finishes_a_running_capture() {
    let t0 = Instant::now();
    let mut link = Link::default();
    link.ask("horizon", t0).unwrap();
    assert_eq!(
        link.answer(Reply::Refused(Refusal::Denied), t0),
        Outcome::Refused(Refusal::Denied)
    );
    assert!(link.is_idle());
    link.ask("horizon", t0).unwrap();
    link.answer(started("p", Some(5)), t0);
    assert!(matches!(
        link.answer(finished("p"), t0),
        Outcome::Finished { .. }
    ));
    assert!(link.is_idle());
}

#[test]
fn an_unanswered_start_times_out_but_a_running_capture_does_not() {
    let t0 = Instant::now();
    let mut link = Link::default();
    link.ask("horizon", t0).unwrap();
    assert!(!link.timed_out(t0 + ANSWER_TIMEOUT));
    assert!(link.timed_out(t0 + ANSWER_TIMEOUT + Duration::from_millis(1)));
    assert!(link.is_idle());
    link.ask("horizon", t0).unwrap();
    link.answer(started("p", None), t0);
    assert!(!link.timed_out(t0 + ANSWER_TIMEOUT * 100));
    assert!(link.running());
}

#[test]
fn wanted_carries_every_switch_a_server_has_and_never_a_file() {
    let options = StartOptions {
        seconds: Some(9),
        hz: Some(250),
        over_ms: Some(40.0),
        threads: Threads::All,
        wall: true,
        samples: false,
        timeline: true,
        file: Some("/etc/passwd".into()),
        ..StartOptions::default()
    };
    let back = Wanted::from(&options).options();
    assert_eq!(
        back,
        StartOptions {
            file: None,
            ..options
        }
    );
}
