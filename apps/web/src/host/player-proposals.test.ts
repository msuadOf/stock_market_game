import assert from "node:assert/strict";
import { describe, test } from "node:test";
import {
  buildCashDividendProposal,
  buildIssuerRepurchaseProposal,
  buildRightsOfferingProposal,
  buildSecondaryOfferingProposal,
  buildShareSplitProposal,
  buildStockDistributionProposal,
  parsePlayerProposalOutcome,
  parseSimplePreferencesValue,
} from "./player-proposals.ts";

describe("parsePlayerProposalOutcome", () => {
  test("受理结果解析为显式 accepted 分支", () => {
    const view = parsePlayerProposalOutcome({
      Accepted: { receipt: { kind: "CashDividend", identity: "player-proposal:C-600888:dividend:2030-01-28:0", approved_on: "2030-01-28", announced_on: "2030-01-28" } },
    });
    assert.deepEqual(view, {
      outcome: "accepted",
      kind: "CashDividend",
      identity: "player-proposal:C-600888:dividend:2030-01-28:0",
      approved_on: "2030-01-28",
      announced_on: "2030-01-28",
    });
  });

  test("无持仓拒绝解析为显式 no_holding 分支", () => {
    const view = parsePlayerProposalOutcome({
      NoHolding: { company: "C-600888", stock: "600888", detail: "账户 AccountId(0) 对发行人上市证券持仓为 0" },
    });
    assert.equal(view.outcome, "no_holding");
    if (view.outcome === "no_holding") {
      assert.equal(view.company, "C-600888");
      assert.equal(view.stock, "600888");
      assert.ok(view.detail.includes("持仓"));
    }
  });

  test("制度拒绝解析为显式 institutional_rejection 分支并保留四分类", () => {
    const view = parsePlayerProposalOutcome({
      InstitutionalRejection: { kind: "RightsOffering", detail: "invalid setup: 本局未启用配股／增发机制（新局开关 rights_offering_enabled=false），显式拒绝", class: "UnsupportedOperation" },
    });
    assert.deepEqual(view, {
      outcome: "institutional_rejection",
      kind: "RightsOffering",
      detail: "invalid setup: 本局未启用配股／增发机制（新局开关 rights_offering_enabled=false），显式拒绝",
      class: "UnsupportedOperation",
    });
  });

  test("未知变体、多键与缺字段形状显式拒绝", () => {
    assert.throws(() => parsePlayerProposalOutcome({ Unknown: {} }), /单键枚举/);
    assert.throws(() => parsePlayerProposalOutcome({ Accepted: { receipt: { kind: "CashDividend" } } as never }), /必填字段/);
    assert.throws(() => parsePlayerProposalOutcome({ InstitutionalRejection: { kind: "CashDividend", detail: " ", class: "BusinessCondition" } }), /拒绝原因必须非空/);
    assert.throws(() => parsePlayerProposalOutcome({ NoHolding: { company: "C-600888", stock: "600888", detail: "x", extra: 1 } }), /NoHolding/);
  });
});

describe("提案构造器的最小参数域校验", () => {
  test("现金分红金额换算为规范分字符串", () => {
    assert.deepEqual(buildCashDividendProposal("C-600888", "0.10"), { CashDividend: { company: "C-600888", gross_per_share: "10" } });
    assert.throws(() => buildCashDividendProposal("C-600888", "0"), /正数/);
    assert.throws(() => buildCashDividendProposal("C-600888", "0.001"), /两位小数/);
    assert.throws(() => buildCashDividendProposal(" ", "0.10"), /64 字符/);
  });

  test("送转比例与拆股比例域校验", () => {
    assert.deepEqual(buildStockDistributionProposal("C-600888", "BonusShares", "1000000"), { StockDistribution: { company: "C-600888", kind: "BonusShares", shares_per_existing_share_micros: "1000000" } });
    assert.throws(() => buildStockDistributionProposal("C-600888", "BonusShares", "0"), /≥ 1/);
    assert.deepEqual(buildShareSplitProposal("C-600888", "Split", "2"), { ShareSplit: { company: "C-600888", direction: "Split", ratio: "2" } });
    assert.throws(() => buildShareSplitProposal("C-600888", "Split", "1"), /≥ 2/);
  });

  test("配股/增发价格、比例与缴款期校验", () => {
    assert.deepEqual(buildRightsOfferingProposal("C-600888", "5.00", "100000", 1), {
      RightsOffering: { company: "C-600888", price_per_share: "500", shares_per_existing_share_micros: "100000", payment_days: 1 },
    });
    assert.throws(() => buildRightsOfferingProposal("C-600888", "0", "100000", 1), /正数/);
    assert.throws(() => buildRightsOfferingProposal("C-600888", "5.00", "100000", 0), /1\.\.=255/);
    assert.deepEqual(buildSecondaryOfferingProposal("C-600888", "5.00", "100"), { SecondaryOffering: { company: "C-600888", price_per_share: "500", shares: "100" } });
    assert.throws(() => buildSecondaryOfferingProposal("C-600888", "5.00", "0"), /≥ 1/);
  });

  test("回购提案字段校验", () => {
    const proposal = buildIssuerRepurchaseProposal("C-600888", "9.00", "900.00", "100", 5, "ValueMaintenance");
    assert.deepEqual(proposal, {
      IssuerRepurchase: { company: "C-600888", price_cap_per_share: "900", total_budget: "90000", max_shares: "100", window_trading_days: 5, purpose: "ValueMaintenance" },
    });
    assert.throws(() => buildIssuerRepurchaseProposal("C-600888", "9.00", "0.00", "100", 5, "ValueMaintenance"), /正数/);
    assert.throws(() => buildIssuerRepurchaseProposal("C-600888", "9.00", "900.00", "100", 0, "ValueMaintenance"), /1\.\.=255/);
  });
});

