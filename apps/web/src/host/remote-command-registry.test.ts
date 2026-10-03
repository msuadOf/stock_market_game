import assert from "node:assert/strict";
import test from "node:test";
import { RemoteCommandRegistry } from "./remote-command-registry.ts";

const options = { concurrency: true, timeout: 10000 };

test("RemoteCommandRegistry 按唯一 request ID 关联逆序确认，settle 前删除登记", options, () => {
  const registry = new RemoteCommandRegistry();
  const first = registry.next();
  const second = registry.next();
  const completed: number[] = [];
  assert.equal(second, first + 1);
  registry.register(first, { resolve() {
    completed.push(first);
    assert.throws(() => registry.resolveQueued(first), /未知写请求.*入队确认/);
  }, reject() { assert.fail("不应拒绝 queued command"); } });
  registry.register(second, { resolve() { completed.push(second); }, reject() { assert.fail("不应拒绝 queued command"); } });
  registry.resolveQueued(second);
  assert.deepEqual(completed, [second]);
  registry.resolveQueued(first);
  assert.deepEqual(completed, [second, first]);
});

test("RemoteCommandRegistry request 错误单独拒绝，fail 全部拒绝并清登记且保留 sequence", options, () => {
  const registry = new RemoteCommandRegistry();
  const first = registry.next();
  const second = registry.next();
  const rejected: string[] = [];
  registry.register(first, { resolve() { assert.fail("不应完成 rejected command"); }, reject(error) {
    rejected.push(error.message);
    assert.throws(() => registry.rejectGateway(first, { code: "E", where: "remote.command.gateway", message: "再次拒绝" }), /未知写请求.*网关错误/);
  } });
  registry.register(second, { resolve() { assert.fail("不应完成 failed command"); }, reject(error) { rejected.push(error.message); } });
  registry.rejectGateway(first, { code: "ONE", where: "remote.command.gateway", message: "单次错误" });
  assert.deepEqual(rejected, ["ONE: 单次错误"]);
  registry.rejectAll({ code: "ALL", where: "remote.publisher", message: "批次错误" });
  assert.deepEqual(rejected, ["ONE: 单次错误", "ALL: 批次错误"]);
  assert.throws(() => registry.resolveQueued(second), /未知写请求/);
  assert.equal(registry.next(), second + 1);
});
