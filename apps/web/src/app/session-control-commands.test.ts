import assert from "node:assert/strict";
import { test } from "node:test";
import { SessionControlCommands } from "./session-control-commands.ts";
import { commandHostFixture } from "./command-host-test-fixture.ts";

test("G40：暂停和倍速收到宿主确认后才更新UI，并串行处理控制", { timeout: 10000 }, async () => {
  let confirm!: () => void;
  const calls: string[] = [];
  const host = commandHostFixture({ stop: async () => { calls.push("stop"); await new Promise<void>((resolve) => { confirm = resolve; }); }, setSpeed: async () => { calls.push("speed"); } });
  const runningRef = { current: true };
  const owner = new SessionControlCommands({ hostRef: { current: host }, runningRef,
    onUpdate() {}, onFatal() {}, onRunning(value) { runningRef.current = value; calls.push(`running:${value}`); },
    onSpeed(value) { calls.push(`speed:${value}`); }, onError(message) { calls.push(message); },
  });
  const paused = owner.toggleRunning(); const sped = owner.setSpeed(6);
  await Promise.resolve(); assert.deepEqual(calls, ["stop"]);
  confirm(); await paused; await sped;
  assert.deepEqual(calls, ["stop", "running:false", "speed", "speed:6"]);
});

test("G40：异步拒绝显错不确认UI，旧宿主晚到成功不能写新局", { timeout: 10000 }, async () => {
  const calls: string[] = [];
  const host = commandHostFixture({ start: async () => { throw new Error("IPC 拒绝"); }, setSpeed: async () => {} });
  const hostRef = { current: host };
  const owner = new SessionControlCommands({ hostRef, runningRef: { current: false }, onUpdate() {}, onFatal() {},
    onRunning(value) { calls.push(`running:${value}`); }, onSpeed(value) { calls.push(`speed:${value}`); }, onError(message) { calls.push(message); },
  });
  await owner.toggleRunning(); assert.match(calls[0]!, /IPC 拒绝/); assert.equal(calls.length, 1);
  let confirm!: () => void;
  host.setSpeed = () => new Promise<void>((resolve) => { confirm = resolve; });
  const sped = owner.setSpeed(30); await Promise.resolve();
  hostRef.current = commandHostFixture(); confirm(); await sped;
  assert.equal(calls.length, 1);
});
