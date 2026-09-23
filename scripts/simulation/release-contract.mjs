import assert from "node:assert/strict";
import { readdir, readFile, mkdir, writeFile, copyFile, mkdtemp, rm, lstat, cp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { setTimeout as delay } from "node:timers/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { fileHash, sourceInputs, assertArtifactHashes } from "./host-parity-build.mjs";
import { NORMAL_DAY_PARITY_SETUP, launch, stop, waitFor, jsonResponse } from "./host-parity-core.mjs";

const forbidden = /npc_decision_trace|NpcDecisionCollector|npc_decision_diagnostics|decision_records|DecisionDiagnosticsCollector|diagnostics-worker|simulation-diagnostics/;
const execute = promisify(execFile);

async function strings(binary) {
  try {
    return (await execute("bash", ["-c", "LC_ALL=C strings \"$1\" | rg 'NpcDecision|decision_records'", "release-contract-symbols", binary], { maxBuffer: 1_048_576 })).stdout.split("\n").filter((value) => value.length >= 12);
  } catch (error) {
    if (error && typeof error === "object" && error.code === 1) return [];
    throw error;
  }
}

export async function deriveDiagnosticSymbols(featureBinary, defaultBinary) {
  const defaultSymbols = new Set(await strings(defaultBinary));
  const symbols = (await strings(featureBinary)).filter((value) => value.includes("NpcDecision") && !defaultSymbols.has(value));
  assert.ok(symbols.length > 0, "feature binary supplied no diagnostic-only symbols");
  return [...new Set(symbols)].sort();
}

async function scanDefaultSymbols(output, packageName, binaryName) {
  const defaultBinary = `${output}/${binaryName}`;
  const featurePath = `${output}/${binaryName}-diagnostics`;
  await build(output, `${binaryName}-diagnostics-build`, "cargo", ["build", "--locked", "--release", "-p", packageName, "--bin", binaryName, "--features", "simulation-diagnostics"]);
  await copyFile(`target/release/${binaryName}`, featurePath);
  const symbols = await deriveDiagnosticSymbols(featurePath, defaultBinary);
  const defaultSymbols = new Set(await strings(defaultBinary));
  assert.ok(symbols.every((symbol) => !defaultSymbols.has(symbol)), `default ${binaryName} binary contains diagnostic-only symbol`);
  await writeFile(`${output}/${binaryName}-symbols.json`, JSON.stringify({ defaultBinary, featureBinary: featurePath, diagnosticOnlySymbols: symbols, defaultMatches: [] }, null, 2));
}

export async function injectedArtifactProof(root, output) {
  const clone = await mkdtemp(`${tmpdir()}/release-injection-`);
  try {
    await cp(root, clone, { recursive: true });
    await execute(process.execPath, ["scripts/simulation/release-contract.mjs", "--inspect", clone]);
    await writeFile(`${clone}/diagnostics.js`, "npc_decision_trace");
    let failure;
    try {
      await execute(process.execPath, ["scripts/simulation/release-contract.mjs", "--inspect", clone]);
    } catch (error) {
      assert.equal(error.code, 1);
      assert.match(error.stderr, /diagnostics artifact/);
      failure = { exitCode: error.code, stderr: error.stderr };
    }
    assert.ok(failure, "injected diagnostics artifact was accepted");
    await writeFile(`${output}/injection.json`, JSON.stringify({ cleanExitCode: 0, ...failure }, null, 2));
  } finally {
    await rm(clone, { recursive: true, force: true });
    await writeFile(`${output}/injection.cleanup.json`, JSON.stringify({ clone, removed: true }));
  }
}

export async function probeReleaseServer(output, port = 19238) {
  const base = `http://127.0.0.1:${port}`;
  const child = launch(`${output}/server`, [], `${output}/server.process.log`, { STOCK_MARKET_GAME_SERVER_BIND_ADDR: `127.0.0.1:${port}` });
  try {
    await waitFor(`${base}/healthz`);
    const session = await jsonResponse(await fetch(`${base}/api/new`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ setup: NORMAL_DAY_PARITY_SETUP, seed: "7" }) }));
    const headers = { authorization: `Bearer ${session.session_token}` };
    const route = `${base}/api/diagnostics/npc/1?session_id=${session.session_id}&generation=1`;
    assert.equal((await fetch(route)).status, 401);
    const result = await jsonResponse(await fetch(route, { headers }));
    assert.deepEqual(result, { generation: "1", diagnostics: { kind: "unsupported" } });
    const snapshot = await jsonResponse(await fetch(`${base}/api/snapshot?session_id=${session.session_id}`, { headers }));
    assert.ok(Number.isSafeInteger(snapshot.seq));
    assert.equal((await fetch(`${base}/api/session?session_id=${session.session_id}`, { method: "DELETE", headers })).status, 204);
    await writeFile(`${output}/server.json`, JSON.stringify({ authenticated: true, unauthorizedStatus: 401, result, snapshotSeq: snapshot.seq, sessionDeleted: true }, null, 2));
  } finally {
    await writeFile(`${output}/server.cleanup.json`, JSON.stringify(await stop(child), null, 2));
  }
}

