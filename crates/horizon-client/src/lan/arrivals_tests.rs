use super::*;

const A: u128 = 0xA;
const B: u128 = 0xB;

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn an_expected_player_is_taken_once_before_the_deadline() {
    let t0 = Instant::now();
    let mut e = Expectations::default();
    assert_eq!(e.expect(A, secs(10), t0), Ok(false));
    assert!(e.take(A, t0 + secs(9)));
    // The login consumed it: a second login of the same player is ordinary.
    assert!(!e.take(A, t0 + secs(9)));
    assert!(e.is_empty());
}

#[test]
fn an_expectation_is_dead_at_its_deadline() {
    let t0 = Instant::now();
    let mut e = Expectations::default();
    e.expect(A, secs(10), t0).unwrap();
    // Exactly at the deadline is already too late.
    assert!(!e.take(A, t0 + secs(10)));
    assert!(e.is_empty());
}

#[test]
fn a_player_nobody_expected_has_no_expectation() {
    let mut e = Expectations::default();
    assert!(!e.take(B, Instant::now()));
}

#[test]
fn asking_again_moves_the_deadline_and_says_so() {
    let t0 = Instant::now();
    let mut e = Expectations::default();
    e.expect(A, secs(5), t0).unwrap();
    assert_eq!(e.expect(A, secs(30), t0 + secs(4)), Ok(true));
    assert!(e.take(A, t0 + secs(20)));
}

#[test]
fn a_held_player_waits_one_minute_of_ticks() {
    let max = Settings::default().max_hold_ticks;
    assert_eq!(max, 1200);
    assert!(!held_too_long(100, 100 + 1199, max));
    assert!(held_too_long(100, 100 + 1200, max));
    // A clock that went backwards never drops anyone.
    assert!(!held_too_long(500, 100, max));
}

#[test]
fn forgetting_removes_only_that_player() {
    let t0 = Instant::now();
    let mut e = Expectations::default();
    e.expect(A, secs(10), t0).unwrap();
    e.expect(B, secs(10), t0).unwrap();
    assert!(e.forget(A));
    assert!(!e.forget(A));
    assert!(e.take(B, t0));
}

#[test]
fn a_long_wait_is_cut_to_the_maximum() {
    let t0 = Instant::now();
    let mut e = Expectations::default();
    e.expect(A, secs(3600), t0).unwrap();
    assert!(!e.take(A, t0 + Settings::default().max_expect));
}

#[test]
fn expiry_returns_the_players_that_never_came_and_keeps_the_others() {
    let t0 = Instant::now();
    let mut e = Expectations::default();
    e.expect(A, secs(5), t0).unwrap();
    e.expect(B, secs(30), t0).unwrap();
    assert_eq!(e.expire(t0 + secs(6)), vec![A]);
    assert_eq!(e.len(), 1);
    assert!(e.take(B, t0 + secs(7)));
}

#[test]
fn the_table_refuses_new_players_when_full_but_expired_ones_make_room() {
    let t0 = Instant::now();
    let mut e = Expectations::default();
    for n in 0..Settings::default().max_expected as u128 {
        e.expect(100 + n, secs(10), t0).unwrap();
    }
    assert_eq!(e.expect(A, secs(10), t0), Err(ExpectError::Full));
    // A player already in the table can still be renewed when it is full.
    assert_eq!(e.expect(100, secs(10), t0), Ok(true));
    // Once the old ones are past their time, there is room again.
    assert_eq!(e.expect(A, secs(10), t0 + secs(11)), Ok(false));
    assert_eq!(e.len(), 1);
}

#[test]
fn the_settings_are_read_from_the_supervisors_line() {
    assert_eq!(Settings::parse(""), Ok(Settings::default()));
    let s = Settings::parse(
        "expect_seconds=5,max_expect_seconds=20,max_expected=3,max_freeze_seconds=12",
    )
    .unwrap();
    assert_eq!(s.expect, secs(5));
    assert_eq!(s.max_expect, secs(20));
    assert_eq!(s.max_expected, 3);
    assert_eq!(s.max_freeze_ticks, 240);
    // One key alone keeps the others' defaults.
    assert_eq!(
        Settings::parse("max_expected=8").unwrap(),
        Settings {
            max_expected: 8,
            ..Settings::default()
        }
    );
    for bad in [
        "expect_seconds=0",
        "expect_seconds=-1",
        "expect_seconds",
        "colour=blue",
        "expect_seconds=90",
        "stop_wait_seconds=61",
        "max_freeze_seconds=81",
        "stop_wait_seconds=60,max_freeze_seconds=31",
    ] {
        assert!(Settings::parse(bad).is_err(), "{bad:?} was taken");
    }
    // A stop that does not wait is zero seconds.
    assert_eq!(Settings::default().stop_wait, secs(10));
    assert_eq!(
        Settings::parse("stop_wait_seconds=0").unwrap().stop_wait,
        secs(0)
    );
    assert_eq!(
        Settings::parse("stop_wait_seconds=60").unwrap().stop_wait,
        secs(60)
    );
    // The two fit in the stop's limit, or the stop does not wait.
    assert!(Settings::parse("stop_wait_seconds=60,max_freeze_seconds=30").is_ok());
    assert!(Settings::parse("stop_wait_seconds=0,max_freeze_seconds=300").is_ok());
}

