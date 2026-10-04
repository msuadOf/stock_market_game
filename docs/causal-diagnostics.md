# Offline Source Diagnostics

Run `cargo run -p engine --features simulation-diagnostics --example causal_diagnostics`.
The artifact contains a report plus immutable, sequence-addressed source facts.
All collection fields and hooks disappear when the feature is disabled. No RNG,
public event, host contract, snapshot or save representation is modified.

`price_volume_baseline` now emits `{price_volume, causal_runs}`. Each causal run
starts a real session from the input setup; it does not restore an old ledger.
The historical task-1 baseline fixture/report is unchanged.

量价与 causal 同批采集时，`price_volume_baseline` 示例默认对每个 seed 仅推进一个真实
`GameSession`。量价报告读取该次运行的 `Trade` receipt；causal 报告读取同一 session 的执行事实，
共享 `run_id`，并输出笔数、成交股数与成交额对账摘要。示例的 `--independent` 模式仍分别启动两个
session，来源标记为 `independent_sessions`，两个报告使用不同 `run_id`。seed 相同不保证自由调度
轨迹相同，不能跨独立运行声称成交逐笔相同。

## Definitions

- Submitted quantity is the quantity entering an allocated order-ID lifecycle,
  including settlement-aborted orders, not OrderAccepted remaining quantity.
  Validation-rejected intents never enter that lifecycle.
- Bilateral filled quantity equals twice executed market quantity. Per-order
  fill quantities and values reconcile against execution IDs, price and shares.
  Submitted equals filled + canceled + open + aborted. Filled/submitted uses
  share quantities, not order counts.
- Order market minutes use the existing 240 continuous-progress game buckets.
  Auction windows do not advance that clock. Civil seconds map opening windows
  to 09:15-09:30, continuous progress to the two sessions separated by lunch,
  and the optional closing window to 14:57-15:00. This deliberately preserves
  ADR-0011's game-bucket simplification, not 240 real continuous minutes.
- Acquisition latency uses the actual recorded acquired instant minus actual
  publication instant.
  历史离线诊断复核曾记录决策时钟缺少午休的来源问题；诊断不得为掩盖时钟缺陷篡改来源时刻。
- Direction persistence is the same-side fraction of adjacent actual continuous
  aggressor fills within each stock, pooled over eligible pairs. Auction trades
  have no aggressor and are excluded, explicitly.
- Impact is signed midpoint basis-point change from the pre-commit book to the
  first later valid two-sided quote. Quote sequence identifies that observation;
  intervening transitions may contribute. This is not a causal estimate.
- Depth is total displayed bid plus ask shares. A loss is any decrease between
  recorded quote snapshots, including cancellation. Recovery is the first
  market-minute observation reaching at least 50% of pre-loss depth with valid
  bilateral quotes. Recovery can legitimately be zero minutes. Unrecovered
  observations and absent impacts are null with reasons, never filled with zero.
- Decision attribution means the current execution observation at submission,
  not the original creation decision of a persistent plan. Ordinary/player
  orders have absent plan/decision references; company maps from the issuer.
- Restore emits an explicit observation restart. Reporting returns
  `RestoredObservation` because original timings cannot be recovered from a
  save; it neither invents submissions nor persists private diagnostic data.

The full offline fact vector is intentionally retained for auditability and is
not suitable for unlimited product-session collection. Large baseline consumers
must budget its memory and artifact size; product builds leave it absent.

## JSON 大整数编码

causal report 与 source facts 中的 `u64` seed、sequence、market minute、累计数量、
depth 和可选 identity / lifetime 均输出十进制字符串，避免 JavaScript 安全整数范围导致
精度丢失；缺失 Option 仍输出 `null`，不以 `"0"` 冒充缺失。`information_delays`
每项的第一个元素是字符串 identity，第二个元素仍是原 `i64` 时间差。
本约定仅调整诊断 JSON 投影，不改变 Rust 内部计算、正式交易协议或存档契约。
