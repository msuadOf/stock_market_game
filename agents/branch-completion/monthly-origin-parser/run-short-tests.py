# 本轮短测的原命令、源码指纹与进程外 deadline 收据；不运行 Cargo 或生成类型。
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone

root = pathlib.Path(__file__).resolve().parents[3]
phase = sys.argv[1]
if phase not in ("red", "green"):
    raise ValueError("阶段只允许 red 或 green")
output = pathlib.Path(__file__).resolve().parent
receipt_path = output / f"{phase}.receipt.json"
if receipt_path.exists():
    raise FileExistsError(receipt_path)
node = shutil.which("node")
files = ["apps/web/src/save/schema/company/monthly-origin.test.ts", "apps/web/src/save/schema/setup-company-options.test.ts", "apps/web/src/save/company-schema.test.ts"]
command = [node, "scripts/run-with-deadline.mjs", "10000", "--", node, "--test", "--test-timeout=10000", "--test-concurrency=3", *files]
sources = files + ["apps/web/src/save/schema/company/reports.ts", "apps/web/src/save/schema/setup-company-options.ts", "apps/web/src/save/schema/monthly-report-schedule.ts", "packages/engine/src/information/monthly_schedule.rs", "packages/engine/src/information/publication.rs", "scripts/run-with-deadline.mjs"]
sha = {name: hashlib.sha256((root / name).read_bytes()).hexdigest() if (root / name).exists() else None for name in sources}
started = datetime.now(timezone.utc).isoformat()
start = time.monotonic()
result = subprocess.run(command, cwd=root, capture_output=True)
wall_ms = round((time.monotonic() - start) * 1000, 3)
(output / f"{phase}.stdout.log").write_bytes(result.stdout)
(output / f"{phase}.stderr.log").write_bytes(result.stderr)
receipt = {"phase": phase, "started_at_utc": started, "cwd": str(root), "base_oid": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(), "node_version": subprocess.check_output([node, "--version"], text=True).strip(), "command": command, "process_tree_deadline_ms": 10000, "case_timeout_ms": 10000, "file_concurrency": 3, "source_sha256": sha, "exit_code": result.returncode, "wall_ms": wall_ms, "stdout": f"{phase}.stdout.log", "stderr": f"{phase}.stderr.log"}
receipt_path.write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(receipt, ensure_ascii=False, indent=2))
print(result.stdout.decode())
print(result.stderr.decode(), file=sys.stderr)
sys.exit(result.returncode)
