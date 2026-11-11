import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import {compareMeasurements} from './compare.mjs';

export function compareWorkerSamples(standard, experimental) {
  assert.equal(standard.workerMode, 'std', '对照组必须使用 std 下行');
  assert.equal(experimental.workerMode, 'libos', '实验组必须使用 LibOS 下行');
  for (const sample of [standard, experimental]) {
    assert.equal(sample.mode, 'libos', '两组必须使用 LibOS 上行');
    assert.ok(Array.isArray(sample.errors) && sample.errors.length === 0, '样本包含页面错误');
    assert.ok(sample.result && !Object.hasOwn(sample.result, 'failure') && sample.result.result, '样本未成功完成');
    assert.match(sample.wasmHash, /^[0-9a-f]{64}$/, 'WASM 摘要无效');
    assert.match(sample.glueHash, /^[0-9a-f]{64}$/, 'glue 摘要无效');
    const value = sample.result.result;
    assert.match(value.setupSha256, /^[0-9a-f]{64}$/, 'setup 摘要无效');
    assert.equal(sample.fineWaitUs, value.fineWaitUs, '实际等待参数不符');
    assert.ok(Number.isSafeInteger(sample.fineWaitUs) && sample.fineWaitUs > 0, '等待参数无效');
    assert.ok(Number.isFinite(value.elapsedMs) && value.elapsedMs > 0, '推进耗时无效');
  }
  const left = standard.result.result, right = experimental.result.result;
  // 旧批次没有数学模式字段；新批次必须两端与各自内部记录一致。
  assert.equal(standard.mathMode, experimental.mathMode, '数学模式不一致');
  if (standard.mathMode !== undefined) {
    assert.ok(['std', 'libm'].includes(standard.mathMode), '数学模式无效');
    assert.equal(left.mathMode, standard.mathMode, '对照组实际数学模式不符');
    assert.equal(right.mathMode, experimental.mathMode, '实验组实际数学模式不符');
  } else {
    assert.equal(left.mathMode, undefined, '旧对照组数学模式记录不一致');
    assert.equal(right.mathMode, undefined, '旧实验组数学模式记录不一致');
  }
  assert.equal(standard.fineWaitUs, experimental.fineWaitUs, '等待参数不一致');
  assert.equal(left.seed, right.seed, 'seed 不一致');
  assert.equal(left.seed, 42, 'seed 不符');
  assert.equal(left.setupSha256, right.setupSha256, 'setup 不一致');
  return compareMeasurements(left, right);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 4) throw Error('必须依次提供 std 下行和 LibOS 下行样本');
  const [standard, experimental] = await Promise.all(process.argv.slice(2).map(async file => JSON.parse(await readFile(file, 'utf8'))));
  console.log(JSON.stringify(compareWorkerSamples(standard, experimental)));
}
