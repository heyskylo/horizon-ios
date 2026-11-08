#!/usr/bin/env python3
"""Actual modeled jukebox saved-clock presence; complete raw diagnostics retained.

Admit Java with --reference-only --work-dir REF. Compare an archived Game with
--reference-dir REF --binary ELF --work-dir OUT. No full song lifecycle or unsupported loaded-song/catalog claim.
"""
import json
from pathlib import Path
import shutil
import sys

import test_command_exec_parity as runner

GROUP = "jukebox saved clock"


def recorded(program):
    return [op for op in program if (op["op"] == "cmd" and op.get("record", True)) or (op["op"] == "tick" and op.get("record"))]


def admit(ops, rows):
    assert len(ops) == len(rows) and ops
    for op, row in zip(ops, rows):
        assert op.get("cmd", "#tick") == row["cmd"]
        if "expect_result" in op:
            assert row["results"] == [[True, op["expect_result"]]], (op, row)
        if "expect_callback" in op:
            assert row["results"] == [op["expect_callback"]], (op, row)


def compare(ops, java, native):
    admit(ops, java)
    assert len(native) == len(ops)
    report = {"records": len(ops), "strict_records": 0, "strict_matches": 0,
              "admissions": 0, "failed_admissions": [], "diagnostic_records": 0,
              "diagnostic_differences": [], "differences": []}
    for index, (op, left, right) in enumerate(zip(ops, java, native)):
        assert op.get("cmd", "#tick") == right["cmd"]
        same = left == right
        if not same:
            report["differences"].append({"row": index, "java": left, "native": right})
        if op.get("diagnostic"):
            report["diagnostic_records"] += 1
            if not same:
                report["diagnostic_differences"].append(index)
        else:
            report["strict_records"] += 1
            report["strict_matches"] += same
        if op.get("h032_admission"):
            report["admissions"] += 1
            if not same:
                report["failed_admissions"].append(index)
    return report


def main():
    program = dict(runner.corpus_module.groups())[GROUP]
    reference = None
    if "--reference-dir" in sys.argv:
        i = sys.argv.index("--reference-dir")
        reference = Path(sys.argv[i + 1]).resolve(strict=True)
        del sys.argv[i:i + 2]
    assert "--work-dir" in sys.argv
    folder = Path(sys.argv[sys.argv.index("--work-dir") + 1]).resolve()
    if reference is None:
        assert "--reference-only" in sys.argv
        sys.argv += ["--only", GROUP]
        runner.main()
    else:
        assert "--binary" in sys.argv and "--reference-only" not in sys.argv
        expected = "".join(json.dumps({k: v for k, v in op.items() if k != "expect_result"},
                                     ensure_ascii=False) + "\n" for op in program)
        assert (reference / "script.jsonl").read_text() == expected
        java = [json.loads(s) for s in (reference / "vanilla.jsonl").read_text().splitlines()]
        admit(recorded(program), java)
        runner.base.verify_artifacts()
        binary = Path(sys.argv[sys.argv.index("--binary") + 1]).resolve(strict=True)
        folder.mkdir(parents=True, exist_ok=False)
        for name in ["script.jsonl", "vanilla.jsonl"]:
            shutil.copyfile(reference / name, folder / name)
        scratch = folder / "rust"
        scratch.mkdir()
        env = runner.lab.game_env(HORIZON_PLAYER_NAME="Player",
                                 HORIZON_COMMAND_SCRIPT=folder / "script.jsonl",
                                 HORIZON_COMMAND_OUTPUT=folder / "rust.jsonl",
                                 HORIZON_API_DISCOVERY="http://127.0.0.1:9/")
        with (folder / "rust.log").open("w") as log:
            result = runner.run_game([str(binary), "--resources", str(runner.lab.RESOURCES),
                                      "--world", "harness", "--seed", str(runner.SEED), "--radius", "4"],
                                     cwd=scratch, env=env, stdout=log, stderr=runner.subprocess.STDOUT)
        assert result.returncode == 0, f"native process failed; see {folder / 'rust.log'}"
    java = [json.loads(s) for s in (folder / "vanilla.jsonl").read_text().splitlines()]
    admit(recorded(program), java)
    if reference is None:
        print(f"Java admitted {len(java)} complete records.")
        return 0
    native = [json.loads(s) for s in (folder / "rust.jsonl").read_text().splitlines()]
    report = compare(recorded(program), java, native)
    (folder / "jukebox-clock-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"{report['strict_matches']}/{report['strict_records']} strict; "
          f"{len(report['diagnostic_differences'])}/{report['diagnostic_records']} raw diagnostic differences")
    return bool(report["failed_admissions"] or report["strict_matches"] != report["strict_records"])


if __name__ == "__main__":
    sys.exit(main())
