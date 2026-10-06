import assert from "node:assert/strict"
import test from "node:test"

import { parseSessionCorporateActions } from "./schema/corporate-actions.ts"
import { parseSaveSnapshot, validateCashExReferenceFacts } from "./schema/save-snapshot.ts"

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

test("公司行为状态要求全部六个显式数组，空数组表示已配置的空状态", () => {
  assert.deepEqual(parseSessionCorporateActions({ registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: [] }, emptyContext), {
    registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: [],
  })
  for (const missing of [
    { dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: [] },
    { registries: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: [] },
    { registries: [], dividends: [], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: [] },
    { registries: [], dividends: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] },
    { registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [] },
    { registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], applied_ex_dividend_groups: [] },
  ]) {
    assert.throws(() => parseSessionCorporateActions(missing, emptyContext), /corporate_actions.*必填/)
  }
  assert.throws(() => parseSessionCorporateActions({ registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: [], extra: [] }, emptyContext), /corporate_actions\.extra/)
})

test("公司行为状态要求六个显式数组并严格校验股息税账序列", () => {
  const taxBook = {
    operation_seq: 0, account: "0", stock: "600101", profile: "IndividualPublicMarket",
    opened_on: "2030-01-03", opening_lots: [], settled_on: "2030-01-03", lots: [], days: [], dividends: [], collections: [],
  }
  const lot = { id: "account-lot", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const registry = { ...validRegistry(), holdings: [{ holder: { Account: "0" }, lots: [lot] }, { holder: { External: "holder-a" }, lots: [] }] }
  const actions = { registries: [registry], dividends: [], dividend_tax_books: [taxBook], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: [] }
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
  const registration = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-04", issued_shares: "10", issuer_repurchase_account: null, holdings: [{ holder, lots: [lot] }] }
  const registry = { ...validRegistry(), settled_on: "2030-01-08", holdings: [{ holder, lots: [lot] }], registrations: [registration] }
  const payment = { payment_id: "payment-a", paid_on: "2030-01-06", within_six_month_deadline: true, outcomes: [{ Paid: { holder, amount: "10" } }] }
  const dividend = {
    plan: { plan_id: "plan-a", issuer: "C-600101", stock: "600101", exchange: "Shanghai", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-04", ex_dividend_on: "2030-01-05", payable_on: "2030-01-06", gross_per_share: "1", distributable_amount: "10" },
    status: "Paid", registration, entitlements: [{ holder, shares: "10", gross: "10" }], paid: [[holder, "10"]], failures: [], payments: [payment],
  }
  const taxLot = { id: "tax-account-lot", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, class: "PublicMarket" }
  const taxBook = { operation_seq: 0, account: "0", stock: "600101", profile: "IndividualPublicMarket", opened_on: "2030-01-08", opening_lots: [taxLot], settled_on: "2030-01-08", lots: [taxLot], days: [], dividends: [], collections: [] }
  const receipt = { payment_id: "payment-a", plan_id: "plan-a", account: "0", paid_on: "2030-01-06", gross: "10", tax_status: "IndividualPublicMarket" }
  const actions = { registries: [registry], dividends: [dividend], dividend_tax_books: [taxBook], account_gross_receipts: [receipt], external_receipts: [], applied_ex_dividend_groups: [] }
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
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }, emptyContext), /issued_shares.*字符串/)
})

test("股东名册保留 u64 字符串并验证身份、FIFO 和股数守恒", () => {
  const maximum = "18446744073709551615"
  const actions = { registries: [validRegistry(maximum)], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }
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
  const snapshot = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", issued_shares: "10", issuer_repurchase_account: null, holdings: validRegistry().holdings }
  const registry = { ...validRegistry(), registrations: [snapshot, snapshot] }
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }, registryContext()), /登记快照.*身份重复/)
})

test("分红书必须显式保存 required nullable registration 和完整空付款状态", () => {
  const book = {
    plan: { plan_id: "plan-a", issuer: "C-600101", stock: "600101", exchange: "Shanghai", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-04", ex_dividend_on: "2030-01-07", payable_on: "2030-01-08", gross_per_share: "1", distributable_amount: "100" },
    status: "Approved", registration: null, entitlements: [], paid: [], failures: [], payments: [],
  }
  const registry = { ...validRegistry(), settled_on: "2030-01-03" }
  assert.equal(parseSessionCorporateActions({ registries: [registry], dividends: [book], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }, registryContext()).dividends[0]?.registration, null)
  const { registration: _registration, ...missing } = book
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [missing], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }, registryContext()), /registration.*必填/)
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [{ ...book, status: "Unexpected" }], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }, registryContext()), /status.*分红状态/)
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends: [{ ...book, plan: { ...book.plan, exchange: "Shenzhen" } }], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }, registryContext()), /exchange.*交易所与证券不一致/)
})

