import assert from "node:assert/strict"
import test from "node:test"
import { parseSaveJson, parseSaveSlot } from "./save-schema.ts"
import { parseSaveSnapshot } from "./schema/save-snapshot.ts"
import { parseSaveRuntime } from "./schema/runtime-state.ts"
import { parseOrderState } from "./schema/orders.ts"
import { currentSaveFixture } from "./current-save-fixture.ts"

function mutateRuntime(mutator: (runtime: Record<string, unknown>) => void): unknown {
  const save = structuredClone(currentSaveFixture())
  const runtime = save.runtime_state
  assert.ok(runtime !== null && typeof runtime === "object" && !Array.isArray(runtime))
  mutator(runtime as Record<string, unknown>)
  return save
}

function validEnvelope(): Record<string, unknown> {
  return {
    key: { account: 0, stock: "600888", order: 1, side: "Buy" },
    charged: { commission: "0", stamp_tax: "0", transfer_fee: "0" },
  }
}

function validReceipt(): Record<string, unknown> {
  return {
    index: "0",
    local_key: {
      journal: "SealedBatch",
      source: { SealedIntent: "0" },
      transition: {
        envelope: { account: 0, stock: "600888", order: 1, side: "Buy" },
        ordinal: "0",
      },
    },
  }
}

test("存档运行时校验边界保留必填权威状态，不保存旧 profile", () => {
  const save = currentSaveFixture()
  assert.equal("strategy_profiles" in save, false)
  assert.deepEqual(parseSaveSlot(save), save)
  assert.deepEqual(parseSaveJson(JSON.stringify(save)), save)
})

test("存档流通盘设置完整保存类间与类内方式，并拒绝旧 shape", () => {
  const save = currentSaveFixture()
  const setup = save.setup as Record<string, unknown>
  assert.deepEqual(setup.float_allocation, {
    between_kinds: { Percentage: { retail: 1, inst: 0, hot: 0 } },
    within_kind: "Random",
  })
  const oldShape = structuredClone(save)
  const oldSetup = oldShape.setup as Record<string, unknown>
  oldSetup.float_allocation = { ByKind: { retail: 1, inst: 0, hot: 0 } }
  assert.throws(() => parseSaveSlot(oldShape), /setup\.float_allocation\.between_kinds/)
})

test("存档 Money 保留完整 i64 分值并拒绝旧 number 编码", { timeout: 10_000 }, () => {
  for (const cash of ["0", "9007199254740993", "9223372036854775807"]) {
    const save = currentSaveFixture()
    const snapshot = save.snapshot as Record<string, unknown>
    snapshot.accounts = { "0": { cash, positions: {} } }
    assert.deepEqual(parseSaveJson(JSON.stringify(save)), save)
  }
  for (const cash of [0, 1000000000000, "00", "+1", "-0", "1.0", "-1", "-9223372036854775808", "9223372036854775808", "-9223372036854775809"]) {
    const save = currentSaveFixture()
    const snapshot = save.snapshot as Record<string, unknown>
    snapshot.accounts = { "0": { cash, positions: {} } }
    assert.throws(() => parseSaveSlot(save), /snapshot\.accounts\.0\.cash/)
  }
})

test("save snapshot accepts only raw account and market facts", () => {
  const save = currentSaveFixture()
  const snapshot = save.snapshot as Record<string, unknown>
  const market = (snapshot.markets as Record<string, Record<string, unknown>>)["600101"]
  const account = (snapshot.accounts as Record<string, Record<string, unknown>>)["0"]
  assert.deepEqual(Object.keys(market ?? {}).sort(), ["last_close", "last_price"])
  assert.deepEqual(Object.keys(account ?? {}).sort(), ["cash", "positions"])
  assert.throws(() => parseSaveSlot({
    ...save,
    snapshot: {
      ...snapshot,
      markets: { "600101": { ...market, bids: [] } },
    },
  }), /snapshot\.markets\.600101\.bids/)
  assert.throws(() => parseSaveSlot({
    ...save,
    snapshot: {
      ...snapshot,
      accounts: { "0": { ...account, reserved_cash: 0 } },
    },
  }), /snapshot\.accounts\.0\.reserved_cash/)
})

