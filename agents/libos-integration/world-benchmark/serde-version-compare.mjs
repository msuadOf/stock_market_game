import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {gunzipSync} from 'node:zlib';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

export function decodeSample(bytes) {
  const contents = bytes[0] === 0x1f && bytes[1] === 0x8b ? gunzipSync(bytes) : bytes;
  return JSON.parse(contents.toString('utf8'));
}

export function compareVersions(before, after) {
  assert.ok(Array.isArray(before) && Array.isArray(after) && before.length > 0 && before.length === after.length, '候选记录数量不一致');
  const ids = new Set();
  let rawChanged = 0, canonicalChanged = 0;
  for (let index = 0; index < before.length; index++) {
    const a = before[index], b = after[index];
    for (const row of [a,b]) {
      assert.ok(Array.isArray(row) && row.length === 4, '候选记录格式无效');
      assert.match(row[0], /^[1-9][0-9]*$/, '账户 ID 无效');
      for (const value of row.slice(1)) assert.match(value, /^[0-9a-f]{16}$/, '位值无效');
      const number = value => Buffer.from(value, 'hex').readDoubleBE();
      assert.ok(Number.isFinite(number(row[1])) && number(row[1]) < 0, '输入参数无效');
      for (const value of row.slice(2)) assert.ok(Number.isFinite(number(value)) && number(value) > 0 && number(value) <= 1, '概率无效');
    }
    assert.ok(!ids.has(a[0]), '账户 ID 重复'); ids.add(a[0]);
    assert.deepEqual(a.slice(0,2), b.slice(0,2), `账户或参数不一致：${index}`);
    rawChanged += Number(a[2] !== b[2]);
    canonicalChanged += Number(a[3] !== b[3]);
  }
  return {count:before.length, rawChanged, canonicalChanged};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 5) throw Error('必须依次提供模式、旧样本和新样本');
  const [mode, oldPath, newPath] = process.argv.slice(2);
  assert.ok(mode === 'native' || mode === 'libos', '模式无效');
  const oldFile = decodeSample(await readFile(oldPath));
  const newFile = decodeSample(await readFile(newPath));
  const before = mode === 'native' ? oldFile : oldFile.result.result;
  const after = mode === 'native' ? newFile : newFile.result.result;
  if (mode === 'libos') for (const file of [oldFile,newFile]) {
    assert.equal(file.mode, 'libos');
    assert.ok(Array.isArray(file.errors) && file.errors.length === 0 && !Object.hasOwn(file.result, 'failure'));
  }
  for (const file of [oldFile,newFile]) {
    const hashes = mode === 'native' ? [file.binarySha256] : [file.wasmHash, file.glueHash];
    for (const hash of hashes) assert.match(hash, /^[0-9a-f]{64}$/, '产物摘要无效');
  }
  const ids = Array.from({length:20007}, (_, index) => String(index + 1));
  for (const value of [before,after]) {
    assert.equal(value.seed, 42);
    assert.equal(value.warm.from, 0); assert.equal(value.warm.to, 1000);
    assert.equal(value.sample.from, 1000); assert.equal(value.sample.to, 4000);
    assert.deepEqual(value.accountRoster, ['0', ...ids]);
    assert.deepEqual(value.candidateTrace.map(row => row[0]), ids);
  }
  assert.match(before.setupSha256, /^[0-9a-f]{64}$/);
  assert.equal(before.setupSha256, after.setupSha256);
  const artifacts = mode === 'native'
    ? {before:oldFile.binarySha256, after:newFile.binarySha256}
    : {before:oldFile.wasmHash, after:newFile.wasmHash, beforeGlue:oldFile.glueHash, afterGlue:newFile.glueHash};
  console.log(JSON.stringify({...compareVersions(before.candidateTrace, after.candidateTrace), artifacts}));
}
