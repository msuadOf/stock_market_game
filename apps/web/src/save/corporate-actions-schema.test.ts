import assert from "node:assert/strict"
import test from "node:test"

import { parseSessionCorporateActions } from "./schema/corporate-actions.ts"
import type { AcquisitionSource, Context, ShareRestriction } from "./schema/corporate-actions.ts"
import { parseSaveSnapshot, validateCashExReferenceFacts } from "./schema/save-snapshot.ts"

// 送转测试内深拷贝回执的可变形状：与 schema 的 ShareDayReceipt 同构，但允许
// 测试改写 changes 与 acquisition.source（schema 导出类型为 readonly），并保持
// acquisition 可空、source 为完整 AcquisitionSource 联合，供负例直接构造。
type MutableShareDayReceipt = {
  request: {
    event_id: string
    day: string
    scope: "PublicMarket" | { NonTradingTransfer: { basis: string } }
    changes: { holder: { Account: string } | { External: string }; change: string; acquisition: { lot_id: string; source: AcquisitionSource; restriction: ShareRestriction } | null }[]
  }
  disposals: unknown[]
}

const emptyContext = {
  issuers: {},
  setup: { stocks: [] },
  snapshot: { markets: {}, accounts: {} },
  currentDate: "2030-01-01",
}

function registryContext(issuedShares = "10", exchange: "Shanghai" | "Shenzhen" = "Shanghai") {
  return {
    issuers: { "C-600101": { listed_stock: "600101", issued_shares: issuedShares } },
    setup: { stocks: [{ code: "600101", total_shares: issuedShares, exchange, tick: "1" }] },
    snapshot: { markets: {}, accounts: {} },
    currentDate: "2030-01-03",
  }
}

function validRegistry(issuedShares = "10") {
  return {
    stock: "600101", issuer: "C-600101", issued_shares: issuedShares, settled_on: "2030-01-03",
    issuer_repurchase_account: null,
    holdings: [{ holder: { External: "holder-a" }, lots: [{ id: "lot-a", qty: issuedShares, acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }] }],
    receipts: [], registrations: [],
  }
}

test("公司行为状态要求全部十个显式数组，空数组表示已配置的空状态", () => {
  assert.deepEqual(parseSessionCorporateActions({ registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, emptyContext), {
    registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [],
  })
  const allKeys = ["registries", "dividends", "dividend_tax_books", "account_gross_receipts", "external_receipts", "applied_ex_reference_groups", "stock_distributions", "share_splits", "rights_offerings", "rights_subscription_queue", "rejected_rights_subscriptions", "issuer_repurchases"] as const
  for (const absent of allKeys) {
    const missing: Record<string, unknown> = {}
    for (const key of allKeys) if (key !== absent) missing[key] = []
    assert.throws(() => parseSessionCorporateActions(missing, emptyContext), new RegExp(`corporate_actions\\.${absent}.*必填`), `缺少 ${absent} 必须显式拒绝`)
  }
  assert.throws(() => parseSessionCorporateActions({ registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [], extra: [] }, emptyContext), /corporate_actions\.extra/)
})

test("公司行为状态要求十个显式数组并严格校验股息税账序列", () => {
  const taxBook = {
    operation_seq: 0, account: "0", stock: "600101", profile: "IndividualPublicMarket",
    opened_on: "2030-01-03", opening_lots: [], settled_on: "2030-01-03", lots: [], days: [], dividends: [], collections: [], redenominations: [],
  }
  const lot = { id: "account-lot", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const registry = { ...validRegistry(), holdings: [{ holder: { Account: "0" }, lots: [lot] }, { holder: { External: "holder-a" }, lots: [] }] }
  const actions = { registries: [registry], dividends: [], dividend_tax_books: [taxBook], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  const context = { ...registryContext(), snapshot: { markets: {}, accounts: { "0": { positions: { "600101": { qty: 10 } } } } } }
  assert.deepEqual(parseSessionCorporateActions(actions, context).dividend_tax_books, [taxBook])
  const { dividend_tax_books: _taxBooks, ...missingTaxBooks } = actions
  assert.throws(() => parseSessionCorporateActions(missingTaxBooks, context), /dividend_tax_books.*必填/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [{ ...taxBook, operation_seq: Number.MAX_SAFE_INTEGER + 1 }] }, context), /operation_seq.*安全整数/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [{ ...taxBook, days: [{ operation_seq: 1, event_id: "day-a", day: "2030-01-04", net_change: "0", acquisition: null, dispositions: [] }] }] }, context), /操作序列与事实数量不一致/)
})

test("账户分红到账税身份必须与同账户同证券税账配置一致", () => {
  const holder = { Account: "0" }
  const lot = { id: "account-lot", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const registration = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-04", issued_shares: "10", settled_receipts: "0", issuer_repurchase_account: null, holdings: [{ holder, lots: [lot] }] }
  const registry = { ...validRegistry(), settled_on: "2030-01-08", holdings: [{ holder, lots: [lot] }], registrations: [registration] }
  const payment = { payment_id: "payment-a", paid_on: "2030-01-06", within_six_month_deadline: true, outcomes: [{ Paid: { holder, amount: "10" } }] }
  const dividend = {
    plan: { plan_id: "plan-a", issuer: "C-600101", stock: "600101", exchange: "sse", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-04", ex_dividend_on: "2030-01-05", payable_on: "2030-01-06", gross_per_share: "1", distributable_amount: "10" },
    status: "Paid", registration, entitlements: [{ holder, shares: "10", gross: "10" }], paid: [[holder, "10"]], failures: [], payments: [payment],
  }
  const taxLot = { id: "tax-account-lot", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, class: "PublicMarket" }
  const taxBook = { operation_seq: 0, account: "0", stock: "600101", profile: "IndividualPublicMarket", opened_on: "2030-01-08", opening_lots: [taxLot], settled_on: "2030-01-08", lots: [taxLot], days: [], dividends: [], collections: [], redenominations: [] }
  const receipt = { payment_id: "payment-a", plan_id: "plan-a", account: "0", paid_on: "2030-01-06", gross: "10", tax_status: "IndividualPublicMarket" }
  const actions = { registries: [registry], dividends: [dividend], dividend_tax_books: [taxBook], account_gross_receipts: [receipt], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  const context = { ...registryContext(), currentDate: "2030-01-08", snapshot: { markets: {}, accounts: { "0": { positions: { "600101": { qty: 10 } } } } } }
  assert.equal(parseSessionCorporateActions(actions, context).account_gross_receipts[0]?.tax_status, "IndividualPublicMarket")
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [] }, context), /税身份与股息税账配置不一致/)
})

test("公司行为状态拒绝裸数股数并要求无损 u64 字符串", () => {
  const registry = {
    stock: "600101", issuer: "C-600101", issued_shares: Number.MAX_SAFE_INTEGER + 1,
    issuer_repurchase_account: null,
    settled_on: "2030-01-01", holdings: [], receipts: [], registrations: [],
  }
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, emptyContext), /issued_shares.*字符串/)
})

test("股东名册保留 u64 字符串并验证身份、FIFO 和股数守恒", () => {
  const maximum = "18446744073709551615"
  const actions = { registries: [validRegistry(maximum)], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  assert.equal(parseSessionCorporateActions(actions, registryContext(maximum)).registries[0]?.issued_shares, maximum)
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...validRegistry(), issuer: "C-other" }] }, registryContext()), /身份|发行股数/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...validRegistry(), issued_shares: "11" }] }, registryContext()), /发行股数不守恒/)
  const badFifo = validRegistry()
  badFifo.holdings[0]!.lots = [
    { id: "late", qty: "5", acquired_on: "2030-01-03", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" },
    { id: "early", qty: "5", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" },
  ]
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [badFifo] }, registryContext()), /FIFO/)
})

test("登记快照事件身份必须唯一", () => {
  const snapshot = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", issued_shares: "10", settled_receipts: "0", issuer_repurchase_account: null, holdings: validRegistry().holdings }
  const registry = { ...validRegistry(), registrations: [snapshot, snapshot] }
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, registryContext()), /登记快照.*身份重复/)
})

test("分红书必须显式保存 required nullable registration 和完整空付款状态", () => {
  const book = {
    plan: { plan_id: "plan-a", issuer: "C-600101", stock: "600101", exchange: "sse", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-04", ex_dividend_on: "2030-01-07", payable_on: "2030-01-08", gross_per_share: "1", distributable_amount: "100" },
    status: "Approved", registration: null, entitlements: [], paid: [], failures: [], payments: [],
  }
  const registry = { ...validRegistry(), settled_on: "2030-01-03" }
  assert.equal(parseSessionCorporateActions({ registries: [registry], dividends: [book], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, registryContext()).dividends[0]?.registration, null)
  const { registration: _registration, ...missing } = book
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [missing], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, registryContext()), /registration.*必填/)
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [{ ...book, status: "Unexpected" }], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, registryContext()), /status.*分红状态/)
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [{ ...book, plan: { ...book.plan, exchange: "szse" } }], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, registryContext()), /exchange.*交易所与证券不一致/)
})