test("save snapshot derives day and phase instead of accepting persisted mirrors", () => {
  const snapshot = currentSaveFixture().snapshot
  assert.ok(typeof snapshot === "object" && snapshot !== null && !Array.isArray(snapshot))
  assert.deepEqual(parseSaveSnapshot(snapshot, "snapshot"), snapshot)
  assert.throws(() => parseSaveSnapshot({ ...snapshot, day: 0 }, "snapshot"), /snapshot\.day/)
  assert.throws(() => parseSaveSnapshot({ ...snapshot, phase: "Continuous" }, "snapshot"), /snapshot\.phase/)
})

test("runtime envelope persists identity and actual charges only", () => {
  const charged = { commission: "3", stamp_tax: "2", transfer_fee: "1" }
  const envelope = {
    key: { account: 0, stock: "600888", order: 1, side: "Sell" },
    charged,
  }
  const runtime = {
    poisoned: false,
    next_receipt_base: "0",
    live_envelopes: [envelope],
    retail_projection_seen: [],
    strategy_states: {},
  }
  assert.deepEqual(parseSaveRuntime(runtime), runtime)
  for (const field of ["live", "limit", "remaining_qty", "filled_qty", "filled_value", "nominal"]) {
    assert.throws(() => parseSaveRuntime({
      ...runtime,
      live_envelopes: [{ ...envelope, [field]: field === "live" ? { cash: "0", shares: 1 } : 1 }],
    }), new RegExp(`live_envelopes\\[0\\]\\.${field}`))
  }
})

test("saved parent plans omit child remaining quantity and reject injected mirrors", () => {
  const base = {
    code: "600101",
    side: "Buy",
    target_qty: 100,
    filled_qty: 0,
    child_qty: 100,
    active_child_order_id: 1,
    limit_price: "1000",
    expires_market_minute: "500",
  }
  const orderState = { ...currentSaveFixture(), parent_orders: { "1": { "600101": base } } }
  assert.deepEqual(parseOrderState(orderState).parent_orders, orderState.parent_orders)
  assert.throws(() => parseOrderState({
    ...orderState,
    parent_orders: { "1": { "600101": { ...base, active_child_remaining_qty: 100 } } },
  }), /parent_orders\.1\.600101\.active_child_remaining_qty/)
})

test("book sequence cursors preserve empty-book history and lossless u64 boundaries", () => {
  for (const cursor of ["0", "17", "9007199254740992", "18446744073709551615"]) {
    const save = { ...currentSaveFixture(), book_next_sequences: { "600101": cursor } }
    assert.deepEqual(parseSaveSlot(save), save)
    assert.deepEqual(parseSaveJson(JSON.stringify(save)), save)
  }
})

test("book sequence cursors are mandatory and reject malformed u64 strings", () => {
  const { book_next_sequences: _removed, ...missing } = currentSaveFixture()
  assert.throws(() => parseSaveSlot(missing), /book_next_sequences.*必填/)
  for (const value of [null, [], "0", { "600101": 0 }, { "600101": "-1" }, { "600101": "0.5" }, { "600101": "18446744073709551616" }, { "600101": "" }]) {
    assert.throws(() => parseSaveSlot({ ...currentSaveFixture(), book_next_sequences: value }), /book_next_sequences/)
  }
})

test("book sequence cursor keys must match both configured stocks and snapshot markets", () => {
  const current = currentSaveFixture()
  for (const cursors of [{}, { "600101": "0", "600102": "0" }, { "600102": "0" }, { "": "0" }]) {
    assert.throws(() => parseSaveSlot({ ...current, book_next_sequences: cursors }), /book_next_sequences/)
  }
  const snapshot = current.snapshot as Record<string, unknown>
  const markets = snapshot.markets as Record<string, unknown>
  for (const changedMarkets of [{}, { ...markets, "600102": markets["600101"] }]) {
    assert.throws(() => parseSaveSlot({ ...current, snapshot: { ...snapshot, markets: changedMarkets } }), /book_next_sequences/)
  }
})

test("pending player and NPC limit intents preserve fixed, highest and lowest prices", () => {
  const intents = [
    { PlaceLimit: { code: "600888", side: "Buy", price: { Fixed: "1000" }, qty: 100 } },
    { PlaceLimit: { code: "600888", side: "Buy", price: "Highest", qty: 100 } },
    { PlaceLimit: { code: "600888", side: "Sell", price: "Lowest", qty: 100 } },
  ]
  const save = {
    ...currentSaveFixture(),
    pending_player: intents.map((intent, index) => ({ owner: 0, intent, account_ordinal: String(index), stock_ordinal: String(index) })),
    pending_npc: { observed_tick: 0, observed_accounts: [1], intents: intents.map((intent, index) => ({ owner: 1, intent, account_ordinal: String(index), stock_ordinal: String(index) })), dependencies: [] },
  }
  assert.deepEqual(parseSaveSlot(save).pending_player, save.pending_player)
  assert.deepEqual(parseSaveJson(JSON.stringify(save)).pending_npc, save.pending_npc)
})

