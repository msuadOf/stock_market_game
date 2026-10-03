# 独立审读：批次 020（owner 5）

## 基线与范围

- 目标 worktree：`.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；初始 `git status --short` 为空。
- 连续读至 EOF 的材料：`agents/oop-refactor-audit/completeness-2026-10-03/root/summary-before.md`（105 行，SHA-256 `45d650170c55b30d7ddeb2aa87437f4d7689bcaa0eff690cea077cc28e323258`）、`scope/report.md`（67 行，SHA-256 `725511605d881658a5981889a3c88d7fdd590635289bc308fe37e49fb2072d1d`）、`session/independent-review-incomplete.md`（25 行，SHA-256 `f0cc946afe1751930e1d700fc4e7d3f5e083873724528851e973dfe8fa6d316f`）。三篇都已逐行读至末行；未将搜索或 hash 核对当作全文审读。
- 对照现行 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、ADR-0023–0028、实现审计总账及实际生产 owner/caller。没有运行产品测试、构建或生成器；没有修改产品代码。

## 审读结论

1. **版本标记关键且基本诚实。** `summary-before.md:5` 的 1,186 文件、22 动作组与“均未实施”是整合前入口版本的陈述；它在 `:15, :46` 明确指向最终复核入口并提醒依赖最终 review/closure，不应独立当作最终统计。最终入口 `final-review.md:40-44` 将最终索引更新为 1,198 唯一路径，且列明新增 workflow 容器/入口；`scope/report.md:20-32` 解释了从 1,186 冻结源码到 1,198 处理路径的分类。该增量是 3 个 workflow 执行容器、2 个 package scripts 入口和 7 个薄 workflow 配置，不是 12 个新增实现模块。没有发现把这两个统计冒称等价的现行正式主张。
2. **scope 报告边界透明。** `scope/report.md:13-18, 34-51, 61-67` 明确将目录列举、哈希继承、本轮 101 文件全文阅读分开；同时说明动态拼接路径不能被静态检索证明绝对不存在，也不声称产品验收。3 个有 `run` 的 workflow 转交 hosts 组，另外 7 个配置和两个 scripts 入口登记为薄入口；最终 `final-review.md:42` 确认 file-index 有 1,198 路径。该范围结论不替代 workflow 算法审查或 G/Q 的产品缺口判断。
3. **session 初审不能被误读为通过。** `session/independent-review-incomplete.md:3-7, 19-25` 明确写明未读 parts/08–15、17–30且结论未通过，并记录了“六组/七组”及 N01 caller 覆盖问题。最终 `session/independent-review.md:3, 11-17` 明确该初审只作历史、不计通过；经另一 reviewer 对最终三份正式材料独立复核，结论是静态调查门禁通过，并核对 113 个路径/hash 与修订后的七组状态。`final-review.md:19, 34` 也明确追踪此修订链。故初审的缺口是真实历史发现，但已由新复核与修订处理；不得仅因 incomplete 文件仍在仓库就判当前总审未通过。
4. **源码 caller/owner 与复核结论相符。** 在目标 HEAD，`packages/engine/src/session/execution.rs:14-28, 35-40, 50-103, 136-143` 可见 `ParentOrderPlan` 持有母单字段、局部构造及 child 转换入口；最终独立 review `session/independent-review.md:13-15` 进一步核对 `records.rs`、`orders.rs`、`auction_day_end.rs` 的连续/竞价调用差异、临时批量安装及双 `Option` 语义。`packages/engine/src/session/protocol/civil/session.rs:9-28, 59-80` 可见私有 `ProtocolState` 持有七项 rollback 状态并由 checkpoint/rollback 消费；与最终 review 所述七组一致。现代码存在这些 owner/方法不等于 OOP 调查提出的全部 22 动作均已实施；这些 session-N01/N02 是完整性补审的另外两个候选，不能混算进 `summary-before` 的 22 项清单。
5. **G/Q 编号映射：无可确认的直接关联。** `implementation-audit-2026-10-02.md:25-28` 定义 G01–G68 现行核销范围（G27 已核销，其余有未完成部分），而 `docs/open-questions.md:94-138` 保留 Q11/Q12 的历史描述并在后续段落更新现行决定；Q12 已解决，Q11 当前机构补充已由 ADR-0026 定义。三篇指定审读材料没有将其 OOP 候选映射到任一 G 或未决 Q。不得根据同用 engine/session 关键词，将 N01/N02 或范围核对硬配给 G 编号；也不能以 OOP 调查代替 67 项实现缺口复核。ADR-0023–0028 的最新决定（合成历史/撮合边界、现金池、日终存档、个人机构经验、部署/发布）与这些范围/所有权候选无冲突；本批未改变 A 股规则，无需生成新的交易制度结论。

## 有效发现及处置

- 对三篇指定材料，未发现尚未处理的内容错误。历史 incomplete review 的有效缺陷已在最终材料修订并由新 reviewer 复核；`summary-before` 与 scope 统计差异被显式版本和类别解释。
- 限制：`implementation-audit-2026-10-02.md` 记录的是产品审计基线 `08e4fc7`（该文 `:7`），本次目标源码基线为 `43b1aa5`；本记录只用 G/Q 文档做编号与适用范围对照，不将其 08e4fc7 的缺口状态冒称为对 43b1aa5 的重新完整源码审计。
- 审读结论：**本批文档审读通过；不构成 1,186/1,198 文件 OOP 复审、G01–G68 全量再验或产品验收。**
