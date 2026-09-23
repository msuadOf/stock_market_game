import { setTimeout as delay } from "node:timers/promises";

const argumentValues = process.argv.slice(2);
const portFlagIndex = argumentValues.indexOf("--port");
const expectedFlagIndex = argumentValues.indexOf("--expect");

if (portFlagIndex < 0 || expectedFlagIndex < 0) {
  throw new Error("usage: node scripts/desktop/wayland-ipc-probe.mjs --port <port> --expect <unsupported|supported>");
}

const port = Number(argumentValues[portFlagIndex + 1]);
const expectedKind = argumentValues[expectedFlagIndex + 1];

if (!Number.isSafeInteger(port) || port <= 0 || port > 65_535) {
  throw new Error(`invalid inspector port: ${argumentValues[portFlagIndex + 1]}`);
}
if (expectedKind !== "unsupported" && expectedKind !== "supported") {
  throw new Error(`invalid diagnostics expectation: ${expectedKind}`);
}

async function inspectorTarget() {
  const address = `http://127.0.0.1:${port}/`;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch(address, { signal: AbortSignal.timeout(1_000) });
      if (response.ok) {
        const page = await response.text();
        const target = page.match(/\/socket\/(\d+)\/(\d+)\/([A-Za-z-]+)/);
        if (target) {
          const targetType = target[3].replace(/([a-z])([A-Z])/g, "$1-$2").toLowerCase();
          return `ws://127.0.0.1:${port}/socket/${target[1]}/${target[2]}/${targetType}`;
        }
      }
    } catch (error) {
      if (!(error instanceof TypeError)) throw error;
    }
    await delay(100);
  }
  throw new Error(`no WebKit remote-inspector target on ${address}`);
}

async function connect(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  let nextId = 1;
  let targetId = null;

  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", () => reject(new Error(`could not connect to ${url}`)), { once: true });
  });
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(String(data));
    if (message.method === "Target.targetCreated" && message.params?.targetInfo?.type === "page") {
      targetId = message.params.targetInfo.targetId;
      return;
    }
    if (message.method === "Target.dispatchMessageFromTarget") {
      const nested = JSON.parse(message.params.message);
      const request = pending.get(nested.id);
      if (!request) return;
      pending.delete(nested.id);
      if (nested.error) request.reject(new Error(nested.error.message));
      else request.resolve(nested.result);
      return;
    }
    if (!Object.hasOwn(message, "id")) return;
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    if (message.error) request.reject(new Error(message.error.message));
    else request.resolve(message.result);
  });

  return {
    async request(method, params = {}) {
      for (let attempt = 0; targetId === null && attempt < 100; attempt += 1) {
        await delay(10);
      }
      if (targetId === null) throw new Error("remote inspector did not announce a web-page target");
      const id = nextId;
      nextId += 1;
      const response = new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
      });
      socket.send(JSON.stringify({
        id: 0,
        method: "Target.sendMessageToTarget",
        params: { targetId, message: JSON.stringify({ id, method, params }) },
      }));
      return response;
    },
    close() {
      socket.close();
    },
  };
}

const setup = {
  stocks: [{
    code: "600101",
    exchange: "Shanghai",
    initial_price: 1_000,
    category: "MainBoard",
    limit_pct: 0.1,
    tick: 1,
    total_shares: "100000",
    float_shares: 100_000,
  }],
  npcs: { retail_count: 0, inst_count: 1, hot_count: 0, retail_cash_median: 10_000_000 },
  config: {
    commission_rate: 0.00025,
    commission_min: 500,
    stamp_tax_rate: 0.0005,
    default_limit: 0.1,
    st_limit: 0.1,
    lot_size: 100,
    starting_cash: 1_000_000_000,
  },
  strategy_params: {
    retail: { arrival_rate: 0.8, order_size_mean: 200, chase_prob: 0.4, tick_cents: 1 },
    inst: { margin: 0.02, order_size: 500 },
    hot: { lookback: 3, trend_threshold: 0.01, order_size: 300 },
  },
  ticks_per_day: 30,
  auction_ticks: 0,
  closing_auction_ticks: 0,
  history_len: 20,
  t1_enabled: true,
  float_allocation: "Random",
  start_date: "2030-01-01",
  simulation_policy_id: "a-share-simulation-v1",
};