test("receipt cursors and pending ordinals preserve lossless u64 decimal strings", () => {
  const current = currentSaveFixture()
  const { ingress_receipt_cursors: _removed, ...missing } = current
  assert.throws(() => parseSaveSlot(missing), /ingress_receipt_cursors.*必填/)
  const item = { owner: 0, intent: { Cancel: { code: "600888", id: 1 } }, account_ordinal: "18446744073709551615", stock_ordinal: "18446744073709551615" }
  assert.deepEqual(parseSaveSlot({ ...current, pending_player: [item] }).pending_player, [item])
  for (const ordinal of [-1, 1.5, Number.MAX_SAFE_INTEGER + 1, "01", "+1", "-1", "18446744073709551616"]) {
    assert.throws(() => parseSaveSlot({ ...current, pending_player: [{ ...item, account_ordinal: ordinal }] }), /pending_player\[0\]\.account_ordinal/)
  }
  for (const cursors of [null, {}, { next_account_ordinal: { "01": "0" }, next_stock_ordinal: {} }, { next_account_ordinal: { "1": "01" }, next_stock_ordinal: {} }, { next_account_ordinal: { "1": "+1" }, next_stock_ordinal: {} }, { next_account_ordinal: {}, next_stock_ordinal: { "600101": 0 } }]) {
    assert.throws(() => parseSaveSlot({ ...current, ingress_receipt_cursors: cursors }), /ingress_receipt_cursors/)
  }
})

test("pending limit intents reject legacy numeric and malformed symbolic prices", () => {
  for (const price of [1000, { Fixed: 1000 }, { Fixed: "1.5" }, { Fixed: "01" }, { Fixed: "-0" }, { Fixed: "1000", Highest: true }, { Unknown: 1000 }, "Unknown", null]) {
    const intent = { PlaceLimit: { code: "600888", side: "Buy", price, qty: 100 } }
    const queued = { owner: 0, intent, account_ordinal: "0", stock_ordinal: "0" }
    const save = { ...currentSaveFixture(), pending_player: [queued] }
    assert.throws(() => parseSaveSlot(save), /pending_player\[0\].*price/, JSON.stringify(price))
    assert.throws(() => parseSaveSlot({ ...save, pending_player: [], pending_npc: { observed_tick: 0, observed_accounts: [1], intents: [{ owner: 1, intent, account_ordinal: "0", stock_ordinal: "0" }], dependencies: [] } }), /pending_npc\.intents\[0\].*price/, JSON.stringify(price))
  }
})

test("pending fixed price preserves integer amounts for later engine rejection", () => {
  for (const value of ["0", "-1"]) {
    const intent = { PlaceLimit: { code: "600888", side: "Buy", price: { Fixed: value }, qty: 100 } }
    const save = { ...currentSaveFixture(), pending_player: [{ owner: 0, intent, account_ordinal: "0", stock_ordinal: "0" }] }
    assert.deepEqual(parseSaveSlot(save).pending_player, save.pending_player)
  }
})

function pendingNpcReplacementBatch() {
  return {
    observed_tick: 0,
    observed_accounts: [1],
    intents: [
      { owner: 1, intent: { Cancel: { code: "600888", id: 42 } }, account_ordinal: "0", stock_ordinal: "0" },
      { owner: 1, intent: { Cancel: { code: "600888", id: 43 } }, account_ordinal: "1", stock_ordinal: "1" },
      { owner: 1, intent: { Cancel: { code: "600888", id: 44 } }, account_ordinal: "2", stock_ordinal: "2" },
      { owner: 1, intent: { PlaceLimit: { code: "600888", side: "Buy", price: { Fixed: "1000" }, qty: 100 } }, account_ordinal: "3", stock_ordinal: "3" },
      { owner: 1, intent: { PlaceMarket: { code: "600888", side: "Buy", qty: 100 } }, account_ordinal: "4", stock_ordinal: "4" },
    ],
    dependencies: [[0, 3], [1, 3], [1, 4]],
  }
}

