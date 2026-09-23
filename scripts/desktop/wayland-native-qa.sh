#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
readonly REPO_ROOT
readonly EVIDENCE_ROOT="${WAYLAND_EVIDENCE_ROOT:-$REPO_ROOT/.omo/evidence/resolve-blockers-wayland}"
RUN_ID=""
EVIDENCE_DIR=""
readonly APP_BINARY="${WAYLAND_APP_BINARY:-$REPO_ROOT/target/debug/stock-market-game}"
readonly WIDTH=1280
readonly HEIGHT=800
readonly WAIT_SECONDS=35
readonly INSPECTOR_PORT=9230
readonly SOCKET_PREFIX="rbw-wayland"

weston_pid=""
app_pid=""
runtime_dir=""
config_dir=""
capture_dir=""
run_completed=false

fail() {
  printf '%s\n' "$1" >&2
  exit 1
}

allocate_evidence_dir() {
  local base_id=""
  local candidate=""
  local sequence=0

  if [[ -n "${WAYLAND_EVIDENCE_DIR+x}" ]]; then
    [[ -n "$WAYLAND_EVIDENCE_DIR" ]] || fail "WAYLAND_EVIDENCE_DIR must not be empty"
    if ! mkdir -m 700 "$WAYLAND_EVIDENCE_DIR"; then
      fail "refusing to overwrite existing evidence directory: $WAYLAND_EVIDENCE_DIR"
    fi
    EVIDENCE_DIR="$WAYLAND_EVIDENCE_DIR"
    RUN_ID="$(basename "$EVIDENCE_DIR")"
    return
  fi

  mkdir -p "$EVIDENCE_ROOT"
  if [[ -n "${WAYLAND_RUN_ID+x}" ]]; then
    [[ "$WAYLAND_RUN_ID" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || fail "WAYLAND_RUN_ID must use only letters, digits, dot, underscore, or hyphen"
    candidate="$EVIDENCE_ROOT/task-3-wayland.run-$WAYLAND_RUN_ID"
    if ! mkdir -m 700 "$candidate"; then
      fail "refusing to overwrite existing evidence directory: $candidate"
    fi
    RUN_ID="$WAYLAND_RUN_ID"
    EVIDENCE_DIR="$candidate"
    return
  fi

  base_id="$(date -u +%Y%m%d-%H%M%S)"
  while ((sequence < 100)); do
    RUN_ID="$base_id"
    if ((sequence > 0)); then
      RUN_ID="$base_id-$sequence"
    fi
    candidate="$EVIDENCE_ROOT/task-3-wayland.run-$RUN_ID"
    if mkdir -m 700 "$candidate"; then
      EVIDENCE_DIR="$candidate"
      return
    fi
    if [[ ! -e "$candidate" && ! -L "$candidate" ]]; then
      fail "could not allocate evidence directory: $candidate"
    fi
    ((sequence += 1))
  done
  fail "could not allocate a unique evidence directory after 100 attempts under $EVIDENCE_ROOT"
}

cleanup() {
  local status=$?
  trap - EXIT
  local app_alive=false
  local weston_alive=false
  local matching_processes=""

  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill "$app_pid"
    wait "$app_pid" || true
  fi
  if [[ -n "$weston_pid" ]] && kill -0 "$weston_pid" 2>/dev/null; then
    kill "$weston_pid"
    wait "$weston_pid" || true
  fi
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    app_alive=true
  fi
  if [[ -n "$weston_pid" ]] && kill -0 "$weston_pid" 2>/dev/null; then
    weston_alive=true
  fi
  matching_processes="$(pgrep -a -f 'weston|stock-market-game|WebKitWebProcess|WebKitNetworkProcess|WebKitGPUProcess' || true)"
  rm -rf "$runtime_dir" "$config_dir" "$capture_dir"
  cat >"$EVIDENCE_DIR/task-3-wayland.cleanup.txt" <<EOF
app_pid=$app_pid
weston_pid=$weston_pid
app_alive_after_cleanup=$app_alive
weston_alive_after_cleanup=$weston_alive
runtime_dir_removed=$([[ ! -e "$runtime_dir" ]] && printf true || printf false)
config_dir_removed=$([[ ! -e "$config_dir" ]] && printf true || printf false)
matching_processes_after_cleanup=${matching_processes:-none}
EOF
  if [[ "$run_completed" == true && "$status" -eq 0 ]]; then
    if ! node "$REPO_ROOT/scripts/desktop/wayland-evidence-integrity.mjs" generate "$EVIDENCE_DIR"; then
      status=1
    elif ! node "$REPO_ROOT/scripts/desktop/wayland-evidence-integrity.mjs" verify "$EVIDENCE_DIR"; then
      status=1
    fi
  fi
  exit "$status"
}

stop_app() {
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill "$app_pid"
    wait "$app_pid" || true
  fi
  app_pid=""
}

launch_app() {
  local binary="$1"
  local log="$2"
  local receipt="$3"
  env -u DBUS_SESSION_BUS_ADDRESS -u DESKTOP_SESSION -u XDG_CURRENT_DESKTOP -u GSETTINGS_BACKEND \
    GDK_BACKEND=wayland WAYLAND_DISPLAY="$SOCKET_PREFIX-final" XDG_RUNTIME_DIR="$runtime_dir" \
    XDG_CONFIG_HOME="$config_dir" WEBKIT_DISABLE_COMPOSITING_MODE=1 LIBGL_ALWAYS_SOFTWARE=1 \
    WEBKIT_INSPECTOR_HTTP_SERVER="127.0.0.1:$INSPECTOR_PORT" \
    timeout "$((WAIT_SECONDS + 15))" "$binary" >"$log" 2>&1 &
  app_pid=$!
  local child_pid=""
  local attempts_remaining=100
  while ((attempts_remaining > 0)); do
    child_pid="$(pgrep -P "$app_pid" || true)"
    [[ -n "$child_pid" ]] && break
    sleep 0.1
    ((attempts_remaining -= 1))
  done
  {
    printf 'binary=%s\n' "$binary"
    printf 'launcher_pid=%s\n' "$app_pid"
    printf 'child_pid=%s\n' "${child_pid:-missing}"
    printf 'expected_GDK_BACKEND=wayland\n'
    printf 'expected_WAYLAND_DISPLAY=%s\n' "$SOCKET_PREFIX-final"
    printf 'expected_XDG_RUNTIME_DIR=%s\n' "$runtime_dir"
    printf 'expected_XDG_CONFIG_HOME=%s\n' "$config_dir"
    printf 'expected_WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:%s\n' "$INSPECTOR_PORT"
    for process_pid in "$app_pid" $child_pid; do
      [[ -d "/proc/$process_pid" ]] || continue
      printf 'process_%s_cmdline=' "$process_pid"
      tr '\0' ' ' <"/proc/$process_pid/cmdline"
      printf '\nprocess_%s_environment:\n' "$process_pid"
      tr '\0' '\n' <"/proc/$process_pid/environ" | rg '^(GDK_BACKEND|WAYLAND_DISPLAY|XDG_RUNTIME_DIR|XDG_CONFIG_HOME|WEBKIT_INSPECTOR_HTTP_SERVER)='
    done
  } >"$receipt"
}

run_actor_tests() {
  local output="$1"
  shift
  env -u DBUS_SESSION_BUS_ADDRESS -u DESKTOP_SESSION -u XDG_CURRENT_DESKTOP -u GSETTINGS_BACKEND \
    GDK_BACKEND=wayland WAYLAND_DISPLAY="$SOCKET_PREFIX-final" XDG_RUNTIME_DIR="$runtime_dir" \
    XDG_CONFIG_HOME="$config_dir" WEBKIT_DISABLE_COMPOSITING_MODE=1 \
    cargo test -p stock-market-game "$@" >"$output" 2>&1
}

run_engine_bounded_test() {
  env -u DBUS_SESSION_BUS_ADDRESS -u DESKTOP_SESSION -u XDG_CURRENT_DESKTOP -u GSETTINGS_BACKEND \
    GDK_BACKEND=wayland WAYLAND_DISPLAY="$SOCKET_PREFIX-final" XDG_RUNTIME_DIR="$runtime_dir" \
    XDG_CONFIG_HOME="$config_dir" WEBKIT_DISABLE_COMPOSITING_MODE=1 \
    cargo test -p engine --features simulation-diagnostics retains_the_latest_exactly_128_records -- --nocapture \
    >"$EVIDENCE_DIR/task-3-wayland.engine-bounded.txt" 2>&1
}

wait_for_socket() {
  local socket_path="$1"
  local retries_remaining=100
  while ((retries_remaining > 0)); do
    [[ -S "$socket_path" ]] && return 0
    sleep 0.1
    ((retries_remaining -= 1))
  done
  printf 'Weston socket was not created: %s\n' "$socket_path" >&2
  return 1
}

latest_capture() {
  local directory="$1"
  local captures=()
  shopt -s nullglob
  captures=("$directory"/*.png)
  shopt -u nullglob
  if ((${#captures[@]} == 0)); then
    return 1
  fi
  printf '%s\n' "${captures[${#captures[@]} - 1]}"
}

cd "$REPO_ROOT"
allocate_evidence_dir
trap cleanup EXIT
cat >"$EVIDENCE_DIR/task-3-wayland.run.txt" <<EOF
run_id=$RUN_ID
evidence_dir=$EVIDENCE_DIR
EOF
git status --short >"$EVIDENCE_DIR/task-3-wayland.worktree-status.txt"
if [[ "$APP_BINARY" == "$REPO_ROOT/target/debug/stock-market-game" ]]; then
  cargo build -p stock-market-game
fi
if [[ ! -x "$APP_BINARY" ]]; then
  printf 'missing desktop binary: %s\n' "$APP_BINARY" >&2
  exit 1
fi

printf 'renderer\tdimensions\trenderer_evidence\tadvertised_mode\tsocket\twayland_info\tscreenshooter_exit\tcapture\n' >"$EVIDENCE_DIR/task-3-wayland.probe-matrix.tsv"
for renderer in pixman default; do
  for dimensions in explicit default; do
    bash "$REPO_ROOT/scripts/desktop/wayland-probe.sh" "$EVIDENCE_DIR" "$WIDTH" "$HEIGHT" "$SOCKET_PREFIX" "$renderer" "$dimensions"
  done
done

runtime_dir="$(mktemp -d /tmp/rbw-wayland-final.XXXXXX)"
config_dir="$(mktemp -d /tmp/rbw-wayland-config.XXXXXX)"
chmod 700 "$runtime_dir" "$config_dir"
cat >"$EVIDENCE_DIR/task-3-wayland.command.txt" <<EOF
weston --backend=headless --renderer=pixman --width=$WIDTH --height=$HEIGHT --scale=1 --refresh-rate=60000 --debug --socket=$SOCKET_PREFIX-final --no-config
GDK_BACKEND=wayland WAYLAND_DISPLAY=$SOCKET_PREFIX-final XDG_RUNTIME_DIR=<isolated> XDG_CONFIG_HOME=<isolated> target/debug/stock-market-game
GDK_BACKEND=wayland WAYLAND_DISPLAY=$SOCKET_PREFIX-final cargo test -p stock-market-game -- --nocapture
GDK_BACKEND=wayland WAYLAND_DISPLAY=$SOCKET_PREFIX-final cargo test -p stock-market-game --release -- --nocapture
GDK_BACKEND=wayland WAYLAND_DISPLAY=$SOCKET_PREFIX-final cargo test -p stock-market-game --features simulation-diagnostics -- --nocapture
EOF

XDG_RUNTIME_DIR="$runtime_dir" weston \
  --backend=headless --renderer=pixman --width="$WIDTH" --height="$HEIGHT" --scale=1 \
  --refresh-rate=60000 --debug --socket="$SOCKET_PREFIX-final" --no-config \
  --log="$EVIDENCE_DIR/task-3-wayland.weston.log" \
  >"$EVIDENCE_DIR/task-3-wayland.weston.stdout.log" 2>&1 &
weston_pid=$!
wait_for_socket "$runtime_dir/$SOCKET_PREFIX-final"
XDG_RUNTIME_DIR="$runtime_dir" WAYLAND_DISPLAY="$SOCKET_PREFIX-final" \
  wayland-info >"$EVIDENCE_DIR/task-3-wayland.wayland-info.txt" 2>&1
rg -F "width: $WIDTH px, height: $HEIGHT px" "$EVIDENCE_DIR/task-3-wayland.wayland-info.txt"
rg -F "Using Pixman renderer" "$EVIDENCE_DIR/task-3-wayland.weston.log"

capture_dir="$EVIDENCE_DIR/task-3-wayland.capture-source"
rm -rf "$capture_dir"
mkdir -p "$capture_dir"
run_actor_tests "$EVIDENCE_DIR/task-3-wayland.actor-default.txt" -- --nocapture
run_actor_tests "$EVIDENCE_DIR/task-3-wayland.actor-release.txt" --release -- --nocapture
launch_app "$APP_BINARY" "$EVIDENCE_DIR/task-3-wayland.tauri-default.log" "$EVIDENCE_DIR/task-3-wayland.environment-default.txt"
node "$REPO_ROOT/scripts/desktop/wayland-ipc-probe.mjs" --port "$INSPECTOR_PORT" --expect unsupported \
  >"$EVIDENCE_DIR/task-3-wayland.ipc-default.json"
stop_app

if [[ "$APP_BINARY" == "$REPO_ROOT/target/debug/stock-market-game" ]]; then
  cargo build -p stock-market-game --features simulation-diagnostics
  feature_app_binary="$APP_BINARY"
else
  feature_app_binary="${WAYLAND_FEATURE_APP_BINARY:-}"
  if [[ ! -x "$feature_app_binary" ]]; then
    printf 'WAYLAND_FEATURE_APP_BINARY must name an executable diagnostic build when WAYLAND_APP_BINARY is overridden\n' >&2
    exit 1
  fi
fi

run_actor_tests "$EVIDENCE_DIR/task-3-wayland.actor-feature.txt" --features simulation-diagnostics -- --nocapture
run_engine_bounded_test
launch_app "$feature_app_binary" "$EVIDENCE_DIR/task-3-wayland.tauri-feature.log" "$EVIDENCE_DIR/task-3-wayland.environment-feature.txt"
node "$REPO_ROOT/scripts/desktop/wayland-ipc-probe.mjs" --port "$INSPECTOR_PORT" --expect supported \
  >"$EVIDENCE_DIR/task-3-wayland.ipc-feature.json"
sleep "$WAIT_SECONDS"
kill -0 "$app_pid"
printf 'attempt\texit\tcapture_count\n' >"$EVIDENCE_DIR/task-3-wayland.capture-attempts.tsv"
for capture_attempt in 1 2; do
  if XDG_RUNTIME_DIR="$runtime_dir" XDG_PICTURES_DIR="$capture_dir" WAYLAND_DISPLAY="$SOCKET_PREFIX-final" \
    timeout 20s weston-screenshooter >>"$EVIDENCE_DIR/task-3-wayland.screenshooter.log" 2>&1; then
    capture_exit=0
  else
    capture_exit=$?
  fi
  shopt -s nullglob
  captures=("$capture_dir"/*.png)
  shopt -u nullglob
  printf '%s\t%s\t%s\n' "$capture_attempt" "$capture_exit" "${#captures[@]}" \
    >>"$EVIDENCE_DIR/task-3-wayland.capture-attempts.tsv"
  ((${#captures[@]} > 0)) && break
  sleep 1
done
capture="$(latest_capture "$capture_dir" || true)"
if [[ -z "$capture" ]]; then
  printf 'Weston screenshooter produced no final PNG\n' >&2
  exit 1
fi
mv "$capture" "$EVIDENCE_DIR/task-3-wayland.capture.png"
rm -rf "$capture_dir"
uv run "$REPO_ROOT/scripts/desktop/validate-wayland-png.py" "$EVIDENCE_DIR/task-3-wayland.capture.png" "$WIDTH" "$HEIGHT" \
  >"$EVIDENCE_DIR/task-3-wayland.capture-validation.txt"
run_completed=true
