import { createFacadeChildBootstrap, type FacadeChildGlue } from "./facade-child-bootstrap.ts";
import { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView, requireExitIntent } from "./engine-clock-env.ts";

const scope = self as unknown as {
  readonly location: Location;
  readonly close: () => void;
  readonly postMessage: (message: unknown) => void;
  readonly reportError?: (error: Error) => void;
  readonly addEventListener: (type: "message", listener: (event: MessageEvent<unknown>) => void) => void;
};
const bootstrap = createFacadeChildBootstrap({
  environment: { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView, requireExitIntent },
  async loadGlue(diagnostics) {
    if (diagnostics) {
      if (!import.meta.env.DEV) throw new Error("正式构建禁止加载 diagnostics WASM");
      const moduleUrl = new URL("/wasm-diagnostics-pkg/web_wasm.js", scope.location.origin);
      return await import(/* @vite-ignore */ moduleUrl.href) as FacadeChildGlue;
    }
    return await import("../../wasm-pkg/web_wasm.js") as unknown as FacadeChildGlue;
  },
  postControl(message, input) { input.controlPort.postMessage(message); },
  postValidationFailure(message) { scope.postMessage(message); },
  reportError(error) {
    if (typeof scope.reportError === "function") scope.reportError(error);
    else queueMicrotask(() => { throw error; });
  },
  close() { scope.close(); },
});

scope.addEventListener("message", (event) => {
  // 不串接 Promise 队列；加载期间的重复 bootstrap 必须立即拒绝并关闭新收到的端。
  void bootstrap.handleMessage(event.data);
});
