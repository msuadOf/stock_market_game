import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdtemp, readFile, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {runBoundedCommand} from '../../../scripts/run-with-deadline.mjs';
import {failureDetail} from './failure-detail.mjs';

export function validateRestore(value) {
  assert.equal(value.seed, 42);
  assert.ok(value.executionOrder === 'old-first' || value.executionOrder === 'candidate-first', '执行顺序无效');
  assert.equal(value.saveTick, 1000);
  assert.equal(value.baselineStartTick, 1000);
  assert.equal(value.baselineEndTick, 4000);
  assert.equal(value.baselineRestoredEqualsInput, true, '旧算法恢复后的存档与输入不一致');
  assert.equal(value.npcCount, 20007);
  assert.ok(Number.isSafeInteger(value.recomputedProbabilityDifferences) && value.recomputedProbabilityDifferences > 0 && value.recomputedProbabilityDifferences <= value.npcCount);
  assert.equal(value.controlTraceEqual, true, '旧算法重复重建概率不同');
  for (const branch of [value.control, value.candidate]) {
    assert.equal(branch.startTick, 1000); assert.equal(branch.endTick, 4000);
    for (const field of ['restoredSaveEqual','restoredHashEqual','finalSaveEqual','finalHashEqual']) assert.equal(branch[field], true, `${field} 不一致`);
    assert.equal(branch.firstEventDifference, null, '事件发生差异');
    assert.equal(branch.firstLocalEventDifference, null, '局部事件顺序发生差异');
    assert.equal(branch.eventSteps, 3000);
    assert.equal(branch.firstScopedFactDifference, null, '局部 runtime 事实发生差异');
    assert.equal(branch.firstBusinessFactDifference, null, '资金、股份或交易业务事实发生差异');
    assert.equal(branch.factSteps, 3000, 'runtime 事实步数不完整');
    for (const field of ['trades','lifecycle','deskReceipts','groupRejections','auction','cancellations']) {
      assert.ok(Number.isSafeInteger(branch.factCounts?.[field]) && branch.factCounts[field] >= 0, `${field} 事实计数无效`);
    }
    assert.ok(branch.businessFacts && typeof branch.businessFacts === 'object', '缺少逐项业务事实');
    const factComparison = compareBusinessFacts(branch.businessFacts.baseline, branch.businessFacts.replay);
    assert.equal(factComparison.equal, true, `逐项业务事实发生差异：${factComparison.differences.join(', ')}`);
    for (const field of ['trades','rejections','cancellations','accountStates','marketStates']) {
      for (const phase of ['baseline','replay']) {
        assert.ok(Array.isArray(branch.businessFacts[phase][field]), `缺少 ${phase}.${field} 逐项业务事实`);
        assert.ok(branch.businessFacts[phase][field].every(fact => fact && typeof fact === 'object' && !Array.isArray(fact)), `${phase}.${field} 含无效事实`);
      }
    }
    for (const phase of ['baseline','replay']) {
      assert.ok(branch.businessFacts[phase].accountStates.length > 0, `${phase} 缺少账户资金/股份状态事实`);
      assert.ok(branch.businessFacts[phase].marketStates.length > 0, `${phase} 缺少行情快照事实`);
      for (const name of ['cancellations','trades','rejections']) assert.ok(branch.businessFacts[phase][name].length > 0, `${phase} 缺少非空 ${name} 事实`);
    }
  }
  assert.ok(Number.isSafeInteger(value.saveBytes) && value.saveBytes > 0);
  const requiredFacts = ['trades','deskReceipts','groupRejections','auction','cancellations'];
  const missingCoverage = requiredFacts.flatMap(field =>
    [ 'control', 'candidate' ].filter(name => value[name].factCounts[field] === 0).map(name => `${name}.${field}`));
  return {
    status: missingCoverage.length === 0 ? '计数类别非空；须结合逐项业务事实判定' : '未完成：真实业务事实覆盖不足',
    missingCoverage,
    limitations: ['计数只作诊断，不能替代逐项业务事实', 'accountStates 来自包含 NPC 的账户快照，含现金、持仓与预留资源', 'marketStates 是行情快照，不代表全量证券业务资产', '撤单、成交与拒单需由 businessFacts 中的完整事实逐条对照'],
  };
}

const hash = bytes => createHash('sha256').update(bytes).digest('hex');

