import assert from 'node:assert/strict';

const reclaimerName = 'engine-deferred-drop';
const panicSentinel = 'LIBOS_RECLAIMER_PANIC_PROBE';

export const deferredLifecycleModes = Object.freeze({
  saturation: 1, close: 2, consumerPanic: 3, spawnFailure: 4,
});

function exactlyOne(events, predicate, description) {
  const matches = events.filter(predicate);
  assert.equal(matches.length, 1, description);
  return matches[0];
}

function reclaimerSpawn(events) {
  const spawn = exactlyOne(events, event => event.type === 'childSpawn' && event.runtimeName === reclaimerName,
    '只能建立一个真实析构 Worker');
  assert.ok([spawn.workerId, spawn.slot, spawn.gen].every(Number.isSafeInteger), '必须保留实际槽世代身份');
  return spawn;
}

function sameIdentity(event, spawn) {
  return event.workerId === spawn.workerId && event.slot === spawn.slot && event.gen === spawn.gen;
}

function assertNormalRetirement(events, result) {
  const spawn = reclaimerSpawn(events);
  const exit = exactlyOne(events, event => event.type === 'childExit' && sameIdentity(event, spawn),
    '结果消费不能替代同一槽世代的真实 Worker 退出');
  const spawnAt = events.indexOf(spawn);
  const resultAt = events.indexOf(result);
  assert.ok(spawnAt < events.indexOf(exit) && events.indexOf(exit) < resultAt, '成功结果必须晚于真实退出');
  assert.ok(events.some((event, index) => index > spawnAt && index < resultAt && event.type === 'childrenDrained'
    && event.activeChildren === 0 && event.manifestEntries === 0 && event.pendingRecovery === 0),
  '成功结果必须晚于 supervisor 的零资源证据');
  assert.deepEqual([result.children, result.protocolEntries, result.pendingRecovery], [0, 0, 0]);
}

function assertRejectedTransfer(probe, code) {
  assert.deepEqual(probe, {
    result: 'rejected', code, sameValueReturned: true, dropsAtReturn: 0, dropsAfterCallerDrop: 1,
  }, '拒绝时不能静默内联、入队、转移或重复析构');
}

function assertConsumerPanic(evidence, events) {
  const spawn = reclaimerSpawn(events);
  const armed = exactlyOne(events, event => event.type === 'deferred-fault-armed', '必须先证明真实消费者已进入受控故障任务');
  assert.deepEqual(armed.witness, {
    submission: 'queued', workerEntered: true, releasedByCaller: false,
  });
  const trigger = exactlyOne(events, event => event.type === 'deferred-fault-trigger', '必须有显式外部释放故障栏杆的证据');
  const pending = exactlyOne(events, event => event.type === 'failure-pending', '实际 panic 必须走宿主终止路径');
  assert.ok(events.indexOf(spawn) < events.indexOf(armed) && events.indexOf(armed) < events.indexOf(trigger)
     && events.indexOf(trigger) < events.indexOf(pending),
  '必须先入队与进入真实消费者，再受控触发故障');
  const record = exactlyOne(evidence.failure?.context?.records ?? [], entry => sameIdentity(entry.identity, spawn),
    '故障必须发生在真实析构 Worker 并保留一致槽世代');
  assert.equal(record.tombstone?.state, 'live');
  assert.equal(record.tombstone?.kind, 'panic');
  assert.equal(record.tombstone?.message, panicSentinel);
  assert.equal(evidence.failure?.context?.incomplete, true, '故障不能声称证明正常回收');
  assert.ok(!events.some(event => event.type === 'verification-result'
    || (event.type === 'childExit' && sameIdentity(event, spawn)) || (event.type === 'finalRelease' && event.kind === 'released')),
  '消费者死亡后不得误报 join/drain 成功、正常退出或内存回收');
}

function rejectObservedBaseline(evidence, mode, observed) {
  const { events } = evidence;
  assert.equal(observed.mode, mode);
  assert.equal(observed.witness?.kind, 'observed-submissions');
  const stages = ['saturation', 'close', 'arm-consumer-panic', 'spawn-failure'];
  const stage = stages[mode - 1];
  assert.ok(stage !== undefined);
  assert.equal(observed.witness.stage, stage);
  const production = observed.witness.production;
  const count = stage === 'saturation' ? 65 : 1;
  for (const field of ['submissions', 'completedOnReturn', 'completedAtObservation']) {
    assert.equal(production[field]?.length, count, '实际生产探针必须观察完整的提交与析构计数');
  }
  if (stage === 'spawn-failure') {
    const denied = exactlyOne(events, event => event.spawnFailure?.code === 'WORKER_CONSTRUCT',
      '独立负控必须经过真实 supervisor Worker 工厂，不能用臆造的起线程失败替代');
    assert.ok(events.indexOf(denied) < events.indexOf(observed));
    assert.match(denied.spawnFailure.message, /LIBOS_RECLAIMER_SPAWN_DENIED_PROBE/);
    assert.equal(observed.witness.spawnControl?.result, 'rejected');
    assert.match(observed.witness.spawnControl?.detail, /SPAWN_WORKER_FAILED/);
    assert.ok(!events.some(event => event.type === 'childSpawn'), '受控拒绝不能产生已成立的 Worker');
  }
  assert.equal(evidence.failure, undefined, '观测 Inline 时不能在 engine 上误触发 panic');
  assert.ok(!events.some(event => event.type === 'deferred-fault-trigger'), '仅观察到 Inline 时不能触发消费者 panic');
  const allInline = production.submissions.every(value => value === 'inline')
    && production.completedOnReturn.every(value => value === 1)
    && production.completedAtObservation.every(value => value === 1);
  if (allInline) {
    assert.fail(
      `LibOS ${stage}：真实生产 defer 的 ${count} 项均为 Inline，且每项在提交返回前已析构；未满足异步 owner 契约`,
    );
  }
  assert.fail(`LibOS ${stage}：已有真实提交观测，但还没有 owner、栏杆和退出的完整证据：${JSON.stringify(production)}`);
}

