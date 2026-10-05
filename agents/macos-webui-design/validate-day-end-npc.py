"""日终 NPC 修复的定向验收；每个预构建普通 case 由进程外十秒 deadline 保护。"""
import concurrent.futures
import os
import pathlib
import subprocess
import sys
import time

root = pathlib.Path(__file__).resolve().parents[2]
binary = root / "target/debug/deps/engine-9ee374b1ab641f42"
case_list = pathlib.Path(__file__).with_name("day-end-npc-related-case-list.txt")
cases = [line.removesuffix(": test") for line in case_list.read_text().splitlines()]
assert len(cases) >= 115, "定向 case 清单不完整"
started = time.monotonic()
env = dict(os.environ, RAYON_NUM_THREADS="1")
print(f"定向长验收：cases={len(cases)}；case并发=4；每case harness=1，Rayon=1；共享deadline=300000ms；case/命令=10000ms", flush=True)

def run_case(case):
    result = subprocess.run([
        "node", "scripts/run-with-deadline.mjs", "10000", "--", str(binary),
        case, "--exact", "--test-threads=1",
    ], cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    output = result.stdout
    valid = result.returncode == 0 and "1 passed; 0 failed" in output
    return case, valid, output

failures = []
with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
    for case, valid, output in executor.map(run_case, cases):
        if not valid:
            failures.append(case)
            print(output, flush=True)
print(f"完成：{len(cases)-len(failures)}/{len(cases)}，用时{time.monotonic()-started:.2f}s；失败={failures}", flush=True)
sys.exit(bool(failures))