test("External 分红到账回执必须逐项匹配 Paid 金额、持有人、方案和付款日", () => {
  const holder = { External: "holder-a" }
  const lot = { id: "lot-a", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const registration = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-04", issued_shares: "10", settled_receipts: "0", issuer_repurchase_account: null, holdings: [{ holder, lots: [lot] }] }
  const registry = { stock: "600101", issuer: "C-600101", issued_shares: "10", issuer_repurchase_account: null, settled_on: "2030-01-08", holdings: [{ holder, lots: [lot] }], receipts: [], registrations: [registration] }
  const payment = { payment_id: "payment-a", paid_on: "2030-01-06", within_six_month_deadline: true, outcomes: [{ Paid: { holder, amount: "10" } }] }
  const book = {
    plan: { plan_id: "plan-a", issuer: "C-600101", stock: "600101", exchange: "sse", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-04", ex_dividend_on: "2030-01-05", payable_on: "2030-01-06", gross_per_share: "1", distributable_amount: "100" },
    status: "Paid", registration, entitlements: [{ holder, shares: "10", gross: "10" }], paid: [[holder, "10"]], failures: [], payments: [payment],
  }
  const grossReceipt = { payment_id: payment.payment_id, plan_id: "plan-a", holder, paid_on: payment.paid_on, gross: "10", tax_status: "TreatmentNotConfigured" }
  const actions = { registries: [registry], dividends: [book], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [grossReceipt], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  const context = { ...registryContext(), currentDate: "2030-01-08" }
  assert.equal(parseSessionCorporateActions(actions, context).external_receipts[0]?.gross, "10")
  assert.throws(() => parseSessionCorporateActions({ ...actions, external_receipts: [] }, context), /缺少已支付 External/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, external_receipts: [{ ...grossReceipt, gross: "11" }] }, context), /与分红支付成功事实不一致/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, external_receipts: [{ ...grossReceipt, tax_status: "Unknown" }] }, context), /tax_status.*枚举/)
  const futurePayment = { ...payment, paid_on: "2030-01-09" }
  const futureBook = { ...book, plan: { ...book.plan, payable_on: "2030-01-09" }, payments: [futurePayment] }
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividends: [futureBook], external_receipts: [{ ...grossReceipt, paid_on: "2030-01-09" }] }, context), /实际到账日期晚于存档日/)
})

test("MarketSnap 显式保留除息状态、日内活动与 required nullable 锚点", () => {
  const market = { last_price: "1000", last_close: "1000", cash_ex_reference_pending_trade: false, day_market_activity: true, last_cash_ex_reference: null }
  const snapshot = { seq: 0, tick: 0, markets: { "600101": market }, accounts: {}, daily_candles: {}, active_daily_candles: {} }
  assert.deepEqual(parseSaveSnapshot(snapshot, "snapshot").markets["600101"], market)
  for (const field of ["cash_ex_reference_pending_trade", "day_market_activity", "last_cash_ex_reference"]) {
    const missing = { ...market } as Record<string, unknown>
    delete missing[field]
    assert.throws(() => parseSaveSnapshot({ ...snapshot, markets: { "600101": missing } }, "snapshot"), new RegExp(`snapshot\\.markets\\.600101\\.${field}.*必填`))
  }
  assert.throws(() => parseSaveSnapshot({ ...snapshot, markets: { "600101": { ...market, day_market_activity: "false" } } }, "snapshot"), /day_market_activity.*布尔值/)
  assert.throws(() => parseSaveSnapshot({ ...snapshot, markets: { "600101": { ...market, last_cash_ex_reference: { ex_date: "2030-01-01", reference_price: "0" } } } }, "snapshot"), /参考价必须为正/)
  const anchor = { ex_date: "2030-01-01", reference_price: "1000" }
  const pendingMarket = { ...market, cash_ex_reference_pending_trade: true, last_cash_ex_reference: anchor }
  const parsed = parseSaveSnapshot({ ...snapshot, markets: { "600101": pendingMarket } }, "snapshot")
  validateCashExReferenceFacts(parsed, [{ code: "600101", tick: "1" }], "2030-01-01")
  assert.throws(() => validateCashExReferenceFacts(parsed, [{ code: "600101", tick: "3" }], "2030-01-01"), /最小价位单位/)
  assert.throws(() => validateCashExReferenceFacts(parsed, [{ code: "600101", tick: "1" }], "2029-12-31"), /晚于当前存档日/)
  const orphanPending = parseSaveSnapshot({ ...snapshot, markets: { "600101": { ...market, cash_ex_reference_pending_trade: true } } }, "snapshot")
  assert.throws(() => validateCashExReferenceFacts(orphanPending, [{ code: "600101", tick: "1" }], "2030-01-01"), /必须已安装/)
  const staleClose = parseSaveSnapshot({ ...snapshot, markets: { "600101": { ...pendingMarket, last_close: "999" } } }, "snapshot")
  assert.throws(() => validateCashExReferenceFacts(staleClose, [{ code: "600101", tick: "1" }], "2030-01-01"), /等于昨收价/)
})

test("已应用除息组与 MarketSnap 锚点日期、参考价双向匹配", () => {
  const anchor = { ex_date: "2030-01-01", reference_price: "1000" }
  const context = { ...registryContext(), snapshot: { markets: { "600101": { last_cash_ex_reference: anchor } }, accounts: {} } }
  const group = { date: anchor.ex_date, stock: "600101", cash_plan_ids: ["plan-a"], stock_event_ids: [], rights_event_ids: [], split_event_ids: [], reference: anchor }
  const actions = { registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [group], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  assert.throws(() => parseSessionCorporateActions(actions, context), /除权除息组与登记分红方案、送转或拆股／缩股事件不一致/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, context), /除权除息锚点缺少对应已应用组/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, applied_ex_reference_groups: [{ ...group, stock: "600101", reference: { ...anchor, reference_price: "1002" } }] }, context), /MarketSnap 锚点不一致/)
})

test("MarketSnap 除息锚点必须匹配证券最新已应用除息组", () => {
  const holder = { External: "holder-a" }
  const lot = { id: "lot-a", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const registrationA = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-01", issued_shares: "10", settled_receipts: "0", issuer_repurchase_account: null, holdings: [{ holder, lots: [lot] }] }
  const registrationB = { ...registrationA, event_id: "plan-b" }
  const registry = { ...validRegistry(), registrations: [registrationA, registrationB] }
  const makeDividend = (plan_id: string, ex_dividend_on: string, registration: typeof registrationA) => ({
    plan: { plan_id, issuer: "C-600101", stock: "600101", exchange: "sse", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-01", registered_on: "2030-01-01", ex_dividend_on, payable_on: ex_dividend_on, gross_per_share: "1", distributable_amount: "10" },
    status: "Registered", registration, entitlements: [{ holder, shares: "10", gross: "10" }], paid: [], failures: [], payments: [],
  })
  const dividends = [makeDividend("plan-a", "2030-01-02", registrationA), makeDividend("plan-b", "2030-01-03", registrationB)]
  const anchor = { ex_date: "2030-01-02", reference_price: "1000" }
  const laterReference = { ex_date: "2030-01-03", reference_price: "999" }
  const context = { ...registryContext(), snapshot: { markets: { "600101": { last_cash_ex_reference: anchor } }, accounts: {} } }
  const groups = [
    { date: anchor.ex_date, stock: "600101", cash_plan_ids: ["plan-a"], stock_event_ids: [], rights_event_ids: [], split_event_ids: [], reference: anchor },
    { date: laterReference.ex_date, stock: "600101", cash_plan_ids: ["plan-b"], stock_event_ids: [], rights_event_ids: [], split_event_ids: [], reference: laterReference },
  ]
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends, dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: groups, stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, context), /锚点必须匹配最新已应用除权除息组/)
  const currentAnchorContext = { ...context, snapshot: { markets: { "600101": { last_cash_ex_reference: laterReference } }, accounts: {} } }
  assert.equal(parseSessionCorporateActions({ registries: [registry], dividends, dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: groups, stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, currentAnchorContext).applied_ex_reference_groups.length, 2)
})

test("股东名册与登记快照必须显式保存 issuer_repurchase_account nullable", () => {
  const registry = validRegistry()
  const { issuer_repurchase_account: _registryFacts, ...missingRegistryFacts } = registry
  assert.throws(() => parseSessionCorporateActions({ registries: [missingRegistryFacts], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, registryContext()), /issuer_repurchase_account.*必填/)
  const registration = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", issued_shares: "10", settled_receipts: "0", issuer_repurchase_account: null, holdings: registry.holdings }
  const { issuer_repurchase_account: _snapshotFacts, ...missingSnapshotFacts } = registration
  assert.throws(() => parseSessionCorporateActions({ registries: [{ ...registry, registrations: [missingSnapshotFacts] }], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }, registryContext()), /issuer_repurchase_account.*必填/)
})

test("回购专户事实按建立日期绑定到不可变登记快照", () => {
  const facts = { account_reference: "issuer-buyback-1", source_evidence: "exchange-confirmation-1", established_on: "2030-01-02" }
  const before = { event_id: "before", stock: "600101", issuer: "C-600101", registered_on: "2030-01-01", issued_shares: "10", settled_receipts: "0", issuer_repurchase_account: null, holdings: validRegistry().holdings }
  const after = { ...before, event_id: "after", registered_on: "2030-01-03", issuer_repurchase_account: facts }
  const registry = { ...validRegistry(), issuer_repurchase_account: facts, registrations: [before, after] }
  const actions = { registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  assert.deepEqual(parseSessionCorporateActions(actions, registryContext()).registries[0]?.registrations.map(item => item.issuer_repurchase_account), [null, facts])
  const earliestFacts = { ...validRegistry(), registrations: [], issuer_repurchase_account: { ...facts, established_on: "1900-01-01" } }
  assert.doesNotThrow(() => parseSessionCorporateActions({ ...actions, registries: [earliestFacts] }, registryContext()))
  const latestFacts = { ...validRegistry(), settled_on: "2199-12-31", registrations: [], issuer_repurchase_account: { ...facts, established_on: "2199-12-31" } }
  assert.doesNotThrow(() => parseSessionCorporateActions({ ...actions, registries: [latestFacts] }, { ...registryContext(), currentDate: "2199-12-31" }))
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, registrations: [{ ...before, issuer_repurchase_account: { ...facts, account_reference: "forged-account", established_on: "2030-01-01" } }, after] }] }, registryContext()), /历史不一致/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, issuer_repurchase_account: { ...facts, established_on: "2030-01-04" } }] }, registryContext()), /不得晚于适用日期/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, registrations: [], issuer_repurchase_account: { ...facts, established_on: "1899-12-31" } }] }, registryContext()), /CivilDate.*1900–2199/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, registrations: [], issuer_repurchase_account: { ...facts, established_on: "2200-01-01" } }] }, { ...registryContext(), currentDate: "2199-12-31" }), /CivilDate.*1900–2199/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, issuer_repurchase_account: { ...facts, account_reference: " " } }] }, registryContext()), /不能为空/)
})

