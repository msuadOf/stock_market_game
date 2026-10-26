import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { assertCheckedGamePath, verifyBuildReceiptChain, verifyNativeReceiptChain } from "./verify-v11-build-bindings.mjs";

const FINAL_SHA = "c".repeat(64);
const callerClaims = () => ({
  nativeSourceManifestSha256: FINAL_SHA,
  wasmSourceManifestSha256: FINAL_SHA,
  sourceManifest: { sha256: FINAL_SHA },
});

async function nativeReceiptFixture(run) {
  const parent = "/data1/baiyifan/workplace/stock_market_game/.tmp/mixed-calendar-c2c";
  await mkdir(parent, { recursive: true });
  const directory = await mkdtemp(path.join(parent, "synthetic-receipt-contract-"));
  // 这里只测试已知receipt字段关联；这些明确synthetic文件不冒真实Rust/WASM产物。
  async function file(name, value) {
    const bytes = Buffer.from(typeof value === "string" ? value : JSON.stringify(value));
    const filename = path.join(directory, name);
    await writeFile(filename, bytes, { flag: "wx" });
    return { path: filename, bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
  }
  try {
    const head = "a".repeat(40);
    const sourceManifest = await file("engine-source-before.json", { head, files: [] });
    const tracer = await file("synthetic-tracer.rs", "synthetic-test-only tracer source");
    const rlib = await file("synthetic-engine.rlib", "synthetic-test-only rlib bytes");
    const binary = await file("synthetic-tracer", "synthetic-test-only native bytes");
    const fixture = await file("fixture.json", { synthetic: true });
    const nativeTrace = await file("trace.json", { synthetic: true });
    const engine = { target: "engine", normal_rlib: true, test: false, sealed: rlib.path, bytes: rlib.bytes, sha256: rlib.sha256 };
    const compileArgv = ["node", "supervisor", "10000", "--", "rustc", tracer.path, "--extern", "engine=" + rlib.path, "-o", binary.path];
    const runArgv = ["node", "supervisor", "10000", "--", binary.path, fixture.path, nativeTrace.path];
    const nativeReceipts = {
      enginePlan: await file("build-plan.json", { head, sourceManifest: "engine-source-before.json" }),
      engineResult: await file("build-result.json", { exitCode: 0 }),
      engineBinding: await file("artifact-binding.json", { head, build_finished_success: true, artifacts: [engine] }),
      tracePlan: await file("trace-plan.json", { engine_artifact: engine, compile_outer_argv: compileArgv, run_outer_argv: runArgv, source: tracer, input: fixture }),
      traceCompile: await file("trace-compile.json", { wrapper_exit_code: 0, outer_argv: compileArgv, source_after: tracer.sha256, binary }),
      traceRun: await file("trace-run.json", { wrapper_exit_code: 0, outer_argv: runArgv, trace: nativeTrace }),
    };
    const manifest = { sourceHead: head, sourceManifest, fixture, nativeTrace, sourceFiles: [tracer], nativeReceipts };
    await run(manifest, file);
  } finally {
    await rm(directory, { recursive: true });
  }
}

describe("V11 初始化前来源门", { concurrency: 4 }, () => {
  it("R1拒绝只有caller同源声明而缺少真实native/WASM receipt的输入", async () => {
    await assert.rejects(verifyBuildReceiptChain(callerClaims()), /receipt/);
  });

  it("R1已知native链逐层接受source/build/rlib/binary/input/trace的真实文件hash", async () => {
    await nativeReceiptFixture(async (manifest) => {
      await verifyNativeReceiptChain(manifest);
    });
  });

  it("R1即使caller都自报新source也拒绝实际旧native build receipt", async () => {
    await nativeReceiptFixture(async (manifest) => {
      manifest.sourceHead = "b".repeat(40);
      manifest.nativeSourceManifestSha256 = manifest.sourceManifest.sha256;
      manifest.wasmSourceManifestSha256 = manifest.sourceManifest.sha256;
      await assert.rejects(verifyNativeReceiptChain(manifest), /source/);
    });
  });

  it("R1拒绝trace run绑定另一个输入JSON", async () => {
    await nativeReceiptFixture(async (manifest, file) => {
      manifest.fixture = await file("different-fixture.json", { synthetic: "another-input" });
      await assert.rejects(verifyNativeReceiptChain(manifest), /输入JSON/);
    });
  });

  it("R1没有正式WASM schema时即使native合法也在初始化前明确拒绝", async () => {
    await nativeReceiptFixture(async (manifest) => {
      await assert.rejects(verifyBuildReceiptChain(manifest), /WASM build receipt/);
      manifest.wasmReceipts = { syntheticUnapproved: true };
      await assert.rejects(verifyBuildReceiptChain(manifest), /schema尚未冻结/);
    });
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
