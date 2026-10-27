import assert from "node:assert/strict";
import test from "node:test";

import { parsePlayerCompanyProposal } from "./player-proposals.ts";

const serdeVariants = [
  { CashDividend: { company: 3, gross_per_share: "5" } },
  { StockDistribution: { company: 3, kind: "BonusShares", shares_per_existing_share_micros: "1000000" } },
  { RightsOffering: { company: 3, payment_days: 2, price_per_share: "125", shares_per_existing_share_micros: "100000" } },
  { SecondaryOffering: { company: 3, price_per_share: "125", shares: "9007199254740993" } },
  { IssuerRepurchase: { company: 3, max_shares: "100", price_cap_per_share: "200", purpose: "ReduceCapital", total_budget: "30000000000", window_trading_days: 2 } },
  { ShareSplit: { company: 3, direction: "Split", ratio: "2" } },
] as const;

test("六类 Proposal 严格保留 serde 字段，结构通过仅表示格式有效", { timeout: 10000 }, () => {
  for (const input of serdeVariants) {
    assert.deepEqual(parsePlayerCompanyProposal(input), input);
  }
});

test("精确保留 u64 边界，不接受越界与非规范十进制字符串", { timeout: 10000 }, () => {
  const max = "18446744073709551615";
  const input = { ShareSplit: { company: 0, direction: "Consolidate", ratio: max } };
  assert.deepEqual(parsePlayerCompanyProposal(input), input);
  for (const ratio of ["18446744073709551616", "01", "+1", "-1", "1.0", 1, "", " 1"]) {
    assert.throws(() => parsePlayerCompanyProposal({ ShareSplit: { company: 0, direction: "Split", ratio } }), /Proposal\.ShareSplit\.ratio/);
  }
});

test("Money 严格保持规范 i64 分字符串，接受负数和零但不接受数值或越界", { timeout: 10000 }, () => {
  for (const amount of ["-9223372036854775808", "9223372036854775807", "0", "-1"]) {
    const input = { CashDividend: { company: 0, gross_per_share: amount } };
    assert.deepEqual(parsePlayerCompanyProposal(input), input);
  }
  for (const amount of ["9223372036854775808", "-9223372036854775809", "-0", "01", "+1", "1.0", 1]) {
    assert.throws(() => parsePlayerCompanyProposal({ CashDividend: { company: 0, gross_per_share: amount } }), /Proposal\.CashDividend\.gross_per_share/);
  }
});

test("CompanyId 与 u32 日期窗口字段接受上下界并拒绝类型和范围错误", { timeout: 10000 }, () => {
  assert.deepEqual(parsePlayerCompanyProposal({ RightsOffering: { company: 4294967295, price_per_share: "0", shares_per_existing_share_micros: "0", payment_days: 4294967295 } }), {
    RightsOffering: { company: 4294967295, price_per_share: "0", shares_per_existing_share_micros: "0", payment_days: 4294967295 },
  });
  for (const company of [-1, 1.5, 4294967296, "3", Number.NaN]) {
    assert.throws(() => parsePlayerCompanyProposal({ CashDividend: { company, gross_per_share: "0" } }), /Proposal\.CashDividend\.company/);
  }
  for (const days of [-1, 1.5, 4294967296, "2"]) {
    assert.throws(() => parsePlayerCompanyProposal({ RightsOffering: { company: 3, price_per_share: "0", shares_per_existing_share_micros: "0", payment_days: days } }), /Proposal\.RightsOffering\.payment_days/);
  }
  assert.deepEqual(parsePlayerCompanyProposal({ IssuerRepurchase: { company: 0, price_cap_per_share: "0", total_budget: "0", max_shares: "0", window_trading_days: 4294967295, purpose: "ValueMaintenance" } }), {
    IssuerRepurchase: { company: 0, price_cap_per_share: "0", total_budget: "0", max_shares: "0", window_trading_days: 4294967295, purpose: "ValueMaintenance" },
  });
  for (const days of [-1, 1.5, 4294967296, "2"]) {
    assert.throws(() => parsePlayerCompanyProposal({ IssuerRepurchase: { company: 3, price_cap_per_share: "0", total_budget: "0", max_shares: "0", window_trading_days: days, purpose: "ReduceCapital" } }), /Proposal\.IssuerRepurchase\.window_trading_days/);
  }
});

