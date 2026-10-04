import assert from "node:assert/strict";
import test from "node:test";
import { parseMoney, moneyFromBigInt, addMoney, subtractMoney, multiplyMoney, compareMoney, centsToYuanText, yuanTextToCents, divideMoneyBankers, moneyToChartNumber, ratioMoney } from "./money.ts";

test("Money 严格接受规范 i64 分字符串并保留安全整数外精度", { timeout: 10000 }, () => {
  for (const value of ["0", "-1", "9007199254740993", "9223372036854775807", "-9223372036854775808"]) assert.equal(parseMoney(value), value);
  for (const value of [0, 100, "-0", "+1", "01", "1.0", "1e3", " 1", "1\n", "", "9223372036854775808", "-9223372036854775809"]) assert.throws(() => parseMoney(value), /金额/);
});

test("Money 算术保留整数精度并拒绝 i64 溢出和非整数股数", { timeout: 10000 }, () => {
  assert.equal(addMoney("9007199254740993", "1"), "9007199254740994");
  assert.equal(subtractMoney("0", "1"), "-1");
  assert.equal(multiplyMoney("9007199254740993", 2), "18014398509481986");
  assert.equal(compareMoney("10", "2"), 1);
  assert.throws(() => addMoney("9223372036854775807", "1"), /金额/);
  assert.throws(() => moneyFromBigInt(-9223372036854775809n), /金额/);
  assert.throws(() => multiplyMoney("100", 0.5), /整数/);
});

test("元输入和元输出不经浮点账务转换", { timeout: 10000 }, () => {
  assert.equal(centsToYuanText("9007199254740993"), "90071992547409.93");
  assert.equal(centsToYuanText("-1"), "-0.01");
  assert.equal(yuanTextToCents("10000000000"), "1000000000000");
  assert.equal(yuanTextToCents("-0.01"), "-1");
  assert.equal(yuanTextToCents("0.1"), "10");
  for (const value of ["1.001", "1e2", "01", " 1", "-0", "-0.00", "92233720368547758.08"]) assert.throws(() => yuanTextToCents(value));
});

test("成本除法按银行家舍入并支持负金额", { timeout: 10000 }, () => {
  assert.equal(divideMoneyBankers("5", 2), "2");
  assert.equal(divideMoneyBankers("7", 2), "4");
  assert.equal(divideMoneyBankers("-5", 2), "-2");
  assert.equal(divideMoneyBankers("-7", 2), "-4");
  assert.throws(() => divideMoneyBankers("1", 0));
  assert.equal(moneyToChartNumber("123"), 123);
  assert.equal(ratioMoney("1", "4"), 0.25);
  assert.throws(() => ratioMoney("1", "0"));
});
