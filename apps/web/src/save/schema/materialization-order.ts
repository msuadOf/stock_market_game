// 对照 parseStrictSaveEnvelope 的最终返回投影；不能用 schema 阶段代替物化访问顺序。
export const SAVE_MATERIALIZATION_KEYS = ["desk_mirrored_resting", "runtime_state", "setup", "seed", "snapshot", "resting_orders", "book_next_sequences", "filled_orders", "price_history", "market_minute_closes", "rng_state", "npc_attention", "npc_order_lifecycles", "pending_player", "ingress_receipt_cursors", "next_order_id", "retail_experience", "civil_clock", "company_state", "report_correction_operations", "public_library", "disclosures", "plans", "urgency_policy", "information_states", "belief_books", "watchlists", "price_memories", "history_reads", "market_memberships", "market_minute_bars", "retained_market_days", "personal_trade_confirmations", "receipt_watermark"] as const

export function saveMaterializationOrder(field: string): number {
  const index = SAVE_MATERIALIZATION_KEYS.indexOf(field as typeof SAVE_MATERIALIZATION_KEYS[number])
  if (index < 0) throw new Error(`未知的存档物化字段：${field}`)
  return index
}
