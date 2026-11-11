import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compareMath} from './math-compare.mjs';

test('对齐账户和输入后比较候选的原始与 JSON 往返位值', () => {
  const source = [['310','4031a0d3d6da5ed0','15300','bf52e08f525d6d3a','3f52ddc6e6251ce9','3f52ddc6e6251ce9']];
  const native = [['310','bf52e08f525d6d3a','3f52ddc6e6251cea','3f52ddc6e6251ceb']];
  const wasm = [['310','bf52e08f525d6d3a','3f52ddc6e6251cea','3f52ddc6e6251ceb']];
  const result = compareMath(source, source, native, wasm);
  assert.equal(result.rawDifferent, 0);
  assert.equal(result.canonicalDifferent, 0);
  assert.equal(result.nativeChanged, 1);
  assert.equal(result.nativeCanonicalChanged, 1);
});

test('缺失、重排、错输入或非法位值均失败', () => {
  const source = [['310','4031a0d3d6da5ed0','15300','bf52e08f525d6d3a','3f52ddc6e6251ce9','3f52ddc6e6251ce9']];
  const candidate = [['310','bf52e08f525d6d3a','3f52ddc6e6251cea','3f52ddc6e6251ceb']];
  for (const invalid of [[], [['311',...candidate[0].slice(1)]], [['310','0000000000000000',...candidate[0].slice(2)]], [['310',candidate[0][1],'none',candidate[0][3]]]]) {
    assert.throws(() => compareMath(source, source, candidate, invalid));
  }
});
