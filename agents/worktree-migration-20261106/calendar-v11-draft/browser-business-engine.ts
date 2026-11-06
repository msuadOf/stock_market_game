// V11 tests-first 验收源码：真实 DedicatedWorker 中复用 production env/bootstrap，执行相同公共 JSON 与业务脚本。
import { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView } from "/apps/web/src/host/engine-clock-env.ts";
import { instantiateFacadeModule } from "/apps/web/src/host/facade-wasm-module.ts";
import { normalizeSerdeMaps } from "/apps/web/src/host/serde-normalize.ts";

type BusinessScript = {
  closed: number;
  open: number;
  retainedDate: string;
  finalRetainedDate: string;
  steps: { intents: unknown[] }[];
};

let glue: any;
let memory: WebAssembly.Memory | undefined;
let ready = false;
let consumed = false;

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function observe(handle: number, script: BusinessScript) {
  const civilDate = glue.civil_date(handle);
  const currentMinute: Record<string, unknown> = {};
  const retained: Record<string, unknown> = {};
  for (const stock of [script.closed, script.open]) {
    currentMinute[String(stock)] = normalizeSerdeMaps(glue.current_minute_history(handle, { stock }));
    if (civilDate > script.retainedDate) {
      retained[String(stock)] = normalizeSerdeMaps(glue.market_history(handle, {
        stock, date_from: script.retainedDate, date_to: civilDate > script.finalRetainedDate ? script.finalRetainedDate : script.retainedDate, after: null, page_size: 2,
      }));
    }
  }
  const dayEnd = normalizeSerdeMaps(glue.save(handle));
  return {
    civilDate,
    snapshot: normalizeSerdeMaps(glue.snapshot(handle)),
    runtimeSnapshot: normalizeSerdeMaps(glue.runtime_snapshot(handle)),
    workingOrders: normalizeSerdeMaps(glue.player_working_orders(handle)),
    currentMinute,
    retained,
    dayEndConfirmations: dayEnd.personal_trade_confirmations,
    dayEndAccounts: dayEnd.snapshot.accounts,
  };
}

self.onmessage = async ({ data }) => {
  let handle: number | undefined;
  try {
    if (data.type === "bootstrap") {
      check(!ready && !consumed, "每个真实 engine realm 只能绑定一次来源");
      check(crossOriginIsolated, "必须是真实隔离浏览器，不伪造 crossOriginIsolated");
      memory = data.memory;
      check(memory instanceof WebAssembly.Memory && memory.buffer instanceof SharedArrayBuffer, "必须使用宿主传入的真实 shared memory");
      bindFacadeThreadEnvironment({ memory, role: "engine", postControl(message) { data.controlPort.postMessage(message); } });
      glue = await import(/* @vite-ignore */ self.location.origin + "/wasm-diagnostics-pkg/web_wasm.js");
      const raw = instantiateFacadeModule(glue, data.module, memory) as any;
      check(raw.memory === memory, "glue 必须复用传入 memory");
      for (const name of ["facade_slot_init", "facade_slot_layout", "restore_json", "enqueue", "step", "snapshot", "runtime_snapshot", "player_working_orders", "civil_date", "current_minute_history", "market_history", "save", "drop_session"]) {
        check(typeof glue[name] === "function", `同源真实 glue 缺少 ${name}`);
      }
      glue.facade_slot_init(16);
      const layout = normalizeSerdeMaps(glue.facade_slot_layout());
      installFacadeSlotLayout(layout);
      check(getArenaView().words.buffer === memory.buffer, "production env view 必须绑定同一真实 memory");
      ready = true;
      self.postMessage({ type: "engineReady", layout });
      return;
    }
    check(data.type === "business" && ready && !consumed, "只接受bootstrap后唯一业务脚本");
    consumed = true;
    check(typeof data.fixtureJson === "string" && typeof data.scriptJson === "string", "必须保留同字节公共输入与脚本");
    const script: BusinessScript = JSON.parse(data.scriptJson);
    check(Number.isInteger(script.closed) && Number.isInteger(script.open) && script.closed !== script.open, "业务脚本必须绑定两只不同证券");
    check(Array.isArray(script.steps) && script.steps.length > 0, "业务脚本必须有实际步骤");
    handle = glue.restore_json(data.fixtureJson);
    check(Number.isSafeInteger(handle) && handle > 0, "真实 strict restore 必须成功");
    const trace = { closed: script.closed, open: script.open, initial: observe(handle, script), frames: [] as unknown[] };
    for (const step of script.steps) {
      check(Array.isArray(step.intents), "每步必须显式声明完整 intents");
      for (const intent of step.intents) glue.enqueue(handle, intent);
      // step 是官方 SessionRegistry 公共编排；不使用 step_n 跨日或直接驱动私有市场。
      trace.frames.push({ update: normalizeSerdeMaps(glue.step(handle)), view: observe(handle, script) });
    }
    glue.drop_session(handle);
    handle = undefined;
    // 先交付完整实际输出，独立oracle在宿主保原件后执行。
    self.postMessage({ type: "businessResult", trace });
  } catch (error) {
    let cleanupError: unknown;
    if (handle !== undefined) {
      try { glue.drop_session(handle); } catch (failure) { cleanupError = failure; }
    }
    self.postMessage({ type: "driverFailure", message: String(error), stack: error instanceof Error ? error.stack : undefined,
      cleanupError: cleanupError === undefined ? undefined : String(cleanupError) });
  }
};
