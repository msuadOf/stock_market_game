import json, os, signal, subprocess, time
from pathlib import Path
root = Path.cwd()
ev = root / "agents/escrow-harness-contract"
partitions = [("harness", "^performance harness"), ("lifecycle", "^(PerformanceComparisonRun|ProcessSampleRun|runProcessSample)")]
children, observations = [], []
start = time.monotonic()
try:
    for name, pattern in partitions:
        argv = ["node", str(root / "scripts/run-with-deadline.mjs"), "10000", "--", "node", "--test", "--test-timeout=10000", "--test-isolation=none", "--test-concurrency=2", f"--test-name-pattern={pattern}", str(root / "scripts/simulation/escrow-performance-harness.test.mjs")]
        out, err = (ev / f"green-{name}.stdout").open("w"), (ev / f"green-{name}.stderr").open("w")
        external_tmp = str(root.parent.parent / ".tmp/escrow-harness-contract-external")
        env = dict(os.environ, TMPDIR=external_tmp, TMP=external_tmp, TEMP=external_tmp)
        child = subprocess.Popen(argv, cwd=root, env=env, stdout=out, stderr=err, start_new_session=True)
        children.append((name, child, argv, out, err))
    while any(child.poll() is None for _, child, *_ in children):
        if time.monotonic() - start >= 9:
            raise TimeoutError("共享普通测试 deadline 的执行预算耗尽")
        snapshot = []
        for name, child, *_ in children:
            if child.poll() is not None:
                continue
            todo, processes = [child.pid], []
            while todo:
                pid = todo.pop()
                try:
                    stat = Path(f"/proc/{pid}/stat").read_text().split()
                    processes.append({"pid": pid, "cpu_ticks": int(stat[13]) + int(stat[14]), "threads": int(stat[19])})
                    for task in Path(f"/proc/{pid}/task").iterdir():
                        todo.extend(map(int, (task / "children").read_text().split()))
                except FileNotFoundError:
                    pass
            snapshot.append({"shard": name, "processes": processes})
        observations.append({"wall_ms": round((time.monotonic() - start) * 1000, 3), "running": snapshot})
        time.sleep(.01)
finally:
    for name, child, argv, out, err in children:
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=.5)
        out.close()
        err.close()
result = {"wall_ms": round((time.monotonic() - start) * 1000, 3), "available_cpus": len(os.sched_getaffinity(0)), "concurrent_shards": 2, "case_timeout_ms": 10000, "supervisor_timeout_ms": 10000, "outer_timeout_ms": 10000, "cwd": str(root), "TMPDIR": external_tmp, "fixture_residue": [str(p) for p in (root / ".tmp/process-tmp/escrow-performance-test").glob("perf-fixtures-*")], "shards": [{"name": name, "exit_code": child.returncode, "argv": argv} for name, child, argv, _, _ in children], "observations": observations}
(ev / "green-result.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps({k: v for k, v in result.items() if k != "observations"}, indent=2))
if any(child.returncode != 0 for _, child, *_ in children):
    raise SystemExit(1)
