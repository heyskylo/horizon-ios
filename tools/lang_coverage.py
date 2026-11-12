#!/usr/bin/env python3
"""How much of Horizon's own text each language carries, and where it still says English.

`crates/horizon-client/assets/horizon/lang/en_us.json` is the reference: every key the
game asks for. A language that misses a key falls back to English at runtime
(`Language::load` in `crates/horizon-client-core/src/lang.rs` layers `en_us` under it), so
a gap is not an error — it is an English sentence on a translated screen. That file's
tests catch stale keys, repeated keys, empty values and changed placeholders, but
deliberately not gaps, because a language is allowed to lag. This tool reports them.

A crate that ships its own text keeps it in `crates/<name>/assets/lang/<code>.json`, the same
shape with its own `en_us.json` reference (docs/layers.md, "Lang files"); each such folder is
reported beside the game's, and its review record is `tools/lang_review/<name>/`.

`launcher/lang/<code>.json` is the launcher's: its keys are the English sentence itself,
so a file is complete by construction (`launcher/src/i18n/tests.rs` checks every `tr("…")`
the code asks for). What it can still hold is a value identical to its English key, and a
`{hole}` the value dropped or added.

Most translations were written by machine. A translated entry is *unreviewed* until a person
who speaks the language has read it: `tools/lang_review/{game,launcher}/<code>.json` records
the text a person wrote or checked, key for key, and any other entry, or one whose text changed
since, counts as unreviewed. Players see no difference; the record is for reviewers
(docs/translations.md).

    python3 tools/lang_coverage.py                 # every file, both folders, most complete first
    python3 tools/lang_coverage.py --lang es_es es # only those file stems
    python3 tools/lang_coverage.py --game-only
    python3 tools/lang_coverage.py --summary       # one line per file, for a pull request
    python3 tools/lang_coverage.py --show-keys     # the keys behind every count
    python3 tools/lang_coverage.py --summary --show-keys  # the summary, then the keys behind it
    python3 tools/lang_coverage.py --json          # the same, for a script
    python3 tools/lang_coverage.py --fail-under 95 # exit 1 under that cover (for CI, later)
    python3 tools/lang_coverage.py --unreviewed --lang fr_fr        # what a French reviewer reads
    python3 tools/lang_coverage.py --lang fr_fr --mark-reviewed KEY # a person checked KEY's text
    python3 tools/lang_coverage.py --lang fr --mark-reviewed --keys-file checked.txt

It needs no `.local/`, and writes nothing but the review record, and that only with
`--mark-reviewed`.
"""

import argparse
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GAME = ROOT / "crates/horizon-client/assets/horizon/lang"
CRATES = ROOT / "crates"
LAUNCHER = ROOT / "launcher/lang"
REVIEW = ROOT / "tools/lang_review"
REFERENCE = "en_us.json"


def shown(path: Path) -> str:
    """A path relative to the repository when it is inside it, portable separators either way."""
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return str(path)


def placeholders(text: str) -> list[str]:
    """The fields the code fills in, sorted, as `lang.rs` counts them: `%s`, `%d`, `%f`,
    `%1$s`, `%.2f`, `%%`. A percent sign that starts none of them ("100% is") is plain text."""
    found: list[str] = []
    rest = text
    while (at := rest.find("%")) != -1:
        rest = rest[at + 1:]
        spec = next((i for i, c in enumerate(rest) if c not in "0123456789$."), len(rest))
        after = rest[spec:spec + 1]
        if after in ("s", "d", "f", "%"):
            end = spec + 1
            found.append("%" + rest[:end])
            rest = rest[end:]
    return sorted(found)


HOLE = re.compile(r"\{([A-Za-z0-9_]+)\}")


def holes(text: str) -> set[str]:
    """The `{name}` a launcher string fills in (`trf`)."""
    return set(HOLE.findall(text))


def entry_lines(body: str) -> list[str]:
    """The keys of the lines that hold one `"key": "value"`, in file order, duplicates
    included: the same count `lang.rs`'s test makes to catch a repeated key."""
    keys: list[str] = []
    for line in body.splitlines():
        if not line.startswith('  "'):
            continue
        try:
            value, _ = json.JSONDecoder().raw_decode(line.lstrip())
        except json.JSONDecodeError:
            continue
        if isinstance(value, str):
            keys.append(value)
    return keys


