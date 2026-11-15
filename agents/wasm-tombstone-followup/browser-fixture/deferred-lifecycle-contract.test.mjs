import assert from 'node:assert/strict';
import { test } from 'node:test';
import { assertDeferredLifecycleContract, deferredLifecycleModes as mode } from './deferred-lifecycle-contract.mjs';

const id = { workerId: 1, slot: 0, gen: 9 };
const ready = { type: 'ready' };
const spawned = { type: 'childSpawn', runtimeName: 'engine-deferred-drop', ...id };
const exited = { type: 'childExit', ...id };
const drained = { type: 'childrenDrained', activeChildren: 0, manifestEntries: 0, pendingRecovery: 0 };
const counters = { children: 0, protocolEntries: 0, pendingRecovery: 0 };
const timeout = { timeout: 10_000 };

function rejected(code) {
  return { result: 'rejected', code, sameValueReturned: true, dropsAtReturn: 0, dropsAfterCallerDrop: 1 };
}

function normal(which, witness) {
  return { events: structuredClone([ready, spawned, exited, drained, {
    type: 'verification-result', mode: which, ...counters, witness,
  }]) };
}

function saturated() {
  return normal(mode.saturation, {
    capacity: 2, gateEntered: true,
    submissions: ['queued', 'queued', 'full-inline'],
    completedWhileGated: [{ id: 2, by: 'caller' }],
    degradationAfterOverflow: null,
    close: 'drained-and-joined',
    completedAtClose: [{ id: 2, by: 'caller' }, { id: 0, by: 'consumer' }, { id: 1, by: 'consumer' }],
  });
}

function retired() {
  return normal(mode.close, {
    submitBeforeStart: rejected('not-started'), closeBeforeStart: 'not-started',
    start: 'started', duplicateStart: 'already-started', submissions: ['queued', 'queued'],
    completedAtCloseEntry: [], order: ['close-entered', 'gate-released', 'consumer-completed', 'close-returned'],
    completedAtClose: [{ id: 0, by: 'consumer' }, { id: 1, by: 'consumer' }],
    close: 'drained-and-joined', duplicateClose: 'already-closed', submitAfterClose: rejected('closed'),
  });
}

function consumerPanic() {
  return {
    events: structuredClone([ready, spawned, {
      type: 'deferred-fault-armed', witness: { submission: 'queued', workerEntered: true, releasedByCaller: false },
    }, { type: 'deferred-fault-trigger' }, { type: 'failure-pending' }]),
    failure: { context: { incomplete: true, records: [{ identity: { ...id }, tombstone: {
      state: 'live', kind: 'panic', message: 'LIBOS_RECLAIMER_PANIC_PROBE',
    } }] } },
  };
}

function spawnFailure() {
  return { events: structuredClone([ready, { spawnFailure: { code: 'WORKER_CONSTRUCT' } }, {
    type: 'verification-result', mode: mode.spawnFailure, ...counters,
    witness: { start: 'spawn-failed', close: 'not-started', submitAfterFailedStart: rejected('start-failed') },
  }]) };
}

test('真实饱和样例接受；区分 FullInline、后端故障与无界排队', timeout, () => {
  assert.doesNotThrow(() => assertDeferredLifecycleContract(saturated(), mode.saturation));
  for (const invalid of ['disconnected-inline', 'queued', 'inline']) {
    const result = saturated();
    result.events.at(-1).witness.submissions[2] = invalid;
    assert.throws(() => assertDeferredLifecycleContract(result, mode.saturation));
  }
  const broken = saturated();
  broken.events.at(-1).witness.degradationAfterOverflow = 'unexpected-disconnected';
  assert.throws(() => assertDeferredLifecycleContract(broken, mode.saturation), /队列满不能冒充/);
});

test('满队列分支不能提前释放已入队任务、丢失或重排已接纳工作', timeout, () => {
  const early = saturated();
  early.events.at(-1).witness.completedWhileGated.push({ id: 0, by: 'consumer' });
  assert.throws(() => assertDeferredLifecycleContract(early, mode.saturation), /不得提前释放/);
  const reordered = saturated();
  reordered.events.at(-1).witness.completedAtClose[1].id = 1;
  assert.throws(() => assertDeferredLifecycleContract(reordered, mode.saturation), /恰好执行/);
});

test('关闭使用同一 owner，一次 drain；未启动和已关闭提交保留所有权', timeout, () => {
  assert.doesNotThrow(() => assertDeferredLifecycleContract(retired(), mode.close));
  for (const stage of ['submitBeforeStart', 'submitAfterClose']) {
    const inline = retired();
    inline.events.at(-1).witness[stage] = { result: 'inline', dropsAtReturn: 1 };
    assert.throws(() => assertDeferredLifecycleContract(inline, mode.close), /不能静默内联/);
  }
  const duplicateClose = retired();
  duplicateClose.events.at(-1).witness.duplicateClose = 'drained-and-joined';
  assert.throws(() => assertDeferredLifecycleContract(duplicateClose, mode.close));
  const duplicateStart = retired();
  duplicateStart.events.splice(2, 0, { ...spawned, gen: id.gen + 1 });
  assert.throws(() => assertDeferredLifecycleContract(duplicateStart, mode.close), /只能建立一个/);
});

