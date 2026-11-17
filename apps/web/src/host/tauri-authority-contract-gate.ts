/** 纯验证入口门禁；默认分发既不安装全局桥，也不注册私有owner身份。 */
export function requireTauriContractEnvironment(env: { DEV?: boolean; MODE?: string; VITE_SESSION_AUTHORITY_CONTRACT?: string } | undefined): void {
  if (env?.MODE !== "e2e" || env.VITE_SESSION_AUTHORITY_CONTRACT !== "1") {
    throw new Error("Desktop产品合同桥仅允许e2e独立验证构建");
  }
}
export function requireTauriContractBackend(value: unknown): asserts value is { verification: true; scenarioVersion: 1; supportedActions: string[] } {
  if (value === null || typeof value !== "object") throw new Error("Desktop缺少实际host-parity验证能力");
  const source = value as Record<string, unknown>;
  if (source.verification !== true || source.scenarioVersion !== 1 || !Array.isArray(source.supportedActions) || !source.supportedActions.every(action => typeof action === "string") || !source.supportedActions.includes("initial") || !source.supportedActions.includes("replaced")) {
    throw new Error("Desktop缺少实际host-parity验证能力");
  }
}
