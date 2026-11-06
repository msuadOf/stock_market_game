// V11 tests-first 验收源码：真实隔离 Chromium 主线程负责 DedicatedWorker 生命周期与控制面。
import { createFacadeSharedMemory } from "/apps/web/src/host/facade-wasm-module.ts";

declare global { interface Window { c2cBusinessResult?: unknown } }
const engineUrl = "/agents/branch-completion/mixed-calendar-c2c/v11-business-source-33/browser-business-engine.ts";
const logs: unknown[] = [];
let engine: Worker | undefined;
let control: MessageChannel | undefined;
let deadline: ReturnType<typeof setTimeout> | undefined;

function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
async function checkedFetch(path: string) {
  const response = await fetch(path);
  check(response.ok, `实际输入请求失败：${path} status=${response.status}`);
  return response;
}

async function run() {
  check(crossOriginIsolated, "业务验收必须是真实 crossOriginIsolated 浏览器");
  const [module, fixtureJson, scriptJson] = await Promise.all([
    checkedFetch("/wasm-diagnostics-pkg/web_wasm_bg.wasm").then((response) => WebAssembly.compileStreaming(response)),
    checkedFetch("/__c2c_fixture.json").then((response) => response.text()),
    checkedFetch("/__c2c_script.json").then((response) => response.text()),
  ]);
  const memory = createFacadeSharedMemory({ isolated: crossOriginIsolated });
  check(memory.buffer instanceof SharedArrayBuffer, "production helper必须创建shared memory");
  engine = new Worker(engineUrl, { type: "module" });
  control = new MessageChannel();
  const result = new Promise<unknown>((resolve, reject) => {
    deadline = setTimeout(() => reject(new Error("C2C单业务case超过10000ms，停止真实Worker")), 10000);
    engine!.onerror = (event) => reject(new Error(`真实业务worker失败：${event.message}`));
    engine!.onmessageerror = () => reject(new Error("真实业务worker结构化消息失败"));
    control!.port1.onmessage = ({ data }) => {
      logs.push({ control: data });
      // 当前wasm32生产市场路径为serial；出现业务线程申请即显式失败，不伪造Ready/Join。
      reject(new Error(`serial业务路径意外请求facade控制：${JSON.stringify(data)}`));
    };
    control!.port1.onmessageerror = () => reject(new Error("真实facade控制面消息失败"));
    control!.port1.start();
    engine!.onmessage = ({ data }) => {
      logs.push({ type: data.type });
      if (data.type === "driverFailure") reject(new Error(JSON.stringify(data)));
      else if (data.type === "engineReady") engine!.postMessage({ type: "business", fixtureJson, scriptJson });
      else if (data.type === "businessResult") resolve({ trace: data.trace, layoutCapacity: 16, mode: "real-browser-wasm32-serial", control: logs });
      else reject(new Error(`未知业务worker回执：${JSON.stringify(data)}`));
    };
    engine!.postMessage({ type: "bootstrap", module, memory, controlPort: control!.port2 }, [control!.port2]);
  });
  return await result;
}

try {
  const result = await run();
  window.c2cBusinessResult = { ok: true, result };
} catch (error) {
  window.c2cBusinessResult = { ok: false, error: String(error), stack: error instanceof Error ? error.stack : undefined, logs };
} finally {
  if (deadline !== undefined) clearTimeout(deadline);
  engine?.terminate();
  control?.port1.close();
}
