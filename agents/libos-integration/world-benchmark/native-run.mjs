import {createHash} from 'node:crypto';
import {readFile, writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import {runBoundedCommand} from '../../../scripts/run-with-deadline.mjs';
import {validateRequestedProfile} from './step-profile.mjs';

const sha256 = data => createHash('sha256').update(data).digest('hex');

export async function collectNative(binary, setup, {read, run}) {
  const setupSha256 = sha256(await read(setup));
  const binarySha256 = sha256(await read(binary));
  const stdout = await run(binary, setup);
  if (sha256(await read(setup)) !== setupSha256 || sha256(await read(binary)) !== binarySha256) {
    throw Error('Native 输入或二进制在执行期间发生变化');
  }
  const result = JSON.parse(stdout);
  if (!result || typeof result !== 'object' || Array.isArray(result)) throw Error('Native 输出不是对象');
  if (Object.hasOwn(result, 'setupSha256') || Object.hasOwn(result, 'binarySha256')) throw Error('Native 输出包含冲突的摘要字段');
  return {...result, setupSha256, binarySha256};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 5) throw Error('必须依次提供 Native 二进制、setup 和输出路径');
  const [binary, setup, output] = process.argv.slice(2).map(value => path.resolve(value));
  const result = await collectNative(binary, setup, {
    read: readFile,
    run: async (executable, input) => (await runBoundedCommand({command:executable,args:[input,'3000'],timeoutMs:120000,captureOutput:true})).stdout,
  });
  validateRequestedProfile(result, process.env.LIBOS_DIAGNOSTIC_PROFILE ?? '0');
  await writeFile(output, JSON.stringify(result, null, 2), {flag:'wx'});
  console.log(JSON.stringify(result));
}
