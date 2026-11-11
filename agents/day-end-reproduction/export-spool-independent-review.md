# DayEndExportSpool 独立复核

- 日期：2026-11-09。
- 实施 agent：`candidate_fix`。
- 未参与实施的复核 agent：`candidate_fix/spool_independent_review`。
- 范围：`packages/native-store/src/lib.rs` 中新增错误与 export、`src/export_spool.rs`、`tests/export_spool.rs`，以及候选探针和生命周期工作记录；不包含其他 agent 正在实施的 actor/routes/Web 接线。

## 首轮有效发现与修复

1. 容量检查原先发生在 `serde_json::to_string` 完整分配之后，队列已满或小额度仍可能分配数百 MB。已改为零 payload 存储的 `PayloadCounter` 先计数，额度检查通过后才分配精确容量。
2. 原先 `publish` 可接受超过 `decode_day_end` 默认 512 MiB 解码限制的候选，导致保留成功后永远无法读取。已对发布计数使用同一 `SaveDecodeLimits::default().max_total_bytes`，超过上限早停并返回 `ExportCandidateTooLarge`。

## 第二轮结论

上述两项均已修复，未发现本模块新的必须修复问题。

- 大 A 语义：未改变交易制度、资金股份、T+1、公司经营或自然日边界，仅接收已验证的完整成功 `DayEndCandidate`。真实周末 Closed 日 fixture 符合 ADR-0025 的成功自然日日结语义，无需新增交易所规则解释。
- 必要性和范围：显式订阅保留精确候选、多客户端各自 ACK、无 owner 后删除，能够处理自动 SQLite 提前消费候选的跨消费者问题。独立临时 SQLite 不改变主库 schema，无订阅不生成导出历史，范围合理。
- 边界：精确 key、重复读取、独立 owner、错误日期/未知 token 拒绝、晚订阅无追溯授权、容量失败保持已有数据、清理、reset 均有行为测试；新增统计断言适合 actor 的慢订阅接线。
- 资源说明：SQLite `max_page_count` 在 payload budget 外增加 4 MiB 页预算，是有界数据库页保障；不能宣称等于含 journal、元数据及页对齐开销的精确磁盘总额度。cache 也是目标值，不能宣称整个导出仅占 2 MiB。

复核 agent 独立并行运行 4 项 integration + 2 项 unit，各命令均有 10000ms 外部 supervisor，使用 4 / 2 个测试线程，全绿；耗时分别为 0.62 秒和 0.00 秒。

完整 actor/routes/Web 生命周期仍须完成跨层 diff 独立复核；本记录不代表端到端文件导出已验收。
