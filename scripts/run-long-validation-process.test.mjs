import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { existsSync, readFileSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { it } from "node:test";
import { fileURLToPath } from "node:url";

const fixturePreload = String.raw`import fs from "node:fs";
import { registerHooks, syncBuiltinESMExports } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

if (process.env.STOCK_GAME_FIXTURE_DEADLINE_MODE === "retained-child") {
  const target = pathToFileURL(path.join(process.env.STOCK_GAME_FIXTURE_DEADLINE_SOURCE, "scripts/run-with-deadline.mjs")).href;
  registerHooks({ load(url, context, nextLoad) {
    const loaded = nextLoad(url, context);
    if (url !== target) return loaded;
    const source = loaded.source.toString();
    const binding = "terminateProcessTree = terminateTree";
    if (source.split(binding).length !== 2) throw new Error("fixture 无法唯一绑定原 terminate hook 默认参数");
    // 只在 fixture 内存模块中模拟不收敛的 cleanup；真实 ChildProcess 与生产磁盘源码保持原样。
    return { ...loaded, source: source.replace(binding, "terminateProcessTree = () => new Promise(() => {})") };
  } });
} else if (process.env.STOCK_GAME_FIXTURE_DEADLINE_MODE === "slow-proc") {
  const original = fs.readFileSync;
  let delayed = 0;
  fs.readFileSync = (file, ...args) => {
    if (typeof file === "string" && /^\/proc\/\d+\/stat$/.test(file) && delayed++ < 16) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50);
    return original(file, ...args);
  };
  syncBuiltinESMExports();
} else throw new Error("fixture mode 必须明确指定");
`;

const sourceRoot = fileURLToPath(new URL("../", import.meta.url));
const now = () => Number(process.hrtime.bigint()) / 1e6;

function officialCliCase(mode) {
  it(`正式 long CLI 在 ${mode} 时仍必须真实退出且 owned child 不得越过 deadline`, { timeout: 10000, skip: process.platform !== "linux" }, async () => {
    const tmp = path.resolve(".tmp/all-deadline-overrun");
    await mkdir(tmp, { recursive: true });
    const dir = await mkdtemp(path.join(tmp, `official-${mode}-`));
    const childRecord = path.join(dir, "owned-child.json");
    const marker = path.join(dir, "late-marker");
    const childCode = `const fs=require('node:fs');const raw=fs.readFileSync('/proc/'+process.pid+'/stat','utf8');fs.writeFileSync(${JSON.stringify(childRecord)},JSON.stringify({pid:process.pid,birth:raw.slice(raw.lastIndexOf(')')+2).split(' ')[19]}));setTimeout(()=>fs.writeFileSync(${JSON.stringify(marker)},String(Number(process.hrtime.bigint())/1e6)),1800);setInterval(()=>{},1000);`;
    const preload = path.join(dir, "deadline-preload.mjs");
    await writeFile(preload, fixturePreload);
    const started = now();
    const worker = spawn(process.execPath, [path.join(sourceRoot, "scripts/run-long-validation.mjs"), "1600", "--", process.execPath, "-e", childCode], {
      env: { ...process.env, STOCK_GAME_FIXTURE_DEADLINE_MODE: mode, STOCK_GAME_FIXTURE_DEADLINE_SOURCE: sourceRoot,
        NODE_OPTIONS: `--import=${preload}` }, stdio: ["ignore", "pipe", "pipe"],
    });
    const closed = once(worker, "close");
    let stderr = "";
    worker.stderr.on("data", (chunk) => { stderr += chunk; }); worker.stdout.resume();
    try {
      let timer;
      const close = await Promise.race([closed, new Promise((resolve) => { timer = setTimeout(() => resolve(undefined), 2200); })]);
      clearTimeout(timer);
      const wallMs = now() - started;
      console.log(JSON.stringify({ mode, wall_ms: wallMs, close: close ?? "still-live", marker_exists: existsSync(marker), stderr }));
      assert.ok(existsSync(childRecord), "必须运行真实 child 后再验证 watchdog");
      assert.equal(existsSync(marker), false, "真实 owned child 不得越过 deadline 写 marker");
      assert.notEqual(close, undefined, "hard rejection 后 CLI 不能被真实 child 句柄继续保活");
      assert.ok(wallMs <= 1600, `真实 CLI wall ${wallMs}ms 超出 1600ms 总预算`);
      const owned = JSON.parse(readFileSync(childRecord, "utf8"));
      try {
        const stat = readFileSync(`/proc/${owned.pid}/stat`, "utf8"); const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
        assert.equal(fields[19], owned.birth, "正式 CLI 终态必须绑定实际 child birth");
        assert.match(fields[0], /^[ZX]$/, "不能让 finally cleanup 掩盖 CLI 退出时仍活的 owned root");
      } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
      assert.notEqual(close[0], 0, "timeout 或未确认清理必须明确非零退出");
      assert.match(stderr, /deadline|termination.*unconfirmed/);
    } finally {
      if (existsSync(childRecord)) {
        const owned = JSON.parse(readFileSync(childRecord, "utf8"));
        try {
          const stat = readFileSync(`/proc/${owned.pid}/stat`, "utf8"); const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
          assert.equal(fields[19], owned.birth, "清理前核对 fixture child birth");
          if (fields[0] !== "Z" && fields[0] !== "X") process.kill(owned.pid, "SIGKILL");
        } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
      }
      if (worker.exitCode === null && worker.signalCode === null) worker.kill("SIGKILL");
      await closed; await rm(dir, { recursive: true, force: true });
    }
  });
}

officialCliCase("retained-child");
officialCliCase("slow-proc");
