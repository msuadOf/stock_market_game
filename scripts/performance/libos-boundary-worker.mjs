// 只在隔离的真实 Browser Worker 中测量，不接受主线程或 Node WASM 冒充。
try {
  if (!(self instanceof DedicatedWorkerGlobalScope) || !crossOriginIsolated) {
    throw new Error("需要 crossOriginIsolated DedicatedWorker");
  }
  const module = await WebAssembly.compileStreaming(fetch("/probe.wasm"));
  const imports = WebAssembly.Module.imports(module);
  if (imports.length !== 3 || !["env:memory:memory", "env:engine_clock_now:function", "env:benchmark_relative_now:function"]
    .every((entry) => imports.some(({ module, name, kind }) => [module, name, kind].join(":") === entry))) {
    throw new Error("WASM 探针 import 边界漂移：" + JSON.stringify(imports));
  }
  // 构建命令显式将 probe 的最小共享内存固定为 16MiB；最大与链接 ABI 上限一致。
  // 在独立的实例中用同源 host import 计数核对实际执行路径；
  // 正式测量实例不插入计数，也不复用探针内存/时钟锚。
  const clockWitness = await (async () => {
    let reads = 0;
    const probe = await WebAssembly.instantiate(module, { env: {
      memory: new WebAssembly.Memory({ initial: 256, maximum: 32768, shared: true }),
      engine_clock_now: () => { reads++; return performance.timeOrigin + performance.now(); },
      benchmark_relative_now: () => performance.now(),
    } });
    const {
      libos_benchmark_init: init, libos_benchmark_batch: batch, libos_benchmark_contracts: contracts,
      libos_benchmark_probe_version: version,
    } = probe.exports;
    if (typeof version !== "function" || version() !== 2) throw new Error("必须重建 v2 性能探针，不能混用旧验证包");
    init();
    if (typeof contracts !== "function") throw new Error("缺少 verification-harness 契约验证导出");
    contracts();
    const operations = 32;
    const observations = [];
    for (const [caseId, perCase] of [[0, operations + 1], [1, 2 * operations]]) {
      for (const facade of [0, 1]) {
        const before = reads;
        batch(caseId, facade, operations);
        if (reads - before !== perCase) {
          throw new Error("直连与门面的实际 host clock 调用契约不等价，操作码：" + caseId + "，臂：" + facade);
        }
        observations.push({ caseId, facade: Boolean(facade), hostClockReads: reads - before });
      }
    }
    return { probeVersion: 2, operations, observations, contractsPassed: [
      "clock-duration", "worker-delivery", "coordinator-delivery",
    ] };
  })();
  if (new URL(location.href).searchParams.get("contracts") === "1") {
    postMessage({ clockWitness });
  } else {
    const memory = new WebAssembly.Memory({ initial: 256, maximum: 32768, shared: true });
    if (!(memory.buffer instanceof SharedArrayBuffer)) throw new Error("探针没有实际共享线性内存");
    const instance = await WebAssembly.instantiate(module, { env: {
      memory, engine_clock_now: () => performance.timeOrigin + performance.now(),
      benchmark_relative_now: () => performance.now(),
    } });
    const { libos_benchmark_init: init, libos_benchmark_batch: batch } = instance.exports;
    if (typeof init !== "function" || typeof batch !== "function") throw new Error("WASM 探针缺所需导出");
    init();
    let quantum = Infinity;
    let previous = performance.now();
    // Worker 实测最小正跳变，不把 API 声称的理论精度写成实测值。
    for (let i = 0; i < 100_000; i++) {
      const now = performance.now();
      if (now > previous) quantum = Math.min(quantum, now - previous);
      previous = now;
    }
    if (!Number.isFinite(quantum)) throw new Error("无法观测 Chromium 时钟量化");
    const observedResolutionNs = Math.ceil(quantum * 1_000_000);
    const samples = [];
    for (const [caseId, name] of [[0, "clock-now"], [1, "buffered-recv"], [3, "idle-deadline"]]) {
      // 冷等待真实休眠，不能沿用热循环的 4096 次初值。
      let operations = name === "idle-deadline" ? 16 : 4096;
      for (;;) {
        const a = batch(caseId, 0, operations);
        const b = batch(caseId, 1, operations);
        if (Math.min(a, b) >= Math.max(2_000_000, 200 * observedResolutionNs)) break;
        if (operations >= 1 << 20) throw new Error(name + " 无法在有界 batch 中超出计时分辨率");
        operations *= 2;
      }
      for (let pair = 0; pair < 31; pair++) {
        const forward = pair % 2 === 0;
        const first = batch(caseId, forward ? 0 : 1, operations);
        const second = batch(caseId, forward ? 1 : 0, operations);
        samples.push({
          case: name, operations, segments: 1, observedResolutionNs,
          order: forward ? "direct-facade" : "facade-direct",
          directNs: forward ? first : second, facadeNs: forward ? second : first,
        });
      }
    }
    postMessage({ samples, clockWitness, memoryPages: memory.buffer.byteLength / 65536 });
  }
} catch (error) {
  postMessage({ error: error.name + ": " + error.message });
}
