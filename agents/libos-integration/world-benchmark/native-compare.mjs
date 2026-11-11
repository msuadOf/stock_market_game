import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import {compareMeasurements} from './compare.mjs';

export function compareNative(native, browser) {
  assert.equal(native.seed, 42, 'Native seed 不符');
  assert.equal(browser.mode, 'libos', '浏览器样本必须启用 LibOS');
  assert.ok(Array.isArray(browser.errors) && browser.errors.length === 0, '浏览器样本错误');
  assert.ok(browser.result && !Object.hasOwn(browser.result, 'failure') && browser.result.result, '浏览器样本未成功');
  const wasm = browser.result.result;
  assert.equal(wasm.seed, 42, '浏览器 seed 不符');
  assert.match(native.setupSha256, /^[0-9a-f]{64}$/, 'Native setup 摘要无效');
  assert.equal(native.setupSha256, wasm.setupSha256, 'setup 摘要不一致');
  for (const elapsed of [native.elapsedMs, wasm.elapsedMs]) {
    assert.ok(typeof elapsed === 'number' && Number.isFinite(elapsed) && elapsed > 0, '采样耗时无效');
  }
  const {businessHash} = compareMeasurements(native, wasm);
  return {businessHash, nativeToWasm:wasm.elapsedMs / native.elapsedMs};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 4) throw Error('必须依次提供 Native 和浏览器样本路径');
  const [native, browser] = await Promise.all(process.argv.slice(2).map(async file => JSON.parse(await readFile(file, 'utf8'))));
  console.log(JSON.stringify(compareNative(native, browser)));
}
