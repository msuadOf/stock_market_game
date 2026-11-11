import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, readFile, readdir, rm, writeFile} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {validateRestore} from './restore-run.mjs';

function result() {
  const branch = () => ({startTick:1000,endTick:4000,restoredSaveEqual:true,restoredHashEqual:true,finalSaveEqual:true,finalHashEqual:true,firstEventDifference:null,firstLocalEventDifference:null,eventSteps:3000,firstScopedFactDifference:null,factSteps:3000,factCounts:{trades:0,lifecycle:1,deskReceipts:0,groupRejections:0,auction:0}});
  return {seed:42, executionOrder:'old-first', saveTick:1000, baselineStartTick:1000, baselineEndTick:4000, baselineRestoredEqualsInput:true, npcCount:20007, recomputedProbabilityDifferences:96,
    controlTraceEqual:true, control:branch(), candidate:branch(), saveBytes:100};
}
test('完整命中候选、恢复和继续推进一致时通过', () => { assert.doesNotThrow(() => validateRestore(result())); });
test('拒绝无候选差异、缺账户、事件或状态差异', () => {
  for (const mutate of [
    value => { value.npcCount--; }, value => { value.recomputedProbabilityDifferences = 0; },
    value => { value.baselineRestoredEqualsInput = false; }, value => { value.controlTraceEqual = false; }, value => { value.control.restoredSaveEqual = false; },
    value => { value.candidate.finalHashEqual = false; }, value => { value.candidate.firstEventDifference = {offset:0}; },
    value => { value.candidate.firstLocalEventDifference = {offset:0}; },
    value => { value.candidate.firstScopedFactDifference = {tick:1001}; },
    value => { value.control.factSteps--; },
    value => { delete value.candidate.factCounts.trades; },
    value => { value.candidate.factCounts.lifecycle = -1; },
    value => { value.control.eventSteps--; },
  ]) { const value = result(); mutate(value); assert.throws(() => validateRestore(value)); }
});

test('对照失败时保存差异证据并清理临时存档', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'restore-run-test-'));
  try {
    const child = path.join(root, 'probe.mjs'), setup = path.join(root, 'setup.json'), output = path.join(root, 'result.json');
    const value = result();
    value.saveBytes = 4;
    value.control.firstEventDifference = {offset:3,baselineUtf8:'[1]',otherUtf8:'[2]'};
    await writeFile(child, `#!${process.execPath}\nimport {writeFileSync} from 'node:fs';\nwriteFileSync(process.argv[3], 'save');\nconsole.log(${JSON.stringify(JSON.stringify(value))});\n`, {mode:0o700});
    await writeFile(setup, '{}');
    const runner = fileURLToPath(new URL('./restore-run.mjs', import.meta.url));
    const processResult = spawnSync(process.execPath, [runner, child, setup, output], {env:{...process.env,TMPDIR:root},encoding:'utf8',timeout:8000});
    assert.notEqual(processResult.status, 0);
    const failure = JSON.parse(await readFile(`${output}.failure`, 'utf8'));
    assert.equal(failure.observed.control.firstEventDifference.offset, 3);
    assert.match(failure.observed.control.firstEventDifference.baselineSha256, /^[0-9a-f]{64}$/);
    assert.equal((await readdir(root)).filter(name => name.startsWith('stock-libos-restore-')).length, 0);
  } finally { await rm(root, {recursive:true,force:true}); }
});

test('子进程失败与无效输出仍保存阶段和首错', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'restore-run-errors-'));
  try {
    const setup = path.join(root, 'setup.json');
    await writeFile(setup, '{}');
    for (const [name, source, phase, errorPattern] of [
      ['exit', "console.error('旧算法对照证据：保留');process.exit(7);", '运行恢复探针', /旧算法对照证据：保留/],
      ['json', "console.error('旧算法对照证据：保留');console.log('无效 JSON');", '解析并校验恢复结果', /JSON/],
      ['signal', "console.error('旧算法对照证据：保留');process.kill(process.pid, 'SIGTERM');", '运行恢复探针', /signal/],
    ]) {
      const child = path.join(root, `${name}.mjs`), output = path.join(root, `${name}.json`);
      await writeFile(child, `#!${process.execPath}\n${source}\n`, {mode:0o700});
      const runner = fileURLToPath(new URL('./restore-run.mjs', import.meta.url));
      const run = spawnSync(process.execPath, [runner, child, setup, output], {env:{...process.env,TMPDIR:root},encoding:'utf8',timeout:8000});
      assert.notEqual(run.status, 0);
      const failure = JSON.parse(await readFile(`${output}.failure`, 'utf8'));
      assert.equal(failure.phase, phase);
      assert.equal(failure.cleanupSucceeded, true);
      assert.equal(failure.observed.requestedOrder, 'old-first');
      assert.match(failure.error.message, errorPattern);
      assert.match(failure.childStderr, /旧算法对照证据：保留/);
      assert.equal(failure.childStderrTruncated, false);
      assert.match(failure.observed.binarySha256, /^[0-9a-f]{64}$/);
      assert.equal((await readdir(root)).filter(value => value.startsWith('stock-libos-restore-')).length, 0);
    }
  } finally { await rm(root, {recursive:true,force:true}); }
});
