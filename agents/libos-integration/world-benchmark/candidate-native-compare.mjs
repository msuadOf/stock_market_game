import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import {compareNative} from './native-compare.mjs';
import {compareCanonical} from './canonical-compare.mjs';
import {validateRequestedProfile} from './step-profile.mjs';

function validateCandidateTrace(value) {
  assert.ok(Array.isArray(value.observationTrace) && value.observationTrace.length > 0, '缺少实际概率追踪');
  assert.ok(Array.isArray(value.candidateTrace), '缺少候选概率追踪');
  assert.equal(value.observationTrace.length, value.candidateTrace.length, '候选追踪数量不符');
  for (let index = 0; index < value.observationTrace.length; index++) {
    const observed = value.observationTrace[index], candidate = value.candidateTrace[index];
    assert.ok(Array.isArray(observed) && observed.length === 6 && Array.isArray(candidate) && candidate.length === 4, '概率追踪形状不符');
    assert.deepEqual([observed[0], observed[3], observed[4], observed[5]], candidate, `第 ${index} 项实际概率与候选不一致`);
    for (const bits of candidate.slice(1)) assert.match(bits, /^[0-9a-f]{16}$/, '概率位值无效');
  }
}

export function compareCandidate(native, browser, expectedIds = Array.from({length:20007}, (_, index) => String(index + 1))) {
  assert.equal(native.mathMode, 'libm', 'Native 必须使用候选数学模式');
  assert.equal(browser.mathMode, 'libm', '浏览器必须使用候选数学模式');
  assert.equal(browser.result?.result?.mathMode, 'libm', '浏览器实际数学模式不符');
  assert.equal(browser.workerMode, 'libos', '浏览器必须使用实验性 LibOS 下行');
  assert.match(native.binarySha256, /^[0-9a-f]{64}$/, 'Native 产物摘要无效');
  assert.match(browser.wasmHash, /^[0-9a-f]{64}$/, 'WASM 摘要无效');
  assert.match(browser.glueHash, /^[0-9a-f]{64}$/, 'glue 摘要无效');
  // 早期样本未声明画像模式；仅在两侧都没有账本时按未启用处理。
  const profileMode = Object.hasOwn(browser, 'profileMode') ? browser.profileMode : '0';
  validateRequestedProfile(native, profileMode);
  validateRequestedProfile(browser.result.result, profileMode);
  validateCandidateTrace(native);
  validateCandidateTrace(browser.result.result);
  assert.deepEqual(native.accountRoster, ['0', ...expectedIds], 'Native 名册不符');
  assert.deepEqual(browser.result.result.accountRoster, native.accountRoster, '两端名册不符');
  const canonical = compareCanonical(native.observationTrace, browser.result.result.observationTrace, expectedIds);
  assert.equal(canonical.rawDifferent, 0, '两端实际概率不同');
  assert.equal(canonical.canonicalDifferent, 0, '两端规范化概率不同');
  assert.deepEqual(native.candidateTrace, browser.result.result.candidateTrace, '两端候选概率追踪不一致');
  return compareNative(native, browser);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 4) throw Error('必须依次提供 Native 与 WASM 样本');
  const [native, browser] = await Promise.all(process.argv.slice(2).map(async file => JSON.parse(await readFile(file, 'utf8'))));
  console.log(JSON.stringify(compareCandidate(native, browser)));
}
