import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { assertCheckedGamePath, verifyBuildReceiptChain } from "./verify-v11-build-bindings.mjs";

const FINAL_SHA = "c".repeat(64);
const callerClaims = () => ({
  nativeSourceManifestSha256: FINAL_SHA,
  wasmSourceManifestSha256: FINAL_SHA,
  sourceManifest: { sha256: FINAL_SHA },
});

describe("V11 初始化前来源门", { concurrency: 8 }, () => {
  it("R1拒绝只有caller同源声明而缺少真实native/WASM receipt的输入", async () => {
    await assert.rejects(verifyBuildReceiptChain(callerClaims()), /receipt/);
  });

  it("R2拒绝目录合规game与manifest另一个old.wasm不相同", () => {
    assert.throws(() => assertCheckedGamePath(["web_wasm_bg.wasm"], "/fixture/pkg", "/fixture/pkg/old.wasm"), /WASM/);
  });

  it("R2拒绝checker没有唯一返回实际game", () => {
    assert.throws(() => assertCheckedGamePath([], "/fixture/pkg", "/fixture/pkg/web_wasm_bg.wasm"), /WASM/);
    assert.throws(() => assertCheckedGamePath(["web_wasm_bg.wasm", "other.wasm"], "/fixture/pkg", "/fixture/pkg/web_wasm_bg.wasm"), /WASM/);
  });

  it("R2接受checker实际校验的同一个绝对game路径", () => {
    assert.equal(assertCheckedGamePath(["web_wasm_bg.wasm"], "/fixture/pkg", "/fixture/pkg/web_wasm_bg.wasm"), "/fixture/pkg/web_wasm_bg.wasm");
  });
});