test("税账首个日结必须紧邻开账日下一自然日", () => {
  const taxLot = { id: "tax-account-lot", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, class: "PublicMarket" }
  const lot = { id: "account-lot", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const makeActions = (settledOn: string, firstDay: string) => {
    const registry = { ...validRegistry(), settled_on: settledOn, holdings: [{ holder: { Account: "0" }, lots: [lot] }, { holder: { External: "holder-a" }, lots: [] }] }
    const taxBook = {
      operation_seq: 1, account: "0", stock: "600101", profile: "IndividualPublicMarket",
      opened_on: "2030-01-03", opening_lots: [taxLot], settled_on: settledOn, lots: [taxLot],
      days: [{ operation_seq: 1, event_id: "day-a", day: firstDay, net_change: "0", acquisition: null, dispositions: [] }],
      dividends: [], collections: [], redenominations: [],
    }
    return { registries: [registry], dividends: [], dividend_tax_books: [taxBook], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  }
  const context = { ...registryContext(), currentDate: "2030-01-05", snapshot: { markets: {}, accounts: { "0": { positions: { "600101": { qty: 10 } } } } } }
  assert.doesNotThrow(() => parseSessionCorporateActions(makeActions("2030-01-04", "2030-01-04"), context), "紧邻开账日的首个日结必须被接受")
  assert.throws(() => parseSessionCorporateActions(makeActions("2030-01-05", "2030-01-05"), context), /税账日结必须.*自然日/)
})

test("精确税额分数必须完整约简（gcd=1），非仅拒绝可整除", () => {
  const taxBook = {
    operation_seq: 1, account: "0", stock: "600101", profile: "IndividualPublicMarket",
    opened_on: "2030-01-03", opening_lots: [], settled_on: "2030-01-03", lots: [], days: [], dividends: [],
    collections: [{ operation_seq: 1, event_id: "col-a", day: "2030-01-03", available_cash: "10", collected: "0", remaining_cash: "10", outstanding: { numerator: "2", denominator: "4" }, needs_funds: true }],
    redenominations: [],
  }
  const lot = { id: "account-lot", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const registry = { ...validRegistry(), settled_on: "2030-01-03", holdings: [{ holder: { Account: "0" }, lots: [lot] }, { holder: { External: "holder-a" }, lots: [] }] }
  const actions = { registries: [registry], dividends: [], dividend_tax_books: [taxBook], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  const context = { ...registryContext(), snapshot: { markets: {}, accounts: { "0": { positions: { "600101": { qty: 10 } } } } } }
  assert.throws(() => parseSessionCorporateActions(actions, context), /已约简非负分数/)
  const reduced = structuredClone(taxBook)
  reduced.collections = [{ ...reduced.collections[0]!, outstanding: { numerator: "1", denominator: "2" } }]
  assert.doesNotThrow(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [reduced] }, context))
})

test("股息税未划收查询视图按严格 parser 校验余额与原因一致性", async () => {
  const view = { account: "0", stock: "600101", outstanding: { numerator: "3", denominator: "1" }, needs_funds: true, cause: "InsufficientAvailableCash" }
  const { parseDividendTaxOutstandingView } = await import("./schema/corporate-actions.ts")
  assert.deepEqual(parseDividendTaxOutstandingView(view), view)
  assert.throws(() => parseDividendTaxOutstandingView({ ...view, needs_funds: false }), /资金不足标志与未划收税额不一致/)
  assert.throws(() => parseDividendTaxOutstandingView({ ...view, cause: "Cleared" }), /原因与余额不一致/)
  assert.throws(() => parseDividendTaxOutstandingView({ ...view, cause: "Unknown" }), /cause.*枚举/)
  assert.throws(() => parseDividendTaxOutstandingView({ ...view, outstanding: { numerator: "2", denominator: "4" } }), /已约简非负分数/)
  assert.throws(() => parseDividendTaxOutstandingView({ ...view, account: "-1" }), /account/)
  assert.deepEqual(
    parseDividendTaxOutstandingView({ account: "0", stock: "600101", outstanding: { numerator: "0", denominator: "1" }, needs_funds: false, cause: "Cleared" }),
    { account: "0", stock: "600101", outstanding: { numerator: "0", denominator: "1" }, needs_funds: false, cause: "Cleared" },
  )
})

test("非交易过户送转入账按同日追加回执登记并守恒发行股数", () => {
  const holder = { Account: "0" }
  const lot = { id: "account-lot", qty: "6", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const marketDay = {
    request: { event_id: "session-market:600101:2030-01-02", day: "2030-01-02", scope: "PublicMarket", changes: [] },
    disposals: [],
  }
  const issueDay: MutableShareDayReceipt = {
    request: {
      event_id: "stock-distribution:distribution-1", day: "2030-01-02", scope: { NonTradingTransfer: { basis: "shareholders-resolution-1" } },
      changes: [{ holder, change: "2", acquisition: { lot_id: "stock-distribution:distribution-1:account-0", source: { CorporateAction: { event: "distribution-1" } }, restriction: "Unrestricted" } }],
    },
    disposals: [],
  }
  const creditedLot = { ...lot, id: "stock-distribution:distribution-1:account-0", qty: "2", acquired_on: "2030-01-02", source: { CorporateAction: { event: "distribution-1" } } }
  const registry = {
    ...validRegistry("12"), settled_on: "2030-01-02",
    holdings: [{ holder, lots: [lot, creditedLot] }, { holder: { External: "holder-a" }, lots: [{ ...lot, id: "external-lot", qty: "4" }] }],
    receipts: [marketDay, issueDay], registrations: [],
  }
  // setup 初始股数 10，送转入账后名册与发行人身份均为 12。
  const context: Context = {
    issuers: { "C-600101": { listed_stock: "600101", issued_shares: "12" } },
    setup: { stocks: [{ code: "600101", total_shares: "10", exchange: "Shanghai", tick: "1" }] },
    snapshot: { markets: {}, accounts: { "0": { positions: { "600101": { qty: 8 } } } } },
    currentDate: "2030-01-02",
  }
  const actions = { registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  assert.doesNotThrow(() => parseSessionCorporateActions(actions, context))
  // 名册当前股数必须等于 setup 初始股数加非交易过户增发合计，不允许无法解释的差额。
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, issued_shares: "13", holdings: [{ holder, lots: [lot, creditedLot] }, { holder: { External: "holder-a" }, lots: [{ ...lot, id: "external-lot", qty: "5" }] }] }] }, { ...context, issuers: { "C-600101": { listed_stock: "600101", issued_shares: "13" } } }), /初始股数加公司行为净增发不一致/)
  // 非交易过户不得减持、不得使用二级市场来源，也不得凭空造零股。
  const disposal = JSON.parse(JSON.stringify(issueDay)) as typeof issueDay
  disposal.request.changes = [{ holder, change: "-1", acquisition: null }]
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, receipts: [marketDay, disposal] }] }, context), /非交易过户不得减持/)
  const secondary = JSON.parse(JSON.stringify(issueDay)) as typeof issueDay
  secondary.request.changes[0]!.acquisition!.source = { SecondaryMarket: { settlement: "not-a-corporate-action" } }
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, receipts: [marketDay, secondary] }] }, context), /公司行为来源/)
  const zeroIssue = JSON.parse(JSON.stringify(issueDay)) as typeof issueDay
  zeroIssue.request.changes = []
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, receipts: [marketDay, zeroIssue] }] }, context), /必须新增正数股份/)
  // 公开市场回执不能跳日落在非交易过户之后。
  const jumpDay = { request: { event_id: "session-market:600101:2030-01-04", day: "2030-01-04", scope: "PublicMarket", changes: [] }, disposals: [] }
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, settled_on: "2030-01-04", receipts: [marketDay, issueDay, jumpDay] }] }, { ...context, currentDate: "2030-01-04" }), /逐自然日连续/)
})

