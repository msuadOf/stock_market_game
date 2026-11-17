#!/usr/bin/env node
// 必须在run-long-validation的整进程树300秒监督内调用；本驱动只用新编译验证包。
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { captureWasmBuildSourceIdentity, wasmBuildSourceSha256 } from "./wasm-package-capability.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";
import { authorityProductSourceIdentity } from "./authority-product-build.mjs";
import { PRODUCT_EXTENSIONS, validateProductContractEvidence } from "./session-authority-product.mjs";
import { AUTHORITY_SCENARIO, validateAuthorityTrace } from "./session-authority-trace.mjs";
const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
export function requireAuthorityVerificationExports(exports) {
  for (const name of ["market_runner_contract_setup", "market_runner_contract_snapshot", "market_runner_contract_fail", "market_runner_contract_queue_full"]) {
    assert.ok(exports.includes(name), `需要verification-harness独立产物：缺少${name}`);
  }
}

export async function runSessionAuthorityBrowser({ sourceRoot = root, browserExecutable, completeProduct = false }) {
  const deadline = Number(process.env.AUTHORITY_PRODUCT_DEADLINE);
  assert.ok(process.env.AUTHORITY_PRODUCT_SUPERVISOR === String(process.ppid) && deadline > Date.now() && deadline - Date.now() <= 300000, "Browser产品driver必须由其300秒进程树supervisor启动");
  const frontendSource = await authorityProductSourceIdentity(sourceRoot);
  const frontendSourceSha256 = hash(JSON.stringify(frontendSource));
  const web = path.join(sourceRoot, "apps/web");
  const packagePath = path.join(web, "wasm-diagnostics-pkg");
  const [source, metadata, wasm, glue] = await Promise.all([
    captureWasmBuildSourceIdentity(sourceRoot), readFile(path.join(packagePath, "wasm-verification-receipt.json"), "utf8").then(JSON.parse),
    readFile(path.join(packagePath, "web_wasm_bg.wasm")), readFile(path.join(packagePath, "web_wasm.js")),
  ]);
  const sourceSha256 = wasmBuildSourceSha256(source), artifactSha256 = hash(wasm), glueSha256 = hash(glue);
  assert.equal(metadata.kind, "wasm-verification-build", "缺少独立验证构建receipt");
  assert.equal(metadata.sourceSha256, sourceSha256, "验证WASM不是当前源码编译");
  assert.equal(metadata.wasmSha256, artifactSha256, "验证WASM指纹不符");
  assert.equal(metadata.glueSha256, glueSha256, "验证glue指纹不符");
  const module = await WebAssembly.compile(wasm);
  requireAuthorityVerificationExports(WebAssembly.Module.exports(module).map(value => value.name));
  const { createServer } = await import(pathToFileURL(path.join(web, "node_modules/vite/dist/node/index.js")).href);
  const { chromium } = await import(pathToFileURL(path.join(web, "node_modules/@playwright/test/index.mjs")).href);
  let vite, browser, primary;
  try {
    vite = await createServer({ root: web, mode: "e2e", appType: "custom", define: {
      "import.meta.env.VITE_ENGINE_DIAGNOSTICS": JSON.stringify("1"),
      "import.meta.env.VITE_SESSION_AUTHORITY_CONTRACT": JSON.stringify("1"),
    }, server: { host: "127.0.0.1", port: 0, fs: { allow: [sourceRoot] } } });
    vite.middlewares.use((request, response, next) => {
      if (request.url !== "/session-authority-contract") return next();
      response.setHeader("Content-Type", "text/html");
      response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
      response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
      response.end("<!doctype html><title>共享会话真实Browser合同</title>");
    });
    await vite.listen();
    browser = await chromium.launch({ headless: true, executablePath: browserExecutable });
    const page = await browser.newPage();
    const errors = [];
    page.on("pageerror", error => errors.push(String(error)));
    const address = vite.resolvedUrls?.local[0];
    assert.ok(address, "Vite未分配回环地址");
    await page.goto(new URL("/session-authority-contract", address).href);
    const evidence = await page.evaluate(async () => {
      if (!crossOriginIsolated) throw new Error("真实共享WASM缺少跨源隔离");
      const { DEFAULT_SETUP } = await import("/src/config/defaults.ts");
      const { createWorkerHost } = await import("/src/host/worker-host.ts");
      const lifecycle = [];
      let spawnFailure;
      const host = await createWorkerHost(DEFAULT_SETUP, 7n, {
        enableE2EStepping: true,
        lifecycleObserver(value) { lifecycle.push(value); },
        onSpawnFailure(value) { spawnFailure = value; },
      });
      const describe = (error, depth = 0) => {
        if (depth > 4) return "错误嵌套过深";
        const parts = [error instanceof Error ? error.message : String(error)];
        if (error instanceof AggregateError) for (const child of error.errors) parts.push(describe(child, depth + 1));
        if (error instanceof Error && error.cause !== undefined) parts.push(describe(error.cause, depth + 1));
        return parts.join("；");
      };
      let primary;
      try {
        if (typeof host.authorityContractForE2E !== "function") throw new Error("host缺少真实合同驱动");
        const rows = await host.authorityContractForE2E();
        if (spawnFailure !== undefined) throw new Error(`真实Worker启动失败：${JSON.stringify(spawnFailure)}`);
        return { rows, lifecycle };
      } catch (error) { primary = error; throw error; }
      finally {
        try { await host.dispose(); }
        catch (cleanup) {
          throw new Error(`${primary === undefined ? "" : `合同失败：${describe(primary)}；`}清理失败：${describe(cleanup)}`);
        }
      }
    });
    assert.deepEqual(errors, [], "真实Browser发生未处理异常");
    const trace = validateAuthorityTrace({ schema: 1, host: "chromium-shared-wasm", execution: "real", sourceSha256, artifactSha256,
      scenario: AUTHORITY_SCENARIO.map(({ id }) => id), rows: evidence.rows });
    const extensions = [];
    if (completeProduct) {
      for (const id of PRODUCT_EXTENSIONS) {
        if (id === "multi-observer-stalled") {
          extensions.push({id,status:"not-applicable",mechanism:"single-outbound-consumer",reason:"当前Browser会话只有一个有信用的Outbound消费端，多Worker listener不等于独立观察者"}); continue;
        }
        const extensionPage = await browser.newPage();
        let extensionFailure;
        try {
          await extensionPage.goto(new URL("/session-authority-contract", address).href);
          const result = await extensionPage.evaluate(async id => {
            const { DEFAULT_SETUP } = await import("/src/config/defaults.ts");
            const { createWorkerHost } = await import("/src/host/worker-host.ts");
            const lifecycle = [];
            const host = await createWorkerHost(DEFAULT_SETUP, 7n, { enableE2EStepping:true,
              lifecycleObserver:value=>lifecycle.push(value), onSpawnFailure:value=>lifecycle.push({spawnFailure:value}) });
            let result, primary, cleanup;
            try { result = await host.authorityContractExtensionForE2E(id); }
            catch(error) {primary=error;}
            try {await host.dispose();} catch(error) {cleanup={message:String(error),cause:error?.cause};}
            if(result?.transportStageRequired && !(result.clientEvidence?.channelDetached===true && result.clientEvidence?.channelRebound===true && result.clientEvidence?.stalePacketRejected===true)) throw new Error("Browser断线扩展缺少actualworker-host传输阶段证据，不能只以Rusthold/resume通过");
            if(primary) throw new Error(`${String(primary)}${cleanup ? `；清理:${JSON.stringify(cleanup)}` : ""}`);
            const knownNondurable = id === "shutdown-known-archive-failure-cleanup" && cleanup?.cause?.archiveFailure?.kind === "nondurable" && cleanup.cause.archiveFailure.workerQuiescence === "confirmed" && cleanup.cause.ownerConsumed === true && cleanup.cause.memoryReusable === true && ["released", "not-allocated"].includes(cleanup.cause.release?.kind) && cleanup.cause.shutdownReport != null;
            if(id === "shutdown-known-archive-failure-cleanup" && !knownNondurable) throw new Error(`已知非耐久失败未显式保留正确清理证据:${JSON.stringify({result,cleanup,lifecycle})}`);
            if(id === "shutdown-unknown-worker-exit-no-reuse" && (!cleanup || cleanup.cause?.quiescence !== "unknown" || cleanup.cause?.memoryReusable !== false || result?.assertions?.quiescenceUnknown !== true || result?.assertions?.reuseRejected !== true)) throw new Error(`未知Worker退出不能报告成功清理或复用:${JSON.stringify({result,cleanup,lifecycle})}`);
            if(cleanup && !knownNondurable && !(id==="shutdown-unknown-worker-exit-no-reuse" && result?.assertions?.quiescenceUnknown===true && result?.assertions?.reuseRejected===true)) throw new Error(cleanup.message);
            return {result,lifecycle,...(cleanup ? (knownNondurable ? {expectedNondurableCleanup:cleanup} : {expectedQuiescenceUnknown:cleanup}) : {})};
          }, id);
          extensions.push({id,status:"passed",evidence:result});
        } catch(error) {extensionFailure=error;throw error;}
        finally {try {await extensionPage.close();} catch(error) {throw extensionFailure ? new AggregateError([extensionFailure,error], "Browser扩展失败且页面清理失败") : error;}}
      }
    }
    assert.deepEqual(await authorityProductSourceIdentity(sourceRoot), frontendSource, "真实Browser执行期间前端源码闭包发生改变");
    const result = { ...trace, frontendSourceSha256, frontendSource, ...(completeProduct ? {kind:"authority-product-contract/v1",extensions,
      productTransport:{kind:"real-chromium-shared-wasm-messageport",events:evidence.rows.flatMap(row=>row.productPackets ?? [])}} : {}), glueSha256, buildReceipt: metadata, browserVersion: browser.version(), artifactKind: "verification-harness-diagnostics",
      storage: "真实IndexedDB Worker写入与Rust接纳收据", publication: "真实Outbound与Browser MessagePort；慢观察者停ACK，前缀排空恢复接纳",
      queueFullEvidence: "实际ArchivePort入队位置注入拒绝，不是物理容量测量", lifecycle: evidence.lifecycle };
    return completeProduct ? validateProductContractEvidence(result) : result;
  } catch (error) { primary = error; throw error; }
  finally {
    const cleanup = [];
    try { await browser?.close(); } catch (error) { cleanup.push(error); }
    try { await vite?.close(); } catch (error) { cleanup.push(error); }
    if (cleanup.length) throw new AggregateError(primary === undefined ? cleanup : [primary, ...cleanup], "浏览器验收清理失败；保留主错及全部清理错误");
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.env.AUTHORITY_PRODUCT_SUPERVISOR !== String(process.ppid)) {
    await runBoundedCommand({ command: process.execPath, args: process.argv.slice(1), timeoutMs: 300000,
      env: { ...process.env, AUTHORITY_PRODUCT_SUPERVISOR: String(process.pid), AUTHORITY_PRODUCT_DEADLINE: String(Date.now() + 299000) } });
  } else {
  const args = new Map();
  for (let i = 2; i < process.argv.length; i += 2) {
    assert.ok(["--output", "--browser", "--complete-product"].includes(process.argv[i]) && !args.has(process.argv[i]) && process.argv[i + 1], "用法：--output <新绝对JSON路径> [--browser <绝对Chromium路径>]");
    args.set(process.argv[i], process.argv[i + 1]);
  }
  assert.ok(path.isAbsolute(args.get("--output") ?? ""), "输出必须是新绝对路径");
  if (args.has("--browser")) assert.ok(path.isAbsolute(args.get("--browser")), "浏览器路径必须绝对");
  const result = await runSessionAuthorityBrowser({ browserExecutable: args.get("--browser"), completeProduct: args.get("--complete-product") === "true" });
  await writeFile(args.get("--output"), JSON.stringify(result, null, 2) + "\n", { flag: "wx" });
  }
}
