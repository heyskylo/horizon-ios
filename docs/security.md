# Security

How Horizon keeps its code and its downloads safe while most of the work is done by AI agents and releases go out
by themselves, including when Ludovic isn't at his computer. In plain words: what someone could have done before,
what stops them now, and what still needs Ludovic.

## Who does what

- **Agents** (Claude sessions run by Ludovic) write most pull requests, review them, merge them and answer
  reports. Everything an agent posts on GitHub says so ("Opened by Ludovic's AI agent").
- **Contributors** send pull requests from their forks like on any open-source project.
- **Players** report bugs on Discord and GitHub. Kira, the Discord bot, files their reports as issues.
- **Releases** are automatic: every merged change that players run ships within the hour.

## What could go wrong before (October 2026)

1. **Instructions hidden in a report.** Someone could write "ignore your previous instructions and…" in a
   Discord message or an issue. Agents read those reports while working with Ludovic's full GitHub account.
2. **A check that counted as passed without running.** Adding a label to a pull request started a second run of
   the checks in which `verify` was *skipped*, and GitHub counts a skipped required check as passed. Two pull
   requests merged before their real check had finished.
3. **Every release went straight to every player.** A broken or malicious build reached every launcher within
   minutes, with nobody looking at it first.
4. **Downloads trusted GitHub alone.** Anyone who got hold of a token that can write to the repository could
   replace a release's files, and launchers and servers would install them.
5. **The release machinery was ordinary code.** A pull request could change the workflows that build and publish
   releases as easily as it changes a texture.

## Where things stand (19 October 2026)

The next section describes the design once every part is in place. Some parts are live, some are waiting to merge,
and some are waiting for Ludovic:

