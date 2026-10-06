"""核对普通 server 探针的源字节、受控依赖与 Cargo 实际 artifact feature。"""
from pathlib import Path
import hashlib
import json
import sys
import tomllib

probe = Path(__file__).resolve().parent
root = probe.parents[2]


def require(condition, detail):
    if not condition:
        raise RuntimeError(f"production-server-probe 构建证据无效：{detail}")


require(len(sys.argv) == 2, "用法：verify-build.py <cargo-build.jsonl>")
log = Path(sys.argv[1])
rows = [json.loads(line) for line in log.read_text().splitlines()]
require(bool(rows), f"构建日志为空：{log}")
require(rows[-1].get("reason") == "build-finished" and rows[-1].get("success") is True,
        "Cargo 未报告构建成功；仍须由调用方确认构建进程 exit0")


def artifact(name):
    matches = [row for row in rows if row.get("reason") == "compiler-artifact"
               and row["target"]["name"] == name]
    require(len(matches) == 1, f"必须有且仅有一个 {name} artifact，实际 {len(matches)}")
    return matches[0]


engine = artifact("engine")
server = artifact("server")
tokio = artifact("tokio")
binary_artifact = artifact("production-server-max-probe")
require(engine["features"] == [], f"engine 开启了 {engine['features']}")
require(server["features"] == [], f"server 开启了 {server['features']}")
require("test-util" not in tokio["features"], "Tokio 开启 test-util")
require(binary_artifact["target"]["kind"] == ["bin"], "探针必须为普通 binary")
require(binary_artifact["profile"]["opt_level"] == "3", "探针必须使用 release opt-level 3")
require(Path(binary_artifact["target"]["src_path"]).resolve() == probe / "src/main.rs",
        "binary 并非来自本探针 main.rs")
original_source = (root / "apps/server/examples/server_max_speed_probe.rs").read_bytes()
require(original_source == (probe / "src/main.rs").read_bytes(), "main.rs 与既有真实 actor 探针漂移")


def locked_packages(path):
    packages = tomllib.loads(path.read_text())["package"]
    return {(row["name"], row["version"], row.get("source", ""), row.get("checksum", ""))
            for row in packages}


extra = locked_packages(probe / "Cargo.lock") - locked_packages(root / "Cargo.lock")
require(extra == {("production-server-max-probe", "0.0.0", "", "")},
        f"依赖版本/source/checksum 与 workspace lock 不一致：{sorted(extra)}")
binary = Path(binary_artifact["executable"])
require(binary.is_file(), f"binary 不存在：{binary}")
# 旧 Cargo 日志不能用于验证源文件改动后的 binary。
inputs = [root / "Cargo.toml", root / "Cargo.lock", probe / "Cargo.toml", probe / "Cargo.lock", probe / "src/main.rs"]
for package in [root / "packages/engine", root / "apps/server"]:
    inputs.append(package / "Cargo.toml")
    inputs.extend((package / "src").rglob("*.rs"))
newer = [str(path.relative_to(root)) for path in inputs if path.stat().st_mtime_ns > binary.stat().st_mtime_ns]
require(not newer, f"binary 早于当前源码/配置，需要重编：{newer[:5]}")
print(json.dumps({
    "engine_features": engine["features"], "server_features": server["features"],
    "tokio_features": tokio["features"], "target_kind": binary_artifact["target"]["kind"],
    "source_sha256": hashlib.sha256(original_source).hexdigest(),
    "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
    "binary": str(binary), "dependency_versions_match_root_lock": True,
    "build_finished_success": True,
}, indent=2))
