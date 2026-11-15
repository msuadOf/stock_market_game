import assert from "node:assert/strict";
import { it } from "node:test";
import {
  browserWorkerEntry,
  isWebSocketOpen,
  openWebSocket,
  spawnModuleWorker,
  transferMessageChannel,
  isMessagePort,
  ownMessagePort,
} from "../packages/libos/browser/browser-transport.ts";

function replaceGlobal(t, name, value) {
  const before = Object.getOwnPropertyDescriptor(globalThis, name);
  Object.defineProperty(globalThis, name, { configurable: true, value, writable: true });
  t.after(() => {
    if (before === undefined) Reflect.deleteProperty(globalThis, name);
    else Object.defineProperty(globalThis, name, before);
  });
}

it("Worker 工厂保留打包 URL 和调用时选定的构造器", { timeout: 10000 }, (t) => {
  const calls = [];
  replaceGlobal(t, "Worker", class {
    constructor(url, options) { calls.push([url.href, options]); }
  });
  const source = "./example.ts";
  const base = "https://example.test/app/bundle.js";
  spawnModuleWorker(browserWorkerEntry(source, base, { [source]: "/assets/example-a1.js" }), "observer");
  assert.deepEqual(calls, [["https://example.test/assets/example-a1.js", { type: "module", name: "observer" }]]);
  assert.equal(browserWorkerEntry(source, base, undefined).href, "https://example.test/app/example.ts");
  assert.throws(() => browserWorkerEntry(source, base, {}), /Worker 入口/);
});

it("WebSocket 工厂保留子协议列表并通过标准 readyState 判定可发送", { timeout: 10000 }, (t) => {
  const calls = [];
  replaceGlobal(t, "WebSocket", class {
    readyState = 0;
    constructor(url, protocols) { calls.push([url, protocols]); }
  });
  const socket = openWebSocket("wss://example.test/updates", ["example"]);
  assert.deepEqual(calls, [["wss://example.test/updates", ["example"]]]);
  assert.equal(isWebSocketOpen(socket), false);
  socket.readyState = 1;
  assert.equal(isWebSocketOpen(socket), true);
  assert.equal(isWebSocketOpen(null), false);
});

it("持有者关闭时释放真正 MessagePort，重复关闭安全", { timeout: 10000 }, () => {
  const channel = transferMessageChannel();
  assert.equal(isMessagePort(channel.port1), true);
  assert.equal(isMessagePort({ postMessage() {} }), false);
  const held = ownMessagePort(channel.port1);
  try {
    held.start();
    held.close();
    held.close();
    assert.throws(() => held.postMessage("不允许再发"), /port 已关闭/);
  } finally {
    held.close();
    channel.port2.close();
  }
});

it("宿主只选 Worker 入口和 transport policy，不直接构造浏览器原语", { timeout: 10000 }, async () => {
  const { readFile } = await import("node:fs/promises");
  const hosts = ["engine-control-client", "engine-control-worker", "wasm-publisher-worker", "remote-host"];
  await Promise.all(hosts.map(async (host) => {
    const filename = new URL(`../apps/web/src/host/${host}.ts`, import.meta.url);
    const text = await readFile(filename, "utf8");
    assert.match(text, /libos\/browser\/browser-transport\.ts/, host);
    assert.doesNotMatch(text, /\bnew\s+(?:Worker|MessageChannel|WebSocket)\s*\(|\b(?:setTimeout|clearTimeout|setInterval|clearInterval)\s*\(/, host);
  }));
});
