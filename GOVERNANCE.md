# Horizon governance

> Adopted 2026-11-09 by the founder as his last decision as head of the project. It takes effect when he
> announces that he is stepping down. From then on only the council changes it, by a 2/3 poll (section 15).
>
> Amended 2026-11-10 by the founder (three kinds of action, elections by tier, who may do what, and council poll P41:
> the mods decide moderation matters). The council can overturn this amendment by poll.

Horizon has no boss. A **council of equals** decides by polls. **Theme groups** do the work in their areas, each
with a **lead**. The **Horizon Team** reviews and merges code. **Moderators** keep the Discord server and the
official server safe. Every role of trust is earned with work the project can measure, and it ends when that work
stops.

## Contents

1. [Principles](#1-principles)
2. [The levels at a glance](#2-the-levels-at-a-glance)
3. [Each level in detail](#3-each-level-in-detail)
4. [The council and its polls](#4-the-council-and-its-polls)
5. [The selection filter](#5-the-selection-filter)
6. [Theme groups](#6-theme-groups)
7. [Moderation and appeals](#7-moderation-and-appeals)
8. [Conflicts and hard cases](#8-conflicts-and-hard-cases)
9. [The age rule (18+)](#9-the-age-rule-18)
10. [The agents](#10-the-agents)
11. [Accounts and the handover path](#11-accounts-and-the-handover-path)
12. [Money](#12-money)
13. [Transparency](#13-transparency)
14. [Rules that are never voted on](#14-rules-that-are-never-voted-on)
15. [Changing this document](#15-changing-this-document)

## 1. Principles

- **Equals.** Every council member has one vote. Nobody, the founder included, stands above the others.
- **The founder.** Ludovic keeps the title of **Founder**: his role stays at the top of the member list and he is
  credited as Horizon's founder everywhere. The title is symbolic. It carries no extra vote or power: on the council
  he counts exactly like every other member.
- **Evidence, not popularity.** You get a role by doing the work first. Kira measures that work: merged pull
  requests, reviews, triage, support answers and days active. A vote then confirms it.
- **Small, clear powers.** Each role can do a short, written list of things. Anything else goes to the council.
- **Roles are on loan.** Every role has a 30-day probation and an inactivity rule. Stepping down for inactivity is
  not a sanction, and you can come back.
- **Public by default.** Polls, results and rules are public. Only matters about a person stay private, and even then
  the result is published.
- **18+ for roles of trust** (section 9).
- **Mojang's and Microsoft's rules come first.** They are never put to a vote (section 14).

## 2. The levels at a glance

From the most open level to the most trusted. A person can hold several levels at once: a group lead can also be on
the Horizon Team, for example.

| Level | What it can do | How you get in | How you leave | 18+ |
| --- | --- | --- | --- | --- |
| **Member** | Play, chat, report bugs, suggest ideas, open issues and pull requests, pick group roles | Join the Discord server or the GitHub repo | Leave, or get banned | No |
| **Contributor** (⛏️ Iron, 🥇 Gold, 💎 Diamond, 🟪 Netherite ranks) | Write in #contributors; `/claim` issues; vote in their group's elections (if active in that group) | One merged pull request in a public horizon-rust repo, linked with `/link-github` and `/verify-github`. Ranks come from merged pull requests: 10, 40, 100 and 200 | Contributor status and ranks stay | No |
| **Group member** | Read and write in the group's channel; get the group's pings; vote in the group's elections and votes when active | Pick the group's role in Channels & Roles, or with `/groups` | Remove the role | No |
| **🤝 Support** | Answer and close posts in #horizon-support | Given by any moderator (a free action, section 2.1) | Resign, 30 days inactive, or removed by a moderator, the Chief of Mods or the council | No |
| **🛡️ Moderator** | Time out, delete messages, manage threads, quarantine. No bans | Elected by the mods (section 5.3); council veto within 24 h | Resign, 30 days inactive, suspended by the Chief of Mods, recalled by the mods, removed by a council poll (simple majority) | **Yes** |
| **🛡️ Chief of Mods** | A moderator's powers, plus bans; leads the mods; suspends a mod at once pending the council | Elected by the mods, from the mods with at least 30 days (section 5.3); council veto within 24 h | Resign, 30 days inactive, recalled by the mods, council poll (simple majority) | **Yes** |
| **🧭 Group lead** | Decides inside the group's topic (section 6); manages the group's channel; triage on the group's GitHub issues | Elected by the group's active members and the Team (section 5.3); council veto within 24 h; 30 days probation | Resign, 30 days inactive (automatic), recalled by the same electorate, council poll (simple majority); confirmed again every 6 months | **Yes** |
| **🌌 Horizon Team** | Review and merge pull requests (GitHub `horizon-team`, write access); `/nightly` and triage commands (trusted reviewers); #horizon-team | Elected by the Team, 2/3 (section 5.3); council veto within 24 h; 30 days probation | Resign, 60 days without a merge or review (automatic), recalled by the Team (2/3), council poll (2/3, without the person) | **Yes** |
| **🏛️ Council** | Decides everything else by poll (section 4); bans; emergency actions by any 2 members | Nominated by a council member, the filter, a 24 h public discussion, then a council poll (2/3); 30 days probation | Resign, inactivity (section 5.5), council poll (2/3, without the person) | **Yes** |

**The account holder** isn't a level. The founder (Ludovic) still holds the project's accounts (GitHub organisation
owner, Discord server owner, the Microsoft app approved by Mojang, the domains, Railway, money). He carries out the
council's decisions on them and hands them over step by step (section 11). As a council member he has one vote,
like everyone else.

### 2.1 Three kinds of action

Most things need no vote at all. A vote is for power over other people, and for decisions that bind a whole body.

| Kind | What | How |
| --- | --- | --- |
| **Free** | Everyday actions inside your own role (the list below, and anything of the same weight) | Just do it. Kira logs the ones she sees in #staff-log. The tier above can reverse it; if people disagree, the tier above votes. A pattern of abuse is a recall |
| **Body vote** | Giving or taking a role that carries power (an election), or a decision that belongs to one body: the mods, the Team, a group | `/elect` or `/vote body:` in Kira (section 5.3). Any council member can call a veto within 24 hours |
| **Council poll** | Rules, money, accounts and ownership, the council's own members, vetoes, anything that spans several bodies | `/poll` (section 4) |

Free actions, for example:

- a lead gives or removes their group's helper role (🧩 Horizon Modding Team), through Kira;
- a moderator gives or removes 🤝 Support, through Kira;
- a moderator times out, deletes, quarantines, or changes a channel's slowmode or topic; the Chief of Mods bans and
  unbans;
- the Team merges and labels;
- a server operator restarts the official server, or prunes backups and chunks by the written rule;
- a lead pins, renames or creates threads in their group's channels;
- Kira's automatic actions.

**The 24-hour rule.** Every vote lasts 24 hours at most, and closes earlier once its result can't change. Only council
polls on this document, the licence, money and the GitHub organisation's ownership last 48 hours. A veto is called
within 24 hours of a result.

### 2.2 Who may do what

What each post decides, and what it holds on Discord, on the official server and on GitHub. Anything not listed is a
council matter.

| Post | Decides | Discord | Official server | GitHub |
| --- | --- | --- | --- | --- |
| **Member** | — | Chat, threads, picks group roles | Play | Issues, pull requests, `/claim` |
| **Group helper** (🧩 Horizon Modding Team) | — | Writes in the group's team channel | — | — |
| **🤝 Support** | Closes support posts | Manages threads in #horizon-support | — | — |
| **🛡️ Moderator** | Moderation matters, by the mods' vote (section 7) | Time out, delete messages, manage threads and nicknames, audit log, voice mute and move; quarantine through Kira; the staff channels | In-game op if also elected operator | — |
| **🛡️ Chief of Mods** | A moderator's, plus bans and unbans; suspends a mod at once; breaks ties in the mods' votes | Plus Ban | As a moderator | — |
| **Server operator** | In-game moderation by the server rules | — | Op (the server's ops list); no panel, no `server.toml` | — |
| **🧭 Server & SMP lead** | The server's settings, by the written rules (backup first, warning in game) | As every lead | A panel sub-user once a council accounts poll gives one | As every lead |
| **🧭 Group lead** | The group's free actions; gives and removes its helper role | In the group's channels: delete messages, manage and pin threads, ping the group; the helper role through Kira | — | Triage on the group's labels through Kira (no repository role) |
| **🌌 Horizon Team** | Merges; elects Team members | #horizon-team and the staff channels. The role still carries timeout and delete; whether it keeps them is a mod matter | — | `horizon-team`: write on horizon, horizon-mods, horizon-mixins, horizon-native-mod-template and horizon-server; also kira and horizon-site if P37 passes |
| **🔧 Server Manager** | — | Manage Channels and Manage Roles (the server's layout); given by council poll | — | — |
| **🏛️ Council** | Council polls, vetoes, emergencies (any 2) | A moderator's permissions plus Ban, mention @everyone and pin; #council | Panel sub-users for two members if P38 passes | The `council` team: code-owner reviews and the release tools' owner powers if P35 passes, security managers if P36 passes; a second organisation owner if P28 passes |
| **Account holder** (the founder) | Carries out decisions on his accounts (section 11) | Server owner | The panel's owner account | Organisation owner |

Kira applies a passed election everywhere she can: Discord roles herself, GitHub team membership when the council has
given her a token for it. Whatever she can't do (a panel sub-user, the ops list, a GitHub team without a token) she
posts as a list of clicks for a named person.

## 3. Each level in detail

### Member

Anyone in the Discord server or on GitHub. Members follow the server rules and Horizon's
[contributing guide](CONTRIBUTING.md). They can propose anything: an idea in #horizon-ideas, a bug in #horizon-bugs,
an issue or a pull request on GitHub, or a request for a council poll (any council member can pick it up).

### Contributor

- **Discord:** the 🛠️ Contributor role writes in #contributors and in the threads of #polls-and-decisions. A rank
  role (⛏️ Iron at 10 merged pull requests, 🥇 Gold at 40, 💎 Diamond at 100, 🟪 Netherite at 200, counted over every
  public horizon-rust repo) shows how much has been merged. Ranks only go up.
- **GitHub:** no repository role. Anyone can `/claim` and `/unclaim` their own issues.
- **Alone:** their own work.

### Group member

Each theme group (section 6) has a role that anyone can pick and drop. It opens the group's channel and receives the
group's pings. Picking a role grants no power. A group member counts as **active in the group** when Kira has seen at
least one counted action in the group's area (section 5.1) in the last 60 days. Active group members vote in the
group's elections and decisions (section 5.3).

### 🤝 Support

Unchanged: Support members help players in #horizon-support. They answer every post there and mark posts solved.
The role carries no permission, so it has no age rule. A moderator gives it, and takes it back, through Kira.

### 🛡️ Moderators and the Chief of Mods

- **Discord:** time out members, delete messages, manage threads and nicknames, read the audit log, quarantine
  through Kira, and see the staff channels. **Moderators don't ban.** Bans are done by council members and the Chief of
  Mods. Kira's automatic actions (the honeypot, clear scams, illegal images) stay as they are.
- **Official server:** server operators (ops) are moderators in game. They need the 18+ rule and are confirmed the
  same way (section 6, Server and SMP).
- **Alone:** a timeout, a deleted message, a quarantine, with a reason in the mod case.
- **The Chief of Mods** is the first contact for moderation questions. They organise the mods, can ban, and can take a
  moderator's role away at once when there is a problem. The council confirms or reverses that within 72 hours.

### 🧭 Group leads

Section 6 says what a lead decides alone and what goes to the council.

- **Discord:** in their group's channel only, delete messages, manage and pin threads, and ping the group role. The
  🧭 lead role carries no server-wide permission.
- **GitHub:** triage on the issues and pull requests that carry their group's label (`/label`, `/unlabel`, `/close`,
  `/reopen`, `/unclaim`), from a list of group leads that grants no repository role. Leads merge only if they are on
  the Horizon Team too.

### 🌌 Horizon Team

The Horizon Team **merges pull requests**. Its rules are in [the release flow](docs/release-flow.md): read the whole
diff, never merge your own pull request, and use `needs-proof` for anything risky.

- **GitHub:** member of the `horizon-team` team (write access to the Horizon repos), on the trusted reviewers list
  (`/nightly` and triage commands).
- **Discord:** the 🌌 Horizon Team role, #horizon-team, and writing in #contributors and the read-only channels.
  Moderation permissions belong to the moderators and the council: a Team member moderates only if they also hold
  one of those roles.
- **During the 30-day probation:** they merge only ordinary pull requests. They don't merge `needs-proof` pull
  requests, don't approve files that CODEOWNERS guards, and don't use `/nightly`.

### 🏛️ Council

- **Discord:** the 🏛️ Council role: the moderators' permissions plus ban and mention @everyone; #council (private);
  starting polls in #polls-and-decisions.
- **GitHub:** the `council` team. Over time it takes over the founder's CODEOWNERS review and his owner powers in the
  release tools (section 11).
- **Alone (any one member):** start a poll; pick up a member's request for a poll; do anything their other roles
  allow.
- **Any two members together:** an emergency action (section 4.6).
- **Everything else** is decided by poll.

## 4. The council and its polls

### 4.1 Founding council

kacpherek, levzzz5154, rdbtCVS, Online5277, dezer-x and Ludovic: six members, one vote each. The founding members
don't go through the filter or a probation. They do make the 18+ declaration (section 9). The council has at most
**9** members. It adds a seat only when a 2/3 poll says it needs one.

### 4.2 How a poll works

| Rule | Value |
| --- | --- |
| Who starts one | Any council member, with `/poll` (Kira). Anyone else asks a council member to start it |
| Where | #polls-and-decisions, in public. Polls about a person (removal, appeals, the age rule) run in #council, and only their result is public |
| How long | 24 hours; 48 hours for this document, the licence, money and the GitHub organisation's ownership. It closes early only if every council member has voted |
| Ballot | Yes, No or Abstain. You can change it until the poll closes |
| Quorum | At least half of the council votes (Abstain counts). With 6 members, 3 votes |
| Simple majority | More Yes than No |
| Two-thirds | Yes is at least 2/3 of the Yes and No votes, **and at least 3 Yes votes** |
| Tie | Nothing changes |
| No quorum | Nothing changes. Kira reminds the members who haven't voted 12 hours and 2 hours before the end |
| About yourself | You don't vote, and you don't count in the council's size for that poll |
| Asking again | A poll that failed can't be asked again for 7 days, unless two members agree that new facts justify it |

### 4.3 Which threshold applies

| Simple majority | Two-thirds |
| --- | --- |
| Day-to-day project decisions: direction, priorities, settling disputes between groups, a public statement | Adding council members |
| Vetoing a body's vote or election (section 5.3) | Removing a council member or a Team member (without the person concerned) |
| Removing a group lead, a moderator, the Chief of Mods or Support (their bodies can also recall them) | Rules: this document, the server rules, the contributing policy, the licence |
| Ending a probation early | Money (section 12) |
| Confirming an emergency action | Account handover steps (section 11) |
| Deciding for a body that has fewer than 3 voters | Waiving part of the selection filter (section 5.4) |
| Bringing back someone who stepped down for inactivity, within 6 months | Changing the council's size |

**When in doubt, two-thirds.** Any council member can say a poll needs 2/3, and Kira then counts it at 2/3.

### 4.4 Discussion

Polls that add someone to the council start with a **24-hour public discussion**. Other polls, and elections, are
discussed in their own thread while they run. A member who wants more time votes No and says
why. A failed poll can come back after 7 days.

### 4.5 Conflicts of interest

A member doesn't vote on a poll about themselves: their own role, their own removal, an appeal of their own
moderation action, or money paid to them. They say so in the poll's thread and Kira leaves them out of the count.

### 4.6 Emergencies

For **security** (a leaked secret, a malicious pull request or mod, a hijacked account), **Mojang or Microsoft** (a
request from them, a risk to the approved sign-in) or **legal** matters (a takedown, the law), **any two council
members act at once**. They may:

- pause releases and the Nightly;
- take a mod down from the registry;
- remove someone's roles or access for now;
- lock or hide channels and content;
- ask the account holder to rotate a secret or a key.

They record it with `/emergency` in Kira, and both of them confirm it there. Kira opens a council poll at once
(simple majority) to confirm it. If the poll doesn't pass, the action is undone where it can be. An emergency never
adds members, changes rules or spends money.

### 4.7 Removing a council member

A 2/3 poll of the other members, in #council. The person concerned doesn't vote and doesn't count in the quorum.
Only the result is published, never the reasons. Other ways to leave: resigning, inactivity (section 5.5), or a
false age declaration (section 9).

### 4.8 When the council drops below a quorum

- **Fewer than 5 members:** Kira opens nominations for the council at once. The council fills seats within 30 days.
- **Fewer than 3 members, or fewer than 3 who voted in the last 30 days:** an **interim council** forms. It counts the
  remaining council members, the Horizon Team and the group leads past probation. Its only task is adding council
  members, by its own 2/3 poll with a quorum of half. Any two of its members can take emergency actions. All other
  decisions wait. The Team keeps merging and the leads keep leading.
- **Nobody left:** the Team and the group leads form the interim council in the same way.

## 5. The selection filter

Nobody becomes a lead, a Team member or a council member just because they are well liked or talk a lot. Kira
measures the work, and only people above a floor can be nominated. Their body then elects them (section 5.3).

### 5.1 What Kira measures

| Signal | What counts | Source |
| --- | --- | --- |
| **Merged pull requests** | Pull requests merged in a public horizon-rust repo. For Modding, also in the mod API's fork (arismoko/horizon). Counted overall, and per group by label or by files touched | GitHub search, as the ranks are counted today |
| **Reviews** | Others' pull requests you reviewed: approved, asked for changes, or left a review with comments. One per pull request | GitHub search `reviewed-by:` and Kira's GitHub webhook |
| **Triage** | Triage commands run (`/label`, `/close`, duplicates), issues labelled or closed, bug posts routed on Discord | The triage workflow's comments and Kira's webhook |
| **Support answers** | #horizon-support posts you answered that ended solved | Kira's support feature |
| **Active days** | Distinct days with any of the above, or with messages in #contributors or a group's channel. Chat elsewhere doesn't count | Kira |
| **Tenure** | Days since your first counted contribution (since you joined the server, for moderators) | GitHub, Discord |
| **Clean record** | No timeout, quarantine or ban in the last 90 days | Kira's mod cases |
| **Verified GitHub** | `/verify-github` done | Kira |
| **18+ declaration** | Made (section 9) | Kira |

Kira turns these into an **evidence card**: the person's numbers, overall and in the group's area, for the last 60
days and in total. Anyone can see their own card with `/evidence`. A nomination shows the card publicly.

### 5.2 The floors

A nomination is refused below these floors.

| Role | Floor |
| --- | --- |
| **Group lead** | Verified GitHub; tenure of 30 days or more; **in the group's area: 8 or more merged pull requests, or 20 or more reviews** (Translations: 5 or more merged translation pull requests; Server and SMP: 8 pull requests, or 30 days as an official server operator with activity); 15 or more active days in the last 60; clean record; 18+ |
| **Moderator** | In the server for 30 days or more; Kira level 10 or more; 15 or more active days in the last 60; clean record; 18+ |
| **Chief of Mods** | Moderator for 30 days or more, past probation; 18+ |
| **Horizon Team** | 🥇 Gold rank (40 merged pull requests); 20 or more reviews of others' pull requests; tenure of 30 days or more; 20 or more active days in the last 60; clean record; verified GitHub; 18+ |
| **Council** | Horizon Team member, group lead or Chief of Mods for 60 days or more, past probation; 20 or more active days in the last 60; clean record; 18+ |

The numbers are checked again once a year (section 15). They should stay reachable for people who really work on
Horizon and out of reach for people who don't.

### 5.3 Elections

The body that works with a person elects them. Kira runs it: `/elect post: member: [group:]`, and `action: remove`
for a recall.

| Post | Who votes | Passes with | Who can start it |
| --- | --- | --- | --- |
| **🛡️ Moderator** | The mods (🛡️ Mod and 🛡️ Chief of Mods) | Simple majority; the Chief of Mods breaks a tie | Any mod, a council member |
| **🛡️ Chief of Mods** | The mods; candidates are mods for 30 days or more | Simple majority | Any mod, a council member |
| **Server operator** | The mods and the 🧭 Server lead | Simple majority | The Server lead, the Chief of Mods, a council member |
| **🧭 Group lead** (a co-lead once the group has 8 active members) | The group's active members, and the Team | Simple majority | The current lead, a Team member, a council member |
| **🌌 Horizon Team** | The Team | Two thirds, and at least 3 Yes | A Team member, a council member |
| **🏛️ Council** | The council, by poll after a 24-hour discussion (section 4) | Two thirds, and at least 3 Yes | A council member |
| 🤝 Support, a group's helper role | Nobody: a free action (section 2.1) | — | A moderator; the group's lead |

An **active group member** has held the group's role for 14 days or more (Kira counts from when she first saw it)
and has at least one merged pull request or review with the group's label in the last 60 days.

1. **Start.** Kira checks the floor (5.2) and the 18+ declaration. If the candidate falls short, the person starting
   gets a private "not now", and the candidate a private message saying what is missing. Nothing is posted and no
   reason is ever shown to anyone else. Kira checks what she can measure (verified GitHub, merged pull requests and
   reviews, overall and by the group's label, rank, days in the server, Kira level, clean record, the declaration);
   the voters judge the rest.
2. **Vote, 24 hours,** in the body's channel, discussed in the vote's thread. Yes, No or Abstain, changeable while it
   runs. Kira shows how many voted, never who. It closes early once the result can't change.
3. **Result,** applied at once. Kira gives or takes the Discord role and, with a token, the GitHub team; she lists the
   rest as clicks for a named person. Only the result is published, never the ballots.
4. **Veto, 24 hours.** Any council member can call it with the button under the result. A council poll (simple
   majority, without the people involved) decides; if it passes, Kira undoes the result.
5. **Probation, 30 days,** for leads and Team members. It ends by itself unless someone who can vote starts a recall.

The council can always remove someone by its own poll. **Body votes** work the same way for a body's own decisions:
`/vote body:mods`, `body:team` or `body:group`, simple majority, 24 hours, the same veto.

#### Hard cases

| Case | What happens |
| --- | --- |
| Fewer than 3 can vote | The council decides by poll |
| Quorum | A third of the electorate, and at least 3 ballots (Abstain counts). Without it, nothing changes |
| Tie | Fails, and nothing changes. In the mods' votes, the Chief of Mods' ballot breaks it |
| The candidate | Doesn't vote on themselves, and can decline (the election ends). The person who started it votes normally |
| Conflicts | Whoever the vote is about, or who gave or received the moderation action in question, doesn't vote |
| Several candidates for one seat | Nominations within the first 12 hours join one approval ballot. The seat goes to the most approvals if that is at least half of the voters; otherwise a 24-hour run-off between the top two |
| A seat that is held | Changes only by a recall or by the 6-month confirmation |
| The council is already voting on it | No election starts for the same person and post |
| Cooldown | A failed or vetoed election can't be started again for the same person and post for 30 days. At most 3 elections run at once in a body |
| Vacancy | When a lead or the Chief of Mods leaves, Kira opens the seat's election herself: 24 hours of nominations, then the vote with everyone nominated. Nobody nominated: the seat stays open, and the tier above acts meanwhile. Kira never picks a candidate |
| Abuse of a free action | The tier above reverses it; a disagreement goes to its vote. A pattern is a recall |
| Emergency | Section 4.6. The Chief of Mods can still suspend a mod at once |
| Kira is down | The body may run the same vote as a Discord poll in its channel for 24 hours; Kira records the result when she is back |

#### Reminders, not nominations

Once a month in a group's channel, Kira says how to propose someone, only when the group has no lead or someone in it
meets a post's floor. When a seat opens, she posts once in the electorate's channel. Once a month in #council, she
lists privately the people who meet a floor. These are suggestions: only people start elections.

### 5.4 Exceptions

Some valuable people don't show up in the numbers, such as a designer for the website. The council can **waive the
tenure or the area floors** by a 2/3 poll that says why. The age rule and the clean record are never waived.

### 5.5 Inactivity

Kira checks every day. It warns the person in a DM and in #council, then posts the step-down in
#polls-and-decisions with neutral wording ("stepped down: inactive"), and removes the Discord role. The account
holder or an org owner removes GitHub access when the Team role ends.

| Role | What counts as activity | Warning | Steps down |
| --- | --- | --- | --- |
| Group lead | A counted action in the group's area, or messages in the group's channel | Day 21 | Day 30 |
| Horizon Team | A merge or a review | Day 45 | Day 60 |
| Moderator, Chief of Mods | A moderation action, or messages in the staff channels | Day 21 | Day 30 |
| Support | An answer in #horizon-support | Day 21 | Day 30 |
| Council | A vote in a poll. At 30 days with no vote, while at least 2 polls were held | Day 23 | Day 30, or 45 days with no counted activity at all |

- **Away.** Anyone can announce an absence with `/away` for up to 45 days, once every 6 months. Away days don't
  count. A lead names someone from the group to cover for them.
- **Coming back.** Within 6 months: a simple-majority poll, with no new probation. After 6 months: the normal path.

### 5.6 Terms

- **Group leads** are confirmed every 6 months by an election in their group. If someone else in the group passes the
  floor and is nominated within its first 12 hours, both go on the same ballot.
- **Council and Team** have no fixed term. They stay as long as they are active and keep the council's trust. Removal
  is always possible (section 4.7).

## 6. Theme groups

### 6.1 The groups

| Group | Covers | GitHub | Discord |
| --- | --- | --- | --- |
| **Modding** | The mod API, native and content mods, mixins, the mod registry, `cargo horizon-mod` | The mod API lives in Ari's fork **arismoko/horizon**. Its owner runs that repo by their own rules; the council governs only what is Horizon's: horizon-rust/horizon-mods, horizon-mixins, horizon-native-mod-template, the registry. Label `mods` | The 🧩 MODDING category (#modding, #horizon-modding-team); roles 🧩 Modder (picked) and 🧩 Horizon Modding Team (the lead gives it) |
| **Shaders and rendering** | The renderer, shaders, shader packs, OptiFine and Polytone pack formats | Label `rendering`; `crates/horizon-client-render` (gfx), `*.wgsl`, `horizon-shaders`, `horizon-shader-api`, `horizon-optifine`, `horizon-polytone` | #shaders-rendering |
| **Performance** | Frame rate, memory, server tick, benchmarks | Label `perf`; `horizon-bench`, the benchmark tools | #performance |
| **Launcher** | The launcher: installs, updates, instances, its pages | Label `launcher`; `launcher/` | #launcher |
| **Server and SMP** | Dedicated server, hosting, the anticheat, multiplayer, and the official SMP | Labels `server`, `multiplayer`; `horizon-server*`, `horizon-anticheat`, `lan/`, `dedicated*`; horizon-rust/horizon-server | #server-smp (plus #join-server, #server-chat) |
| **Translations** | The game's and the launcher's languages | Label `roadmap:languages`; `launcher/lang/`, `crates/horizon-client/assets/horizon/lang/` | #translations and the 🌍 language channels |
| **Website** | playhorizon.dev | horizon-rust/horizon-site; label `website`. The site's design stays as it is unless the council decides otherwise | #website |
| **Vanilla accuracy** | Parity with vanilla 26.3: harnesses, oracles, the port ledger | Labels `parity`, `parity-regression`, `full-parity`; `tools/test_*_parity.py`, the Java oracles | #vanilla-accuracy |
| **UI** | The game's menus, HUD, settings, controller navigation, the design system | Labels `ui`, `accessibility`; `title/`, `settings_shell.rs`, `hud*`, `ui.rs`, `docs/design-system.md`, `horizon-theme`. The launcher's look is shared with Launcher | #ui |

A pull request can belong to two groups. Both leads can comment, and the Team merges as usual. If the leads disagree,
the council decides.

### 6.2 Leads per group

- **One lead per group.** A group with 8 or more active members may also have **one co-lead**, chosen the same way.
- A person leads **at most two groups**. Council members may lead a group, though a lead from outside the council is
  preferred: it spreads the work.

### 6.3 What a lead decides alone

Inside their group's topic, and within this document, the contributing guide, the vanilla-first rule, the layers and
the design system:

- the group's priorities and the order of its roadmap items;
- which issues are `good first issue`, and triage of the group's issues;
- technical and design direction inside the area, when vanilla behaviour and the default look don't change;
- accepting or declining an idea or a proposal in the area. Any two active group members or one council member can
  take that decision to the council;
- giving and removing the group's helper role, through Kira (a free action);
- how the group organises itself (threads, meetings, the channel's rules on top of the server's);
- asking the Team for reviews, adding `needs-proof`, and starting elections in the group.

### 6.4 What goes to the council

- the rules, money, accounts and secrets;
- who is on the council, the Team or the mods;
- anything that changes vanilla behaviour or turns a Horizon-only feature on by default;
- breaking a public promise: the mod API, server compatibility, save and file formats;
- disputes between groups;
- anything involving Mojang, Microsoft or the law;
- speaking for the whole project outside the group's topic.

### 6.5 A group without a lead

The work goes on. The Team reviews and merges, and Kira still routes the group's issues and questions to its
channel. Until a lead is chosen, any council member may take a lead's decision for that group and says so in the
channel. Kira posts once that the seat is open and how to start its election (section 5.3). **No lead is better than a lead who doesn't do the work:**
nobody is appointed just to fill the seat.

## 7. Moderation and appeals

- **The mods decide moderation matters** (council poll P41, imposed by the founder on 2026-11-10): who joins or
  leaves the mod team, the mods' permissions and the staff channels, bans and unbans, and channel settings for
  moderation. Everyday actions are free (section 2.1). Anything bigger is a mods' vote (`/vote body:mods`, simple
  majority, 24 hours), and the Chief of Mods breaks ties. **The council keeps a veto:** within 24 hours of a mods'
  decision, any council member can call a council poll, and a simple majority overturns it.
- **The server rules** are in #rules. Moderators enforce them, the Chief of Mods leads the mods, and the council can
  change the rules (2/3).
- **Every action has a mod case**: who acted, what they did, and why. Kira keeps the cases.
- **First appeal:** within 14 days, through #appeal (for a quarantine) or mod mail. A moderator who wasn't involved,
  or the Chief of Mods, answers within 72 hours.
- **Second appeal:** for bans, timeouts over 24 hours and quarantines, a mods' vote in #mod-chat. Mods who were
  involved don't vote, and the council's veto applies. The answer is final for 90 days.
- **Not appealable here:** actions that Discord's rules or the law require. Those are reported where the law says.
- **Removing a moderator:** the Chief of Mods can suspend a moderator at once, and the mods decide within 72 hours by
  a recall vote. The mods recall the Chief of Mods the same way. The council can also remove either by poll.
- **Council members keep the ban permission** for emergencies and when no moderator is around. They log the ban like
  any other, and the mods can reverse it by vote.
- **Staff are moderated like everyone.** When a council member is involved, the council decides without them.

## 8. Conflicts and hard cases

| Case | What happens |
| --- | --- |
| Two groups disagree | The leads talk first. If they still disagree, the council decides (simple majority) |
| A lead's decision is contested | Any two active group members or one council member can bring it to the council |
| A Team member merges against the rules | Any council member can revert the merge. The council can remove the member (2/3) |
| A council member breaks the rules | The council decides without them. Removal takes 2/3 (section 4.7) |
| Someone holds a role and is under 18 | Section 9 |
| The council can't reach quorum | Section 4.8 |
| The founder and the council disagree about one of his own accounts | Until a step of section 11 hands it over, the account stays his. He carries out the council's decisions on project matters, or he says publicly that he won't |

## 9. The age rule (18+)

**Who must be 18 or older:** members of the council and the Horizon Team, trusted reviewers, group leads and
co-leads, the Chief of Mods and the moderators, official server operators, and anyone with admin rights anywhere in
the project: Discord (Administrator, Manage Roles, Manage Channels, ban), GitHub (write, admin, organisation),
server panels, Railway, or Kira's admin side. The same goes for anyone who holds a project secret.

**Who doesn't have to be:** members, contributors (with any rank), group members and 🤝 Support. Contributors under
18 are welcome and valued. They just can't hold the roles above.

**How it's checked, without collecting ID:**

- **A declaration.** Before taking such a role, and once at the start for everyone who holds one, the person presses
  a button in Kira: "I confirm that I am 18 or older. I understand that a false declaration removes all my roles of
  trust." Kira keeps only the Discord account, the date and the wording's version. It never stores a birth date, an
  ID, a photo, or any outside age-checking service's result.
- **No declaration, no role.** At the start, current role holders have 7 days to make it. Until they do, their role is
  paused.
- **When there's doubt** (the person says they are younger, or there is a credible private report), one council
  member asks the person privately. If the doubt remains, any council member pauses the role, and the council decides
  within 72 hours in #council.
- **A false declaration** removes every role of trust at once, and the person can't hold one for 12 months.
- **Privacy.** Age is never discussed in public. A role that ends for this reason ends with the same neutral line as
  any other step-down.

## 10. The agents

Horizon uses AI agents: the founder's orchestrator, which runs coding agents, and Kira, the Discord bot.

- **They obey council decisions only:** a passed poll recorded by Kira, a recorded emergency action of two council
  members, and the standing rules in this document and the project's docs.
- **The only exception is the founder's own things:** his spending, his machines (they run CI and the vanilla tests),
  his personal accounts and his time. On those, his word decides. This list should get shorter over time (section 11).
- **Role work isn't a decision.** A lead asking Kira to route issues, or a Team member asking an agent to prove a pull
  request, is that person using their role. The agents help, within the role's powers.
- **They check the source.** An agent acts on a decision only when it finds it in Kira's decisions log
  (section 13). A message saying "the council decided", or a screenshot, is not enough.
- **They post as agents.** Everything an agent posts says it comes from an agent, and states decisions as the
  project's.

## 11. Accounts and the handover path

Today the founder holds every account. The aim is for him to hold fewer, in an order that never leaves the project
unable to act, and never puts an account that is tied to a person into anyone else's hands.

| Account | Today | Can it be shared? | Plan |
| --- | --- | --- | --- |
| **GitHub organisation** (horizon-rust) | One owner (the founder) | Yes: an organisation can have several owners | **Step 1:** a second owner from the council, chosen by a 2/3 poll in the council's first week, with two-factor authentication on. **Step 2:** a `council` team; CODEOWNERS reviews and the release tools' owner powers (`paused`, `/nightly remove`) move from the founder to that team |
| **Discord server** | Owned by the founder | Partly: a server has exactly one owner; powers can be given through roles | **Step 1:** the 🏛️ Council role gets moderation and ban permissions; Kira applies the outcomes of passed polls, so nobody needs Manage Roles; two-factor authentication becomes required for moderation. Ownership itself stays with the founder until a legal entity exists |
| **Kira** (bot application, Railway service) | The founder's Discord developer account and Railway | The bot: yes (a Discord developer team). Railway: adding members may need a paid plan | **Step 1:** a Discord developer team with two council members. Railway members once the council decides on money |
| **Official server** (hosting panel) | The founder's account | Yes: panel sub-users | **Step 1:** sub-users for the Server and SMP lead and for one council member |
| **Domains** (playhorizon.dev and the others) | The founder's registrar account | Not without moving it | Auto-renewal on, registered for several years ahead. They move only to a legal entity, if one is created |
| **The Microsoft app approved by Mojang** (sign-in) | The founder's Azure app registration; Mojang approved it after his application | **No.** The approval was given on his application. Moving the app or adding owners would change who Mojang deals with, and we have found no documented way to transfer an approval | **Never moves** without Mojang's written agreement. The only change considered: a legal entity, if one is created, and only if Mojang agrees |
| **Signing** (Apple developer certificate, release signing keys) | The founder's | The Apple certificate is in his legal name: no | The Apple certificate never moves. Release keys stay in GitHub's protected environments, and the council team approves their use from step 2 |
| **Money** (sponsorship, donations) | Personal accounts of the founder | Through a fiscal host | Section 12 |
| **The founder's machines, AI accounts and password vault** | His | No | His own things: they never move. The project should depend on them less over time |

**Order:** first the safety nets that remove a single point of failure (a second organisation owner, the bot's
developer team, panel sub-users). Then the day-to-day powers (CODEOWNERS and release tools to the council team). Last,
whatever needs a legal entity: domains, money, and with Mojang's agreement the sign-in app. Each step is a 2/3 poll,
and the founder carries it out.

## 12. Money

*Placeholder: the options (Open Collective and others) are being researched.*

- The council approves every expense by a **2/3 poll**.
- **Every income and every expense is public.**
- Nobody votes on money paid to themselves.

## 13. Transparency

- **#polls-and-decisions:** every poll and every result, posted by Kira, and the 24-hour discussions of nominations.
  For polls that aren't about a person, each member's vote is shown once the poll closes. The results of elections
  and body votes are posted there too; for elections, only the result.
- **#staff-log:** the free actions Kira sees (roles given or taken, bans, unbans), so the tier above can check them.
- **The decisions log:** Kira keeps an append-only record of every poll, emergency action, appointment and
  step-down, with its date and result. It is public and machine-readable, so the agents can check a decision there.
- **This document** lives in horizon-rust/horizon as `GOVERNANCE.md`. A short summary is pinned in #rules and in
  #polls-and-decisions.
- **Who holds which role:** the people page of playhorizon.dev's hub shows the council, the Team, the group leads,
  the Chief of Mods, the moderators and Support.
- **#council** is private for discussion and for polls about people. Everything the council decides is published.

## 14. Rules that are never voted on

These are Horizon's conditions for existing, not choices. No poll can change them:

- Playing needs a **Microsoft account that owns Minecraft: Java Edition**. There is no play without one.
- **No Mojang files** in Horizon's repos or downloads. Players' own copies stay on their own machines.
- **No gambling**, in any form, including with in-game currency.
- **Mojang's and Microsoft's terms and Usage Guidelines** apply to everything Horizon does.
- **The law**, and the terms of service of Discord and GitHub.

## 15. Changing this document

A 2/3 council poll, then a pull request that a Team member merges. The council reviews the floors' numbers
(section 5.2) once a year, or sooner if they turn out wrong.