test("pending NPC replacement dependencies are mandatory", () => {
  const { dependencies: _removed, ...batch } = pendingNpcReplacementBatch()
  assert.throws(
    () => parseSaveSlot({ ...currentSaveFixture(), pending_npc: batch }),
    /pending_npc\.dependencies.*必填/,
  )
})

test("pending NPC replacement dependencies preserve explicit edges without requiring live old orders", () => {
  const batch = pendingNpcReplacementBatch()
  const save = { ...currentSaveFixture(), pending_npc: batch }
  assert.deepEqual(parseSaveSlot(save).pending_npc, batch)
  assert.deepEqual(parseSaveJson(JSON.stringify(save)).pending_npc, batch)
})

test("pending NPC replacement dependencies reject malformed or unrelated edges", () => {
  const batch = pendingNpcReplacementBatch()
  const invalidEdges: readonly unknown[] = [
    null, [[0]], [[0, 3, 4]], [[-1, 3]], [[0.5, 3]], [[0, "3"]],
    [[0, Number.MAX_SAFE_INTEGER + 1]], [[0, 5]], [[3, 0]], [[0, 0]],
    [[0, 3], [0, 3]], [[0, 1]], [[3, 4]],
  ]
  for (const dependencies of invalidEdges) {
    assert.throws(
      () => parseSaveSlot({ ...currentSaveFixture(), pending_npc: { ...batch, dependencies } }),
      /pending_npc\.dependencies/,
      `invalid dependencies ${JSON.stringify(dependencies)}`,
    )
  }
  for (const replacement of [
    { owner: 2, intent: { PlaceLimit: { code: "600888", side: "Buy", price: { Fixed: "1000" }, qty: 100 } }, account_ordinal: "3", stock_ordinal: "3" },
    { owner: 1, intent: { PlaceLimit: { code: "000001", side: "Buy", price: { Fixed: "1000" }, qty: 100 } }, account_ordinal: "3", stock_ordinal: "3" },
  ]) {
    const intents = structuredClone(batch.intents)
    intents[3] = replacement
    assert.throws(
      () => parseSaveSlot({ ...currentSaveFixture(), pending_npc: { ...batch, intents } }),
      /pending_npc\.dependencies/,
    )
  }
})

test("存档仅接受当前结构，不接受任何 schema_version 标记", () => {
  const current = currentSaveFixture()
  const { schema_version: _removed, ...missing } = current
  assert.deepEqual(parseSaveSlot(missing), missing)
  for (const schema_version of [1, 2, 3, 4, null, "3"]) {
    assert.throws(() => parseSaveSlot({ ...missing, schema_version }), /schema_version/)
  }
})

test("auction save identifies orders without accepting the old arrival field", () => {
  const order = { owner: 0, side: "Buy", limit: "100", qty: 100, order_id: 1 }
  const save = { ...currentSaveFixture(), auction_orders: { "600888": [order] } }
  assert.deepEqual(parseSaveSlot(save).auction_orders, save.auction_orders)
  const { order_id: _removed, ...oldOrder } = order
  assert.throws(
    () => parseSaveSlot({ ...save, auction_orders: { "600888": [{ ...oldOrder, arrival_seq: 1 }] } }),
    /auction_orders.*order_id/,
  )
  assert.throws(
    () => parseSaveSlot({ ...save, auction_orders: { "600888": [{ ...order, arrival_seq: 1 }] } }),
    /auction_orders.*arrival_seq/,
  )
})

test("存档运行时校验边界拒绝非法 scalar 和未知字段", () => {
  assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => { runtime.poisoned = "false" })), /runtime_state\.poisoned/)
  assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => { runtime.next_receipt_base = 0 })), /runtime_state\.next_receipt_base/)
  assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => { runtime.unexpected = true })), /runtime_state\.unexpected/)
})

