#!/usr/bin/env node
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import fsp from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const AUDIT_SCHEMA = "escrow-diagnostic-divergence-audit-v1";
const DIFF_PATHS = ["packages/engine/src/diagnostics.rs", "packages/engine/tests"];
const ANNOTATION = /\/\/\s*分歧\s*#([1-9])\s*[;；]\s*evidence:\s*(\S+)\s*$/;

function fail(message) {
  throw new Error(message);
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function isAllowedFile(file) {
  return file === "packages/engine/src/diagnostics.rs" || /^packages\/engine\/tests\/[^/].*\.rs$/.test(file);
}

function expectationLine(line) {
  const code = line.replace(/\/\/.*$/, "").trim();
  if (code.length === 0) return false;
  if (/assert(?:_eq|_ne|_matches)?!/.test(code)) return true;
  if (/engine_error_events|submitted|filled|canceled|open|aborted|expected/i.test(code) && /(?:\d|"|::)/.test(code)) return true;
  return /^(?:-?\d[\d_]*(?:\.\d+)?|"[^"]+"|[A-Za-z_][A-Za-z0-9_]*::[A-Za-z_][A-Za-z0-9_]*)\s*[,)]/.test(code);
}

function normalizedExpectation(line) {
  return line
    .replace(/\/\/.*$/, "")
    .replace(/"(?:[^"\\]|\\.)*"/g, '"<value>"')
    .replace(/-?\d[\d_]*(?:\.\d+)?/g, "<number>")
    .replace(/\s+/g, " ")
    .trim();
}

function parseHunkHeader(line) {
  const match = line.match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/);
  if (!match) fail(`malformed unified diff hunk: ${line}`);
  return { oldLine: Number(match[1]), newLine: Number(match[3]), symbol: match[5].trim() || null };
}

function validateEvidencePath(evidence, label) {
  if (!evidence.startsWith(".omo/evidence/escrow-parallel-engine/task-9/")
    || evidence.includes("\\")
    || evidence.split("/").some((part) => part === "" || part === "." || part === "..")) {
    fail(`${label} evidence path must point to a safe Task 9 corpus artifact`);
  }
}

function validateEvidenceArtifact(evidence, evidenceFiles, label) {
  validateEvidencePath(evidence, label);
  if (!isRecord(evidenceFiles) || !Object.hasOwn(evidenceFiles, evidence)) fail(`${label} evidence artifact is missing: ${evidence}`);
  const content = evidenceFiles[evidence];
  if (!(typeof content === "string" || Buffer.isBuffer(content) || content instanceof Uint8Array)) fail(`${label} evidence artifact has invalid bytes: ${evidence}`);
  const bytes = Buffer.from(content);
  if (bytes.length === 0) fail(`${label} evidence artifact is empty: ${evidence}`);
  if (evidence.endsWith(".json")) {
    try {
      JSON.parse(bytes.toString("utf8"));
    } catch (error) {
      fail(`${label} evidence artifact is not valid JSON: ${error.message}`);
    }
  }
  return { evidence_byte_length: bytes.length, evidence_sha256: sha256(bytes) };
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function analyzeHunk(hunk, file, evidenceFiles) {
  const removedCode = hunk.removed.filter((line) => line.text.trim().length > 0);
  const addedCode = hunk.added.filter((line) => line.text.trim().length > 0);
  const removedExpectations = removedCode.filter((line) => expectationLine(line.text));
  const addedExpectations = addedCode.filter((line) => expectationLine(line.text));
  if (removedExpectations.length > 0 && addedExpectations.length === 0) {
    fail(`${file}:${hunk.oldStart} removes a diagnostic expectation without an annotated replacement`);
  }
  const changes = [];
  for (const added of addedExpectations) {
    const annotation = added.text.match(ANNOTATION);
    if (!annotation) fail(`${file}:${added.line} diagnostic expectation change lacks // 分歧 #N; evidence: <Task-9-path>`);
    const divergence = Number(annotation[1]);
    const evidence = annotation[2];
    const evidenceReceipt = validateEvidenceArtifact(evidence, evidenceFiles, `${file}:${added.line}`);
    const shape = normalizedExpectation(added.text);
    const original = removedExpectations.find((line) => normalizedExpectation(line.text) === shape)
      ?? removedExpectations[changes.length]
      ?? null;
    changes.push({
      file,
      symbol: hunk.symbol,
      old_line: original?.line ?? null,
      new_line: added.line,
      original: original?.text.trim() ?? null,
      replacement: added.text.trim(),
      divergence,
      evidence,
      ...evidenceReceipt,
    });
  }
  if (removedExpectations.length > changes.length) fail(`${file}:${hunk.oldStart} has an unpaired removed diagnostic expectation`);
  const implementationRemoved = removedCode.filter((line) => !removedExpectations.includes(line));
  const implementationAdded = addedCode.filter((line) => !addedExpectations.includes(line));
  const implementation = implementationRemoved.length === 0 && implementationAdded.length === 0 ? null : {
    file,
    symbol: hunk.symbol,
    old_start: hunk.oldStart,
    new_start: hunk.newStart,
    removed: implementationRemoved.map((line) => ({ line: line.line, text: line.text.trim() })),
    added: implementationAdded.map((line) => ({ line: line.line, text: line.text.trim() })),
    hunk_sha256: sha256(hunk.lines.map((line) => `${line.kind}:${line.text}`).join("\n")),
  };
  return { changes, implementation };
}

export function parseDiagnosticDiff(diff, { baseSha, headSha, evidenceFiles }) {
  if (typeof diff !== "string" || diff.length === 0) fail("diagnostic audit refuses an empty commit-range diff");
  const lines = diff.split(/\r?\n/);
  const files = new Set();
  const changes = [];
  const implementationHunks = [];
  let file;
  let hunk;
  const flushHunk = () => {
    if (hunk && file) {
      const analysis = analyzeHunk(hunk, file, evidenceFiles);
      changes.push(...analysis.changes);
      if (analysis.implementation) implementationHunks.push(analysis.implementation);
    }
    hunk = undefined;
  };
  for (const line of lines) {
    if (line.startsWith("diff --git ")) {
      flushHunk();
      file = undefined;
      continue;
    }
    if (line.startsWith("new file mode ") || line.startsWith("deleted file mode ") || line.startsWith("rename from ") || line.startsWith("rename to ")) {
      fail(`diagnostic audit does not accept file creation, deletion, or rename: ${line}`);
    }
    if (line.startsWith("+++ ")) {
      const target = line.slice(4);
      if (!target.startsWith("b/")) fail(`diagnostic audit target is malformed: ${target}`);
      file = target.slice(2);
      if (!isAllowedFile(file)) fail(`diagnostic audit diff escaped its allowlist: ${file}`);
      files.add(file);
      continue;
    }
    if (line.startsWith("@@ ")) {
      flushHunk();
      if (!file) fail("diagnostic audit hunk appeared before a target file");
      const header = parseHunkHeader(line);
      hunk = { ...header, oldStart: header.oldLine, newStart: header.newLine, lines: [], removed: [], added: [] };
      continue;
    }
    if (!hunk) continue;
    if (line.startsWith("\\ No newline")) continue;
    const prefix = line[0];
    const text = line.slice(1);
    if (prefix === " ") {
      hunk.lines.push({ kind: "context", text, oldLine: hunk.oldLine, newLine: hunk.newLine });
      hunk.oldLine += 1;
      hunk.newLine += 1;
    } else if (prefix === "-") {
      const record = { kind: "removed", text, line: hunk.oldLine };
      hunk.lines.push(record);
      hunk.removed.push(record);
      hunk.oldLine += 1;
    } else if (prefix === "+") {
      const record = { kind: "added", text, line: hunk.newLine };
      hunk.lines.push(record);
      hunk.added.push(record);
      hunk.newLine += 1;
    }
  }
  flushHunk();
  if (files.size === 0) fail("diagnostic audit range changed no allowlisted files");
  if (changes.length === 0) fail("diagnostic audit found no annotated expectation changes; refusing an empty pass");
  return {
    schema: AUDIT_SCHEMA,
    base_sha: baseSha,
    head_sha: headSha,
    command: ["git", "diff", baseSha, headSha, "--", ...DIFF_PATHS],
    diff_sha256: sha256(diff),
    files: [...files].sort(),
    expectation_changes: changes,
    implementation_hunks: implementationHunks,
  };
}

function execGit(repoRoot, args) {
  return new Promise((resolve, reject) => {
    execFile("git", args, { cwd: repoRoot, maxBuffer: 32 * 1024 * 1024 }, (error, stdout, stderr) => {
      if (error) {
        reject(new Error(`git ${args.join(" ")} failed: ${stderr.trim() || error.message}`));
        return;
      }
      resolve(stdout);
    });
  });
}

export async function auditDiagnosticRange(repoRoot, baseSha, headSha) {
  if (typeof repoRoot !== "string" || !path.isAbsolute(repoRoot)) fail("diagnostic audit repo root must be absolute");
  for (const [label, revision] of [["base", baseSha], ["head", headSha]]) {
    if (typeof revision !== "string" || !/^[a-f0-9]{40,64}$/.test(revision)) fail(`diagnostic audit ${label} SHA must be a full object id`);
    const resolved = (await execGit(repoRoot, ["rev-parse", "--verify", `${revision}^{commit}`])).trim();
    if (resolved !== revision) fail(`diagnostic audit ${label} SHA did not resolve exactly`);
  }
  if (baseSha === headSha) fail("diagnostic audit base and head SHA must differ");
  const diff = await execGit(repoRoot, ["diff", "--unified=3", baseSha, headSha, "--", ...DIFF_PATHS]);
  const evidenceFiles = {};
  for (const match of diff.matchAll(/\/\/\s*分歧\s*#[1-9]\s*[;；]\s*evidence:\s*(\S+)\s*$/gm)) {
    const evidence = match[1];
    validateEvidencePath(evidence, "diagnostic audit");
    if (Object.hasOwn(evidenceFiles, evidence)) continue;
    const treeEntry = (await execGit(repoRoot, ["ls-tree", baseSha, "--", evidence])).trim();
    const treeMatch = treeEntry.match(/^(100644|100755) blob [a-f0-9]+\t/);
    if (!treeMatch) fail(`diagnostic audit evidence is missing or not a committed regular file at Task 9 SHA: ${evidence}`);
    const object = `${baseSha}:${evidence}`;
    evidenceFiles[evidence] = await execGit(repoRoot, ["show", object]);
  }
  return parseDiagnosticDiff(diff, { baseSha, headSha, evidenceFiles });
}

async function writeNewFile(filePath, content) {
  try {
    await fsp.lstat(filePath);
    fail(`diagnostic audit output already exists: ${filePath}`);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  await fsp.writeFile(filePath, content, { flag: "wx" });
}

export async function main(argv) {
  if (argv.length !== 4 || argv.some((value) => value.startsWith("-"))) {
    fail("usage: node scripts/simulation/audit-diagnostic-divergence.mjs <repo-root> <task9-sha> <task10-sha> <new-audit.json>");
  }
  const [repoRootInput, baseSha, headSha, outputInput] = argv;
  const repoRoot = path.resolve(repoRootInput);
  const output = path.resolve(outputInput);
  const audit = await auditDiagnosticRange(repoRoot, baseSha, headSha);
  await writeNewFile(output, `${JSON.stringify(audit, null, 2)}\n`);
  console.log(JSON.stringify({ output, expectation_changes: audit.expectation_changes.length, implementation_hunks: audit.implementation_hunks.length, diff_sha256: audit.diff_sha256 }));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`diagnostic divergence audit failed: ${error.message}`);
    process.exitCode = 1;
  });
}