test("登记快照发行股数按登记日后的非交易过户增发回放核对", () => {
  const holder = { External: "holder-a" }
  const lot = { id: "lot-a", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const marketDay = { request: { event_id: "session-market:600101:2030-01-02", day: "2030-01-02", scope: "PublicMarket", changes: [] }, disposals: [] }
  const issueDay: MutableShareDayReceipt = {
    request: {
      event_id: "stock-distribution:distribution-1", day: "2030-01-02", scope: { NonTradingTransfer: { basis: "shareholders-resolution-1" } },
      changes: [{ holder: { Account: "0" }, change: "2", acquisition: { lot_id: "bonus-a", source: { CorporateAction: { event: "distribution-1" } }, restriction: "Unrestricted" } }],
    },
    disposals: [],
  }
  const registration = { event_id: "record-1", stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", issued_shares: "10", settled_receipts: "1", issuer_repurchase_account: null, holdings: [{ holder, lots: [lot] }] }
  const bonusLot = { id: "bonus-a", qty: "2", acquired_on: "2030-01-02", source: { CorporateAction: { event: "distribution-1" } }, restriction: "Unrestricted" }
  const registry = { ...validRegistry("12"), settled_on: "2030-01-02", holdings: [{ holder, lots: [lot] }, { holder: { Account: "0" }, lots: [bonusLot] }], receipts: [marketDay, issueDay], registrations: [registration] }
  const context: Context = {
    issuers: { "C-600101": { listed_stock: "600101", issued_shares: "12" } },
    setup: { stocks: [{ code: "600101", total_shares: "10", exchange: "Shanghai", tick: "1" }] },
    snapshot: { markets: {}, accounts: { "0": { positions: { "600101": { qty: 2 } } } } },
    currentDate: "2030-01-02",
  }
  const actions = { registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  assert.doesNotThrow(() => parseSessionCorporateActions(actions, context))
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [{ ...registry, registrations: [{ ...registration, issued_shares: "12", holdings: [{ holder, lots: [{ ...lot, qty: "12" }] }] }] }] }, context), /按非交易过户回放的发行股数不一致/)
})

function creditedStockDistributionRegistry() {
  const holder = { Account: "0" }
  const lot = { id: "account-lot", qty: "6", acquired_on: "2030-01-02", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const registration = { event_id: "distribution-1", stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", issued_shares: "10", settled_receipts: "1", issuer_repurchase_account: null, holdings: [{ holder, lots: [lot] }, { holder: { External: "holder-a" }, lots: [{ ...lot, id: "external-lot", qty: "4" }] }] }
  const marketDay = { request: { event_id: "session-market:600101:2030-01-02", day: "2030-01-02", scope: "PublicMarket", changes: [] }, disposals: [] }
  const marketNextDay = { request: { event_id: "session-market:600101:2030-01-03", day: "2030-01-03", scope: "PublicMarket", changes: [] }, disposals: [] }
  const issueDay = {
    request: {
      event_id: "stock-distribution:distribution-1", day: "2030-01-03", scope: { NonTradingTransfer: { basis: "shareholders-resolution-1" } },
      changes: [{ holder, change: "2", acquisition: { lot_id: "stock-distribution:distribution-1:account-0", source: { CorporateAction: { event: "distribution-1" } }, restriction: "Unrestricted" } }],
    },
    disposals: [],
  }
  const creditedLot = { id: "stock-distribution:distribution-1:account-0", qty: "2", acquired_on: "2030-01-03", source: { CorporateAction: { event: "distribution-1" } }, restriction: "Unrestricted" }
  const registry = {
    ...validRegistry("12"), settled_on: "2030-01-03",
    holdings: [{ holder, lots: [lot, creditedLot] }, { holder: { External: "holder-a" }, lots: [{ ...lot, id: "external-lot", qty: "4" }] }],
    receipts: [marketDay, marketNextDay, issueDay], registrations: [registration],
  }
  return { registry, holder, lot, registration }
}

test("送转账簿严格解析并交叉核对入账非交易过户回执", () => {
  const { registry, holder, lot, registration } = creditedStockDistributionRegistry()
  const receipt = {
    event_id: "distribution-1", approval_reference: "shareholders-resolution-1", registration_event_id: "distribution-1",
    stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", issued_shares_before: "10", issuer_treasury_shares_excluded: "0",
    kind: "BonusShares", shares_per_existing_share_micros: "250000", approved_total_new_shares: "2", tie_break_seed: "12345",
    source_lot_attribution: "SourceLotAttributionPending",
    holders: [{ holder, original_shares: "6", whole_shares: "2", fractional_numerator: "500000", original_lots: [lot] }],
  }
  const book = {
    plan: { event_id: "distribution-1", approval_reference: "shareholders-resolution-1", issuer: "C-600101", stock: "600101", exchange: "sse", kind: "BonusShares", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-02", ex_rights_on: "2030-01-03", shares_per_existing_share_micros: "250000", approved_total_new_shares: "2" },
    status: "Credited", registration, receipt, credited_on: "2030-01-03",
  }
  const anchor = { ex_date: "2030-01-03", reference_price: "800" }
  const group = { date: "2030-01-03", stock: "600101", cash_plan_ids: [], stock_event_ids: ["distribution-1"], rights_event_ids: [], split_event_ids: [], reference: anchor }
  const context: Context = {
    issuers: { "C-600101": { listed_stock: "600101", issued_shares: "12" } },
    setup: { stocks: [{ code: "600101", total_shares: "10", exchange: "Shanghai", tick: "1" }] },
    snapshot: { markets: { "600101": { last_cash_ex_reference: anchor } }, accounts: { "0": { positions: { "600101": { qty: 8 } } } } },
    currentDate: "2030-01-03",
  }
  const actions = { registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [group], stock_distributions: [book], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  const parsed = parseSessionCorporateActions(actions, context)
  assert.equal(parsed.stock_distributions[0]?.status, "Credited")
  assert.deepEqual(parsed.applied_ex_reference_groups[0]?.stock_event_ids, ["distribution-1"])
  // 每户整股合计必须等于获批新增股数。
  assert.throws(() => parseSessionCorporateActions({ ...actions, stock_distributions: [{ ...book, receipt: { ...receipt, approved_total_new_shares: "3" } }] }, context), /整股合计与获批新增股数不一致/)
  // 未登记就入账、入账日期与 R+1 不一致都要拒绝。
  assert.throws(() => parseSessionCorporateActions({ ...actions, stock_distributions: [{ ...book, status: "Registered", credited_on: "2030-01-03" }] }, context), /入账事实必须为 Credited 状态/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, stock_distributions: [{ ...book, credited_on: "2030-01-04" }] }, context), /入账事实必须为 Credited 状态/)
  // 入账非交易过户回执缺失或股数不符时拒绝。
  const missingReceipt = JSON.parse(JSON.stringify(actions)) as typeof actions
  missingReceipt.registries[0]!.receipts = missingReceipt.registries[0]!.receipts.filter(row => row.request.event_id !== "stock-distribution:distribution-1")
  missingReceipt.registries[0]!.issued_shares = "10"
  missingReceipt.registries[0]!.holdings[0]!.lots = missingReceipt.registries[0]!.holdings[0]!.lots.filter(lot => lot.id !== "stock-distribution:distribution-1:account-0")
  assert.throws(() => parseSessionCorporateActions(missingReceipt, { ...context, issuers: { "C-600101": { listed_stock: "600101", issued_shares: "10" } }, snapshot: { markets: { "600101": { last_cash_ex_reference: anchor } }, accounts: { "0": { positions: { "600101": { qty: 6 } } } } } }), /送转入账股数与非交易过户增发合计不一致/)
})

test("税账日结允许同日正向公司行为续记且拒绝同日非正向续记", () => {
  const taxLot = { id: "account-lot", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, class: "PublicMarket" }
  const creditLot = { id: "tax:stock-distribution:d1:account-0", qty: "3", acquired_on: "2030-01-04", source: { CorporateAction: { event: "d1" } }, class: "PublicMarket" }
  const marketDay = { operation_seq: 1, event_id: "session-market:600101:0:2030-01-04", day: "2030-01-04", net_change: "0", acquisition: null, dispositions: [] }
  const creditDay = { operation_seq: 2, event_id: "stock-distribution:d1:0", day: "2030-01-04", net_change: "3", acquisition: creditLot, dispositions: [] }
  const book = { operation_seq: 2, account: "0", stock: "600101", profile: "IndividualPublicMarket", opened_on: "2030-01-03", opening_lots: [taxLot], settled_on: "2030-01-04", lots: [taxLot, creditLot], days: [marketDay, creditDay], dividends: [], collections: [], redenominations: [] }
  const registry = { ...validRegistry("13"), settled_on: "2030-01-04", holdings: [{ holder: { Account: "0" }, lots: [{ id: "account-lot", qty: "13", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }] }] }
  const actions = { registries: [registry], dividends: [], dividend_tax_books: [book], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  const context = { ...registryContext("13"), currentDate: "2030-01-05", snapshot: { markets: {}, accounts: { "0": { positions: { "600101": { qty: 13 } } } } } }
  assert.deepEqual(parseSessionCorporateActions(actions, context).dividend_tax_books, [book])
  // 同日零净变动不得作为续记：送转到账必须是正向净增。
  const zeroContinuation = { ...book, days: [marketDay, { ...creditDay, net_change: "0", acquisition: null }] }
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [zeroContinuation] }, context), /税账日结必须紧邻/)
  // 同日负向续记（同日第二笔处置）必须拒绝。
  const negativeContinuation = { ...book, days: [marketDay, { ...creditDay, net_change: "-1", acquisition: null }] }
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [negativeContinuation] }, context), /税账日结必须紧邻/)
  // 首条日结不得等于开账日。
  const firstDayEqualsOpening = { ...book, opened_on: "2030-01-04", settled_on: "2030-01-04", days: [{ ...creditDay, operation_seq: 1 }], opening_lots: [taxLot, creditLot], operation_seq: 1 }
  const firstDayRegistry = { ...registry, settled_on: "2030-01-04" }
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [firstDayRegistry], dividend_tax_books: [firstDayEqualsOpening] }, context), /税账日结必须紧邻/)
})

