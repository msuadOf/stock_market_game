import { lstat, readFile } from "node:fs/promises";
import { relative, resolve, join } from "node:path";
import { sha256 } from "./closure.mjs";

export function parseArgs(args) {
  if (args.length === 1 && args[0] === "--help") return { mode: "help" };
  if (args.length !== 2 || !["collect", "verify"].includes(args[0])) throw new Error("usage: collect-baseline-corpus.mjs <collect|verify> <new-evidence-directory>");
  return { mode: args[0], output: args[1] };
}

export async function validateOutput(root, output) {
  const path = relative(resolve(root), resolve(output));
  if (!path.startsWith(".omo/evidence/escrow-parallel-engine/baseline-corpus/") || path.includes("..") || !/^[A-Za-z0-9_./-]+$/.test(path)) throw new Error(`unsafe output path: ${output}`);
  let current = resolve(root);
  for (const part of path.split("/")) {
    current = join(current, part);
    try { if ((await lstat(current)).isSymbolicLink()) throw new Error(`output symlink rejected: ${current}`); }
    catch (error) { if (error.code !== "ENOENT") throw error; }
  }
  return resolve(output);
}

export async function verifyReuse(root, manifest, identity) {
  if (JSON.stringify(manifest.identity) !== JSON.stringify(identity)) throw new Error("stale composite/harness identity; resume rejected");
  for (const receipt of manifest.runs) {
    if (!/^[A-Za-z0-9_.-]+$/.test(receipt.file)) throw new Error("unsafe receipt path");
    const path = join(root, receipt.file);
    if (!(await lstat(path)).isFile()) throw new Error("receipt must be regular file");
    if (sha256(await readFile(path)) !== receipt.sha256) throw new Error(`artifact digest mismatch: ${receipt.file}`);
  }
}
