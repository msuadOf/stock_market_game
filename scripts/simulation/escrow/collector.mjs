import { mkdtemp, mkdir, readFile, writeFile, rm, lstat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { captureClosure, captureFiles, applyArchive, verifyClosure, walk, sha256, HEAD } from "./closure.mjs";
import { run, git } from "./process.mjs";
import { parseArgs, validateOutput, verifyReuse } from "./options.mjs";
import { feasibility } from "./compare.mjs";
import { buildInventory, verifyInventory } from "./inventory.mjs";
import { surfaces, transformations } from "./contracts.mjs";
import { inspectRun } from "./coverage.mjs";

const scenarios = ["equivalence", "divergence-9", "representation", "stress"];
const seeds = Array.from({ length: 10 }, (_, index) => index + 1);
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;

async function identities(root) {
  const baseline = await captureClosure(root);
  const harnessPaths = ["packages/engine/examples/escrow_baseline_corpus.rs", "scripts/simulation/collect-baseline-corpus.mjs",
    "scripts/simulation/escrow-baseline.test.mjs", ...await walk(root, "packages/engine/examples/escrow_baseline"), ...await walk(root, "scripts/simulation/escrow")];
  const harness = await captureFiles(root, harnessPaths, new Map());
  const identity = { composite: sha256(JSON.stringify(baseline)), harness: sha256(JSON.stringify(harness)), node: process.version };
  return { baseline, harness, identity };
}

export async function collect(root, options) {
  parseArgs([options.mode, options.output]);
  const output = await validateOutput(root, options.output);
  const { baseline, harness, identity } = await identities(root);
  if (options.mode === "verify") {
    const manifestBytes = await readFile(join(output, "manifest.json"));
    const seal = JSON.parse(await readFile(join(output, "seal.json"), "utf8"));
    if (seal.manifest_sha256 !== sha256(manifestBytes)) throw new Error("manifest seal digest mismatch");
    const manifest = JSON.parse(manifestBytes);
    if (seal.corpus_digest !== manifest.corpus_digest || seal.run_count !== manifest.runs.length) throw new Error("seal fields mismatch");
    const cleanupBytes = await readFile(join(output, "cleanup.json"));
    if (seal.cleanup_sha256 !== sha256(cleanupBytes)) throw new Error("cleanup digest mismatch");
    const cleanup = JSON.parse(cleanupBytes);
    if (!cleanup.complete || !cleanup.detached_worktree_removed || !cleanup.temporary_directory_removed) throw new Error("incomplete cleanup");
    await verifyReuse(output, manifest, identity);
    const frozenInventory = JSON.parse(await readFile(join(output, "b-test-inventory.json"), "utf8"));
    const currentInventory = await buildInventory(root, baseline.closure);
    verifyInventory(frozenInventory, currentInventory);
    for (const receipt of manifest.artifacts) {
      if (!/^[A-Za-z0-9_.-]+$/.test(receipt.file) || sha256(await readFile(join(output, receipt.file))) !== receipt.sha256) throw new Error(`sealed artifact mismatch: ${receipt.file}`);
    }
    return { verified: true, ...seal };
  }
  await mkdir(output);
  const temporary = await mkdtemp(join(tmpdir(), "escrow-composite-"));
  const detached = join(temporary, "tree");
  const controller = new AbortController();
  const interrupt = () => controller.abort();
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", interrupt);
  let attached = false;
  let complete = false;
  const receipts = [];
  const artifacts = [];
  const store = async (file, value) => {
    const bytes = typeof value === "string" ? value : json(value);
    await writeFile(join(output, file), bytes, { flag: "wx" });
    artifacts.push({ file, sha256: sha256(bytes) });
  };
  let result;
  try {
    await git(root, ["worktree", "add", "--detach", detached, HEAD]);
    attached = true;
    if ((await git(detached, ["status", "--porcelain"])).trim()) throw new Error("detached HEAD not clean");
    await applyArchive(detached, baseline.overlay);
    await verifyClosure(detached, baseline);
    await applyArchive(detached, harness);
    await verifyClosure(detached, baseline);
    await store("closure.json", baseline);
    await store("overlay.json", baseline.overlay);
    await store("harness.json", harness);
    const inventory = await buildInventory(detached, baseline.closure);
    await store("b-test-inventory.json", inventory);
    const execute = async (program, args) => {
      const execution = await run(program, args, { cwd: detached, timeout: 120_000, signal: controller.signal });
      receipts.push({ ...execution, stdout: program.endsWith("/escrow_baseline_corpus") ? `sha256:${sha256(execution.stdout)}` : execution.stdout });
      return execution;
    };
    await execute("cargo", ["test", "--locked", "--offline", "-p", "engine", "--example", "escrow_baseline_corpus"]);
    await execute("cargo", ["build", "--locked", "--offline", "-p", "engine", "--example", "escrow_baseline_corpus"]);
    await execute("cargo", ["clippy", "--locked", "--offline", "-p", "engine", "--example", "escrow_baseline_corpus", "--", "-D", "warnings"]);
    await execute("cargo", ["check", "--locked", "--offline", "-p", "engine", "--example", "escrow_baseline_corpus"]);
    await execute("rustfmt", ["--check", "--edition", "2021", "packages/engine/examples/escrow_baseline_corpus.rs"]);
    const executable = join(detached, "target/debug/examples/escrow_baseline_corpus");
    const runs = [];
    for (const scenario of scenarios) for (const seed of seeds) {
      const args = [scenario, String(seed)];
      const first = await execute(executable, args);
      const second = await execute(executable, args);
      if (sha256(first.stdout) !== sha256(second.stdout)) throw new Error(`flaky repeat: ${scenario}/${seed}`);
      const records = first.stdout.trim().split("\n").map((line) => JSON.parse(line));
      const frames = records.filter((record) => record.kind === "TickFrame");
      const comparison = feasibility(frames);
      const coverage = inspectRun(records, scenario);
      const file = `${scenario}-${seed}.jsonl`;
      await writeFile(join(output, file), first.stdout, { flag: "wx" });
      runs.push({ scenario, seed, file, sha256: sha256(first.stdout), repeat_sha256: sha256(second.stdout), frame_count: frames.length, comparison, coverage });
    }
    let malformed;
    try { await execute(executable, ["malformed", "1"]); }
    catch (error) { malformed = String(error); }
    if (!malformed?.includes("unknown scenario")) throw new Error("malformed scenario was not rejected explicitly");
    const observed = new Set(runs.flatMap((receipt) => receipt.coverage.observed_divergence_surfaces));
    for (let id = 1; id <= 9; id++) if (!observed.has(id)) throw new Error(`unexercised divergence surface: ${id}`);
    await verifyClosure(detached, baseline);
    const after = await identities(root);
    if (JSON.stringify(after.identity) !== JSON.stringify(identity)) throw new Error("live identity changed during collection");
    await store("commands.json", receipts.map((receipt) => ({ ...receipt, stdout: receipt.command[0] === executable ? `sha256:${sha256(receipt.stdout)}` : receipt.stdout })));
    await store("malformed-scenario.txt", malformed);
    const manifest = { schema: 2, status: "sealed", preserved_test_baseline_sha: baseline.commit,
      baseline_policy: "HEAD + applicable pre-existing closure overlay; new harness identity separate",
      identity, head_tree: baseline.tree, closure_manifest_digest: baseline.closure_digest,
      overlay_archive_digest: baseline.overlay_digest, harness_digest: identity.harness,
      closure: baseline.closure, seeds, scenarios, runs, b_test_inventory: inventory.b_test_inventory,
      inventory_file: "b-test-inventory.json", artifacts,
      per_surface_construction_matrix: surfaces, allowed_transformations: transformations,
      protocol: "TickFrame fact multiset; seq coverage strict; tick/timeseries strict; old frame-local order ignored",
      reuse_requirements: ["exact composite and harness and Node identity", "manifest seal", "every artifact digest", "no resume into partial collection"],
      corpus_digest: sha256(JSON.stringify(runs.map(({ scenario, seed, sha256: hash }) => ({ scenario, seed, hash })))),
    };
    const bytes = json(manifest);
    await writeFile(join(output, "manifest.json"), bytes, { flag: "wx" });
    result = { manifest_sha256: sha256(bytes), corpus_digest: manifest.corpus_digest, run_count: runs.length };
    await verifyReuse(output, manifest, identity);
    complete = true;
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", interrupt);
    if (attached) await git(root, ["worktree", "remove", "--force", detached]);
    await rm(temporary, { recursive: true, force: true });
    for (const path of [detached, temporary]) {
      try { await lstat(path); throw new Error(`cleanup leaked path: ${path}`); }
      catch (error) { if (error.code !== "ENOENT") throw error; }
    }
    const cleanupBytes = json({ complete, detached_worktree_removed: attached, temporary_directory_removed: true,
      temporary, archive_policy: "temporary overlay/harness copies removed with detached tree; content-addressed evidence archives intentionally retained", interrupted: controller.signal.aborted });
    await writeFile(join(output, "cleanup.json"), cleanupBytes);
    if (complete) {
      result.cleanup_sha256 = sha256(cleanupBytes);
      await writeFile(join(output, "seal.json"), json(result), { flag: "wx" });
    }
  }
  return { output: resolve(output), ...result };
}
