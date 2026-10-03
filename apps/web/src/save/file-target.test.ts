import assert from "node:assert/strict";
import { after, before, test, type TestContext } from "node:test";
import { createServer, type ViteDevServer } from "vite";
import { currentSaveFixture } from "./current-save-fixture.ts";

let vite: ViteDevServer;
let files: typeof import("./save-file.ts");
const initialWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
const originalWindows = new WeakMap<TestContext, PropertyDescriptor | undefined>();

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  files = await vite.ssrLoadModule("/src/save/save-file.ts") as typeof files;
});

after(async () => {
  if (vite) await vite.close();
  assert.deepEqual(Object.getOwnPropertyDescriptor(globalThis, "window"), initialWindow);
});

function installWindowMock(context: TestContext, value: object): void {
  if (!originalWindows.has(context)) {
    const original = Object.getOwnPropertyDescriptor(globalThis, "window");
    originalWindows.set(context, original);
    context.after(() => {
      if (original === undefined) Reflect.deleteProperty(globalThis, "window");
      else Object.defineProperty(globalThis, "window", original);
      originalWindows.delete(context);
    });
  }
  Object.defineProperty(globalThis, "window", { configurable: true, value });
}

async function selectTarget() {
  assert.equal(typeof files.selectDayEndFileTarget, "function", "必须提供只选择、不保存的日终目标接口");
  return files.selectDayEndFileTarget();
}

function browserTarget(context: TestContext, hooks: {
  afterCreate?: () => void;
  afterWrite?: () => void;
  createError?: unknown;
  writeError?: unknown;
  closeError?: unknown;
  abortError?: unknown;
} = {}) {
  const calls: string[] = [];
  let stored = "previous valid save";
  const handle = {
    async createWritable() {
      calls.push("createWritable");
      if (hooks.createError !== undefined) throw hooks.createError;
      hooks.afterCreate?.();
      let pending = "";
      return {
        async write(text: string) {
          calls.push("write");
          pending = text;
          if (hooks.writeError !== undefined) throw hooks.writeError;
          hooks.afterWrite?.();
        },
        async close() {
          calls.push("close");
          if (hooks.closeError !== undefined) throw hooks.closeError;
          stored = pending;
        },
        async abort() {
          calls.push("abort");
          if (hooks.abortError !== undefined) throw hooks.abortError;
        },
      };
    },
    async getFile() {
      calls.push("getFile");
      return { text: async () => JSON.stringify(currentSaveFixture()) };
    },
  };
  installWindowMock(context, {
    showSaveFilePicker: async () => { calls.push("picker"); return handle; },
  });
  return { calls, handle, stored: () => stored };
}

test("旧 saveToFile 出口拒绝日内保存，不请求权限、不下载或写当前状态", async (context) => {
  const browser = browserTarget(context);
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      showSaveFilePicker: async () => { browser.calls.push("picker"); return browser.handle; },
      showOpenFilePicker: async () => [],
    },
  });
  await assert.rejects(files.saveToFile(currentSaveFixture()), /日内.*禁止|禁止.*日内/);
  assert.deepEqual(browser.calls, []);
});

test("仅需保存选择器即可选择授权目标，后续两次日结复用同一 handle", async (context) => {
  const browser = browserTarget(context);
  const target = await selectTarget();
  assert.ok(target);
  assert.deepEqual(browser.calls, ["picker"]);
  const first = currentSaveFixture();
  await target.write(first);
  assert.deepEqual(browser.calls, ["picker", "createWritable", "write", "close"]);
  assert.equal(JSON.parse(browser.stored()).seed, "42");
  const next = currentSaveFixture();
  next.seed = "43";
  await target.write(next);
  assert.equal(JSON.parse(browser.stored()).seed, "43");
  assert.deepEqual(browser.calls, ["picker", "createWritable", "write", "close", "createWritable", "write", "close"]);
});

test("取消选择返回 null，授权失败可见，不支持覆盖时不伪装成下载", async (context) => {
  installWindowMock(context, { showSaveFilePicker: async () => { throw new DOMException("cancelled", "AbortError"); } });
  assert.equal(await selectTarget(), null);
  const permissionError = new DOMException("permission denied", "NotAllowedError");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { showSaveFilePicker: async () => { throw permissionError; } },
  });
  await assert.rejects(selectTarget(), (error: unknown) => error instanceof Error && error.cause === permissionError && /选择.*失败/.test(error.message));
  Object.defineProperty(globalThis, "window", { configurable: true, value: {} });
  await assert.rejects(selectTarget(), /不支持.*覆盖/);
});

test("非法候选在创建 writable 前拒绝，不掩盖存档校验失败", async (context) => {
  const browser = browserTarget(context);
  const target = await selectTarget();
  assert.ok(target);
  await assert.rejects(target.write({ schema_version: 3 }), /存档|根节点/);
  assert.deepEqual(browser.calls, ["picker"]);
});

