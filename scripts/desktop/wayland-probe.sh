#!/usr/bin/env bash
set -euo pipefail

readonly EVIDENCE_DIR="$1"
readonly WIDTH="$2"
readonly HEIGHT="$3"
readonly SOCKET_PREFIX="$4"
readonly RENDERER="$5"
readonly DIMENSIONS="$6"
readonly SUFFIX="$RENDERER-$DIMENSIONS"
readonly SOCKET="$SOCKET_PREFIX-$SUFFIX"
PROBE_RUNTIME="$(mktemp -d /tmp/rbw-wayland-probe.XXXXXX)"
readonly PROBE_RUNTIME
readonly PROBE_LOG="$EVIDENCE_DIR/task-3-wayland.probe-$SUFFIX.weston.log"
readonly PROBE_INFO="$EVIDENCE_DIR/task-3-wayland.probe-$SUFFIX.wayland-info.txt"
readonly PROBE_CAPTURE_DIR="$EVIDENCE_DIR/task-3-wayland.probe-$SUFFIX.capture"
readonly PROBE_CAPTURE_LOG="$EVIDENCE_DIR/task-3-wayland.probe-$SUFFIX.screenshooter.log"

probe_pid=""
chmod 700 "$PROBE_RUNTIME"
mkdir -p "$PROBE_CAPTURE_DIR"

cleanup() {
  stop_probe
  rm -rf "$PROBE_RUNTIME"
}
trap cleanup EXIT

stop_probe() {
  if [[ -n "$probe_pid" ]] && kill -0 "$probe_pid" 2>/dev/null; then
    kill "$probe_pid"
    wait "$probe_pid" || true
  fi
  probe_pid=""
}

renderer_arg=()
width_arg=()
height_arg=()
if [[ "$RENDERER" != "default" ]]; then
  renderer_arg=(--renderer="$RENDERER")
fi
if [[ "$DIMENSIONS" == "explicit" ]]; then
  width_arg=(--width="$WIDTH")
  height_arg=(--height="$HEIGHT")
fi

XDG_RUNTIME_DIR="$PROBE_RUNTIME" XDG_PICTURES_DIR="$PROBE_CAPTURE_DIR" weston \
  --backend=headless --scale=1 --refresh-rate=0 --debug --no-config "${renderer_arg[@]}" \
  --socket="$SOCKET" --log="$PROBE_LOG" "${width_arg[@]}" "${height_arg[@]}" \
  >"$EVIDENCE_DIR/task-3-wayland.probe-$SUFFIX.weston.stdout.log" 2>&1 &
probe_pid=$!

socket_state=timeout
wayland_info_state=missing
advertised_mode=missing
screenshooter_exit=not-run
retries_remaining=100
while ((retries_remaining > 0)); do
  if [[ -S "$PROBE_RUNTIME/$SOCKET" ]]; then
    socket_state=present
    break
  fi
  sleep 0.1
  ((retries_remaining -= 1))
done
if [[ "$socket_state" == present ]]; then
  if XDG_RUNTIME_DIR="$PROBE_RUNTIME" WAYLAND_DISPLAY="$SOCKET" wayland-info >"$PROBE_INFO" 2>&1; then
    wayland_info_state=present
    advertised_mode="$(rg -o 'width: [0-9]+ px, height: [0-9]+ px' "$PROBE_INFO" | tail -n 1)"
  else
    wayland_info_state=failed
  fi
  XDG_RUNTIME_DIR="$PROBE_RUNTIME" WAYLAND_DISPLAY="$SOCKET" XDG_PICTURES_DIR="$PROBE_CAPTURE_DIR" \
    weston-screenshooter >"$PROBE_CAPTURE_LOG" 2>&1 || screenshooter_exit=$?
fi
if [[ "$screenshooter_exit" == not-run ]]; then
  screenshooter_exit=0
fi
stop_probe

if rg -q 'Using Pixman renderer' "$PROBE_LOG"; then
  renderer_evidence=pixman
elif rg -q 'no-op renderer' "$PROBE_LOG"; then
  renderer_evidence=noop
else
  renderer_evidence=missing
fi
shopt -s nullglob
captures=("$PROBE_CAPTURE_DIR"/*.png)
shopt -u nullglob
capture_state=missing
if ((${#captures[@]} > 0)); then
  capture_state=present
fi
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
  "$RENDERER" "$DIMENSIONS" "$renderer_evidence" "$advertised_mode" "$socket_state" "$wayland_info_state" "$screenshooter_exit" "$capture_state" \
  >>"$EVIDENCE_DIR/task-3-wayland.probe-matrix.tsv"
