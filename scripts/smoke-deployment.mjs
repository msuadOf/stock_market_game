import { spawn } from "node:child_process";
import { constants } from "node:fs";
import { access, lstat, readFile, realpath } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const MAX_MS = 10000;
const USAGE = "usage: node scripts/smoke-deployment.mjs <server|webui-server> --input target/build-artifacts/NAME";

export function parseArgs(args) {
  if (args.length !== 3 || !["server", "webui-server"].includes(args[0]) || args[1] !== "--input" || !args[2] || args[2].startsWith("--")) {
    throw new Error(USAGE);
  }
  return { product: args[0], input: args[2] };
}

function remaining(deadline, context) {
  const milliseconds = deadline - Date.now();
  if (milliseconds <= 0) throw new Error(`${context}: deadline exhausted`);
  return milliseconds;
}

async function within(promise, deadline, context) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((resolve, reject) => {
      timer = setTimeout(() => reject(new Error(`${context}: deadline exhausted`)), Math.max(1, deadline - Date.now()));
    })]);
  } finally {
    clearTimeout(timer);
  }
}

export async function resolveExecutable(input, { root = process.cwd(), platform = process.platform } = {}) {
  if (typeof input !== "string" || input.length === 0) throw new Error("--input must name an artifact directory");
  const base = path.resolve(root, "target/build-artifacts");
  const directory = path.resolve(root, input);
  if (path.dirname(directory) !== base) throw new Error("--input must be a direct child: target/build-artifacts/NAME");
  if (!["win32", "darwin", "linux"].includes(platform)) throw new Error(`unsupported native platform: ${platform}`);
  if (!(await lstat(directory)).isDirectory() || await realpath(directory) !== directory) {
    throw new Error(`artifact input must be a real directory without symlink parents: ${directory}`);
  }
  const executable = path.join(directory, platform === "win32" ? "server.exe" : "server");
  const metadata = await lstat(executable);
  if (!metadata.isFile() || metadata.size === 0) throw new Error(`native executable missing, non-file or empty: ${executable}`);
  await access(executable, platform === "win32" ? constants.R_OK : constants.X_OK);
  return executable;
}

export async function availablePort(deadline = Date.now() + 1000) {
  const server = net.createServer();
  const controller = new AbortController();
  try {
    await within(new Promise((resolve, reject) => {
      server.once("error", reject);
      server.listen({ port: 0, host: "127.0.0.1", signal: controller.signal }, resolve);
    }), deadline, "allocate ephemeral loopback port");
    const address = server.address();
    if (address === null || typeof address === "string") throw new Error("ephemeral port allocation returned no TCP address");
    return address.port;
  } finally {
    if (server.listening) {
      await within(new Promise((resolve, reject) => {
        server.close(error => error ? reject(new Error(`port allocation cleanup failed: ${error.message}`)) : resolve());
      }), deadline, "close ephemeral port listener");
    } else {
      controller.abort();
    }
  }
}

export function requestBytes(url, { deadline = Date.now() + 2000 } = {}) {
  const target = new URL(url);
  if (target.protocol !== "http:" || target.hostname !== "127.0.0.1") throw new Error(`smoke requests must use loopback HTTP: ${url}`);
  const timeout = remaining(deadline, `GET ${url}`);
  return new Promise((resolve, reject) => {
    const request = http.get(target, { agent: false }, response => {
      const chunks = [];
      response.on("data", chunk => chunks.push(chunk));
      response.once("error", reject);
      response.once("end", () => resolve({ status: response.statusCode, headers: response.headers, body: Buffer.concat(chunks) }));
    });
    const timer = setTimeout(() => request.destroy(new Error(`GET ${url}: request/body deadline exhausted`)), timeout);
    request.once("error", reject);
    request.once("close", () => clearTimeout(timer));
  });
}

function assetReferences(source, from) {
  const origin = "http://127.0.0.1";
  const references = [];
  for (const match of source.matchAll(/["'`]([^"'`\s<>]+\.(?:js|wasm)(?:[?#][^"'`\s<>]*)?)["'`]/g)) {
    const reference = match[1].startsWith("assets/") ? `/${match[1]}` : match[1];
    const url = new URL(reference, origin + from);
    if (url.origin === origin && /^\/assets\/(?:[A-Za-z0-9_-]+\/)*[A-Za-z0-9_.-]+\.(?:js|wasm)$/.test(url.pathname)) {
      references.push(url.pathname);
    }
  }
  return [...new Set(references)];
}