@dataclass
class LangFile:
    """One `lang/<code>.json` and whatever is wrong with its shape (its text is not checked here)."""

    path: Path
    entries: dict[str, str] = field(default_factory=dict)
    problem: str = ""

    @property
    def code(self) -> str:
        return self.path.stem

    def __post_init__(self):
        try:
            body = self.path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as error:
            self.problem = f"cannot read: {error}"
            return
        if body.startswith("\ufeff"):
            self.problem = "starts with a byte order mark"
            return
        try:
            data = json.loads(body)
        except json.JSONDecodeError as error:
            self.problem = f"not JSON: {error}"
            return
        if not isinstance(data, dict) or not all(
                isinstance(key, str) and isinstance(value, str) for key, value in data.items()):
            self.problem = "not a flat map of key to text"
            return
        self.entries = data
        if len(entry_lines(body)) != len(data):
            self.problem = ('a key repeats, or an entry is not one `"key": "value"` line with '
                            "two spaces of indentation")


def reviewed(path: Path) -> dict[str, str]:
    """The record of what a person wrote or checked in one lang file: key to the text as it
    was checked. No record means no entry is checked yet."""
    if not path.exists():
        return {}
    record = LangFile(path)
    if record.problem:
        raise ValueError(f"{shown(path)}: {record.problem}")
    return record.entries


def unreviewed(entries: dict[str, str], record: dict[str, str]) -> list[str]:
    """The entries no person has checked in their present text. An empty value is a gap, not a
    translation, so it is never one."""
    return sorted(key for key, value in entries.items() if value.strip() and record.get(key) != value)


def game_rows(directory: Path, codes: set[str],
              review: Path | None = None) -> tuple[LangFile, list[dict]]:
    """Every language in `directory` against `en_us.json`, most complete first. With `review`
    (a `lang_review/game` folder), each row also says which entries no person checked."""
    reference = LangFile(directory / REFERENCE)
    if reference.problem:
        raise ValueError(f"{shown(reference.path)}: {reference.problem}")
    if not reference.entries:
        raise ValueError(f"{shown(reference.path)}: no key at all")
    english = reference.entries
    rows = []
    for path in sorted(directory.glob("*.json")):
        if codes and path.stem not in codes:
            continue
        lang = LangFile(path)
        entries = lang.entries
        missing = sorted(set(english) - set(entries))
        extra = sorted(set(entries) - set(english))
        changed = []
        for key in sorted(set(entries) & set(english)):
            want, got = placeholders(english[key]), placeholders(entries[key])
            if want != got:
                changed.append({"key": key, "english": english[key], "value": entries[key]})
        same = [] if lang.code == "en_us" else sorted(
            key for key in set(entries) & set(english) if entries[key] == english[key])
        rows.append({
            "code": lang.code,
            "path": shown(lang.path),
            "keys": len(entries),
            "coverage": (len(english) - len(missing)) / len(english),
            "missing": missing,
            "extra": extra,
            "placeholders": changed,
            "empty": sorted(key for key, value in entries.items() if not value.strip()),
            "same": same,
            "unreviewed": [] if lang.code == "en_us" else unreviewed(
                entries, reviewed(review / path.name) if review else {}),
            "problem": lang.problem,
        })
    rows.sort(key=lambda row: (-row["coverage"], row["code"]))
    return reference, rows


def crate_dirs(crates: Path) -> list[tuple[str, Path]]:
    """The crates that ship their own lang files: (crate name, its `assets/lang`), by name."""
    return sorted((path.parents[2].name, path.parent)
                  for path in crates.glob("*/assets/lang/" + REFERENCE))


def launcher_rows(directory: Path, codes: set[str], review: Path | None = None) -> list[dict]:
    """Every launcher language: the values still identical to their English key, the
    `{holes}` a value does not match it on, and with `review` (a `lang_review/launcher` folder)
    the entries no person checked. Its key set is the code's job (`i18n/tests.rs`)."""
    rows = []
    for path in sorted(directory.glob("*.json")):
        if codes and path.stem not in codes:
            continue
        lang = LangFile(path)
        same, changed = [], []
        for key, value in sorted(lang.entries.items()):
            if key == value:
                same.append(key)
                continue
            want, got = holes(key), holes(value)
            if want != got:
                changed.append({"key": key, "missing": sorted(want - got), "extra": sorted(got - want)})
        rows.append({
            "code": lang.code,
            "path": shown(lang.path),
            "keys": len(lang.entries),
            "same": same,
            "holes": changed,
            "empty": sorted(key for key, value in lang.entries.items() if not value.strip()),
            "unreviewed": unreviewed(lang.entries, reviewed(review / path.name) if review else {}),
            "problem": lang.problem,
        })
    return rows


