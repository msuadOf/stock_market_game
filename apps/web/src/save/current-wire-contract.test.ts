import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { createHash } from "node:crypto"
import test from "node:test"
import { currentSaveFixture } from "./current-save-fixture.ts"
import { parseSetup } from "./schema/market.ts"
import { parseSaveJson, parseSaveSlot } from "./save-schema.ts"
import { parseInformationStates } from "./schema/personal/information.ts"
import { parseBeliefBooks, type BeliefBook } from "./schema/personal/beliefs.ts"
import { parseRawSaveJson } from "./schema/raw-json.ts"
import { finite, integer, materializeSaveValue, RawSaveNumber, SaveSchemaError } from "./schema/primitives.ts"

// 从真实 Rust producer 原档提取，字段和值均未投影；来源与原档哈希见工作记录。
const fragments = JSON.parse(readFileSync(new URL("./fixtures/current-producer-fragments.json", import.meta.url), "utf8")) as Record<string, unknown>

// synthetic：当前身份 codec 的独立正控，不改写历史 Rust fragment 冒充新 producer。
const syntheticBelief = {
  npc: "3", profile: { Retail: "Noise" }, institution_account_risk_paused: false,
  analysis: { fundamental_bp: 0, trend_bp: 2500, price_volume_bp: 2500, technical_bp: 2500, experience_cost_bp: 2500, fundamental_method: null },
  assumptions: { growth_deviation_bp: 0, quality_coefficient_bp: 0, pe_multiple: 0, equity_cost_bp: 0, terminal_growth_bp: 0, roe_deviation_bp: 0 },
  entries: { "0": { company: 0, method: null, forecast: { growth_bp: null, basis: "InitialWithoutHistory" }, confidence_bp: 3000, valuation: { Unavailable: { reason: "MethodDisabled" } }, used_report_ids: [], anchor: { kind: "NoPriorSession" }, horizon_trading_days: 1, last_cause: null, applied_experience_orders: [] } },
  experience: { reference_equity: null, peak_equity: null, consecutive_failed_buys: 0, stocks: {} }, institution_policy: null,
} satisfies BeliefBook

test("parseSaveJson 保留原始 JSON 约束并将数值完整物化", { timeout: 10_000 }, () => {
  const fixtureText = JSON.stringify(currentSaveFixture())
  const parsed = parseSaveJson(fixtureText)
  assert.deepEqual(parsed, currentSaveFixture())
  const visit = (value: unknown): void => {
    assert.ok(!(value instanceof RawSaveNumber), "物化存档不得泄漏 RawSaveNumber")
    if (Array.isArray(value)) value.forEach(visit)
    else if (value !== null && typeof value === "object") Object.values(value).forEach(visit)
  }
  visit(parsed)

  assert.deepEqual(parseRawSaveJson(String.raw`{"\u6c49\u5b57":"\u503c\ud83d\ude00"}`), { "汉字": "值😀" })
  assert.throws(() => parseSaveJson(fixtureText.replace('"start_date"', String.raw`"\ud800"`)), /孤立 Unicode surrogate/)
  assert.throws(() => parseSaveJson(fixtureText.replace('"start_date":"2030-01-01"', String.raw`"start_date":"\ud800"`)), /孤立 Unicode surrogate/)
  assert.throws(() => parseSaveJson(fixtureText.replace('"receipt_watermark":0', '"receipt_watermark":0,"receipt_watermark":0')), /重复 JSON 键/)
  for (const token of ["9007199254740992", "-0", "0.0", "0e0"]) {
    const text = fixtureText.replace('"receipt_watermark":0', `"receipt_watermark":${token}`)
    assert.notEqual(text, fixtureText)
    assert.throws(() => parseSaveJson(text), /receipt_watermark/)
  }
})

