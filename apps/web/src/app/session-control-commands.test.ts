import assert from "node:assert/strict";
import { test } from "node:test";
import { SessionControlCommands } from "./session-control-commands.ts";
import { commandHostFixture } from "./command-host-test-fixture.ts";
import { remoteTestContext } from "../host/remote-test-context.ts";

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

test("共享市场同host换generation后旧控制成功和失败均不能修改新timeline UI", { timeout: 10000 }, async () => {
  for (const rejected of [false, true]) {
    let generation = "1";
    let settle!: () => void;
    const changes: unknown[] = [];
    const host = commandHostFixture({ marketContext: () => ({ ...remoteTestContext(), generation }),
      setSpeed: () => new Promise<void>((resolve, reject) => { settle = () => rejected ? reject(new Error("旧generation拒绝")) : resolve(); }),
    });
    const owner = new SessionControlCommands({ hostRef: { current: host }, runningRef: { current: false }, onUpdate() {}, onFatal() {}, onRunning(value) { changes.push(value); }, onSpeed(value) { changes.push(value); }, onError(value) { changes.push(value); } });
    const pending = owner.setSpeed(30);
    await Promise.resolve();
    generation = "2";
    settle();
    await pending;
    assert.deepEqual(changes, []);
  }
});
