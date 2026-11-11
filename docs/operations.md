# Operations: running Horizon by hand

How the Horizon Team and the council keep Horizon working when no AI agent is running and the maintainer's machines
are off. Each section says what normally happens by itself, who can do it by hand, and what to do when it breaks.
How decisions are made is in [GOVERNANCE.md](../GOVERNANCE.md); how a change reaches players is in
[release-flow.md](release-flow.md). This page only says how to operate them.

## Who can do what

| Who | Can |
| --- | --- |
| **Horizon Team** (GitHub team `horizon-team`, write access) | Review, label and merge pull requests; approve a fork's workflow runs; re-run failed jobs; start workflows by hand (`gh workflow run`); add `release-blocker` to an issue; merge in horizon-mods (form pull requests too) and upload its `staging` files |
| **Trusted reviewers** (`.github/trusted-reviewers.txt`) | `/nightly`, `/close`, `/reopen`, `/label`, `/unlabel`, `/unclaim` by comment ([release-flow.md](release-flow.md#trusted-reviewers)) |
| **Council** (🏛️ Council on Discord) | Decide by poll (`/vote`, `/poll start`); emergency actions (GOVERNANCE.md 4.6) |
| **Organisation owners** (Ludovic and rdbtCVS since 2026-11-11, council poll P28) | Repository variables and secrets (`SELF_HOSTED_CI`, `RELEASES_PAUSED`), branch protection, adding people to GitHub teams, merging past a missing review, the security advisories |
| **Code owners** (`.github/CODEOWNERS`: the team `horizon-rust/council`, P35) | Approve pull requests that touch `.github/`, `tools/ci/`, the release, signing and update code |

The organisation has two owners since the council's poll P28, so the last two rows no longer wait for one person.

## Merging a pull request

Nobody merges automatically. A Team member:

1. Reads the whole diff. For a fork, do it before approving its workflow runs (they wait under **Actions**:
   "Approve and run") and before adding `run-parity`.
2. Waits for `verify`, the one required check. A pull request that changes no Rust finishes it in about a minute.
3. Merges with a merge commit once `verify` passed, no change request is open and every comment has been read. A
   "don't merge" from anyone holds it. Never merge your own pull request, never use `gh pr merge --auto`, never
   force-push a contributor's branch (merge main into it instead).

Risky pull requests (saves, the network or server thread, world generation, the launcher's installs and updates,
the harnesses, anything over ~500 lines) get `needs-proof` and wait until someone has played the change: both
single-player paths, a LAN guest, a screenshot for anything visible. The list is in
[release-flow.md](release-flow.md#merging).

A pull request that touches a CODEOWNERS path needs a code owner's approving review: GitHub keeps its merge button
locked until then. Leave it open and say in the thread that it waits for a code owner.

Pull requests opened by the maintainer's agents (author `ludovic111`, body starting "Opened by Ludovic's AI agent")
are ordinary pull requests: review and merge them like any other, or close them if they went stale. Closing keeps
the branch.

## Releases

Everything below runs by itself on GitHub; you only step in when something is wrong.

- **The Nightly** is the only build players get. The train (`release.yml`, every 10 minutes) starts one after 5
  player-facing merges, or when the oldest has waited 6 hours, or at once for a pull request labelled
  `release-now`. Only a commit where both `checks` and `parity-main` passed ships. Who may add `release-now` is the
  council's choice; until it decides, leave the label to the maintainer.
- **Stopping Nightlies:** open an issue about the problem and label it `release-blocker`. Nothing is published
  while it is open, forced builds included. Close it when the fix is on main.
- **Rolling back:** revert the pull request and merge the revert. Players can pick an earlier build on the
  launcher's Versions page meanwhile.
- **The reviewers' picks:** `/nightly status` shows the set, `/nightly clear` builds main alone.
- **Launcher releases** follow the same train (`launcher.yml`).

The Nightly's macOS builds, its smoke test and parity-main run on the maintainer's Mac mini. While it is off,
**no Nightly and no launcher release can be published**: players keep the last ones, which keep working. See below.

## Self-hosted runners are offline

Some jobs run only on the maintainer's machines, because they need his own copy of Minecraft or a Mac
([ci-runners.md](ci-runners.md)). When those machines are off, those jobs wait up to a day and then fail. The
**runner watch** opens one issue, "CI: self-hosted runners are not taking jobs", within about 2 hours, listing what
is waiting, and closes it once the runners are back.

| Runner label | What waits | What to do |
| --- | --- | --- |
| `horizon-light` (bots) | Triage and claim commands, labels, overlap notes, the Mojang check | An organisation owner sets `SELF_HOSTED_CI` to `off` (Settings → Secrets and variables → Actions → Variables). Then everything runs on GitHub |
| `horizon-checks` | `verify` of this repository's own pull requests ending in 0 or 1 | Same switch (`off` or `light`), then re-run the waiting checks. Forks' pull requests never use these runners |
| `horizon-game` | A pull request's own parity job | It isn't required: merge on `verify`, and label pull requests that touch gameplay `needs-proof` |
| `horizon-parity-main` | parity-main on main | Nothing can be done on GitHub. Nightlies wait for the last commit it passed on |
| `horizon-nightly`, `horizon-release` | The Nightly's and the launcher's macOS builds | Nothing can be done on GitHub. No Nightly or launcher release until the Mac is back |

What keeps working without them: `verify` and merging, triage and labels (once `SELF_HOSTED_CI=off`), Kira, the
website, the mods registry and its checks, the official server.

What the council decides about it (which pull requests may still merge without the vanilla harnesses, and who could
host a runner with their own Minecraft) is a poll, not a rule on this page.

## Mojang republished the version

The hourly `mojang republish` workflow opens an issue "Mojang republished 26.3: <hash>" when Mojang changes the
version file the game is pinned to. If its text says the jars are unchanged, a pull request that pastes the snippet
from the issue into `research-lock.json` fixes it. New players can't prepare the game until a Nightly with that fix
is out, so label that pull request `release-now` (or ask the council to) once it merged.

## The mods registry (horizon-rust/horizon-mods)

- **Submissions from the website** arrive as pull requests authored by `ludovic111` (the registry's account). A
  Team member reviews the mod (what it does, its licence, no sign-in or token handling, no Mojang files; see
  `MOD-POLICY.md` there), approves, and squash-merges once `check` passed. Publishing to the registry is hourly
  (`publish.yml`).
- **Fork pull requests:** approve their workflow run, and upload the mod's file to the `staging` release as
  `<sha256>.<extension>` after checking its hash.
- **Taking a mod down:** a pull request adding it to `revoked.toml`. That file is a code-owner path: today only
  Ludovic can approve it. For malware, two council members act at once as an emergency (GOVERNANCE.md 4.6), post
  in #mod-log, and ask Ludovic to approve.

## Kira

Kira runs on Railway and needs nobody's machine.

- **Questions Kira can't answer** ("passed to the team") are listed in #kira-handoffs. Answer the member in their
  thread.
- **Switches anyone on staff can use:** `/kira-ai off|on` (AI answers), the triage pause.
- **Polls:** `/vote question:` (council members) starts a 24-hour poll; `/poll start` for the other kinds. The
  results are in #polls-and-decisions and in `https://kira-production-500e.up.railway.app/decisions.json`.
- **If Kira is down:** only the account holder can reach Railway today. Post in #council so someone tells him.

## The official server

The server (play.playhorizon.dev) runs on its host's panel, not on the maintainer's machines, and keeps running the
engine it has. Engine updates, backups and disk clean-ups are done by people with panel access; who has it, and the
steps, are in the council's private notes. Players' reports go to #mod-chat as usual. If the server is down, a
council member tells the panel holders and the host.

## Security problems

- **Reports** come through GitHub's private vulnerability reporting ([security.md](security.md#reporting-a-security-problem)).
- **A leaked secret, a malicious pull request or mod, a hijacked account:** any two council members act at once
  (GOVERNANCE.md 4.6): label an issue `release-blocker` to stop Nightlies, `/nightly clear` the picks, close the pull
  request, take the mod down, remove the account's roles, and ask the account holder to rotate the secret. Record it
  with `/emergency` in Kira once that command exists, else in #council.

## Who to ask

- Code and merges: the Horizon Team, in #horizon-team.
- Decisions: the council, by poll.
- Accounts (GitHub organisation, Discord ownership, Railway, the server panel, domains): the account holders; the
  list is in GOVERNANCE.md section 11.