test("外层只允许一个已知 tag，payload 必须完整且拒绝未知字段", { timeout: 10000 }, () => {
  assert.throws(() => parsePlayerCompanyProposal([{ CashDividend: { company: 3, gross_per_share: "5" } }]), /Proposal/);
  assert.throws(() => parsePlayerCompanyProposal({ CashDividend: { company: 3, gross_per_share: "5" }, ShareSplit: { company: 3, direction: "Split", ratio: "2" } }), /Proposal/);
  assert.throws(() => parsePlayerCompanyProposal({ Unknown: { company: 3 } }), /Proposal/);
  assert.throws(() => parsePlayerCompanyProposal({ CashDividend: { company: 3 } }), /Proposal\.CashDividend\.gross_per_share/);
  assert.throws(() => parsePlayerCompanyProposal({ CashDividend: { company: 3, gross_per_share: "5", owner: 7 } }), /Proposal\.CashDividend\.owner/);
  assert.throws(() => parsePlayerCompanyProposal({ CashDividend: { company: 3, gross_per_share: "5", approved_on: "2026-10-27" } }), /Proposal\.CashDividend\.approved_on/);
});

test("嵌套领域 enum 闭包严格拒绝未知值", { timeout: 10000 }, () => {
  assert.deepEqual(parsePlayerCompanyProposal({ StockDistribution: { company: 3, kind: "CapitalReserveConversion", shares_per_existing_share_micros: "1" } }), {
    StockDistribution: { company: 3, kind: "CapitalReserveConversion", shares_per_existing_share_micros: "1" },
  });
  for (const purpose of ["EmployeeIncentive", "ConvertibleConversion", "ValueMaintenance"]) {
    assert.deepEqual(parsePlayerCompanyProposal({ IssuerRepurchase: { company: 3, price_cap_per_share: "1", total_budget: "1", max_shares: "1", window_trading_days: 1, purpose } }), {
      IssuerRepurchase: { company: 3, price_cap_per_share: "1", total_budget: "1", max_shares: "1", window_trading_days: 1, purpose },
    });
  }
  assert.deepEqual(parsePlayerCompanyProposal({ ShareSplit: { company: 3, direction: "Consolidate", ratio: "2" } }), {
    ShareSplit: { company: 3, direction: "Consolidate", ratio: "2" },
  });
  assert.throws(() => parsePlayerCompanyProposal({ StockDistribution: { company: 3, kind: "Unknown", shares_per_existing_share_micros: "1" } }), /Proposal\.StockDistribution\.kind/);
  assert.throws(() => parsePlayerCompanyProposal({ IssuerRepurchase: { company: 3, price_cap_per_share: "1", total_budget: "1", max_shares: "1", window_trading_days: 1, purpose: "Unknown" } }), /Proposal\.IssuerRepurchase\.purpose/);
  assert.throws(() => parsePlayerCompanyProposal({ ShareSplit: { company: 3, direction: "Unknown", ratio: "2" } }), /Proposal\.ShareSplit\.direction/);
});

test("解析不修改输入或伪造受理信息", { timeout: 10000 }, () => {
  const input = { ShareSplit: { company: 3, direction: "Split", ratio: "0" } };
  const before = structuredClone(input);
  const output = parsePlayerCompanyProposal(input);
  assert.deepEqual(input, before);
  assert.deepEqual(output, before);
  assert.equal("receipt" in output, false);
  assert.equal("outcome" in output, false);
  assert.equal("accepted" in output, false);
});

