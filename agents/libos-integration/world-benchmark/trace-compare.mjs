import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

export function compareTraces(native, wasm) {
  for (const trace of [native, wasm]) {
    assert.ok(Array.isArray(trace), '字段追踪必须是数组');
    for (const field of trace) {
      assert.ok(Array.isArray(field) && field.length === 2 && Number.isSafeInteger(field[0]) && field[0] >= 0 && typeof field[1] === 'string' && /^[0-9a-f]{16}$/.test(field[1]), '字段追踪格式无效');
    }
  }
  const different = [];
  for (let index = 0; index < Math.max(native.length, wasm.length); index++) {
    if (!native[index] || !wasm[index] || native[index][0] !== wasm[index][0] || native[index][1] !== wasm[index][1]) different.push(index);
  }
  return {nativeCount:native.length, wasmCount:wasm.length, different};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 4) throw Error('必须依次提供 Native 和 WASM 样本路径');
  const [native, wasm] = await Promise.all(process.argv.slice(2).map(async file => JSON.parse(await readFile(file, 'utf8'))));
  console.log(JSON.stringify(compareTraces(native.fieldTrace, wasm.result.result.fieldTrace)));
}
