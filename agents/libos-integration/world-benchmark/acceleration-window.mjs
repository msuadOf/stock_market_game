import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFile} from 'node:fs/promises';
import {compareCandidate} from './candidate-native-compare.mjs';
import {continuousAcceleration} from './acceleration.mjs';

if (process.argv.length !== 5) throw Error('必须依次提供 setup、Native 与 WASM 样本');
const [setupBytes, nativeBytes, browserBytes] = await Promise.all(process.argv.slice(2).map(file => readFile(file)));
const setup = JSON.parse(setupBytes), native = JSON.parse(nativeBytes), browser = JSON.parse(browserBytes);
const hash = createHash('sha256').update(setupBytes).digest('hex');
assert.equal(native.setupSha256, hash, 'Native setup 摘要不符');
assert.equal(browser.result.result.setupSha256, hash, 'WASM setup 摘要不符');
compareCandidate(native, browser);
const wasm = browser.result.result;
assert.deepEqual(native.sample, wasm.sample, '两端窗口或事件数不符');
console.log(JSON.stringify({setupSha256:hash, window:native.sample,
  native:continuousAcceleration(setup, native.sample, native.elapsedMs),
  wasm:continuousAcceleration(setup, wasm.sample, wasm.elapsedMs)}));
