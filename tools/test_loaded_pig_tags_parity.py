#!/usr/bin/env python3
"""Actual pig Tags load/save/mutation proof; complete raw list order is diagnostic.

A plain run (tools/parity.py) runs fresh Java and this checkout's game and compares them.
To compare archived binaries: admit fresh Java with --reference-only --work-dir REF, then
reuse exact REF bytes for each with --reference-dir REF --binary GAME --work-dir NEW_OUT.
No output masks, ordering normalization or synthetic tag observer are applied.
"""
import json
from pathlib import Path
import shutil
import sys

import test_command_exec_parity as runner
from loaded_pig_tags_corpus import cases


def callback(row):
    values = row.get("results")
    assert isinstance(values, list) and len(values) == 1
    assert isinstance(values[0], list) and len(values[0]) == 2
    assert type(values[0][0]) is bool and type(values[0][1]) is int
    return values


def saved_schema(row, count):
    """Validate actual saved-list type/count without changing the raw body."""
    assert callback(row) == [[bool(count), count]]
    if count:
        assert len(row["messages"]) == 1
        text = "".join(span[0] for span in row["messages"][0])
        # These predeclared ASCII strings make the SNBT string-list a JSON array.
        prefix, marker, literal = text.partition(" has the following entity data: ")
        assert prefix and marker
        values = json.loads(literal)
        assert type(values) is list and len(values) == count
        assert all(type(value) is str for value in values)


def admit(program, java):
    ops = [op for op in program if op["op"] == "cmd" and op.get("record", True)]
    assert len(ops) == len(java) and len(ops) > 0
    for op, row in zip(ops, java):
        assert op["cmd"] == row["cmd"]
        if "expect_result" in op:
            assert callback(row) == [[True, op["expect_result"]]], (op, row)
        if "expect_callback" in op:
            assert callback(row) == [op["expect_callback"]], (op, row)
        if "saved_tags_count" in op:
            saved_schema(row, op["saved_tags_count"])
    return ops


def native_from_reference(reference, folder, program):
    runner.base.verify_artifacts()
    expected = "".join(json.dumps({k: v for k, v in op.items() if k != "expect_result"},
                                  ensure_ascii=False) + "\n" for op in program)
    assert (reference / "script.jsonl").read_text() == expected
    java = [json.loads(line) for line in (reference / "vanilla.jsonl").read_text().splitlines()]
    admit(program, java)
    assert "--binary" in sys.argv, "an archived normal Game binary is required"
    binary = Path(sys.argv[sys.argv.index("--binary") + 1]).resolve(strict=True)
    folder.mkdir(parents=True, exist_ok=True)
    for name in ["script.jsonl", "vanilla.jsonl"]:
        shutil.copyfile(reference / name, folder / name)
    scratch = folder / "rust"
    scratch.mkdir(exist_ok=False)
    env = runner.lab.game_env(HORIZON_PLAYER_NAME="Player",
                             HORIZON_COMMAND_SCRIPT=folder / "script.jsonl",
                             HORIZON_COMMAND_OUTPUT=folder / "rust.jsonl",
                             HORIZON_API_DISCOVERY="http://127.0.0.1:9/")
    with (folder / "rust.log").open("w") as log:
        result = runner.run_game([str(binary), "--resources", str(runner.lab.RESOURCES),
                                  "--world", "harness", "--seed", str(runner.SEED), "--radius", "4"],
                                 cwd=scratch, env=env, stdout=log, stderr=runner.subprocess.STDOUT)
    assert result.returncode == 0, f"native process failed; see {folder / 'rust.log'}"


def main():
    reference = None
    if "--reference-dir" in sys.argv:
        index = sys.argv.index("--reference-dir")
        reference = Path(sys.argv[index + 1]).resolve(strict=True)
        del sys.argv[index:index + 2]
    program = [*runner.corpus_module.group_setup(), *cases()]
    runner.corpus_module.groups = lambda: [("loaded pig Tags", program)]
    if "--work-dir" not in sys.argv:
        sys.argv += ["--work-dir", str(runner.lab.LOCAL / "loaded-pig-tags-parity")]
    if "--allow-gaps" not in sys.argv:
        sys.argv.append("--allow-gaps")
    folder = Path(sys.argv[sys.argv.index("--work-dir") + 1]).resolve()
    if reference is None:
        runner.main()
    else:
        assert "--reference-only" not in sys.argv
        native_from_reference(reference, folder, program)
    java = [json.loads(line) for line in (folder / "vanilla.jsonl").read_text().splitlines()]
    ops = admit(program, java)
    if "--reference-only" in sys.argv:
        print(f"Fresh Java admitted {len(ops)} records and saved-list type/count guards.")
        return 0
    native = [json.loads(line) for line in (folder / "rust.jsonl").read_text().splitlines()]
    assert len(ops) == len(native)
    report = {"strict_records": 0, "strict_saved_list_projections": 0,
              "strict_failures": [], "admission_records": 0, "admission_failures": [],
              "diagnostic_records": 0, "diagnostic_differences": []}
    mismatches = []
    for index, (op, left, right) in enumerate(zip(ops, java, native)):
        assert left["cmd"] == right["cmd"] == op["cmd"]
        same = all(left.get(key) == right.get(key) for key in ["messages", "results", "state"])
        if op.get("admission"):
            report["admission_records"] += 1
            if not same:
                report["admission_failures"].append(index)
        if not same:
            mismatches.append({"row": index, "java": left, "native": right})
        if op.get("diagnostic"):
            report["diagnostic_records"] += 1
            if not same:
                report["diagnostic_differences"].append(index)
            if "saved_tags_count" in op:
                report["strict_saved_list_projections"] += 1
                try:
                    saved_schema(right, op["saved_tags_count"])
                except (AssertionError, ValueError, TypeError):
                    report["strict_failures"].append({"row": index, "kind": "saved type/count/callback"})
        else:
            report["strict_records"] += 1
            if not same:
                report["strict_failures"].append({"row": index, "kind": "exact record"})
    (folder / "tags-report.json").write_text(json.dumps(report, indent=2) + "\n")
    (folder / "raw-mismatches.jsonl").write_text("".join(json.dumps(row) + "\n" for row in mismatches))
    print(json.dumps(report))
    return bool(report["strict_failures"])


if __name__ == "__main__":
    sys.exit(main())
