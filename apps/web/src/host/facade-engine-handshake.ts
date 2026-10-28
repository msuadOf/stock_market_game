import type { FacadeControlMessage } from "./facade-thread-binding.ts";
import type { FacadeSubstrate } from "./engine-supervisor.ts";
import { parseFacadeEngineConfigured } from "./facade-engine-protocol.ts";

interface HandshakeTimers {
  setTimeout(callback: () => void, milliseconds: number): number;
  clearTimeout(id: number): void;
}
/** 只在 engine 初始化期等一次主线程确认；运行期 spawn/join 不依赖 JS 收件。 */
export function createFacadeEngineHandshake(options: { readonly port: MessagePort; readonly onFailure: (error: unknown) => void; readonly timers?: HandshakeTimers }) {
  const { port, onFailure } = options;
  const timers = options.timers === undefined ? {
    setTimeout: (callback: () => void, milliseconds: number) => globalThis.setTimeout(callback, milliseconds) as unknown as number,
    clearTimeout: (id: number) => globalThis.clearTimeout(id),
  } : options.timers;
  let started = false, configured = false, closed = false;
  let timer: number | undefined;
  let resolve: (() => void) | undefined, reject: ((error: unknown) => void) | undefined;
  function close(): void {
    if (closed) return;
    closed = true; configured = false;
    if (timer !== undefined) { timers.clearTimeout(timer); timer = undefined; }
    port.removeEventListener("message", receive); port.removeEventListener("messageerror", messageError); port.close();
  }
  function fail(error: unknown): void {
    if (closed) return;
    const pending = reject; resolve = undefined; reject = undefined;
    close();
    if (pending !== undefined) pending(error); else onFailure(error);
  }
  function receive(event: MessageEvent<unknown>): void {
    if (closed) return;
    try {
      parseFacadeEngineConfigured(event.data);
      if (!started || configured) throw new Error("facadeEngineConfigured 提前或重复发送");
      configured = true;
      if (timer !== undefined) { timers.clearTimeout(timer); timer = undefined; }
      const done = resolve; resolve = undefined; reject = undefined; done?.();
    } catch (error) { fail(error); }
  }
  function messageError(): void { fail(new Error("engine 控制端 messageerror")); }
  port.addEventListener("message", receive); port.addEventListener("messageerror", messageError); port.start();
  function assertConfigured(): void {
    if (!configured || closed) throw new Error("engine 尚未 configured 或控制端已关闭");
  }
  return {
    configure(substrate: FacadeSubstrate): Promise<void> {
      if (started || closed) throw new Error("engine substrate 只允许发送一次");
      started = true;
      return new Promise<void>((done, failed) => {
        resolve = done; reject = failed;
        timer = timers.setTimeout(() => fail(new Error("engine configured 等待超过 10000ms")), 10000);
        try { port.postMessage({ type: "facadeEngineSubstrate", ...substrate }); }
        catch (error) { fail(error); }
      });
    },
    assertConfigured,
    postControl(message: FacadeControlMessage): void { assertConfigured(); port.postMessage(message); },
    dispose(): void {
      if (closed) return;
      const pending = reject; reject = undefined; resolve = undefined; close(); pending?.(new Error("engine 初始化已 dispose"));
    },
  };
}
