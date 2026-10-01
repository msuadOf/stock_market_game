#!/usr/bin/env bash
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
if [[ ${1:-} == server ]]; then
  shift
  exec bash "$script_dir/server-build.sh" "$@"
fi
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  [[ $# == 1 ]] || { echo '[build] help does not accept additional arguments.' >&2; exit 1; }
fi
if [[ $# == 0 || ${1:-} == --help || ${1:-} == -h ]]; then
  printf '%s\n' 'Usage: scripts/build.sh <desktop|webui|webui-server|server> [--jobs N] [--dry-run] [--output target/build-artifacts/NAME]' \
    'Server needs Rust and timeout (gtimeout on macOS), not Node. UI build machines need Node, Corepack pnpm, Rust and wasm-pack.' \
    'Desktop builds native Tauri installers on the matching OS; no cross-platform installer promise.' \
    'Builds have a shared 300000ms deadline. Default existing outputs are refused; use --output for a fresh repeat build.' \
    'Compilation does not run full regression. CI regression gates are unchanged.'
  exit 0
fi
exec node "$script_dir/build-targets.mjs" "$@"
