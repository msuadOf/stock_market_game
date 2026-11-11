import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compareCanonical} from './canonical-compare.mjs';

const row = ['310','4031a0d3d6da5ed0','15300','bf52e08f525d6d3a','3f52ddc6e6251ce9','3f52ddc6e6251ce9'];
test('检查完整身份与输入并统计原始和规范化差异', () => {
  const other = [...row]; other[4] = '3f52ddc6e6251cea'; other[5] = '3f52ddc6e6251ceb';
  const result = compareCanonical([row], [other], ['310']);
  assert.equal(result.rawDifferent, 1);
  assert.equal(result.canonicalDifferent, 1);
  assert.deepEqual(result.native310, row);
});
test('拒绝缺失、重复、输入不一致和非法字段', () => {
  const invalid = [
    [[], [row], ['310']],
    [[row,row], [row,row], ['310','311']],
    [[row], [[...row.slice(0,5),'none']], ['310']],
    [[row], [[row[0], '0000000000000000', ...row.slice(2)]], ['310']],
    [[row], [row], ['311']],
    [[[...row.slice(0,4),'7ff0000000000000',row[5]]], [row], ['310']],
  ];
  for (const args of invalid) assert.throws(() => compareCanonical(...args));
});
