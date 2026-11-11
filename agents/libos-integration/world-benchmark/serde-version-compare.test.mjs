import {test} from 'node:test';
import assert from 'node:assert/strict';
import {gzipSync} from 'node:zlib';
import {compareVersions, decodeSample} from './serde-version-compare.mjs';

test('读取原始或 gzip 样本并拒绝损坏数据', () => {
  const raw = Buffer.from('{"seed":42}');
  assert.deepEqual(decodeSample(raw), {seed:42});
  assert.deepEqual(decodeSample(gzipSync(raw)), {seed:42});
  assert.throws(() => decodeSample(Buffer.from([0x1f, 0x8b, 0])));
});

test('相同账户与输入下区分候选原始值及 JSON 值的变化', () => {
  const before = [['310','bf52e08f525d6d3a','3f52ddc6e6251cea','3f52ddc6e6251ceb']];
  const after = [['310','bf52e08f525d6d3a','3f52ddc6e6251cea','3f52ddc6e6251cea']];
  assert.deepEqual(compareVersions(before, after), {count:1, rawChanged:0, canonicalChanged:1});
});

test('不允许丢失、重排或替换输入', () => {
  const before = [['310','bf52e08f525d6d3a','3f52ddc6e6251cea','3f52ddc6e6251ceb']];
  for (const after of [[], [['311',...before[0].slice(1)]], [['310','0000000000000000',...before[0].slice(2)]], [['310',before[0][1],'none',before[0][3]]], [['310',before[0][1],'7ff0000000000000',before[0][3]]]]) {
    assert.throws(() => compareVersions(before, after));
  }
});
