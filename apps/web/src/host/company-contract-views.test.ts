import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, test } from "node:test";
import {
  parseCompanyCapabilities,
  parseFlatWithholdingReceipts,
  parseOwnerRightsOfferings,
  parsePeriodChangeExplanation,
} from "./company-contract-views.ts";

const CAPABILITIES = {
  revenue: true,
  net_income: true,
  equity: true,
  cash_flow: true,
  full_financial_statements: true,
  cash_settlement: false,
  unsupported_reason: "汇总财务已接通；共同股本行为的实际投资者结算仍待接线",
  par_value_per_share: { Unavailable: { reason: "尚无送转／拆股绑定的每股面值事实" } },
  issued_shares: "10000000",
  registered_capital: { Available: { amount_yuan: "100000.00" } },
  distributable_profit: {
    Available: {
      accumulated_after_loss_yuan: "12000.00",
      statutory_reserve_yuan: "500.00",
      available_for_distribution_yuan: "11500.00",
      reserve_basis_year: null,
    },
  },
  active_plans: [
    {
      kind: "RightsOffering",
      identity: "rights-2030",
      stage: "Entitled",
      key_dates: [
        { label: "股权登记日", date: "2030-01-04" },
        { label: "缴款截止日", date: "2030-01-11" },
      ],
    },
  ],
  action_readiness: [
    { kind: "CashDividend", ready: true, blockers: [] },
    { kind: "StockDistribution", ready: true, blockers: [] },
    { kind: "RightsOffering", ready: true, blockers: [] },
    { kind: "IssuerRepurchase", ready: false, blockers: ["新局未启用发行人回购机制（issuer_repurchase_enabled=false）"] },
    { kind: "ShareSplit", ready: true, blockers: [] },
  ],
  owner_rights: [
    {
      event_id: "rights-2030",
      stock: "600101",
      stage: "Entitled",
      payment_window: "Open",
      entitled_shares: "1500000",
      open_subscription_remaining: null,
    },
  ],
};

describe("parseCompanyCapabilities", () => {
  test("接受完整契约形状并保留显式不可用原因", () => {
    const view = parseCompanyCapabilities(CAPABILITIES);
    assert.equal(view.issued_shares, "10000000");
    if (!("Available" in view.registered_capital)) throw new Error("注册资本应可用");
    assert.equal(view.registered_capital.Available.amount_yuan, "100000.00");
    if (!("Unavailable" in view.par_value_per_share)) throw new Error("面值应不可用");
    assert.equal(
      view.par_value_per_share.Unavailable.reason,
      "尚无送转／拆股绑定的每股面值事实",
    );
    assert.equal(view.active_plans[0].key_dates[1].date, "2030-01-11");
    assert.deepEqual(view.owner_rights[0].entitled_shares, "1500000");
    assert.equal(view.action_readiness[3].blockers[0].includes("未启用"), true);
  });

  test("拒绝未知字段（严格形状）", () => {
    assert.throws(() => parseCompanyCapabilities({ ...CAPABILITIES, extra: 1 }), /不是允许字段/);
  });

  test("拒绝五类条件条目数量不为五", () => {
    const broken = { ...CAPABILITIES, action_readiness: CAPABILITIES.action_readiness.slice(0, 4) };
    assert.throws(() => parseCompanyCapabilities(broken), /action_readiness 必须恰好五类/);
  });

  test("拒绝空不可用原因（不可用必须显式给原因）", () => {
    const broken = {
      ...CAPABILITIES,
      registered_capital: { Unavailable: { reason: "  " } },
    };
    assert.throws(() => parseCompanyCapabilities(broken), /不可用原因必须非空/);
  });

  test("拒绝非规范十进制股本字符串", () => {
    assert.throws(() => parseCompanyCapabilities({ ...CAPABILITIES, issued_shares: 10_000_000 }), /issued_shares/);
  });

  // F 修复轮 blocker-1 回归锁：fixture 由 engine 真实 serde 序列化产出
  // （`capabilities_wire_u64_fields_serialize_as_canonical_decimal_strings`
  // 以 UPDATE_COMPANY_WIRE_FIXTURES=1 重生成并逐字段校验同步），本测试把
  // engine 真实 wire 喂给严格 parser——手写 fixture 曾掩盖「ts 类型为 string
  // 而 serde 实际输出 number」的断裂，真机 companyCapabilities 恒解析失败。
  test("engine 真实序列化 wire 经严格 parser 完整往返（u64 股数为规范字符串）", () => {
    const wire: unknown = JSON.parse(
      readFileSync(new URL("../save/fixtures/company-capabilities-wire.json", import.meta.url), "utf8"),
    ) as unknown;
    const view = parseCompanyCapabilities(wire);
    assert.equal(typeof (wire as { issued_shares: unknown }).issued_shares, "string");
    assert.match(view.issued_shares, /^(0|[1-9]\d*)$/);
    assert.equal(view.issued_shares, (wire as { issued_shares: string }).issued_shares);
    // 金额与权利摘要按 engine 实际口径原样保留（不填零、不转 number）。
    const wireRegistered = (wire as { registered_capital: { Available?: { amount_yuan: string } } }).registered_capital;
    if (!("Available" in view.registered_capital) || wireRegistered.Available === undefined) {
      throw new Error("fixture 的注册资本应为可用（Available）");
    }
    assert.equal(view.registered_capital.Available.amount_yuan, wireRegistered.Available.amount_yuan);
    assert.equal(view.owner_rights[0]?.entitled_shares, (wire as { owner_rights: Array<{ entitled_shares: string | null }> }).owner_rights[0]?.entitled_shares);
    // 五类 readiness 逐类保留 engine 判定（含显式 blockers 原文）。
    assert.equal(view.action_readiness.length, 5);
    assert.equal(view.action_readiness[3]?.blockers.length, (wire as { action_readiness: Array<{ blockers: string[] }> }).action_readiness[3]?.blockers.length);
  });
});

