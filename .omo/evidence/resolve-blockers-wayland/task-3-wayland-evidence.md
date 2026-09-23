# Task 3 Current Immutable Evidence

The current validated Todo-3 run is
[`task-3-wayland.run-20260913-144406`](task-3-wayland.run-20260913-144406/). The harness creates a new
timestamped run directory rather than overwriting an earlier receipt.

## Evidence Map

| Claim | Current artifact |
| --- | --- |
| Exclusive run identity and all file hashes | [`generated review`](task-3-wayland.run-20260913-144406/task-3-wayland.review.md), [`machine manifest`](task-3-wayland.run-20260913-144406/task-3-wayland.integrity.json) |
| 2×2 renderer/dimensions compositor-capture probe | [`probe matrix`](task-3-wayland.run-20260913-144406/task-3-wayland.probe-matrix.tsv) |
| Explicit Pixman Weston 1280×800 compositor | [`weston log`](task-3-wayland.run-20260913-144406/task-3-wayland.weston.log), [`wayland-info`](task-3-wayland.run-20260913-144406/task-3-wayland.wayland-info.txt) |
| Effective Wayland environment, default and feature process PIDs | [`default environment`](task-3-wayland.run-20260913-144406/task-3-wayland.environment-default.txt), [`feature environment`](task-3-wayland.run-20260913-144406/task-3-wayland.environment-feature.txt) |
| Pixel-valid native Tauri capture | [`capture`](task-3-wayland.run-20260913-144406/task-3-wayland.capture.png), [`validator`](task-3-wayland.run-20260913-144406/task-3-wayland.capture-validation.txt), [`bounded capture attempt`](task-3-wayland.run-20260913-144406/task-3-wayland.capture-attempts.tsv) |
| Default/release/feature desktop diagnostics tests | [`default`](task-3-wayland.run-20260913-144406/task-3-wayland.actor-default.txt), [`release`](task-3-wayland.run-20260913-144406/task-3-wayland.actor-release.txt), [`feature`](task-3-wayland.run-20260913-144406/task-3-wayland.actor-feature.txt) |
| Exact 128-record collector ceiling | [`engine bound`](task-3-wayland.run-20260913-144406/task-3-wayland.engine-bounded.txt) |
| Live Tauri WebView IPC | [`default IPC`](task-3-wayland.run-20260913-144406/task-3-wayland.ipc-default.json), [`feature IPC`](task-3-wayland.run-20260913-144406/task-3-wayland.ipc-feature.json) |
| Cleanup | [`cleanup receipt`](task-3-wayland.run-20260913-144406/task-3-wayland.cleanup.txt) |

The current run records `GDK_BACKEND=wayland` and the isolated `WAYLAND_DISPLAY` in `/proc` for both the
timeout launcher and the real Tauri process. The default live IPC output has `kind:"unsupported"`,
`hasRecordsProperty:false`, and `records:null`; the feature output has `kind:"supported"`,
`hasRecordsProperty:true`, five records, and malformed/stale rejection.

The screenshot establishes rendering infrastructure, not chart completion. The known blank chart and clipped
`TV` fragments remain an explicitly out-of-scope chart product observation. Headless Weston has no keyboard
seat, so the run does not claim native input coverage.
