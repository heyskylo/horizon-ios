# Translations

Horizon's own text (its menus, settings and the launcher) is translated into about 85 languages. Vanilla's text
comes from the player's own Minecraft install and is not translated here. Most of Horizon's translations were
written by machine, and players reported some of them as wrong, so the project decided on 2026-11-09 that every
machine-written string carries an **unreviewed** mark until a person who speaks the language has read it. Players see
no difference: the mark lives in a file the game and the launcher never read.

## Where the text lives

| | Files | Keys | Read by |
|---|---|---|---|
| Game | `crates/horizon-client/assets/horizon/lang/<code>.json` (`fr_fr`, `de_de`...) | `horizon.options.foo` style; `en_us.json` is the reference | `crates/horizon-client-core/src/lang.rs` (`Text::tr`, `lang::get`); a missing key falls back to English |
| A crate's own text | `crates/<name>/assets/lang/<code>.json`, the same shape | the crate's own keys, never one of the engine's | the same, once the crate registers them ([layers.md](layers.md#lang-files-horizon_platformlang)) |
| Launcher | `launcher/lang/<code>.json` (`fr`, `de`, `pt-br`...) | the English sentence itself | `launcher/src/i18n.rs` (`tr("…")`, `trf("… {arg} …")`) |
| Review record | `tools/lang_review/game/<code>.json`, `tools/lang_review/<crate name>/<code>.json`, `tools/lang_review/launcher/<code>.json` | the same keys, with the text a person checked | `tools/lang_coverage.py` only |

An entry is **reviewed** when the review record holds its key with exactly its present text. Anything else is
unreviewed: an entry the record does not hold, and one whose text changed since a person checked it (so a later
machine edit makes it unreviewed again). New strings need no extra step: added by machine, they start unreviewed.

## What is left to review

```sh
python3 tools/lang_coverage.py --summary                   # one line per file, "unreviewed N" at the end
python3 tools/lang_coverage.py --unreviewed --lang fr_fr fr # every unreviewed entry, its English and its text
python3 tools/lang_coverage.py --json                      # the same, for a script ("unreviewed" per file)
```

## Reviewing a language

Review happens in **#translations on Horizon's Discord** ([discord.gg/playhorizon](https://discord.gg/playhorizon)).

1. **Report a wrong string**: post the language, where you saw it (screen, setting, launcher page), the current text
   and the better text. Anyone can do this; you don't need GitHub.
2. **Review a whole language**: ask in #translations for your language's list of unreviewed strings (it is the
   `--unreviewed` output above). Go through it, in the game and launcher where you can, and answer with the strings
   to change (current text, better text) and the ones that are fine as they are.
3. **The fix lands as a pull request**: a maintainer or agent changes the lang files with your wording and records
   the checked entries as reviewed:

   ```sh
   python3 tools/lang_coverage.py --lang fr_fr --mark-reviewed horizon.options.foo horizon.options.bar
   python3 tools/lang_coverage.py --lang fr --mark-reviewed --keys-file checked.txt   # one key per line
   ```

   `--mark-reviewed` records the entries' present text, so change the text first, then mark it. It also drops
   entries the file no longer has. A code that is both a game and a launcher file (`lmo`, `szl`) needs
   `--game-only` or `--launcher-only`. Pull requests that only change translations take no label.

You can also open the pull request yourself: change the lang file, run `--mark-reviewed` for the entries you wrote
or checked, and say in the pull request which language you speak.

## Rules

- **Only a person who speaks the language clears a mark.** Never run `--mark-reviewed` on machine-written text, and
  never on text an AI tool rewrote, even when it reads well. An agent marks only entries whose wording came from a
  person, such as a #translations report it turns into a pull request (quote the report in the pull request).
- A new launcher string still needs an entry in every launcher lang file (`launcher/src/i18n/tests.rs` checks it),
  and a new game string falls back to English where a language lacks it. A machine translation is fine for both: it
  starts unreviewed by itself.
- Keep placeholders as they are (`%s`, `%1$s` in the game, `{name}` in the launcher): `lang_coverage.py` reports a
  changed one.

## How the first record was made

On 2026-11-09 every entry of every lang file was traced to the commit that last changed it (`git blame -M`). An entry
counts as written by a person, and went into the record, only when that commit:

- was not made under an agent's identity (`Ludovic Marie`, which his agents commit as, `Horizon agent`, `Claude`,
  bots) and is not a merge,
- has no AI co-author or AI mention in its message (Claude, Codex, GPT, Copilot, Cursor, Gemini, DeepSeek,
  opencode...),
- changed at most three non-English lang files,
- and is by an author whose lang changes, over the whole history, cover at most two languages (a person writes the
  one or two languages they speak; an author covering ten is a tool).

That kept 1,917 entries by nine contributors (Czech, German, Japanese, Brazilian Portuguese, Russian and Turkish).
The rule errs towards unreviewed: a wrong "unreviewed" costs a reviewer a glance, a wrong "reviewed" hides a bad
string. Everything else, about 197,000 entries, starts unreviewed.