export function assertDeferredLifecycleContract(evidence, mode) {
  const { events } = evidence;
  exactlyOne(events, event => event.type === 'ready', '必须真实初始化共享 WASM');
  const harnessErrors = events.filter(event => event.type === 'verification-error');
  assert.deepEqual(harnessErrors, [], '测试探针缺失或失败不能充当合同证据');
  const observed = events.filter(event => event.type === 'deferred-submissions-observed');
  if (observed.length > 0) {
    assert.equal(observed.length, 1, '必须只有一份实际基线观测');
    rejectObservedBaseline(evidence, mode, observed[0]);
  }
  if (mode === deferredLifecycleModes.consumerPanic) {
    assertConsumerPanic(evidence, events);
    return;
  }
  assert.equal(evidence.failure, undefined, '正常压力或可区分的启动失败不得被误报为在途消费者终止');
  const result = exactlyOne(events, event => event.type === 'verification-result', '必须取得真实测试探针结果');
  assert.equal(result.mode, mode, '结果不得冒充其他场景');
  const w = result.witness;

  if (mode === deferredLifecycleModes.spawnFailure) {
    const denied = exactlyOne(events, event => event.spawnFailure?.code === 'WORKER_CONSTRUCT',
      '启动失败正控必须来自 supervisor 的实际 Worker 创建入口');
    assert.ok(events.indexOf(denied) < events.indexOf(result));
    assert.equal(w.start, 'spawn-failed');
    assert.equal(w.close, 'not-started', '未启动不能报告 drain 或 join');
    assertRejectedTransfer(w.submitAfterFailedStart, 'start-failed');
    assert.ok(!events.some(event => event.type === 'childSpawn'), '启动失败不能留下已登记消费者或静默重启');
    assert.deepEqual([result.children, result.protocolEntries, result.pendingRecovery], [0, 0, 0]);
    return;
  }

  assertNormalRetirement(events, result);
  if (mode === deferredLifecycleModes.saturation) {
    assert.equal(w.gateEntered, true, '必须在实际消费者端命中确定性栏杆');
    assert.ok(Number.isSafeInteger(w.capacity) && w.capacity > 0 && w.capacity <= 4096, '需要实际有界队列容量');
    const ids = Array.from({ length: w.capacity }, (_, i) => i);
    assert.deepEqual(w.submissions, [...ids.map(() => 'queued'), 'full-inline']);
    assert.deepEqual(w.completedWhileGated, [{ id: w.capacity, by: 'caller' }],
      '溢出值在调用者内联恰一次，已排队值不得提前释放');
    assert.equal(w.degradationAfterOverflow, null, '队列满不能冒充消费者断开');
    assert.equal(w.close, 'drained-and-joined');
    assert.deepEqual(w.completedAtClose, [
      { id: w.capacity, by: 'caller' }, ...ids.map(id => ({ id, by: 'consumer' })),
    ], '排空必须由消费者按 FIFO 恰好执行每个已接纳析构');
    return;
  }

  assert.equal(mode, deferredLifecycleModes.close, '未知析构合同模式');
  assertRejectedTransfer(w.submitBeforeStart, 'not-started');
  assert.equal(w.closeBeforeStart, 'not-started');
  assert.equal(w.start, 'started');
  assert.equal(w.duplicateStart, 'already-started');
  assert.deepEqual(w.submissions, ['queued', 'queued']);
  assert.deepEqual(w.completedAtCloseEntry, [], '关闭入口进入时，受控任务必须仍未完成');
  assert.deepEqual(w.order, ['close-entered', 'gate-released', 'consumer-completed', 'close-returned'],
    '必须先在真实关闭路径内观察到排空等待，关闭不能在消费者完成前返回');
  assert.deepEqual(w.completedAtClose, [{ id: 0, by: 'consumer' }, { id: 1, by: 'consumer' }]);
  assert.equal(w.close, 'drained-and-joined');
  assert.equal(w.duplicateClose, 'already-closed');
  assertRejectedTransfer(w.submitAfterClose, 'closed');
}
