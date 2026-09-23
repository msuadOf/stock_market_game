import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = path.resolve(import.meta.dirname, "..");
const checker = path.join(root, "scripts/check-documentation-contract.mjs");
const documents = ["README.md", "docs/diagnostics.md", "docs/causal-diagnostics.md", "docs/trading-rules.md"];

function fixture(extra) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "doc-contract-test-"));
  for (const document of documents) {
    const destination = path.join(directory, document);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(root, document), destination);
  }
  fs.appendFileSync(path.join(directory, "README.md"), `\n${extra}\n`);
  return directory;
}

const cases = [
  ["Weston headless universally supports all compositors.", "universal compositor claim"],
  ["Weston is compatible with each Wayland compositor.", "each compositor claim"],
  ["Wayland supports every compositor.", "all compositor claim"],
  ["Wayland 支持所有合成器。", "Chinese universal compositor claim"],
  ["Weston works with any Wayland compositor.", "any compositor claim"],
  ["Wayland 兼容各个合成器。", "各个 compositor claim"],
  ["Weston screenshot passed.", "unqualified screenshot success"],
  ["The Weston image check is green.", "generic green image success"],
  ["Wayland pixel capture valid.", "unqualified pixel success"],
  ["Wayland rendering is green.", "generic rendering success"],
  ["Wayland pixels are correct.", "generic pixel correctness"],
  ["Wayland 截图成功。", "Chinese screenshot success"],
  ["C06 completed with historical validation.", "English C06 completion"],
  ["Historical calibration and holdout validation are now complete.", "generic English calibration completion"],
  ["C06 已完成。", "Chinese C06 completion"],
  ["The calibration holdout is finalized.", "finalized calibration completion"],
  ["C06 is resolved.", "resolved C06 completion"],
  ["The old decision clock omits lunch.", "English lunch omission"],
  ["The observation timeline skips the midday break.", "generic English midday-break omission"],
  ["旧决策时钟省略午休。", "Chinese lunch omission"],
  ["The observation clock leaves out noon break.", "alternate English lunch omission"],
  ["观察时间线遗漏中午休息。", "alternate Chinese lunch omission"],
  ["The archive is 8 MiB.", "detached 8 MiB claim is rejected by coherence requirements"],
  ["MAX_LOAD_BODY_BYTES is 8 MiB.", "detached MAX body-limit claim"],
  ["server_body_fit=false for a payload.", "detached server fit claim"],
  ["The body limit is 8,388,608 bytes.", "detached exact body-limit claim"],
  ["The transport cap is 8192 KiB.", "detached normalized body-limit claim"],
];

for (const [claim, description] of cases) {
  test(`rejects ${description}`, () => {
    const directory = fixture(claim);
    const result = spawnSync(process.execPath, [checker, directory], { encoding: "utf8" });
    assert.notEqual(result.status, 0, `${claim} unexpectedly passed`);
  });
}