test("materializeSaveValue 保留新对象树、键契约、数值 codec 与稀疏数组语义", { timeout: 10_000 }, () => {
  const rawInteger = parseRawSaveJson("7")
  const rawFinite = parseRawSaveJson("1.25")
  assert.ok(rawInteger instanceof RawSaveNumber)
  assert.ok(rawFinite instanceof RawSaveNumber)
  integer(rawInteger, "根节点.count")
  finite(rawFinite, "根节点.ratio")

  const source: Record<string, unknown> = { "10": "ten", "2": "two", nested: { value: rawFinite }, count: rawInteger }
  Object.defineProperty(source, "__proto__", { value: { safe: true }, writable: true, enumerable: true, configurable: true })
  const materialized = materializeSaveValue(source)
  assert.notEqual(materialized, source)
  assert.notEqual(materialized.nested, source.nested)
  assert.equal(source.count, rawInteger)
  assert.equal((source.nested as { value: unknown }).value, rawFinite)
  assert.equal(materialized["2"], "two")
  assert.equal(materialized["10"], "ten")
  assert.deepEqual(materialized.nested, { value: 1.25 })
  assert.equal(materialized.count, 7)
  assert.deepEqual(materialized.__proto__, { safe: true })
  assert.notEqual(materialized.__proto__, source.__proto__)
  assert.deepEqual(Object.keys(materialized), ["2", "10", "nested", "count", "__proto__"])
  assert.equal(Object.getPrototypeOf(materialized), Object.prototype)
  assert.deepEqual(Object.getOwnPropertyDescriptor(materialized, "__proto__"), {
    value: { safe: true }, writable: true, enumerable: true, configurable: true,
  })
  assert.ok(rawInteger instanceof RawSaveNumber)
  assert.ok(rawFinite instanceof RawSaveNumber)
  assert.equal(rawInteger.decoded, 7)
  assert.equal(rawFinite.decoded, 1.25)

  const unconsumed = parseRawSaveJson("2")
  assert.ok(unconsumed instanceof RawSaveNumber)
  assert.throws(() => materializeSaveValue({ nested: unconsumed }, "存档"), (error) => {
    assert.ok(error instanceof SaveSchemaError)
    assert.equal(error.path, "存档.nested")
    assert.equal(error.message, "存档 存档.nested 数值未经过字段 codec 校验")
    return true
  })

  const sparse = new Array<unknown>(3)
  sparse[1] = { value: true }
  const sourceArray = [sparse] as const
  const materializedArray = materializeSaveValue(sourceArray)
  const materializedSparse = materializedArray[0]
  assert.notEqual(materializedArray, sourceArray)
  assert.notEqual(materializedSparse, sparse)
  assert.notEqual(materializedSparse[1], sparse[1])
  assert.equal(materializedSparse.length, 3)
  assert.equal(Object.hasOwn(materializedSparse, 0), false)
  assert.equal(Object.hasOwn(materializedSparse, 1), true)
  assert.equal(Object.hasOwn(materializedSparse, 2), false)

  const empty = materializeSaveValue({})
  assert.equal(Object.getPrototypeOf(empty), Object.prototype)
})

test("materializeSaveValue 对继承 setter 与不可写属性仍建立普通 own data 属性", { timeout: 10_000 }, () => {
  const setterKey = "materializeSetterProbe"
  const lockedKey = "materializeLockedProbe"
  const setterDescriptor = Object.getOwnPropertyDescriptor(Object.prototype, setterKey)
  const lockedDescriptor = Object.getOwnPropertyDescriptor(Object.prototype, lockedKey)
  let setterCalls = 0
  try {
    Object.defineProperty(Object.prototype, setterKey, {
      configurable: true,
      set() { setterCalls++ },
    })
    Object.defineProperty(Object.prototype, lockedKey, {
      configurable: true,
      value: "inherited sentinel",
      writable: false,
    })

    const materialized = materializeSaveValue({ [setterKey]: "setter value", [lockedKey]: "locked value" })
    assert.equal(setterCalls, 0)
    for (const [key, value] of [[setterKey, "setter value"], [lockedKey, "locked value"]] as const) {
      assert.equal(Object.hasOwn(materialized, key), true)
      assert.deepEqual(Object.getOwnPropertyDescriptor(materialized, key), {
        value, writable: true, enumerable: true, configurable: true,
      })
    }
  } finally {
    if (setterDescriptor === undefined) delete (Object.prototype as Record<string, unknown>)[setterKey]
    else Object.defineProperty(Object.prototype, setterKey, setterDescriptor)
    if (lockedDescriptor === undefined) delete (Object.prototype as Record<string, unknown>)[lockedKey]
    else Object.defineProperty(Object.prototype, lockedKey, lockedDescriptor)
  }
})

test("parseSaveSlot 对外部对象继续拒绝循环、稀疏数组和非 JSON 值", { timeout: 10_000 }, () => {
  const cyclic = currentSaveFixture()
  ;(cyclic as Record<string, unknown>).cycle = cyclic
  assert.throws(() => parseSaveSlot(cyclic), /循环引用/)

  const sparse = currentSaveFixture()
  const holes = new Array(1)
  ;(sparse as Record<string, unknown>).sparse = holes
  assert.throws(() => parseSaveSlot(sparse), /数组元素缺失/)

  for (const value of [undefined, 1n, () => undefined, Symbol("value")]) {
    const invalid = currentSaveFixture()
    ;(invalid as Record<string, unknown>).invalid = value
    assert.throws(() => parseSaveSlot(invalid), /不是 JSON 值/)
  }
})

