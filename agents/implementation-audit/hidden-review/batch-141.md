# 独立审读：batch-141（owner 1）

## 基线与来源

- 目标 worktree HEAD：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；日期：2026-10-03。
- 三篇指定来源均从主工作区精确路径连续全文读至 EOF；行数与 SHA-256 和 scan-plan 一致。另读目标 worktree 的 `AGENTS.md`、`docs/principles.md`。
- 历史材料是 OOP 对象归属候选记录，不能视作已实施变更；其中旧 agent 指令只作历史记录，不产生当前行动授权。
- 静态审读，没有运行测试/构建，没有改产品代码或使用 Git 写操作。

## 审读结论

1. **对象归属候选：保留现有对象，未见需要实施的 OOP action。** batch 01 对 `AccountBudget` 窄 API 的提议仍只是未实施候选；相关 A 股边界（cash 分、shares 股、按配置类别校验）没有在此重构提议中改写。batch 02 建议保留 `AdaptivePlanChainCoordinator` 的单 tick 生命周期边界，避免新增投影器或拆散状态；当前架构仍符合这项判断。
2. **batch 02 的独立 defect lead 在基线仍可复现于代码路径，不能因 OOP 结论为 retain 而视为已修复。** `adaptive_plan_chain.rs:305-333` 两个续行入口分别先执行 `ensure_active()`，再用 `outcomes_in_plan_identity_order(steps)?` 预校验；只有之后的 `advance_*_batch` 返回错误才设置 `self.failed = true`。预校验因重复 candidate key、非 pending 或身份错配返回错误时直接早退，coordinator 未标记失败。`outcomes_in_plan_identity_order` 的分支见 `:544-568`；`ensure_active`/`fail` 见 `:879-892`。连续与竞价生产 caller 分别为 `ready_stock_stream.rs:78`、`:114`，拒绝计划续行 caller 为 `:182`。这是现存失败状态一致性缺口，需单独行为修复，不是对象拆分 action。
3. **现有相邻测试不覆盖该早退。** `adaptive_plan_chain_tests.rs:720-746` 测的是进入 batch 后由错误 P4 candidate/sealed identity 触发失败；它不能证明 P3 排序/身份预校验错误也锁存失败。针对两个入口的预校验错误及之后 coordinator 停用仍是测试缺口。未运行测试。
4. **契约及编号边界。** ADR-0017 accepted，规定阶段 shadow、失败不提交与 P9 单点提交；ADR-0018 整体仍 proposed，但 §7 明确计划后续命令依赖 typed 执行结果，要求及时续行；ADR-0019 accepted 并取消任意请求/挂单数量配额。检查 `open-questions.md` 与 G01–G68 总账，三篇材料没有直接映射到某个 G 项或未决 Q；不得因同处 engine/session 强行归类。未发现后续 ADR 明确取代上述对象归属或该缺陷 lead。此项属于错误路径处理，不改变交易优先级、A 股规则、资金/股份单位或已决定的容量政策。

## 处理建议

- OOP 结论按 retain 记录；不要把“候选设计未实施”报告成代码改动。
- defect lead 按独立 bug 修复事项保留。修复需令两个续行入口的预校验错误也进入 coordinator failed 状态，并用连续/竞价行为测试证明错误返回后不能继续取 ready batch；不得改变执行顺序、交易事实或提交边界。
- 复核结论：**历史 OOP 审计结论仍成立；batch 02 defect lead 再确认未解决。**

## 来源 EOF / 指纹

- `areas/engine-pipeline.md`：108 行，已读至 EOF。
- `modules/engine-pipeline-01.md`：77 行，已读至 EOF。
- `modules/engine-pipeline-02.md`：50 行，已读至 EOF。