test("序列化在第一个 await 前冻结输入，不读取后来已变动的盘中状态", async (context) => {
  const candidate = currentSaveFixture();
  const browser = browserTarget(context, { afterCreate: () => { candidate.seed = "99"; } });
  const target = await selectTarget();
  assert.ok(target);
  await target.write(candidate);
  assert.equal(JSON.parse(browser.stored()).seed, "42");
  assert.equal(candidate.seed, "99");
});

test("write 失败 abort 临时写入，绝不 finally close 提交损坏候选", async (context) => {
  const writeError = new Error("disk full");
  const browser = browserTarget(context, { writeError });
  const target = await selectTarget();
  assert.ok(target);
  await assert.rejects(target.write(currentSaveFixture()), (error: unknown) => error instanceof Error && error.cause === writeError && /日终.*失败.*disk full/.test(error.message));
  assert.deepEqual(browser.calls, ["picker", "createWritable", "write", "abort"]);
  assert.equal(browser.stored(), "previous valid save");
});

test("write 与 abort 都失败时保留两个真实错误及主 cause", async (context) => {
  const writeError = new Error("disk full");
  const abortError = new Error("abort denied");
  const browser = browserTarget(context, { writeError, abortError });
  const target = await selectTarget();
  assert.ok(target);
  await assert.rejects(target.write(currentSaveFixture()), (error: unknown) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.cause, writeError);
    assert.deepEqual(error.errors, [writeError, abortError]);
    assert.match(error.message, /disk full/);
    assert.match(error.message, /abort denied/);
    return true;
  });
  assert.equal(browser.stored(), "previous valid save");
  assert.ok(!browser.calls.includes("close"));
});

test("close 失败也尝试 abort；创建 writable 的 AbortError 不能被当成取消选择", async (context) => {
  const closeError = new Error("commit failed");
  const hooks: { closeError?: unknown; createError?: unknown } = { closeError };
  const browser = browserTarget(context, hooks);
  const target = await selectTarget();
  assert.ok(target);
  await assert.rejects(target.write(currentSaveFixture()), (error: unknown) => error instanceof Error && error.cause === closeError);
  assert.deepEqual(browser.calls, ["picker", "createWritable", "write", "close", "abort"]);
  assert.equal(browser.stored(), "previous valid save");
  const createError = new DOMException("write permission lost", "AbortError");
  hooks.createError = createError;
  await assert.rejects(target.write(currentSaveFixture()), (error: unknown) => error instanceof Error && error.cause === createError);
});

test("旧 generation 在创建前、创建后和 write await 后均拒绝；close 不得提交旧写", async (context) => {
  for (const stage of ["before", "create", "write"]) {
    let current = stage !== "before";
    const browser = browserTarget(context, {
      afterCreate: () => { if (stage === "create") current = false; },
      afterWrite: () => { if (stage === "write") current = false; },
    });
    const target = await selectTarget();
    assert.ok(target);
    await assert.rejects(target.write(currentSaveFixture(), () => current), /旧局|generation/);
    assert.ok(!browser.calls.includes("close"));
    assert.equal(browser.stored(), "previous valid save");
    assert.deepEqual(browser.calls, stage === "before" ? ["picker"] : stage === "create"
      ? ["picker", "createWritable", "abort"] : ["picker", "createWritable", "write", "abort"]);
  }
});

test("loadFromFile 保留读档行为，读后不创建 writable 或写回文件", async (context) => {
  const browser = browserTarget(context);
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { showSaveFilePicker: async () => browser.handle, showOpenFilePicker: async () => [browser.handle] },
  });
  const loaded = await files.loadFromFile();
  assert.ok(loaded);
  assert.equal(loaded.seed, "42");
  assert.deepEqual(browser.calls, ["getFile"]);
});

