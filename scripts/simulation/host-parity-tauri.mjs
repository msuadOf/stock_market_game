import { lstat, mkdtemp, rm, writeFile } from "node:fs/promises";
import { once } from "node:events";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";

import { CLOSED_DAY_PARITY_SETUP, launch, publicSnapshot, stop, waitFor } from "./host-parity-core.mjs";

function eventSequence(event) {
  const payload = Object.values(event)[0];
  return payload?.seq;
}

function observedEventsAreOrdered(payloads, sessionId) {
  const sequences = payloads
    .filter((payload) => payload.session_id === sessionId)
    .flatMap((payload) => payload.events.map(eventSequence));
  return sequences.every((sequence, index) => Number.isSafeInteger(sequence)
    && (index === 0 || sequence > sequences[index - 1]));
}

function waitForSocket(runtimeDir, socket) {
  return new Promise((resolveSocket, rejectSocket) => {
    const timeout = setTimeout(() => rejectSocket(new Error(`Weston socket missing: ${runtimeDir}/${socket}`)), 10_000);
    const poll = async () => {
      try {
        await lstat(`${runtimeDir}/${socket}`);
        clearTimeout(timeout);
        resolveSocket();
      } catch { setTimeout(poll, 50); }
    };
    poll();
  });
}

export async function tauriScenario(output, inspectorPort, digest, setup, diagnosticsFeature, label) {
  const runtimeDir = await mkdtemp(`${tmpdir()}/host-parity-wayland-runtime.`);
  const configDir = await mkdtemp(`${tmpdir()}/host-parity-wayland-config.`);
  const socket = `host-parity-${process.pid}`;
  const westonLog = `${output}/tauri.${label}.weston.log`;
  const appLog = `${output}/tauri.${label}.process.log`;
  await writeFile(westonLog, "");
  await writeFile(appLog, "");
  const weston = launch("weston", ["--backend=headless", "--renderer=pixman", "--width=1280", "--height=800", "--scale=1", "--socket", socket, "--no-config"], westonLog, { XDG_RUNTIME_DIR: runtimeDir });
  let app;
  let artifact;
  try {
    await waitForSocket(runtimeDir, socket);
    app = launch(`${output}/tauri-${label}`, [], appLog, {
      GDK_BACKEND: "wayland",
      WAYLAND_DISPLAY: socket,
      XDG_RUNTIME_DIR: runtimeDir,
      XDG_CONFIG_HOME: configDir,
      WEBKIT_DISABLE_COMPOSITING_MODE: "1",
      LIBGL_ALWAYS_SOFTWARE: "1",
      WEBKIT_INSPECTOR_HTTP_SERVER: `127.0.0.1:${inspectorPort}`,
    });
    await waitFor(`http://127.0.0.1:${inspectorPort}/`, 400);
    const probe = spawn(process.execPath, ["scripts/simulation/host-parity-tauri-probe.mjs", String(inspectorPort), JSON.stringify(setup), JSON.stringify(CLOSED_DAY_PARITY_SETUP)], { stdio: ["ignore", "pipe", "pipe"] });
    let probeOutput = "";
    probe.stdout.on("data", (chunk) => { probeOutput += String(chunk); });
    probe.stderr.on("data", (chunk) => { probeOutput += String(chunk); });
    await once(probe, "exit");
    await writeFile(`${output}/tauri.${label}.ipc.log`, probeOutput);
    if (probe.exitCode !== 0) throw new Error(`Tauri IPC probe exited ${probe.exitCode}: ${probeOutput}`);
    const raw = JSON.parse(probeOutput.trim());
    const normalEvents = raw.normalSteps.flatMap((events) => events).concat(raw.civil.events);
    const closedEvents = raw.closedDay.events;
    artifact = {
      identity: { transport: "tauri-invoke-ipc", proof: "Weston native WebView invoked Tauri commands through WebKit inspector" },
      publicHash: digest(publicSnapshot(raw.afterCivil)),
      restoreHash: digest(publicSnapshot(raw.restored.snapshot)),
      normalEventHash: digest(normalEvents),
      closedEventHash: digest(closedEvents),
      disclosureHash: digest(raw.closedReports.value),
      corruptRestoreRejected: raw.corruptRejected,
      corruptRestoreAtomic: digest(publicSnapshot(raw.afterCivil)) === digest(publicSnapshot(raw.afterCorrupt)),
      staleRequestRejected: raw.staleRejected,
      stalePrivateRecordsExposed: raw.staleError?.includes("records") ?? false,
      capabilities: { npcDecisionDiagnostics: diagnosticsFeature },
      scenarios: {
        identity: true,
        "civil-day": raw.civilBefore.value === "2030-01-02" && raw.civilAfter.value === "2030-01-03"
          && raw.normalSteps.some((events) => events.some((event) => Object.hasOwn(event, "DayBoundary")))
          && raw.observedEvents.some((payload) => payload.events.some((event) => Object.hasOwn(event, "CivilDateAdvanced")))
          && observedEventsAreOrdered(raw.observedEvents, raw.sessionId),
        "no-trade-disclosure": closedEvents.some((event) => Object.hasOwn(event, "CompanyDisclosurePublished"))
          && closedEvents.some((event) => Object.hasOwn(event, "CivilDateAdvanced"))
          && !closedEvents.some((event) => Object.hasOwn(event, "Trade") || Object.hasOwn(event, "DayBoundary"))
          && raw.closedReports.value.reports.length > 0,
        "sequence-rejection": raw.duplicateRejected,
        "corrupt-restore": raw.corruptRejected,
        reload: raw.restored.generation === "2",
        concurrency: raw.concurrent.length === 2,
        "stale-privacy": !raw.staleError?.includes("records"),
        "diagnostics-default": !diagnosticsFeature && raw.diagnostics.value.kind === "unsupported" && !Object.hasOwn(raw.diagnostics.value, "records"),
        "diagnostics-feature": diagnosticsFeature && raw.diagnostics.value.kind === "supported"
          && Array.isArray(raw.diagnostics.value.records)
          && raw.diagnostics.value.records.length <= 128,
      },
      raw,
    };
    await writeFile(`${output}/tauri.${label}.raw.json`, `${JSON.stringify(artifact, null, 2)}\n`);
    return artifact;
  } finally {
    const appCleanup = app ? await stop(app) : null;
    const westonCleanup = await stop(weston);
    await rm(runtimeDir, { recursive: true, force: true });
    await rm(configDir, { recursive: true, force: true });
    const cleanup = { app: appCleanup, weston: westonCleanup, runtimeDirRemoved: true, configDirRemoved: true };
    if (artifact) artifact.cleanup = cleanup;
    await writeFile(`${output}/tauri.${label}.cleanup.json`, `${JSON.stringify(cleanup, null, 2)}\n`);
  }
}