describe("parseSimplePreferencesValue", () => {
  test("未配置偏好（两项皆空）与完整偏好均可解析", () => {
    assert.deepEqual(parseSimplePreferencesValue({ cash_dividend: null, stock_distribution: null }), { cash_dividend: null, stock_distribution: null });
    const parsed = parseSimplePreferencesValue({
      cash_dividend: { target_payout_bp: 3000, min_distributable_profit: "100000000", cycles_between_proposals: 1 },
      stock_distribution: null,
    });
    assert.equal(parsed.cash_dividend?.target_payout_bp, 3000);
  });

  test("域越界与形状漂移显式拒绝", () => {
    assert.throws(() => parseSimplePreferencesValue({ cash_dividend: { target_payout_bp: 0, min_distributable_profit: "1", cycles_between_proposals: 1 }, stock_distribution: null }), /不小于 1/);
    assert.throws(() => parseSimplePreferencesValue({ cash_dividend: { target_payout_bp: 10001, min_distributable_profit: "1", cycles_between_proposals: 1 }, stock_distribution: null }), /1\.\.=10000/);
    assert.throws(() => parseSimplePreferencesValue({ cash_dividend: null }), /stock_distribution/);
    assert.throws(() => parseSimplePreferencesValue({ cash_dividend: null, stock_distribution: { min_distributable_profit: "0", shares_per_existing_share_micros: 1, max_cumulative_expansion_micros: 1, cycles_between_proposals: 1 } }), /正数/);
  });
});

// —— Worker 层请求字段校验（同 S2 批 wasm-worker 契约测试模式）——
const posted: unknown[] = [];
let messageListener: ((event: MessageEvent<Record<string, unknown> & { type: string }>) => void) | null = null;
const originalSelf = Object.getOwnPropertyDescriptor(globalThis, "self");
Object.defineProperty(globalThis, "self", {
  configurable: true,
  value: {
    postMessage(message: unknown) {
      posted.push(message);
    },
    addEventListener(_type: "message", listener: typeof messageListener) {
      messageListener = listener;
    },
  },
});
await import("./wasm-worker.ts");
if (originalSelf === undefined) delete (globalThis as { self?: unknown }).self;
else Object.defineProperty(globalThis, "self", originalSelf);

test("玩家提案消息先做请求字段校验：非对象提案走 operationError 显式拒绝", () => {
  posted.length = 0;
  messageListener!({ data: { type: "proposeCompanyAction", generation: 0, requestId: 7, proposal: "not-an-object" } } as MessageEvent);
  assert.deepEqual(posted, [{ type: "operationError", requestId: 7, generation: 0, message: "玩家提案必须是对象" }]);
});

test("偏好编辑消息先做公司身份校验：空白公司身份走 operationError 显式拒绝", () => {
  posted.length = 0;
  messageListener!({ data: { type: "setSimplePreferences", generation: 0, requestId: 8, company: " ", preferences: { cash_dividend: null, stock_distribution: null } } } as MessageEvent);
  assert.deepEqual(posted, [{ type: "operationError", requestId: 8, generation: 0, message: "偏好编辑公司身份必须是非空且不超过 64 字符的字符串" }]);
});
