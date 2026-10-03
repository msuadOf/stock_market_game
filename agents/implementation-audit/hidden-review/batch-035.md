# 隐藏扫描批次 035

## 范围与来源完整性

- 按 `scan-plan.json` 的 owner=5，审查三份指定材料；基线工作树为 `.worktree/implementation-reaudit`，HEAD `43b1aa5`。来源材料位于 source root `/data1/baiyifan/workplace/stock_market_game`。
- 三份材料均从首行连续读至 EOF；SHA-256、行数与 scan-plan 一致：`personal-state-encapsulation-evidence.md` 30 行，`session_strategy.md` 13 行，`trading-plan-encapsulation-evidence.md` 33 行。各文件章节已全部阅读，hash/EOF 证据详见 `batch-035.json`。
- 本批同时按当前实现检验 caller/owner/consumer，并核对当前 G/Q 状态。没有运行测试、构建或 Git 操作；没有改产品代码。

## 对照结果

### PlanPersonalState

- 来源证据 `personal-state-encapsulation-evidence.md:9-12,16-22,28-30` 记载 `pub(super)` 字段、`take/install` 以及进一步封装的成本。基线实现与之相符：`packages/engine/src/session/decision_chain/personal_state.rs:60-114` 将五个成员设为 `pub(super)`，通过 `take/install` 在 `GameSession` 的 attention 与 participant 状态表间转移；缺失/重复情形有显式 panic/assert。
- Caller/owner 对照：真实字段读取集中在 `decision_chain.rs` 及其子模块 `decision_chain/roots.rs`；例如 `decision_chain.rs:3686-3700` 把个人状态交给账户决策流程，`roots.rs:260-470` 更新 belief/watchlist/information。`plan_chain_candidates.rs` 是账户 worker/协调路径的使用者，未发现它在该路径直接读写五字段。子模块也是 `decision_chain` 的 owner 子树成员，故不能把“仅父模块本文件直接访问”说成唯一消费者，但来源材料已明确要求验证该边界（第24-26行）。
- 分类：现行 take/install 所有权交接已实现；字段仍允许 `decision_chain` 子树访问，属于现状而非已批准待办。证据不支持新造 getter/service，也没有发现新的已批准 OOP 候选遗漏。直接移动整份个人状态给账户 root 后进行纯计算，与当前决定链分工相符。

### TradingPlan

- 来源证据 `trading-plan-encapsulation-evidence.md:5-7,11-20,22-33` 是候选方案，并明确未核读外部 caller，不能据它单独断言全仓无兼容影响。
- 基线实现已具备该方案大部分结构：`packages/engine/src/plans/state.rs:167-190` 的字段限定在 `crate::plans` 可见范围，`state.rs:193-264` 提供只读 getter；`packages/engine/src/plans/revision.rs:54-212,214-330` 由 `TradingPlan` receiver 方法执行转移。plans 聚合层通过 getter 调用，例如 `packages/engine/src/plans/mod.rs:252-292,328-385,450-491`；策略/执行路径亦调用 getter，例如 `session/decision_chain.rs:937-973`。没有看到 plans 子模块之外的字段直接访问。
- 这不等于字段达到 Rust 最窄私有可见性：字段 `pub(in crate::plans)`，使 `plans` 父模块仍能读取；但当前父层实际读取使用 getter。把字段再改为只在 `state` 模块私有会阻断 `revision.rs` 的 receiver 转移，需移动这些方法或增加权限层，超出“仅收窄字段”的无代价改动。候选中“receiver 转移 + getter”在基线已经实现，且没有证据要求为了名义私有再增加转发层。
- 来源第30-32行正确提醒公开 serde/Rust API 与恢复验证是独立边界。公开 `Serialize/Deserialize` 和 `deny_unknown_fields` 仍在 `state.rs:164-166`；该封装审查不证明 `Deserialize`/`PlanBook::from_parts` 的业务验证充分，也不应把这一候选当成算术、恢复缺陷的修复或核销。

## G/Q 与语义状态

- 当前 `docs/open-questions.md:94-121` 将 Q11 策略模块方向记为已解决；ADR-0026 定义机构个人成本/风险暂停补充。不能把历史候选材料重新解释为 Q11 尚待选择的技术方向。
- 当前实现缺口总账 `agents/implementation-audit/implementation-audit-2026-10-02.md:49-52,82,117` 仍列 G06-G09、G38、G42-G43 等业务链问题（估值、散户分析/经历消费、年报中报、候选资格、个人记忆边界、预算分类）。这些是各自功能调用链的既有缺口，不是本次封装候选；本批未发现它们被 `PlanPersonalState` 或 `TradingPlan` 封装自动解决。G 编号总览为 G01-G68；与这三份 OOP 证据直接等价的 G 项未发现。
- 三份材料讨论的是代码所有权与 API，不提出交易规则或单位修改。本批未发现 A 股交易语义变化、ADR 冲突或需要重开 Q 项；A 股规则合规仍由正式规则/验证材料覆盖，不从对象封装结论推导。
- 结论：没有可确认的已批准封装承诺遗漏或历史错误核销。历史候选只作为分析证据；现有状态交接、receiver 转移和查询 API 已落地。G06-G09/G38/G42-G43 等仍按正式总账跟踪，不能拿它们作为扩大本批封装范围的理由。
