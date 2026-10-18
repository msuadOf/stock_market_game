import assert from "node:assert/strict";
import test from "node:test";
import { WorkerTaskScheduler } from "./worker-task-scheduler.ts";

test("WorkerTaskScheduler 即时 task 不使用嵌套 timer，保留异步执行机会", { timeout: 10000 }, async () => {
  let timerCalls = 0;
  const scheduler = new WorkerTaskScheduler({
    schedule: (callback, delay) => { timerCalls++; return setTimeout(callback, delay); },
    cancel: timer => clearTimeout(timer),
  });
  try {
    let executed = false;
    const completed = new Promise<void>(resolve => {
      scheduler.schedule(() => { executed = true; resolve(); }, 0);
    });
    assert.equal(executed, false, "yield 必须产生独立 task");
    await completed;
    assert.equal(timerCalls, 0, "即时任务不能受到嵌套 timer 的 clamp");
  } finally { scheduler.dispose(); }
});

test("WorkerTaskScheduler 可以取消已投递即时 task，dispose 后全部取消", { timeout: 10000 }, async () => {
  const scheduler = new WorkerTaskScheduler();
  let executed = 0;
  const cancelled = scheduler.schedule(() => executed++, 0);
  scheduler.cancel(cancelled);
  await new Promise<void>(resolve => scheduler.schedule(resolve, 0));
  assert.equal(executed, 0);
  scheduler.schedule(() => executed++, 0);
  scheduler.dispose();
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(executed, 0);
  assert.throws(() => scheduler.schedule(() => executed++, 0), /已关闭/);
});

test("WorkerTaskScheduler 有限 delay 保留 timer，cancel 移除回调", { timeout: 10000 }, async () => {
  const scheduler = new WorkerTaskScheduler();
  try {
    let executed = false;
    const timer = scheduler.schedule(() => { executed = true; }, 5);
    scheduler.cancel(timer);
    await new Promise(resolve => setTimeout(resolve, 20));
    assert.equal(executed, false);
  } finally { scheduler.dispose(); }
});
