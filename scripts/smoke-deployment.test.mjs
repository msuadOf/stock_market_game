import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { chmod, mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, it } from "node:test";

const api = () => import("./smoke-deployment.mjs");
const fixtureSource = `
import { writeSync } from 'node:fs';
import http from 'node:http';
const args = process.argv.slice(1);
const fault = process.env.SMOKE_FIXTURE_FAULT;
const mode = args[args.indexOf('--services') + 1];
if (args.includes('--help')) {
  if (fault === 'help-hang') setInterval(() => {}, 1000);
  else { writeSync(1, 'server --services webui|server|all --bind host:port\\n'); process.exit(fault === 'help-exit' ? 2 : 0); }
} else if (mode === 'webui' && process.env.SMOKE_FIXTURE_PURE === 'yes') {
  if (fault === 'reject-hang') setInterval(() => {}, 1000);
  else { console.error(fault === 'reject-other' ? 'web-root validation failed' : '--services webui/all requires a binary compiled with Cargo feature web-ui'); process.exit(fault === 'reject-zero' ? 0 : 1); }
} else if (fault === 'startup-exit') {
  console.error('cannot bind: port race fixture'); process.exit(3);
} else {
  const address = args[args.indexOf('--bind') + 1];
  const server = http.createServer((request, response) => {
    if (fault === 'request-hang') return;
    const web = mode !== 'server';
    if (web && fault !== 'headers') {
      response.setHeader('cross-origin-opener-policy', 'same-origin');
      response.setHeader('cross-origin-embedder-policy', 'require-corp');
    }
    if (request.url === '/') {
      response.statusCode = web ? 200 : (fault === 'server-root' ? 200 : 404);
      response.end(web ? '<script src="/assets/app.js"></script>' : 'not found');
    } else if (request.url === '/healthz') {
      response.statusCode = mode === 'webui' ? (fault === 'web-health' ? 200 : 404) : 200;
      response.end(fault === 'health-body' ? 'not ok' : 'ok');
    } else if (request.url.startsWith('/api/')) {
      response.statusCode = mode === 'webui' ? (fault === 'web-api' ? 200 : 404) : (fault === 'missing-api' ? 404 : 401);
      response.end('no session');
    } else if (web && request.url === '/assets/app.js') {
      response.end(fault === 'empty-js' ? '' : 'new URL("/assets/worker.js", import.meta.url)');
    } else if (web && request.url === '/assets/worker.js') {
      response.statusCode = fault === 'missing-worker' ? 404 : 200;
      response.end(fault === 'empty-worker' ? '' : 'new URL("./game.wasm", import.meta.url)');
    } else if (web && request.url === '/assets/game.wasm') {
      response.statusCode = fault === 'missing-wasm' ? 404 : 200;
      if (fault === 'body-hang') { response.write('partial'); return; }
      response.end(fault === 'empty-wasm' ? '' : Buffer.from([0, 97, 115, 109]));
    } else { response.statusCode = 404; response.end('not found'); }
  });
  server.on('error', error => { console.error(error.message); process.exit(4); });
  server.listen(Number(address.split(':')[1]), '127.0.0.1');
}
`;

