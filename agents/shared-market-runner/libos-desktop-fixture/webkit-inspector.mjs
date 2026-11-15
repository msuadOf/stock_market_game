// 只连接这次云端 fixture 的本地 WebKit Inspector。
const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

export async function waitFor(label, read, budgetMs) {
  const deadline = performance.now() + budgetMs;
  let last;
  while (performance.now() < deadline) {
    try {
      const next = await read();
      if (next !== undefined && next !== false) return next;
    } catch (error) {
      last = error;
    }
    await sleep(100);
  }
  throw new Error(`${label} 超时`, last === undefined ? undefined : { cause: last });
}

export async function connectWebKitInspector(port) {
  if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Inspector fixture 端口无效");
  const loopback = `127.0.0.1:${port}`;
  const path = await waitFor("WebKit Inspector target", async () => {
    const response = await fetch(`http://${loopback}/`, { signal: AbortSignal.timeout(1_000), redirect: "error" });
    if (!response.ok) throw new Error(`Inspector HTTP ${response.status}`);
    const text = await response.text();
    const paths = [...text.matchAll(/\/socket\/\d+\/\d+\/WebPage\b/g)].map(([value]) => value);
    if (paths.length > 1) throw new Error("本地 fixture 只能有一个 WebPage target");
    return paths[0];
  }, 15_000);
  const socket = new WebSocket(`ws://${loopback}${path}`);
  const connected = Promise.withResolvers();
  const page = Promise.withResolvers();
  const pending = new Map();
  let serial = 0;
  socket.addEventListener("open", connected.resolve, { once: true });
  socket.addEventListener("error", () => {
    const error = new Error("WebKit Inspector WebSocket 无法连接");
    connected.reject(error);
    page.reject(error);
  }, { once: true });
  socket.addEventListener("close", () => {
    const error = new Error("WebKit Inspector WebSocket 已关闭");
    connected.reject(error);
    page.reject(error);
    for (const request of pending.values()) request.reject(error);
  });
  socket.addEventListener("message", (event) => {
    try {
      const envelope = JSON.parse(String(event.data));
      if (envelope.method === "Target.targetCreated" && envelope.params.targetInfo.type === "page") {
        page.resolve(envelope.params.targetInfo.targetId);
        return;
      }
      if (envelope.method === "Target.dispatchMessageFromTarget") {
        const message = JSON.parse(envelope.params.message);
        const request = pending.get(message.id);
        if (request === undefined) return;
        if (message.error === undefined) request.resolve(message.result);
        else request.reject(new Error(`WebKit Inspector：${JSON.stringify(message.error)}`));
      } else if (envelope.error !== undefined) {
        pending.get(envelope.id)?.reject(new Error(`WebKit Inspector：${JSON.stringify(envelope.error)}`));
      }
    } catch (error) {
      for (const request of pending.values()) request.reject(error);
    }
  });
  const startup = setTimeout(() => page.reject(new Error("WebKit Inspector 未报告 WebPage target")), 4_000);
  let targetId;
  try { [, targetId] = await Promise.all([connected.promise, page.promise]); }
  catch (error) { socket.close(); throw error; }
  finally { clearTimeout(startup); }

  async function call(method, params) {
    const id = ++serial;
    const envelopeId = ++serial;
    const request = Promise.withResolvers();
    pending.set(id, request);
    pending.set(envelopeId, request);
    const alarm = setTimeout(() => request.reject(new Error(`${method} 超时`)), 4_000);
    try {
      socket.send(JSON.stringify({
        id: envelopeId, method: "Target.sendMessageToTarget",
        params: { targetId, message: JSON.stringify({ id, method, params }) },
      }));
      return await request.promise;
    } finally {
      clearTimeout(alarm);
      pending.delete(id);
      pending.delete(envelopeId);
    }
  }

  return {
    close() { socket.close(); },
    async evaluate(expression) {
      const { result, wasThrown } = await call("Runtime.evaluate", { expression, returnByValue: true });
      if (wasThrown) throw new Error(`WebKit Browser JS 异常：${JSON.stringify(result)}`);
      if (result === undefined || result.type === "undefined" || !Object.hasOwn(result, "value")) throw new Error(`WebKit Browser JS 缺少结果：${JSON.stringify(result)}`);
      return result.value;
    },
  };
}
