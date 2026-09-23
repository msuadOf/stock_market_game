import { setTimeout as delay } from "node:timers/promises";

const [portText, setupText, closedSetupText] = process.argv.slice(2);
const port = Number(portText);
if (!Number.isSafeInteger(port) || port < 1 || port > 65_535 || !setupText || !closedSetupText) {
  throw new Error("usage: node scripts/simulation/host-parity-tauri-probe.mjs <inspector-port> <setup-json> <closed-setup-json>");
}

async function inspectorTarget() {
  const address = `http://127.0.0.1:${port}/`;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch(address, { signal: AbortSignal.timeout(1_000) });
      const page = response.ok ? await response.text() : "";
      const target = page.match(/\/socket\/(\d+)\/(\d+)\/([A-Za-z-]+)/);
      if (target) return `ws://127.0.0.1:${port}/socket/${target[1]}/${target[2]}/${target[3].replace(/([a-z])([A-Z])/g, "$1-$2").toLowerCase()}`;
    } catch (error) {
      if (!(error instanceof TypeError)) throw error;
    }
    await delay(100);
  }
  throw new Error(`no WebKit inspector target on ${address}`);
}

async function connect(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  let nextId = 1;
  let targetId = null;
  await new Promise((resolveOpen, rejectOpen) => {
    socket.addEventListener("open", resolveOpen, { once: true });
    socket.addEventListener("error", () => rejectOpen(new Error(`cannot connect to ${url}`)), { once: true });
  });
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(String(data));
    if (message.method === "Target.targetCreated" && message.params?.targetInfo?.type === "page") targetId = message.params.targetInfo.targetId;
    if (message.method !== "Target.dispatchMessageFromTarget") return;
    const nested = JSON.parse(message.params.message);
    const request = pending.get(nested.id);
    if (!request) return;
    pending.delete(nested.id);
    nested.error ? request.reject(new Error(nested.error.message)) : request.resolve(nested.result);
  });
  for (let attempt = 0; targetId === null && attempt < 100; attempt += 1) await delay(10);
  if (targetId === null) throw new Error("WebKit inspector did not announce a page target");
  return {
    async evaluate(expression) {
      const id = nextId;
      nextId += 1;
      const result = new Promise((resolveResult, rejectResult) => pending.set(id, { resolve: resolveResult, reject: rejectResult }));
      socket.send(JSON.stringify({ id: 0, method: "Target.sendMessageToTarget", params: { targetId, message: JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, returnByValue: true } }) } }));
      return result;
    },
    close() { socket.close(); },
  };
}

