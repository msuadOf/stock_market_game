import assert from "node:assert/strict";
import test from "node:test";
import {
  formatDecimalCentsAsYuan,
  formatLotAmount,
  formatSharesAsLots,
  formatYuanAmount,
  rejectionText,
  RecursiveChineseNumberFormatter,
} from "./format.ts";

test("无损十进制分值按元和中文数量级显示", () => {
  assert.equal(formatDecimalCentsAsYuan("69900"), "699");
  assert.equal(formatDecimalCentsAsYuan("6990000"), "6.99万");
  assert.equal(formatDecimalCentsAsYuan("900719925474099300"), "9007.2万亿");
  assert.throws(() => formatDecimalCentsAsYuan("9007199254740993.00"), /非负十进制整数/);
});

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

test("每一种引擎拒单原因都有明确中文说明", () => {
  assert.equal(rejectionText("PriceCageExceeded"), "委托价格超出连续竞价价格笼子");
  assert.equal(rejectionText("AuctionOrderEntryClosed"), "09:25–09:30 不接受新委托");
  assert.equal(rejectionText("ResourceLimitExceeded"), "当前未成交委托过多，请先撤单后再试");
});