test("当前存档严格保留必填 desk_mirrored_resting，不混入默认值", { timeout: 10_000 }, () => {
  const current = currentSaveFixture()
  for (const desk_mirrored_resting of [false, true]) {
    const saved = { ...current, desk_mirrored_resting }
    assert.deepEqual(parseSaveSlot(saved), saved)
  }
  const { desk_mirrored_resting: _removed, ...missing } = current
  assert.throws(() => parseSaveSlot(missing), /desk_mirrored_resting.*必填/)
  for (const invalid of [0, "false", null, undefined]) {
    assert.throws(() => parseSaveSlot({ ...current, desk_mirrored_resting: invalid }), /desk_mirrored_resting/)
  }
})

test("真实 InformationState 保留身份；历史 Rust BeliefBook scalar 明确拒绝", { timeout: 10_000 }, () => {
  assert.deepEqual(parseInformationStates({ "1": fragments.information_state }), { "1": fragments.information_state })
  assert.throws(() => parseBeliefBooks({ "3": fragments.belief_book }), /belief_books\.3\.entries\.0\.anchor/)
  assert.deepEqual(parseBeliefBooks({ "3": syntheticBelief }), { "3": syntheticBelief })
})

test("真实 Rust 三场景完整非空 Book 保留 typed INFO anchor 与 Cause", { timeout: 10_000 }, () => {
  const bytes = readFileSync(new URL("./fixtures/typed-info/rust-three-belief-books.json", import.meta.url))
  assert.equal(createHash("sha256").update(bytes).digest("hex"), "2351428e962a56c9becadc2b0e4817d809ec9597df8c89220c32b63ecf9a5ec6")
  const rows = JSON.parse(bytes.toString("utf8")) as { scenario: string; book: BeliefBook; anchor: unknown }[]
  assert.deepEqual(rows.map(row => row.scenario), ["no-prior", "trading-zero", "day-before-zero"])
  assert.deepEqual(rows.map(row => row.anchor), [{ kind: "NoPriorSession" }, { kind: "TradingDay", ordinal: "0" }, { kind: "DayBefore", ordinal: "0" }])
  for (const { book, anchor } of rows) {
    assert.deepEqual(parseBeliefBooks({ [book.npc]: book }), { [book.npc]: book })
    assert.deepEqual(materializeSaveValue(parseBeliefBooks(parseRawSaveJson(JSON.stringify({ [book.npc]: book })))), { [book.npc]: book })
    const entries = Object.values(book.entries)
    assert.ok(entries.length > 0)
    for (const entry of entries) {
      assert.deepEqual(entry.anchor, anchor)
      assert.ok(entry.last_cause !== null)
      assert.deepEqual(entry.last_cause.as_of_anchor, anchor)
      assert.ok(entry.used_report_ids.length > 0)
    }
  }
})

test("个人存档拒绝非规范 AccountId、未知字段、错误 owner 和非数值 CompanyId", { timeout: 10_000 }, () => {
  const information = fragments.information_state as Record<string, unknown>
  const belief = syntheticBelief
  for (const invalid of [1, "01", "+1", "-1", "18446744073709551616"]) {
    assert.throws(() => parseInformationStates({ "1": { ...information, owner: invalid } }), /owner/)
    assert.throws(() => parseBeliefBooks({ "3": { ...belief, npc: invalid } }), /npc/)
  }
  assert.throws(() => parseInformationStates({ "1": { ...information, owner: "2" } }), /owner/)
  assert.throws(() => parseBeliefBooks({ "3": { ...belief, npc: "2" } }), /npc/)
  assert.throws(() => parseInformationStates({ "1": { ...information, extra: true } }), /extra/)
  const entries = belief.entries
  for (const company of ["0", -1, 4_294_967_296, null]) {
    assert.throws(() => parseBeliefBooks({ "3": { ...belief, entries: { "0": { ...entries["0"], company } } } }), /company/)
  }
})