// 以下 plan 片段的 exchange 等字段值取自 engine serde 真实输出
// （`.tmp/company-system/rights-repurchase/fix-round-exchange-probe.log`，
// 由 `packages/engine/examples/fix_round_exchange_probe.rs` 生成），防止 Web
// 枚举域再次漂移。setup 股票交易所是 StockExchange（Shanghai/Shenzhen），公司
// 行为 plan 是 CalendarExchange（sse/szse）：沪市对应 sse、深市对应 szse。

const engineRightsPlan = { event_id: "rights-event", approval_reference: "board-rights", issuer: "C-600101", stock: "600101", exchange: "sse", approved_on: "2030-01-02", announced_on: "2030-01-03", registered_on: "2030-01-06", payment_start_on: "2030-01-07", payment_deadline_on: "2030-01-08", ex_rights_on: "2030-01-09", settlement_on: "2030-01-10", listing_on: "2030-01-10", price_per_share: "10", mode: { RightsToAllShareholders: { shares_per_existing_share_micros: "500000" } }, npc_subscription_strategy: "FullByDefault" }

const engineRepurchasePlan = { event_id: "repurchase-event", approval_reference: "board-repurchase", issuer: "C-600101", stock: "600101", exchange: "sse", approved_on: "2030-01-02", announced_on: "2030-01-02", window_start_on: "2030-01-03", window_deadline_on: "2030-01-07", price_cap_per_share: "1100", total_budget: "5000000", max_shares: "100", purpose: "ReduceCapital" }

test("配股 plan 的交易所枚举域与 engine 真实序列化值一致", async () => {
  const { parseRightsOfferingEventPlanValue } = await import("./schema/corporate-actions.ts")
  assert.equal(parseRightsOfferingEventPlanValue(engineRightsPlan, "plan").exchange, "sse")
  assert.equal(parseRightsOfferingEventPlanValue({ ...engineRightsPlan, exchange: "szse" }, "plan").exchange, "szse")
  assert.throws(() => parseRightsOfferingEventPlanValue({ ...engineRightsPlan, exchange: "Shanghai" }, "plan"), /exchange.*枚举/)
  assert.throws(() => parseRightsOfferingEventPlanValue({ ...engineRightsPlan, exchange: "Shenzhen" }, "plan"), /exchange.*枚举/)
})

test("回购 plan 的交易所枚举域与 engine 真实序列化值一致", async () => {
  const { parseIssuerRepurchasePlanValue } = await import("./schema/corporate-actions.ts")
  assert.equal(parseIssuerRepurchasePlanValue(engineRepurchasePlan, "plan").exchange, "sse")
  assert.throws(() => parseIssuerRepurchasePlanValue({ ...engineRepurchasePlan, exchange: "Shanghai" }, "plan"), /exchange.*枚举/)
  assert.throws(() => parseIssuerRepurchasePlanValue({ ...engineRepurchasePlan, exchange: "Shenzhen" }, "plan"), /exchange.*枚举/)
})

test("分红与送转 plan 的交易所按 setup 股票交易所映射勾稽", () => {
  const registry = validRegistry()
  const dividend = {
    plan: { plan_id: "plan-a", issuer: "C-600101", stock: "600101", exchange: "sse", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-04", ex_dividend_on: "2030-01-07", payable_on: "2030-01-08", gross_per_share: "1", distributable_amount: "100" },
    status: "Approved", registration: null, entitlements: [], paid: [], failures: [], payments: [],
  }
  const actions = { registries: [registry], dividends: [dividend], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  assert.equal(parseSessionCorporateActions(actions, registryContext()).dividends[0]?.plan.exchange, "sse")
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividends: [{ ...dividend, plan: { ...dividend.plan, exchange: "szse" } }] }, registryContext()), /交易所与证券不一致/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividends: [{ ...dividend, plan: { ...dividend.plan, exchange: "Shanghai" } }] }, registryContext()), /exchange.*枚举/)
  const distribution = {
    plan: { event_id: "distribution-1", approval_reference: "shareholders-resolution-1", issuer: "C-600101", stock: "600101", exchange: "sse", kind: "BonusShares", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-02", ex_rights_on: "2030-01-03", shares_per_existing_share_micros: "100000", approved_total_new_shares: "2" },
    status: "Approved", registration: null, receipt: null, credited_on: null,
  }
  const distributionActions = { ...actions, dividends: [], stock_distributions: [distribution], share_splits: [] }
  assert.equal(parseSessionCorporateActions(distributionActions, registryContext()).stock_distributions[0]?.plan.exchange, "sse")
  assert.throws(() => parseSessionCorporateActions({ ...distributionActions, stock_distributions: [{ ...distribution, plan: { ...distribution.plan, exchange: "szse" } }] }, registryContext()), /交易所与证券不一致/)
})

test("税账与名册回执覆盖勾稽：按引擎同款 scope→事件 id 派生核对每个账户变动回执", () => {
  // 与 engine `SessionCorporateActions::validate` 的「送转×税账交互未入账」勾稽同构：
  // 公开市场日结 → `session-market:{stock}:{account}:{day}`；
  // 非交易过户（送转到账）→ `{回执事件 id}:{account}`；回购注销只核减 IssuerTreasury，跳过。
  const account = { Account: "0" }
  const external = { External: "holder-a" }
  const marketLot = { id: "market-lot-1", qty: "2", acquired_on: "2030-01-04", source: { SecondaryMarket: { settlement: "settle-1" } }, restriction: "Unrestricted" }
  const distributionLot = { id: "d1:account-0-lot", qty: "3", acquired_on: "2030-01-04", source: { CorporateAction: { event: "d1" } }, restriction: "Unrestricted" }
  const externalLot = { id: "ext-lot", qty: "12", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const marketReceipt = {
    request: {
      event_id: "session-market:600101:2030-01-04", day: "2030-01-04", scope: "PublicMarket",
      changes: [
        { holder: account, change: "2", acquisition: { lot_id: marketLot.id, source: marketLot.source, restriction: "Unrestricted" } },
        { holder: external, change: "-2", acquisition: null },
      ],
    },
    disposals: [{ holder: external, lot: { ...externalLot, qty: "2" }, disposed_on: "2030-01-04" }],
  }
  const distributionReceipt = {
    request: {
      event_id: "stock-distribution:d1", day: "2030-01-04", scope: { NonTradingTransfer: { basis: "shareholders-resolution-1" } },
      changes: [{ holder: account, change: "3", acquisition: { lot_id: distributionLot.id, source: distributionLot.source, restriction: "Unrestricted" } }],
    },
    disposals: [],
  }
  const registry = {
    ...validRegistry("15"), settled_on: "2030-01-04",
    holdings: [
      { holder: account, lots: [marketLot, distributionLot] },
      { holder: external, lots: [{ ...externalLot, qty: "10" }] },
    ],
    receipts: [marketReceipt, distributionReceipt], registrations: [],
  }
  const taxMarketLot = { id: `tax:${marketLot.id}`, qty: "2", acquired_on: "2030-01-04", source: { SecondaryMarket: { settlement: "settle-1" } }, class: "PublicMarket" }
  const taxDistributionLot = { id: `tax:${distributionLot.id}`, qty: "3", acquired_on: "2030-01-04", source: { CorporateAction: { event: "d1" } }, class: "PublicMarket" }
  const fullCoverageBook = {
    operation_seq: 2, account: "0", stock: "600101", profile: "IndividualPublicMarket",
    opened_on: "2030-01-03", opening_lots: [], settled_on: "2030-01-04",
    days: [
      { operation_seq: 1, event_id: "session-market:600101:0:2030-01-04", day: "2030-01-04", net_change: "2", acquisition: taxMarketLot, dispositions: [] },
      { operation_seq: 2, event_id: "stock-distribution:d1:0", day: "2030-01-04", net_change: "3", acquisition: taxDistributionLot, dispositions: [] },
    ],
    lots: [taxMarketLot, taxDistributionLot], dividends: [], collections: [], redenominations: [],
  }
  const actions = { registries: [registry], dividends: [], dividend_tax_books: [fullCoverageBook], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  // setup 初始股数 12：公开市场日只在持有人间转移（发行数不变），送转非交易过户 +3 → 名册 15。
  const context: Context = {
    issuers: { "C-600101": { listed_stock: "600101", issued_shares: "15" } },
    setup: { stocks: [{ code: "600101", total_shares: "12", exchange: "Shanghai", tick: "1" }] },
    snapshot: { markets: {}, accounts: { "0": { positions: { "600101": { qty: 5 } } } } },
    currentDate: "2030-01-05",
  }
  assert.doesNotThrow(() => parseSessionCorporateActions(actions, context), "公开市场与非交易过户回执都有对应税账日结时必须通过")
  // 缺少非交易过户（送转到账）日结：税账自身仍自洽（replay/序列/连续性均合法），
  // 只有覆盖勾稽能发现静默缺股。
  const missingDistribution = {
    ...fullCoverageBook, operation_seq: 1,
    days: [fullCoverageBook.days[0]],
    lots: [taxMarketLot],
  }
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [missingDistribution] }, context), /送转×税账交互未入账/)
  // 缺少公开市场日结事实。
  const missingMarket = {
    ...fullCoverageBook, operation_seq: 1,
    days: [{ ...fullCoverageBook.days[1]!, operation_seq: 1 }],
    lots: [taxDistributionLot],
  }
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [missingMarket] }, context), /送转×税账交互未入账/)
  // 事件 id 派生格式不匹配（缺少账户后缀）不算覆盖。
  const wrongDerivation = {
    ...fullCoverageBook,
    days: [fullCoverageBook.days[0], { ...fullCoverageBook.days[1], event_id: "stock-distribution:d1" }],
  }
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [wrongDerivation] }, context), /送转×税账交互未入账/)
})

