#!/usr/bin/env python3
"""Actual loot-mine pot component boundary; raw snapshots and clocks retained.

A plain run (tools/parity.py) runs fresh Java and this checkout's game and compares them.
To compare archived binaries: admit fresh Java with --reference-only --work-dir REF, then
compare each normal Game against --reference-dir REF with a fresh --work-dir OUT.
"""
import json
from pathlib import Path
import shutil
import sys

import test_command_exec_parity as runner
from loot_mine_pot_components_corpus import cases

def callback(row):
    values = row.get("results")
    assert isinstance(values, list) and len(values) == 1
    assert isinstance(values[0], list) and len(values[0]) == 2
    assert type(values[0][0]) is bool and type(values[0][1]) is int
    return values


def exact(left, right):
    return all(left.get(k) == right.get(k) for k in ["cmd", "messages", "results", "state"])


def clock_receipt(row):
    result = callback(row)
    assert result[0][0] is True and result[0][1] >= 0


def source_invariants(ops, rows):
    pairs = {}
    for op, row in zip(ops, rows):
        if "source_snapshot" in op:
            pairs.setdefault(op["source_snapshot"], {})[op["snapshot_phase"]] = row
    assert len(pairs) == 2 and all(set(p) == {"before", "after"} for p in pairs.values())
    return [name for name, pair in pairs.items() if not exact(pair["before"], pair["after"])]


def admit(program, java):
    ops = [op for op in program if op["op"] == "cmd" and op.get("record", True)]
    assert len(ops) == len(java) and ops
    for op, row in zip(ops, java):
        assert op["cmd"] == row["cmd"]
        if "expect_result" in op:
            assert callback(row) == [[True, op["expect_result"]]], (op, row)
        if "expect_callback" in op:
            assert callback(row) == [op["expect_callback"]], (op, row)
        if op.get("require_success"):
            clock_receipt(row)
    assert not source_invariants(ops, java), "Java source changed during loot mine"
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
        i = sys.argv.index("--reference-dir")
        reference = Path(sys.argv[i + 1]).resolve(strict=True)
        del sys.argv[i:i + 2]
    program = [*runner.corpus_module.group_setup(), *cases()]
    runner.corpus_module.groups = lambda: [("loot mine pot components", program)]
    if "--work-dir" not in sys.argv:
        sys.argv += ["--work-dir", str(runner.lab.LOCAL / "loot-mine-pot-components-parity")]
    if "--allow-gaps" not in sys.argv:
        sys.argv.append("--allow-gaps")
    folder = Path(sys.argv[sys.argv.index("--work-dir") + 1]).resolve()
    if reference is None:
        runner.main()
    else:
        assert "--reference-only" not in sys.argv
        native_from_reference(reference, folder, program)
    java = [json.loads(s) for s in (folder / "vanilla.jsonl").read_text().splitlines()]
    ops = admit(program, java)
    if "--reference-only" in sys.argv:
        print(f"Java admitted {len(ops)} raw records, two unchanged sources and fresh clock receipts.")
        return 0
    native = [json.loads(s) for s in (folder / "rust.jsonl").read_text().splitlines()]
    assert len(ops) == len(native)
    report = {"strict_records": 0, "clock_projections": 0, "strict_failures": [],
              "admission_records": 0, "admission_failures": [], "diagnostic_records": 0,
              "diagnostic_differences": [], "source_invariants": 2,
              "source_invariant_failures": source_invariants(ops, native)}
    mismatches = []
    for index, (op, left, right) in enumerate(zip(ops, java, native)):
        assert op["cmd"] == left["cmd"] == right["cmd"]
        same = exact(left, right)
        passed = same
        if not same:
            mismatches.append({"row": index, "java": left, "native": right})
        if op.get("diagnostic"):
            report["diagnostic_records"] += 1
            if not same:
                report["diagnostic_differences"].append(index)
            if op.get("require_success"):
                report["clock_projections"] += 1
                try:
                    clock_receipt(right)
                    passed = True
                except (AssertionError, ValueError, TypeError):
                    passed = False
                    report["strict_failures"].append({"row": index, "kind": "clock receipt"})
        else:
            report["strict_records"] += 1
            if not same:
                report["strict_failures"].append({"row": index, "kind": "exact record"})
        if op.get("admission"):
            report["admission_records"] += 1
            if not passed:
                report["admission_failures"].append(index)
    (folder / "pot-components-report.json").write_text(json.dumps(report, indent=2) + "\n")
    (folder / "raw-mismatches.jsonl").write_text("".join(json.dumps(x) + "\n" for x in mismatches))
    print(json.dumps(report))
    return bool(report["strict_failures"] or report["source_invariant_failures"])


if __name__ == "__main__":
    sys.exit(main())
