"""记录 benchmark 进程各线程 CPU 时间；不把服务器其他任务的占用算进引擎。"""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

topic = Path(__file__).resolve().parent
label, threads, cpus = sys.argv[1:4]
root = topic.parents[1]
command = ["taskset", "-c", cpus, str(root / ".tmp/perf-target/release/examples/max_speed_probe"),
           str(topic / "default-setup.json"), "3000", threads]
clock_ticks = os.sysconf("SC_CLK_TCK")
allocated_cpus = []
for part in cpus.split(","):
    endpoints = [int(value) for value in part.split("-")]
    allocated_cpus.extend(range(endpoints[0], endpoints[-1] + 1))

def system_cpu():
    rows = {}
    for source in Path("/proc/stat").read_text().splitlines():
        fields = source.split()
        if fields and fields[0].startswith("cpu") and fields[0][3:].isdigit():
            cpu = int(fields[0][3:])
            if cpu in allocated_cpus:
                values = [int(value) for value in fields[1:9]]
                rows[cpu] = {"total": sum(values), "idle": values[3] + values[4]}
    return rows

def sample(pid):
    rows = {}
    for path in Path(f"/proc/{pid}/task").glob("*/stat"):
        try:
            source = path.read_text()
        except FileNotFoundError:
            continue  # 两次系统调用之间已退出的线程没有后续样本。
        fields = source[source.rindex(")") + 2:].split()
        rows[int(path.parent.name)] = {"cpu_s": (int(fields[11]) + int(fields[12])) / clock_ticks,
                                      "processor": int(fields[36]), "state": fields[0]}
    return rows

def process_cpu(pid):
    try:
        source = Path(f"/proc/{pid}/stat").read_text()
    except FileNotFoundError:
        return None
    fields = source[source.rindex(")") + 2:].split()
    return (int(fields[11]) + int(fields[12])) / clock_ticks

samples = []
started = time.monotonic()
with (topic / f"{label}.json").open("w") as output, (topic / f"{label}.err").open("w") as error:
    process = subprocess.Popen(command, stdout=output, stderr=error)
    previous = sample(process.pid)
    previous_cpu = process_cpu(process.pid)
    previous_system = system_cpu()
    previous_at = time.monotonic()
    try:
        while process.poll() is None:
            if time.monotonic() - started > 295:
                raise TimeoutError("CPU probe 超过执行 deadline")
            time.sleep(1)
            now = time.monotonic()
            current = sample(process.pid)
            current_cpu = process_cpu(process.pid)
            current_system = system_cpu()
            duration = now - previous_at
            per_thread = [{"tid": tid, "cpu_pct": max(0, row["cpu_s"] - previous.get(tid, {"cpu_s": 0})["cpu_s"]) / duration * 100,
                           "processor": row["processor"], "state": row["state"]} for tid, row in current.items()]
            if current_cpu is None:
                break
            row = {"wall_s": now-started, "cpu_pct": (current_cpu-previous_cpu) / duration * 100,
                   "observed_thread_cpu_pct": sum(thread["cpu_pct"] for thread in per_thread),
                   "budget_used_pct": (current_cpu-previous_cpu) / duration * 100 / len(allocated_cpus),
                   "system_cpu_pct": {cpu: 100 * (1 - (value["idle"]-previous_system[cpu]["idle"]) / (value["total"]-previous_system[cpu]["total"]))
                      for cpu, value in current_system.items() if value["total"] > previous_system[cpu]["total"]},
                   "threads": len(current), "per_thread": sorted(per_thread, key=lambda thread: -thread["cpu_pct"])}
            samples.append(row)
            print(json.dumps({key: row[key] for key in ["wall_s", "cpu_pct", "threads"]}), flush=True)
            previous, previous_at, previous_cpu = current, now, current_cpu
            previous_system = current_system
        if process.returncode != 0:
            raise RuntimeError(f"benchmark exit={process.returncode}")
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=1)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=1)
        (topic / f"{label}-cpu.json").write_text(json.dumps({"command": command, "threads": int(threads),
               "cpu_affinity": cpus, "samples": samples}, indent=2))
