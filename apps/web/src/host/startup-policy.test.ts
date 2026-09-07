import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { fatalDesktopInitializationMessage, fatalWasmInitializationMessage } from "./startup-policy.ts";

describe("WASM startup failure policy", () => {
  it("turns the original multi-thread failure into a fatal, non-fallback error", () => {
    const message = fatalWasmInitializationMessage(
      new Error("SharedArrayBuffer is unavailable because COOP/COEP headers are missing"),
    );

    assert.match(message, /游戏已中止/);
    assert.match(message, /不会回退到主线程/);
    assert.match(message, /SharedArrayBuffer/);
    assert.match(message, /COOP\/COEP/);
  });
});

describe("desktop startup failure policy", () => {
  it("reports the Tauri engine cause without mislabeling it as a WASM failure", () => {
    const message = fatalDesktopInitializationMessage(new Error("create_session IPC failed"));

    assert.match(message, /桌面引擎初始化失败/);
    assert.match(message, /create_session IPC failed/);
    assert.doesNotMatch(message, /WASM/);
  });
});