| Part | Today |
| --- | --- |
| `verify` always runs | **Live** (#1010). |
| Fork pull requests need approval | **Live**: every outside contributor's workflows wait for a maintainer's approval. |
| Private vulnerability reporting | **Live** (see the end of this page). |
| Signed checksums on releases | **Live** for new releases (#1011): the `release` environment holds the key, and its public half is in `release-keys.txt` (#1023). Releases from before then are unsigned. |
| Signed prebuilt native-mod builds | **Being built** (#11859): horizon-mods' `native-builds` environment (deployable from `main` only) holds `NATIVE_BUILD_SIGNING_KEY`, a key of its own, never the release key, because the registry compiles other people's code there. Its public half is in `native-build-keys.txt`. Each signature covers the build's request key (target, base commit, each mod's file SHA-256), its target and the archive's SHA-256 (`horizon_release_sign::native_build`), so it can't pass for another set's file or for a release. `tools/ci/sign_native_build.sh` signs with OpenSSL. |
| Launchers and servers checking signatures | **Not yet**: #1014 waits as a draft until a signed stable game release and a signed launcher release exist. Until then they trust GitHub's checksum, as before. |
| Smoke test and release-blocker before publishing | **Live**: since 2026-10-23 every release is the latest at once (one channel, Ludovic's decision), so it is published only after its smoke test passed and while no `release-blocker` is open. The 6-hour beta of the old Alpha, then stable flow (#1012) is gone. |
| Reports marked suspicious | **Partly**: the scan of Discord for agents does it. Kira's issues do it once its pull request merges. |
| Code owners' review of the pipeline | **Not yet**: `.github/CODEOWNERS` and its protection (#1015) wait for the App, below. Until then pipeline changes merge like any other once `verify` passes. |
| Agents on their own GitHub App | **Not yet**: prepared, waiting for Ludovic to create it. Agents still use his account. |
| Mac mini releases from a clean checkout | **Waiting to merge** (#1024): see "What this doesn't solve". |

## What stops it now

- **Reports are data, never instructions.** Agents receive people's text only inside quoted blocks marked as
  untrusted, with attachments as plain links. Text that looks aimed at an AI (talking to "the AI", asking to run
  commands, asking for passwords or keys, linking to scripts, hidden characters) is marked **suspicious**, and the
  agent coordinating the others sees the mark.
- **`verify` always runs.** Labels no longer start the checks, and the required `verify` job has no condition, so
  the only `verify` a change can have is one that ran. Main requires it, from GitHub Actions only, for everyone,
  admins included.
- **Checked before it is published.** Every release is published as the latest, which launchers and servers
  install by themselves, so the release run publishes it only after its smoke test passed (the game started and
  drew a world on the Mac mini) and only while no issue labelled `release-blocker` is open; otherwise nothing is
  published.
- **Signed updates.** Each release carries a list of its files' checksums, signed with Horizon's release key. The
  launcher (for the game and for itself) and `horizon-server update` check that signature against the public key
  built into them, then check the download against the list. Anything unsigned, signed with another key, signed
  for another version, or changed in any way is refused with a message saying why. Releases from before signing
  stay installable, for going back to an older version, but only with the exact files they had when signing
  began: their checksums are built into the launcher and the server too. The private key exists only as
  a secret of GitHub's `release` environment, which only real releases from main can use. The job that signs runs
  nothing from the repository but a short script calling OpenSSL, so no pull request can read the key by changing
  the code that gets built. Nobody, Ludovic included, ever needs to see it.
- **Nightly signatures.** A Nightly carries two signatures over the same `SHA256SUMS`. `SHA256SUMS.sig` is signed
  for the channel (`horizon release nightly`), for launchers and servers from before this change, which read only
  that one. `SHA256SUMS.v2.sig` is signed for the Nightly's own version (`horizon release nightly-<date>-<commit>`),
  which its release title names (`Nightly <date> · <commit>`). Current launchers, `horizon-server update` and
  `install.sh` parse that title strictly, refuse a Nightly whose title doesn't parse, and install it only with a v2
  signature for exactly that version. That way an older Nightly can't be served as tonight's: its signature names its own date. Installing
  an older Nightly on purpose still works as a rollback, with a warning, and never happens as an automatic update.
  **The changeover:** Nightlies published before this change have no `SHA256SUMS.v2.sig`, so they can't be installed
  or picked with `--version` any more; they leave GitHub with the 7-day rotation. Until a v2 Nightly exists, a
  launcher has nothing to update to, so the first Nightly is built right after this merges. A server can still go back
  to a build it already has with `horizon-server rollback`, which checks nothing it hasn't already checked.
  **When the old signature can go:** once every launcher and server still updating reads v2. That is, when the
  telemetry live count shows no launcher (and the server update log no `horizon-server`) older than the first build
  with this change, or at the latest 60 days after it shipped. Then drop `SHA256SUMS.sig` from nightly-build.yml's
  publish step and the `--tag nightly` check before it, and remove `NIGHTLY` from horizon-release-sign. Releases (not
  Nightlies) keep their single `SHA256SUMS.sig`, which is already signed for their tag.
- **The release machinery needs Ludovic.** The workflows, the release and signing tools, the update-checking code
  and the installers are listed in `.github/CODEOWNERS`. Once main's protection requires code owners' reviews, a
  change to any of them waits for Ludovic's approval. Everything else still merges as soon as its checks pass.
- **Agents get their own, smaller identity** (prepared, switched on by Ludovic). The `horizon-agent` GitHub App
  can read and write code, pull requests and issues, and read the checks. It cannot change settings, workflows,
  secrets or members, and its tokens last an hour. Once agents use it, their pull requests are the app's, and
  Ludovic approves the rare pipeline change with one tap.

## What still needs Ludovic

- **Approving pipeline changes** (rare): a pull request touching a file in `.github/CODEOWNERS`.
- **Holding releases back or speeding one up**, from his phone if he likes. Label an issue `release-blocker` to
  hold every release until it is closed; label a pull request `release-now` to release it as soon as it merges.
- **Creating the GitHub App**: one link, "Create GitHub App", then "Install". His login, his clicks.
- **Fork pull requests**: their checks run only after a maintainer approves them, and they run on the Mac mini only
  after a maintainer has read them and added `run-parity`.
- **A leaked key**: rotate it (docs/release-flow.md, "Signed checksums"). Launchers older than the new key then
  refuse new versions until players download the launcher again.

## What this doesn't solve

- **Code that merges is code that ships.** Signing proves a build came from Horizon's pipeline, not that the code
  is good. Against a bad change, the safety nets are the checks and harnesses, review, the release smoke test and
  the release-blocker label. Since the single channel there is no 6-hour beta: a change reaches every player
  within minutes of merging.
- **Until agents switch to the App**, they still work with Ludovic's own account, so nothing technical tells his
  actions from theirs. Code owners' reviews are switched on together with the App.
- **The Mac mini** builds the macOS release and also runs pull requests' parity checks: a pull request from this
  repository runs its code there before review, and a fork's does once a maintainer adds `run-parity`. Code
  running there as the runner's user could tamper with what the next release builds. Releases check out clean
  (#1024), which removes anything a pull request left in the work folder by accident, but
  not a deliberate attack. The protection is review: only agents and maintainers push to this repository, and a
  maintainer reads a fork's change before labelling it.
- **The first install** with `install.sh` or `install.ps1` checks the signature when OpenSSL 3 is installed: it
  verifies `SHA256SUMS.sig` (a Nightly's `SHA256SUMS.v2.sig`, for its own version) against the release key built
  into the script, then the archive against `SHA256SUMS`. `install.ps1` looks for OpenSSL on the PATH and in Git
  for Windows, which ships it; Windows itself can't check Ed25519. With an older OpenSSL, LibreSSL (what macOS
  ships) or none, the script says so and checks only the SHA-256 GitHub publishes. Every update after that checks
  the signature.

## Reporting a security problem

Use GitHub's private vulnerability reporting (the repository's **Security** tab, "Report a vulnerability"), not a
public issue or Discord. Say what you found and how to reproduce it; we answer there.
