export type StartupMode = "local" | "remote";

export type StartupTarget =
  | { readonly kind: "wasm" }
  | { readonly kind: "tauri" }
  | { readonly kind: "remote"; readonly baseUrl: string };

export interface WasmEnvironment {
  readonly isSecureContext: boolean;
}

export function initialStartupTarget(buildMode: string): StartupTarget | null {
  return buildMode === "e2e" ? { kind: "wasm" } : null;
}

export function browserWasmEnvironment(): WasmEnvironment {
  // 批 9 A10：crossOriginIsolated/SharedArrayBuffer 门禁随 wasm 多核退役——
  // 串行 wasm 无跨源隔离需求。
  return {
    isSecureContext: globalThis.isSecureContext === true,
  };
}

export function assertWasmEnvironment(environment: WasmEnvironment): void {
  const missing = [
    ...(!environment.isSecureContext ? ["isSecureContext（安全上下文）"] : []),
  ];
  if (missing.length === 0) return;
  throw new Error(`浏览器本地 WASM 无法启动：缺少 ${missing.join("、")}。请使用 HTTPS（或浏览器认可的回环地址安全上下文）；也可返回启动界面选择远程 Server。不会降级也不会自动切换宿主。请复制此错误反馈部署环境。`);
}

export function validateRemoteServerAddress(value: string): string {
  const address = value.trim();
  if (!/^https?:\/\/[^/]/i.test(address) || /[\s\\?#]/.test(address)) {
    throw new Error("Server 地址必须是完整 HTTP(S) 基地址，可包含端口和路径前缀，但不能包含空白、反斜杠、查询或片段。");
  }
  let url: URL;
  try {
    url = new URL(address);
  } catch (error) {
    throw new Error("Server 地址无效，请检查主机、IPv6 括号和端口。", { cause: error });
  }
  if (url.username !== "" || url.password !== "") {
    throw new Error("Server 地址不能包含用户名或密码；本次启动选择不新增账户认证。");
  }
  return url.href.replace(/\/+$/, "");
}

export function resolveStartupTarget(mode: string, remoteAddress: string, desktop: boolean, readWasmEnvironment: () => WasmEnvironment): StartupTarget {
  if (mode === "remote") return { kind: "remote", baseUrl: validateRemoteServerAddress(remoteAddress) };
  if (mode !== "local") throw new Error(`未知启动模式：${mode}`);
  if (desktop) return { kind: "tauri" };
  assertWasmEnvironment(readWasmEnvironment());
  return { kind: "wasm" };
}

function initializationFailureReason(error: unknown): string {
  return error instanceof Error
    ? `${error.name}: ${error.message}`
    : String(error);
}

export function fatalWasmInitializationMessage(error: unknown): string {
  const reason = initializationFailureReason(error);
  return `WASM 多线程引擎初始化失败，游戏已中止（不会回退到主线程）。\n具体原因：${reason}`;
}

export function fatalDesktopInitializationMessage(error: unknown): string {
  const reason = initializationFailureReason(error);
  return `桌面引擎初始化失败，游戏已中止。\n具体原因：${reason}`;
}

export function fatalRemoteInitializationMessage(error: unknown): string {
  const reason = initializationFailureReason(error);
  return `远程引擎初始化失败，游戏已中止。\n具体原因：${reason}`;
}
