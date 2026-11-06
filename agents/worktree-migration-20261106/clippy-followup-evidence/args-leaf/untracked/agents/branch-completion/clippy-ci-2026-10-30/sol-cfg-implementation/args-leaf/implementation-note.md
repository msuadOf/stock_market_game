# Clippy 参数上下文叶修复记录

日期：2026-10-30
基线：`50d493701aa0fbb7d62563855a84dc07c89c29b0`
分支：`codex/clippy-cfg-args-leaf-20261030`

## 基线失败记录

已有 Clippy JSON `agents/performance-delivery/formal-runtime-sync-2026-10-30/clippy-50d493-diagnostics.json` 记录本批四项：

- `decision_chain/lifecycle.rs:12`：`open_position_action` 为 16/7 参数。
- `decision_chain/retail_analysis.rs:503`：`retail_candidate_signals_on` 为 8/7 参数。
- `decision_chain/roots.rs:65`：`RootReadContext::from_parts` 为 8/7 参数。
- `session/notices.rs:125`：`deliver_information_for_npc` 为 11/7 参数。

同一 JSON 对 `retail_analysis.rs` 的 `retail_candidate_signals_core` 还记录了未使用的 `plans` 参数。基线源码中的 `RootReadContext::from_parts` 当前为 8 个参数。

按父任务限制，本工作树未运行 Cargo 或写入 `target`，所以没有重跑 Clippy，也未执行行为测试。已有测试与源代码检查可定位，但“基线行为测试失败”尚无执行证据；不得将其表述为已验证的 Red。

## 实施范围

本次改动只触及父任务授权的四个诊断源文件与必要调用点：

- `packages/engine/src/session/decision_chain/lifecycle.rs`
- `packages/engine/src/session/decision_chain/retail_analysis.rs`
- `packages/engine/src/session/decision_chain/roots.rs`
- `packages/engine/src/session/notices.rs`
- `packages/engine/src/runtime/group.rs`
- `packages/engine/src/runtime/group/admin_transaction/information.rs`
- `packages/engine/src/session/decision_chain/company_source_tests.rs`
- `packages/engine/src/session/information_anchor/export_tests.rs`
- `packages/engine/src/session.rs`（仅追加两个 crate 内转发类型）

`open_position_action` 将候选评估事实和开仓策略政策分别收进 `OpenPositionAssessment` 与 `OpenPositionPolicy`。字段保留原有输入及次序，函数内逻辑未变。

零售候选核使用 `RetailCandidateInput` 归组证券、个人 belief、观察和市场基准；移除从未读取的 `plans` 参数。无额外业务行为变化。

`RootReadContext::from_parts` 被移除，组线程与测试在同一 `RootReadContext` 结构上逐字段构造。该结构字段改为 `pub(crate)`，因此需要独立复核这一可见性扩大是否可接受；未创建等价 Parts 类型。

`deliver_information_for_npc` 改为接收 `InformationDeliveryContext`（账户、时点、证券集合、逐股 anchor 与公开来源）和 `InformationDeliveryState`（attention、information、belief）。四处调用按原闭包及引用来源逐项搬入；函数体从上下文解构后继续原有顺序执行。

## 静态核对与未完成验证

- `rg` 确认四个函数的所有已知调用点均已更新，`RootReadContext::from_parts` 调用已清零。
- `git diff --check` 通过。
- 未运行 `cargo test`、`cargo clippy` 或 `cargo fmt`；Rustfmt 全文件可能产生与本修复无关的格式差异，故保持最小调用点格式并等待集成窗。
- 尚需在父任务指定的唯一 Cargo 集成排窗内执行对应现有测试及 Clippy；需先由未参与实现者审查完整 diff，特别是 RootReadContext 字段可见性与调用/字段求值顺序。
