import {test} from 'node:test';
import assert from 'node:assert/strict';
import {webcrypto} from 'node:crypto';
import {setupHash} from './setup-hash.mjs';

test('对实际 JSON UTF-8 字节计算 SHA-256', async () => {
  assert.equal(await setupHash({a:1}, webcrypto.subtle), '015abd7f5cc57a2dd94b7590f04ad8084273905ee33ec5cebeae62276a97f862');
});

test('摘要失败时显式传播错误', async () => {
  await assert.rejects(setupHash({}, {digest: async () => { throw Error('摘要失败'); }}), /摘要失败/);
});
