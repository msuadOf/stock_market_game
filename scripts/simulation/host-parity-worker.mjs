import { writeFile } from "node:fs/promises";

import { CLOSED_DAY_PARITY_SETUP, launch, publicSnapshot, stop, waitFor } from "./host-parity-core.mjs";

export async function workerScenario(output, port, digest, setup) {
  const workerLog = `${output}/wasm-worker.process.log`;
  await writeFile(workerLog, "");
  const vite = launch("corepack", ["pnpm", "--filter", "web", "dev", "--host", "127.0.0.1", "--port", String(port)], workerLog, { VITE_HOST_PARITY: "true" });
  let artifact;
  try {
    await waitFor(`http://127.0.0.1:${port}`);
    const playwright = await import("../../node_modules/.pnpm/playwright@1.63.0/node_modules/playwright/index.js");
    const browser = await playwright.default.chromium.launch({ headless: true });
    try {
      const page = await browser.newPage();
      await page.goto(`http://127.0.0.1:${port}`, { waitUntil: "networkidle" });
      const raw = await page.evaluate(async ({ setup, closedSetup }) => {
        const worker = new Worker("/src/host/wasm-worker.ts", { type: "module" });
        const messages = [];
        worker.addEventListener("message", (event) => messages.push(event.data));
        const receive = (type, requestId) => new Promise((resolveResult, rejectResult) => {
          const timeout = setTimeout(() => rejectResult(new Error(`timed out waiting for ${type}`)), 15_000);
          const poll = () => {
            const index = messages.findIndex((message) => message.type === type && (requestId === undefined || message.requestId === requestId));
            if (index >= 0) { clearTimeout(timeout); resolveResult(messages.splice(index, 1)[0]); return; }
            setTimeout(poll, 10);
          };
          poll();
        });
        worker.postMessage({ type: "init" });
        await receive("ready");
        worker.postMessage({ type: "create", setup, seed: 7n });
        const created = await receive("created");
        const before = await receive("snapshot");
        const normalSteps = [];
        for (let step = 0; step < setup.ticks_per_day; step += 1) {
          worker.postMessage({ type: "hostParityStep", requestId: 10 + step, generation: created.generation });
          normalSteps.push(await receive("hostParityStepped", 10 + step));
        }
        worker.postMessage({ type: "civilDate", requestId: 20, generation: created.generation });
        const civilBefore = await receive("civilDate", 20);
        worker.postMessage({ type: "endCivilDay", requestId: 21, generation: created.generation });
        const civilDayEnded = await receive("civilDayEnded", 21);
        const civilUpdate = await receive("hostUpdate");
        worker.postMessage({ type: "endCivilDay", requestId: 26, generation: created.generation });
        const duplicateAdvance = await receive("operationError", 26);
        worker.postMessage({ type: "civilDate", requestId: 22, generation: created.generation });
        const civilAfter = await receive("civilDate", 22);
        worker.postMessage({ type: "snapshot" });
        const afterCivil = await receive("snapshot");
        const concurrentSave = receive("saved", 23);
        const concurrentDate = receive("civilDate", 24);
        worker.postMessage({ type: "save", requestId: 23, generation: created.generation });
        worker.postMessage({ type: "civilDate", requestId: 24, generation: created.generation });
        const [concurrentSaved, concurrentCivilDate] = await Promise.all([concurrentSave, concurrentDate]);
        worker.postMessage({ type: "npcDecisionDiagnostics", requestId: 25, generation: created.generation, account: 1 });
        const diagnostics = await receive("npcDecisionDiagnostics", 25);
        worker.postMessage({ type: "save", requestId: 1, generation: created.generation });
        const saved = await receive("saved", 1);
        worker.postMessage({ type: "save", requestId: 9, generation: created.generation - 1 });
        const stale = await receive("operationError", 9);
        worker.postMessage({ type: "restore", requestId: 2, generation: created.generation, slot: {} });
        const corrupt = await receive("operationError", 2);
        worker.postMessage({ type: "snapshot" });
        const afterCorrupt = await receive("snapshot");
        worker.postMessage({ type: "restore", requestId: 3, generation: created.generation, slot: saved.slot });
        const restored = await receive("restored", 3);
        worker.postMessage({ type: "drop" });
        worker.postMessage({ type: "create", setup: closedSetup, seed: 7n });
        const closedCreated = await receive("created");
        await receive("snapshot");
        for (let step = 0; step < closedSetup.ticks_per_day; step += 1) {
          worker.postMessage({ type: "hostParityStep", requestId: 40 + step, generation: closedCreated.generation });
          await receive("hostParityStepped", 40 + step);
        }
        worker.postMessage({ type: "endCivilDay", requestId: 30, generation: closedCreated.generation });
        await receive("civilDayEnded", 30);
        await receive("hostUpdate");
        worker.postMessage({ type: "endCivilDay", requestId: 32, generation: closedCreated.generation });
        await receive("civilDayEnded", 32);
        const closedUpdate = await receive("hostUpdate");
        worker.postMessage({ type: "publicReports", requestId: 31, generation: closedCreated.generation, query: { company_id: "C-600101", cursor: null, page_size: 20 } });
        const closedReports = await receive("publicReports", 31);
        worker.postMessage({ type: "drop" });
        await worker.terminate();
        return {
          created,
          before: before.snapshot,
          normalSteps,
          afterCivil: afterCivil.snapshot,
          civilBefore,
          civilDayEnded,
          civilUpdate,
          duplicateAdvance,
          civilAfter,
          concurrentSaved,
          concurrentCivilDate,
          diagnostics,
          saved: saved.slot,
          stale,
          corrupt,
          afterCorrupt: afterCorrupt.snapshot,
          restored,
          closedUpdate,
          closedReports,
        };
      }, { setup, closedSetup: CLOSED_DAY_PARITY_SETUP });
      const normalEvents = raw.normalSteps.flatMap((response) => response.events).concat(raw.civilUpdate.update.events.at(-1));
      const closedEvents = raw.closedUpdate.update.events;
      artifact = {
        identity: { transport: "worker-postmessage", proof: "Chromium Worker loaded wasm-worker.ts" },
        publicHash: digest(publicSnapshot(raw.afterCivil)),
        restoreHash: digest(publicSnapshot(raw.restored.snapshot)),
        normalEventHash: digest(normalEvents),
        closedEventHash: digest(closedEvents),
        disclosureHash: digest(raw.closedReports.page),
        corruptRestoreRejected: raw.corrupt.type === "operationError",
        corruptRestoreAtomic: digest(publicSnapshot(raw.afterCivil)) === digest(publicSnapshot(raw.afterCorrupt)),
        staleRequestRejected: raw.stale.type === "operationError" && raw.stale.generation === raw.created.generation - 1,
        stalePrivateRecordsExposed: Object.hasOwn(raw.stale, "records"),
        capabilities: { npcDecisionDiagnostics: raw.diagnostics.diagnostics.kind === "supported" },
        scenarios: {
          identity: true,
          "civil-day": raw.normalSteps.some((events) => events.events.some((event) => Object.hasOwn(event, "DayBoundary")))
            && raw.civilBefore.date === "2030-01-02" && raw.civilAfter.date === "2030-01-03"
            && raw.civilDayEnded.type === "civilDayEnded",
          "no-trade-disclosure": closedEvents.some((event) => Object.hasOwn(event, "CompanyDisclosurePublished"))
            && closedEvents.some((event) => Object.hasOwn(event, "CivilDateAdvanced"))
            && !closedEvents.some((event) => Object.hasOwn(event, "Trade") || Object.hasOwn(event, "DayBoundary"))
            && raw.closedReports.page.reports.length > 0,
          "sequence-rejection": raw.duplicateAdvance.type === "operationError",
          "corrupt-restore": raw.corrupt.type === "operationError",
          reload: raw.restored.type === "restored"
            && digest(publicSnapshot(raw.afterCivil)) === digest(publicSnapshot(raw.restored.snapshot)),
          concurrency: raw.concurrentSaved.requestId === 23 && raw.concurrentCivilDate.requestId === 24,
          "stale-privacy": !Object.hasOwn(raw.stale, "records"),
          "diagnostics-default": raw.diagnostics.diagnostics.kind === "unsupported" && !Object.hasOwn(raw.diagnostics.diagnostics, "records"),
        },
        raw,
      };
      await writeFile(`${output}/wasm-worker.raw.json`, `${JSON.stringify(artifact, null, 2)}\n`);
      return artifact;
    } finally { await browser.close(); }
  } finally {
    const cleanup = await stop(vite);
    if (artifact) artifact.cleanup = cleanup;
  }
}
