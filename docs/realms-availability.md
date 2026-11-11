# Realms availability: may this client use Realms?

Read this before a screen shows anything of Realms: the Realms screen, the title screen's Realms icons
(`RealmsNotificationsScreen`), a button, a badge. Vanilla asks once whether the client may use Realms at all
(`RealmsAvailability`), and its two consumers wait for the answer before their own requests. Horizon has the same check in
`crates/horizon-client/src/realms/availability.rs`; this page is how to use it.

Vanilla has two consumers. `RealmsMainScreen` (the Realms screen) asks when it is created, shows its list as loading
and makes none of its data requests until the answer comes; a success starts them, any other answer replaces the
screen with the refusal screen the result picks (client outdated, parental consent, or the generic error screen).
`RealmsNotificationsScreen` (the Realms icons on the title screen) asks when it is created and draws its icons only
once the answer is a success.

## The check

The check makes two requests on a worker thread, in this order: `GET mco/client/compatible` (`RealmsClient::client_compatible`),
then `GET mco/available` (`RealmsClient::has_parental_consent`). The second is skipped when the first says the client
isn't compatible, or fails.

## The results

`Availability` has five values:

| Value | Meaning | Cause |
| --- | --- | --- |
| `Success` | Realms may be used | compatible, and parental consent given |
| `IncompatibleClient` | this version can't use the service | `client_compatible` answered `Outdated` or `Other` |
| `NeedsParentalConsent` | the account hasn't agreed yet | `has_parental_consent` answered no |
| `AuthenticationError` | the service refused the account | a request failed with an error whose `code()` is 401 (`RealmsError::Authentication`, or a service error whose body carries code 401, whatever the HTTP status), or there is no session |
| `UnexpectedError(RealmsError)` | anything else went wrong | any other `RealmsError`: no answer, a 5xx, a retry request, an unknown compatibility word |

`Availability::is_success()` is vanilla's `result.type() == SUCCESS`. A check whose request failed is logged (`Couldn't connect to realms`).

## Using it

A screen that only needs yes or no, such as the title screen's icons:

1. When the screen is created, call `crate::realms::availability::current()` once.
2. Keep the `Pending` it returns in the screen.
3. Every frame or tick, read `pending.valid_client()` (or `pending.now()` for the full `Availability`).
4. Treat `None` as "not yet": the check is still running. Vanilla's `getNow(false)` is `.unwrap_or(false)`.
5. Never call `current()` or `get()` per frame.

```rust
struct Notifications {
    availability: Pending,
}

impl Notifications {
    fn new() -> Self {
        Self { availability: availability::current() }
    }
    /// `validClient`, read every tick.
    fn valid_client(&self) -> bool {
        self.availability.valid_client().unwrap_or(false)
    }
}
```

Why not `get()` per frame: after an `UnexpectedError` the next `get()` starts a new check (see Caching). A screen that
asked every frame would send a new pair of requests every frame for as long as the service is down. `Pending` is cheap to
clone and shares its answer with its clones, so pass it around instead of asking again.

A `Pending` never changes once it has answered. A screen that stays open does not see a retry: to try again it
must call `current()` again, as vanilla's `RealmsMainScreen` does each time it opens.

## What it guarantees

- Nothing blocks. `current()`, `get()`, `now()` and `valid_client()` return at once. `get()` takes a short lock and, when
  it starts a check, spawns the thread named `Realms`; only that thread makes the requests. `check()` blocks: it is
  the worker's body, visible inside the crate only, and tests call it directly.
- Every check answers. A service that panics on the worker ends the check in `UnexpectedError` (the panic's text as
  a `RealmsError::Connectivity`), so the next `get()` checks again. Vanilla's future would never complete there.
- Every call is safe from any thread. `Pending` is `Clone` and `Send`.
- Consumers who ask while a check runs share that one check and its single pair of requests. The two vanilla
  consumers opening together cost two requests, not four.

## Caching

The first answer is kept for the rest of the process, except one: a check that ended in `UnexpectedError` is replaced
by a new one at the next `get()`. `Success`, `IncompatibleClient`, `NeedsParentalConsent` and `AuthenticationError` are
kept. This is vanilla's `shouldRefresh` rule. The screen that saw the error keeps its old `Pending` and its error.

## Where Horizon differs from vanilla

- Vanilla's account can't change while the game runs, so its one future has no key. Horizon's session can be missing,
  expire or be replaced, so the cache holds one entry, keyed on the account: the session's UUID and access token. A
  `get()` for another UUID, or the same account with a new token, starts a check of its own and never sees the previous
  answer. A check still running for the old session finishes into the old `Pending` only.
- Without a session, `get(None)` answers `AuthenticationError` at once, with no request and no thread. `current()` uses
  the launcher's session while its token is good, so an expired token gives the same answer.

## Testing a consumer

`Pending::ready(Availability::…)` is a check that has already answered, with no thread and no network. A screen that
takes a `Pending` is tested by giving it one:

```rust
let screen = Notifications { availability: Pending::ready(Availability::Success) };
assert!(screen.valid_client());
```

`Pending::default()` is a check that never answers, for the "not yet" case. In the crate's tests, `resolve(..)` gives
it its answer later, as the worker would, for the "not yet, then the answer" case.

To test the check itself, make an `AvailabilityCache::new()` and call `cache.get(&session, || fake)` with a fake
`AvailabilityService` (the trait with the two requests), or, inside the crate, call `check(&fake)` for the result
alone. The tests at the bottom of `availability.rs` do both. No test may reach the real service: never call `current()` or `get(Some(..))`
from a test, since they use `RealmsClient::production`.

## Files

- `crates/horizon-client/src/realms/availability.rs`: `Availability`, `Pending`, `AvailabilityService`, `check`, `AvailabilityCache`, `get`, `current`.
- `crates/horizon-client/src/realms/client.rs`: `RealmsClient` (the two requests), `CompatibleVersion`, `RealmsError` (`code()`, `message()`, `text()`).
