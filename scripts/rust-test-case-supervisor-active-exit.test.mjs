import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { it } from "node:test";
import { executeSupervisedRustTestBinaries } from "./rust-test-case-supervisor.mjs";

async function setup(topic) {
  const root = path.resolve(".tmp/deadline-hardgate-tests");
  await mkdir(root, { recursive: true });
  const dir = await mkdtemp(path.join(root, `${topic}-`));
  return { dir, launcherPath: path.join(dir, "launcher.mjs") };
}

// 核销并行 WIP 的边界：launcher 在已知 case 活跃时以 0 退出，不能当作成功。
it("活跃 case 的 launcher 无结果正常退出时必须保留明确失败", { timeout: 10000 }, async () => {
  const { dir, launcherPath } = await setup("active-clean-exit");
  await writeFile(launcherPath, `const now=()=>Number(process.hrtime.bigint())/1e6;
process.on('message',m=>{if(m.kind==='initialize'){const started=now();process.send({kind:'arm',token:String(process.pid)+':active',started,total:started+2500,execution:started+1500,timeoutMs:2500,context:'active case'});}else if(m.kind==='armed')process.exit(0);});process.send({kind:'ready'});`);
  try {
    await assert.rejects(executeSupervisedRustTestBinaries({ artifacts: [], policy: {}, cwd: process.cwd(), env: process.env,
      remainingMs: () => 4000, launcherPath }), /launcher unexpectedly closed with 0/);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
