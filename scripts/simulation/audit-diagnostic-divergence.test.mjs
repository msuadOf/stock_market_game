import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { after, describe, it } from "node:test";

import { auditDiagnosticRange, parseDiagnosticDiff } from "./audit-diagnostic-divergence.mjs";

const BASE = "a".repeat(40);
const HEAD = "b".repeat(40);
const EVIDENCE = ".omo/evidence/escrow-parallel-engine/task-9/corpus-diff.json";
const EVIDENCE_FILES = { [EVIDENCE]: '{"schema":"task-9-corpus-diff-v1"}\n' };
const REPO_TMP_ROOT = path.resolve(
  process.env.TASK_TMP_ROOT
    ?? path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", ".tmp"),
);
const fixtures = [];

function diffFor(replacement, file = "packages/engine/src/diagnostics.rs") {
  return [
    `diff --git a/${file} b/${file}`,
    "index 1111111..2222222 100644",
    `--- a/${file}`,
    `+++ b/${file}`,
    "@@ -10,3 +10,3 @@ fn diagnostic_expectation() {",
    "     let report = run();",
    "-    assert_eq!(report.engine_error_events, 55);",
    `+    ${replacement}`,
    " }",
    "",
  ].join("\n");
}

function git(root, args) {
  return new Promise((resolve, reject) => {
    execFile("git", args, { cwd: root }, (error, stdout, stderr) => {
      if (error) reject(new Error(stderr || error.message));
      else resolve(stdout.trim());
    });
  });
}

class DiagnosticAuditRepositoryFixture {
  constructor(root, gitRunner) {
    this.root = root;
    this.diagnosticsPath = path.join(root, "packages", "engine", "src", "diagnostics.rs");
    this.evidencePath = path.join(root, EVIDENCE);
    this.gitRunner = gitRunner;
    this.baseSha = undefined;
    this.headSha = undefined;
    // 目录创建后立即登记，初始化失败也有明确的清理 owner。
    fixtures.push(this);
  }

  static async create({ gitRunner = git } = {}) {
    await mkdir(REPO_TMP_ROOT, { recursive: true });
    const root = await mkdtemp(path.join(REPO_TMP_ROOT, "diagnostic-audit-test-"));
    const fixture = new DiagnosticAuditRepositoryFixture(root, gitRunner);
    try {
      await mkdir(path.dirname(fixture.diagnosticsPath), { recursive: true });
      await gitRunner(root, ["init", "--quiet"]);
      await gitRunner(root, ["config", "user.email", "audit@example.test"]);
      await gitRunner(root, ["config", "user.name", "Audit Test"]);
      return fixture;
    } catch (error) {
      try { await fixture.dispose(); } catch (cleanupError) {
        throw new AggregateError([error, cleanupError], "DiagnosticAuditRepositoryFixture 初始化与清理均失败");
      }
      throw error;
    }
  }

  async writeEvidence() {
    await mkdir(path.dirname(this.evidencePath), { recursive: true });
    await writeFile(this.evidencePath, EVIDENCE_FILES[EVIDENCE]);
  }

  async commit(label) {
    await this.gitRunner(this.root, ["add", "."]);
    await this.gitRunner(this.root, ["commit", "--quiet", "-m", label]);
    return this.gitRunner(this.root, ["rev-parse", "HEAD"]);
  }

  async commitBase(source) {
    await writeFile(this.diagnosticsPath, source);
    await this.writeEvidence();
    this.baseSha = await this.commit("base");
    return this.baseSha;
  }

  async commitHead(source) {
    await writeFile(this.diagnosticsPath, source);
    this.headSha = await this.commit("head");
    return this.headSha;
  }

  dispose() {
    return rm(this.root, { recursive: true, force: true });
  }
}

after(async () => {
  await Promise.all(fixtures.map((fixture) => fixture.dispose()));
});

it("cleans a registered fixture when repository initialization fails", async () => {
  let createdRoot;
  await assert.rejects(() => DiagnosticAuditRepositoryFixture.create({ gitRunner: async (root) => {
    createdRoot = root;
    throw new Error("fixture initialization failure");
  } }), /fixture initialization failure/);
  assert.ok(createdRoot);
  await assert.rejects(lstat(createdRoot), { code: "ENOENT" });
});