function scopedProjection(facts, scopeFields, label) {
  assert.ok(Array.isArray(facts), `${label} 必须是事实数组`);
  return scopeFields.map(fields => {
    const projection = new Map();
    for (const fact of facts) {
      assert.ok(fact && typeof fact === 'object' && !Array.isArray(fact), `${label} 含无效事实`);
      const key = fields.map(field => fact[field]).find(value => value !== undefined);
      assert.ok(key !== undefined && (typeof key === 'string' || Number.isSafeInteger(key)), `${label} 缺少作用域身份`);
      const identity = JSON.stringify(key);
      const values = projection.get(identity) ?? [];
      values.push(fact);
      projection.set(identity, values);
    }
    return [...projection].sort(([a], [b]) => a.localeCompare(b));
  });
}

/// 对完整业务事实逐项比较；只忽略无关证券间交错，保留同作用域顺序。
export function compareBusinessFacts(left, right) {
  assert.ok(left && right, '缺少前后业务事实快照');
  const definitions = {
    trades: [['code','stock'],['account','accountId','account_id']],
    rejections: [['code','stock'],['account','accountId','account_id']],
    cancellations: [['code','stock'],['account','accountId','account_id']],
    accountStates: [['account','accountId','account_id']],
    marketStates: [['code','stock']],
  };
  const differences = [];
  for (const [name, scopes] of Object.entries(definitions)) {
    const a = scopedProjection(left[name], scopes, `基准 ${name}`);
    const b = scopedProjection(right[name], scopes, `回放 ${name}`);
    if (JSON.stringify(a) !== JSON.stringify(b)) differences.push(name);
  }
  return {equal:differences.length === 0, differences};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 5 && process.argv.length !== 6) throw Error('必须依次提供 Native 二进制、setup、输出路径和可选执行顺序');
  const [binary, setup, output] = process.argv.slice(2,5).map(value => path.resolve(value));
  const order = process.argv[5] ?? 'old-first';
  if (order !== 'old-first' && order !== 'candidate-first') throw Error('执行顺序无效');
  const directory = await mkdtemp(path.join(tmpdir(), 'stock-libos-restore-'));
  let result, failure, observed = {requestedOrder:order}, phase = '读取输入', cleanupSucceeded = false;
  let childStderr = '', childStderrTruncated = false;
  try {
    const setupHash = hash(await readFile(setup));
    const binaryHash = hash(await readFile(binary));
    observed = {requestedOrder:order, setupSha256:setupHash, binarySha256:binaryHash};
    const savePath = path.join(directory, 'legacy.json');
    phase = '运行恢复探针';
    const {stdout} = await runBoundedCommand({command:binary,args:[setup,savePath,order],timeoutMs:120000,captureOutput:true,onStderr:chunk => {
      const text = String(chunk);
      const remaining = 1_048_576 - childStderr.length;
      childStderr += text.slice(0, remaining);
      if (text.length > remaining) childStderrTruncated = true;
    }});
    phase = '解析并校验恢复结果';
    const values = JSON.parse(stdout);
    observed = {...values, requestedOrder:order, setupSha256:setupHash, binarySha256:binaryHash};
    assert.equal(values.executionOrder, order, '探针执行顺序与请求不符');
    const saved = await readFile(savePath);
    assert.equal(saved.length, values.saveBytes, '存档长度不一致');
    assert.equal(hash(await readFile(setup)), setupHash, 'setup 在运行期间改变');
    assert.equal(hash(await readFile(binary)), binaryHash, '二进制在运行期间改变');
    observed.legacySaveSha256 = hash(saved);
    for (const branch of [observed.control, observed.candidate]) {
      for (const field of ['firstEventDifference','firstLocalEventDifference']) if (branch?.[field]) {
        const difference = branch[field];
        difference.baselineSha256 = hash(Buffer.from(difference.baselineUtf8, 'utf8'));
        difference.otherSha256 = hash(Buffer.from(difference.otherUtf8, 'utf8'));
      }
    }
    const verification = validateRestore(observed);
    result = {...observed, verification};
  } catch (error) { failure = error; }
  try { await rm(directory, {recursive:true, force:true}); cleanupSucceeded = true; }
  catch (error) { failure = failure ? new AggregateError([failure,error], '验证和清理均失败') : error; }
  if (failure) {
    try { await writeFile(`${output}.failure`, JSON.stringify({phase, cleanupSucceeded, error:failureDetail(failure), childStderr, childStderrTruncated, observed}, null, 2), {flag:'wx'}); }
    catch (error) { throw new AggregateError([failure,error], '验证与失败证据发布均失败'); }
    throw failure;
  }
  await writeFile(output, JSON.stringify(result, null, 2), {flag:'wx'});
  console.log(JSON.stringify(result));
}
