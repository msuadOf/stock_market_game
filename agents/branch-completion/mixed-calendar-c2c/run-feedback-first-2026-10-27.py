"""先独立确认open正控，再2并发执行open/closed；不编辑旧全矩阵准备。"""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import subprocess
import time

ROOT = Path("/data1/baiyifan/workplace/stock_market_game")
WORKTREE = Path(__file__).resolve().parents[3]
EVIDENCE = Path(__file__).resolve().parent / "feedback-first-red"
NODE = str(ROOT / ".tmp/node24-env/node")
SUPERVISOR = str(ROOT / ".worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs")
compiled = json.loads((EVIDENCE / "compile-result.json").read_text())
assert compiled["wrapper_exit_code"] == 0
binary = Path(compiled["binary"]["path"])
assert hashlib.sha256(binary.read_bytes()).hexdigest() == compiled["binary"]["sha256"]
before = {name: hashlib.sha256(Path(name).read_bytes()).hexdigest() for name in compiled["source_after"]}
assert before == compiled["source_after"]
OPEN = "open_limit_is_accepted_and_reserves_real_cash"
CLOSED = "closed_limit_returns_strict_exchange_closed_feedback"
plan = {
    "outer_argv": [NODE, SUPERVISOR, "10000", "--", "python3", str(Path(__file__).resolve())],
    "binary": compiled["binary"], "source_before": before,
    "case_parallelism": 2, "harness_threads_per_case": 1,
    "case_deadline_ms": 10000, "batch_deadline_ms": 10000,
    "sequence": ["open独立正控", "2并发open与closed领域红"],
}
(EVIDENCE / "run-plan.json").write_text(json.dumps(plan, ensure_ascii=False, indent=2) + "\n")


def run(case, prefix):
    command = [NODE, SUPERVISOR, "10000", "--", str(binary), case, "--exact", "--nocapture", "--test-threads=1"]
    start = time.monotonic()
    with (EVIDENCE / f"{prefix}-{case}.stdout").open("w") as stdout, (EVIDENCE / f"{prefix}-{case}.stderr").open("w") as stderr:
        result = subprocess.run(command, cwd=WORKTREE, stdout=stdout, stderr=stderr)
    record = {"case": case, "prefix": prefix, "command": command, "wrapper_exit_code": result.returncode, "wall_seconds": time.monotonic()-start}
    print(json.dumps(record, ensure_ascii=False), flush=True)
    return record


start = time.monotonic()
positive = run(OPEN, "positive")
(EVIDENCE / "positive-result.json").write_text(json.dumps(positive, ensure_ascii=False, indent=2) + "\n")
assert positive["wrapper_exit_code"] == 0, "open合法条件必须先由真实正控证明，不能把fixture错误当closed反馈红"
with ThreadPoolExecutor(max_workers=2) as workers:
    cases = list(workers.map(lambda case: run(case, "parallel"), [OPEN, CLOSED]))
after = {name: hashlib.sha256(Path(name).read_bytes()).hexdigest() for name in before}
summary = {"positive": positive, "cases": cases, "wall_seconds": time.monotonic()-start, "source_before": before, "source_after": after, "binary": compiled["binary"], "说明": "退出码为deadline CLI wrapper；原始stderr另保Rust失败文本，不归一化为raw child exit。"}
(EVIDENCE / "run-result.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
assert before == after
