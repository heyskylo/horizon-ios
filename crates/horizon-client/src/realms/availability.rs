//! `RealmsAvailability`: may this client use Realms at all? Asked once before anything else
//! Realms shows: `RealmsMainScreen` waits for it before its data requests, and the title
//! screen's `RealmsNotificationsScreen` shows its icons only once it says yes (`validClient`).
//!
//! The check (`RealmsAvailability.check`) is `GET mco/client/compatible`, then, for a
//! compatible client, `GET mco/available` (`hasParentalConsent`), on a worker thread. A
//! service error whose code is 401 is [`Availability::AuthenticationError`]; any other is
//! [`Availability::UnexpectedError`].
//!
//! # Sharing one check
//!
//! [`get`] is `RealmsAvailability.get()`: it hands out a [`Pending`], the check's future.
//! Every caller shares the check in flight and, once it answered, its result for the rest of
//! the process, with one exception: a check that ended in [`Availability::UnexpectedError`]
//! is replaced by a new one at the next [`get`] (`shouldRefresh`). An incompatible client,
//! a missing parental consent and an authentication error are kept.
//!
//! A consumer calls [`get`] (or [`current`]) once when its screen is created, as vanilla's
//! screens do in their field initialisers, keeps the [`Pending`], and asks it every frame or
//! tick with [`Pending::now`] or [`Pending::valid_client`]. Neither call blocks or touches
//! the network; [`get`] only takes a short lock and, when it has to, starts the worker. All
//! of them may be called from any thread. Calling [`get`] every frame instead would start a
//! new check every frame after an unexpected error.
//!
//! # Accounts (Horizon differs from vanilla here)
//!
//! Vanilla's account can't change while the game runs, so its single future needs no key.
//! Horizon's session may be missing, may expire, and could be replaced, so the cache is keyed
//! on the account it was asked for: the session's UUID and access token. A [`get`] for any
//! other session starts a check of its own and never sees the previous result; one already in
//! flight for the old session finishes into the old [`Pending`] only.
//!
//! Without a session there is nothing to ask: [`get`] answers
//! [`Availability::AuthenticationError`] at once, without a request, as vanilla does in its
//! offline developer mode. (A consumer may treat a missing session in its own way before it
//! asks: the Realms screen keeps Horizon's signed-out message there rather than vanilla's
//! "Invalid Session" screen.)
//!
//! A worker whose service panics still answers, with [`Availability::UnexpectedError`], so the
//! next [`get`] checks again. Vanilla's future would never complete there, and its screens
//! would wait for good.
use super::client::{CompatibleVersion, RealmsClient, RealmsError};
use horizon_client_render::auth::AuthSession;
use std::sync::{Arc, Mutex, OnceLock};

/// `RealmsAvailability.Result`: its `Type`, the unexpected error carrying the service's
/// exception.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Success,
    IncompatibleClient,
    NeedsParentalConsent,
    AuthenticationError,
    UnexpectedError(RealmsError),
}

impl Availability {
    /// `result.type() == SUCCESS`: what `RealmsNotificationsScreen.validClient` holds.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }
}

/// The two requests the check makes: [`RealmsClient`] in the game, a fake in tests.
pub trait AvailabilityService {
    /// `RealmsClient.clientCompatible`.
    fn client_compatible(&self) -> Result<CompatibleVersion, RealmsError>;
    /// `RealmsClient.hasParentalConsent`.
    fn has_parental_consent(&self) -> Result<bool, RealmsError>;
}

impl AvailabilityService for RealmsClient {
    fn client_compatible(&self) -> Result<CompatibleVersion, RealmsError> {
        RealmsClient::client_compatible(self)
    }
    fn has_parental_consent(&self) -> Result<bool, RealmsError> {
        RealmsClient::has_parental_consent(self)
    }
}

/// `RealmsAvailability.check`'s worker body, run on the calling thread: it blocks on the
/// requests. [`AvailabilityCache::get`] runs it on its worker; elsewhere only tests call it.
pub(crate) fn check(service: &impl AvailabilityService) -> Availability {
    let answer = service.client_compatible().and_then(|version| {
        if version != CompatibleVersion::Compatible {
            return Ok(Availability::IncompatibleClient);
        }
        Ok(match service.has_parental_consent()? {
            true => Availability::Success,
            false => Availability::NeedsParentalConsent,
        })
    });
    answer.unwrap_or_else(|error| {
        log::error!("Couldn't connect to realms: {error:?}");
        match error.code() {
            401 => Availability::AuthenticationError,
            _ => Availability::UnexpectedError(error),
        }
    })
}

