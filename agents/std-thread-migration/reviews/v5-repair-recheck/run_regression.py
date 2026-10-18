"""再复核运行器：每个普通 case 独立进程，10 秒内终止进程树。"""
import collections
import concurrent.futures
import json
import os
from pathlib import Path
import resource
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).resolve().parent
START = time.monotonic()
DEADLINE = START + 299
WORKERS = 32


def invoke(args, env, limit=9.5):
    p = subprocess.Popen(args, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, start_new_session=True)
    try:
        data = p.communicate(timeout=min(limit, max(0.01, DEADLINE-time.monotonic())))[0]
        return p.returncode, data.decode(errors="replace")
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        data = p.communicate()[0]
        return 124, data.decode(errors="replace")


artifacts = []
for line in (OUT / "build.jsonl").read_text().splitlines():
    x = json.loads(line)
    if x.get("executable") and x.get("profile", {}).get("test"):
        package = "engine" if "/packages/engine/" in x["target"]["src_path"] else "server"
        artifacts.append((package, x["target"]["name"], x["target"]["kind"], x["executable"]))

tasks = []
counts = collections.defaultdict(collections.Counter)
for package, name, kind, binary in artifacts:
    modes = ["0", "1"] if package == "engine" and "rlib" in kind else ["1"]
    for mode in modes:
        group = "lib-" + mode if package == "engine" and "rlib" in kind else package
        env = dict(os.environ, ENGINE_STEP_RUNTIME=mode)
        rc, listing = invoke([binary, "--list", "--format=terse"], env)
        assert rc == 0, (name, listing)
        rc, ignored_listing = invoke([binary, "--list", "--ignored", "--format=terse"], env)
        assert rc == 0, (name, ignored_listing)
        ignored = {line[:-6] for line in ignored_listing.splitlines() if line.endswith(": test")}
        names = [line[:-6] for line in listing.splitlines() if line.endswith(": test")]
        counts[group]["ignored"] += len(ignored)
        for case in names:
            if case not in ignored:
                tasks.append((group, name, binary, case, env))

print(json.dumps({"workers": WORKERS, "case_deadline_s": 9.5, "batch_deadline_s": 299,
                  "binaries": len(artifacts), "cases": len(tasks)}, ensure_ascii=False), flush=True)


def run(task):
    group, name, binary, case, env = task
    started = time.monotonic()
    rc, output = invoke([binary, case, "--exact", "--test-threads=1"], env)
    return dict(group=group, binary=name, case=case, rc=rc,
                seconds=round(time.monotonic()-started, 3), output=output)


with (OUT / "cases.jsonl").open("w") as log:
    # ts-rs export 会递归生成共享依赖，减少同时重复生成的进程数。
    phases = [(WORKERS, [t for t in tasks if "export_bindings" not in t[3]]),
              (4, [t for t in tasks if "export_bindings" in t[3]])]
    for workers, phase_tasks in phases:
      print("阶段并发", workers, "case 数", len(phase_tasks), flush=True)
      with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        for index, result in enumerate(pool.map(run, phase_tasks), 1):
            counts[result["group"]]["passed" if result["rc"] == 0 else "failed"] += 1
            if result["group"] == "lib-1":
                counts["engine"]["passed" if result["rc"] == 0 else "failed"] += 1
            log.write(json.dumps(result, ensure_ascii=False) + "\n")
            if result["rc"]:
                print(json.dumps({k:v for k,v in result.items() if k != "output"}), flush=True)
            if index % 500 == 0:
                print("完成", index, "耗时", round(time.monotonic()-START, 1), flush=True)
counts["engine"]["ignored"] += counts["lib-1"]["ignored"]
usage = resource.getrusage(resource.RUSAGE_CHILDREN)
summary = dict(counts={k:dict(v) for k,v in counts.items()}, seconds=round(time.monotonic()-START, 3),
               child_cpu_seconds=round(usage.ru_utime+usage.ru_stime, 3))
(OUT / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(summary, ensure_ascii=False), flush=True)