test("税账覆盖勾稽对回购注销回执显式跳过、对零变动账户条目仍要求覆盖", () => {
  // 与引擎 validate 同构的两个边界：IssuerRepurchaseCancellation 即使（异常地）
  // 含 Account 分录也显式跳过不要求税账日结；公开市场回执中 change=0 的账户条目
  // 仍算「涉及该账户变动」，须有对应事件 id 的日结事实。
  const account = { Account: "0" }
  const marketLot = { id: "market-lot-1", qty: "2", acquired_on: "2030-01-04", source: { SecondaryMarket: { settlement: "settle-1" } }, restriction: "Unrestricted" }
  const distributionLot = { id: "d1:account-0-lot", qty: "3", acquired_on: "2030-01-04", source: { CorporateAction: { event: "d1" } }, restriction: "Unrestricted" }
  const cancellationLot = { id: "cancel:account-0-lot", qty: "1", acquired_on: "2030-01-04", source: { CorporateAction: { event: "cancel-1" } }, restriction: "Unrestricted" }
  const externalLot = { id: "ext-lot", qty: "12", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const marketReceipt = {
    request: {
      event_id: "session-market:600101:2030-01-04", day: "2030-01-04", scope: "PublicMarket",
      changes: [
        { holder: account, change: "2", acquisition: { lot_id: marketLot.id, source: marketLot.source, restriction: "Unrestricted" } },
        { holder: { External: "holder-a" }, change: "-2", acquisition: null },
      ],
    },
    disposals: [{ holder: { External: "holder-a" }, lot: { ...externalLot, qty: "2" }, disposed_on: "2030-01-04" }],
  }
  const distributionReceipt = {
    request: {
      event_id: "stock-distribution:d1", day: "2030-01-04", scope: { NonTradingTransfer: { basis: "shareholders-resolution-1" } },
      changes: [{ holder: account, change: "3", acquisition: { lot_id: distributionLot.id, source: distributionLot.source, restriction: "Unrestricted" } }],
    },
    disposals: [],
  }
  const cancellationReceipt = {
    request: {
      event_id: "repurchase-cancellation:c1", day: "2030-01-04", scope: { IssuerRepurchaseCancellation: { basis: "cancellation-resolution-1" } },
      changes: [{ holder: account, change: "1", acquisition: { lot_id: cancellationLot.id, source: cancellationLot.source, restriction: "Unrestricted" } }],
    },
    disposals: [],
  }
  const zeroChangeReceipt = {
    request: {
      event_id: "session-market:600101:2030-01-05", day: "2030-01-05", scope: "PublicMarket",
      changes: [{ holder: account, change: "0", acquisition: null }],
    },
    disposals: [],
  }
  const registry = {
    ...validRegistry("16"), settled_on: "2030-01-05",
    holdings: [
      { holder: account, lots: [marketLot, distributionLot, cancellationLot] },
      { holder: { External: "holder-a" }, lots: [{ ...externalLot, qty: "10" }] },
    ],
    receipts: [marketReceipt, distributionReceipt, cancellationReceipt, zeroChangeReceipt], registrations: [],
  }
  const taxMarketLot = { id: "tax:market-lot-1", qty: "2", acquired_on: "2030-01-04", source: { SecondaryMarket: { settlement: "settle-1" } }, class: "PublicMarket" }
  const taxDistributionLot = { id: "tax:d1:account-0-lot", qty: "3", acquired_on: "2030-01-04", source: { CorporateAction: { event: "d1" } }, class: "PublicMarket" }
  const bookWithZeroDay = {
    operation_seq: 3, account: "0", stock: "600101", profile: "IndividualPublicMarket",
    opened_on: "2030-01-03", opening_lots: [], settled_on: "2030-01-05",
    days: [
      { operation_seq: 1, event_id: "session-market:600101:0:2030-01-04", day: "2030-01-04", net_change: "2", acquisition: taxMarketLot, dispositions: [] },
      { operation_seq: 2, event_id: "stock-distribution:d1:0", day: "2030-01-04", net_change: "3", acquisition: taxDistributionLot, dispositions: [] },
      { operation_seq: 3, event_id: "session-market:600101:0:2030-01-05", day: "2030-01-05", net_change: "0", acquisition: null, dispositions: [] },
    ],
    lots: [taxMarketLot, taxDistributionLot], dividends: [], collections: [], redenominations: [],
  }
  const actions = { registries: [registry], dividends: [], dividend_tax_books: [bookWithZeroDay], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  // setup 初始 12 ＋ 送转 3 ＋ 回购注销 1 = 名册/发行人 16。
  const context: Context = {
    issuers: { "C-600101": { listed_stock: "600101", issued_shares: "16" } },
    setup: { stocks: [{ code: "600101", total_shares: "12", exchange: "Shanghai", tick: "1" }] },
    snapshot: { markets: {}, accounts: { "0": { positions: { "600101": { qty: 6 } } } } },
    currentDate: "2030-01-05",
  }
  // 回购注销回执被显式跳过（税账没有 c1 派生日结仍通过）；零变动日已有覆盖。
  assert.doesNotThrow(() => parseSessionCorporateActions(actions, context))
  // 拿掉零变动日的日结事实：change=0 的账户条目仍要求覆盖。
  const missingZeroDay = { ...bookWithZeroDay, operation_seq: 2, days: bookWithZeroDay.days.slice(0, 2) }
  assert.throws(() => parseSessionCorporateActions({ ...actions, dividend_tax_books: [missingZeroDay] }, context), /送转×税账交互未入账/)
})

test("税账覆盖勾稽与 replay 纳入 ShareReDenomination 重新计值回执", () => {
  // 与引擎 `SessionCorporateActions::validate` 的 ShareReDenomination 分支同构：
  // 事件 id 派生为 `{回执事件 id}:{account}`；拆股正向增量以日结续记覆盖、
  // 缩股核减以 redenominations 回执覆盖（非应税、不进处置口径），二者任一即可；
  // replay 必须按 operation_seq 交织应用核减片段，否则缩股后的 lots 无法重构。
  const account = { Account: "0" }
  const external = { External: "holder-a" }
  // —— 拆股方向：1 拆 2，R+1=2030-01-03 入账 ——
  const splitLot = { id: "lot-a", qty: "4", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const splitExternalLot = { ...splitLot, id: "lot-b" }
  const splitRegistration = { event_id: "split-1", stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", issued_shares: "8", settled_receipts: "0", issuer_repurchase_account: null, holdings: [{ holder: account, lots: [splitLot] }, { holder: external, lots: [splitExternalLot] }] }
  const splitCreditedLot = { id: "share-split:split-1:account-0", qty: "4", acquired_on: "2030-01-03", source: { CorporateAction: { event: "split-1" } }, restriction: "Unrestricted" }
  const splitCreditedExternal = { id: "share-split:split-1:external-holder-a", qty: "4", acquired_on: "2030-01-03", source: { CorporateAction: { event: "split-1" } }, restriction: "Unrestricted" }
  const splitReceipt = {
    request: { event_id: "share-split:split-1", day: "2030-01-03", scope: { ShareReDenomination: { basis: "shareholders-resolution-1" } }, changes: [
      { holder: account, change: "4", acquisition: { lot_id: "share-split:split-1:account-0", source: { CorporateAction: { event: "split-1" } }, restriction: "Unrestricted" } },
      { holder: external, change: "4", acquisition: { lot_id: "share-split:split-1:external-holder-a", source: { CorporateAction: { event: "split-1" } }, restriction: "Unrestricted" } },
    ] },
    disposals: [],
  }
  const splitRegistry = {
    ...validRegistry("16"), settled_on: "2030-01-03",
    holdings: [{ holder: account, lots: [splitLot, splitCreditedLot] }, { holder: external, lots: [splitExternalLot, splitCreditedExternal] }],
    receipts: [splitReceipt], registrations: [splitRegistration],
  }
  const splitBook = {
    plan: { event_id: "split-1", approval_reference: "shareholders-resolution-1", issuer: "C-600101", stock: "600101", exchange: "sse", direction: "Split", ratio: "2", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-02", ex_rights_on: "2030-01-03" },
    status: "Settled", registration: splitRegistration, receipt: {
      event_id: "split-1", approval_reference: "shareholders-resolution-1", registration_event_id: "split-1",
      stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", direction: "Split", ratio: "2",
      issued_shares_before: "8", issued_shares_after: "16", tie_break_seed: "999",
      holders: [
        { holder: account, original_shares: "4", new_shares: "8", fractional_numerator: "0", tie_break_award: "0", original_lots: [splitLot] },
        { holder: external, original_shares: "4", new_shares: "8", fractional_numerator: "0", tie_break_award: "0", original_lots: [splitExternalLot] },
      ],
    }, settled_on: "2030-01-03",
  }
  const taxOpeningLot = { id: "tax:lot-a", qty: "4", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, class: "PublicMarket" }
  const taxSplitLot = { id: "tax:share-split:split-1:account-0", qty: "4", acquired_on: "2030-01-03", source: { CorporateAction: { event: "split-1" } }, class: "PublicMarket" }
  const splitTaxBook = {
    operation_seq: 1, account: "0", stock: "600101", profile: "IndividualPublicMarket",
    opened_on: "2030-01-02", opening_lots: [taxOpeningLot], settled_on: "2030-01-03",
    days: [{ operation_seq: 1, event_id: "share-split:split-1:0", day: "2030-01-03", net_change: "4", acquisition: taxSplitLot, dispositions: [] }],
    lots: [taxOpeningLot, taxSplitLot], dividends: [], collections: [], redenominations: [],
  }
  const splitAnchor = { ex_date: "2030-01-03", reference_price: "500" }
  const splitActions = { registries: [splitRegistry], dividends: [], dividend_tax_books: [splitTaxBook], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [{ date: "2030-01-03", stock: "600101", cash_plan_ids: [], stock_event_ids: [], rights_event_ids: [], split_event_ids: ["split-1"], reference: splitAnchor }], stock_distributions: [], share_splits: [splitBook], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  const splitContext: Context = {
    issuers: { "C-600101": { listed_stock: "600101", issued_shares: "16" } },
    setup: { stocks: [{ code: "600101", total_shares: "8", exchange: "Shanghai", tick: "1" }] },
    snapshot: { markets: { "600101": { last_cash_ex_reference: splitAnchor } }, accounts: { "0": { positions: { "600101": { qty: 8 } } } } },
    currentDate: "2030-01-04",
  }
  assert.doesNotThrow(() => parseSessionCorporateActions(splitActions, splitContext), "拆股正向增量有同日续记日结时必须通过")
  // 事件 id 派生缺少账户后缀不算覆盖。
  const wrongSplitDerivation = { ...splitTaxBook, days: [{ ...splitTaxBook.days[0]!, event_id: "share-split:split-1" }] }
  assert.throws(() => parseSessionCorporateActions({ ...splitActions, dividend_tax_books: [wrongSplitDerivation] }, splitContext), /未入账/)

  // —— 缩股方向：2 并 1，R=2030-01-04、R+1=2030-01-05 入账 ——
  const initialLot = { id: "lot-a", qty: "6", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const externalInitialLot = { ...initialLot, id: "lot-b", qty: "4" }
  const marketLot = { id: "market-lot-1", qty: "2", acquired_on: "2030-01-04", source: { SecondaryMarket: { settlement: "settle-1" } }, restriction: "Unrestricted" }
  const consolidationRegistration = { event_id: "consolidation-1", stock: "600101", issuer: "C-600101", registered_on: "2030-01-04", issued_shares: "10", settled_receipts: "0", issuer_repurchase_account: null, holdings: [{ holder: account, lots: [initialLot, marketLot] }, { holder: external, lots: [{ ...externalInitialLot, qty: "2" }] }] }
  const marketReceipt = {
    request: {
      event_id: "session-market:600101:2030-01-04", day: "2030-01-04", scope: "PublicMarket",
      changes: [
        { holder: account, change: "2", acquisition: { lot_id: "market-lot-1", source: { SecondaryMarket: { settlement: "settle-1" } }, restriction: "Unrestricted" } },
        { holder: external, change: "-2", acquisition: null },
      ],
    },
    disposals: [{ holder: external, lot: { ...externalInitialLot, qty: "2" }, disposed_on: "2030-01-04" }],
  }
  const zeroChangeReceipt = {
    request: {
      event_id: "session-market:600101:2030-01-05", day: "2030-01-05", scope: "PublicMarket",
      changes: [{ holder: account, change: "0", acquisition: null }],
    },
    disposals: [],
  }
  const consolidationReceipt = {
    request: { event_id: "share-split:consolidation-1", day: "2030-01-05", scope: { ShareReDenomination: { basis: "shareholders-resolution-2" } }, changes: [
      { holder: account, change: "-4", acquisition: null },
      { holder: external, change: "-1", acquisition: null },
    ] },
    disposals: [
      { holder: account, lot: { ...initialLot, qty: "4" }, disposed_on: "2030-01-05" },
      { holder: external, lot: { ...externalInitialLot, qty: "1" }, disposed_on: "2030-01-05" },
    ],
  }
  const consolidationRegistry = {
    ...validRegistry("5"), settled_on: "2030-01-05",
    holdings: [{ holder: account, lots: [{ ...initialLot, qty: "2" }, marketLot] }, { holder: external, lots: [{ ...externalInitialLot, qty: "1" }] }],
    receipts: [marketReceipt, zeroChangeReceipt, consolidationReceipt], registrations: [consolidationRegistration],
  }
  const consolidationBook = {
    plan: { event_id: "consolidation-1", approval_reference: "shareholders-resolution-2", issuer: "C-600101", stock: "600101", exchange: "sse", direction: "Consolidate", ratio: "2", approved_on: "2030-01-03", announced_on: "2030-01-04", registered_on: "2030-01-04", ex_rights_on: "2030-01-05" },
    status: "Settled", registration: consolidationRegistration, receipt: {
      event_id: "consolidation-1", approval_reference: "shareholders-resolution-2", registration_event_id: "consolidation-1",
      stock: "600101", issuer: "C-600101", registered_on: "2030-01-04", direction: "Consolidate", ratio: "2",
      issued_shares_before: "10", issued_shares_after: "5", tie_break_seed: "7",
      holders: [
        { holder: account, original_shares: "8", new_shares: "4", fractional_numerator: "0", tie_break_award: "0", original_lots: [initialLot, marketLot] },
        { holder: external, original_shares: "2", new_shares: "1", fractional_numerator: "0", tie_break_award: "0", original_lots: [{ ...externalInitialLot, qty: "2" }] },
      ],
    }, settled_on: "2030-01-05",
  }
  const taxInitialLot = { id: "tax:lot-a", qty: "6", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, class: "PublicMarket" }
  const taxMarketLot = { id: "tax:market-lot-1", qty: "2", acquired_on: "2030-01-04", source: { SecondaryMarket: { settlement: "settle-1" } }, class: "PublicMarket" }
  const consolidationTaxBook = {
    operation_seq: 3, account: "0", stock: "600101", profile: "IndividualPublicMarket",
    opened_on: "2030-01-03", opening_lots: [taxInitialLot], settled_on: "2030-01-05",
    days: [
      { operation_seq: 1, event_id: "session-market:600101:0:2030-01-04", day: "2030-01-04", net_change: "2", acquisition: taxMarketLot, dispositions: [] },
      { operation_seq: 2, event_id: "session-market:600101:0:2030-01-05", day: "2030-01-05", net_change: "0", acquisition: null, dispositions: [] },
    ],
    redenominations: [{ operation_seq: 3, event_id: "share-split:consolidation-1:0", day: "2030-01-05", net_change: "-4", removed: [{ lot: { ...taxInitialLot, qty: "4" }, disposed_on: "2030-01-05" }] }],
    lots: [{ ...taxInitialLot, qty: "2" }, taxMarketLot], dividends: [], collections: [],
  }
  const consolidationAnchor = { ex_date: "2030-01-05", reference_price: "500" }
  const consolidationActions = { registries: [consolidationRegistry], dividends: [], dividend_tax_books: [consolidationTaxBook], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [{ date: "2030-01-05", stock: "600101", cash_plan_ids: [], stock_event_ids: [], rights_event_ids: [], split_event_ids: ["consolidation-1"], reference: consolidationAnchor }], stock_distributions: [], share_splits: [consolidationBook], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  const consolidationContext: Context = {
    issuers: { "C-600101": { listed_stock: "600101", issued_shares: "5" } },
    setup: { stocks: [{ code: "600101", total_shares: "10", exchange: "Shanghai", tick: "1" }] },
    snapshot: { markets: { "600101": { last_cash_ex_reference: consolidationAnchor } }, accounts: { "0": { positions: { "600101": { qty: 4 } } } } },
    currentDate: "2030-01-06",
  }
  assert.doesNotThrow(() => parseSessionCorporateActions(consolidationActions, consolidationContext), "缩股核减有 redenominations 回执且 replay 交织应用核减时必须通过")
  // 缺少重新计值事实（既无日结也无核减回执）：覆盖勾稽显式拒绝。
  const missingRedenomination = {
    ...consolidationTaxBook, operation_seq: 2,
    redenominations: [],
    lots: [taxInitialLot, taxMarketLot],
  }
  assert.throws(() => parseSessionCorporateActions({ ...consolidationActions, dividend_tax_books: [missingRedenomination] }, consolidationContext), /未入账/)
  // 核减事实存在但 replay 未应用核减片段：lots 与开账批次及事实重构不一致。
  const unreplayedReduction = {
    ...consolidationTaxBook,
    lots: [taxInitialLot, taxMarketLot],
  }
  assert.throws(() => parseSessionCorporateActions({ ...consolidationActions, dividend_tax_books: [unreplayedReduction] }, consolidationContext), /replay 不一致/)
})

test("公开公告库接受拆股／缩股方案公告并拒绝发行人或公告日错配", async () => {
  // 与引擎 `AnnouncementContent::ShareSplit` 同构：披露事实只含方案条款，发行人
  // 与公告发生日必须与公告头一致（public_view 同款校验）。
  const { parsePublicLibrary } = await import("./schema/company/reports.ts")
  const instant = { date: "2030-01-04", second_of_day: 64800 }
  const plan = { event_id: "split-1", approval_reference: "board-1", issuer: "C-600101", stock: "600101", exchange: "sse", direction: "Split", ratio: "2", approved_on: "2030-01-03", announced_on: "2030-01-04", registered_on: "2030-01-07", ex_rights_on: "2030-01-08" }
  const announcement = { id: 1, company: "C-600101", occurred_on: "2030-01-04", published_at: instant, content: { kind: "ShareSplit", value: plan } }
  const library = { next_seq: 2, reports: [], announcements: [announcement] }
  const parsed = parsePublicLibrary(library)
  const content = parsed.announcements[0]?.content
  assert.ok(content !== undefined && "ShareSplit" in content, "解析结果必须是 ShareSplit 变体")
  assert.equal(content.ShareSplit.event_id, "split-1")
  // 发行人错配拒绝。
  assert.throws(() => parsePublicLibrary({ ...library, announcements: [{ ...announcement, company: "C-other" }] }), /拆股／缩股公告发行人或发生日不一致/)
  // 公告日错配拒绝。
  assert.throws(() => parsePublicLibrary({ ...library, announcements: [{ ...announcement, occurred_on: "2030-01-05" }] }), /拆股／缩股公告发行人或发生日不一致/)
  // 比例非法拒绝。
  assert.throws(() => parsePublicLibrary({ ...library, announcements: [{ ...announcement, content: { kind: "ShareSplit", value: { ...plan, ratio: "1" } } }] }), /比例必须为/)
})


test("配股认购拒绝回执严格解析并勾稽事件存在与净认购唯一性", () => {
  const receipt = { event_id: "rights-event", account: "0", requested_shares: "41", submitted_on: "2030-01-07", rejected_on: "2030-01-07", reason: "公开配售剩余额度 40 股，申请 41 股超出额度" }
  const rightsBook = { plan: engineRightsPlan, status: "Approved", registration: null, entitlement: null, subscriptions: [], closed_on: null, settlement: null, credited_on: null }
  const actions = { registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_reference_groups: [], stock_distributions: [], share_splits: [], rights_offerings: [rightsBook], rights_subscription_queue: [], rejected_rights_subscriptions: [receipt], issuer_repurchases: [] }
  const context = { ...emptyContext, currentDate: "2030-01-07", snapshot: { markets: {}, accounts: { "0": { positions: {} } } } }
  assert.equal(parseSessionCorporateActions(actions, context).rejected_rights_subscriptions[0]?.reason, receipt.reason)
  assert.throws(() => parseSessionCorporateActions(actions, emptyContext), /拒绝回执账户不存在/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, rejected_rights_subscriptions: [{ ...receipt, rejected_on: "2030-01-06" }] }, context), /拒绝回执日期不在提交日与存档日之间/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, rejected_rights_subscriptions: [{ ...receipt, rejected_on: "2030-01-08" }] }, context), /拒绝回执日期不在提交日与存档日之间/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, rejected_rights_subscriptions: [{ ...receipt, reason: " " }] }, context), /原因非法/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, rejected_rights_subscriptions: [{ ...receipt }, { ...receipt }] }, context), /重复拒绝回执/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, rights_subscription_queue: [{ event_id: "rights-event", account: "0", requested_shares: "41", submitted_on: "2030-01-07" }] }, context), /与仍在排队的认购并存/)
})