def print_problems(rows: list[dict]) -> None:
    for row in rows:
        if row["problem"]:
            print(f"  ! {row['code']}: {row['problem']}")


def print_game(reference: LangFile, directory: Path, rows: list[dict], kind: str = "GAME") -> None:
    print(f"{kind}  {shown(directory)}  ({reference.code}: {len(reference.entries)} keys, the reference)")
    print()
    print(f"  {'code':<10}{'keys':>6}{'cover':>8}{'missing':>9}{'extra':>7}{'empty':>7}{'same':>6}"
          f"{'unreviewed':>12}")
    for row in rows:
        print(f"  {row['code']:<10}{row['keys']:>6}{100 * row['coverage']:>7.1f}%"
              f"{len(row['missing']):>9}{len(row['extra']):>7}{len(row['empty']):>7}"
              f"{len(row['same']):>6}{len(row['unreviewed']):>12}{'  *' if row['problem'] else ''}")
    print_problems(rows)


def print_launcher(directory: Path, rows: list[dict]) -> None:
    print(f"LAUNCHER  {shown(directory)}  (the English sentence is the key)")
    print()
    print(f"  {'code':<10}{'keys':>6}{'same':>7}{'holes':>7}{'empty':>7}{'unreviewed':>12}")
    for row in rows:
        print(f"  {row['code']:<10}{row['keys']:>6}{len(row['same']):>7}{len(row['holes']):>7}"
              f"{len(row['empty']):>7}{len(row['unreviewed']):>12}{'  *' if row['problem'] else ''}")
    print_problems(rows)


def print_groups(row: dict, groups: list[tuple[str, str]]) -> None:
    for field, title in groups:
        if row[field]:
            print(f"\n{row['code']}: {title} ({len(row[field])})")
            for key in row[field]:
                print(f"  {key}")


def print_game_keys(rows: list[dict]) -> None:
    for row in rows:
        print_groups(row, [("missing", "missing"), ("extra", "not in the reference"),
                           ("empty", "empty"), ("same", "identical to English")])
        for bad in row["placeholders"]:
            print(f"\n{row['code']}: {bad['key']} changes the placeholders of the English text")
            print(f"  english: {bad['english']}")
            print(f"  {row['code']}: {bad['value']}")


def print_launcher_keys(rows: list[dict]) -> None:
    for row in rows:
        print_groups(row, [("empty", "empty"), ("same", "identical to English")])
        for bad in row["holes"]:
            print(f"\n{row['code']}: {bad['key']} does not carry the key's holes")
            for side in ("missing", "extra"):
                if bad[side]:
                    print(f"  {side}: {', '.join(bad[side])}")


def print_game_summary(reference: LangFile, rows: list[dict], kind: str = "GAME") -> None:
    for row in rows:
        line = (f"{kind} {row['code']} {row['keys']}/{len(reference.entries)} keys, "
                f"missing {len(row['missing'])}, extra {len(row['extra'])}, "
                f"placeholders {len(row['placeholders'])}, empty {len(row['empty'])}, "
                f"same {len(row['same'])}, unreviewed {len(row['unreviewed'])}")
        print(line + (f", {row['problem']}" if row["problem"] else ""))


def print_launcher_summary(rows: list[dict]) -> None:
    for row in rows:
        line = (f"LAUNCHER {row['code']} {row['keys']} keys, same {len(row['same'])}, "
                f"holes {len(row['holes'])}, empty {len(row['empty'])}, "
                f"unreviewed {len(row['unreviewed'])}")
        print(line + (f", {row['problem']}" if row["problem"] else ""))


