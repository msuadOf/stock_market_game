import assert from "node:assert/strict";
import { test } from "node:test";

test("真实接收入口的reportError缺失时用独立microtask抛出可见错误", async () => {
  const previousSelf = Object.getOwnPropertyDescriptor(globalThis, "self");
  const previousQueue = globalThis.queueMicrotask;
  const tasks: (() => void)[] = [];
  let listener!: (event: { data: unknown }) => void;
  let closed = 0;
  Object.defineProperty(globalThis, "self", { configurable: true, value: {
    addEventListener(_type: string, callback: typeof listener) { listener = callback; },
    postMessage() { throw new Error("全局投递失败"); }, close() { closed++; },
  } });
  globalThis.queueMicrotask = (callback) => { tasks.push(callback); };
  try {
    await import("./facade-child-worker.ts");
    listener({ data: null });
    assert.equal(tasks.length, 1);
    assert.throws(tasks[0], /控制\/全局消息均无法投递/);
    assert.equal(closed, 1);
  } finally {
    globalThis.queueMicrotask = previousQueue;
    if (previousSelf === undefined) Reflect.deleteProperty(globalThis, "self");
    else Object.defineProperty(globalThis, "self", previousSelf);
  }
});
