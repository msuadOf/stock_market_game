# 公开 payload enum 布局红测试

- worktree：`.worktree/clippy-public-payload-tests-2026-10-30`
- 基线：`50d49370`
- 测试过滤器：`outcome_payload_preserves_value_semantics_and_stays_within_inline_budget`、`public_report_availability_preserves_complete_report_wire_and_inline_budget`
- 修改范围：仅追加 `disclosure_roe/tests.rs` 与 `information/source_tests.rs` 测试源码；未改 enum 定义、运行时实现或 wire schema。
- 预期红因：当前 `DisclosureRoeOutcome::Available(DisclosureRoeResult)` 与 `PublicReportAvailability::Available { report: PublicReportSummary }` 按值内嵌大型 payload；`size_of` / `size_of_val` 的 128-byte 上限预期失败。布局收窄为 `Box` 后，断言应通过，同时其他断言固定 ROE 完整精确值、Available/Unavailable JSON tag、真实报表完整 JSON 与 ts-rs 联合类型声明。
- 验证：按任务要求未运行 Cargo、构建或测试，因此红灯原因尚未实测。仅运行 `rustfmt` 与 `git diff --check`。