const setup = JSON.parse(setupText);
const closedSetup = JSON.parse(closedSetupText);
const expression = `
  window.__hostParityResult = null;
  window.__hostParityError = null;
  (async () => {
    try {
      const invoke = window.__TAURI_INTERNALS__?.invoke;
      if (typeof invoke !== "function") throw new Error("Tauri invoke bridge unavailable");
      const observedEvents = [];
      const transformCallback = window.__TAURI_INTERNALS__?.transformCallback;
      if (typeof transformCallback !== "function") throw new Error("Tauri event callback registrar unavailable");
      const handler = transformCallback((event) => observedEvents.push(event.payload), false);
      const eventId = await invoke("plugin:event|listen", { event: "engine-event", target: { kind: "Any" }, handler });
      const unlisten = () => invoke("plugin:event|unlisten", { event: "engine-event", eventId });
      const sessionId = await invoke("create_session", { setup: ${JSON.stringify(setup)}, seed: "7" });
      try {
        const before = await invoke("snapshot", { sessionId });
        const civilBefore = await invoke("civil_date", { sessionId, generation: "1" });
        const normalSteps = [];
        for (let step = 0; step < ${JSON.stringify(setup.ticks_per_day)}; step += 1) {
          normalSteps.push(await invoke("host_parity_step", { sessionId, generation: "1" }));
        }
        const civil = await invoke("host_parity_advance_civil_day", { sessionId, generation: "1" });
        let duplicateRejected = false;
        try { await invoke("host_parity_advance_civil_day", { sessionId, generation: "1" }); } catch { duplicateRejected = true; }
        await new Promise((resolve) => setTimeout(resolve, 0));
        const civilAfter = await invoke("civil_date", { sessionId, generation: "1" });
        const afterCivil = await invoke("snapshot", { sessionId });
        const closedSessionId = await invoke("create_session", { setup: ${JSON.stringify(closedSetup)}, seed: "7" });
        let closedDay;
        let closedReports;
        try {
          for (let step = 0; step < ${JSON.stringify(closedSetup.ticks_per_day)}; step += 1) {
            await invoke("host_parity_step", { sessionId: closedSessionId, generation: "1" });
          }
          await invoke("host_parity_advance_civil_day", { sessionId: closedSessionId, generation: "1" });
          closedDay = await invoke("host_parity_advance_civil_day", { sessionId: closedSessionId, generation: "1" });
          closedReports = await invoke("public_reports", { sessionId: closedSessionId, generation: "1", query: { company_id: "C-600101", cursor: null, page_size: 20 } });
        } finally { await invoke("stop_session", { sessionId: closedSessionId }); }
        const concurrent = await Promise.all([
          invoke("snapshot", { sessionId }),
          invoke("civil_date", { sessionId, generation: "1" }),
        ]);
        const saved = await invoke("save_session", { sessionId });
        let corruptRejected = false;
        try { await invoke("restore_session", { sessionId, generation: "1", slot: {} }); } catch { corruptRejected = true; }
        const afterCorrupt = await invoke("snapshot", { sessionId });
        const restored = await invoke("restore_session", { sessionId, generation: "1", slot: saved });
        let staleRejected = false;
        let staleError = null;
        try { await invoke("npc_decision_diagnostics", { sessionId, generation: "0", account: 0 }); } catch (error) { staleRejected = true; staleError = String(error); }
        const diagnostics = await invoke("npc_decision_diagnostics", { sessionId, generation: restored.generation, account: 0 });
        window.__hostParityResult = { sessionId, before, civilBefore, normalSteps, civil, duplicateRejected, civilAfter, afterCivil, closedDay, closedReports, concurrent, observedEvents, saved, afterCorrupt, restored, corruptRejected, staleRejected, staleError, diagnostics };
      } finally { await invoke("stop_session", { sessionId }); await unlisten(); }
    } catch (error) {
      window.__hostParityError = String(error);
    }
  })();
  "started"
`;

const client = await connect(await inspectorTarget());
try {
  let bridgeReady = false;
  for (let attempt = 0; attempt < 400; attempt += 1) {
    const bridge = await client.evaluate("typeof window.__TAURI_INTERNALS__?.invoke");
    if (bridge.result?.value === "function") {
      bridgeReady = true;
      break;
    }
    await delay(25);
  }
  if (!bridgeReady) throw new Error("Tauri invoke bridge did not become ready");
  const started = await client.evaluate(expression);
  if (started.wasThrown || started.result?.value !== "started") throw new Error(`Tauri parity probe did not start: ${JSON.stringify(started)}`);
  for (let attempt = 0; attempt < 400; attempt += 1) {
    const state = await client.evaluate("JSON.stringify({ result: window.__hostParityResult, error: window.__hostParityError })");
    if (typeof state.result?.value !== "string") throw new Error(`invalid Tauri parity state: ${JSON.stringify(state)}`);
    const parsed = JSON.parse(state.result.value);
    if (parsed.error) throw new Error(`Tauri parity probe failed: ${parsed.error}`);
    if (parsed.result) {
      console.log(JSON.stringify(parsed.result));
      break;
    }
    await delay(25);
    if (attempt === 399) throw new Error("Tauri parity probe timed out");
  }
} finally { client.close(); }