test("parseSetup 接入完整公司选项且保留独立 report_frequency", { timeout: 10_000 }, () => {
  const base = currentSaveFixture().setup as Record<string, unknown>
  const setup = {
    ...base,
    company_system: { mode: "Simulation" }, dividend_tax_mode: "FlatWithholding", flat_withholding_bp: 1_000,
    rights_offering_enabled: true, issuer_repurchase_enabled: true, auto_corporate_foundation: true,
    par_value_per_share: "9007199254740993", report_frequency: { Monthly: { schedule: { Custom: { day: 15, second_of_day: 64_800, delay: "None" } } } },
  }
  assert.deepEqual(parseSetup(setup, "setup"), setup)
  assert.deepEqual(parseSetup({ ...base, report_frequency: "Quarterly" }, "setup"), { ...base, report_frequency: "Quarterly" })
  assert.throws(() => parseSetup({ ...setup, company_system: { mode: "Simulation", extra: true } }, "setup"), /extra/)
  assert.throws(() => parseSetup({ ...setup, dividend_tax_mode: "Legacy" }, "setup"), /dividend_tax_mode/)
  assert.throws(() => parseSetup({ ...setup, extra: true }, "setup"), /extra/)
})

test("parseRawSaveJson 保留 JSON 词法与字符串边界", { timeout: 10_000 }, () => {
  assert.deepEqual(parseRawSaveJson(String.raw`{"plain":"same","escaped":"\u0073ame"}`), { plain: "same", escaped: "same" })
  assert.deepEqual(parseRawSaveJson(String.raw`{"quote":"a\"b","slash":"a\\b"}`), { quote: 'a"b', slash: "a\\b" })
  assert.deepEqual(parseRawSaveJson('{"plain":"😀","escaped":"\\ud83d\\ude00"}'), { plain: "😀", escaped: "😀" })
  assert.deepEqual(parseRawSaveJson(String.raw`{"\u0001":"\u0001"}`), { "\u0001": "\u0001" })
  assert.throws(() => parseRawSaveJson(String.raw`{"a":1,"\u0061":2}`), /重复 JSON 键/)

  assert.deepEqual(parseRawSaveJson("[ \t\r\n null , true , false ]"), [null, true, false])
  for (const whitespace of ["\v", "\f"]) assert.throws(() => parseRawSaveJson(`[${whitespace}null]`), /合法 JSON/)
  assert.throws(() => parseRawSaveJson("[true:false]"), /缺少逗号或结束符/)
  for (const value of ["nullx", "true0", "false_", "truefalse"]) {
    assert.throws(() => parseRawSaveJson(value), /根值之后包含多余内容/)
  }

  for (const token of ["0", "-1", "1.25", "2e+3", "4E-2"]) {
    const parsed = parseRawSaveJson(token)
    assert.ok(parsed instanceof RawSaveNumber)
    assert.equal(parsed.token, token)
  }
  for (const token of ["+1", "01", "1.", ".1", "1e"]) assert.throws(() => parseRawSaveJson(token), /非法 JSON 数值或值/)

  const escapedControls = parseRawSaveJson(String.raw`{"value":"line\n\u0001"}`) as Record<string, unknown>
  assert.equal(escapedControls.value, "line\n\u0001")
  for (const raw of [
    `{"value":"a\u0001b"}`,
    `{"a\u0001b":1}`,
    String.raw`{"value":"\ud800"}`,
    `{"value":"\ud800"}`,
    String.raw`{"value":"\udfff"}`,
    String.raw`{"value":"\q"}`,
    '{"value":"truncated',
    String.raw`{"\ud800":1}`,
    `{"\ud800":1}`,
  ]) assert.throws(() => parseRawSaveJson(raw), /非法 JSON 字符串|JSON 字符串未闭合|孤立 Unicode surrogate/)

  const prototypeKey = parseRawSaveJson(String.raw`{"__proto__":{"polluted":true}}`) as Record<string, unknown>
  assert.equal(Object.getPrototypeOf(prototypeKey), Object.prototype)
  assert.equal(Object.hasOwn(prototypeKey, "__proto__"), true)
  assert.deepEqual(prototypeKey.__proto__, { polluted: true })
  assert.equal(({} as { polluted?: boolean }).polluted, undefined)

  const emptyObject = parseRawSaveJson("{}") as Record<string, unknown>
  assert.equal(Object.getPrototypeOf(emptyObject), Object.prototype)

  const specialKeys = parseRawSaveJson(String.raw`{"__proto__":1,"constructor":2,"toString":3,"get":4,"set":5,"10":"ten","2":"two","nested":{"__proto__":{"safe":true}},"other":{"value":"tail"}}`) as Record<string, unknown>
  assert.equal(Object.getPrototypeOf(specialKeys), Object.prototype)
  assert.deepEqual(Object.keys(specialKeys), ["2", "10", "__proto__", "constructor", "toString", "get", "set", "nested", "other"])
  for (const key of ["__proto__", "constructor", "toString", "get", "set"]) {
    assert.equal(Object.hasOwn(specialKeys, key), true)
    assert.deepEqual(Object.getOwnPropertyDescriptor(specialKeys, key), {
      value: specialKeys[key], writable: true, enumerable: true, configurable: true,
    })
  }
  const nested = specialKeys.nested as Record<string, unknown>
  assert.equal(Object.getPrototypeOf(nested), Object.prototype)
  assert.equal(Object.hasOwn(nested, "__proto__"), true)
  assert.deepEqual(nested.__proto__, { safe: true })
  assert.deepEqual(specialKeys.other, { value: "tail" })
  assert.notEqual(specialKeys.nested, specialKeys.other)

  const setterKey = "rawJsonSetterProbe"
  const lockedKey = "rawJsonLockedProbe"
  const setterDescriptor = Object.getOwnPropertyDescriptor(Object.prototype, setterKey)
  const lockedDescriptor = Object.getOwnPropertyDescriptor(Object.prototype, lockedKey)
  let setterCalls = 0
  try {
    Object.defineProperty(Object.prototype, setterKey, {
      configurable: true,
      set() { setterCalls++ },
    })
    Object.defineProperty(Object.prototype, lockedKey, {
      configurable: true,
      value: "inherited sentinel",
      writable: false,
    })

    const guarded = parseRawSaveJson(String.raw`{"rawJsonSetterProbe":"setter value","rawJsonLockedProbe":"locked value"}`) as Record<string, unknown>
    assert.equal(setterCalls, 0)
    for (const [key, value] of [[setterKey, "setter value"], [lockedKey, "locked value"]] as const) {
      assert.equal(Object.hasOwn(guarded, key), true)
      assert.deepEqual(Object.getOwnPropertyDescriptor(guarded, key), {
        value, writable: true, enumerable: true, configurable: true,
      })
    }
  } finally {
    if (setterDescriptor === undefined) delete (Object.prototype as Record<string, unknown>)[setterKey]
    else Object.defineProperty(Object.prototype, setterKey, setterDescriptor)
    if (lockedDescriptor === undefined) delete (Object.prototype as Record<string, unknown>)[lockedKey]
    else Object.defineProperty(Object.prototype, lockedKey, lockedDescriptor)
  }
})

