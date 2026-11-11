import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compareTraces} from './trace-compare.mjs';

test('按顺序报告字段摘要的全部差异', () => {
  const a = [[3,'e71fa2190541574b'],[2,'0123456789abcdef']];
  const b = [[3,'e71fa2190541574b'],[3,'fedcba9876543210']];
  assert.deepEqual(compareTraces(a,b), {nativeCount:2,wasmCount:2,different:[1]});
  assert.deepEqual(compareTraces(a,b.slice(0,1)), {nativeCount:2,wasmCount:1,different:[1]});
});
test('拒绝结构不完整及格式错误的 trace', () => {
  assert.throws(() => compareTraces([[1,'bad']], []));
  assert.throws(() => compareTraces([], [[-1,'0123456789abcdef']]));
});