/// The check's `CompletableFuture`: empty until the worker answers, then the answer for
/// good. Clones share it.
#[derive(Clone, Debug, Default)]
pub struct Pending(Arc<OnceLock<Availability>>);

impl Pending {
    /// A check that already answered `availability`: what [`get`] hands out without a
    /// session, and what a consumer's test gives its screen instead of a real check.
    pub fn ready(availability: Availability) -> Self {
        let pending = Self::default();
        let _ = pending.0.set(availability);
        pending
    }
    /// `CompletableFuture.getNow(null)`: the answer, or `None` while the check runs.
    pub fn now(&self) -> Option<Availability> {
        self.0.get().cloned()
    }
    /// `RealmsNotificationsScreen.validClient`: `Some(true)` once the check said
    /// [`Availability::Success`], `Some(false)` once it said anything else, `None` while it
    /// runs (vanilla's `getNow(false)` is `.unwrap_or(false)`).
    pub fn valid_client(&self) -> Option<bool> {
        self.0.get().map(Availability::is_success)
    }
    /// `shouldRefresh`: the check is over and ended in an unexpected error.
    fn should_refresh(&self) -> bool {
        matches!(self.0.get(), Some(Availability::UnexpectedError(_)))
    }
    /// For tests only: the worker's answer, given by hand to a check made with
    /// `Pending::default()`, so a consumer's test sees "not yet" and then the answer.
    #[cfg(test)]
    pub fn resolve(&self, availability: Availability) {
        let _ = self.0.set(availability);
    }
}

/// `RealmsAvailability.future`, for one account at a time.
#[derive(Default)]
pub struct AvailabilityCache {
    current: Mutex<Option<(Arc<AuthSession>, Pending)>>,
}

impl AvailabilityCache {
    pub const fn new() -> Self {
        Self {
            current: Mutex::new(None),
        }
    }

    /// `RealmsAvailability.get()` for `session`: the check already started for this account
    /// unless it ended in an unexpected error, else a new check on a worker thread, asking
    /// the service `service` makes (called only then).
    pub fn get<S: AvailabilityService + Send + 'static>(
        &self,
        session: &Arc<AuthSession>,
        service: impl FnOnce() -> S,
    ) -> Pending {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((owner, pending)) = current.as_ref()
            && owner.uuid == session.uuid
            && owner.access_token() == session.access_token()
            && !pending.should_refresh()
        {
            return pending.clone();
        }
        let pending = Pending::default();
        let service = service();
        let out = pending.clone();
        let started = std::thread::Builder::new()
            .name("Realms".into())
            .spawn(move || {
                // A panicking service answers too (the module docs), with its panic's text.
                let answer =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&service)));
                let _ = out.0.set(answer.unwrap_or_else(|panic| {
                    let text = panic.downcast_ref::<&str>().map(|s| s.to_string());
                    no_answer(text.or_else(|| panic.downcast_ref::<String>().cloned()))
                }));
            });
        if let Err(e) = started {
            let _ = pending.0.set(no_answer(Some(e.to_string())));
        }
        *current = Some((Arc::clone(session), pending.clone()));
        pending
    }
}

/// A check that got no answer, like a request that got none (the reason, when there is one):
/// an unexpected error, so the next `get` tries again.
fn no_answer(reason: Option<String>) -> Availability {
    Availability::UnexpectedError(RealmsError::Connectivity(reason.unwrap_or_default()))
}

/// The process's one cache (vanilla's static field).
static SHARED: AvailabilityCache = AvailabilityCache::new();

/// `RealmsAvailability.get()` for `session` against the production service. Without a
/// session: [`Availability::AuthenticationError`], already answered, nothing asked.
pub fn get(session: Option<&Arc<AuthSession>>) -> Pending {
    match session {
        Some(session) => SHARED.get(session, || RealmsClient::production(session)),
        None => Pending::ready(Availability::AuthenticationError),
    }
}