function tauriTarget(context: TestContext, hooks: {
  failAt?: string;
  cleanupError?: unknown;
  afterStage?: (stage: string) => void;
  selection?: unknown;
} = {}) {
  const calls: string[] = [];
  const paths = new Map<string, string>([["/virtual/save.json", "previous valid save"]]);
  const nativeError = new Error("native operation failed");
  installWindowMock(context, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown>, options?: { headers: Record<string, string> }) => {
        calls.push(command);
        if (command === "plugin:dialog|save") return Object.hasOwn(hooks, "selection") ? hooks.selection : "/virtual/save.json";
        if (command === "plugin:fs|open") {
          assert.notEqual(args.path, "/virtual/save.json");
          assert.match(String(args.path), /^\/virtual\/stock-game-day-end-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.tmp$/);
          assert.deepEqual(args.options, { write: true, createNew: true });
          if (hooks.failAt === command) {
            paths.set(String(args.path), "existing temporary file owned by another operation");
            throw nativeError;
          }
          paths.set(String(args.path), "");
          hooks.afterStage?.(command);
          return 7;
        }
        if (command === "plugin:fs|write_text_file") {
          assert.ok(options);
          const path = decodeURIComponent(options.headers.path!);
          assert.notEqual(path, "/virtual/save.json");
          assert.equal(JSON.parse(options.headers.options!).create, false);
          paths.set(path, new TextDecoder().decode(args as unknown as Uint8Array));
        }
        if (command === "plugin:fs|remove") {
          assert.notEqual(args.path, "/virtual/save.json");
          if (hooks.cleanupError !== undefined) throw hooks.cleanupError;
          paths.delete(String(args.path));
          return;
        }
        if (hooks.failAt === command) throw nativeError;
        if (command === "plugin:fs|rename") {
          assert.equal(args.newPath, "/virtual/save.json");
          assert.ok(paths.has(String(args.oldPath)));
          paths.set(String(args.newPath), paths.get(String(args.oldPath))!);
          paths.delete(String(args.oldPath));
        } else if (command !== "plugin:resources|close" && command !== "plugin:fs|write_text_file") {
          throw new Error(`unexpected IPC ${command}`);
        }
        hooks.afterStage?.(command);
      },
    },
  });
  return { calls, paths, nativeError };
}

test("Tauri 选择仅复用路径，两次写入都先生成独占同目录临时文件再 rename", async (context) => {
  const native = tauriTarget(context);
  const target = await selectTarget();
  assert.ok(target);
  assert.deepEqual(native.calls, ["plugin:dialog|save"]);
  await target.write(currentSaveFixture());
  const next = currentSaveFixture();
  next.seed = "43";
  await target.write(next);
  assert.equal(JSON.parse(native.paths.get("/virtual/save.json")!).seed, "43");
  assert.deepEqual([...native.paths.keys()], ["/virtual/save.json"]);
  assert.deepEqual(native.calls, ["plugin:dialog|save", ...Array(2).fill([
    "plugin:fs|open", "plugin:resources|close", "plugin:fs|write_text_file", "plugin:fs|rename",
  ]).flat()]);
});

test("Tauri 取消不写；非法选择路径明确拒绝", async (context) => {
  const hooks = { selection: null as unknown };
  const native = tauriTarget(context, hooks);
  assert.equal(await selectTarget(), null);
  assert.deepEqual(native.calls, ["plugin:dialog|save"]);
  hooks.selection = [];
  await assert.rejects(selectTarget(), /路径/);
});

test("Tauri 临时写入或 rename 失败均保旧并清理；创建冲突绝不删他人临时文件", async (context) => {
  for (const stage of ["plugin:fs|open", "plugin:fs|write_text_file", "plugin:fs|rename"]) {
    const native = tauriTarget(context, { failAt: stage });
    const target = await selectTarget();
    assert.ok(target);
    await assert.rejects(target.write(currentSaveFixture()), (error: unknown) => error instanceof Error && error.cause === native.nativeError);
    assert.equal(native.paths.get("/virtual/save.json"), "previous valid save");
    if (stage === "plugin:fs|open") {
      assert.ok(!native.calls.includes("plugin:fs|remove"));
      assert.ok([...native.paths.values()].includes("existing temporary file owned by another operation"));
    } else {
      assert.deepEqual([...native.paths.keys()], ["/virtual/save.json"]);
      assert.equal(native.calls.at(-1), "plugin:fs|remove");
    }
  }
});

test("Tauri generation 在任意准备 await 后变旧时清理临时文件，绝不 rename", async (context) => {
  for (const stage of ["before", "plugin:fs|open", "plugin:resources|close", "plugin:fs|write_text_file"]) {
    let current = stage !== "before";
    const native = tauriTarget(context, { afterStage: (finished) => { if (finished === stage) current = false; } });
    const target = await selectTarget();
    assert.ok(target);
    await assert.rejects(target.write(currentSaveFixture(), () => current), /旧局|generation/);
    assert.equal(native.paths.get("/virtual/save.json"), "previous valid save");
    assert.ok(!native.calls.includes("plugin:fs|rename"));
    assert.deepEqual([...native.paths.keys()], ["/virtual/save.json"]);
  }
});

test("Tauri 写失败且清理失败时两个错误显式可见，目标旧档仍保留", async (context) => {
  const cleanupError = new Error("cleanup permission denied");
  const native = tauriTarget(context, { failAt: "plugin:fs|write_text_file", cleanupError });
  const target = await selectTarget();
  assert.ok(target);
  await assert.rejects(target.write(currentSaveFixture()), (error: unknown) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.cause, native.nativeError);
    assert.deepEqual(error.errors, [native.nativeError, cleanupError]);
    assert.match(error.message, /native operation failed/);
    assert.match(error.message, /cleanup permission denied/);
    return true;
  });
  assert.equal(native.paths.get("/virtual/save.json"), "previous valid save");
});
