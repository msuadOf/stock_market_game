import assert from "node:assert/strict";
import test from "node:test";
import { formatLotAmount, formatSharesAsLots, formatYuanAmount, RecursiveChineseNumberFormatter } from "./format.ts";

test("金额从万元开始按四位一组递归显示中文单位", () => {
  assert.equal(formatYuanAmount(9_999.99), "9999.99");
  assert.equal(formatYuanAmount(10_000), "1万");
  assert.equal(formatYuanAmount(300_000), "30万");
  assert.equal(formatYuanAmount(100_000_000), "1亿");
  assert.equal(formatYuanAmount(3_200_000_000_000), "3.2万亿");
  assert.equal(formatYuanAmount(10_000_000_000_000_000), "1亿亿");
  assert.equal(formatYuanAmount(-125_000_000), "-1.25亿");
});

test("交易量先由股换算为手再按同一数量级规则显示", () => {
  assert.equal(formatSharesAsLots(999_999), "9999.99");
  assert.equal(formatSharesAsLots(1_000_000), "1万");
  assert.equal(formatSharesAsLots(10_000_000_000), "1亿");
  assert.equal(formatSharesAsLots(100_000_000_000_000), "1万亿");
  assert.equal(formatLotAmount(125_000_000), "1.25亿");
});

test("格式化器显式拒绝非有限值和配置范围外数值", () => {
  const formatter = new RecursiveChineseNumberFormatter({ minimum: 0, maximum: 100, baseFractionDigits: 2 });
  assert.throws(() => formatter.format(-1), /超出范围/);
  assert.throws(() => formatter.format(101), /超出范围/);
  assert.throws(() => formatter.format(Number.NaN), /超出范围/);
});
