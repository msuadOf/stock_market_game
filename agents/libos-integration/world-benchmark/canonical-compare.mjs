import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

function asFloat(hex) {
  const bytes = Buffer.from(hex, 'hex');
  return bytes.readDoubleBE();
}

export function compareCanonical(native, wasm, expectedIds) {
  const expectedCount = expectedIds.length;
  for (const trace of [native, wasm]) {
    assert.ok(Array.isArray(trace) && trace.length === expectedCount, '换算记录数量不符');
    const ids = new Set();
    for (const row of trace) {
      assert.ok(Array.isArray(row) && row.length === 6, '换算记录格式无效');
      assert.match(row[0], /^[1-9][0-9]*$/, '账户 ID 无效');
      assert.ok(!ids.has(row[0]), '账户 ID 重复');
      ids.add(row[0]);
      for (const index of [1,3,4,5]) assert.match(row[index], /^[0-9a-f]{16}$/, '浮点位值缺失或无效');
      assert.match(row[2], /^[1-9][0-9]*$/, '交易日 tick 数无效');
      assert.ok(BigInt(row[2]) <= 18446744073709551615n, '交易日 tick 数越界');
      assert.ok(Number.isFinite(asFloat(row[1])) && asFloat(row[1]) > 0, '观察次数无效');
      assert.ok(Number.isFinite(asFloat(row[3])) && asFloat(row[3]) < 0, 'exp_m1 参数无效');
      for (const index of [4,5]) assert.ok(Number.isFinite(asFloat(row[index])) && asFloat(row[index]) > 0 && asFloat(row[index]) <= 1, '概率无效');
    }
    assert.deepEqual(trace.map(row => row[0]), expectedIds, '账户与预期 NPC 名册不一致');
    assert.ok(ids.has('310'), '缺少账户 310');
  }
  let rawDifferent = 0, canonicalDifferent = 0;
  for (let index = 0; index < expectedCount; index++) {
    const a = native[index], b = wasm[index];
    assert.deepEqual(a.slice(0,4), b.slice(0,4), `账户或计算输入不一致：${index}`);
    rawDifferent += Number(a[4] !== b[4]);
    canonicalDifferent += Number(a[5] !== b[5]);
  }
  return {count:expectedCount, rawDifferent, canonicalDifferent, native310:native.find(row => row[0] === '310'), wasm310:wasm.find(row => row[0] === '310')};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 4) throw Error('必须依次提供 Native 和 WASM 样本路径');
  const [native, wasm] = await Promise.all(process.argv.slice(2).map(async file => JSON.parse(await readFile(file, 'utf8'))));
  assert.equal(native.seed, 42);
  assert.equal(wasm.mode, 'libos');
  assert.ok(Array.isArray(wasm.errors) && wasm.errors.length === 0 && wasm.result && !Object.hasOwn(wasm.result, 'failure'));
  const browser = wasm.result.result;
  assert.equal(browser.seed, 42);
  assert.match(native.setupSha256, /^[0-9a-f]{64}$/);
  assert.equal(native.setupSha256, browser.setupSha256);
  for (const hash of [native.binarySha256, wasm.wasmHash, wasm.glueHash]) assert.match(hash, /^[0-9a-f]{64}$/);
  for (const run of [native, browser]) {
    assert.equal(run.warm.from, 0); assert.equal(run.warm.to, 1000);
    assert.equal(run.sample.from, 1000); assert.equal(run.sample.to, 4000);
  }
  assert.deepEqual(native.accountRoster, browser.accountRoster);
  const ids = Array.from({length:20007}, (_, index) => String(index + 1));
  assert.deepEqual(native.accountRoster, ['0', ...ids]);
  const result = compareCanonical(native.observationTrace, browser.observationTrace, ids);
  for (const [run, row] of [[native, result.native310], [browser, result.wasm310]]) {
    const tuple = JSON.parse(run.fieldSamples[0]);
    const strategy = JSON.parse(run.fieldSamples[1]);
    assert.equal(tuple[0], '310');
    assert.equal(strategy.ZiNoise.base_observation_probability, row[5], '账户 310 的最终策略值与规范化结果不同');
  }
  console.log(JSON.stringify(result));
}
