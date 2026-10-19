import assert from "node:assert/strict";
import test from "node:test";
import { logoutRemoteIdentity } from "./remote-logout.ts";

test("Server已撤销credential后本地存储失败仍断连接，明确报告清理失败", { timeout: 10000 }, async () => {
  const calls: string[] = [];
  await assert.rejects(logoutRemoteIdentity({ revoke: async () => { calls.push("revoke"); }, forget: async () => { calls.push("forget"); throw new Error("IndexedDB不可用"); }, disconnect: async () => { calls.push("disconnect"); } }), /已退出.*登录存储.*IndexedDB不可用/);
  assert.deepEqual(calls, ["revoke", "forget", "disconnect"]);
});

test("撤销失败不冒称退出，双重清理失败显式保留详情", { timeout: 10000 }, async () => {
  let disconnected = false;
  await assert.rejects(logoutRemoteIdentity({ revoke: async () => { throw new Error("401"); }, forget: async () => {}, disconnect: async () => { disconnected = true; } }), /401/);
  assert.equal(disconnected, false);
  await assert.rejects(logoutRemoteIdentity({ revoke: async () => {}, forget: async () => { throw new Error("存储失败"); }, disconnect: async () => { throw new Error("断开失败"); } }), (failure: unknown) => failure instanceof AggregateError && failure.errors.length === 2);
});
