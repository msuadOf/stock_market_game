// V11 tests-first 验收源码：仅由统一300秒批次在10000ms进程树硬deadline内调用。
// 保真实浏览器原件；独立业务oracle与native比较必须在保存原件之后运行。
import assert from "node:assert/strict";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { readFileSync, realpathSync, writeFileSync, lstatSync } from "node:fs";
import { createHash } from "node:crypto";
import { captureResponse, errorRecord, createWasmResponseEvidence, createGlueResponseEvidence } from "./browser-response-tasks.mjs";
import { createProductResponseBinding } from "./product-response-binding.mjs";

const [artifactArg, fixtureArg, scriptArg, outputArg] = process.argv.slice(2);
assert.equal(process.argv.length, 6, "必须提供artifact、同字节fixture、同字节script、独占output目录");
const root = realpathSync(process.cwd());
const artifact = realpathSync(artifactArg), output = realpathSync(outputArg);
const fixture = readFileSync(fixtureArg), script = readFileSync(scriptArg);
const seal = JSON.parse(readFileSync(path.join(output, "browser-input-binding.json"), "utf8"));
const sourceBefore = JSON.parse(readFileSync(path.join(output, "source-before.json"), "utf8"));
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
for (const [name, bytes] of [["fixture", fixture], ["script", script]]) {
  assert.equal(bytes.length, seal[name].bytes);assert.equal(sha(bytes), seal[name].sha256);
}
assert.equal(seal.productRoot, root);assert.equal(seal.artifactRoot, artifact);
function verifyGlueSource() {
  const file = path.join(artifact, "web_wasm.js");
  const stat = lstatSync(file);assert(stat.isFile() && !stat.isSymbolicLink());assert.equal(realpathSync(file), file);
  const bytes = readFileSync(file);assert.equal(bytes.length, seal.glue.bytes);assert.equal(sha(bytes), seal.glue.sha256);
  return { file, bytes: bytes.length, sha256: sha(bytes), dev: stat.dev, ino: stat.ino };
}
const artifactGlueSourceBefore = verifyGlueSource();
const { createServer } = await import(pathToFileURL(path.join(root, "apps/web/node_modules/vite/dist/node/index.js")));
const { chromium } = await import(pathToFileURL(path.join(root, "apps/web/node_modules/@playwright/test/index.mjs")));
const productResponses = createProductResponseBinding(root, output, "business");
const glueEvidence = createGlueResponseEvidence();
const wasmEvidence = createWasmResponseEvidence(artifact, seal.wasm);
const logs = [], served = [], responseTasks = [], workerLifecycle = [], driverResponses = [];
const activeWorkers = new Set();
const driverRoot = "agents/branch-completion/mixed-calendar-c2c/v11-business-source-33/";
const inputResponses = [];
const server = await createServer({ configFile: false, root, logLevel: "warn",
  cacheDir: path.join(output, "vite-cache"),
  resolve: { alias: [
    { find: /^env$/, replacement: path.join(root, "apps/web/src/host/engine-clock-env.ts") },
    { find: "../../wasm-pkg/web_wasm.js", replacement: path.join(artifact, "web_wasm.js") },
  ] },
  server: { host: "127.0.0.1", port: 0, fs: { allow: [root, artifact] } },
  plugins: [{ name: "c2c-business-real-isolated-source", configureServer(vite) {
    vite.middlewares.use((req, res, next) => {
      res.setHeader("Cross-Origin-Opener-Policy", "same-origin");
      res.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
      res.setHeader("Cache-Control", "no-store");
      glueEvidence.observe(req, res);
      if (wasmEvidence.handle(req, res)) return;
      if (req.url === "/__c2c_business") {
        res.setHeader("Content-Type", "text/html");
        res.end(`<!doctype html><meta charset="utf-8"><script type="module" src="/${driverRoot}browser-business-main.ts"></script>`);
        return;
      }
      const bytes = req.url === "/__c2c_fixture.json" ? fixture : req.url === "/__c2c_script.json" ? script : undefined;
      if (bytes !== undefined) {
        assert.equal(req.method, "GET");assert.equal(req.headers.range, undefined);
        const record = { url: req.url, bytes: bytes.length, sha256: sha(bytes), finished: false };
        inputResponses.push(record);res.on("finish", () => { record.finished = true; });
        res.setHeader("Content-Type", "application/json");res.setHeader("Content-Length", String(bytes.length));
        res.end(bytes);return;
      }
      if (req.url?.startsWith("/wasm-diagnostics-pkg/")) req.url = `/@fs/${path.join(artifact, req.url.slice("/wasm-diagnostics-pkg/".length))}`;
      next();
    });
  } }],
});
let browser, rawResult, scenarioError;
try {
  await server.listen();assert.equal(server.config.root, root);
  const address = server.httpServer.address();assert(address !== null && typeof address !== "string");
  browser = await chromium.launch({ headless: true, args: ["--disable-dev-shm-usage"] });
  const page = await browser.newPage();
  page.on("response", (response) => {
    const product = productResponses.capture(response);if (product !== undefined) responseTasks.push(product);
    const pathname = new URL(response.url()).pathname;
    if (/^\/wasm-diagnostics-pkg\/web_wasm(?:_bg\.wasm|\.js)$/.test(pathname)) {
      responseTasks.push(captureResponse(response, served, wasmEvidence, {}, glueEvidence));
    } else if (pathname.startsWith(`/${driverRoot}`)) {
      responseTasks.push((async () => {
        const relative = pathname.slice(1), file = path.join(root, relative);
        assert.equal(realpathSync(file), file);const source = readFileSync(file);
        assert.equal(sourceBefore[relative].bytes, source.length);assert.equal(sourceBefore[relative].sha256, sha(source));
        assert.equal(response.status(), 200);assert.equal(await response.finished(), null);
        const body = await response.body();driverResponses.push({ url: response.url(), relative,
          sourceBytes: source.length, sourceSha256: sha(source), transformedBytes: body.length, transformedSha256: sha(body) });
      })().then(() => ({ status: "fulfilled" }), (reason) => ({ status: "rejected", reason, context: { url: response.url() } })));
    }
  });
  page.on("worker", (worker) => {
    activeWorkers.add(worker);workerLifecycle.push({ event: "created", url: worker.url() });
    worker.on("close", () => { activeWorkers.delete(worker);workerLifecycle.push({ event: "closed", url: worker.url() }); });
  });
  page.on("console", (message) => logs.push({ type: "console", level: message.type(), text: message.text() }));
  page.on("pageerror", (error) => logs.push({ type: "pageerror", error: errorRecord(error) }));
  page.on("requestfailed", (request) => logs.push({ type: "requestfailed", url: request.url(), failure: request.failure() }));
  await page.goto(`http://127.0.0.1:${address.port}/__c2c_business`);
  await page.waitForFunction(() => window.c2cBusinessResult !== undefined, undefined, { timeout: 8000 });
  rawResult = await page.evaluate(() => window.c2cBusinessResult);
  // 即便结果业务失败也先保存原件，再由finally收束取证；不得用oracle错误覆盖浏览器原输出。
  writeFileSync(path.join(output, "browser-business-raw.json"), JSON.stringify(rawResult, null, 2), { flag: "wx" });
  const closeDeadline = Date.now() + 250;
  while (activeWorkers.size > 0 && Date.now() < closeDeadline) await page.waitForTimeout(10);
  assert.equal(activeWorkers.size, 0, "真实DedicatedWorker应由产品driver的finally完成terminate");
  const created = workerLifecycle.filter((row) => row.event === "created");
  const closed = workerLifecycle.filter((row) => row.event === "closed");
  assert.equal(created.length, 1, "必须真实观察到唯一engine DedicatedWorker创建，空集不能通过");
  assert.equal(closed.length, 1, "必须真实观察到同一engine DedicatedWorker关闭");
  assert.equal(created[0].url, closed[0].url);
  const workerUrl = new URL(created[0].url);
  assert.equal(workerUrl.origin, `http://127.0.0.1:${address.port}`);
  assert.equal(workerUrl.pathname, `/${driverRoot}browser-business-engine.ts`);
  assert.equal(rawResult.ok, true, "真实浏览器业务driver返回失败；原件已保留");
  await Promise.all(responseTasks);
} catch (error) { scenarioError = error; }
finally {
  const cleanupErrors = [];
  let artifactGlueSourceAfter;
  if (browser) try { await browser.close(); } catch (error) { cleanupErrors.push(error); }
  try { await server.close(); } catch (error) { cleanupErrors.push(error); }
  const responses = await Promise.all(responseTasks);
  try { artifactGlueSourceAfter = verifyGlueSource();assert.deepEqual(artifactGlueSourceAfter, artifactGlueSourceBefore); } catch (error) { cleanupErrors.push(error); }
  try { wasmEvidence.verify(); } catch (error) { cleanupErrors.push(error); }
  try { glueEvidence.verify(); } catch (error) { cleanupErrors.push(error); }
  try {
    productResponses.write();productResponses.verify(["apps/web/src/host/engine-clock-env.ts", "apps/web/src/host/facade-wasm-module.ts", "apps/web/src/host/serde-normalize.ts"]);
    for (const name of ["browser-business-engine.ts", "browser-business-main.ts"]) {
      assert(driverResponses.some((row) => row.relative === driverRoot + name), `缺真实driver响应${name}`);
    }
    assert.equal(inputResponses.length, 2);assert(inputResponses.every((row) => row.finished));
  } catch (error) { cleanupErrors.push(error); }
  const rejected = responses.filter((response) => response.status === "rejected");
  writeFileSync(path.join(output, "browser-business-evidence.json"), JSON.stringify({ productRoot: root, artifactRoot: artifact,
    evidenceMode: "server-transmitted-bytes-and-browser-metadata", directBrowserWasmBodySha256Proven: false,
    directBrowserGlueBodySha256Proven: false, artifactGlueSourceBefore, artifactGlueSourceAfter, inputResponses, driverResponses, served,
    wasmResponses: wasmEvidence.records, glueResponses: glueEvidence.records,
    workerLifecycle, observedActiveWorkers: activeWorkers.size, logs,
    scenarioError: scenarioError === undefined ? undefined : errorRecord(scenarioError),
    responseErrors: rejected.map((row) => ({ context: row.context, error: errorRecord(row.reason) })),
    cleanupErrors: cleanupErrors.map((error) => errorRecord(error)) }, null, 2), { flag: "wx" });
  const errors = [...(scenarioError === undefined ? [] : [scenarioError]), ...rejected.map((row) => row.reason), ...cleanupErrors];
  if (errors.length) throw new AggregateError(errors, "真实C2C业务browser/来源/清理失败", { cause: scenarioError });
}
