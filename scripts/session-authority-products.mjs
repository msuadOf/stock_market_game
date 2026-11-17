#!/usr/bin/env node
// 只连接明确PID的本轮验证产品；运行必须放在外层300秒进程树监督中。
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, readlink, realpath, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { authorityProductSourceIdentity, authorityFrontendDistHash } from "./authority-product-build.mjs";
import { createNativeAuthorityProductPort } from "./fixtures/authority-native-product-port.mjs";
import { createTauriAuthorityProductPort } from "./fixtures/authority-tauri-product-port.mjs";
import { executeProductContract, validateProductContractEvidence } from "./session-authority-product.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";
import { AUTHORITY_SCENARIO } from "./session-authority-trace.mjs";
import { connectWebKitInspector, waitFor } from "../agents/shared-market-runner/libos-desktop-fixture/webkit-inspector.mjs";
const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const sha = bytes => createHash("sha256").update(bytes).digest("hex");

export async function runAuthorityNativeProduct({ baseUrl, browserExecutable, evaluate }) {
  // evaluate由真实Chromium page提供；工厂在页面内使用真实fetch/WebSocket。
  await evaluate(`globalThis.__authorityProductPort = (${createNativeAuthorityProductPort.toString()})(${JSON.stringify(baseUrl)}); true`);
}