test("拆股／缩股账簿严格解析并交叉核对重新计值回执", () => {
  const holder = { Account: "0" }
  const lot = { id: "lot-a", qty: "4", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const external = { ...lot, id: "lot-b" }
  const registration = { event_id: "split-1", stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", issued_shares: "8", settled_receipts: "0", issuer_repurchase_account: null, holdings: [{ holder, lots: [lot] }, { holder: { External: "holder-a" }, lots: [external] }] }
  // 拆股 1 拆 2 后：账户 8 股（4 原 + 4 新 lot）、外部 8 股；总股本 16。
  const splitCreditedLot = { id: "share-split:split-1:account-0", qty: "4", acquired_on: "2030-01-03", source: { CorporateAction: { event: "split-1" } }, restriction: "Unrestricted" }
  const splitCreditedExternal = { id: "share-split:split-1:external-holder-a", qty: "4", acquired_on: "2030-01-03", source: { CorporateAction: { event: "split-1" } }, restriction: "Unrestricted" }
  const redenominationDay = {
    request: { event_id: "share-split:split-1", day: "2030-01-03", scope: { ShareReDenomination: { basis: "shareholders-resolution-1" } }, changes: [
      { holder, change: "4", acquisition: { lot_id: "share-split:split-1:account-0", source: { CorporateAction: { event: "split-1" } }, restriction: "Unrestricted" } },
      { holder: { External: "holder-a" }, change: "4", acquisition: { lot_id: "share-split:split-1:external-holder-a", source: { CorporateAction: { event: "split-1" } }, restriction: "Unrestricted" } },
    ] },
    disposals: [],
  }
  const registry = {
    ...validRegistry("16"), settled_on: "2030-01-03",
    holdings: [{ holder, lots: [lot, splitCreditedLot] }, { holder: { External: "holder-a" }, lots: [external, splitCreditedExternal] }],
    receipts: [redenominationDay], registrations: [registration],
  }
  const receipt = {
    event_id: "split-1", approval_reference: "shareholders-resolution-1", registration_event_id: "split-1",
    stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", direction: "Split", ratio: "2",
    issued_shares_before: "8", issued_shares_after: "16", tie_break_seed: "999",
    holders: [
      { holder, original_shares: "4", new_shares: "8", fractional_numerator: "0", tie_break_award: "0", original_lots: [lot] },
      { holder: { External: "holder-a" }, original_shares: "4", new_shares: "8", fractional_numerator: "0", tie_break_award: "0", original_lots: [external] },
    ],
  }
  const book = {
    plan: { event_id: "split-1", approval_reference: "shareholders-resolution-1", issuer: "C-600101", stock: "600101", exchange: "sse", direction: "Split", ratio: "2", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-02", ex_rights_on: "2030-01-03" },
    status: "Settled", registration, receipt, settled_on: "2030-01-03",
  }
  const anchor = { ex_date: "2030-01-03", reference_price: "500" }
  const group = { date: "2030-01-03", stock: "600101", cash_plan_ids: [], stock_event_ids: [], rights_event_ids: [], split_event_ids: ["split-1"], reference: anchor }
  const context: Context = {
    issuers: { "C-600101": { listed_stock: "600101", issued_shares: "16" } },
    setup: { stocks: [{ code: "600101", total_shares: "8", exchange: "Shanghai", tick: "1" }] },
    snapshot: { markets: { "600101": { last_cash_ex_reference: anchor } }, accounts: { "0": { positions: { "600101": { qty: 8 } } } } },
    currentDate: "2030-01-03",
  }
  const actions = { registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_reference_groups: [group], stock_distributions: [], share_splits: [book], rights_offerings: [], rights_subscription_queue: [], rejected_rights_subscriptions: [], issuer_repurchases: [] }
  const parsed = parseSessionCorporateActions(actions, context)
  assert.equal(parsed.share_splits[0]?.status, "Settled")
  assert.deepEqual(parsed.applied_ex_reference_groups[0]?.split_event_ids, ["split-1"])
  // 拆股换算必须精确放大整数倍。
  assert.throws(() => parseSessionCorporateActions({ ...actions, share_splits: [{ ...book, receipt: { ...receipt, issued_shares_after: "15", holders: receipt.holders.map(holder => ({ ...holder, new_shares: holder.new_shares === "8" ? "7" : "8" })) } }] }, context), /前后总股数不一致|精确放大/)
  // 比例为 1 拒绝。
  assert.throws(() => parseSessionCorporateActions({ ...actions, share_splits: [{ ...book, plan: { ...book.plan, ratio: "1" } }] }, context), /比例必须为/)
  // 入账日与重新计值回执日期不一致拒绝。
  assert.throws(() => parseSessionCorporateActions({ ...actions, share_splits: [{ ...book, settled_on: "2030-01-04" }] }, context), /R\+1 日期/)
  // 除权组 split_event_ids 与账簿不一致拒绝（空组同时触发组事实校验）。
  assert.throws(() => parseSessionCorporateActions({ ...actions, applied_ex_reference_groups: [{ ...group, split_event_ids: [] }] }, context), /除权除息组事实非法/)
  // 名册存在无对应账簿的重新计值回执拒绝（前向勾稽先命中：账簿缺少该回执）。
  const orphanRegistry = { ...registry, receipts: [{ ...redenominationDay, request: { ...redenominationDay.request, event_id: "share-split:ghost" } }] }
  assert.throws(() => parseSessionCorporateActions({ ...actions, registries: [orphanRegistry] }, context), /重新计值回执/)
})
