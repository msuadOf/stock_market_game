import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { existsSync, readFileSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { it } from "node:test";
import { fileURLToPath } from "node:url";

const fixturePreload = String.raw`import fsp from "node:fs/promises";
import { registerHooks, syncBuiltinESMExports } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";
import fs from "node:fs";

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
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
  const original = fsp.readFile;
  let delayed = 0;
  // production processIdentity/linuxProcesses 使用 node:fs/promises 的 readFile；
  // 仅 patch node:fs 的同步 readFileSync 不会进入 cleanup 扫描路径。
  fsp.readFile = async (file, ...args) => {
    if (typeof file === "string" && /^\/proc\/\d+\/stat$/.test(file) && delayed < 16) {
      delayed += 1;
      fs.appendFileSync(process.env.STOCK_GAME_FIXTURE_PROC_HITS, JSON.stringify({ pid: process.pid, file, delayed, at: String(process.hrtime.bigint()), stack: new Error("slow-proc hit").stack }) + "\n");
      await delay(50);
    }
    return original(file, ...args);
  };
  syncBuiltinESMExports();
} else throw new Error("fixture mode 必须明确指定");
`;

const sourceRoot = fileURLToPath(new URL("../", import.meta.url));
const observerPath = path.join(sourceRoot, "scripts/run-long-validation-process-test-support.mjs");
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function withTimeout(promise, timeoutMs, message) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(message)), timeoutMs); })]);
  } finally {
    clearTimeout(timer);
  }
}

function officialCliCase(mode) {
  it(`正式 long CLI 在 ${mode} 时仍必须真实退出且 owned child 不得越过 deadline`, { timeout: 10000, skip: process.platform !== "linux" }, async () => {
    const tmp = path.resolve(".tmp/all-deadline-overrun");
    await mkdir(tmp, { recursive: true });
    const dir = await mkdtemp(path.join(tmp, `official-${mode}-`));
    const childRecord = path.join(dir, "owned-child.json");
    const marker = path.join(dir, "late-marker");
    const preload = path.join(dir, "deadline-preload.mjs");
    const ready = path.join(dir, "observer-ready");
    const go = path.join(dir, "observer-go");
    const reportPath = path.join(dir, "observer-report.json");
    await writeFile(preload, fixturePreload);

    const observer = spawn(process.execPath, [observerPath, mode, sourceRoot, dir], { stdio: ["ignore", "pipe", "pipe"] });
    const observerClosed = once(observer, "close");
    let observerStderr = "";
    observer.stderr.on("data", (chunk) => { observerStderr += chunk; });
    observer.stdout.resume();
    try {
      let waited = 0;
      while (!existsSync(ready) && waited < 3000) { await delay(5); waited += 5; }
      assert.ok(existsSync(ready), `独立观察器未在 3000ms 内 ready：${observerStderr}`);
      await writeFile(go, "go");
      await withTimeout(observerClosed, 6000, `独立观察器未在 6000ms 内完成：${observerStderr}`);
      assert.ok(existsSync(reportPath), `独立观察器缺少报告：${observerStderr}`);
      const report = JSON.parse(await readFile(reportPath, "utf8"));
      console.log(JSON.stringify({ mode, ...report }));
      assert.equal(report.error, undefined, `独立观察器失败：${report.error}`);
      assert.ok(existsSync(childRecord), "必须运行真实 child 后再验证 watchdog");
      const owned = JSON.parse(readFileSync(childRecord, "utf8"));
      assert.equal(report.owned.birth, owned.birth, "观察器绑定的 owned child birth 必须与真实记录一致");
      assert.equal(report.markerExists, false, "真实 owned child 不得越过 deadline 写 marker");
      assert.equal(report.timedOut, false, "独立观察器不得靠自身超时清场冒充 CLI 截止");
      assert.ok(BigInt(report.readyAt) < BigInt(report.started), "observer 必须先 ready 再记录 spawn 前起点");
      for (const [label, terminal] of [["owned child", report.ownedTerminal], ["CLI", report.cliTerminal]]) {
        assert.ok(["Z", "X", "disappeared"].includes(terminal?.kind), `${label} 必须出现可独立观察的终态，实际 ${terminal?.kind}`);
        assert.equal(typeof terminal.birth, "string", `${label} 终态必须绑定已观察到的 PID birth`);
        assert.notEqual(terminal.seen, false, `${label} 必须在终态前被 observer 实际观察到`);
      }
      assert.equal(report.ownedTerminal.birth, report.owned.birth, "owned child 终态 birth 必须与真实记录一致");
      assert.equal(report.cliTerminal.birth, report.cliBirth, "CLI 终态 birth 必须与 spawn 后立即绑定的 CLI birth 一致");
      assert.ok(report.ownedTerminalMs <= 1600, `真实 owned child 终态 ${report.ownedTerminalMs}ms 超出 1600ms 总预算`);
      assert.ok(report.cliTerminalMs <= 1600, `真实 CLI 终态 ${report.cliTerminalMs}ms 超出 1600ms 总预算`);
      assert.notEqual(report.exitCode, 0, "timeout 或未确认清理必须明确非零退出");
      assert.match(report.stderr, /deadline|termination.*unconfirmed/);
      if (mode === "slow-proc") {
        assert.ok(report.hits.length > 0, "slow-proc 注入必须命中 production 的 fs/promises cleanup 扫描");
        assert.ok(report.hits.every((entry) => entry.pid === report.cliPid), "slow-proc 证据必须来自被测 CLI 进程");
        const totalMs = Number(/total (\d+)ms deadline/.exec(report.stderr)?.[1]);
        const cleanupReserveMs = Number(/(\d+)ms cleanup reserve/.exec(report.stderr)?.[1]);
        assert.ok(Number.isInteger(totalMs) && Number.isInteger(cleanupReserveMs), "必须从 CLI 错误中取得真实 total/cleanup reserve");
        const executionCutoffMs = totalMs - cleanupReserveMs;
        const hitOffsetsMs = report.hits.map((entry) => Number(BigInt(entry.at) - BigInt(report.started)) / 1e6);
        assert.ok(hitOffsetsMs.some((offset) => offset >= executionCutoffMs), `slow-proc 必须在 execution cutoff ${executionCutoffMs}ms 后的 cleanup 阶段命中`);
        assert.ok(report.hits.some((entry) => /run-with-deadline\.mjs/.test(entry.stack) && /(?:terminateTree|tracedChildren|linuxProcesses)/.test(entry.stack)), "slow-proc 必须命中 production terminateTree 清理调用链");
      } else {
        assert.equal(report.hits.length, 0, "retained-child 不应命中 slow-proc 注入");
      }
    } finally {
      observer.kill("SIGKILL");
      await observerClosed;
      if (existsSync(childRecord)) {
        const owned = JSON.parse(readFileSync(childRecord, "utf8"));
        try {
          const stat = readFileSync(`/proc/${owned.pid}/stat`, "utf8"); const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
          assert.equal(fields[19], owned.birth, "清理前核对 fixture child birth");
          if (fields[0] !== "Z" && fields[0] !== "X") process.kill(owned.pid, "SIGKILL");
        } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
      }
      await rm(dir, { recursive: true, force: true });
    }
  });
}

officialCliCase("retained-child");
officialCliCase("slow-proc");
