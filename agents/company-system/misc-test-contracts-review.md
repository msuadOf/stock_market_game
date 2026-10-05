# 消费者测试契约独立复核

## 范围

复核 `packages/engine/tests/support/event_mapping.rs`、`packages/engine/tests/session.rs`、`packages/engine/tests/auction.rs`、`packages/engine/tests/company_decision_session/main.rs` 相对 `HEAD` 的完整 diff，并查看 `event_mapping_contract.rs`、`event_key.rs` 与工程原则/架构文档。未运行 Cargo 命令；作者说明由 root 集中验证。

## 结论

- `PublicTrade` 映射到现有 Trade phase / Stock entity / Sealed source，`PrivateEventOmitted` 映射到 Trade phase / Session entity / Sealed source；这是稳定事件身份键映射，不读取或改写事件 `seq`。映射没有过滤逻辑，因此与指定语义一致。
- `session.rs` 的 `u64` turnover fixture 到 `i128` 比较使用 `try_from(...).expect(...)`，表达 fixture 必须可表示的前提；越界会显式失败。原拒绝恢复用例仍使用 `u64::MAX` 的原数值，仅以 `.into()` 适配新类型，没有改成 `u128::MAX`。
- session/auction/company decision 的 `company_system` fixture 随股票代码集合更新，避免股票代码变更、裁剪、扩展后夹具与市场股票失配。`WithinKindDistribution` 是 `chain_setup` 当前显式使用的类型，导入必要。变更仍属 SessionSetup 字段迁移和测试夹具适配，没有变更沪深、板块或交易规则语义。
- 本复核范围内没有修改 gold 值或 pinned hash。完整工作区另有 `gold.rs` diff 增加 `is_trading: true` 初始化字段；该差异不在本任务指定文件范围，也没有修改其期望值或哈希。

## Findings

- 增量复核已关闭上项：`events()` 加入 `seq: 12` 的 `PublicTrade` 与 `seq: 13` 的 `PrivateEventOmitted`，契约测试现逐项经过 ADR mapping、`EventStableKey::for_event` 并断言实体、phase、source 与 local index；变体数由 11 更新为 13，另显式确认这两个 seq 存在。session 消费者的 `seq_of` 与 `events_summary` 亦加入两种变体，未将其过滤。

## 复核边界

未运行测试或编译；author/root 报告 engine consumer check 已成功，属于其验证结果，本复核未独立执行。交易领域结论限于本次测试夹具/事件键映射改动，没有新增交易制度规则。