describe("fixed-range diagnostic divergence audit", () => {
  it("emits original/new expectation, divergence id and escrow verification evidence path", () => {
    const audit = parseDiagnosticDiff(
      diffFor(`assert_eq!(report.engine_error_events, 60); // 分歧 #9; evidence: ${EVIDENCE}`),
      { baseSha: BASE, headSha: HEAD, evidenceFiles: EVIDENCE_FILES },
    );
    assert.equal(audit.expectation_changes.length, 1);
    assert.deepEqual(audit.expectation_changes[0], {
      file: "packages/engine/src/diagnostics.rs",
      symbol: "fn diagnostic_expectation() {",
      old_line: 11,
      new_line: 11,
      original: "assert_eq!(report.engine_error_events, 55);",
      replacement: `assert_eq!(report.engine_error_events, 60); // 分歧 #9; evidence: ${EVIDENCE}`,
      original_value: "55",
      new_value: "60",
      divergence: 9,
      evidence: EVIDENCE,
      evidence_byte_length: Buffer.byteLength(EVIDENCE_FILES[EVIDENCE]),
      evidence_sha256: createHash("sha256").update(EVIDENCE_FILES[EVIDENCE]).digest("hex"),
    });
    assert.match(audit.diff_sha256, /^[a-f0-9]{64}$/);
    assert.deepEqual(audit.command, ["git", "diff", BASE, HEAD, "--", "packages/engine/src/diagnostics.rs", "packages/engine/tests"]);
  });

  it("rejects an unannotated expectation change, deletion-only hunk, empty diff and unsafe evidence", () => {
    assert.throws(() => parseDiagnosticDiff(diffFor("assert_eq!(report.engine_error_events, 60);"), { baseSha: BASE, headSha: HEAD, evidenceFiles: EVIDENCE_FILES }), /lacks.*分歧|no annotated/);

    const deletion = diffFor("").replace(/^\+\s*$/m, "");
    assert.throws(() => parseDiagnosticDiff(deletion, { baseSha: BASE, headSha: HEAD, evidenceFiles: EVIDENCE_FILES }), /without an annotated replacement|unpaired|no annotated/);

    assert.throws(() => parseDiagnosticDiff("", { baseSha: BASE, headSha: HEAD, evidenceFiles: EVIDENCE_FILES }), /empty/);

    const unsafe = diffFor("assert_eq!(report.engine_error_events, 60); // 分歧 #9; evidence: ../../invented.json");
    assert.throws(() => parseDiagnosticDiff(unsafe, { baseSha: BASE, headSha: HEAD, evidenceFiles: EVIDENCE_FILES }), /safe escrow verification/);
  });

  it("records a lifecycle implementation hunk and rejects an absent evidence artifact", () => {
    const valid = diffFor(`assert_eq!(report.engine_error_events, 60); // 分歧 #9; evidence: ${EVIDENCE}`);
    const silent = [
      "@@ -30,3 +30,3 @@ fn causal_lifecycle() {",
      "-    let submitted = filled + canceled + open;",
      "+    let submitted = filled + canceled + open + aborted;",
      " }",
      "",
    ].join("\n");
    const audit = parseDiagnosticDiff(`${valid}${silent}`, { baseSha: BASE, headSha: HEAD, evidenceFiles: EVIDENCE_FILES });
    assert.equal(audit.expectation_changes.length, 1);
    assert.deepEqual(audit.implementation_hunks[0].added, [{ line: 30, text: "let submitted = filled + canceled + open + aborted;" }]);
    assert.match(audit.implementation_hunks[0].hunk_sha256, /^[a-f0-9]{64}$/);
    assert.throws(() => parseDiagnosticDiff(valid, { baseSha: BASE, headSha: HEAD, evidenceFiles: {} }), /evidence.*missing|not found/);
  });

  it("extracts the changed value regardless of assertion argument order and pairs repeated shapes once", () => {
    const diff = [
      "diff --git a/packages/engine/tests/diagnostic_parity.rs b/packages/engine/tests/diagnostic_parity.rs",
      "index 1111111..2222222 100644",
      "--- a/packages/engine/tests/diagnostic_parity.rs",
      "+++ b/packages/engine/tests/diagnostic_parity.rs",
      "@@ -10,4 +10,4 @@ fn diagnostic_expectation() {",
      "-    assert_eq!(55, report.engine_error_events);",
      "-    assert_eq!(66, report.engine_error_events);",
      `+    assert_eq!(60, report.engine_error_events); // 分歧 #9; evidence: ${EVIDENCE}`,
      `+    assert_eq!(70, report.engine_error_events); // 分歧 #9; evidence: ${EVIDENCE}`,
      " }",
      "",
    ].join("\n");
    const audit = parseDiagnosticDiff(diff, { baseSha: BASE, headSha: HEAD, evidenceFiles: EVIDENCE_FILES });
    assert.deepEqual(
      audit.expectation_changes.map(({ original_value: originalValue, new_value: newValue }) => [originalValue, newValue]),
      [["55", "60"], ["66", "70"]],
    );
  });

  it("rejects a diff that escapes the diagnostics/tests allowlist", () => {
    const escaped = diffFor(`assert_eq!(report.engine_error_events, 60); // 分歧 #9; evidence: ${EVIDENCE}`, "packages/engine/src/account.rs");
    assert.throws(() => parseDiagnosticDiff(escaped, { baseSha: BASE, headSha: HEAD, evidenceFiles: EVIDENCE_FILES }), /escaped its allowlist/);
  });

  it("reads two exact committed SHAs instead of auditing the mutable working tree", async () => {
    const fixture = await DiagnosticAuditRepositoryFixture.create();
    const { root, diagnosticsPath: file } = fixture;
    const base = await fixture.commitBase("fn diagnostic_expectation() {\n    let report = run();\n    assert_eq!(report.engine_error_events, 55);\n}\n");
    const head = await fixture.commitHead(`fn diagnostic_expectation() {\n    let report = run();\n    assert_eq!(report.engine_error_events, 60); // 分歧 #9; evidence: ${EVIDENCE}\n}\n`);

    const audit = await auditDiagnosticRange(root, base, head);
    assert.equal(audit.base_sha, base);
    assert.equal(audit.head_sha, head);
    assert.equal(audit.expectation_changes.length, 1);

    const baseTree = await git(root, ["rev-parse", `${base}^{tree}`]);
    const headTree = await git(root, ["rev-parse", `${head}^{tree}`]);
    const treeAudit = await auditDiagnosticRange(root, baseTree, headTree);
    assert.deepEqual(treeAudit.expectation_changes, audit.expectation_changes);
    assert.equal(treeAudit.boundaries.base.object_kind, "tree");
    assert.equal(treeAudit.boundaries.head.object_kind, "tree");
    assert.match(treeAudit.boundary_contract, /not commit SHAs/);
    assert.equal(treeAudit.boundaries.base.manifest_sha256,
      createHash("sha256").update(treeAudit.boundaries.base.manifest).digest("hex"));

    await writeFile(file, "working tree noise that must not enter the committed range\n");
    assert.deepEqual(await auditDiagnosticRange(root, base, head), audit);
  });

  it("rejects an invalid SHA and any committed path outside the fixed audit boundary", async () => {
    const fixture = await DiagnosticAuditRepositoryFixture.create();
    const { root } = fixture;
    const base = await fixture.commitBase("fn diagnostic_expectation() {\n    assert_eq!(report.engine_error_events, 55);\n}\n");

    await writeFile(path.join(root, "README.md"), "out-of-scope committed change\n");
    const head = await fixture.commitHead(`fn diagnostic_expectation() {\n    assert_eq!(report.engine_error_events, 60); // 分歧 #9; evidence: ${EVIDENCE}\n}\n`);

    await assert.rejects(() => auditDiagnosticRange(root, "not-a-full-sha", head), /base SHA must be a full object id/);
    await assert.rejects(() => auditDiagnosticRange(root, base, head), /escaped its allowlist: README\.md/);
  });
});
