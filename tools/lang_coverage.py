#!/usr/bin/env python3
"""How much of Horizon's own text each language carries, and where it still says English.

`crates/horizon-client/assets/horizon/lang/en_us.json` is the reference: every key the
game asks for. A language that misses a key falls back to English at runtime
(`Language::load` in `crates/horizon-client-core/src/lang.rs` layers `en_us` under it), so
a gap is not an error — it is an English sentence on a translated screen. That file's
tests catch stale keys, repeated keys, empty values and changed placeholders, but
deliberately not gaps, because a language is allowed to lag. This tool reports them.

`launcher/lang/<code>.json` is the launcher's: its keys are the English sentence itself,
so a file is complete by construction (`launcher/src/i18n/tests.rs` checks every `tr("…")`
the code asks for). What it can still hold is a value identical to its English key, and a
`{hole}` the value dropped or added.

    python3 tools/lang_coverage.py                 # every file, both folders, most complete first
    python3 tools/lang_coverage.py --lang es_es es # only those file stems
    python3 tools/lang_coverage.py --game-only
    python3 tools/lang_coverage.py --summary       # one line per file, for a pull request
    python3 tools/lang_coverage.py --show-keys     # the keys behind every count
    python3 tools/lang_coverage.py --json          # the same, for a script
    python3 tools/lang_coverage.py --fail-under 95 # exit 1 under that cover (for CI, later)

It reads files and prints; it writes nothing and needs no `.local/`.
"""

import argparse
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GAME = ROOT / "crates/horizon-client/assets/horizon/lang"
LAUNCHER = ROOT / "launcher/lang"
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


def game_rows(directory: Path, codes: set[str]) -> tuple[LangFile, list[dict]]:
    """Every language in `directory` against `en_us.json`, most complete first."""
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
            "problem": lang.problem,
        })
    rows.sort(key=lambda row: (-row["coverage"], row["code"]))
    return reference, rows


def launcher_rows(directory: Path, codes: set[str]) -> list[dict]:
    """Every launcher language: the values still identical to their English key, and the
    `{holes}` a value does not match it on. Its key set is the code's job (`i18n/tests.rs`)."""
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
            "problem": lang.problem,
        })
    return rows


def print_problems(rows: list[dict]) -> None:
    for row in rows:
        if row["problem"]:
            print(f"  ! {row['code']}: {row['problem']}")


def print_game(reference: LangFile, directory: Path, rows: list[dict]) -> None:
    print(f"GAME  {shown(directory)}  ({reference.code}: {len(reference.entries)} keys, the reference)")
    print()
    print(f"  {'code':<10}{'keys':>6}{'cover':>8}{'missing':>9}{'extra':>7}{'empty':>7}{'same':>6}")
    for row in rows:
        print(f"  {row['code']:<10}{row['keys']:>6}{100 * row['coverage']:>7.1f}%"
              f"{len(row['missing']):>9}{len(row['extra']):>7}{len(row['empty']):>7}"
              f"{len(row['same']):>6}{'  *' if row['problem'] else ''}")
    print_problems(rows)


def print_launcher(directory: Path, rows: list[dict]) -> None:
    print(f"LAUNCHER  {shown(directory)}  (the English sentence is the key)")
    print()
    print(f"  {'code':<10}{'keys':>6}{'same':>7}{'holes':>7}{'empty':>7}")
    for row in rows:
        print(f"  {row['code']:<10}{row['keys']:>6}{len(row['same']):>7}{len(row['holes']):>7}"
              f"{len(row['empty']):>7}{'  *' if row['problem'] else ''}")
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


def print_game_summary(reference: LangFile, rows: list[dict]) -> None:
    for row in rows:
        line = (f"GAME {row['code']} {row['keys']}/{len(reference.entries)} keys, "
                f"missing {len(row['missing'])}, extra {len(row['extra'])}, "
                f"placeholders {len(row['placeholders'])}, empty {len(row['empty'])}, "
                f"same {len(row['same'])}")
        print(line + (f", {row['problem']}" if row["problem"] else ""))


def print_launcher_summary(rows: list[dict]) -> None:
    for row in rows:
        line = (f"LAUNCHER {row['code']} {row['keys']} keys, same {len(row['same'])}, "
                f"holes {len(row['holes'])}, empty {len(row['empty'])}")
        print(line + (f", {row['problem']}" if row["problem"] else ""))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    which = parser.add_mutually_exclusive_group()
    which.add_argument("--game-only", action="store_true", help="only the game's assets/horizon/lang")
    which.add_argument("--launcher-only", action="store_true", help="only the launcher's lang")
    parser.add_argument("--game-dir", type=Path, default=GAME,
                        help="the game's lang folder (default: %(default)s)")
    parser.add_argument("--launcher-dir", type=Path, default=LAUNCHER,
                        help="the launcher's lang folder (default: %(default)s)")
    parser.add_argument("--lang", nargs="+", metavar="CODE",
                        help="only these file stems (es_es and fr_fr for the game, es and fr for the launcher)")
    parser.add_argument("--summary", action="store_true", help="one line per file, for a pull request")
    parser.add_argument("--show-keys", action="store_true", help="list the keys behind every count")
    parser.add_argument("--json", action="store_true", help="machine output")
    parser.add_argument("--fail-under", type=float, metavar="PERCENT",
                        help="exit 1 when a game language covers less than this")
    args = parser.parse_args(argv)

    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")

    codes = set(args.lang or ())
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
            reference, game = game_rows(args.game_dir, codes)
        except ValueError as error:
            print(error, file=sys.stderr)
            return 2
        result["game"] = {"reference": reference.code, "keys": len(reference.entries), "files": game}
    launcher: list[dict] = []
    if not args.game_only:
        if not args.launcher_dir.is_dir():
            print(f"{args.launcher_dir}: no such folder", file=sys.stderr)
            return 2
        launcher = launcher_rows(args.launcher_dir, codes)
        result["launcher"] = {"files": launcher}
    if codes and not game and not launcher:
        print(f"no lang file is named {' or '.join(sorted(codes))}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    elif args.summary:
        if reference:
            print_game_summary(reference, game)
        print_launcher_summary(launcher)
    else:
        if reference:
            print_game(reference, args.game_dir, game)
        if not args.game_only:
            if reference:
                print()
            print_launcher(args.launcher_dir, launcher)
        if args.show_keys:
            if reference:
                print_game_keys(game)
            if launcher:
                print_launcher_keys(launcher)

    if args.fail_under is not None:
        under = [row for row in game if 100 * row["coverage"] + 1e-9 < args.fail_under]
        for row in under:
            print(f"{row['code']}: {100 * row['coverage']:.1f}% < {args.fail_under:g}%", file=sys.stderr)
        if under:
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