test("存档运行时校验边界拒绝非法策略内部状态", () => {
  assert.throws(
    () => parseSaveSlot(mutateRuntime((runtime) => {
      const states = runtime.strategy_states as Record<string, unknown>
      const account = Object.keys(states)[0]
      if (account === undefined) throw new Error("fixture must contain a strategy")
      states[account] = { Momentum: { style: "Momentum", lookback: 2, trend_threshold: 0.02 } }
    })),
    /runtime_state\.strategy_states/,
  )

  assert.throws(
    () => parseSaveSlot(mutateRuntime((runtime) => {
      const states = runtime.strategy_states as Record<string, unknown>
      const account = Object.keys(states)[0]
      if (account === undefined) throw new Error("fixture must contain a strategy")
      const tagged = states[account] as Record<string, unknown>
      const variant = Object.keys(tagged)[0]
      if (variant === undefined) throw new Error("fixture strategy must contain a variant")
      const payload = tagged[variant] as Record<string, unknown>
      payload.base_observation_probability = "3FF0000000000000"
    })),
    /16 位小写十六进制浮点位串/,
  )

  assert.throws(
    () => parseSaveSlot(mutateRuntime((runtime) => {
      const states = runtime.strategy_states as Record<string, unknown>
      const account = Object.keys(states)[0]
      if (account === undefined) throw new Error("fixture must contain a strategy")
      states[account] = { Momentum: {
        style: "Momentum",
        lookback: Number.MAX_SAFE_INTEGER + 1,
        trend_threshold: "3f847ae147ae147b",
        order_size: 100,
        volume_confirmation: "3ff0000000000000",
        base_observation_probability: "3ff0000000000000",
      } }
    })),
    /lookback.*安全整数/,
  )
})

test("存档运行时校验边界拒绝非有限 exact-float 位串", () => {
  for (const encoded of ["7ff0000000000000", "fff0000000000000", "7ff8000000000000"]) {
    assert.throws(
      () => parseSaveSlot(mutateRuntime((runtime) => {
        const states = runtime.strategy_states as Record<string, unknown>
        const account = Object.keys(states)[0]
        if (account === undefined) throw new Error("fixture must contain a strategy")
        states[account] = { BeliefInstitution: {
          style: "DeepValue",
          margin: encoded,
          order_size: 100,
          position_step_bp: 750,
          base_observation_probability: "3ff0000000000000",
        } }
      })),
      /有限浮点数/,
    )
  }
})

test("存档运行时校验边界拒绝 Rust u32 溢出", () => {
  const overflow = 4_294_967_296
  const runtimeMutators: readonly ((runtime: Record<string, unknown>) => void)[] = [
    ...(["QuoteExpiry", "Auction", "DayEnd"] as const).map((source) => (runtime: Record<string, unknown>) => {
      const receipt = validReceipt()
      const localKey = receipt.local_key as Record<string, unknown>
      localKey.source = { [source]: overflow }
      runtime.retail_projection_seen = [receipt]
    }),
    (runtime) => {
      runtime.strategy_states = { "1": { ZiNoise: {
        retail_style: "Noise",
        arrival_rate: "3ff0000000000000",
        order_size_mean: overflow,
        chase_prob: "3f847ae147ae147b",
        dip_threshold: "3f847ae147ae147b",
        stop_loss_threshold: "3f847ae147ae147b",
        take_profit_threshold: "3f847ae147ae147b",
        volume_confirmation: "3ff0000000000000",
        position_step_bp: 750,
        base_observation_probability: "3ff0000000000000",
      } } }
    },
    (runtime) => {
      runtime.strategy_states = { "1": { Momentum: {
        style: "Momentum",
        lookback: 2,
        trend_threshold: "3f847ae147ae147b",
        order_size: overflow,
        volume_confirmation: "3ff0000000000000",
        base_observation_probability: "3ff0000000000000",
      } } }
    },
    (runtime) => {
      runtime.strategy_states = { "1": { BeliefInstitution: {
        style: "DeepValue",
        margin: "3f847ae147ae147b",
        order_size: overflow,
        position_step_bp: 750,
        base_observation_probability: "3ff0000000000000",
      } } }
    },
  ]

  for (const mutate of runtimeMutators) {
    assert.throws(() => parseSaveSlot(mutateRuntime(mutate)), /超出 u32 范围/)
  }
})

test("存档运行时校验边界拒绝注入派生 envelope 镜像", () => {
  for (const field of ["live", "limit", "remaining_qty", "filled_qty", "filled_value", "nominal"]) {
    assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => {
      runtime.live_envelopes = [{
        ...validEnvelope(),
        [field]: field === "live" ? { cash: "0", shares: 1 } : 1,
      }]
    })), new RegExp(`runtime_state\\.live_envelopes\\[0\\]\\.${field}`))
  }
})

