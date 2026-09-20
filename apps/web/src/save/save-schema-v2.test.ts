import assert from "node:assert/strict"
import test from "node:test"
import { parseSaveJson, parseSaveSlot } from "./save-schema.ts"
import { currentSaveFixture } from "./save-v2-test-fixture.ts"

function mutateRuntime(mutator: (runtime: Record<string, unknown>) => void): unknown {
  const save = structuredClone(currentSaveFixture())
  const runtime = save.runtime_v2
  assert.ok(runtime !== null && typeof runtime === "object" && !Array.isArray(runtime))
  mutator(runtime as Record<string, unknown>)
  return save
}

function validEnvelope(): Record<string, unknown> {
  return {
    key: { account: 0, stock: "600888", order: 1, side: "Buy" },
    live: { cash: 100, shares: 0 },
    audit: {
      limit: 100,
      remaining_qty: 1,
      filled_qty: 0,
      filled_value: 0,
      nominal: { commission: 0, stamp_tax: 0, transfer_fee: 0 },
      charged: { commission: 0, stamp_tax: 0, transfer_fee: 0 },
    },
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

test("schema v2 boundary preserves mandatory runtime authority without legacy profiles", () => {
  const save = currentSaveFixture()
  assert.equal("strategy_profiles" in save, false)
  assert.deepEqual(parseSaveSlot(save), save)
  assert.deepEqual(parseSaveJson(JSON.stringify(save)), save)
})

test("schema v2 boundary rejects legacy, missing, and future schema identities", () => {
  const current = currentSaveFixture()
  const { schema_version: _removed, ...missing } = current
  assert.throws(() => parseSaveSlot(missing), /schema_version/)
  assert.throws(() => parseSaveSlot({ ...current, schema_version: 1 }), /legacy|schema_version/)
  assert.throws(() => parseSaveSlot({ ...current, schema_version: 3 }), /newer|schema_version/)
})

test("schema v2 boundary rejects malformed runtime scalars and unknown fields", () => {
  assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => { runtime.poisoned = "false" })), /runtime_v2\.poisoned/)
  assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => { runtime.next_receipt_base = 0 })), /runtime_v2\.next_receipt_base/)
  assert.throws(() => parseSaveSlot(mutateRuntime((runtime) => { runtime.unexpected = true })), /runtime_v2\.unexpected/)
})

test("schema v2 boundary rejects malformed strategy state internals", () => {
  assert.throws(
    () => parseSaveSlot(mutateRuntime((runtime) => {
      const states = runtime.strategy_states as Record<string, unknown>
      const account = Object.keys(states)[0]
      if (account === undefined) throw new Error("fixture must contain a strategy")
      states[account] = { Momentum: { style: "Momentum", lookback: 2, trend_threshold: 0.02 } }
    })),
    /runtime_v2\.strategy_states/,
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
        max_stock_fraction: "3ff0000000000000",
        base_observation_probability: "3ff0000000000000",
      } }
    })),
    /lookback.*安全整数/,
  )
})

test("schema v2 boundary rejects non-finite exact-float bit patterns", () => {
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
          max_stock_fraction: "3ff0000000000000",
          base_observation_probability: "3ff0000000000000",
        } }
      })),
      /有限浮点数/,
    )
  }
})

test("schema v2 boundary rejects every Rust u32 overflow", () => {
  const overflow = 4_294_967_296
  const runtimeMutators: readonly ((runtime: Record<string, unknown>) => void)[] = [
    (runtime) => {
      const envelope = validEnvelope()
      ;(envelope.live as Record<string, unknown>).shares = overflow
      runtime.live_envelopes = [envelope]
    },
    (runtime) => {
      const envelope = validEnvelope()
      ;(envelope.audit as Record<string, unknown>).remaining_qty = overflow
      runtime.live_envelopes = [envelope]
    },
    (runtime) => {
      const envelope = validEnvelope()
      ;(envelope.audit as Record<string, unknown>).filled_qty = overflow
      runtime.live_envelopes = [envelope]
    },
    ...(["P0Expiry", "Auction", "DayEnd"] as const).map((source) => (runtime: Record<string, unknown>) => {
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
        tick_cents: 1,
        dip_threshold: "3f847ae147ae147b",
        stop_loss_threshold: "3f847ae147ae147b",
        take_profit_threshold: "3f847ae147ae147b",
        volume_confirmation: "3ff0000000000000",
        max_stock_fraction: "3ff0000000000000",
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
        max_stock_fraction: "3ff0000000000000",
        base_observation_probability: "3ff0000000000000",
      } } }
    },
    (runtime) => {
      runtime.strategy_states = { "1": { InstitutionMomentum: {
        style: "ActiveTrader",
        inner: {
          style: "Momentum",
          lookback: 2,
          trend_threshold: "3f847ae147ae147b",
          order_size: overflow,
          volume_confirmation: "3ff0000000000000",
          max_stock_fraction: "3ff0000000000000",
          base_observation_probability: "3ff0000000000000",
        },
      } } }
    },
    (runtime) => {
      runtime.strategy_states = { "1": { BeliefInstitution: {
        style: "DeepValue",
        margin: "3f847ae147ae147b",
        order_size: overflow,
        max_stock_fraction: "3ff0000000000000",
        base_observation_probability: "3ff0000000000000",
      } } }
    },
  ]

  for (const mutate of runtimeMutators) {
    assert.throws(() => parseSaveSlot(mutateRuntime(mutate)), /超出 u32 范围/)
  }
})

test("schema v2 boundary rejects noncanonical strategy account keys before Map conversion", () => {
  assert.throws(
    () => parseSaveSlot(mutateRuntime((runtime) => {
      const states = runtime.strategy_states as Record<string, unknown>
      const account = Object.keys(states)[0]
      if (account === undefined) throw new Error("fixture must contain a strategy")
      states[`0${account}`] = structuredClone(states[account])
    })),
    /runtime_v2\.strategy_states\.0\d+/,
  )
})

test("schema v2 boundary preserves nonempty live envelopes and receipt identities", () => {
  const save = mutateRuntime((runtime) => {
    runtime.next_receipt_base = "1"
    runtime.live_envelopes = [validEnvelope()]
    runtime.retail_projection_seen = [validReceipt()]
  })
  const parsed = parseSaveSlot(save)
  assert.deepEqual(parsed.runtime_v2.live_envelopes, [validEnvelope()])
  assert.deepEqual(parsed.runtime_v2.retail_projection_seen, [validReceipt()])
})

test("schema v2 boundary rejects unsafe or unknown nested live-envelope fields", () => {
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
    })), /runtime_v2\.live_envelopes\[0\]/)
  }
})

test("schema v2 boundary rejects malformed receipt source tags and unknown nested fields", () => {
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
  })), /runtime_v2\.retail_projection_seen\[0\]\.local_key\.transition\.unexpected/)
})