export async function probeReleaseTauri(output) {
  const runtime = await mkdtemp(`${tmpdir()}/release-wayland-`);
  const config = await mkdtemp(`${tmpdir()}/release-config-`);
  const captures = await mkdtemp(`${tmpdir()}/release-captures-`);
  const socket = `release-${process.pid}`;
  const weston = launch("weston", ["--backend=headless", "--renderer=pixman", "--width=1280", "--height=800", "--scale=1", "--refresh-rate=60000", "--debug", "--socket", socket, "--no-config"], `${output}/weston.log`, { XDG_RUNTIME_DIR: runtime });
  let app;
  try {
    let ready = false;
    for (let attempt = 0; attempt < 100; attempt += 1) {
      try { ready = (await lstat(`${runtime}/${socket}`)).isSocket(); }
      catch (error) { if (error.code !== "ENOENT") throw error; }
      if (ready) break;
      await delay(100);
    }
    assert.ok(ready, "Weston socket missing");
    app = launch(`${output}/stock-market-game`, [], `${output}/tauri.log`, { GDK_BACKEND: "wayland", WAYLAND_DISPLAY: socket, XDG_RUNTIME_DIR: runtime, XDG_CONFIG_HOME: config, WEBKIT_DISABLE_COMPOSITING_MODE: "1", LIBGL_ALWAYS_SOFTWARE: "1" });
    await delay(35_000);
    let capture;
    for (let attempt = 0; attempt < 2 && !capture; attempt += 1) {
      try {
        const screenshot = await execute("weston-screenshooter", [], { env: { ...process.env, XDG_RUNTIME_DIR: runtime, WAYLAND_DISPLAY: socket, XDG_PICTURES_DIR: captures }, timeout: 20_000 });
        await writeFile(`${output}/tauri-screenshooter-${attempt + 1}.log`, screenshot.stdout + screenshot.stderr);
      } catch (error) {
        if (!(error && typeof error === "object" && error.code !== undefined)) throw error;
        await writeFile(`${output}/tauri-screenshooter-${attempt + 1}.log`, `${error.stdout ?? ""}${error.stderr ?? ""}`);
      }
      capture = (await readdir(captures)).find((entry) => entry.endsWith(".png"));
      if (!capture) await delay(1_000);
    }
    assert.ok(capture, "Weston screenshooter produced no release Tauri PNG");
    await copyFile(`${captures}/${capture}`, `${output}/tauri-release.png`);
    await execute("uv", ["run", "scripts/desktop/validate-wayland-png.py", `${output}/tauri-release.png`, "1280", "800"]);
    await writeFile(`${output}/tauri-launch.json`, JSON.stringify({ binary: `${output}/stock-market-game`, compositor: "Weston headless Pixman", gdkBackend: "wayland", screenshot: "tauri-release.png", ipcVerification: "release handler tests; external inspector is unavailable in default release" }, null, 2));
  } finally {
    const appCleanup = app ? await stop(app) : null;
    const westonCleanup = await stop(weston);
    await rm(runtime, { recursive: true, force: true });
    await rm(config, { recursive: true, force: true });
    await rm(captures, { recursive: true, force: true });
    await writeFile(`${output}/tauri.cleanup.json`, JSON.stringify({ app: appCleanup, weston: westonCleanup, runtimeRemoved: true, configRemoved: true, capturesRemoved: true }, null, 2));
  }
}

