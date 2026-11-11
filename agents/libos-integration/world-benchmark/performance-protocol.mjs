import assert from 'node:assert/strict';

const PLATFORMS = Object.freeze(['native', 'wasm']);
const PATHS = Object.freeze(['core-diagnostic', 'product-lifecycle']);

function digest(value, label) {
  assert.match(value, /^[0-9a-f]{64}$/, `${label} 必须是 SHA-256`);
}

function median(values) {
  const ordered = [...values].sort((a, b) => a - b);
  const middle = Math.floor(ordered.length / 2);
  return ordered.length % 2 ? ordered[middle] : (ordered[middle - 1] + ordered[middle]) / 2;
}

/** 为每条路径生成 Native/WASM 交错次序，减少固定先后顺序带来的偏差。 */
export function interleavedSchedule(rounds) {
  assert.ok(Number.isSafeInteger(rounds) && rounds > 0, '采样轮数必须由执行前计划指定');
  return Array.from({length:rounds}, (_, round) => {
    const first = round % 2 === 0 ? PLATFORMS : [...PLATFORMS].reverse();
    return first.map((platform, position) => ({round, position, platform}));
  }).flat();
}

/** 校验单次测量身份；core 与产品生命周期必须各自形成独立样本组。 */
export function validatePerformanceSample(sample) {
  assert.ok(sample && typeof sample === 'object', '性能样本必须是对象');
  assert.ok(PLATFORMS.includes(sample.platform), '运行平台无效');
  assert.ok(PATHS.includes(sample.path), '测量路径无效');
  assert.ok(Number.isSafeInteger(sample.round) && sample.round >= 0, '采样轮次无效');
  assert.ok(Number.isSafeInteger(sample.position) && (sample.position === 0 || sample.position === 1), '交错位置无效');
  assert.equal(sample.position, (sample.round % 2 === 0) === (sample.platform === 'native') ? 0 : 1, '样本不符合固定交错次序');
  assert.equal(sample.seed, 42, 'seed 不一致');
  for (const field of ['setupSha256','artifactSha256','dependencyLockSha256','workloadSha256','environmentSha256']) digest(sample[field], field);
  assert.equal(sample.release, true, '性能验收必须使用 release 产物');
  assert.ok(Number.isSafeInteger(sample.threadCount) && sample.threadCount > 0, '线程数无效');
  assert.ok(Number.isFinite(sample.elapsedMs) && sample.elapsedMs > 0, '墙钟耗时必须为正数');
  assert.ok(Number.isFinite(sample.warmupMs) && sample.warmupMs >= 0, '预热耗时无效');
  assert.equal(sample.warmupTicks, 1000, '预热工作量必须固定为 1000 ticks');
  assert.ok(Number.isSafeInteger(sample.measuredTicks) && sample.measuredTicks > 0, '测量工作量无效');
  assert.equal(typeof sample.timingBoundary, 'string');
  assert.ok(sample.timingBoundary.length > 0, '必须记录计时起止边界');
  if (sample.path === 'product-lifecycle') {
    for (const phase of ['uiReady','restoreOrCreate','crossDayAdvance','saveComplete']) {
      assert.equal(sample.productPhases?.[phase], true, `产品路径缺少 ${phase} 证据`);
    }
    assert.ok(Number.isFinite(sample.simulatedSeconds) && sample.simulatedSeconds > 0, '产品路径必须提供实际游戏时钟秒数');
    assert.equal(sample.saveIncluded, true, '完整产品计时必须包含保存耗时');
    assert.equal(sample.productHostKind, 'real-product-host', '产品样本必须来自真实产品宿主');
    digest(sample.productEvidenceSha256, 'productEvidenceSha256');
  }
  return sample;
}

function sameWork(left, right) {
  for (const field of ['seed','setupSha256','dependencyLockSha256','workloadSha256','environmentSha256','threadCount','release','warmupTicks','measuredTicks','timingBoundary']) {
    assert.equal(left[field], right[field], `Native/WASM 工作身份不一致：${field}`);
  }
  if (left.path === 'product-lifecycle') {
    assert.equal(left.simulatedSeconds, right.simulatedSeconds, 'Native/WASM 游戏时钟跨度不一致');
    assert.deepEqual(left.productPhases, right.productPhases, 'Native/WASM 产品生命周期阶段不一致');
    assert.equal(left.saveIncluded, right.saveIncluded, 'Native/WASM 保存计时范围不一致');
    assert.equal(left.productHostKind, right.productHostKind, 'Native/WASM 产品宿主类型不一致');
  }
}

