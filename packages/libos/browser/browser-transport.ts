// 浏览器传输原语；不理解协议消息、会话状态或重连策略。

/** 由调用方声明入口，Vite 给出打包 URL；未打包的直接运行环境使用原入口。 */
export function browserWorkerEntry(source: string, base: string, assets: Readonly<Record<string, string>> | undefined): URL {
  if (assets === undefined) return new URL(source, base);
  const entry = assets[source];
  if (typeof entry !== "string" || entry.length === 0) throw new Error(`缺少打包后的 Worker 入口：${source}`);
  return new URL(entry, base);
}

export function spawnModuleWorker(entry: URL, name: string): Worker {
  return new Worker(entry, { type: "module", name });
}

export function transferMessageChannel(): MessageChannel {
  return new MessageChannel();
}

export function isMessagePort(value: unknown): value is MessagePort {
  return value instanceof MessagePort;
}

export interface OwnedMessagePort {
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  removeEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  postMessage(message: unknown): void;
  start(): void;
  close(): void;
}

/** 关闭后撤销引用，清理全部 listener；不会保留已转交的实际端口。 */
export function ownMessagePort(port: MessagePort | null): OwnedMessagePort {
  let current = port;
  port = null;
  const listeners = new Set<(event: MessageEvent) => void>();
  function requirePort(): MessagePort {
    if (current === null) throw new Error("business port 已关闭");
    return current;
  }
  return {
    addEventListener(type, listener) { requirePort().addEventListener(type, listener); listeners.add(listener); },
    removeEventListener(type, listener) { current?.removeEventListener(type, listener); listeners.delete(listener); },
    postMessage(message) { requirePort().postMessage(message); },
    start() { requirePort().start(); },
    close() {
      const closing = current;
      current = null;
      if (closing === null) return;
      const errors: unknown[] = [];
      for (const listener of listeners) {
        try { closing.removeEventListener("message", listener); } catch (error) { errors.push(error); }
      }
      listeners.clear();
      try { closing.close(); } catch (error) { errors.push(error); }
      if (errors.length > 0) throw new AggregateError(errors, "business port 清理失败");
    },
  };
}

export function openWebSocket(url: string, protocols: string[]): WebSocket {
  return new WebSocket(url, protocols);
}

/** HTML WebSocket 的 OPEN 是固定的 readyState 1；注入的连接也适用。 */
export function isWebSocketOpen(socket: Pick<WebSocket, "readyState"> | undefined | null): boolean {
  return socket?.readyState === 1;
}

export type TimeoutHandle = ReturnType<typeof globalThis.setTimeout>;
export type IntervalHandle = ReturnType<typeof globalThis.setInterval>;

export function scheduleTimeout(callback: () => void, delay: number): TimeoutHandle {
  return globalThis.setTimeout(callback, delay);
}

export function cancelTimeout(handle: TimeoutHandle): void {
  globalThis.clearTimeout(handle);
}

export function scheduleInterval(callback: () => void, delay: number): IntervalHandle {
  return globalThis.setInterval(callback, delay);
}

export function cancelInterval(handle: IntervalHandle): void {
  globalThis.clearInterval(handle);
}

/** 把当前执行上下文的单调时刻转换为别的 Worker 也能比较的时间坐标。 */
export function sharedTimeAt(monotonicMilliseconds: number): number {
  return performance.timeOrigin + monotonicMilliseconds;
}

export function reportBrowserError(error: unknown): void {
  if ("reportError" in globalThis && typeof globalThis.reportError === "function") globalThis.reportError(error);
  else queueMicrotask(() => { throw error; });
}