/// A stop's wait ends when nobody is left, at its time when no hand-over is frozen, and at its
/// limit (its time and a freeze's) whatever is under way; no freeze starts that the limit would
/// cut. Zero seconds is no wait.
#[test]
fn a_stop_waits_for_the_players_to_be_moved_out_but_not_for_ever() {
    let now = Instant::now();
    let settings = Settings::default();
    let tick = Duration::from_millis(50);
    let wait = StopWait::start(&settings, now, tick).unwrap();
    assert_eq!((wait.until, wait.limit), (now + secs(10), now + secs(40)));
    assert!(!wait.over(now, 2, 0));
    assert!(wait.over(now, 0, 0), "nobody is left");
    assert!(!wait.over(now + secs(9), 2, 0));
    assert!(
        wait.over(now + secs(10), 2, 0),
        "its time, and no hand-over"
    );
    assert!(!wait.over(now + secs(39), 2, 1), "a hand-over may finish");
    assert!(wait.over(now + secs(40), 2, 1), "the limit");
    assert!(wait.may_freeze(now + secs(10)));
    assert!(!wait.may_freeze(now + secs(11)));
    // However long the settings, the whole wait fits in the supervisor's time for a stop.
    let long = Settings {
        stop_wait: secs(60),
        max_freeze_ticks: 20 * 300,
        ..Settings::default()
    };
    let wait = StopWait::start(&long, now, tick).unwrap();
    assert_eq!(wait.limit, now + STOP_WAIT_LIMIT);
    assert!(!wait.may_freeze(now), "a freeze of 300 s would be cut");
    let none = Settings {
        stop_wait: secs(0),
        ..Settings::default()
    };
    assert_eq!(StopWait::start(&none, now, tick), None);
}

/// A freeze lasts ticks: at 10 ticks a second it lasts twice as long, the limit with it, and a
/// server that slows down during the wait refuses a freeze it would have let start at 20.
#[test]
fn a_stop_reckons_a_freeze_at_the_tick_length_of_now() {
    let now = Instant::now();
    let slow = Duration::from_millis(100);
    let wait = StopWait::start(&Settings::default(), now, slow).unwrap();
    assert_eq!((wait.until, wait.limit), (now + secs(10), now + secs(70)));
    assert!(wait.may_freeze(now + secs(10)));
    assert!(!wait.may_freeze(now + secs(11)));
    let mut wait = StopWait::start(&Settings::default(), now, Duration::from_millis(50)).unwrap();
    assert!(wait.may_freeze(now));
    wait.period = slow;
    assert!(
        !wait.may_freeze(now),
        "60 s of freeze would pass the limit at 40 s"
    );
    // The tick length: the rate's, or longer when the server is slower than its rate.
    let rate = Duration::from_millis(50);
    assert_eq!(tick_period(rate, 20.0), rate);
    assert_eq!(
        tick_period(rate, 40.0),
        rate,
        "sprinting is not a shorter freeze"
    );
    assert_eq!(tick_period(rate, 10.0), Duration::from_millis(100));
    assert_eq!(tick_period(rate, 0.0), rate, "no measure yet");
    assert_eq!(
        tick_period(Duration::from_millis(100), 20.0),
        slow,
        "/tick rate 10"
    );
}

#[test]
fn the_table_follows_the_settings() {
    let t0 = Instant::now();
    let mut e = Expectations::new(2, secs(5));
    e.expect(A, secs(60), t0).unwrap();
    assert!(!e.take(A, t0 + secs(5)), "cut to five seconds");
    e.expect(A, secs(1), t0).unwrap();
    e.expect(B, secs(1), t0).unwrap();
    assert_eq!(e.expect(3, secs(1), t0), Err(ExpectError::Full));
}

/// A login that does not reach play gives its expectation back, with its
/// own deadline, so the proxy's next attempt is still expected.
#[test]
fn an_expectation_a_failed_login_used_is_put_back_with_its_deadline() {
    let t0 = Instant::now();
    let mut e = Expectations::default();
    e.expect(A, secs(10), t0).unwrap();
    let deadline = e.take_until(A, t0 + secs(2)).unwrap();
    assert!(e.is_empty());
    e.put_back(A, deadline, t0 + secs(4));
    assert!(e.take(A, t0 + secs(9)), "the same deadline, not a new one");
    // Not once its time has passed.
    e.put_back(A, deadline, t0 + secs(10));
    assert!(e.is_empty());
    // Not over a newer expectation of the same player.
    e.expect(A, secs(30), t0 + secs(5)).unwrap();
    e.put_back(A, deadline, t0 + secs(6));
    assert!(e.take(A, t0 + secs(20)), "the newer deadline stands");
}

#[test]
fn a_hand_over_id_is_32_hexadecimal_digits() {
    let id = 0x0123_4567_89ab_cdef_0011_2233_4455_6677;
    assert_eq!(handover_text(id), "0123456789abcdef0011223344556677");
    assert_eq!(parse_handover(&handover_text(id)), Some(id));
    assert_eq!(parse_handover("0123456789ABCDEF0011223344556677"), Some(id));
    assert_eq!(parse_handover("0123456789abcdef001122334455667"), None);
    assert_eq!(parse_handover("01234567-89ab-cdef-0011-223344556677"), None);
    assert_ne!(new_handover_id().unwrap(), new_handover_id().unwrap());
}