/** 只汇总同一路径、同工作量的有效交错样本；不从 tick/s 推导加速比。 */
export function summarizePerformance(samples, {plannedRounds = null, wasmComparableThreshold = null} = {}) {
  if (plannedRounds !== null) assert.ok(Number.isSafeInteger(plannedRounds) && plannedRounds > 0, '采样计划轮数必须是正整数');
  assert.ok(Array.isArray(samples), '性能样本必须是数组');
  for (const sample of samples) validatePerformanceSample(sample);
  const paths = {};
  for (const path of PATHS) {
    const rows = samples.filter(sample => sample.path === path);
    const byRound = new Map();
    for (const row of rows) {
      const key = `${row.round}:${row.platform}`;
      assert.ok(!byRound.has(key), `重复样本：${key}`);
      byRound.set(key, row);
    }
    const completeRounds = [...new Set(rows.map(row => row.round))].filter(round => {
      const native = byRound.get(`${round}:native`), wasm = byRound.get(`${round}:wasm`);
      if (!native || !wasm) return false;
      assert.notEqual(native.position, wasm.position, '每轮 Native/WASM 必须交错先后');
      sameWork(native, wasm);
      return true;
    });
    const paired = completeRounds.flatMap(round => [byRound.get(`${round}:native`), byRound.get(`${round}:wasm`)]);
    if (plannedRounds !== null) {
      assert.ok(rows.every(row => row.round < plannedRounds), `${path} 存在超出预定轮数的样本`);
    }
    if (paired.length > 0) for (const row of paired.slice(1)) sameWork(paired[0], row);
    for (const platform of PLATFORMS) {
      const artifacts = new Set(paired.filter(row => row.platform === platform).map(row => row.artifactSha256));
      assert.ok(artifacts.size <= 1, `${path} 的 ${platform} 产物在采样期间改变`);
    }
    const native = paired.filter(row => row.platform === 'native').map(row => row.elapsedMs);
    const wasm = paired.filter(row => row.platform === 'wasm').map(row => row.elapsedMs);
    const timeSummary = values => values.length ? {medianMs:median(values), minMs:Math.min(...values), maxMs:Math.max(...values)} : {medianMs:null,minMs:null,maxMs:null};
    const artifactIdentities = Object.fromEntries(PLATFORMS.map(platform => [platform,
      [...new Set(paired.filter(row => row.platform === platform).map(row => row.artifactSha256))]]));
    const environmentIdentities = [...new Set(paired.map(row => row.environmentSha256))];
    const dependencyIdentities = [...new Set(paired.map(row => row.dependencyLockSha256))];
    const pathsEntry = {
      samplesPerPlatform: completeRounds.length,
      plannedRounds,
      missingRounds:plannedRounds === null ? null : Array.from({length:plannedRounds}, (_, round) => round).filter(round => !completeRounds.includes(round)),
      unpairedSamples: rows.length - paired.length,
      nativeElapsedMs:timeSummary(native),
      wasmElapsedMs:timeSummary(wasm),
      medianWasmToNative: native.length ? median(wasm) / median(native) : null,
      medianSimulatedSecondsPerWallSecond: path === 'product-lifecycle' && native.length
        ? Object.fromEntries(PLATFORMS.map(platform => [platform, median(paired.filter(row => row.platform === platform).map(row => row.simulatedSeconds * 1000 / row.elapsedMs))]))
        : null,
      artifactSha256:artifactIdentities,
      environmentSha256:environmentIdentities,
      dependencyLockSha256:dependencyIdentities,
      threadCounts:[...new Set(paired.map(row => row.threadCount))],
      releaseArtifacts:paired.every(row => row.release),
      status: plannedRounds !== null && completeRounds.length === plannedRounds && rows.length === paired.length
        ? '样本数量符合执行前计划'
        : plannedRounds === null ? '未完成：执行前采样轮数尚未确定' : '未完成：样本数与执行前计划不符或存在未配对样本',
    };
    paths[path] = pathsEntry;
  }
  if (paths['product-lifecycle'].samplesPerPlatform > 0) {
    paths['product-lifecycle'].productHostEvidenceSha256 = [...new Set(samples.filter(sample => sample.path === 'product-lifecycle').map(sample => sample.productEvidenceSha256))];
  }
  paths['product-lifecycle'].acceptance = '未完成：产品计时边界与样本规模尚待用户审核，且需检查真实宿主证据';
  if (wasmComparableThreshold === null) {
    paths['core-diagnostic'].acceptance = '未完成：WASM 与 Native 相近的阈值待用户确定';
  } else {
    assert.ok(Number.isFinite(wasmComparableThreshold) && wasmComparableThreshold > 0, '相近阈值必须为正数');
    const ratio = paths['core-diagnostic'].medianWasmToNative;
    const planComplete = paths['core-diagnostic'].status === '样本数量符合执行前计划';
    paths['core-diagnostic'].acceptance = !planComplete ? '未完成：采样计划缺失或样本与计划不符'
      : ratio === null ? '未完成：缺少有效配对样本'
      : ratio <= wasmComparableThreshold ? '达到用户设定阈值' : '未达到用户设定阈值';
  }
  paths['product-lifecycle'].productAccelerationTarget = '约 3000×；计时边界与样本规模尚待用户审核';
  return {status:'仅报告证据，不代表性能验收通过', paths};
}
