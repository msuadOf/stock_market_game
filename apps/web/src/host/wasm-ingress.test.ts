import assert from "node:assert/strict";
import test from "node:test";
import { WasmIngressReceiver } from "./wasm-ingress.ts";

test("Browser ingress 不等待 engine worker 返回便把完整 Intent 交给共享 receiver", { timeout: 10000 }, () => {
  const received: unknown[] = [];
  const ingress = new WasmIngressReceiver((token, intent) => { received.push([token, intent]); });
  ingress.bind(1, 42);
  ingress.submit(1, { Cancel: { code: "600000", id: 7 } });
  assert.deepEqual(received, [[42, { Cancel: { code: "600000", id: 7 } }]]);
});

test("Browser ingress 拒绝未绑定、旧 generation 和销毁后请求", { timeout: 10000 }, () => {
  const received: unknown[] = [];
  const ingress = new WasmIngressReceiver((token, intent) => { received.push([token, intent]); });
  assert.throws(() => ingress.submit(1, {}), /尚未就绪/);
  ingress.bind(1, 42);
  ingress.bind(2, 43);
  assert.throws(() => ingress.submit(1, {}), /已过期/);
  assert.throws(() => ingress.bind(1, 42), /generation/);
  ingress.close();
  assert.throws(() => ingress.submit(2, {}), /已关闭/);
  assert.throws(() => ingress.bind(3, 44), /已关闭/);
  assert.deepEqual(received, []);
});

test("Browser ingress 显式传递 Rust receiver 错误，不报告虚假 enqueued", { timeout: 10000 }, () => {
  const ingress = new WasmIngressReceiver(() => { throw new Error("receiver 已关闭"); });
  ingress.bind(1, 42);
  assert.throws(() => ingress.submit(1, {}), /receiver 已关闭/);
});
