import assert from "node:assert/strict";
import test from "node:test";
import { createSeedDraft, changeSeedDraft, editCompanySystemDraft, parseSeedDraft, parseSessionSeed } from "./seed-draft.ts";

test("seed 完整 u64、规范十进制边界，在创建前拒绝非法 seed", () => {
  assert.equal(parseSessionSeed("18446744073709551615"), 18446744073709551615n);
  for (const text of ["-1", "18446744073709551616", "01", "+1", " 1", "", "1.0", "0x10"]) assert.throws(() => parseSessionSeed(text), /seed/);
});
test("预览 seed 原值创建；seed 编辑不覆盖 custom 非法 raw JSON", () => {
  const draft = { seed: "42", origin: "preset" as const, companySystem: '{"mode":"Simulation"}' };
  const custom = editCompanySystemDraft(draft, "unfinished {");
  assert.deepEqual(changeSeedDraft(custom, "000"), { seed: "000", origin: "custom", companySystem: "unfinished {" });
  assert.equal(changeSeedDraft(draft, "99").companySystem, draft.companySystem);
  assert.throws(() => parseSeedDraft(custom), /JSON/);
});
test("Simulation 创建草稿显式不支持，非法 seed 先于运行失败", () => {
  assert.throws(() => createSeedDraft({ mode: "Simulation" }, 42n), /Simulation/);
  assert.throws(() => parseSeedDraft({ seed: "-1", origin: "custom", companySystem: "{}" }), /seed/);
});
