import { normalizeSerdeMaps } from "/apps/web/src/host/serde-normalize.ts";
import { createWorkerHost } from "/apps/web/src/host/worker-host.ts";
import { prepareDocumentWasmRestoreCapability } from "/apps/web/src/host/wasm-restore-capability-source.ts";
import { getDocumentWasmMemoryOwner } from "/apps/web/src/host/wasm-document-memory-owner.ts";

const result: Record<string, unknown> = { stage: "fetch" };
let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
try {
  const response = await fetch("/__c2c_fixture.json");
  if (!response.ok) throw new Error(`fixture HTTP ${response.status}`);
  const archive = await response.json();
  result.stage = "capability";
  await prepareDocumentWasmRestoreCapability(document, { baseUrl: "/wasm-metadata/", runtimeOverride: true });
  const owner = getDocumentWasmMemoryOwner(document);
  const admitted = owner.admit({ kind: "load", archive });
  if (admitted.kind !== "admitted" || admitted.intent.kind !== "load") throw new Error("fixture 未获 admission");
  result.stage = "host";
  host = await createWorkerHost(archive.setup, 40n, { initialArchive: admitted.intent.archive, memoryAdmissionTicket: admitted.ticket,
    enableE2EStepping: true, onSpawnFailure(failure) { result.failure = failure; } });
  result.stage = "started";
  const updates: unknown[] = [];
  await host.start((update) => { updates.push(update.type === "baseline" ? { type: update.type, generation: update.generation, snapshot: structuredClone(update.snapshot), civilDate: update.civilDate } : normalizeSerdeMaps(update)); }, (failure) => { result.failure = failure; });
  await host.stop();
  result.civilDate = await host.civilDate?.();
  result.snapshot = host.snapshot();
  result.updates = updates;
  result.stage = "stepping";
  const scriptResponse = await fetch("/__c2c_script.json");
  if (!scriptResponse.ok) throw new Error(`script HTTP ${scriptResponse.status}`);
  const script = await scriptResponse.json();
  const codeOf = (id: number) => id === script.open ? "000101" : id === script.closed ? "600101" : (() => { throw new Error("未知 StockId"); })();
  const observe = async () => {
    await host!.refreshBaseline();
    const date = await host!.civilDate!();
    const currentMinute = Object.fromEntries(await Promise.all(["000101", "600101"].map(async code => [code, await host!.queryCurrentMinuteHistory({ code })])));
    const retained = date! > script.retainedDate ? Object.fromEntries(await Promise.all(["000101", "600101"].map(async code => [code, await host!.queryMarketHistory({ code, date_from: script.retainedDate, date_to: date! > script.finalRetainedDate ? script.finalRetainedDate : script.retainedDate, after: null, page_size: 2 })]))) : {};
    const confirmations = date! > script.retainedDate ? await host!.queryPersonalTradeConfirmations() : [];
    return { confirmations, civilDate: date, snapshot: structuredClone(host!.snapshot()), workingOrders: await host!.playerWorkingOrders(), currentMinute, retained };
  };
  const initial = await observe();
  const frames = [];
  for (const [index, step] of script.steps.entries()) {
    for (const intent of step.intents) {
      if (!intent.PlaceLimit) throw new Error("只接受固定 PlaceLimit 输入");
      await host.submitIntent({ PlaceLimit: { ...intent.PlaceLimit, code: codeOf(intent.PlaceLimit.code) } });
    }
    const before = updates.length;
    if (index % 3 === 2) await host.endCivilDay!();
    else await host.stepOnceForE2E();
    const view = await observe();
    frames.push({ updates: structuredClone(updates.slice(before)), view });
  }
  result.initial = initial;
  result.frames = frames;
  result.ok = true;
} catch (error) {
  result.ok = false; result.error = String(error); result.stack = error instanceof Error ? error.stack : undefined;
} finally {
  if (host) try { await host.dispose(); } catch (error) { result.cleanupError = String(error); result.ok = false; }
  (window as unknown as { c2cProbe: unknown }).c2cProbe = result;
}
