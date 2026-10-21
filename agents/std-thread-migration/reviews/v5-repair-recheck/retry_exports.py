"""ts-rs 递归 export 写同一依赖文件：隔离重试超时项，保留原始证据。"""
import collections
import json
import os
from pathlib import Path
import signal
import subprocess
import time

OUT = Path(__file__).resolve().parent
ROOT = OUT.parents[3]
START = time.monotonic()
binary = (OUT / "engine-binary.txt").read_text()
cases = [json.loads(line) for line in (OUT / "cases.jsonl").read_text().splitlines()]
with (OUT / "export-retries.jsonl").open("w") as log:
    for x in cases:
        if x["rc"] != 124:
            continue
        assert "export_bindings" in x["case"]
        assert time.monotonic()-START < 298
        env = dict(os.environ, ENGINE_STEP_RUNTIME=x["group"][-1])
        t = time.monotonic()
        p = subprocess.Popen([binary, x["case"], "--exact", "--test-threads=1"], cwd=ROOT,
                             env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            output = p.communicate(timeout=min(9.5, 298-(time.monotonic()-START)))[0]
            rc = p.returncode
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            output = p.communicate()[0]
            rc = 124
        x.update(rc=rc, output=output.decode(errors="replace"), seconds=round(time.monotonic()-t, 3))
        log.write(json.dumps(x, ensure_ascii=False)+"\n")
        log.flush()
        print(x["group"], x["case"], rc, x["seconds"], flush=True)
counts = collections.defaultdict(collections.Counter)
original = json.loads((OUT / "summary.json").read_text())
for g, stats in original["counts"].items():
    counts[g]["ignored"] = stats["ignored"]
for x in cases:
    status = "passed" if x["rc"] == 0 else "timeout" if x["rc"] == 124 else "failed"
    counts[x["group"]][status] += 1
    if x["group"] == "lib-1":
        counts["engine"][status] += 1
summary = dict(counts={k:dict(v) for k,v in counts.items()}, retry_seconds=round(time.monotonic()-START, 3))
(OUT / "final-summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2)+"\n")
print(json.dumps(summary, ensure_ascii=False), flush=True)