test('关闭提前返回或仅有 join 结果但无同代退出/零资源证据都不能报告成功', timeout, () => {
  const early = retired();
  early.events.at(-1).witness.order = ['close-entered', 'gate-released', 'close-returned', 'consumer-completed'];
  assert.throws(() => assertDeferredLifecycleContract(early, mode.close), /不能在消费者完成前返回/);
  const wrongGeneration = retired();
  wrongGeneration.events[2].gen++;
  assert.throws(() => assertDeferredLifecycleContract(wrongGeneration, mode.close), /真实 Worker 退出/);
  const leak = retired();
  leak.events[3].pendingRecovery++;
  assert.throws(() => assertDeferredLifecycleContract(leak, mode.close), /零资源证据/);
});

test('消费者已成立后死亡：只接受真实同槽世代 panic 和未证明释放', timeout, () => {
  assert.doesNotThrow(() => assertDeferredLifecycleContract(consumerPanic(), mode.consumerPanic));
  const unrelated = consumerPanic();
  unrelated.failure.context.records[0].identity.gen++;
  assert.throws(() => assertDeferredLifecycleContract(unrelated, mode.consumerPanic), /保留一致槽世代/);
  const silent = consumerPanic();
  silent.failure.context.records[0].tombstone.message = '别的错误';
  assert.throws(() => assertDeferredLifecycleContract(silent, mode.consumerPanic));
  for (const falseSuccess of [exited, { type: 'verification-result' }, { type: 'finalRelease', kind: 'released' }]) {
    const afterDeath = consumerPanic();
    afterDeath.events.push(falseSuccess);
    assert.throws(() => assertDeferredLifecycleContract(afterDeath, mode.consumerPanic), /不得误报/);
  }
});

test('启动失败必须来自 supervisor 构造入口，不能假报 drain 或启动替代消费者', timeout, () => {
  assert.doesNotThrow(() => assertDeferredLifecycleContract(spawnFailure(), mode.spawnFailure));
  const fake = spawnFailure();
  fake.events.splice(1, 1);
  assert.throws(() => assertDeferredLifecycleContract(fake, mode.spawnFailure), /实际 Worker 创建入口/);
  const leaked = spawnFailure();
  leaked.events.splice(2, 0, spawned);
  assert.throws(() => assertDeferredLifecycleContract(leaked, mode.spawnFailure), /不能留下已登记消费者/);
});

test('测试专用导出缺失是未验证，既不冒充消费者死亡也不冒充启动拒绝', timeout, () => {
  for (const which of Object.values(mode)) {
    assert.throws(() => assertDeferredLifecycleContract({
      events: [ready, { type: 'verification-error', message: '缺少测试专用导出' }],
    }, which), /不能充当合同证据/);
  }
});

function baselineObservation(which) {
  const stage = ['saturation', 'close', 'arm-consumer-panic', 'spawn-failure'][which - 1];
  const count = which === mode.saturation ? 65 : 1;
  const events = [ready];
  if (which === mode.spawnFailure) events.push({ spawnFailure: {
    code: 'WORKER_CONSTRUCT', message: 'LIBOS_RECLAIMER_SPAWN_DENIED_PROBE',
  } });
  events.push({ type: 'deferred-submissions-observed', mode: which, witness: {
    kind: 'observed-submissions', stage,
    production: {
      submissions: Array(count).fill('inline'),
      completedOnReturn: Array(count).fill(1),
      completedAtObservation: Array(count).fill(1),
      degradationBefore: null, degradationAfter: null,
    },
    spawnControl: which === mode.spawnFailure
      ? { result: 'rejected', kind: 'Other', detail: 'SPAWN_WORKER_FAILED' } : null,
  } });
  return { events };
}

test('真实 Inline 观测必须给出行为红灯，而不是缺 ABI 或假通过', timeout, () => {
  for (const which of Object.values(mode)) {
    assert.throws(() => assertDeferredLifecycleContract(baselineObservation(which), which),
      /真实生产 defer 的 (1|65) 项均为 Inline，且每项在提交返回前已析构/);
  }
});

test('构造失败的独立底座负控不能冒充 Reclaimer 启动证据', timeout, () => {
  const fake = baselineObservation(mode.spawnFailure);
  fake.events.splice(1, 1);
  assert.throws(() => assertDeferredLifecycleContract(fake, mode.spawnFailure), /真实 supervisor Worker 工厂/);
  const bad = baselineObservation(mode.spawnFailure);
  bad.events.at(-1).witness.spawnControl.result = 'unexpected-spawn-joined';
  assert.throws(() => assertDeferredLifecycleContract(bad, mode.spawnFailure), /rejected/);
});