test("存档运行时校验边界在 Map 转换前拒绝非规范策略账户 key", () => {
  assert.throws(
    () => parseSaveSlot(mutateRuntime((runtime) => {
      const states = runtime.strategy_states as Record<string, unknown>
      const account = Object.keys(states)[0]
      if (account === undefined) throw new Error("fixture must contain a strategy")
      states[`0${account}`] = structuredClone(states[account])
    })),
    /runtime_state\.strategy_states\.0\d+/,
  )
})

test("存档运行时校验边界保留非空 live envelope 和回执身份", () => {
  const save = mutateRuntime((runtime) => {
    runtime.next_receipt_base = "1"
    runtime.live_envelopes = [validEnvelope()]
    runtime.retail_projection_seen = [validReceipt()]
  })
  const parsed = parseSaveSlot(save)
  assert.deepEqual(parsed.runtime_state.live_envelopes, [validEnvelope()])
  assert.deepEqual(parsed.runtime_state.retail_projection_seen, [validReceipt()])
})

test("存档运行时校验边界拒绝不安全及未知的 live-envelope 嵌套字段", () => {
  for (const mutate of [
    (envelope: Record<string, unknown>) => { envelope.unexpected = true },
    (envelope: Record<string, unknown>) => {
      (envelope.key as Record<string, unknown>).account = Number.MAX_SAFE_INTEGER + 1
    },
    (envelope: Record<string, unknown>) => {
      (envelope.key as Record<string, unknown>).order = Number.MAX_SAFE_INTEGER + 1
    },
  ]) {
    assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => {
      const envelope = validEnvelope()
      mutate(envelope)
      runtime.live_envelopes = [envelope]
    })), /runtime_state\.live_envelopes\[0\]/)
  }
})

test("存档运行时校验边界拒绝非法回执来源标签及未知嵌套字段", () => {
  assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => {
    const receipt = validReceipt()
    const localKey = receipt.local_key as Record<string, unknown>
    localKey.source = { Unsupported: 0 }
    runtime.retail_projection_seen = [receipt]
  })), /无效收据来源变体/)

  assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => {
    const receipt = validReceipt()
    const localKey = receipt.local_key as Record<string, unknown>
    const transition = localKey.transition as Record<string, unknown>
    transition.unexpected = true
    runtime.retail_projection_seen = [receipt]
  })), /runtime_state\.retail_projection_seen\[0\]\.local_key\.transition\.unexpected/)
})


test("当前存档使用 runtime_state，不含实施代号与版本标记", () => {
  const current = currentSaveFixture()
  assert.equal("schema_version" in current, false)
  assert.ok("runtime_state" in current)
  assert.equal("runtime_v2" in current, false)
  const runtime = current.runtime_state
  assert.deepEqual(parseSaveSlot(current), current)
  for (const schema_version of [1, 2, 3]) {
    assert.throws(() => parseSaveSlot({ ...current, schema_version }), /schema_version/)
  }
  assert.throws(() => parseSaveSlot({ ...current, runtime_v2: runtime }), /runtime_v2/)
  const { runtime_state: _removed, ...missing } = current
  assert.throws(() => parseSaveSlot(missing), /runtime_state/)
})

test("SavedReceiptSource 接受 QuoteExpiry 并明确拒绝旧 P0Expiry 标签", () => {
  const runtime = {
    poisoned: false, next_receipt_base: "1", live_envelopes: [], strategy_states: {},
    retail_projection_seen: [{ ...validReceipt(), local_key: { ...validReceipt().local_key as object, source: { QuoteExpiry: 7 } } }],
  }
  assert.deepEqual(parseSaveRuntime(runtime), runtime)
  const legacy = structuredClone(runtime)
  legacy.retail_projection_seen[0]!.local_key.source = { P0Expiry: 7 } as unknown as { QuoteExpiry: number }
  assert.throws(() => parseSaveRuntime(legacy), /runtime_state.*source/)
})


test("存档拒绝额外版本字段及旧 simulation_policy_id", () => {
  const current = currentSaveFixture()
  assert.throws(() => parseSaveSlot({ ...current, schema_version: 2 }), /schema_version/)
  const setup = current.setup as Record<string, unknown>
  assert.throws(() => parseSaveSlot({ ...current, setup: { ...setup, simulation_policy_id: "a-share-simulation-v2" } }), /simulation_policy_id/)
})