export async function runAuthorityProduct({ host, pid, artifact, receiptFile, baseUrl, inspectorPort, browserExecutable }) {
  const deadline = Number(process.env.AUTHORITY_PRODUCT_DEADLINE);
  assert.ok(Number.isFinite(deadline) && deadline > Date.now() && deadline - Date.now() <= 300000 && process.env.AUTHORITY_PRODUCT_SUPERVISOR === String(process.ppid), "产品driver必须由其真实300秒supervisor启动");
  assert.ok(["native", "linux-tauri"].includes(host), "此驱动只负责实际Server/WS和Linux Tauri窗口");
  assert.ok(Number.isInteger(pid) && pid > 0, "必须提供真实产品PID");
  const live = await readlink(`/proc/${pid}/exe`);
  assert.equal(path.resolve(live), path.resolve(artifact), "指定制品不是目标PID正在运行的二进制");
  const [binary, receipt] = await Promise.all([readFile(artifact), readFile(receiptFile, "utf8").then(JSON.parse)]);
  const artifactSha256 = sha(binary);
  assert.equal(receipt.kind, "authority-product-build", "缺少验证产品构建receipt");
  assert.equal(receipt.artifactSha256, artifactSha256, "运行二进制与构建receipt不符");
  assert.match(receipt.sourceSha256, /^[a-f0-9]{64}$/, "receipt缺少源码闭包指纹");
  assert.equal(receipt.verification, true, "完整故障脚本不能连接默认分发构建");
  assert.equal(receipt.sourceSha256, sha(JSON.stringify(await authorityProductSourceIdentity(root))), "运行产品不是当前源码闭包");
  assert.equal(receipt.frontend?.receipt?.kind, "authority-frontend-build", "缺少实际前端构建receipt");
  assert.equal(receipt.frontend.receipt.sourceSha256, receipt.sourceSha256, "前端与Native源码不一致");
  assert.equal(receipt.frontend.receipt.distSha256, await authorityFrontendDistHash(receipt.frontend.dist), "实际前端dist已改变");
  assert.equal(receipt.frontend.config, "apps/desktop/src-tauri/tauri.conf.json", "receipt缺少真实Tauri前端路径绑定");
  const configPath = path.join(root, receipt.frontend.config);
  const config = JSON.parse(await readFile(configPath, "utf8"));
  assert.equal(await realpath(receipt.frontend.dist), await realpath(path.resolve(path.dirname(configPath), config.build.frontendDist)), "产品前端receipt与实际Tauri配置路径不同");
  let browser, vite, inspector, evaluate, primary;
  try {
    if (host === "native") {
      const web = path.join(root, "apps/web");
      const { createServer } = await import(pathToFileURL(path.join(web, "node_modules/vite/dist/node/index.js")));
      const { chromium } = await import(pathToFileURL(path.join(web, "node_modules/@playwright/test/index.mjs")));
      vite = await createServer({ root: web, mode: "e2e", appType: "custom", server: { host: "127.0.0.1", port: 0 } });
      vite.middlewares.use((request, response, next) => {
        if (request.url !== "/authority-product") return next();
        response.setHeader("Content-Type", "text/html"); response.end("<!doctype html><title>真实Server WS故障合同</title>");
      });
      await vite.listen();
      browser = await chromium.launch({ headless: true, executablePath: browserExecutable });
      const page = await browser.newPage();
      await page.goto(new URL("/authority-product", vite.resolvedUrls.local[0]).href);
      evaluate = expression => page.evaluate(expression);
      await runAuthorityNativeProduct({ baseUrl, browserExecutable, evaluate });
    } else {
      inspector = await connectWebKitInspector(inspectorPort);
      evaluate = expression => inspector.evaluate(expression);
      await evaluate(`globalThis.__authorityProductPort = (${createTauriAuthorityProductPort.toString()})(); true`);
    }
    // WebKit Runtime.evaluate不等待Promise；通过真实页面结果槽等待，不伪造同步返回。
    let serial = 0;
    const call = async (method, args = []) => {
      const key = `authority-product-${++serial}`;
      await evaluate(`globalThis.__authorityProductReplies ??= {}; Promise.resolve().then(() => globalThis.__authorityProductPort[${JSON.stringify(method)}](...${JSON.stringify(args)})).then(value => { globalThis.__authorityProductReplies[${JSON.stringify(key)}] = {ok:true,value}; }, error => { globalThis.__authorityProductReplies[${JSON.stringify(key)}] = {ok:false,error:String(error?.stack ?? error)}; }); true`);
      const result = await waitFor(method, () => evaluate(`globalThis.__authorityProductReplies[${JSON.stringify(key)}] ?? false`), Math.max(1, Math.min(10000, deadline - Date.now())));
      await evaluate(`delete globalThis.__authorityProductReplies[${JSON.stringify(key)}]; true`);
      if (!result.ok) throw new Error(result.error);
      return result.value;
    };
    const proxy = {
      host, capabilities: () => call("capabilities"), create: (...args) => call("create", args),
      observeBefore: id => call("observeBefore", [id]), action: id => call("action", [id]),
      observeAfter: id => call("observeAfter", [id]), snapshot: id => call("snapshot", [id]),
      close: () => call("close"), extension: id => call("extension", [id]),
      async fork() {
        await evaluate(`globalThis.__authorityProductPort = (${host === "native" ? createNativeAuthorityProductPort.toString() : createTauriAuthorityProductPort.toString()})(${host === "native" ? JSON.stringify(baseUrl) : ""}); true`);
        return proxy;
      },
      transportEvidence() { return transport; },
      cleanupEvidence: () => call("transportEvidence"),
    };
    // 保存主17步完整收包证据，后续独立会话的证据分别保留在extensions。
    let transport;
    const close = proxy.close;
    proxy.close = async () => { transport ??= await call("transportEvidence"); await close(); };
    const result = await executeProductContract(proxy);
    return validateProductContractEvidence({ schema: 1, kind: "authority-product-contract/v1", execution: "real", host,
      scenario: AUTHORITY_SCENARIO.map(row => row.id), sourceSha256: receipt.sourceSha256, artifactSha256,
      productPid: pid, buildReceipt: receipt, ...(browser ? { browserVersion: browser.version() } : { window: "actual-linux-tauri-webview" }), ...result });
  } catch (error) { primary = error; throw error; }
  finally {
    const errors = [];
    for (const close of [() => inspector?.close(), () => browser?.close(), () => vite?.close()]) {
      try { await close(); } catch (error) { errors.push(error); }
    }
    if (errors.length) throw new AggregateError(primary ? [primary, ...errors] : errors, "完整产品合同清理失败");
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.env.AUTHORITY_PRODUCT_SUPERVISOR !== String(process.ppid)) {
    await runBoundedCommand({ command: process.execPath, args: process.argv.slice(1), timeoutMs: 300000,
      env: { ...process.env, AUTHORITY_PRODUCT_SUPERVISOR: String(process.pid), AUTHORITY_PRODUCT_DEADLINE: String(Date.now() + 299000) } });
  } else {
  const args = new Map();
  for (let index = 2; index < process.argv.length; index += 2) {
    assert.ok(["--host", "--pid", "--artifact", "--receipt", "--base-url", "--inspector-port", "--browser", "--output"].includes(process.argv[index]) && process.argv[index + 1] && !args.has(process.argv[index]), "未知或重复产品合同参数");
    args.set(process.argv[index], process.argv[index + 1]);
  }
  assert.ok(path.isAbsolute(args.get("--output") ?? ""), "必须指定新的绝对输出路径");
  const result = await runAuthorityProduct({ host: args.get("--host"), pid: Number(args.get("--pid")), artifact: args.get("--artifact"), receiptFile: args.get("--receipt"), baseUrl: args.get("--base-url"), inspectorPort: Number(args.get("--inspector-port")), browserExecutable: args.get("--browser") });
  await writeFile(args.get("--output"), JSON.stringify(result, null, 2) + "\n", { flag: "wx" });
  }
}
