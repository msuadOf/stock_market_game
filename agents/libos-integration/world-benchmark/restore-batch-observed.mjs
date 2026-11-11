import {performance} from 'node:perf_hooks';
import {writeFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {runBoundedCommand} from '../../../scripts/run-with-deadline.mjs';

export async function executeBatch(run, now, deadline) {
  const results = [];
  for (const [index, order] of ['old-first','candidate-first','old-first'].entries()) {
    const remaining = Math.floor(deadline - now() - 1000);
    if (remaining < 2) throw Error('共享 deadline 已耗尽');
    const timeoutMs = Math.min(85000, remaining);
    try { await run({index:index + 1, order, timeoutMs}); results.push({index:index + 1, order, status:'passed'}); }
    catch (error) { results.push({index:index + 1, order, status:'failed', error:String(error).slice(0,2000)}); }
  }
  return results;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 5) throw Error('必须依次提供 Native 二进制、setup 和输出目录');
  const [binary, setup, directory] = process.argv.slice(2).map(value => path.resolve(value));
  const deadline = performance.now() + 280000;
  const results = await executeBatch(async ({index,order,timeoutMs}) => {
    const output = path.join(directory, `local-order-${index}.json`);
    await runBoundedCommand({command:process.execPath,args:[fileURLToPath(new URL('./restore-run.mjs', import.meta.url)),binary,setup,output,order],timeoutMs,captureOutput:true});
  }, () => performance.now(), deadline);
  await writeFile(path.join(directory,'batch-summary.json'), JSON.stringify({results}, null, 2), {flag:'wx'});
  console.log(JSON.stringify(results));
  if (results.some(item => item.status !== 'passed')) process.exitCode = 1;
}
