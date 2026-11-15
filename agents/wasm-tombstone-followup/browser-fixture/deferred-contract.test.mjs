import assert from 'node:assert/strict';
import { test } from 'node:test';
import { assertDeferredReclamationContract, assertKnownDeferredObservation } from './deferred-contract.mjs';

function candidate() {
  return { events: [
    { type: 'ready' },
    { type: 'childSpawn', runtimeName: 'engine-deferred-drop', workerId: 1, slot: 0, gen: 1 },
    { type: 'childExit', workerId: 1, slot: 0, gen: 1, activeChildren: 0, manifestEntries: 1, pendingRecovery: 1 },
    { type: 'childrenDrained', activeChildren: 0, manifestEntries: 0, pendingRecovery: 0 },
    { type: 'verification-result', children: 0, protocolEntries: 0, pendingRecovery: 0,
      reclamation: { disposition: ['queued', 'queued'], completedOnReturn: [0, 0], completedAtBoundary: [1, 1] } },
  ] };
}

test('完整的后台提交、释放证据与 Worker 退出通过验收', { timeout: 10_000 }, () => {
  assert.doesNotThrow(() => assertDeferredReclamationContract(candidate()));
});

test('接受 supervisor 在同一退出回调先发布 drain 的合法调度', { timeout: 10_000 }, () => {
  const evidence = candidate();
  [evidence.events[2], evidence.events[3]] = [evidence.events[3], evidence.events[2]];
  assert.doesNotThrow(() => assertDeferredReclamationContract(evidence));
});

test('原有 Inline 路径及仅伪装 Queued 的同步路径均被拒绝', { timeout: 10_000 }, () => {
  const evidence = candidate();
  evidence.events.at(-1).reclamation.completedOnReturn = [1, 1];
  evidence.events.at(-1).reclamation.disposition = ['inline', 'inline'];
  assert.throws(() => assertDeferredReclamationContract(evidence), /正常容量必须真正后台入队/);
  evidence.events.at(-1).reclamation.disposition = ['queued', 'queued'];
  assert.throws(() => assertDeferredReclamationContract(evidence), /提交应在析构之前返回/);
});

test('过早返回、未释放、重复释放都不能冒充完成', { timeout: 10_000 }, () => {
  const evidence = candidate();
  evidence.events.at(-1).reclamation.completedAtBoundary = [0, 2];
  assert.throws(() => assertDeferredReclamationContract(evidence), /恰好完成每个析构一次/);
});

test('成功布尔值无法替代真实 Worker 同世代退出', { timeout: 10_000 }, () => {
  const evidence = candidate();
  evidence.events[2].gen++;
  assert.throws(() => assertDeferredReclamationContract(evidence), /槽世代对应/);
  evidence.events.splice(2, 1);
  assert.throws(() => assertDeferredReclamationContract(evidence), /析构 Worker 的真实退出/);
});

test('拒绝验证结果先于 Worker 退出，也拒绝仍有待回收资源', { timeout: 10_000 }, () => {
  const evidence = candidate();
  [evidence.events[2], evidence.events[4]] = [evidence.events[4], evidence.events[2]];
  assert.throws(() => assertDeferredReclamationContract(evidence), /真实退出必须先于/);
  [evidence.events[2], evidence.events[4]] = [evidence.events[4], evidence.events[2]];
  evidence.events[3].manifestEntries = 1;
  assert.throws(() => assertDeferredReclamationContract(evidence), /零资源/);
  evidence.events.splice(4, 0, { type: 'childrenDrained', activeChildren: 0, manifestEntries: 0, pendingRecovery: 0 });
  assert.doesNotThrow(() => assertDeferredReclamationContract(evidence));
});

test('默认观测准确识别旧 Inline 与新 Queued，拒绝混合及伪造结果', { timeout: 10_000 }, () => {
  const queued = candidate();
  assert.doesNotThrow(() => assertKnownDeferredObservation(queued));
  queued.events.splice(2, 1);
  assert.throws(() => assertKnownDeferredObservation(queued), /析构 Worker 的真实退出/);

  const baseline = candidate();
  baseline.events.splice(1, 2);
  baseline.events.at(-1).reclamation.disposition = ['inline', 'inline'];
  baseline.events.at(-1).reclamation.completedOnReturn = [1, 1];
  assert.doesNotThrow(() => assertKnownDeferredObservation(baseline));
  baseline.events.at(-1).reclamation.disposition[1] = 'queued';
  assert.throws(() => assertKnownDeferredObservation(baseline), /只接受已知 Inline 基线/);
});
