import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import {compareCanonical} from './canonical-compare.mjs';

export function compareMath(nativeSource, wasmSource, native, wasm) {
  const ids = nativeSource.map(row => row[0]);
  compareCanonical(nativeSource, wasmSource, ids);
  assert.equal(native.length, ids.length);
  assert.equal(wasm.length, ids.length);
  let rawDifferent = 0, canonicalDifferent = 0, nativeChanged = 0, wasmChanged = 0, nativeCanonicalChanged = 0, wasmCanonicalChanged = 0;
  let nativeMaxUlp = 0n, wasmMaxUlp = 0n;
  const float = hex => Buffer.from(hex, 'hex').readDoubleBE();
  for (let index = 0; index < ids.length; index++) {
    for (const [source, candidate] of [[nativeSource[index], native[index]], [wasmSource[index], wasm[index]]]) {
      assert.ok(Array.isArray(candidate) && candidate.length === 4);
      assert.equal(candidate[0], source[0]);
      assert.equal(candidate[1], source[3]);
      for (const bits of candidate.slice(2)) {
        assert.match(bits, /^[0-9a-f]{16}$/);
        assert.ok(Number.isFinite(float(bits)) && float(bits) > 0 && float(bits) <= 1);
      }
    }
    rawDifferent += Number(native[index][2] !== wasm[index][2]);
    canonicalDifferent += Number(native[index][3] !== wasm[index][3]);
    for (const [source, candidate, side] of [[nativeSource[index], native[index], 'native'], [wasmSource[index], wasm[index], 'wasm']]) {
      const a = BigInt(`0x${source[4]}`), b = BigInt(`0x${candidate[2]}`), diff = a > b ? a - b : b - a;
      if (side === 'native') { nativeChanged += Number(diff > 0n); nativeCanonicalChanged += Number(source[5] !== candidate[3]); if (diff > nativeMaxUlp) nativeMaxUlp = diff; }
      else { wasmChanged += Number(diff > 0n); wasmCanonicalChanged += Number(source[5] !== candidate[3]); if (diff > wasmMaxUlp) wasmMaxUlp = diff; }
    }
  }
  return {count:ids.length, rawDifferent, canonicalDifferent, nativeChanged, wasmChanged, nativeCanonicalChanged, wasmCanonicalChanged, nativeMaxUlp:String(nativeMaxUlp), wasmMaxUlp:String(wasmMaxUlp)};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 4) throw Error('必须依次提供 Native 和 WASM 样本路径');
  const [native, wasm] = await Promise.all(process.argv.slice(2).map(async file => JSON.parse(await readFile(file, 'utf8'))));
  const browser = wasm.result.result;
  assert.equal(native.seed, 42); assert.equal(browser.seed, 42); assert.equal(wasm.mode, 'libos');
  assert.ok(Array.isArray(wasm.errors) && wasm.errors.length === 0 && !Object.hasOwn(wasm.result, 'failure'));
  assert.equal(native.setupSha256, browser.setupSha256); assert.match(native.setupSha256, /^[0-9a-f]{64}$/);
  for (const hash of [native.binarySha256, wasm.wasmHash, wasm.glueHash]) assert.match(hash, /^[0-9a-f]{64}$/);
  const ids = Array.from({length:20007}, (_, index) => String(index + 1));
  assert.deepEqual(native.accountRoster, ['0', ...ids]); assert.deepEqual(browser.accountRoster, native.accountRoster);
  compareCanonical(native.observationTrace, browser.observationTrace, ids);
  for (const run of [native, browser]) { assert.equal(run.warm.from, 0); assert.equal(run.warm.to, 1000); assert.equal(run.sample.from, 1000); assert.equal(run.sample.to, 4000); }
  console.log(JSON.stringify(compareMath(native.observationTrace, browser.observationTrace, native.candidateTrace, browser.candidateTrace)));
}
