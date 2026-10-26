// V11 准备入口：只消费同一次最终构建的实际 wasm/glue/native trace 与公共输入。
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { registerHooks } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

const [manifestFile, outputFile] = process.argv.slice(2);
assert(manifestFile && outputFile && process.argv.length === 4, "必须提供冻结 manifest 与新的结果文件路径");
const manifest = JSON.parse(await readFile(manifestFile, "utf8"));
assert.equal(manifest.kind, "c2c-native-wasm-public-fixture");
assert.match(manifest.sourceHead, /^[a-f0-9]{40}$/);
assert.equal(manifest.engineRuntime, "wasm-serial");
assert.equal(manifest.fixtureCapacity, 16, "16仅是本fixture声明的arena容量，不是NPC组数或hardware预算");
assert.equal(manifest.sourceRoot, path.resolve(manifest.sourceRoot));
assert.equal(manifest.outerArgv[0], "/data1/baiyifan/workplace/stock_market_game/.tmp/node24-env/node");
assert.equal(manifest.outerArgv[1], "/data1/baiyifan/workplace/stock_market_game/.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs");
assert.equal(manifest.outerArgv[2], "10000");

const source = (relative) => path.join(manifest.sourceRoot, relative);
const { hashArtifactFiles } = await import(pathToFileURL(source("scripts/full-regression-artifact-hash.mjs")).href);
const artifacts = [manifest.wasm, manifest.glue, manifest.fixture, manifest.nativeTrace, manifest.sourceManifest, ...manifest.sourceFiles];
assert(artifacts.length >= 8, "必须绑定wasm/glue/input/native与实际env/normalizer/runtime源码");
for (const artifact of artifacts) {
  assert.equal(artifact.path, path.resolve(artifact.path));
  assert.match(artifact.sha256, /^[a-f0-9]{64}$/);
  assert(Number.isSafeInteger(artifact.bytes) && artifact.bytes > 0);
}
assert.equal(new Set(artifacts.map((item) => item.path)).size, artifacts.length, "manifest不能重复或拼接相同文件身份");
async function verifyArtifacts() {
  await hashArtifactFiles(artifacts, {
    cpuCount: 8,
    prepare: (artifact) => artifact.path,
    verify: (actual, artifact) => {
      assert.equal(actual.bytes, artifact.bytes, artifact.path);
      assert.equal(actual.sha256, artifact.sha256, artifact.path);
    },
  });
}
await verifyArtifacts();
assert.equal(manifest.nativeSourceManifestSha256, manifest.sourceManifest.sha256);
assert.equal(manifest.wasmSourceManifestSha256, manifest.sourceManifest.sha256);
const sourceManifest = JSON.parse(await readFile(manifest.sourceManifest.path, "utf8"));
assert.equal(sourceManifest.head, manifest.sourceHead);
assert(Array.isArray(sourceManifest.files) && sourceManifest.files.length > 0);
async function verifySources() {
  await hashArtifactFiles(sourceManifest.files, {
    cpuCount: 8,
    prepare: (entry) => {
      const filename = source(entry.path);
      assert(filename.startsWith(manifest.sourceRoot + path.sep), "源码路径不得离开冻结根目录");
      return filename;
    },
    verify: (actual, entry) => {
      assert.equal(actual.bytes, entry.bytes, entry.path);
      assert.equal(actual.sha256, entry.sha256, entry.path);
    },
  });
}
await verifySources();
const requiredSources = [
  "apps/web/src/host/engine-clock-env.ts",
  "apps/web/src/host/facade-thread-binding.ts",
  "apps/web/src/host/facade-slot-protocol.ts",
  "apps/web/src/host/serde-normalize.ts",
  "packages/engine/src/runtime/mod.rs",
  "apps/web-wasm/src/lib.rs",
];
for (const relative of requiredSources) {
  assert(manifest.sourceFiles.some((item) => item.path === source(relative)), `必须冻结真实生产源码：${relative}`);
}
const { verifyWebReleaseWasm } = await import(pathToFileURL(source("scripts/check-web-release-wasm.mjs")).href);
await verifyWebReleaseWasm(path.dirname(manifest.wasm.path));