def print_unreviewed(kind: str, rows: list[dict], english: dict[str, str] | None) -> None:
    """The entries a reviewer reads, with the English beside them (a launcher key is its
    English)."""
    for row in rows:
        if not row["unreviewed"]:
            continue
        print(f"\n{kind} {row['code']}: {len(row['unreviewed'])} unreviewed ({row['path']})")
        text = LangFile(ROOT / row["path"]).entries  # ROOT / an absolute path is that path
        for key in row["unreviewed"]:
            print(f"  {key}")
            if english is not None:
                print(f"    en_us: {english.get(key, '')}")
            print(f"    {row['code']}: {text.get(key, '')}")


def mark_reviewed(lang: Path, record_path: Path, keys: list[str]) -> list[str]:
    """Records `keys` of `lang` as checked by a person, in their present text, and drops the
    record's keys the file no longer has. Returns the keys `lang` does not have."""
    entries = LangFile(lang).entries
    record = {key: text for key, text in reviewed(record_path).items() if key in entries}
    unknown = [key for key in keys if key not in entries]
    for key in keys:
        if key in entries:
            record[key] = entries[key]
    ordered = {key: record[key] for key in entries if key in record}
    if ordered:
        record_path.parent.mkdir(parents=True, exist_ok=True)
        record_path.write_text(json.dumps(ordered, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    elif record_path.exists():
        record_path.unlink()
    return unknown


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    which = parser.add_mutually_exclusive_group()
    which.add_argument("--game-only", action="store_true", help="only the game's assets/horizon/lang")
    which.add_argument("--launcher-only", action="store_true", help="only the launcher's lang")
    parser.add_argument("--game-dir", type=Path, default=GAME,
                        help="the game's lang folder (default: %(default)s)")
    parser.add_argument("--crates-dir", type=Path,
                        help="report every <dir>/<crate>/assets/lang too (default: the repository's crates, "
                             "unless --game-dir names another folder)")
    parser.add_argument("--launcher-dir", type=Path, default=LAUNCHER,
                        help="the launcher's lang folder (default: %(default)s)")
    parser.add_argument("--lang", nargs="+", metavar="CODE",
                        help="only these file stems (es_es and fr_fr for the game, es and fr for the launcher)")
    parser.add_argument("--summary", action="store_true", help="one line per file, for a pull request")
    parser.add_argument("--show-keys", action="store_true",
                        help="list the keys behind every count (--json already carries them)")
    parser.add_argument("--json", action="store_true", help="machine output")
    parser.add_argument("--fail-under", type=float, metavar="PERCENT",
                        help="exit 1 when a game language covers less than this")
    parser.add_argument("--review-dir", type=Path, default=REVIEW,
                        help="the record of checked entries, game/ and launcher/ (default: %(default)s)")
    parser.add_argument("--unreviewed", action="store_true",
                        help="list the entries no person checked yet, with their English")
    parser.add_argument("--mark-reviewed", nargs="*", metavar="KEY",
                        help="record these keys of the one --lang file as checked by a person")
    parser.add_argument("--keys-file", type=Path,
                        help="with --mark-reviewed: more keys, one per line (launcher keys are sentences)")
    args = parser.parse_args(argv)

    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")

    codes = set(args.lang or ())
    if args.crates_dir is None and args.game_dir == GAME:
        args.crates_dir = CRATES
    if args.mark_reviewed is not None:
        return mark_command(args, codes)
    if args.fail_under is not None and args.launcher_only:
        print("--fail-under measures the game's languages; --launcher-only has none", file=sys.stderr)
        return 2
    result: dict = {}
    game: list[dict] = []
    reference = None
    if not args.launcher_only:
        if not args.game_dir.is_dir():
            print(f"{args.game_dir}: no such folder", file=sys.stderr)
            return 2
        try:
            reference, game = game_rows(args.game_dir, codes, args.review_dir / "game")
        except ValueError as error:
            print(error, file=sys.stderr)
            return 2
        result["game"] = {"reference": reference.code, "keys": len(reference.entries), "files": game}
    # (crate name, its folder, its reference, its rows): the text crates ship.
    crates: list[tuple[str, Path, LangFile, list[dict]]] = []
    if not args.launcher_only and args.crates_dir is not None:
        for name, folder in crate_dirs(args.crates_dir):
            try:
                crates.append((name, folder, *game_rows(folder, codes, args.review_dir / name)))
            except ValueError as error:
                print(error, file=sys.stderr)
                return 2
        if crates:
            result["crates"] = {name: {"reference": ref.code, "keys": len(ref.entries), "files": rows}
                                for name, _, ref, rows in crates}
    launcher: list[dict] = []
    if not args.game_only:
        if not args.launcher_dir.is_dir():
            print(f"{args.launcher_dir}: no such folder", file=sys.stderr)
            return 2
        try:
            launcher = launcher_rows(args.launcher_dir, codes, args.review_dir / "launcher")
        except ValueError as error:
            print(error, file=sys.stderr)
            return 2
        result["launcher"] = {"files": launcher}
    if codes and not game and not launcher and not any(rows for *_, rows in crates):
        print(f"no lang file is named {' or '.join(sorted(codes))}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    elif args.unreviewed:
        if reference:
            print_unreviewed("GAME", game, reference.entries)
        for name, _, ref, rows in crates:
            print_unreviewed(f"GAME {name}", rows, ref.entries)
        print_unreviewed("LAUNCHER", launcher, None)
    elif args.summary:
        if reference:
            print_game_summary(reference, game)
        for name, _, ref, rows in crates:
            print_game_summary(ref, rows, f"GAME {name}")
        print_launcher_summary(launcher)
    else:
        if reference:
            print_game(reference, args.game_dir, game)
        for name, folder, ref, rows in crates:
            print()
            print_game(ref, folder, rows, f"GAME {name}")
        if not args.game_only:
            if reference:
                print()
            print_launcher(args.launcher_dir, launcher)

    if args.show_keys and not args.json:
        # the keys behind the counts, after the lines that carry the counts; --json holds
        # them already, and printing more would break what a script reads.
        if reference:
            print_game_keys(game)
        for *_, rows in crates:
            print_game_keys(rows)
        if launcher:
            print_launcher_keys(launcher)

    if args.fail_under is not None:
        every = game + [row for *_, rows in crates for row in rows]
        under = [row for row in every if 100 * row["coverage"] + 1e-9 < args.fail_under]
        for row in under:
            print(f"{row['code']}: {100 * row['coverage']:.1f}% < {args.fail_under:g}%", file=sys.stderr)
        if under:
            return 1
    return 0


def mark_command(args, codes: set[str]) -> int:
    """`--mark-reviewed`: one language, named by `--lang`; --game-only or --launcher-only picks the
    folder when the code is in both. The game's text may be split between its own folder and the
    crates' (`--crates-dir`): each key is recorded for the folder that holds it."""
    if len(codes) != 1:
        print("--mark-reviewed needs exactly one --lang code", file=sys.stderr)
        return 2
    code = next(iter(codes))
    if code == "en_us":
        print("en_us is the English reference, not a translation", file=sys.stderr)
        return 2
    game = [("game", args.game_dir)]
    if args.crates_dir is not None:
        game += crate_dirs(args.crates_dir)
    sides = [[(kind, folder / f"{code}.json") for kind, folder in folders
              if (folder / f"{code}.json").exists()]
             for folders, skip in ((game, args.launcher_only), ([("launcher", args.launcher_dir)], args.game_only))
             if not skip]
    sides = [side for side in sides if side]
    if not sides:
        print(f"no lang file is named {code}", file=sys.stderr)
        return 2
    if len(sides) > 1:
        print(f"{code} is a game and a launcher file: add --game-only or --launcher-only", file=sys.stderr)
        return 2
    found = sides[0]
    keys = list(args.mark_reviewed)
    if args.keys_file:
        keys += [line for line in args.keys_file.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not keys:
        print("--mark-reviewed needs keys, on the command line or in --keys-file", file=sys.stderr)
        return 2
    # Each key goes to the first folder that has it; a key none has is reported against them all.
    left = list(keys)
    for kind, lang in found:
        entries = LangFile(lang).entries
        mine = [key for key in left if key in entries]
        left = [key for key in left if key not in entries]
        if not mine and len(found) > 1:
            continue
        try:
            mark_reviewed(lang, args.review_dir / kind / lang.name, mine)
        except ValueError as error:
            print(error, file=sys.stderr)
            return 2
        print(f"{len(mine)} entries of {shown(lang)} recorded as reviewed")
    for key in left:
        print(f"{' or '.join(shown(lang) for _, lang in found)} has no key {key!r}", file=sys.stderr)
    return 1 if left else 0


if __name__ == "__main__":
    raise SystemExit(main())
