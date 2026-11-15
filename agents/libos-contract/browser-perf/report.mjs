import assert from 'node:assert/strict';
import { summarizeBoundarySamples } from '../../../scripts/performance/libos-boundary-cost.mjs';

export const profiles = Object.freeze({
  'spawn-join': { children: ['libos-perf-join'], operations: 1, windows: { 'spawn-join-result': 'elapsedNs', 'spawn-join-retired': 'retiredNs' } },
  'message-handoff': { children: ['libos-perf-handoff'], operations: 512, windows: { 'message-handoff': 'elapsedNs' } },
  'shutdown-wake': { children: ['libos-perf-stop', 'libos-perf-stop'], operations: 1, windows: { 'shutdown-wake': 'elapsedNs' } },
  deferred: { children: ['engine-deferred-drop'], operations: 32, windows: { 'queued-drop-return': 'elapsedNs', 'queued-drop-complete': 'completedNs' } },
});

function key({ workerId, slot, gen }) {
  assert.ok(Number.isSafeInteger(workerId) && workerId > 0 &&
    Number.isSafeInteger(slot) && slot >= 0 && Number.isSafeInteger(gen) && gen >= 0,
  '缺少真实 Worker/slot/generation 身份');
  return `${workerId}:${slot}:${gen}`;
}

export function assertRetiredSample(profile, witness) {
  for (const field of ['children', 'protocolEntries', 'pendingRecovery']) {
    assert.equal(witness[field], 0, `样本结束时仍有 ${field}`);
  }
  assert.ok(Array.isArray(witness.observations), '缺少 Supervisor 事件');
  const spawns = new Map();
  const retired = new Set();
  for (const event of witness.observations) {
    if (event.type === 'childSpawn') {
      const id = key(event);
      assert.ok(!spawns.has(id), '重复 spawn 身份');
      spawns.set(id, event.runtimeName);
    } else if (event.type === 'childExit') {
      const id = key(event);
      assert.ok(spawns.has(id) && !retired.has(id), '错误或重复的 W3 身份');
      retired.add(id);
    }
  }
  assert.deepEqual([...spawns.values()].sort(), [...profile.children].sort(), '实际 Worker 拓扑不同');
  assert.equal(retired.size, spawns.size, '结果返回但实际 Worker 没有退出');
  assert.ok(Number.isFinite(witness.elapsedNs) && witness.elapsedNs > 0 &&
    Number.isFinite(witness.retiredNs) && witness.retiredNs >= witness.elapsedNs, '结果/物理回收顺序不实');
  if ('queued-drop-complete' in profile.windows) {
    assert.ok(Number.isFinite(witness.completedNs) &&
      witness.completedNs >= witness.elapsedNs && witness.completedNs <= witness.retiredNs, '排队/完成/退休计时边界倒置');
  } else {
    assert.equal(witness.completedNs, null, '不可混用析构完成与其他性能边界');
  }
}

export function summarizeBrowserWorkerCost(name, pairs, resolutionNs) {
  const profile = profiles[name];
  assert.ok(profile && Object.hasOwn(profiles, name), '未知性能探针');
  for (const pair of pairs) {
    assert.equal(pair.operations, profile.operations, '两臂必须使用相同业务工作量');
    assertRetiredSample(profile, pair.direct);
    assertRetiredSample(profile, pair.facade);
  }
  return Object.fromEntries(Object.entries(profile.windows).map(([metric, key]) => [metric, summarizeBoundarySamples(
    pairs.map(({ order, operations, direct, facade }) => ({
      order, operations, segments: 1, directNs: direct[key], facadeNs: facade[key],
    })), resolutionNs,
  )]));
}
