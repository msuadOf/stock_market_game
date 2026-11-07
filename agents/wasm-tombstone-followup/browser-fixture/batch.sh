#!/bin/sh
set -eu
# 外部 supervisor 必须将整批及进程树限制在 300 秒内。
: "${WASM_PANIC_SYSROOT:?必须指定受控 sysroot}"
: "${CARGO_TARGET_DIR:?必须指定独立 Cargo target 目录}"
: "${WASM_PKG:?必须指定验证包目录}"
: "${CARGO_BUILD_JOBS:?必须显式指定 Cargo 并发数}"
if [ -n "${RUSTC_WRAPPER:-}" ] || [ -n "${RUSTC_WORKSPACE_WRAPPER:-}" ]; then
  printf '%s\n' '已有 rustc wrapper，拒绝覆盖' >&2
  exit 1
fi
if [ -e agents/wasm-tombstone-followup/browser-fixture/pkg ] || [ -L agents/wasm-tombstone-followup/browser-fixture/pkg ]; then
  printf '%s\n' 'fixture pkg 已存在，拒绝覆盖' >&2
  exit 1
fi
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/stock-w2-fixture.XXXXXX")
cleanup() {
  if [ -L agents/wasm-tombstone-followup/browser-fixture/pkg ]; then rm agents/wasm-tombstone-followup/browser-fixture/pkg; fi
  rm -rf "$work_dir"
}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
export RUSTUP_TOOLCHAIN=nightly-2026-09-05
rustc scripts/wasm-panic/rustc-wrapper.rs -o "$work_dir/rustc-wrapper"
export RUSTC_WRAPPER="$work_dir/rustc-wrapper"
manifest() {
  { printf '%s\0' Cargo.toml Cargo.lock rust-toolchain.toml; find .cargo packages/engine apps/web-wasm apps/web/src scripts/wasm-panic agents/wasm-tombstone-followup/browser-fixture -type f ! -path '*/node_modules/*' ! -path '*/target/*' -print0; } | sort -z | xargs -0 sha256sum
}
manifest > "$work_dir/before.sha256"
printf 'release verification-harness：CARGO_BUILD_JOBS=%s，共享 wall-clock deadline=300s\n' "$CARGO_BUILD_JOBS"
wasm-pack build apps/web-wasm --target web --release --out-dir "$WASM_PKG" --features verification-harness &
compile_pid=$!
while kill -0 "$compile_pid" 2>/dev/null; do
  ps -eo pid,ppid,pgid,stat,pcpu,nlwp,comm >> "$work_dir/cpu.log"
  sleep 3
done &
sample_pid=$!
wait "$compile_pid" || { kill "$sample_pid" 2>/dev/null || :; wait "$sample_pid" 2>/dev/null || :; exit 1; }
kill "$sample_pid" 2>/dev/null || :
wait "$sample_pid" 2>/dev/null || :
node agents/wasm-tombstone-followup/browser-fixture/run.mjs
manifest > "$work_dir/after.sha256"
cmp "$work_dir/before.sha256" "$work_dir/after.sha256"
