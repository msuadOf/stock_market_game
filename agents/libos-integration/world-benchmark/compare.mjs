import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import {validateBusinessHash} from './measure.mjs';

export function compareSamples(standard, libos) {
  assert.equal(standard.mode, 'std', '第一个样本必须是 std');
  assert.equal(libos.mode, 'libos', '第二个样本必须是 libos');
  for (const value of [standard, libos]) {
    assert.ok(Array.isArray(value.errors) && value.errors.length === 0, '样本包含页面错误或缺少错误记录');
    assert.ok(value.result && !Object.hasOwn(value.result, 'failure') && value.result.result, '样本未成功完成');
  }
  return compareMeasurements(standard.result.result, libos.result.result);
}

export function compareMeasurements(left, right) {
  for (const [phase, from, to] of [['warm', 0, 1000], ['sample', 1000, 4000]]) {
    for (const value of [left[phase], right[phase]]) {
      assert.equal(value.from, from, `${phase} 起点不符`);
      assert.equal(value.to, to, `${phase} 终点不符`);
      assert.ok(Number.isSafeInteger(value.events) && value.events >= 0, `${phase} 事件数无效`);
    }
    assert.equal(left[phase].events, right[phase].events, `${phase} 事件数不一致`);
  }
  const businessHash = validateBusinessHash(left.businessHash);
  assert.equal(businessHash, validateBusinessHash(right.businessHash), '业务状态摘要不一致');
  return {businessHash};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 4) throw Error('必须依次提供 std 和 libos 样本路径');
  const [standard, libos] = await Promise.all(process.argv.slice(2).map(async file => JSON.parse(await readFile(file, 'utf8'))));
  console.log(JSON.stringify(compareSamples(standard, libos)));
}