export async function selectAssets(html, webRoot, { deadline = Date.now() + 2000, onScript } = {}) {
  if (onScript !== undefined && typeof onScript !== "function") throw new Error("asset script checker must be a function");
  const initial = assetReferences(html, "/");
  const js = initial.find(reference => reference.endsWith(".js"));
  if (js === undefined) throw new Error("index HTML has no referenced /assets/*.js");
  const visited = new Set();
  let frontier = initial;
  const assetsRoot = path.resolve(webRoot, "assets");
  while (frontier.length > 0) {
    remaining(deadline, "find referenced WASM");
    const wasm = frontier.find(reference => reference.endsWith(".wasm"));
    if (wasm !== undefined) return { js, wasm };
    const scripts = frontier.filter(reference => reference.endsWith(".js") && !visited.has(reference));
    for (const script of scripts) visited.add(script);
    const references = await within(Promise.all(scripts.map(async script => {
      const filename = path.resolve(webRoot, `.${script}`);
      const canonical = await realpath(filename);
      if (!canonical.startsWith(assetsRoot + path.sep)) throw new Error(`referenced asset escapes assets directory: ${script}`);
      if (onScript !== undefined) await onScript(script);
      return assetReferences(await readFile(canonical, "utf8"), script);
    })), deadline, "read referenced JS/Worker assets");
    frontier = [...new Set(references.flat())].filter(reference => !visited.has(reference));
  }
  throw new Error("index HTML/JS/Worker graph has no referenced /assets/*.wasm");
}

