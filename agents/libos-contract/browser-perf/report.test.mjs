import assert from 'node:assert/strict';
import test from 'node:test';
import { assertRetiredSample, profiles, summarizeBrowserWorkerCost } from './report.mjs';

function witness(profile) {
  return {
    children: 0, protocolEntries: 0, pendingRecovery: 0,
    elapsedNs: 1_000_000, retiredNs: 2_000_000,
    completedNs: 'queued-drop-complete' in profile.windows ? 1_100_000 : null,
    observations: profile.children.flatMap((runtimeName, i) => {
      const identity = { workerId: i + 1, slot: i, gen: 1 };
      return [{ ...identity, type: 'childSpawn', runtimeName }, { ...identity, type: 'childExit' }];
    }),
  };
}

test('只有结果与不重复的真实 Worker 退出同身份，且全部资源清空，才可接受样本', () => {
  const profile = profiles['shutdown-wake'];
  const actual = witness(profile);
  assert.doesNotThrow(() => assertRetiredSample(profile, actual));
  assert.throws(() => assertRetiredSample(profile, { ...actual, pendingRecovery: 1 }), /仍有/);
  assert.throws(() => assertRetiredSample(profile, { ...actual, observations: actual.observations.slice(0, -1) }), /没有退出/);
  const wrongGeneration = actual.observations.map((event, i) => i === 1 ? { ...event, gen: 2 } : event);
  assert.throws(() => assertRetiredSample(profile, { ...actual, observations: wrongGeneration }), /身份/);
  const duplicate = [...actual.observations, actual.observations[1]];
  assert.throws(() => assertRetiredSample(profile, { ...actual, observations: duplicate }), /重复/);
  assert.throws(() => assertRetiredSample(profile, { ...actual, retiredNs: 1 }), /顺序/);
});

test('后台完成不得早于全部排队或晚于真实退休', () => {
  const profile = profiles.deferred;
  const actual = witness(profile);
  assert.doesNotThrow(() => assertRetiredSample(profile, actual));
  for (const completedNs of [null, 999_999, 2_000_001]) {
    assert.throws(() => assertRetiredSample(profile, { ...actual, completedNs }), /边界倒置/);
  }
});

test('成功 join 和真实退休分别计算；两臂工作量或顺序不同则不出报告', () => {
  const profile = profiles['spawn-join'];
  const direct = witness(profile);
  const facade = { ...direct, elapsedNs: direct.elapsedNs + 10, retiredNs: direct.retiredNs + 50 };
  const pairs = Array.from({ length: 31 }, (_, i) => ({
    order: i % 2 ? 'facade-direct' : 'direct-facade', operations: 1, direct, facade,
  }));
  const byMetric = summarizeBrowserWorkerCost('spawn-join', pairs, 1);
  assert.equal(byMetric['spawn-join-result'].medianDeltaNsPerOperation, 10);
  assert.equal(byMetric['spawn-join-retired'].medianDeltaNsPerOperation, 50);
  assert.throws(() => summarizeBrowserWorkerCost('spawn-join', [{ ...pairs[0], operations: 2 }, ...pairs.slice(1)], 1), /工作量/);
  assert.throws(() => summarizeBrowserWorkerCost('spawn-join', [{ ...pairs[0], order: 'facade-direct' }, ...pairs.slice(1)], 1), /配对顺序/);
});
