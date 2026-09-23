import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { writeFile } from "node:fs/promises";

import { CLOSED_DAY_PARITY_SETUP, NORMAL_DAY_PARITY_SETUP, jsonResponse, launch, publicSnapshot, stop, waitFor } from "./host-parity-core.mjs";

export async function serverScenario(output, port, digest) {
  const serverLog = `${output}/server.process.log`;
  await writeFile(serverLog, "");
  const baseUrl = `http://127.0.0.1:${port}`;
  const server = launch(`${output}/server`, [], serverLog, { STOCK_MARKET_GAME_SERVER_BIND_ADDR: `127.0.0.1:${port}` });
  let artifact;
  try {
    await waitFor(`${baseUrl}/healthz`);
    const session = await jsonResponse(await fetch(`${baseUrl}/api/new`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ setup: NORMAL_DAY_PARITY_SETUP, seed: "7" }) }));
    const closedSession = await jsonResponse(await fetch(`${baseUrl}/api/new`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ setup: CLOSED_DAY_PARITY_SETUP, seed: "7" }) }));
    const before = await jsonResponse(await fetch(`${baseUrl}/api/snapshot?session_id=${session.session_id}`));
    const unauthenticatedStep = await fetch(`${baseUrl}/api/host-parity/step`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ session_id: session.session_id, generation: "1" }),
    });
    const unauthenticatedAdvance = await fetch(`${baseUrl}/api/host-parity/advance-civil-day`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ session_id: session.session_id, generation: "1" }),
    });
    const wsProbe = spawn(`${output}/ws-parity-probe`, [baseUrl, session.session_id], {
      stdio: ["pipe", "pipe", "pipe"],
    });
    let wsOutput = "";
    let wsError = "";
    wsProbe.stdout.on("data", (chunk) => { wsOutput += String(chunk); });
    wsProbe.stderr.on("data", (chunk) => { wsError += String(chunk); });
    wsProbe.stdin.write(`${session.session_token}\n`);
    const waitForProbeStage = async (stage) => {
      for (let attempt = 0; attempt < 2_400; attempt += 1) {
        const record = wsOutput.split("\n").filter(Boolean).map((line) => JSON.parse(line)).find((line) => line.stage === stage);
        if (record) return record;
        await new Promise((resolveDelay) => setTimeout(resolveDelay, 25));
      }
      throw new Error(`timed out waiting for WS probe ${stage}: ${wsOutput}${wsError}`);
    };
    const wsBaseline = await waitForProbeStage("baseline");
    const normalSteps = await Promise.all(Array.from({ length: NORMAL_DAY_PARITY_SETUP.ticks_per_day }, () => fetch(`${baseUrl}/api/host-parity/step`, {
      method: "POST",
      headers: { authorization: `Bearer ${session.session_token}`, "content-type": "application/json" },
      body: JSON.stringify({ session_id: session.session_id, generation: "1" }),
    }).then(jsonResponse)));
    const civilFirst = await jsonResponse(await fetch(`${baseUrl}/api/host-parity/advance-civil-day`, {
      method: "POST",
      headers: { authorization: `Bearer ${session.session_token}`, "content-type": "application/json" },
      body: JSON.stringify({ session_id: session.session_id, generation: "1" }),
    }));
    const duplicateAdvance = await fetch(`${baseUrl}/api/host-parity/advance-civil-day`, {
      method: "POST",
      headers: { authorization: `Bearer ${session.session_token}`, "content-type": "application/json" },
      body: JSON.stringify({ session_id: session.session_id, generation: "1" }),
    });
    await Promise.all(Array.from({ length: CLOSED_DAY_PARITY_SETUP.ticks_per_day }, () => fetch(`${baseUrl}/api/host-parity/step`, {
      method: "POST",
      headers: { authorization: `Bearer ${closedSession.session_token}`, "content-type": "application/json" },
      body: JSON.stringify({ session_id: closedSession.session_id, generation: "1" }),
    }).then(jsonResponse)));
    await jsonResponse(await fetch(`${baseUrl}/api/host-parity/advance-civil-day`, {
      method: "POST",
      headers: { authorization: `Bearer ${closedSession.session_token}`, "content-type": "application/json" },
      body: JSON.stringify({ session_id: closedSession.session_id, generation: "1" }),
    }));
    const closedDay = await jsonResponse(await fetch(`${baseUrl}/api/host-parity/advance-civil-day`, {
      method: "POST",
      headers: { authorization: `Bearer ${closedSession.session_token}`, "content-type": "application/json" },
      body: JSON.stringify({ session_id: closedSession.session_id, generation: "1" }),
    }));
    const closedReports = await jsonResponse(await fetch(`${baseUrl}/api/companies/C-600101/reports?session_id=${closedSession.session_id}&limit=20`, {
      headers: { authorization: `Bearer ${closedSession.session_token}` },
    }));
    wsProbe.stdin.write("advance-complete\n");
    const wsFrame = await waitForProbeStage("frame");
    const afterCivil = await jsonResponse(await fetch(`${baseUrl}/api/snapshot?session_id=${session.session_id}`));
    const concurrent = await Promise.all([
      fetch(`${baseUrl}/api/snapshot?session_id=${session.session_id}`).then(jsonResponse),
      fetch(`${baseUrl}/api/speed?session_id=${session.session_id}`).then(jsonResponse),
    ]);
    const saved = await jsonResponse(await fetch(`${baseUrl}/api/save`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ session_id: session.session_id }) }));
    const corrupt = await fetch(`${baseUrl}/api/load`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ session_id: session.session_id, slot: {} }) });
    const afterCorrupt = await jsonResponse(await fetch(`${baseUrl}/api/snapshot?session_id=${session.session_id}`));
    const restored = await jsonResponse(await fetch(`${baseUrl}/api/load`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ session_id: session.session_id, slot: saved }) }));
    wsProbe.stdin.write("restore-complete\n");
    wsProbe.stdin.end();
    await once(wsProbe, "exit");
    await writeFile(`${output}/server.ws-probe.log`, `${wsOutput}${wsError}`);
    if (wsProbe.exitCode !== 0) throw new Error(`WS probe exited ${wsProbe.exitCode}: ${wsOutput}${wsError}`);
    const wsComplete = wsOutput.split("\n").filter(Boolean).map((line) => JSON.parse(line)).find((line) => line.stage === "complete");
    const reports = await fetch(`${baseUrl}/api/companies/C-600101/reports?session_id=${session.session_id}&limit=20`, { headers: { authorization: `Bearer ${session.session_token}` } });
    const unauthorized = await fetch(`${baseUrl}/api/companies/C-600101/reports?session_id=${session.session_id}&limit=20`);
    const staleDiagnostics = await fetch(`${baseUrl}/api/diagnostics/npc/1?session_id=${session.session_id}&generation=0`, { headers: { authorization: `Bearer ${session.session_token}` } });
    const staleDiagnosticsBody = await jsonResponse(staleDiagnostics);
    const staleStep = await fetch(`${baseUrl}/api/host-parity/step`, {
      method: "POST",
      headers: { authorization: `Bearer ${session.session_token}`, "content-type": "application/json" },
      body: JSON.stringify({ session_id: session.session_id, generation: "0" }),
    });
    const deleted = await fetch(`${baseUrl}/api/session?session_id=${session.session_id}`, { method: "DELETE" });
    const closedDeleted = await fetch(`${baseUrl}/api/session?session_id=${closedSession.session_id}`, { method: "DELETE" });
    assert.equal(deleted.status, 204, "server session cleanup must succeed");
    assert.equal(closedDeleted.status, 204, "closed-day server session cleanup must succeed");
    artifact = {
      identity: { transport: "server-http-ws", proof: `spawned server authenticated over ${baseUrl}` },
      publicHash: digest(publicSnapshot(afterCivil)),
      restoreHash: digest(publicSnapshot(restored)),
      normalEventHash: digest(normalSteps.flatMap((events) => events).concat(civilFirst.events).sort((left, right) => Object.values(left)[0].seq - Object.values(right)[0].seq)),
      closedEventHash: digest(closedDay.events),
      disclosureHash: digest(closedReports),
      corruptRestoreRejected: !corrupt.ok,
      corruptRestoreAtomic: digest(publicSnapshot(afterCivil)) === digest(publicSnapshot(afterCorrupt)),
      publicReportAuthorized: reports.ok,
      unauthenticatedReportRejected: unauthorized.status === 401,
      unauthenticatedStepRejected: unauthenticatedStep.status === 401,
      unauthenticatedAdvanceRejected: unauthenticatedAdvance.status === 401,
      staleRequestRejected: staleStep.status === 400,
      stalePrivateRecordsExposed: Object.hasOwn(staleDiagnosticsBody.diagnostics, "records"),
      capabilities: { npcDecisionDiagnostics: false },
      scenarios: {
        identity: true,
        "civil-day": normalSteps.some((events) => events.some((event) => Object.hasOwn(event, "DayBoundary")))
          && civilFirst.events.some((event) => Object.hasOwn(event, "CivilDateAdvanced")),
        "no-trade-disclosure": closedDay.events.some((event) => Object.hasOwn(event, "CompanyDisclosurePublished"))
          && closedDay.events.some((event) => Object.hasOwn(event, "CivilDateAdvanced"))
          && !closedDay.events.some((event) => Object.hasOwn(event, "Trade") || Object.hasOwn(event, "DayBoundary"))
          && closedReports.reports.length > 0,
        "sequence-rejection": duplicateAdvance.status === 400,
        "corrupt-restore": !corrupt.ok,
        reload: digest(publicSnapshot(afterCivil)) === digest(publicSnapshot(restored)),
        concurrency: concurrent[0].seq >= 0 && concurrent[1].running === false,
        "stale-privacy": !Object.hasOwn(staleDiagnosticsBody.diagnostics, "records"),
        "diagnostics-default": staleDiagnosticsBody.diagnostics.kind === "unsupported" && !Object.hasOwn(staleDiagnosticsBody.diagnostics, "records"),
        "server-ws-resync": wsBaseline.ok === true && wsFrame.ok === true
          && wsComplete?.resync === true && wsComplete.freshBaseline === true,
      },
      raw: { session: { session_id: session.session_id, session_token_present: typeof session.session_token === "string" }, before, normalSteps, civilFirst, duplicateAdvanceStatus: duplicateAdvance.status, closedDay, closedReports, afterCivil, concurrent, saved, corruptStatus: corrupt.status, afterCorrupt, restored, staleDiagnostics: staleDiagnosticsBody, staleStepStatus: staleStep.status, wsProbe: { baseline: wsBaseline, frame: wsFrame, complete: wsComplete } },
    };
    await writeFile(`${output}/server.raw.json`, `${JSON.stringify(artifact, null, 2)}\n`);
    return artifact;
  } finally {
    const cleanup = await stop(server);
    if (artifact) artifact.cleanup = cleanup;
  }
}