/// [`get`] for the account Realms may use now (the launcher's session while its token is
/// good, as every Realms request uses).
pub fn current() -> Pending {
    get(super::session().as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// The two answers, how often each request was made, and an optional hold: the first
    /// request waits for a message on it.
    struct Fake {
        compatible: Result<CompatibleVersion, RealmsError>,
        consent: Result<bool, RealmsError>,
        calls: Arc<[AtomicUsize; 2]>,
        hold: Option<Mutex<mpsc::Receiver<()>>>,
    }

    impl Fake {
        fn new(
            compatible: Result<CompatibleVersion, RealmsError>,
            consent: Result<bool, RealmsError>,
        ) -> Self {
            Self {
                compatible,
                consent,
                calls: Arc::default(),
                hold: None,
            }
        }
    }

    impl AvailabilityService for Fake {
        fn client_compatible(&self) -> Result<CompatibleVersion, RealmsError> {
            if let Some(hold) = &self.hold {
                let _ = hold.lock().unwrap().recv();
            }
            self.calls[0].fetch_add(1, Ordering::SeqCst);
            self.compatible.clone()
        }
        fn has_parental_consent(&self) -> Result<bool, RealmsError> {
            self.calls[1].fetch_add(1, Ordering::SeqCst);
            self.consent.clone()
        }
    }

    fn session(uuid: u128, token: &str) -> Arc<AuthSession> {
        Arc::new(AuthSession::new("Steve", uuid, token))
    }

    fn wait(pending: &Pending) -> Availability {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(answer) = pending.now() {
                return answer;
            }
            assert!(Instant::now() < deadline, "the check never answered");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn service_error(status: u16, code: Option<i32>) -> RealmsError {
        RealmsError::Service {
            status,
            code,
            reason: None,
            message: None,
        }
    }

    #[test]
    fn the_check_asks_compatibility_then_consent() {
        use CompatibleVersion::*;
        let cases = [
            (Ok(Compatible), Ok(true), Availability::Success, 1),
            (
                Ok(Compatible),
                Ok(false),
                Availability::NeedsParentalConsent,
                1,
            ),
            // An incompatible client is never asked for consent.
            (Ok(Outdated), Ok(true), Availability::IncompatibleClient, 0),
            (Ok(Other), Ok(true), Availability::IncompatibleClient, 0),
        ];
        for (compatible, consent, expected, consent_calls) in cases {
            let fake = Fake::new(compatible, consent);
            assert_eq!(check(&fake), expected);
            assert_eq!(fake.calls[0].load(Ordering::SeqCst), 1);
            assert_eq!(
                fake.calls[1].load(Ordering::SeqCst),
                consent_calls,
                "{expected:?}"
            );
        }
    }

    #[test]
    fn an_error_coded_401_is_an_authentication_error_and_others_are_unexpected() {
        let auth = RealmsError::Authentication("Bearer".into());
        // `ErrorWithJsonPayload.errorCode`: the body's code, whatever the status.
        let coded = service_error(403, Some(401));
        for error in [auth.clone(), coded] {
            let fake = Fake::new(Err(error.clone()), Ok(true));
            assert_eq!(check(&fake), Availability::AuthenticationError, "{error:?}");
            let fake = Fake::new(Ok(CompatibleVersion::Compatible), Err(error));
            assert_eq!(check(&fake), Availability::AuthenticationError);
        }
        for error in [
            RealmsError::Connectivity("refused".into()),
            RealmsError::UnknownCompatibility("MAYBE".into()),
            RealmsError::Retry(5),
            service_error(500, None),
            service_error(403, Some(6002)),
        ] {
            let fake = Fake::new(Err(error.clone()), Ok(true));
            assert_eq!(
                check(&fake),
                Availability::UnexpectedError(error.clone()),
                "{error:?}"
            );
            let fake = Fake::new(Ok(CompatibleVersion::Compatible), Err(error.clone()));
            assert_eq!(check(&fake), Availability::UnexpectedError(error));
        }
    }

    #[test]
    fn two_consumers_share_one_check() {
        let cache = AvailabilityCache::new();
        let steve = session(1, "token");
        let (release, hold) = mpsc::channel();
        let mut fake = Fake::new(Ok(CompatibleVersion::Compatible), Ok(true));
        fake.hold = Some(Mutex::new(hold));
        let calls = Arc::clone(&fake.calls);
        let started = AtomicUsize::new(0);
        let mut fake = Some(fake);
        let mut make = || {
            started.fetch_add(1, Ordering::SeqCst);
            fake.take().expect("a second check was started")
        };
        let main_screen = cache.get(&steve, &mut make);
        let title = cache.get(&steve, &mut make);
        // Neither blocks on the held request: both see "not yet".
        assert_eq!(main_screen.now(), None);
        assert_eq!(title.valid_client(), None);
        release.send(()).unwrap();
        assert_eq!(wait(&main_screen), Availability::Success);
        assert_eq!(title.valid_client(), Some(true));
        assert_eq!(started.load(Ordering::SeqCst), 1);
        assert_eq!(calls[0].load(Ordering::SeqCst), 1);
        assert_eq!(calls[1].load(Ordering::SeqCst), 1);
    }

    #[test]
    fn only_an_unexpected_error_is_checked_again() {
        use CompatibleVersion::*;
        let kept = [
            (Ok(Compatible), Ok(true), Availability::Success),
            (Ok(Outdated), Ok(true), Availability::IncompatibleClient),
            (
                Ok(Compatible),
                Ok(false),
                Availability::NeedsParentalConsent,
            ),
            (
                Err(RealmsError::Authentication(String::new())),
                Ok(true),
                Availability::AuthenticationError,
            ),
        ];
        for (compatible, consent, expected) in kept {
            let cache = AvailabilityCache::new();
            let steve = session(1, "token");
            let first = cache.get(&steve, || Fake::new(compatible, consent));
            assert_eq!(wait(&first), expected);
            let again = cache.get(&steve, || -> Fake {
                panic!("{expected:?} was asked again")
            });
            assert_eq!(again.now(), Some(expected));
        }

        let cache = AvailabilityCache::new();
        let steve = session(1, "token");
        let refused = RealmsError::Connectivity("refused".into());
        let first = cache.get(&steve, || Fake::new(Err(refused.clone()), Ok(true)));
        assert_eq!(wait(&first), Availability::UnexpectedError(refused.clone()));
        // The screen that saw the error keeps it; the next `get` asks the service again.
        let again = cache.get(&steve, || Fake::new(Ok(Compatible), Ok(true)));
        assert_eq!(wait(&again), Availability::Success);
        assert_eq!(first.now(), Some(Availability::UnexpectedError(refused)));
        let third = cache.get(&steve, || -> Fake { panic!("a success was asked again") });
        assert_eq!(third.now(), Some(Availability::Success));
    }

    #[test]
    fn another_account_never_gets_the_previous_accounts_answer() {
        use CompatibleVersion::*;
        let cache = AvailabilityCache::new();
        let steve = cache.get(&session(1, "steve"), || {
            Fake::new(Ok(Compatible), Ok(false))
        });
        assert_eq!(wait(&steve), Availability::NeedsParentalConsent);
        let alex = cache.get(&session(2, "alex"), || Fake::new(Ok(Compatible), Ok(true)));
        assert_eq!(wait(&alex), Availability::Success);
        // The same account with a new token is checked again too.
        let renewed = cache.get(&session(2, "alex-renewed"), || {
            Fake::new(Err(RealmsError::Authentication(String::new())), Ok(true))
        });
        assert_eq!(wait(&renewed), Availability::AuthenticationError);
        assert_eq!(steve.now(), Some(Availability::NeedsParentalConsent));
    }

    #[test]
    fn a_new_account_never_sees_the_check_still_running_for_the_old_one() {
        use CompatibleVersion::*;
        let cache = AvailabilityCache::new();
        let (release, hold) = mpsc::channel();
        let mut held = Fake::new(Ok(Compatible), Ok(false));
        held.hold = Some(Mutex::new(hold));
        let steve = cache.get(&session(1, "steve"), || held);
        let alex_session = session(2, "alex");
        let alex = cache.get(&alex_session, || Fake::new(Ok(Compatible), Ok(true)));
        assert_eq!(wait(&alex), Availability::Success);
        assert_eq!(steve.now(), None);
        // The old check answers after the switch: into its own `Pending` only.
        release.send(()).unwrap();
        assert_eq!(wait(&steve), Availability::NeedsParentalConsent);
        assert_eq!(alex.now(), Some(Availability::Success));
        let again = cache.get(&alex_session, || -> Fake {
            panic!("alex's answer was replaced")
        });
        assert_eq!(again.now(), Some(Availability::Success));
    }

    /// A service that panics on its first request.
    struct Panics;

    impl AvailabilityService for Panics {
        fn client_compatible(&self) -> Result<CompatibleVersion, RealmsError> {
            panic!("the service broke")
        }
        fn has_parental_consent(&self) -> Result<bool, RealmsError> {
            unreachable!("never asked after a panic")
        }
    }

    #[test]
    fn a_panicking_service_answers_an_unexpected_error_and_is_checked_again() {
        let cache = AvailabilityCache::new();
        let steve = session(1, "token");
        let first = cache.get(&steve, || Panics);
        assert_eq!(
            wait(&first),
            Availability::UnexpectedError(RealmsError::Connectivity("the service broke".into()))
        );
        let again = cache.get(&steve, || {
            Fake::new(Ok(CompatibleVersion::Compatible), Ok(true))
        });
        assert_eq!(wait(&again), Availability::Success);
    }

    #[test]
    fn without_a_session_the_answer_is_an_authentication_error_at_once() {
        let pending = get(None);
        assert_eq!(pending.now(), Some(Availability::AuthenticationError));
        assert_eq!(pending.valid_client(), Some(false));
    }
}
