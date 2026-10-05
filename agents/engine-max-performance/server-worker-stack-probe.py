"""由 GDB 自己启动引擎子进程，采样调用栈；调试耗时不作为性能结果。"""
from pathlib import Path
import queue
import subprocess
import threading
import time
import sys

topic = Path(__file__).resolve().parent
root = topic.parents[1]
label = sys.argv[2] if len(sys.argv) > 2 else "server-worker-stack"
command = ["taskset", "-c", "0-15", "gdb", "-q", "-nx", "--interpreter=mi2", "--args",
           str(root / sys.argv[1]) if len(sys.argv) > 1 else str(root / ".tmp/server-proof-baseline-5bc20409"),
           str(topic / "default-setup.json"), "10000", "16"]
process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
messages = queue.Queue()
raw = (topic / f"{label}-raw.log").open("w")

def read_output():
    for line in process.stdout:
        raw.write(line)
        raw.flush()
        messages.put(line.rstrip())

reader = threading.Thread(target=read_output, daemon=True)
reader.start()
token = 0

def request(command):
    global token
    token += 1
    request_id = token
    process.stdin.write(f"{request_id}{command}\n")
    process.stdin.flush()
    until = time.monotonic() + 10
    while time.monotonic() < until:
        line = messages.get(timeout=max(0.01, until-time.monotonic()))
        if line.startswith(f"{request_id}^"):
            if line.startswith(f"{request_id}^error"):
                raise RuntimeError(line)
            return line
    raise TimeoutError(f"GDB 请求超时：{command}")

try:
    request("-gdb-set pagination off")
    request("-gdb-set mi-async on")
    request("-gdb-set print thread-events off")
    request("-exec-run")
    time.sleep(6)
    with (topic / f"{label}-samples.log").open("w") as samples:
        for index in range(48):
            request("-exec-interrupt --all")
            until = time.monotonic() + 10
            while True:
                line = messages.get(timeout=max(0.01, until-time.monotonic()))
                if line.startswith("*stopped"):
                    break
            for thread in list(range(2, 20)):
                result = request(f"-stack-list-frames --thread {thread} 0 12")
                samples.write(f"sample={index} thread={thread} {result}\n")
                samples.flush()
            request("-exec-continue --all")
            time.sleep(0.15)
    request("-gdb-exit")
finally:
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=1)
    reader.join(timeout=1)
    raw.close()