async function build(output, name, command, args) {
  const log = `${output}/${name}.log`;
  const shell = 'log="$1"; shift; nohup "$@" > "$log" 2>&1 & PID=$!; while kill -0 "$PID" 2>/dev/null; do sleep 60; echo "[t+] $(tail -c 150 "$log")"; done; wait "$PID"; status=$?; echo exit=$status; exit "$status"';
  const pending = execute("bash", ["-c", shell, "release-build", log, command, ...args], { timeout: 1_800_000 });
  pending.child.stdout.on("data", (chunk) => process.stdout.write(chunk));
  await pending;
}

export async function buildReleaseArtifacts(output) {
  assert.equal(process.version, "v24.18.0", "pinned Node required");
  await mkdir(output);
  const inputs = await sourceInputs();
  await writeFile(`${output}/inputs.json`, JSON.stringify(inputs, null, 2));
  await build(output, "wasm-build", "bash", ["scripts/wasm-build.sh"]);
  const normal = await fileHash("apps/web/wasm-pkg/web_wasm_bg.wasm");
  assert.equal(normal, await fileHash("apps/web-wasm/pkg/web_wasm_bg.wasm"));
  const diagnostic = await fileHash("apps/web/wasm-diagnostics-pkg/web_wasm_bg.wasm");
  assert.notEqual(normal, diagnostic, "release WASM must not be diagnostic WASM");
  await writeFile(`${output}/normal-wasm.json`, JSON.stringify({ sha256: normal, diagnosticComparisonSha256: diagnostic }));
  const web = await inspectReleaseArtifacts("apps/web/dist");
  await writeFile(`${output}/web.json`, JSON.stringify(web, null, 2));
  for (const [name, binary] of [["server", "server"], ["stock-market-game", "stock-market-game"]]) {
    await build(output, `${name}-build`, "cargo", ["build", "--locked", "--release", "-p", name, "--bin", binary]);
    await copyFile(`target/release/${binary}`, `${output}/${binary}`);
  }
  await assertArtifactHashes(inputs);
  const hashes = Object.fromEntries(await Promise.all(["server", "stock-market-game"].map(async (name) => [`${output}/${name}`, await fileHash(`${output}/${name}`)])));
  await writeFile(`${output}/binary-hashes.json`, JSON.stringify(hashes, null, 2));
  await injectedArtifactProof("apps/web/dist", output);
  await probeReleaseServer(output);
  await build(output, "tauri-release-tests", "cargo", ["test", "--locked", "--release", "-p", "stock-market-game", "--", "--nocapture"]);
  await scanDefaultSymbols(output, "server", "server");
  await scanDefaultSymbols(output, "stock-market-game", "stock-market-game");
  await probeReleaseTauri(output);
  await assertArtifactHashes(hashes);
  await assertArtifactHashes(inputs);
  await assertArtifactHashes(Object.fromEntries(web.files.map((file) => [file.path, file.sha256])));
  assert.equal(await fileHash("apps/web/wasm-pkg/web_wasm_bg.wasm"), normal);
  await writeFile(`${output}/result.json`, JSON.stringify({ passed: true, hashes, normalWasm: normal, diagnosticWasm: diagnostic, web }, null, 2));
}

export async function inspectReleaseArtifacts(root) {
  const files = [];
  async function visit(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = `${directory}/${entry.name}`;
      if (entry.isSymbolicLink()) throw new Error(`unsupported artifact symlink: ${path}`);
      if (entry.isDirectory()) {
        await visit(path);
      } else if (entry.isFile()) {
        const bytes = await readFile(path);
        if (/diagnostic/i.test(entry.name) || forbidden.test(bytes.toString("utf8"))) {
          throw new Error(`diagnostics artifact: ${path}`);
        }
        files.push({ path, bytes: bytes.length, sha256: await fileHash(path) });
      } else {
        throw new Error(`unsupported artifact: ${path}`);
      }
    }
  }
  await visit(root);
  assert.ok(files.length > 0, "empty release artifact tree");
  return { contaminated: false, files: files.sort((left, right) => left.path.localeCompare(right.path)) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  assert.equal(process.version, "v24.18.0", "pinned Node required");
  const args = process.argv.slice(2);
  if (args.length !== 2 || !["--inspect", "--output"].includes(args[0])) {
    throw new Error("usage: node scripts/simulation/release-contract.mjs --inspect <built-surface> | --output <new-evidence-dir>");
  }
  if (args[0] === "--output") await buildReleaseArtifacts(resolve(args[1]));
  else console.log(JSON.stringify(await inspectReleaseArtifacts(resolve(args[1])), null, 2));
}
