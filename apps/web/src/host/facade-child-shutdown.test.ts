import assert from "node:assert/strict";
import test from "node:test";

import { ChildShutdown, parseWorkerExit } from "./facade-child-shutdown.ts";

// §10.2.2 W1-W6 关闭全序（Worker 侧 W3-W5 与守卫）的确定性测。

function fakeScope(): { readonly posts: unknown[]; closed: boolean; scope: { postMessage(m: unknown): void; close(): void } } {
  const posts: unknown[] = [];
  const state = { posts, closed: false, scope: {
    postMessage(m: unknown) { posts.push(m); },
    close() { state.closed = true; },
  } };
  return state;
}

test("W3→W5 序：先 post workerExit，再置 rust_done，最后 close", () => {
  const s = fakeScope();
  const shutdown = new ChildShutdown(s.scope);
  assert.equal(shutdown.isRustDone(), false);
  shutdown.shutdown(7, 3, 1);
  assert.deepEqual(s.posts, [{ type: "facadeWorkerExit", workerId: 7, slot: 3, gen: 1 }]);
  // 三条后置条件同时为真（W4 标与 W5 close 在 W3 post 之后）。
  assert.equal(shutdown.isRustDone(), true);
  assert.equal(shutdown.isClosed(), true);
  assert.equal(s.closed, true);
});

test("close 后 onmessage 一律丢弃（W4 迟到消息守卫）", () => {
  const s = fakeScope();
  const shutdown = new ChildShutdown(s.scope);
  assert.equal(shutdown.shouldDropMessage(), false, "收尾前正常派发");
  shutdown.shutdown(1, 0, 0);
  assert.equal(shutdown.shouldDropMessage(), true, "rust_done 后丢弃，零 Rust 执行");
});

test("重复关闭显式失败（trampoline 单出口）", () => {
  const s = fakeScope();
  const shutdown = new ChildShutdown(s.scope);
  shutdown.shutdown(1, 0, 0);
  assert.throws(() => shutdown.shutdown(1, 0, 0), /重复关闭/);
  assert.equal(s.posts.length, 1, "第二次不得再 post");
});

test("workerExit 消息形状守卫拒绝畸形输入", () => {
  assert.deepEqual(parseWorkerExit({ type: "facadeWorkerExit", workerId: 1, slot: 2, gen: 3 }),
    { type: "facadeWorkerExit", workerId: 1, slot: 2, gen: 3 });
  for (const bad of [null, 42, {}, { type: "other", workerId: 1, slot: 2, gen: 3 },
    { type: "facadeWorkerExit", workerId: 1.5, slot: 2, gen: 3 },
    { type: "facadeWorkerExit", workerId: 1, slot: 2 }]) {
    assert.equal(parseWorkerExit(bad), null, `应拒绝：${JSON.stringify(bad)}`);
  }
});