async function artifact(context) {
  const root = await mkdtemp(path.join(await realpath(tmpdir()), "smoke-deployment-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  const input = path.join(root, "target/build-artifacts/fixture");
  await mkdir(path.join(input, "webui/assets"), { recursive: true });
  const executable = path.join(input, process.platform === "win32" ? "server.exe" : "server");
  await writeFile(executable, "native executable placeholder for injected spawn");
  await chmod(executable, 0o755);
  await writeFile(path.join(input, "webui/assets/app.js"), 'new URL("/assets/worker.js", import.meta.url)');
  await writeFile(path.join(input, "webui/assets/worker.js"), 'new URL("./game.wasm", import.meta.url)');
  await writeFile(path.join(input, "webui/assets/game.wasm"), "fixture wasm");
  return { root, input, executable };
}

function fixtureSpawner(fixture, { pure = false, fault = "" } = {}) {
  const children = [];
  const calls = [];
  const spawnProcess = (command, args, options) => {
    assert.equal(command, fixture.executable);
    assert.equal(options.shell, false);
    assert.equal(options.windowsHide, true);
    calls.push(args);
    const env = { ...process.env, SMOKE_FIXTURE_PURE: pure ? "yes" : "no", SMOKE_FIXTURE_FAULT: fault };
    delete env.NODE_TEST_CONTEXT;
    const child = spawn(process.execPath, ["--input-type=module", "-e", fixtureSource, "--", ...args], {
      ...options,
      env,
    });
    children.push(child);
    return child;
  };
  return { spawnProcess, children, calls };
}

function assertClosed(children) {
  assert.ok(children.length > 0);
  for (const child of children) assert.ok(child.exitCode !== null || child.signalCode !== null, `child ${child.pid} leaked`);
}

describe("short native deployment smoke", { concurrency: 4, timeout: 10000 }, () => {
  it("requires exactly a supported product and --input", async () => {
    const { parseArgs } = await api();
    assert.deepEqual(parseArgs(["server", "--input", "target/build-artifacts/a"]), { product: "server", input: "target/build-artifacts/a" });
    assert.equal(parseArgs(["webui-server", "--input", "target/build-artifacts/b"]).product, "webui-server");
    for (const args of [[], ["webui"], ["desktop", "--input", "a"], ["server"], ["server", "--input", ""], ["server", "--input", "a", "--skip"], ["server", "--input", "a", "--input", "b"]]) {
      assert.throws(() => parseArgs(args), /usage|input|product/i);
    }
  });

  it("selects the native filename and refuses missing, empty or unsafe inputs", async (context) => {
    const fixture = await artifact(context);
    const { resolveExecutable } = await api();
    assert.equal(await resolveExecutable(fixture.input, { root: fixture.root }), fixture.executable);
    for (const input of [fixture.root, path.join(fixture.input, "nested"), path.join(fixture.root, "target/build-artifacts/missing")]) {
      await assert.rejects(resolveExecutable(input, { root: fixture.root }), /input|artifact|ENOENT/i);
    }
    await writeFile(fixture.executable, "");
    await assert.rejects(resolveExecutable(fixture.input, { root: fixture.root }), /empty/i);
    for (const platform of ["win32", "darwin", "linux"]) {
      const filename = path.join(fixture.input, platform === "win32" ? "server.exe" : "server");
      await writeFile(filename, "fixture");
      await chmod(filename, 0o755);
      assert.equal(await resolveExecutable(fixture.input, { root: fixture.root, platform }), filename);
    }
  });

  it("runs pure help, rejected WebUI and health without creating a game", async (context) => {
    const fixture = await artifact(context);
    const spawned = fixtureSpawner(fixture, { pure: true });
    const { smokeDeployment } = await api();
    const result = await smokeDeployment({ product: "server", input: fixture.input }, { root: fixture.root, spawnProcess: spawned.spawnProcess });
    assert.deepEqual(result.modes, ["server"]);
    assert.ok(spawned.calls.some(args => args.join(" ") === "--help"));
    assert.ok(spawned.calls.some(args => args.join(" ") === "--services webui"));
    assert.match(spawned.calls.find(args => args.includes("--bind")).at(-1), /^127\.0\.0\.1:\d+$/);
    assertClosed(spawned.children);
  });

  it("checks three combined modes concurrently, fetching JS/WASM and API separation", async (context) => {
    const fixture = await artifact(context);
    const spawned = fixtureSpawner(fixture);
    const { smokeDeployment } = await api();
    const result = await smokeDeployment({ product: "webui-server", input: fixture.input }, { root: fixture.root, spawnProcess: spawned.spawnProcess });
    assert.deepEqual(result.modes, ["webui", "server", "all"]);
    assert.equal(new Set(spawned.calls.filter(args => args.includes("--bind")).map(args => args.at(-1))).size, 3);
    assertClosed(spawned.children);
  });

  const failures = {
    "help-exit": /--help.*exit 2/s,
    "reject-zero": /unexpectedly accepted.*exit 0/,
    "reject-other": /feature.*rejection|rejection.*feature/i,
    "startup-exit": /port race fixture/,
    "server-root": /server GET \/: expected HTTP 404/,
    "health-body": /healthz: expected body ok/,
    "headers": /cross-origin-opener-policy/,
    "web-health": /webui GET \/healthz: expected HTTP 404/,
    "web-api": /webui game API.*expected HTTP 404/,
    "missing-api": /game API.*expected HTTP 401/,
    "empty-js": /app\.js: empty HTTP body/,
    "empty-wasm": /game\.wasm: empty HTTP body/,
    "missing-wasm": /game\.wasm: expected HTTP 200, got 404/,
    "missing-worker": /worker\.js: expected HTTP 200, got 404/,
    "empty-worker": /worker\.js: empty HTTP body/,
  };
  for (const [fault, expected] of Object.entries(failures)) {
    it(`reports ${fault} and closes every child even when sibling modes fail`, async (context) => {
      const fixture = await artifact(context);
      const pure = ["reject-zero", "reject-other", "server-root", "health-body"].includes(fault);
      const spawned = fixtureSpawner(fixture, { pure, fault });
      const { smokeDeployment } = await api();
      await assert.rejects(smokeDeployment({ product: pure ? "server" : "webui-server", input: fixture.input }, { root: fixture.root, spawnProcess: spawned.spawnProcess }), expected);
      assertClosed(spawned.children);
    });
  }

  for (const fault of ["help-hang", "reject-hang", "request-hang", "body-hang"]) {
    it(`bounds ${fault}, including response body reads, and cleans up`, async (context) => {
      const fixture = await artifact(context);
      const spawned = fixtureSpawner(fixture, { pure: fault !== "body-hang", fault });
      const { smokeDeployment } = await api();
      await assert.rejects(smokeDeployment({ product: fault === "body-hang" ? "webui-server" : "server", input: fixture.input }, {
        root: fixture.root, spawnProcess: spawned.spawnProcess, timeoutMs: 1600, requestTimeoutMs: 250,
      }), /deadline|timeout|timed out/i);
      assertClosed(spawned.children);
    });
  }

  it("does not select an unrelated WASM or traverse out of assets", async (context) => {
    const fixture = await artifact(context);
    const { selectAssets } = await api();
    const root = path.join(fixture.input, "webui");
    assert.deepEqual(await selectAssets('<script src="/assets/app.js"></script>', root), { js: "/assets/app.js", wasm: "/assets/game.wasm" });
    await writeFile(path.join(root, "assets/app.js"), 'import "./app.js";');
    await assert.rejects(selectAssets('<script src="/assets/app.js"></script>', root), /referenced.*wasm/i);
    await assert.rejects(selectAssets('<script src="https://example.invalid/assets/app.js"></script>', root), /referenced.*js/i);
    await assert.rejects(selectAssets('<script src="/assets/../secret.js"></script>', root), /referenced.*js/i);
  });

  it("follows Vite page-relative preload entries as well as JS-relative Worker URLs", async (context) => {
    const fixture = await artifact(context);
    const { selectAssets } = await api();
    await writeFile(path.join(fixture.input, "webui/assets/app.js"), 'const deps = ["assets/worker.js"];');
    assert.deepEqual(await selectAssets('<script src="/assets/app.js"></script>', path.join(fixture.input, "webui")), {
      js: "/assets/app.js", wasm: "/assets/game.wasm",
    });
  });

  it("reports actual spawn errors rather than successful skips", async (context) => {
    const fixture = await artifact(context);
    const { smokeDeployment } = await api();
    await assert.rejects(smokeDeployment({ product: "server", input: fixture.input }, {
      root: fixture.root,
      spawnProcess: (command, args, options) => spawn(path.join(fixture.input, "missing-native"), args, options),
    }), /spawn|ENOENT/i);
  });

  it("allocates and releases a loopback ephemeral port", async () => {
    const { availablePort } = await api();
    const port = await availablePort();
    assert.ok(port > 0 && port <= 65535);
    const server = net.createServer();
    await new Promise((resolve, reject) => { server.once("error", reject); server.listen(port, "127.0.0.1", resolve); });
    await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  });

  it("rejects HTTP redirects instead of contacting an external host", async (context) => {
    const { requestBytes } = await api();
    const server = http.createServer((request, response) => {
      response.writeHead(302, { location: "https://example.invalid/market" }); response.end();
    });
    await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
    context.after(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
    const response = await requestBytes(`http://127.0.0.1:${server.address().port}/`);
    assert.equal(response.status, 302);
  });
});
