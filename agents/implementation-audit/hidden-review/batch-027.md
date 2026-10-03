# 批次 27 隐藏来源复核

## 范围与方法

- 来源基线：产品快照 `43b1aa5`，从 `.worktree/implementation-reaudit` 核对；来源根为主仓 `/data1/baiyifan/workplace/stock_market_game`。
- 本批只含 3 份既有审计工作记录。逐份连续读取至 EOF，并以 `wc -l`、`sha256sum` 对照 `scan-plan.json`；三者行数、摘要均匹配。
- 阅读现行工程原则及 OOP 现行总账 `agents/oop-refactor-audit/summary.md`、独立入口审查 `summary-review.md`，以及复核总账 `implementation-audit-2026-10-02.md` 和 `reaudit-engine.md`。本记录据此追踪保留项与历史功能缺口；不把早期报告的“当前”或建议当作现行要求。
- 产品规则边界遵循 `docs/principles.md`：本批不提出交易规则变更，也不重新认证交易所/中国结算规则。数量股、A 股 100 股 board lot 及账户/挂单储备边界只按来源标明，不推断更广规则。

## 来源全文章节矩阵

| 来源（行数 / SHA-256） | 全文章节与内容 | EOF 证据 |
|---|---|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/18.md`（14 / `df9eaf5698db28732d8a37eb4b1eecaa491034cbee3b6902541171a25b5b0cdc`） | 1–14：第18组反查概述；`snapshot.rs` 与 `views.rs` 的 owner / 投影职责；两项测试边界建议及其非迁移动作定性。全文单一章节，无子标题。 | `cat` 输出至第14行最后句“本任务未运行测试、构建或 Git 命令。”；实测行数 14。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/19.md`（21 / `393cf13ac8060bfdbe1b0feeecbd32dd152b207cbba03a1a9e19deefa2eac13b`） | 1–21：第19组范围与结论；逐文件复核（`acquisition.rs`、`mod.rs`、`npc_view.rs`、`prehistory.rs`、`public_view.rs`）；迁移顺序与验证边界。全文单一章节，无子标题。 | `cat` 输出至第21行最后句“本轮未运行测试、构建或 Git 操作。”；实测行数 21。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/20.md`（23 / `87c579b1eed49d93e2c8e76611bb51e57e3e5950e3720cdb5595fb852f0b33fb`） | 1–23：第20组范围与结论；9个信息/计划文件逐项表格；迁移与验证边界。全文单一章节，无子标题。 | `cat` 输出至第23行最后句“本次遵照任务不运行测试、构建或 Git。”；实测行数 23。 |

校验记录来自主仓来源实际字节；没有以摘要、关键词命中或旧文档中的“已全文阅读”代替本次 EOF 读取。

## 现行结论与关联

| 来源结论 | 当前 caller / owner / consumer 与总账交叉核对 | G/Q 关联及处置 |
|---|---|---|
| 第18组：快照由权威 `GameSession` 生成只读 owned projection，保存与策略观察各有不同 owner；`MarketView` / `PricePathObservation` 是决策输入。原判保留，不提取新生产对象。 | 现行总账 R10 将 session snapshot、协议候选和公开玩家投影分别记账；ADR-0025 区分公共日终存档、候选与内存 checkpoint。`reaudit-engine.md` 说明重构后的实际 owner/caller 已按现行链审查。源文两项跨簿快照、test-only 聚合建议均是“行为边界测试建议”，不能读成已修复或 OOP 动作。 | 来源未给 G/Q 编号；没有证据将两项建议登记为新缺口。既有测试锚点 `packages/engine/tests/session.rs` 可供未来行为变更时核对，不在本次扩写为当前实现任务。 |
| 第19组：`NpcInformationState` 拥有个人 acquisition；`PublicLibrary` 拥有共享出版物、索引及摘要；观察 context 借用只读状态；prehistory 是确定性构造编排。无新增 OOP finding。 | 最新 engine 总账 G09 明确个人获知年报进入 `BeliefBook` 的范围限制；Q02 是主动读取公开历史的开放边界；Q11 是更正/违约 cause 分发边界。它们属于披露/消费路径，不能由 `PublicLibrary` 已有 owner 或本组 retain 结论核销。`PublicLibrary::restore` 的完整性差异按来源引用既有 D01，属于行为契约，不是对象提取要求。 | 可关联 G09、Q02、Q11 作为近邻边界，但三者都不是本来源认定的 OOP 新候选；它们在 `reaudit-engine.md` 仍按原状态登记。不得由本组历史工作报告把 D01 的旧线索升级或关闭。 |
| 第20组：information 与 plans 下9文件全为 retain、actions 为空。显式保留公开信息边界、目标股数 `A_SHARE_BOARD_LOT`=100股、预算现金/冻结现金及活动订单预留的实际 owner。 | 现行计划资金边界由权威 account cash/frozen cash 和 session 活动订单预留持有；engine 总账 R04/R05/R10 维持策略观察、个人数据和会话计划各自职责。`reaudit-engine.md` G38 记录 `ExistingPlan` / `NewOpportunity` 生产分类未接入，但也明确不能误把 AllocationExperience 重复接线当要求；这是业务分类缺口，不支持新 Allocation 对象。 | G38 可作为 allocation 调用边界关联；本来源不提出实现它的 OOP 动作。大 A 语义未因对象提取发生变化；此处只转述游戏已有 100 股 board-lot 常量，未查官方法源，不能扩张为所有证券类别现行规则认证。 |

## 反证、候选与边界

- **新 OOP 候选：无。** 最新总账 `summary.md` 统计当前全仓 128 个动作组、全部尚未实施；来源19/20的 retain 并不与未实施动作相矛盾，因为各自文件明确不属于聚合动作。不要将“当前总账存在其他领域动作”泛化成这些文件也必须对象化。
- **仅改善已有动作：** 第18组列举的两簿汇总快照用例及 test-only 市场/Retail 聚合用例属于测试覆盖建议；它们未形成新的生产 owner、迁移对象或对当前“已实现”的承诺。其余两组没有额外 OOP 改善动作。
- **最新缺口账：** `reaudit-engine.md` 中 G09 / Q02 / Q11 / G38 保持各自功能语义和状态。它们与来源19/20的接口边界相邻，但来源没有声称这些功能完整，也没有证据说明该次 OOP 抽取新增或修复这些缺口。
- **大 A 与最小范围：** 未提出交易语义调整；预留现金、冻结现金、活动订单预留没有被移入 allocator 缓存。board-lot 仅保留项目契约事实，不对官方依据作新主张。
- **未核实事项：** 本批不运行测试/构建/Git；不重新读取全部生产实现全文或运行时验证建议边界；不重新核验官方交易规则。G/Q 状态依据基线现行复核总账静态记录，不能替代动态验收。来源记录提及但未提供的 JSON（18/19/20.json）未作为证据读取。
