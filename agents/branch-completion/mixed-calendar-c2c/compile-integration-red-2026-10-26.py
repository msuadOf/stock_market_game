"""只读链接 root 密封 normal rlib；本脚本不构建 engine crate。"""

from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


REPOSITORY = Path("/data1/baiyifan/workplace/stock_market_game")
WORKTREE = Path(__file__).resolve().parents[3]
EVIDENCE = Path(__file__).resolve().parent / "integration-red"
OUTPUT = REPOSITORY / ".tmp/mixed-calendar-c2c"
BINDING = REPOSITORY / "agents/branch-completion/main-current-normal-rlib-2026-10-26/artifact-binding.json"
SOURCE = REPOSITORY / "agents/branch-completion/main-current-normal-rlib-2026-10-26/engine-source-before.json"
DEPS = REPOSITORY / ".tmp/main-current-normal-rlib-2026-10-26/target/debug/deps"
RUSTC = "/home/baiyifan/.rustup/toolchains/1.96.1-x86_64-unknown-linux-gnu/bin/rustc"
NODE = REPOSITORY / ".tmp/node24-env/node"
FILES = [
    "packages/engine/tests/mixed_calendar_c2c.rs",
    "packages/engine/tests/common/mixed_calendar_c2c_fixture.rs",
]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(name, value):
    (EVIDENCE / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def source_hashes():
    return {name: digest(WORKTREE / name) for name in FILES}


def verify_input(item):
    name = item["path"]
    return name, digest(WORKTREE / name) == item["sha256"]


EVIDENCE.mkdir(parents=True, exist_ok=True)
OUTPUT.mkdir(parents=True, exist_ok=True)
binding = json.loads(BINDING.read_text())
artifact = next(item for item in binding["artifacts"] if item["normal_rlib"])
assert binding["head"] == "6ac167e1d98d1a56b1307822f311d0277d624003"
assert digest(Path(artifact["sealed"])) == artifact["sha256"]
inputs = json.loads(SOURCE.read_text())["files"]
with ThreadPoolExecutor(max_workers=8) as workers:
    verified = list(workers.map(verify_input, inputs))
assert all(ok for _, ok in verified), [name for name, ok in verified if not ok]
binary = OUTPUT / "integration-red-6ac167e1"
assert not binary.exists(), "禁止覆盖此前产物；失败保留后须另行安排"
command = [
    str(NODE), str(REPOSITORY / "scripts/run-with-deadline.mjs"), "10000", "--",
    RUSTC, "--edition=2021", "--test", str(WORKTREE / FILES[0]),
    "--extern", "engine=" + artifact["sealed"],
    "-L", "dependency=" + str(DEPS),
    "-C", "codegen-units=8", "-C", "debuginfo=0", "-C", "strip=debuginfo",
    "-C", "link-arg=-Wl,--threads=8", "-o", str(binary),
]
before = source_hashes()
plan = {
    "说明": "只重编译 integration test，engine 使用 root 已授权同源 normal rlib；无 Cargo。",
    "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=WORKTREE, text=True).strip(),
    "engine_binding": str(BINDING), "binding_sha256": digest(BINDING),
    "engine_source_manifest": str(SOURCE), "source_manifest_sha256": digest(SOURCE),
    "verified_input_count": len(verified), "verification_threads": 8,
    "engine_artifact": artifact, "command": command, "cwd": str(WORKTREE),
    "deadline_ms": 10000, "codegen_units": 8, "linker_threads": 8,
    "test_source_before": before,
}
write("compile-plan.json", plan)
environment = os.environ.copy()
environment["TMPDIR"] = str(OUTPUT)
start = time.monotonic()
samples = []
with (EVIDENCE / "compile.stdout").open("w") as stdout, (EVIDENCE / "compile.stderr").open("w") as stderr:
    process = subprocess.Popen(command, cwd=WORKTREE, env=environment, stdout=stdout, stderr=stderr)
    while process.poll() is None:
        listing = subprocess.run(["ps", "-eo", "pid=,ppid=,pcpu=,nlwp=,args="], capture_output=True, text=True, timeout=1)
        assert listing.returncode == 0, listing.stderr
        rows = [line for line in listing.stdout.splitlines() if "mixed-calendar-c2c" in line or "--threads=8" in line]
        samples.append({"elapsed_seconds": time.monotonic() - start, "rows": rows})
        time.sleep(0.05)
    code = process.wait()
elapsed = time.monotonic() - start
after = source_hashes()
result = {
    "exit_code": code, "wall_seconds": elapsed, "test_source_after": after,
    "sources_unchanged": before == after, "samples": samples,
    "binary": {"path": str(binary), "bytes": binary.stat().st_size, "sha256": digest(binary)} if binary.exists() else None,
}
write("compile-result.json", result)
print(json.dumps({key: value for key, value in result.items() if key != "samples"}, ensure_ascii=False, indent=2))
assert code == 0 and before == after, "真实编译未成功，须读原件，不作为领域红"