test("External 分红到账回执必须逐项匹配 Paid 金额、持有人、方案和付款日", () => {
  const holder = { External: "holder-a" }
  const lot = { id: "lot-a", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const registration = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-04", issued_shares: "10", issuer_repurchase_account: null, holdings: [{ holder, lots: [lot] }] }
  const registry = { stock: "600101", issuer: "C-600101", issued_shares: "10", issuer_repurchase_account: null, settled_on: "2030-01-08", holdings: [{ holder, lots: [lot] }], receipts: [], registrations: [registration] }
  const payment = { payment_id: "payment-a", paid_on: "2030-01-06", within_six_month_deadline: true, outcomes: [{ Paid: { holder, amount: "10" } }] }
  const book = {
    plan: { plan_id: "plan-a", issuer: "C-600101", stock: "600101", exchange: "Shanghai", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-04", ex_dividend_on: "2030-01-05", payable_on: "2030-01-06", gross_per_share: "1", distributable_amount: "100" },
    status: "Paid", registration, entitlements: [{ holder, shares: "10", gross: "10" }], paid: [[holder, "10"]], failures: [], payments: [payment],
  }
  const grossReceipt = { payment_id: payment.payment_id, plan_id: "plan-a", holder, paid_on: payment.paid_on, gross: "10", tax_status: "TreatmentNotConfigured" }
  const actions = { registries: [registry], dividends: [book], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [grossReceipt], applied_ex_dividend_groups: [] }
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
  const group = { date: anchor.ex_date, stock: "600101", plan_ids: ["plan-a"], reference: anchor }
  const actions = { registries: [], dividends: [], dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: [group] }
  assert.throws(() => parseSessionCorporateActions(actions, context), /计划.*不一致/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, applied_ex_dividend_groups: [] }, context), /缺少对应已应用除息组/)
  assert.throws(() => parseSessionCorporateActions({ ...actions, applied_ex_dividend_groups: [{ ...group, stock: "600101", reference: { ...anchor, reference_price: "1002" } }] }, context), /MarketSnap 锚点不一致/)
})

test("MarketSnap 除息锚点必须匹配证券最新已应用除息组", () => {
  const holder = { External: "holder-a" }
  const lot = { id: "lot-a", qty: "10", acquired_on: "2030-01-01", source: { InitialAllocation: { evidence: "setup" } }, restriction: "Unrestricted" }
  const registrationA = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-01", issued_shares: "10", issuer_repurchase_account: null, holdings: [{ holder, lots: [lot] }] }
  const registrationB = { ...registrationA, event_id: "plan-b" }
  const registry = { ...validRegistry(), registrations: [registrationA, registrationB] }
  const makeDividend = (plan_id: string, ex_dividend_on: string, registration: typeof registrationA) => ({
    plan: { plan_id, issuer: "C-600101", stock: "600101", exchange: "Shanghai", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-01", registered_on: "2030-01-01", ex_dividend_on, payable_on: ex_dividend_on, gross_per_share: "1", distributable_amount: "10" },
    status: "Registered", registration, entitlements: [{ holder, shares: "10", gross: "10" }], paid: [], failures: [], payments: [],
  })
  const dividends = [makeDividend("plan-a", "2030-01-02", registrationA), makeDividend("plan-b", "2030-01-03", registrationB)]
  const anchor = { ex_date: "2030-01-02", reference_price: "1000" }
  const laterReference = { ex_date: "2030-01-03", reference_price: "999" }
  const context = { ...registryContext(), snapshot: { markets: { "600101": { last_cash_ex_reference: anchor } }, accounts: {} } }
  const groups = [
    { date: anchor.ex_date, stock: "600101", plan_ids: ["plan-a"], reference: anchor },
    { date: laterReference.ex_date, stock: "600101", plan_ids: ["plan-b"], reference: laterReference },
  ]
  assert.throws(() => parseSessionCorporateActions({ registries: [registry], dividends, dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: groups }, context), /锚点必须匹配最新已应用除息组/)
  const currentAnchorContext = { ...context, snapshot: { markets: { "600101": { last_cash_ex_reference: laterReference } }, accounts: {} } }
  assert.equal(parseSessionCorporateActions({ registries: [registry], dividends, dividend_tax_books: [], account_gross_receipts: [], external_receipts: [], applied_ex_dividend_groups: groups }, currentAnchorContext).applied_ex_dividend_groups.length, 2)
})

test("股东名册与登记快照必须显式保存 issuer_repurchase_account nullable", () => {
  const registry = validRegistry()
  const { issuer_repurchase_account: _registryFacts, ...missingRegistryFacts } = registry
  assert.throws(() => parseSessionCorporateActions({ registries: [missingRegistryFacts], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }, registryContext()), /issuer_repurchase_account.*必填/)
  const registration = { event_id: "plan-a", stock: "600101", issuer: "C-600101", registered_on: "2030-01-02", issued_shares: "10", issuer_repurchase_account: null, holdings: registry.holdings }
  const { issuer_repurchase_account: _snapshotFacts, ...missingSnapshotFacts } = registration
  assert.throws(() => parseSessionCorporateActions({ registries: [{ ...registry, registrations: [missingSnapshotFacts] }], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }, registryContext()), /issuer_repurchase_account.*必填/)
})

test("回购专户事实按建立日期绑定到不可变登记快照", () => {
  const facts = { account_reference: "issuer-buyback-1", source_evidence: "exchange-confirmation-1", established_on: "2030-01-02" }
  const before = { event_id: "before", stock: "600101", issuer: "C-600101", registered_on: "2030-01-01", issued_shares: "10", issuer_repurchase_account: null, holdings: validRegistry().holdings }
  const after = { ...before, event_id: "after", registered_on: "2030-01-03", issuer_repurchase_account: facts }
  const registry = { ...validRegistry(), issuer_repurchase_account: facts, registrations: [before, after] }
  const actions = { registries: [registry], dividends: [], account_gross_receipts: [], dividend_tax_books: [], external_receipts: [], applied_ex_dividend_groups: [] }
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
