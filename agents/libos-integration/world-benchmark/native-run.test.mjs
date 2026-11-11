import {test} from 'node:test';
import assert from 'node:assert/strict';
import {collectNative} from './native-run.mjs';

test('从实际输入字节与二进制计算摘要并记录 Native 输出', async () => {
  const result = await collectNative('binary', 'setup', {
    read: async file => Buffer.from(file === 'setup' ? '{"a":1}' : 'binary'),
    run: async (binary, setup) => {
      assert.equal(binary, 'binary'); assert.equal(setup, 'setup');
      return '{"seed":42}';
    },
  });
  assert.equal(result.setupSha256, '015abd7f5cc57a2dd94b7590f04ad8084273905ee33ec5cebeae62276a97f862');
  assert.match(result.binarySha256, /^[0-9a-f]{64}$/);
  assert.equal(result.seed, 42);
});

test('执行期间输入或产物发生变化时拒绝结果', async () => {
  for (const changed of ['setup', 'binary']) {
    let count = 0;
    await assert.rejects(collectNative('binary', 'setup', {
      read: async file => Buffer.from(file === changed ? String(count++) : file),
      run: async () => '{"seed":42}',
    }), /发生变化/);
  }
});

test('进程失败和冲突字段不能生成结果', async () => {
  const read = async file => Buffer.from(file);
  await assert.rejects(collectNative('binary', 'setup', {
    read, run: async () => { throw Error('进程失败'); },
  }), /进程失败/);
  await assert.rejects(collectNative('binary', 'setup', {
    read, run: async () => '{"setupSha256":"伪造"}',
  }), /冲突/);
});
