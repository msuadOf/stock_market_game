# Q08 分时均价实施记录

## 本批边界

- 移动分时均价按当前交易日真实成交额（分）除以真实成交股数（股），即 `Σ(price × qty) / Σqty`。分子来自引擎 `DailyTradeStats.turnover_cents`，分母来自同一活动日 K 的 `volume`；两项必须取自同一个 `NormalizedTickFrame.activeDailyCandles[code]`。
- `MarketChartProjection` 将上述十进制成交额字符串和累计股数附在分时点上。均价线使用各点的权威日累计分子/分母；窗口裁剪不从可见价格点重新求均值。baseline/重连立即显示 snapshot 中活动日 K 的累计均价；缺少逐时点历史时只显示现有可证点，不重建不存在的分钟线。
- 每交易日行情重建清空前日点；日累计统计由当前 snapshot/frame 接续。日内均价不从最近逐笔缓存累计，不用竞价指示价、昨收或分钟末价补造。
- `packages/engine/src/intraday_average.rs` 提供纯 Rust `calculate_intraday_average`：读取现有 `DailyTradeStats` 和 `volume_shares`，保留精确成交额分子、股数分母，不提前舍入；缺统计或一致零成交返回 `None`，成交额/股数/笔数不一致返回显式错误。
- `UX-CONTRACT.md` 登记了口径、单位、缺失状态及现有数据源选择/能力接线边界。本批没有新增逐笔存档、HostUpdate/API 字段、三宿主 capability 或用户偏好设置，也没有完成必需交割单。

## TDD 与验证

- Web 先改测试运行红灯：旧投影把 `10` 与 `8` 算术平均为 `9`，空成交时使用昨收/竞价价显示；新测试分别命中真实累计成交额/股数、零成交、缺失统计和可见窗口截断。
- 实现后定向 Web 测试：`mobile-intraday-projection.test.ts`、`market-chart-projection.test.ts`、`mobile-component-render.test.ts`，29/29 通过；由 `run-with-deadline.mjs 10000` 外部进程树监督，各 Node case timeout 10000ms。
- Rust 新增三个单测：`keeps_exact_turnover_and_share_ratio_without_rounding`、`missing_statistics_and_consistent_zero_trades_are_unavailable`、`inconsistent_statistics_fail_explicitly`。root 统一构建日志 `.engine-build-2.jsonl` 记录成功编译；三个 exact case 分别 0.00s，通过独立 10000ms 外部期限并行执行；两次 binary 验证均通过。没有运行完整 Rust 回归。

## 后续待办

- 玩家指标 source preference 默认前端、可选 Rust；跨 WASM Worker、Remote Server 与 Tauri 共用同一语义和设置契约。
- `HostCapabilities` 按指标明确 Rust 支持情况，选择 Rust 而 capability=false 时显示“不支持”，不退回前端；Rust 同口径结果需接入三宿主按需纯计算入口。
- 必需交易交割单的权威事实、跨宿主查询及 UI 消费由对应任务单独完成。当前只使用现成活动日成交额和股数，不把 Q08 其余 checklist 记为完成。
