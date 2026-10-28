"""纯 context 的独立轻编译/逐case执行，所有失败原件均保留。"""

from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path("/data1/baiyifan/workplace/stock_market_game")
WORKTREE = Path(__file__).resolve().parents[3]
TOPIC = Path(__file__).resolve().parent
phase, operation = sys.argv[1:]
assert phase in ["red", "green"] and operation in ["compile", "run"]
EVIDENCE = TOPIC / f"pure-{phase}"
OUTPUT = ROOT / ".tmp/mixed-calendar-c2c"
BINDING = ROOT / "agents/branch-completion/main-current-normal-rlib-2026-10-26/artifact-binding.json"
DEPS = ROOT / ".tmp/main-current-normal-rlib-2026-10-26/target/debug/deps"
NODE = ROOT / ".tmp/node24-env/node"
SUPERVISOR = ROOT / "scripts/run-with-deadline.mjs"
RUSTC = "/home/baiyifan/.rustup/toolchains/1.96.1-x86_64-unknown-linux-gnu/bin/rustc"
FILES = [
    "packages/engine/src/session/exchange_calendar_model.rs",
    "packages/engine/src/session/market_calendar_context.rs",
    "packages/engine/src/session/market_calendar_context_tests.rs",
    "agents/branch-completion/mixed-calendar-c2c/pure-context-harness.rs",
]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(name, value):
    (EVIDENCE / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def sources():
    return {name: digest(WORKTREE / name) for name in FILES}


artifact = next(item for item in json.loads(BINDING.read_text())["artifacts"] if item["normal_rlib"])
assert digest(Path(artifact["sealed"])) == artifact["sha256"]
binary = OUTPUT / f"pure-context-{phase}"
before = sources()
if operation == "compile":
    EVIDENCE.mkdir(exist_ok=False)
    assert not binary.exists(), "禁止覆盖此前产物"
    source_dir = EVIDENCE / "source"
    source_dir.mkdir()
    for name in FILES:
        shutil.copy2(WORKTREE / name, source_dir / Path(name).name)
    command = [
        str(NODE), str(SUPERVISOR), "10000", "--", RUSTC,
        "--edition=2021", "--test", str(WORKTREE / FILES[-1]),
        "--extern", "engine=" + artifact["sealed"],
        "--extern", "thiserror=" + str(DEPS / "libthiserror-ba5f344727a28423.rlib"),
        "-L", "dependency=" + str(DEPS),
        "-C", "codegen-units=8", "-C", "debuginfo=0", "-C", "strip=debuginfo",
        "-C", "link-arg=-Wl,--threads=8", "-o", str(binary),
    ]
    write("compile-plan.json", {
        "说明": "仅真实重编译本树C1a/context/tests，domain依赖root同源normal rlib，不是整crate构建。",
        "command": command, "cwd": str(WORKTREE), "source_before": before,
        "engine_artifact": artifact, "binding_sha256": digest(BINDING),
        "thiserror_sha256": digest(DEPS / "libthiserror-ba5f344727a28423.rlib"),
        "codegen_units": 8, "linker_threads": 8, "deadline_ms": 10000,
    })
    environment = os.environ.copy()
    environment["TMPDIR"] = str(OUTPUT)
    start = time.monotonic()
    with (EVIDENCE / "compile.stdout").open("w") as stdout, (EVIDENCE / "compile.stderr").open("w") as stderr:
        result = subprocess.run(command, cwd=WORKTREE, env=environment, stdout=stdout, stderr=stderr)
    summary = {
        "wrapper_exit_code": result.returncode, "wall_seconds": time.monotonic()-start,
        "source_before": before, "source_after": sources(),
        "binary": {"path": str(binary), "bytes": binary.stat().st_size, "sha256": digest(binary)} if binary.exists() else None,
    }
    write("compile-result.json", summary)
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    assert result.returncode == 0 and before == sources(), "编译失败不属于领域红"
else:
    compiled = json.loads((EVIDENCE / "compile-result.json").read_text())
    assert compiled["wrapper_exit_code"] == 0 and before == compiled["source_after"]
    assert digest(binary) == compiled["binary"]["sha256"]
    selected = subprocess.run([str(binary), "market_calendar_context::tests::", "--list"], capture_output=True, text=True, timeout=10)
    (EVIDENCE / "list.stdout").write_text(selected.stdout)
    (EVIDENCE / "list.stderr").write_text(selected.stderr)
    assert selected.returncode == 0
    cases = [line.removesuffix(": test") for line in selected.stdout.splitlines() if line.endswith(": test")]
    assert len(cases) == 22
    if phase == "red":
        cases = [case for case in cases if "::c2c_" in case]
        assert len(cases) == 9
    write("run-plan.json", {"binary": compiled["binary"], "source_before": before, "cases": cases, "case_parallelism": 8, "harness_threads_per_case": 1, "case_deadline_ms": 10000, "batch_deadline_ms": 10000})

    def run(case):
        short = case.split("::")[-1]
        command = [str(NODE), str(SUPERVISOR), "10000", "--", str(binary), case, "--exact", "--nocapture", "--test-threads=1"]
        start = time.monotonic()
        with (EVIDENCE / f"{short}.stdout").open("w") as stdout, (EVIDENCE / f"{short}.stderr").open("w") as stderr:
            result = subprocess.run(command, cwd=WORKTREE, stdout=stdout, stderr=stderr)
        row = {"case": case, "wrapper_exit_code": result.returncode, "wall_seconds": time.monotonic()-start}
        print(json.dumps(row, ensure_ascii=False), flush=True)
        return row

    start = time.monotonic()
    with ThreadPoolExecutor(max_workers=8) as workers:
        rows = list(workers.map(run, cases))
    summary = {"cases": rows, "wall_seconds": time.monotonic()-start, "source_before": before, "source_after": sources(), "binary": compiled["binary"], "说明": "退出码是deadline CLI wrapper；内层Rust失败码仅按stderr原件报告。"}
    write("run-result.json", summary)
    assert before == sources()
    if phase == "green":
        assert all(row["wrapper_exit_code"] == 0 for row in rows)
