import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { parseSaveSlot } from "./save-schema.ts";

/**
 * 公司域存档三宿主契约矩阵（任务 178，checklist §6.6/§6.7）——Web/WASM 路径。
 *
 * 同一份 engine 真实生成的公司域 fixture（`company-domain-host-matrix-save.json`，
 * engine 侧测试负责与真实 wire 同步）必须被 Web 严格 parser 完整解析；
 * 缺字段/多余字段显式拒绝，不默认补齐。engine 直连与 Server/Desktop 宿主
 * 的深等往返由各宿主 crate 的同名矩阵测试锚定。
 */

function matrixFixture(): unknown {
  return JSON.parse(
    readFileSync(new URL("./fixtures/company-domain-host-matrix-save.json", import.meta.url), "utf8"),
  );
}

test("engine 公司域矩阵 fixture 可被 Web 严格 parser 完整解析", { timeout: 10000 }, () => {
  const slot = parseSaveSlot(matrixFixture());
  const actions = slot.corporate_actions;
  assert.equal(actions.registries.length, 1, "自动名册必须恰好一份");
  assert.equal(actions.dividends.length, 2, "两个结算周期产生两笔分红");
  assert.equal(actions.dividends[0].status, "Paid", "首笔分红已派息");
  assert.equal(actions.dividends[1].status, "Announced", "次笔分红公告中");
  assert.equal(actions.flat_withholding_receipts.length, 6, "简税代扣回执为 6 笔");
  assert.equal(actions.external_receipts.length, 1, "外部持有人回执恰 1 笔");
});

test("公司域 fixture 缺字段必须显式拒绝不默认兼容", { timeout: 10000 }, () => {
  const missing = matrixFixture() as { corporate_actions: Record<string, unknown> };
  delete missing.corporate_actions.flat_withholding_receipts;
  assert.throws(
    () => parseSaveSlot(missing),
    /flat_withholding_receipts/,
    "缺简税代扣回执数组必须显式拒绝并指明字段",
  );

  const unknown = matrixFixture() as { company_system: { implementation: { mode: string } } };
  unknown.company_system.implementation.mode = "Bogus";
  assert.throws(
    () => parseSaveSlot(unknown),
    /模式不支持/,
    "未知实现枚举必须显式拒绝",
  );
});
