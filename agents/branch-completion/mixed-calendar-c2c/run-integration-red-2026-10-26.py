"""先单独执行真实 restore 正控，再独立8并发执行行为 case；保留实际退出码。"""

from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import time


REPOSITORY = Path("/data1/baiyifan/workplace/stock_market_game")
WORKTREE = Path(__file__).resolve().parents[3]
EVIDENCE = Path(__file__).resolve().parent / "integration-red"
NODE = REPOSITORY / ".tmp/node24-env/node"
SUPERVISOR = REPOSITORY / "scripts/run-with-deadline.mjs"
INITIAL = "initial_mixed_fixture_restores_with_frozen_policy_and_catalog"
phase = sys.argv[1]
assert phase in ["initial", "behavior"]
compiled = json.loads((EVIDENCE / "compile-result.json").read_text())
assert compiled["exit_code"] == 0 and compiled["sources_unchanged"]
binary = Path(compiled["binary"]["path"])
assert hashlib.sha256(binary.read_bytes()).hexdigest() == compiled["binary"]["sha256"]
before = {
    name: hashlib.sha256((WORKTREE / name).read_bytes()).hexdigest()
    for name in compiled["test_source_after"]
}
assert before == compiled["test_source_after"]
all_cases = re.findall(r"#\[test\]\s+fn ([a-z0-9_]+)\(", (WORKTREE / "packages/engine/tests/mixed_calendar_c2c.rs").read_text())
assert len(all_cases) == 9 and len(set(all_cases)) == 9
if phase == "initial":
    cases = [INITIAL]
else:
    initial = json.loads((EVIDENCE / "initial-results.json").read_text())
    assert initial["cases"][0]["exit_code"] == 0, "初始 restore 正控必须先独立成功"
    assert initial["binary"] == compiled["binary"]
    cases = [case for case in all_cases if case != INITIAL]
plan = {
    "phase": phase, "case_parallelism": 8, "harness_threads_per_case": 1,
    "outer_deadline_ms": 10000, "per_case_deadline_ms": 10000,
    "binary": compiled["binary"], "test_source_before": before, "cases": cases,
}
(EVIDENCE / f"{phase}-plan.json").write_text(json.dumps(plan, ensure_ascii=False, indent=2) + "\n")


def run(case):
    command = [str(NODE), str(SUPERVISOR), "10000", "--", str(binary), case, "--exact", "--nocapture", "--test-threads=1"]
    start = time.monotonic()
    with (EVIDENCE / f"{case}.stdout").open("w") as stdout, (EVIDENCE / f"{case}.stderr").open("w") as stderr:
        process = subprocess.run(command, cwd=WORKTREE, stdout=stdout, stderr=stderr)
    return {"case": case, "command": command, "exit_code": process.returncode, "wall_seconds": time.monotonic() - start}


start = time.monotonic()
results = []
with ThreadPoolExecutor(max_workers=8) as workers:
    futures = [workers.submit(run, case) for case in cases]
    for future in as_completed(futures):
        result = future.result()
        results.append(result)
        print(json.dumps(result, ensure_ascii=False), flush=True)
after = {name: hashlib.sha256((WORKTREE / name).read_bytes()).hexdigest() for name in before}
summary = {
    "phase": phase, "cases": sorted(results, key=lambda item: item["case"]),
    "binary": compiled["binary"], "source_before": before, "source_after": after,
    "source_unchanged": before == after, "wall_seconds": time.monotonic() - start,
    "说明": "exit_code 是实际 child 结果；任何编译失败不进入此表，行为失败须逐条读原始断言。",
}
(EVIDENCE / f"{phase}-results.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
assert before == after
if phase == "initial":
    assert results[0]["exit_code"] == 0, "fixture 正控失败，后续行为红不可归因"