test("parseRawSaveJson 的错误路径保留既有拼接语义", { timeout: 10_000 }, () => {
  const cases: [string, string][] = [
    [String.raw`{"outer":{"a.b[0]":{"key":[{},false,bad]}}}`, "根节点.outer.a.b[0].key[2]"],
    [String.raw`{"\u0061":null,"a":0}`, "根节点.a"],
    ['{"雪":"值😀","next":bad}', "根节点.next"],
    [String.raw`{"outer":{"value":"\ud800"}}`, "根节点.outer.value"],
    [String.raw`{"outer":{"\ud800":1}}`, "根节点.outer.key"],
    [String.raw`{"items":[true,bad]}`, "根节点.items[1]"],
    [String.raw`{"outer":{"field" 1}}`, "根节点.outer"],
    [String.raw`{"outer":{"field":1 "next":2}}`, "根节点.outer"],
    [String.raw`{"outer":[1`, "根节点.outer"],
    [String.raw`[{},[],null false]`, "根节点"],
    [String.raw`{"empty":{},"array":[],"nil":null,"flag":true,"count":1,"next":bad}`, "根节点.next"],
    [String.raw`{"empty":{},"array":[],"nil":null,"flag":true,"count":1,"next":falsefalse}`, "根节点"],
  ]

  for (const [raw, expectedPath] of cases) {
    assert.throws(
      () => parseRawSaveJson(raw),
      (error: unknown) => error instanceof SaveSchemaError && error.path === expectedPath,
      `输入 ${raw} 应保留错误路径 ${expectedPath}`,
    )
  }
})

test("原型存在 get/set 字段时私有解析仍保持完整存档", { timeout: 10000 }, () => {
  const text = JSON.stringify(currentSaveFixture());
  const expected = parseSaveJson(text);
  const descriptor = Object.assign(Object.create(null), { value: 1, writable: true, configurable: true });
  const previousGet = Object.getOwnPropertyDescriptor(Object.prototype, "get");
  const previousSet = Object.getOwnPropertyDescriptor(Object.prototype, "set");
  Object.defineProperty(Object.prototype, "get", descriptor);
  Object.defineProperty(Object.prototype, "set", descriptor);
  try {
    assert.deepEqual(parseSaveJson(text), expected);
  } finally {
    if (previousGet) Object.defineProperty(Object.prototype, "get", previousGet);
    else delete (Object.prototype as Record<string, unknown>).get;
    if (previousSet) Object.defineProperty(Object.prototype, "set", previousSet);
    else delete (Object.prototype as Record<string, unknown>).set;
  }
});
