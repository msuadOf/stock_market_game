import assert from "node:assert/strict";
import test from "node:test";
import { readWasmTombstone } from "./wasm-tombstone.ts";

test("只读取匹配 Worker 的完整记录", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const words = new Int32Array(memory.buffer, 256, 4);
  const bytes = new Uint8Array(memory.buffer, 256, 256);
  assert.deepEqual(readWasmTombstone(memory, 256, 7), { state: "absent" });
  Atomics.store(words, 0, 1);
  assert.deepEqual(readWasmTombstone(memory, 256, 7), { state: "writing" });
  Atomics.store(words, 1, 7);
  bytes[10] = 1;
  bytes[12] = 3;
  bytes.set([97, 98, 99], 16);
  Atomics.store(words, 0, 2);
  assert.deepEqual(readWasmTombstone(memory, 256, 7), { state: "live", kind: "panic", phase: 0, message: "abc", truncated: false });
  assert.throws(() => readWasmTombstone(memory, 256, 8), /worker/);
});

test("越界、破损和非法编码不能伪装为缺席", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  assert.throws(() => readWasmTombstone(memory, 65500, 7), /地址/);
  assert.throws(() => readWasmTombstone(memory, 257, 7), /地址/);
  const words = new Int32Array(memory.buffer, 256, 4);
  const bytes = new Uint8Array(memory.buffer, 256, 256);
  Atomics.store(words, 0, 2); Atomics.store(words, 1, 7);
  bytes[10] = 1; bytes[12] = 1; bytes[16] = 255;
  assert.deepEqual(readWasmTombstone(memory, 256, 7), { state: "invalid-utf8", kind: "panic", phase: 0, bytes: [255], truncated: false });
  bytes[10] = 9;
  assert.throws(() => readWasmTombstone(memory, 256, 7), /kind/);
});

test("截断发生在 UTF-8 字符中间时保留完整前缀", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const words = new Int32Array(memory.buffer, 256, 4);
  const halves = new Uint16Array(memory.buffer, 256, 8);
  const bytes = new Uint8Array(memory.buffer, 256, 256);
  Atomics.store(words, 1, 7); bytes[10] = 1; bytes[11] = 1; Atomics.store(halves, 6, 240);
  bytes.fill(97, 16, 255); bytes[255] = 0xe4;
  Atomics.store(words, 0, 2);
  assert.deepEqual(readWasmTombstone(memory, 256, 7), { state: "live", kind: "panic", phase: 0, message: "a".repeat(239), truncated: true });
});

test("有效 BOM 原样保留", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const words = new Int32Array(memory.buffer, 256, 4);
  const bytes = new Uint8Array(memory.buffer, 256, 256);
  Atomics.store(words, 1, 7); bytes[10] = 1; bytes[12] = 4;
  bytes.set([0xef, 0xbb, 0xbf, 0x61], 16); Atomics.store(words, 0, 2);
  assert.deepEqual(readWasmTombstone(memory, 256, 7), { state: "live", kind: "panic", phase: 0, message: "\ufeffa", truncated: false });
});
