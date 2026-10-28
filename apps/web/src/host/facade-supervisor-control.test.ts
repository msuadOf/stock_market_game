import assert from "node:assert/strict";
import test from "node:test";
import { parseFacadeSupervisorControl } from "./facade-supervisor-control.ts";

test("facade 分流保留新请求身份并放行业务消息", { timeout: 10000 }, () => {
  const input = { type: "facadeSpawn", slot: 3, gen: 5, name: "nested" };
  assert.deepEqual(parseFacadeSupervisorControl(input), input);
  assert.equal(parseFacadeSupervisorControl({ type: "created" }), null);
  assert.equal(parseFacadeSupervisorControl(null), null);
});

test("当前 facade 消息拒绝畸形数值、名称、未知类型及多余字段", { timeout: 10000 }, () => {
  for (const input of [
    { type: "facadeSpawn", slot: -1, gen: 0, name: "x" },
    { type: "facadeSpawn", slot: 0, gen: 0.5, name: "x" },
    { type: "facadeSpawn", slot: 0, gen: 0, name: 2 },
    { type: "facadeTerminateChild", workerId: 2 ** 32 },
    { type: "facadeUnknown" },
    { type: "facadeWorkerExit", workerId: 1, slot: 0, gen: 0, extra: true },
  ]) assert.throws(() => parseFacadeSupervisorControl(input), /facade/);
});

test("bootstrap failure 只有 validate 无身份，其他 phase 身份必填且保留 HostFailure", { timeout: 10000 }, () => {
  const failure = { code: "CLEANUP", message: "destroy failed", where: "fixture.cleanup" };
  const message = { type: "facadeBootstrapFailure", phase: "cleanup", workerId: 1, slot: 0, gen: 2, failure };
  assert.deepEqual(parseFacadeSupervisorControl(message), message);
  assert.deepEqual(parseFacadeSupervisorControl({ type: "facadeBootstrapFailure", phase: "validate", failure }), { type: "facadeBootstrapFailure", phase: "validate", failure });
  assert.throws(() => parseFacadeSupervisorControl({ type: "facadeBootstrapFailure", phase: "cleanup", failure }));
  assert.throws(() => parseFacadeSupervisorControl({ ...message, failure: { ...failure, recoverable: "yes" } }));
});