// 只把裸 env 指向同一真实生产单例；不复制shim、不伪造浏览器隔离或Worker身份。
const environmentUrl = pathToFileURL(source("apps/web/src/host/engine-clock-env.ts")).href;
const hooks = registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier === "env") return { url: environmentUrl, shortCircuit: true };
    return nextResolve(specifier, context);
  },
});
let glue;
let handle;
try {
  const environment = await import(environmentUrl);
  const { normalizeSerdeMaps } = await import(pathToFileURL(source("apps/web/src/host/serde-normalize.ts")).href);
  const memory = new WebAssembly.Memory({ initial: 1024, maximum: 32768, shared: true });
  environment.bindFacadeThreadEnvironment({
    memory,
    role: "engine",
    postControl() { throw new Error("C2C serial fixture不应发送facade控制消息"); },
  });
  glue = await import(pathToFileURL(manifest.glue.path).href);
  for (const name of ["initSync", "facade_slot_init", "facade_slot_layout", "restore_json", "step", "snapshot", "runtime_snapshot", "civil_date", "current_minute_history", "market_history", "drop_session"]) {
    assert.equal(typeof glue[name], "function", `最终真实glue缺少所需导出：${name}`);
  }
  const module = new WebAssembly.Module(await readFile(manifest.wasm.path));
  const raw = glue.initSync({ module, memory });
  assert.equal(raw.memory, memory, "禁止glue另建memory");
  assert.equal(typeof raw.__wbindgen_thread_destroy, "function");
  glue.facade_slot_init(manifest.fixtureCapacity);
  const layout = normalizeSerdeMaps(glue.facade_slot_layout());
  environment.installFacadeSlotLayout(layout);
  assert.equal(environment.getArenaView().words.buffer, memory.buffer);
  assert.equal(layout.capacity, manifest.fixtureCapacity);

  const fixtureJson = await readFile(manifest.fixture.path, "utf8");
  const slot = JSON.parse(fixtureJson);
  assert.equal(slot.setup.start_date, "2030-01-01");
  assert.equal(slot.setup.stocks.length, 2);
  assert.deepEqual([slot.setup.npcs.retail_count, slot.setup.npcs.inst_count, slot.setup.npcs.hot_count], [0, 0, 0]);
  assert.equal(slot.civil_clock.current_date, "2030-01-02");
  assert.equal(slot.civil_clock.settled_through, "2030-01-01");
  assert.equal(slot.snapshot.tick, 0);
  const native = JSON.parse(await readFile(manifest.nativeTrace.path, "utf8"));
  assert.equal(native.closed, 1);
  assert.equal(native.open, 0);
  assert.equal(native.initial.snapshot.day, 0, "SaveSnapshot不持久化day，live day必须由恢复重建");
  assert.equal(native.frames.length, 6);
  handle = glue.restore_json(fixtureJson);
  assert(Number.isSafeInteger(handle) && handle > 0);
  const observe = () => {
    const civilDate = glue.civil_date(handle);
    const currentMinute = {};
    const retained = {};
    for (const stock of [native.closed, native.open]) {
      currentMinute[stock] = normalizeSerdeMaps(glue.current_minute_history(handle, { stock }));
    }
    if (civilDate > "2030-01-02") {
      for (const stock of [native.closed, native.open]) {
        retained[stock] = normalizeSerdeMaps(glue.market_history(handle, {
          stock, date_from: "2030-01-02", date_to: "2030-01-02", after: null, page_size: 1,
        }));
      }
    }
    return {
      civilDate,
      snapshot: normalizeSerdeMaps(glue.snapshot(handle)),
      runtimeSnapshot: normalizeSerdeMaps(glue.runtime_snapshot(handle)),
      currentMinute,
      retained,
    };
  };
  const trace = { closed: native.closed, open: native.open, initial: observe(), frames: [] };
  for (let index = 0; index < 6; index += 1) {
    trace.frames.push({ update: normalizeSerdeMaps(glue.step(handle)), view: observe() });
  }
  // 先保留真实输出，再执行oracle与逐字段对照；断言失败也不丢掉运行事实。
  await writeFile(outputFile, JSON.stringify({ manifest, layout, trace }, null, 2) + "\n", { flag: "wx" });
  const first = trace.frames[0].view;
  assert.equal(first.currentMinute[native.closed].trading, false);
  assert.equal(first.currentMinute[native.closed].phase, "Closed");
  assert(!Object.hasOwn(first.snapshot.active_daily_candles, native.closed));
  assert.equal(trace.frames[1].view.snapshot.daily_candles[native.closed].length, 360);
  assert.equal(trace.frames[1].view.snapshot.daily_candles[native.open].length, 361);
  assert.equal(trace.frames[4].view.snapshot.daily_candles[native.closed].length, 361);
  assert.equal(trace.frames[4].view.snapshot.daily_candles[native.open].length, 362);
  assert.equal(trace.frames[2].view.retained[native.closed].entries[0].availability, "Closed");
  assert.equal(trace.frames[2].view.retained[native.open].entries[0].availability, "NoTrades");
  assert.deepEqual(trace, native, "native与实际wasm32串行的协议、市场、账户、分钟与retained须逐字段一致");
  await verifyArtifacts();
  await verifySources();
  const hashes = artifacts.map((artifact) => ({ path: artifact.path, sha256: artifact.sha256 }));
  console.log(JSON.stringify({ matched: true, sourcesUnchanged: true, sourceHead: manifest.sourceHead, frames: 6, inputSha256: manifest.fixture.sha256, bound: hashes, outputSha256: createHash("sha256").update(await readFile(outputFile)).digest("hex") }));
} finally {
  if (handle !== undefined) glue.drop_session(handle);
  hooks.deregister();
}