const initialExpression = `
  window.__waylandNativeIpcResult = null;
  window.__waylandNativeIpcError = null;
  const invoke = window.__TAURI_INTERNALS__.invoke;
  (async () => {
    const sessionId = await invoke("create_session", { setup: ${JSON.stringify(setup)}, seed: "7" });
    try {
      let malformedRejected = false;
      try {
        await invoke("npc_decision_diagnostics", { sessionId, generation: "1", account: "unsafe" });
      } catch (error) {
        malformedRejected = String(error).includes("account");
      }
      await invoke("set_speed", { sessionId, speed: { Fixed: 720 } });
      await invoke("resume_session", { sessionId });
      await new Promise((resolve) => setTimeout(resolve, 20));
      const current = await invoke("npc_decision_diagnostics", { sessionId, generation: "1", account: 1 });
      let staleRejected = false;
      try {
        await invoke("npc_decision_diagnostics", { sessionId, generation: "0", account: 1 });
      } catch (error) {
        staleRejected = String(error).includes("stale session generation 0");
      }
      window.__waylandNativeIpcResult = {
        malformedRejected,
        staleRejected,
        generation: current.generation,
        kind: current.value.kind,
        hasRecordsProperty: Object.hasOwn(current.value, "records"),
        records: Array.isArray(current.value.records) ? current.value.records.length : null,
      };
    } catch (error) {
      window.__waylandNativeIpcError = String(error);
    } finally {
      await invoke("stop_session", { sessionId });
    }
  })();
  "started";
`;

const target = await inspectorTarget();
const client = await connect(target);
try {
  let bridgeReady = false;
  for (let attempt = 0; attempt < 200; attempt += 1) {
    const bridge = await client.request("Runtime.evaluate", {
      expression: "typeof window.__TAURI_INTERNALS__?.invoke",
      returnByValue: true,
    });
    if (bridge.result?.value === "function") {
      bridgeReady = true;
      break;
    }
    await delay(25);
  }
  if (!bridgeReady) throw new Error("Tauri WebView IPC bridge did not become ready");
  const response = await client.request("Runtime.evaluate", {
    expression: initialExpression,
    objectGroup: "wayland-native-qa",
    returnByValue: true,
  });
  if (response.wasThrown || response.result?.value !== "started") {
    throw new Error(`Tauri IPC scenario did not start: ${JSON.stringify(response)}`);
  }
  let result = null;
  for (let attempt = 0; attempt < 200; attempt += 1) {
    const state = await client.request("Runtime.evaluate", {
      expression: "JSON.stringify({ result: window.__waylandNativeIpcResult, error: window.__waylandNativeIpcError })",
      returnByValue: true,
    });
    if (typeof state.result?.value !== "string") throw new Error(`invalid IPC probe state: ${JSON.stringify(state)}`);
    const parsed = JSON.parse(state.result.value);
    if (typeof parsed.error === "string") throw new Error(`Tauri IPC scenario rejected: ${parsed.error}`);
    if (parsed.result !== null && parsed.result !== undefined) {
      result = parsed.result;
      break;
    }
    await delay(25);
  }
  if (result?.error) throw new Error(`Tauri IPC scenario returned an error: ${result.error}`);
  if (!result || result.generation !== "1" || !result.malformedRejected || !result.staleRejected) {
    throw new Error(`Tauri IPC scenario did not reject malformed/stale requests: ${JSON.stringify(result)}`);
  }
  if (result.kind !== expectedKind) {
    throw new Error(`diagnostic kind ${result.kind} did not match expected ${expectedKind}`);
  }
  if (expectedKind === "unsupported" && (result.hasRecordsProperty || result.records !== null)) {
    throw new Error(`default diagnostics exposed records: ${JSON.stringify(result)}`);
  }
  if (expectedKind === "supported" && (!result.hasRecordsProperty || !Number.isInteger(result.records) || result.records < 1 || result.records > 128)) {
    throw new Error(`feature diagnostics records were not bounded/current: ${JSON.stringify(result)}`);
  }
  console.log(JSON.stringify({ expectedKind, generation: result.generation, kind: result.kind, hasRecordsProperty: result.hasRecordsProperty, records: result.records, malformedRejected: true, staleRejected: true }));
} finally {
  client.close();
}
