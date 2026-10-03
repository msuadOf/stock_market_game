# 隐藏扫描批次 043

## 范围与读取证据

- 按 `scan-plan.json` 扫描 owner=3 的 batch 43。源根为 `/data1/baiyifan/workplace/stock_market_game`，当前 caller 为 `.worktree/implementation-reaudit`，任务基线为 `43b1aa5`。
- 三份来源均从首行连续分段读至 EOF；实读行数及 SHA-256 与计划相同，每项 `aliases=1`。各文件完整章节状态见配套 `batch-043.json`。
- 已读当前 caller 的 `AGENTS.md`（123 行）、`docs/principles.md`（92 行）。相关现行决策核对 ADR-0008、0009、0014、0016、0017、0021、0026；其中 ADR-0017 与 ADR-0018 的交易并行验收修订覆盖旧的逐字节跨 worker 要求，ADR-0021/0022 的现行主动报价与费用配置边界仍有效。另核对 `docs/trading-rules.md` 过户费/T+1/集合竞价简化说明及 2026-10-02 当前 G/Q 总账。来源中的 reader/worker instructions、候选设计和“本轮未运行”陈述只作为历史材料，不是当前授权或实现验收。

| 来源 | 全文状态 | 主要章节与审查范围 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/engine-tests-01.md` | 完整至 EOF；292 行；SHA 匹配 | 文件核销：Account、Accounting、Analysis Profiles、Attention Discovery、Auction；模块关系与验证边界。所有章节将对象归类为集成测试/fixture/support，不建议将测试结构搬入产品。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-tests-03.md` | 完整至 EOF；343 行；SHA 匹配 | Company decision/session、event、opening、operations、public financials/query、scale 与 scenarios；批次边界明确标出 ignored 长验收及未分配 fixture。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-tests-04.md` | 完整至 EOF；157 行；SHA 匹配 | Compute、Config、Consolidation、diagnostics、event mapping、experience feedback、replay、fundamental belief 测试；跨文件关系及未运行验证边界。 |

## 当前实现与承诺核对

- **没有已批准 OOP 候选遗漏，也没有旧核销错误。** 三份源文档均明确是测试、入口或 fixture 支撑，不创建测试专用领域对象；并无把未实施建议声称为已批准实现的依据。当前总账已把“测试源码存在”与生产路径闭环分开，未以这些源记录改写 G 状态。
- **G07/G08/Q02 继续开放。** 本批的 `analysis_profiles`、`experience_feedback`、`fundamental_beliefs` 用例主要固定纯档案、值对象或函数契约；当前总账要求真实 `GameSession` 消费链和实际历史读取。`PersonalPriceMemory::record_public_history_read` 定义于 `packages/engine/src/experience/price_memory.rs:164-180`，当前扫描到的生产调用缺失；`packages/engine/src/session/decision_chain.rs:299-310` 仍把全部 `belief.entry_stocks()` 加进 root 候选，不以关注淡出资格过滤，关联已有 G43。没有把纯测试覆盖误作这些生产链缺口的核销。
- **G17 仍开放。** `packages/engine/tests/compute.rs:51-117` 验证批量 CPU 接缝；`packages/engine/src/compute.rs:25-42,59-90` 定义并行 `ComputeBackend`/CPU 实现。总账 G17 指出 session/三宿主并未消费批量入口，测试与类型存在不代表生产批量接线，故不核销。
- **公司域覆盖不等于闭环承诺完成。** `company_operations`、`company_opening`、`company_public_financials`、`company_query_contract`、`company_scenarios` 的用例覆盖领域 API、公开 DTO 和代表性场景；当前 G28、G35、G36、G41、G58、G59 各自仍有日终报告交付、经营事件调度/持久化、四行业会话装配或合同边界等生产链缺口。caller 入口包括 `packages/engine/src/session.rs:2073-2168` 的日终经营/披露，`packages/engine/src/company/operations/day.rs:21-224` 与 `operations/dispatch.rs:14-96` 的推进/派发；已有 handler 或测试 fixture 不证明 session 消费、调度、公开均已闭合。G28 对应合并算法/金样已存在但完整公共交付仍未完成，符合总账的既有范围。
- **合并及公司测试与 G28/G35-G38 的边界一致。** `consolidation/*` 金样能证明相应合并函数的局部算法契约；不能证明固定集团进入日终公开链、四行业配置进入会话或经营调度，以及计划预算请求分类已满足。总账并未核销这些项目；本批没有反证其仍开放。
- **G39 仍开放，且回放源有明确反证边界。** `engine-tests-04.md` 的 `extraction_replay` 只冻结相同 setup/seed/单 worker 的字节结果，并明确自由并行跨实体不要求字节一致；ADR-0017/0018 后续修订确认不把全局事件顺序或全局编号当跨实体优先级。它不验证 G39 所需的同一实际受理轨迹重放及 after/sensitivity 矩阵，故不能凭这类单 worker 测试核销 G39。
- **未将相邻但不同问题合并。** `diagnostics.rs` 中大 seed JSON 的 baseline 用例不覆盖 G57 所指 causal DTO `seed/qty` 输出；`account.rs` 成本舍入用例不覆盖 G49 的 Web 成本/PnL 跨层一致性；`company_scale.rs` 报告的长规模场景标为 ignored，不是运行验收。上述边界与总账相符，不从源码/测试名称推断通过。

## A 股语义及结论

- 本批只有测试支撑记录，没有实施交易规则更改。`docs/trading-rules.md` 记载沪深 A 股金额分、股份股、T+1、交易阶段，以及买卖双方过户费 0.01‰ 的当前游戏模型。`packages/engine/src/config.rs:11-13,234-238` 固定费率，`packages/engine/src/account.rs:287-319,398-430` 在买卖结算计算该费；单独的 `GameConfig::transfer_fee` 算术测试本身不等同撮合/实际结算验收，但这里有账户结算路径可对照。来源没有提供新的 A 股法源主张，本轮未重新联网核验交易所或中国结算规则。
- **新候选：无。** 可定位的相邻缺口均已在现行 G/Q 总账中有独立条目或其范围外有明确限定；不把 ignored/未运行、或测试没有覆盖某生产 caller 的事实本身另建产品缺陷。
- 仅新增本审查记录；未改产品代码，未运行测试、构建或回归，未做 Git 操作。逐源元数据、章节清单、caller 路径、G 关联和反证见 `batch-043.json`。
