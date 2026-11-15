import assert from 'node:assert/strict';

const workerRuntimeName = 'engine-deferred-drop';

function readOutcome(evidence) {
  assert.equal(evidence.failure, undefined, '正常析构不应触发宿主故障清理');
  assert.equal(evidence.events.filter(event => event.type === 'ready').length, 1, '必须启动一个真实 WASM engine');
  const outcomes = evidence.events.filter(event => ['verification-result', 'verification-error'].includes(event.type));
  assert.equal(outcomes.length, 1, '必须收到唯一析构验证结果');
  const [outcome] = outcomes;
  assert.equal(outcome.type, 'verification-result', outcome.message);
  return outcome;
}

export function assertDeferredReclamationContract(evidence) {
  const outcome = readOutcome(evidence);
  assert.deepEqual(outcome.reclamation?.disposition, ['queued', 'queued'], '正常容量必须真正后台入队，不能同步析构');
  assert.deepEqual(outcome.reclamation.completedOnReturn, [0, 0], '固定消费者栏杆时，提交应在析构之前返回');
  assert.deepEqual(outcome.reclamation.completedAtBoundary, [1, 1], '回收边界后必须恰好完成每个析构一次');
  const spawns = evidence.events.filter(event => event.type === 'childSpawn' && event.runtimeName === workerRuntimeName);
  assert.equal(spawns.length, 1, '必须由 supervisor 观测到一个专属真实析构 Worker');
  const [spawn] = spawns;
  const exits = evidence.events.filter(event => event.type === 'childExit' && event.workerId === spawn.workerId);
  assert.equal(exits.length, 1, '必须收到析构 Worker 的真实退出，不只依赖 Rust 成功布尔值');
  const [exit] = exits;
  assert.deepEqual([exit.slot, exit.gen], [spawn.slot, spawn.gen], '退出必须与实际 Worker 槽世代对应');
  assert.ok(evidence.events.indexOf(spawn) < evidence.events.indexOf(exit)
    && evidence.events.indexOf(exit) < evidence.events.indexOf(outcome), '真实退出必须先于发布验证结果');
  // childExit 与同一回调内的 checkpoint/drain 观测先后可变；两者都必须早于结果。
  assert.ok(evidence.events.some((event, index) => event.type === 'childrenDrained'
    && evidence.events.indexOf(spawn) < index && index < evidence.events.indexOf(outcome)
    && event.activeChildren === 0 && event.manifestEntries === 0 && event.pendingRecovery === 0),
  '真实退出后，发布必须等待 supervisor 收敛至零资源');
  assert.deepEqual([outcome.children, outcome.protocolEntries, outcome.pendingRecovery], [0, 0, 0], '观察结束不得残留 Worker 或 slot');
}

/** 保留当前基线的事实观测；出现后台结果时不得用较弱的 Inline 口径通过。 */
export function assertKnownDeferredObservation(evidence) {
  const outcome = readOutcome(evidence);
  const disposition = outcome.reclamation?.disposition;
  if (Array.isArray(disposition) && disposition.every(item => item === 'queued')) {
    assertDeferredReclamationContract(evidence);
    return;
  }
  assert.deepEqual(outcome.reclamation, {
    disposition: ['inline', 'inline'],
    completedOnReturn: [1, 1],
    completedAtBoundary: [1, 1],
  }, '只接受已知 Inline 基线或满足严格条件的后台析构');
  assert.ok(!evidence.events.some(event => event.type === 'childSpawn' && event.runtimeName === workerRuntimeName),
    'Inline 基线不应创建后台析构 Worker');
}
