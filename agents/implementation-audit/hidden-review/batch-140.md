# 批次 140 独立复核

## 基线与来源

- 按 scan-plan batch 140 / owner 5；source root `/data1/baiyifan/workplace/stock_market_game`，output root `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit/agents/implementation-audit/hidden-review`。主仓及 caller worktree 当前 HEAD 均为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。
- 三份指定来源均从头连续读取至 EOF，行数和 SHA-256 与 scan-plan 一致；详细值见同目录 `batch-140.json`。
- 当前 caller/owner/consumer 静态核对 `packages/engine/src/account.rs`、`packages/engine/src/session.rs`、`packages/engine/src/session/pipeline/settlement.rs` 和账户账簿访问点。另核对本审计现行 G/Q 总账、`docs/open-questions.md`、ADR-0027/0028。未运行测试、构建或官方规则查询。

## 结论

- **领域分类与必要性：** `domain.md` 提出的 Account A03 控制写入口候选在基线已部分落实：Account 身份/状态字段私有，`cash`/`positions` 为只读访问器；账务恢复为 crate-private `restore_balances`，生产成交通过结算收据进入 `apply_settlement`，状态以 shadow 后提交。NPC 策略更换从 Session/AccountBook 的受控可变访问进入。故不能把 A03 按旧记录继续报为未实现，也不应因历史 OOP 清单授权新改动；本轮没有动态验证全部调用可达性。
- A02 被归为可选函数组织、非对象迁移与账户边界一致；三份历史材料没有证据要求恢复该动作。`apply_buy`/`apply_sell`/`apply_trade_batch` 仍为公共 API，但扫描到生产结算实际调用是 receipt 到 crate-private `apply_settlement`，旧公共函数的存在不等于生产漏接。
- RNG 说明边界仍准确：随机决策显式收调用方 `&mut dyn Rng`，Session 持有 RNG；复现需要相同输入与 RNG 初始状态。不能称决策函数零副作用，也不能称 RNG 为策略对象自有。
- A 股语义：本批只是历史架构/领域审计材料的回看，没有新增费率、交易时段、T+1 或交易所规则结论，不能代替官方现行规则核验。当前 docs G49 仍登记 Rust 与 Web 成本/浮盈舍入差异；Q16 区分负净成本金额和收益率可用性，避免把旧文档口径误当实现要求。ADR-0027/0028 聚焦宿主构建与发布，不改变交易规则。
- **文档证据差异：** 分类前 `reviews-domain-closure.md` 声称 handoff `managers/domain.md` 的 SHA 为 `f5d46c…`，但本批指定的同路径 `domain.md` 实测为 scan-plan 中的 `a7468439…`。两份均不可互相冒充；闭合报告关于该 handoff 指纹与当前指定文件相符的暗示不成立。其余闭合结论受其自身“文档闭合、非源码行为验收”范围限制。
- **结论状态：** 三份来源可用于理解历史分类，但 closure 中 handoff 指纹存在可核实的不一致；Account A03 旧候选相对当前基线已过时。未发现据此应新增或核销 G/Q 的依据；将指纹差异交主审计总账评估，不改来源文件。

## 限制

只做静态 caller/consumer 搜索，未动态验证；未运行测试/构建；未查官方交易所规则。没有修改产品、历史来源或 Git 内容，仅新增本批记录与 JSON。
