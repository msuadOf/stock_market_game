import assert from "node:assert/strict";
import test from "node:test";
import { InitializationDeadline } from "./initialization-deadline.ts";

test("初始化进度只能递进，阶段与总期限不重置且关闭后不可重启", { timeout: 10000 }, () => {
  const tasks = new Map<number, { callback: () => void; ms: number }>(); let next = 0; const failures: string[] = []; const progress: string[] = [];
  let now = 0;
  const deadline = new InitializationDeadline((phase) => failures.push(phase), (phase) => progress.push(phase), {
    now: () => now, set: (callback, ms) => { tasks.set(++next, { callback, ms }); return next; }, clear: (id) => { tasks.delete(id); },
  });
  assert.equal(tasks.get(1)?.ms, 120_000);
  deadline.advance("package"); assert.equal(tasks.get(2)?.ms, 30_000);
  deadline.advance("engine"); assert.equal(tasks.get(1)?.ms, 120_000); assert.equal(tasks.has(2), false); assert.equal(tasks.get(3)?.ms, 30_000);
  assert.throws(() => deadline.advance("engine"), /顺序/);
  assert.throws(() => deadline.advance("package"), /顺序/);
  assert.deepEqual(progress, ["正在校验行情引擎", "正在初始化行情引擎"]);
  tasks.get(3)!.callback(); assert.deepEqual(failures, ["正在初始化行情引擎"]);
  deadline.dispose(); assert.equal(tasks.size, 0);
  assert.throws(() => deadline.advance("restore"), /关闭/);
});

test("进度迟于绝对阶段期限时不能因 timer 尚未调度而续期", { timeout: 10000 }, () => {
  let now = 0; const tasks = new Map<number, () => void>(); let id = 0; const failures: string[] = [];
  const d = new InitializationDeadline((phase) => failures.push(phase), () => {}, { now: () => now, set: (fn) => { tasks.set(++id, fn); return id; }, clear: (key) => { tasks.delete(key); } });
  d.advance("package"); now = 30_001;
  assert.throws(() => d.advance("engine"), /超时/);
  assert.deepEqual(failures, ["正在校验行情引擎"]); d.dispose();
});

test("完成迟于阶段期限时不能清除尚未调度的超时", { timeout: 10000 }, () => {
  let now = 0; let id = 0; const failures: string[] = [];
  const d = new InitializationDeadline((phase) => failures.push(phase), () => {}, { now: () => now, set: () => ++id, clear: () => {} });
  d.advance("package"); d.advance("engine"); d.advance("restore"); now = 30_001;
  assert.equal(d.complete(), false);
  assert.deepEqual(failures, ["正在建立市场会话"]); d.dispose();
});