function launch(executable, args, spawnProcess) {
  const child = spawnProcess(executable, args, { shell: false, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  let result;
  child.stdout.on("data", chunk => { output = (output + chunk).slice(-65536); });
  child.stderr.on("data", chunk => { output = (output + chunk).slice(-65536); });
  let spawnError;
  child.once("error", error => { spawnError = error; });
  const closed = new Promise(resolve => child.once("close", (code, signal) => {
    result = { code, signal, spawnError };
    resolve(result);
  }));
  const context = `${path.basename(executable)} ${args.join(" ")}`;
  const detail = () => `${context}; ${spawnError ? `spawn failed: ${spawnError.message}` : `exit ${result?.code}, signal ${result?.signal}`}; output: ${output.trim()}`;
  return {
    async exit(deadline) {
      const status = await within(closed, deadline, context);
      if (status.spawnError || status.code === null) throw new Error(detail());
      return { ...status, output, detail: detail() };
    },
    assertRunning() {
      if (spawnError || result || child.exitCode !== null || child.signalCode !== null) throw new Error(`service exited before smoke completed: ${detail()}`);
    },
    async stop(deadline) {
      if (result === undefined && spawnError === undefined) {
        if (!child.kill("SIGKILL")) throw new Error(`native process cleanup failed (kill refused): ${context}, pid ${child.pid}`);
      }
      await within(closed, deadline, `confirm native process cleanup: ${context}`);
    },
    detail,
  };
}

async function managed(executable, args, settings, action) {
  const processHandle = launch(executable, args, settings.spawnProcess);
  let failure;
  try {
    return await action(processHandle);
  } catch (error) {
    failure = new Error(`${error.message}; ${processHandle.detail()}`, { cause: error });
    throw failure;
  } finally {
    try {
      await processHandle.stop(settings.cleanupDeadline);
    } catch (error) {
      throw new Error(`${failure ? failure.message + "; " : ""}cleanup failed: ${error.message}`, { cause: error });
    }
  }
}

function expectStatus(response, expected, context) {
  if (response.status !== expected) throw new Error(`${context}: expected HTTP ${expected}, got ${response.status}`);
}

function expectIsolation(response, context) {
  for (const [header, expected] of [["cross-origin-opener-policy", "same-origin"], ["cross-origin-embedder-policy", "require-corp"]]) {
    if (response.headers[header] !== expected) throw new Error(`${context}: ${header} expected ${expected}, got ${response.headers[header]}`);
  }
}

async function smokeMode(executable, mode, settings) {
  const port = await availablePort(Math.min(settings.deadline, Date.now() + 1000));
  const base = `http://127.0.0.1:${port}`;
  return managed(executable, ["--services", mode, "--bind", `127.0.0.1:${port}`], settings, async processHandle => {
    const request = route => requestBytes(base + route, { deadline: Math.min(settings.deadline, Date.now() + settings.requestTimeoutMs) });
    let page;
    let lastConnectionError;
    const readinessDeadline = Math.min(settings.deadline, Date.now() + 4000);
    while (page === undefined) {
      processHandle.assertRunning();
      try {
        page = await request("/");
      } catch (error) {
        if (!["ECONNREFUSED", "ECONNRESET"].includes(error.code)) throw error;
        lastConnectionError = error;
        if (Date.now() + 25 >= readinessDeadline) {
          throw new Error(`${mode} ${base}: startup deadline exhausted; last connection error: ${lastConnectionError.message}; possible ephemeral-port bind race`, { cause: error });
        }
        await delay(25);
      }
    }
    processHandle.assertRunning();
    expectStatus(page, mode === "server" ? 404 : 200, `${mode} GET /`);
    const health = await request("/healthz");
    expectStatus(health, mode === "webui" ? 404 : 200, `${mode} GET /healthz`);
    if (mode !== "webui" && health.body.toString() !== "ok") throw new Error(`${mode} /healthz: expected body ok, got ${health.body.toString()}`);
    expectStatus(await request("/api/snapshot?session_id=smoke-missing"), mode === "webui" ? 404 : 401, `${mode} game API /api/snapshot`);
    if (mode !== "server") {
      expectIsolation(page, `${mode} index HTML`);
      if (page.body.length === 0) throw new Error(`${mode} index HTML is empty`);
      const checkAsset = async asset => {
        const response = await request(asset);
        expectStatus(response, 200, `${mode} ${asset}`);
        expectIsolation(response, `${mode} ${asset}`);
        if (response.body.length === 0) throw new Error(`${mode} ${asset}: empty HTTP body`);
      };
      const assets = await selectAssets(page.body.toString(), path.join(path.dirname(executable), "webui"), {
        deadline: settings.deadline, onScript: checkAsset,
      });
      await checkAsset(assets.wasm);
    }
    processHandle.assertRunning();
  });
}

export async function smokeDeployment(options, { root = process.cwd(), spawnProcess = spawn, timeoutMs = MAX_MS, requestTimeoutMs = 1000 } = {}) {
  if (!Number.isInteger(timeoutMs) || timeoutMs < 500 || timeoutMs > MAX_MS) throw new Error(`smoke deadline must be 500..${MAX_MS}ms`);
  if (!Number.isInteger(requestTimeoutMs) || requestTimeoutMs <= 0 || requestTimeoutMs > MAX_MS) throw new Error("invalid request deadline");
  if (!["server", "webui-server"].includes(options.product)) throw new Error(USAGE);
  const start = Date.now();
  const cleanupDeadline = start + timeoutMs;
  const deadline = cleanupDeadline - Math.min(1000, Math.floor(timeoutMs / 4));
  const settings = { spawnProcess, cleanupDeadline, deadline, requestTimeoutMs };
  const executable = await within(resolveExecutable(options.input, { root }), deadline, "resolve artifact input");
  const modes = options.product === "server" ? ["server"] : ["webui", "server", "all"];
  const tasks = [managed(executable, ["--help"], settings, async processHandle => {
    const result = await processHandle.exit(Math.min(deadline, Date.now() + 1500));
    if (result.code !== 0 || !result.output.includes("--services") || !result.output.includes("--bind")) throw new Error(`--help did not validate CLI startup: ${result.detail}`);
  })];
  if (options.product === "server") {
    tasks.push(managed(executable, ["--services", "webui"], settings, async processHandle => {
      const result = await processHandle.exit(Math.min(deadline, Date.now() + 1500));
      if (result.code === 0) throw new Error("pure Server unexpectedly accepted --services webui (exit 0)");
      if (!result.output.includes("--services webui/all requires a binary compiled with Cargo feature web-ui")) {
        throw new Error(`pure Server failed without the expected web-ui feature rejection: ${result.detail}`);
      }
    }));
  }
  tasks.push(...modes.map(mode => smokeMode(executable, mode, settings)));
  const results = await Promise.allSettled(tasks);
  const failures = results.filter(result => result.status === "rejected");
  if (failures.length > 0) throw new Error(`deployment smoke failed: ${failures.map(result => result.reason.message).join("\n")}`);
  return { modes, elapsedMs: Date.now() - start };
}

export async function main(args) {
  const options = parseArgs(args);
  const result = await smokeDeployment(options);
  process.stdout.write(`[smoke ${options.product}] passed: ${result.modes.join(", ")}; ${result.elapsedMs}ms; native startup/HTTP/assets only (no game/browser acceptance)\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => {
    process.stderr.write(`[smoke] ${error.message}; check artifact/CLI/bind and report this log\n`);
    process.exitCode = 1;
  });
}
