import {test} from 'node:test';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const dir = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(dir, 'acceleration-window.mjs');
const setup = path.join(dir, 'acceleration-setup.json');
const native = path.join(dir, 'subprofile-control-native-a.json');
const wasm = path.join(dir, 'subprofile-control-wasm-b.result.json');
const run = (s, n, w) => execFileSync(process.execPath, [script, s, n, w], {encoding:'utf8', timeout:5000, stdio:['ignore','pipe','pipe']});

test('真实字节与跨端样本可重复换算', {timeout:10000}, () => {
  const output = JSON.parse(run(setup, native, wasm));
  assert.equal(output.native.simulatedSeconds, 3000);
  assert.equal(output.wasm.simulatedSeconds, 3000);
  assert.ok(output.native.acceleration > 3700 && output.native.acceleration < 3800);
  assert.ok(output.wasm.acceleration > 2300 && output.wasm.acceleration < 2500);
});

test('拒绝 setup 字节变化、跨端不一致与异常采样窗口', {timeout:10000}, () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'libos-acceleration-'));
  try {
    const badSetup = path.join(temp, 'setup.json');
    writeFileSync(badSetup, readFileSync(setup, 'utf8') + '\n');
    assert.throws(() => run(badSetup, native, wasm));
    const changed = JSON.parse(readFileSync(wasm, 'utf8'));
    changed.result.result.businessHash = '1';
    const badWasm = path.join(temp, 'wasm.json');
    writeFileSync(badWasm, JSON.stringify(changed));
    assert.throws(() => run(setup, native, badWasm));
    const changedNative = JSON.parse(readFileSync(native, 'utf8'));
    const changedBrowser = JSON.parse(readFileSync(wasm, 'utf8'));
    changedNative.sample.from = 899;
    changedBrowser.result.result.sample.from = 899;
    const badNative = path.join(temp, 'native.json');
    writeFileSync(badNative, JSON.stringify(changedNative));
    writeFileSync(badWasm, JSON.stringify(changedBrowser));
    assert.throws(() => run(setup, badNative, badWasm));
  } finally { rmSync(temp, {recursive:true, force:true}); }
});
