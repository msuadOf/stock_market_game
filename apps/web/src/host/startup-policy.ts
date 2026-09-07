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
