#!/usr/bin/env bash
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
root=$(cd -- "$script_dir/.." && pwd)
cd "$root"
jobs=$(getconf _NPROCESSORS_ONLN)
dry_run=false
output=target/build-artifacts/server
jobs_seen=false
output_seen=false
arguments=("$@")
if [[ $# == 1 && ( $1 == --help || $1 == -h ) ]]; then
  echo 'Usage: scripts/server-build.sh [--jobs N] [--dry-run] [--output target/build-artifacts/NAME]'; exit 0
fi
while [[ $# -gt 0 ]]; do
  case "$1" in
    --jobs)
      [[ $# -ge 2 && $jobs_seen == false && $2 =~ ^[1-9][0-9]{0,5}$ ]] || { echo '[build server] --jobs requires a positive integer (maximum 999999), once.' >&2; exit 1; }
      jobs=$2; jobs_seen=true; shift 2 ;;
    --output)
      [[ $# -ge 2 && $output_seen == false ]] || { echo '[build server] --output requires a directory, once.' >&2; exit 1; }
      output=$2; output_seen=true; shift 2 ;;
    --dry-run)
      [[ $dry_run == false ]] || { echo '[build server] duplicate option: --dry-run.' >&2; exit 1; }
      dry_run=true; shift ;;
    *) echo "[build server] unknown argument: $1" >&2; exit 1 ;;
  esac
done
[[ $jobs =~ ^[1-9][0-9]*$ ]] || { echo '[build server] cannot detect CPU jobs; specify --jobs N.' >&2; exit 1; }
[[ $output == "$root/"* ]] && output=${output#"$root/"}
[[ $output =~ ^target/build-artifacts/[a-zA-Z0-9][a-zA-Z0-9_-]*$ ]] || { echo '[build server] --output must be a direct child: target/build-artifacts/NAME.' >&2; exit 1; }
printf '%s\n' "[build server] jobs=$jobs; shared deadline=300000ms; worker stops by 298000ms, tree termination by 299000ms, final 1000ms reserved for cleanup." \
  'Build requirements: Rust/Cargo, GNU timeout + GNU mv (macOS: gtimeout + gmv/Coreutils). Deployment: native executable + LICENSE; no build tools.' \
  'Native target command: rustc -vV (host field); dry-run does not probe Rust.' \
  "CARGO_TARGET_DIR=$root/target/build-cache/server cargo build --locked --release -p server --bin server --jobs $jobs --target <rustc-host>" \
  "Artifact: $root/$output/server + LICENSE" "Start: $root/$output/server --services server" \
  'Default existing outputs are refused without deletion; use --output target/build-artifacts/NAME for repeat builds.'
if [[ $dry_run == true ]]; then echo 'Mode: dry-run (no build commands executed; no artifact created).'; exit 0; fi
deadline_command=timeout
if ! command -v timeout >/dev/null; then deadline_command=gtimeout; fi
command -v "$deadline_command" >/dev/null || { echo '[build server] GNU timeout is required (macOS: gtimeout).' >&2; exit 1; }
move_command=mv
if ! mv --version >/dev/null 2>&1; then move_command=gmv; fi
command -v "$move_command" >/dev/null || { echo '[build server] GNU mv is required for atomic no-clobber publishing (macOS: gmv).' >&2; exit 1; }
phase=${STOCK_SERVER_BUILD_PHASE:-entry}
if [[ $phase == entry ]]; then
  exec env STOCK_SERVER_BUILD_PHASE=supervisor "$deadline_command" --signal=KILL 300s "$BASH" "$script_dir/server-build.sh" "${arguments[@]}"
fi
for directory in target target/build-cache target/build-cache/server target/build-work target/build-artifacts; do
  [[ ! -L $directory && ( ! -e $directory || -d $directory ) ]] || { echo "[build server] unsafe directory (symlink or non-directory): $directory" >&2; exit 1; }
  mkdir -p "$directory"
done
[[ ! -e $output && ! -L $output ]] || { echo "[build server] refusing existing output: $output; choose --output target/build-artifacts/NAME." >&2; exit 1; }
if [[ $phase == supervisor ]]; then
  work="$root/target/build-work/server-$(date +%s)-$$"
  mkdir "$work"
  cleanup() { rm -rf -- "$work"; }
  trap cleanup EXIT
  mkdir "$work/artifact"
  status=0
  env STOCK_SERVER_BUILD_PHASE=worker STOCK_SERVER_BUILD_WORK="$work" "$deadline_command" --signal=TERM --kill-after=1s 298s "$BASH" "$script_dir/server-build.sh" "${arguments[@]}" || status=$?
  if [[ $status != 0 ]]; then echo "[build server] worker failed or exhausted its deadline (exit $status); no incomplete package is published." >&2; fi
  exit "$status"
fi
[[ $phase == worker && ${STOCK_SERVER_BUILD_WORK:-} =~ ^"$root"/target/build-work/server-[0-9]+-[0-9]+$ ]] || { echo '[build server] internal worker requires the external deadline supervisor.' >&2; exit 1; }
work=$STOCK_SERVER_BUILD_WORK
export CARGO_TARGET_DIR="$root/target/build-cache/server"
export CARGO_BUILD_JOBS="$jobs"
version=$(rustc -vV)
native_target=
while IFS= read -r line; do
  if [[ $line == 'host: '* ]]; then
    [[ -z $native_target ]] || { echo '[build server] duplicate rustc host field.' >&2; exit 1; }
    native_target=${line#host: }
  fi
done <<< "$version"
[[ $native_target =~ ^[a-zA-Z0-9_-]+$ ]] || { echo '[build server] rustc -vV did not report a valid native host.' >&2; exit 1; }
cargo build --locked --release -p server --bin server --jobs "$jobs" --target "$native_target"
executable="$CARGO_TARGET_DIR/$native_target/release/server"
[[ -f $executable && ! -L $executable && -s $executable ]] || { echo "[build server] missing/unsafe native release executable: $executable" >&2; exit 1; }
[[ -f LICENSE && ! -L LICENSE && -s LICENSE ]] || { echo '[build server] root LICENSE missing/unsafe.' >&2; exit 1; }
cp -p "$executable" "$work/artifact/server"
cp -p LICENSE "$work/artifact/LICENSE"
[[ ! -e $output && ! -L $output ]] || { echo "[build server] refusing output created during build: $output" >&2; exit 1; }
"$move_command" -T -n -- "$work/artifact" "$output"
[[ ! -e $work/artifact ]] || { echo "[build server] refusing concurrent output: $output; atomic no-clobber move was skipped." >&2; exit 1; }
echo "[build server] native executable published: $root/$output/server"
