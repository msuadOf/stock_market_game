# Problems — company-information-npc-intentions

Unresolved blockers and technical debt discovered during work on this plan.

_Auto-scaffolded by /start-work. Append new entries below - never overwrite._

---

## 2026-09-12 Cross-host public report period contract

- Task 30's strict WASM bridge requires public report `period` as a canonical period-end civil date, but the shared
  engine `PublicReportSummary` currently defines/emits it as accounting month `YYYY-MM`; server and desktop return
  that DTO directly. The Task 30 WASM-only repair is intentionally limited by scope and cannot correct engine,
  server, or desktop. Owner must establish one uniform cross-host representation and tests before release.

## 2026-09-12 Resolution: cross-host public report period contract

- Resolved centrally in engine after a failing fixture observed `1998-03`: `PublicReportSummary::from` now emits
  report period end as canonical `YYYY-MM-DD`, so WASM, server, and desktop all consume the same public DTO. WASM
  date arithmetic was removed. Desktop runtime verification remains environmentally blocked by missing GTK/WebKit
  development libraries; engine and server outputs plus source delegation provide the available contract evidence.

## 2026-09-13 Task 35 Tauri diagnostics verification blocker

- `cargo check -p stock-market-game --features simulation-diagnostics` reaches native sys crates but stops because the Linux runner lacks pkg-config metadata for GLib/GIO/GDK/Cairo/Pango/ATK/GDK-Pixbuf/libsoup/javascriptcoregtk. The new Tauri diagnostics actor/command cannot be independently compiled or driven here; no desktop pass is claimed.

## 2026-09-13 Resolution: Task 35 native Tauri diagnostics verification

- User authorized documented Ubuntu package installation. Native feature/default checks now compile, Tauri's real command queue is exercised by official MockRuntime actor tests under Xvfb, and the real Wry desktop executable launches under Xvfb. The previous native dependency blocker is resolved.

## 2026-09-13 Task 35 Wayland screenshot limitation

- Weston 14.0.2 headless compositor and the feature Wry binary both launched with a real socket under an isolated runtime directory, but `weston-screenshooter` failed on that compositor's zero-sized output (`width > 0` assertion). This does not invalidate the actor security repair or Wayland launch evidence, but it means no Wayland screenshot is claimed. A compositor configuration with an explicit nonzero headless output or a supported capture client is needed for pixel evidence.

## 2026-09-13 Task 35 blocker retry

- User authorized a blocker-resolution attempt but not package installation or environment changes. A fresh feature build again stopped before the desktop crate at `gobject-sys`: `pkg-config --libs --cflags gobject-2.0 'gobject-2.0 >= 2.70'` failed. A direct `pkg-config --modversion` probe also found none of `glib-2.0 gio-2.0 gdk-3.0 cairo pango atk gdk-pixbuf-2.0 libsoup-3.0 javascriptcoregtk-4.1`. Raw outputs are in Task-35 evidence. No source changes or fake `.pc` files were made.

## 2026-09-13 Task 34 residual chart artifact

- Desktop production captures retain detached black `TV` watermark fragments from the chart surface. The Task-34
  company-information/new-game report surface is otherwise independently approved, and this artifact is out of
  scope for the Task-34 UI/E2E repair. It should be assigned to chart rendering work rather than hidden or patched
  through the company panel.

## 2026-09-13 Task 34 controlled date no-silent-fallback defect

- The rejected controlled-date field used `value || DEFAULT_START_DATE`, displaying a valid-looking date after the
  user cleared the state while validation correctly rejected the actual empty state. The repair removes the display
  fallback, leaving the existing `DEFAULT_SETUP.start_date` initialization as the sole 2030 default. Production
  E2E and final independent visual review now prove empty/error agreement, unchanged session, valid 2031 reset,
   report readiness, and visible date-input keyboard focus.

## 2026-09-13 Task 36 engine-only causal diagnostics blocker

- The delegated scope permits only `packages/engine/src/diagnostics*`, diagnostics tests/examples, and evidence/notepads, but the required raw facts do not exist in those surfaces. Extending `simulation-diagnostics` with the missing actual lifecycle and causal fields necessarily changes `GameSession` collectors/events and related matching/auction plumbing, which the task explicitly forbids. The task cannot produce an honest actual-event-derived implementation without a scope amendment.

## 2026-09-13 Task 36 source permission resolved; clock review remains

- User authorized minimal source hooks; actual feature-only lifecycle collection,
  aggregation, tests and an executable artifact are now implemented. The prior
  source-scope blocker is resolved without public/save schema changes.
- Independent review identifies existing chain_observation_instant as missing
  the lunch interval. Acquired timestamps in diagnostics faithfully retain the
  actual source acquisition instant; replacing it only in reports would invent
  timing. Correcting the authoritative decision clock changes NPC behavior and
  requires a separately approved behavioral repair. Task 36 is not claimed fully
  approved while this exact A-share acquisition-clock issue remains.

## 2026-09-13 Resolution: Task 36 authoritative lunch clock

- User explicitly authorized source correctness repair. The old decision clock
  is replaced by one shared tick-to-civil mapping including lunch and auction
  windows; causal timestamps consume the same source. Actual lunch regression
  passes in default and feature builds. Existing publication guards are unchanged.
- Independent reviewer ses_f68a6dac4ffezUHqmooMOFPtUY returned ACCEPT for this
  repair with no blocker. Prior clock qualification is resolved. Primary
  orchestrator's final Task36 re-review/acceptance is requested; no checkbox changed.