// 以下规格输入冻结自已经核对 SHA256 的真实 Rust serde fixture。
const fixture: ReadonlyArray<Readonly<Record<string, Readonly<Record<string, unknown>>>>> = [
  {
    "CashDividend": {
      "company": 3,
      "gross_per_share": "5"
    }
  },
  {
    "StockDistribution": {
      "company": 3,
      "kind": "BonusShares",
      "shares_per_existing_share_micros": "1000000"
    }
  },
  {
    "RightsOffering": {
      "company": 3,
      "payment_days": 2,
      "price_per_share": "125",
      "shares_per_existing_share_micros": "100000"
    }
  },
  {
    "SecondaryOffering": {
      "company": 3,
      "price_per_share": "125",
      "shares": "9007199254740993"
    }
  },
  {
    "IssuerRepurchase": {
      "company": 3,
      "max_shares": "100",
      "price_cap_per_share": "200",
      "purpose": "ReduceCapital",
      "total_budget": "30000000000",
      "window_trading_days": 2
    }
  },
  {
    "ShareSplit": {
      "company": 3,
      "direction": "Split",
      "ratio": "2"
    }
  }
];
test("直接读取真实 Rust serde 六类 fixture 并无损解析", {timeout:10000}, () => {
 assert.equal(fixture.length,6);
 for(const input of fixture) assert.deepEqual(parsePlayerCompanyProposal(input), input);
});
test("全部变体逐字段缺键多键与null拒绝", {timeout:10000}, () => {
 for(const input of fixture){
  const [tag]=Object.keys(input);const payload=input[tag];
  for(const field of Object.keys(payload)){
   const missing: Record<string, unknown>=structuredClone(payload);delete missing[field];
   assert.throws(()=>parsePlayerCompanyProposal({[tag]:missing}),field);
   assert.throws(()=>parsePlayerCompanyProposal({[tag]:{...payload,[field]:null}}),field);
  }
  for(const extra of ["account","owner","approved_on","receipt","outcome"]){
   assert.throws(()=>parsePlayerCompanyProposal({[tag]:{...payload,[extra]:"0"}}),extra);
  }
 }
});
test("所有u64及窗口字段遵守零和端点；Money按输入形状接受负值", {timeout:10000}, () => {
 const u64=new Set(["shares_per_existing_share_micros","shares","max_shares","ratio"]);
 const windows=new Set(["payment_days","window_trading_days"]);
 const money=new Set(["gross_per_share","price_per_share","price_cap_per_share","total_budget"]);
 for(const input of fixture){const [tag]=Object.keys(input);const payload=input[tag];
  for(const field of Object.keys(payload)){
   const values=u64.has(field)?["0","18446744073709551615"]:windows.has(field)?[0,4294967295]:money.has(field)?["-9223372036854775808","9223372036854775807","0"]:[];
   for(const value of values){const patched={[tag]:{...payload,[field]:value}};assert.deepEqual(parsePlayerCompanyProposal(patched),patched);}
   const invalid=u64.has(field)?["18446744073709551616","01",0]:windows.has(field)?[-1,4294967296,"0"]:money.has(field)?["9223372036854775808","-9223372036854775809","-0",0]:[];
   for(const value of invalid)assert.throws(()=>parsePlayerCompanyProposal({[tag]:{...payload,[field]:value}}),field);
  }
 }
});

// Rust serde_json 将 raw -0 视为浮点负零，拒绝解码为 u32；这里只固定数值输入，不增加 raw JSON API。
test("所有u32身份和窗口字段拒绝负零并保留普通零", { timeout: 10000 }, () => {
  for (const input of fixture) {
    const [tag] = Object.keys(input); const payload = input[tag];
    const fields = ["company", ...Object.keys(payload).filter(field => field === "payment_days" || field === "window_trading_days")];
    for (const field of fields) {
      assert.throws(() => parsePlayerCompanyProposal({ [tag]: { ...payload, [field]: -0 } }), field);
      const valid = { [tag]: { ...payload, [field]: 0 } };
      const result = parsePlayerCompanyProposal(valid);
      assert.deepEqual(result, valid);
    }
  }
});