const OWNER_RIGHTS = [
  {
    event_id: "rights-2030",
    stock: "600101",
    issuer: "C-600101",
    stage: "Entitled",
    payment_window: "Open",
    price_per_share: "10",
    payment_start_on: "2030-01-07",
    payment_deadline_on: "2030-01-11",
    ex_rights_on: "2030-01-14",
    settlement_on: "2030-01-15",
    owner_entitlement: { rights_shares: "1500000", lock_until: null },
    open_subscription_remaining_shares: null,
    queued_subscription: { requested_shares: "1500000", submitted_on: "2030-01-08" },
    settled_subscription: null,
  },
];

describe("parseOwnerRightsOfferings", () => {
  test("接受本人权利视图并保留排队认购", () => {
    const [view] = parseOwnerRightsOfferings(OWNER_RIGHTS);
    assert.equal(view.event_id, "rights-2030");
    assert.equal(view.owner_entitlement?.rights_shares, "1500000");
    assert.equal(view.queued_subscription?.requested_shares, "1500000");
    assert.equal(view.settled_subscription, null);
  });

  test("已结算认购必须满足 划扣+弃配=申请", () => {
    const broken = [
      {
        ...OWNER_RIGHTS[0],
        queued_subscription: null,
        settled_subscription: {
          requested_shares: "100",
          paid_shares: "60",
          paid_amount: "600",
          waived_shares: "30",
        },
      },
    ];
    assert.throws(() => parseOwnerRightsOfferings(broken), /划扣\+弃配必须等于申请认购数/);
  });

  test("拒绝未知阶段枚举值", () => {
    const broken = [{ ...OWNER_RIGHTS[0], stage: "Settled" }];
    assert.throws(() => parseOwnerRightsOfferings(broken), /stage/);
  });

  test("拒绝非正具名权利", () => {
    const broken = [
      {
        ...OWNER_RIGHTS[0],
        owner_entitlement: { rights_shares: "0", lock_until: null },
      },
    ];
    assert.throws(() => parseOwnerRightsOfferings(broken), /具名权利必须为正/);
  });
});

const EXPLANATION = {
  previous: { revenue: "1010.00", fixed_expense: "1100.00", variable_expense: "0.00" },
  cycle: "Monthly",
  environment_change_bp: 0,
  demand_contribution_bp: -120,
  revenue_segments: [{ annual_growth_bp: 1268, months: 8 }],
  fixed_expense_segments: [],
  variable_expense_segments: [],
  revenue_noise_bp: 0,
  fixed_expense_noise_bp: 0,
  variable_expense_noise_bp: 0,
  restart_revenue: null,
  restart_source: null,
};

describe("parsePeriodChangeExplanation", () => {
  test("接受解释并保留元字符串金额与 bp 整数", () => {
    const view = parsePeriodChangeExplanation(EXPLANATION);
    assert.equal(view.previous.revenue, "1010.00");
    assert.equal(view.demand_contribution_bp, -120);
    assert.equal(view.revenue_segments[0].months, 8);
    assert.equal(view.restart_revenue, null);
  });

  test("复业事实保留非空金额与来源", () => {
    const view = parsePeriodChangeExplanation({
      ...EXPLANATION,
      restart_revenue: "5000.00",
      restart_source: "复业场景",
    });
    assert.equal(view.restart_revenue, "5000.00");
    assert.equal(view.restart_source, "复业场景");
  });

  test("拒绝非整数 bp 值", () => {
    assert.throws(
      () => parsePeriodChangeExplanation({ ...EXPLANATION, environment_change_bp: 1.5 }),
      /environment_change_bp/,
    );
  });

  test("拒绝未知结算周期枚举", () => {
    assert.throws(
      () => parsePeriodChangeExplanation({ ...EXPLANATION, cycle: "Weekly" }),
      /cycle/,
    );
  });
});

const RECEIPTS = [
  {
    payment_id: "pay-1",
    plan_id: "plan-1",
    account: "0",
    paid_on: "2030-01-08",
    gross: "50",
    rate_bp: 1000,
    withheld: "5",
  },
];

describe("parseFlatWithholdingReceipts", () => {
  test("接受本人回执并保留分字符串金额", () => {
    const [receipt] = parseFlatWithholdingReceipts(RECEIPTS);
    assert.equal(receipt.gross, "50");
    assert.equal(receipt.withheld, "5");
    assert.equal(receipt.rate_bp, 1000);
  });

  test("拒绝非规范账户字符串", () => {
    assert.throws(
      () => parseFlatWithholdingReceipts([{ ...RECEIPTS[0], account: "abc" }]),
      /规范账户/,
    );
  });

  test("拒绝非正税前应得", () => {
    assert.throws(
      () => parseFlatWithholdingReceipts([{ ...RECEIPTS[0], gross: "0" }]),
      /金额非法/,
    );
  });
});
