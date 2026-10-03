# Batch 068：全文核验记录

## 基线、约束与来源

- 基线为 `43b1aa5`；源树 `/data1/baiyifan/workplace/stock_market_game`，caller `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 已读取 caller `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、相关决策 ADR-0016/ADR-0026、`docs/trading-rules.md` 及实现审计总账/reaudit-engine。领域依据未涉及交易所规则变更；计划期限属 Q11/ADR-0016 持续交易意图，不推导任何 A 股市场规则。
- 各来源按连续 `cat` 全文读取至 EOF，核对扫描计划行数及 SHA-256，均匹配；无截断补读需求，每项 aliases=1。

## 来源章节矩阵

| 来源（行数；SHA-256） | 全文范围 | 复核意见 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-07.md`（13；`b4cf6c14aa2295fbf246b2ddd9ff7f9083f10e69906b7c405738a30d678a73f3`） | 全文：结论、历史完整复核位置、最终签署范围与限制 | 只说明调查文档准确性被通过、实际 OOP actions 为无；明确未代表方案实现或测试运行。未提供新产品缺陷证据。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-01-delta-final.md`（14；`7b9193b2e4552a4be405678f44b988da6d1e374a2828ec706086ed43b3c1c23b`） | §复核范围、§复核结论及 D01/D02 两项论证 | 记录恢复完整性与期限溢出作为未实施 defect lead 的转移；D02 明确公开构造/独立 serde/PlanBook 恢复绕过及 checked 查询边界。与当前代码相符。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-01-final.md`（56；`65697e402856d4726918ce082ec7bb24e8a82e54f02aad0294735fb971302f06`） | 全文：前轮修订核对、A03 阻断、23 文件逐项结论、大 A/范围/验证 | A03 的期限有效性边界仍被当前公开 API/serde/簿恢复路径反证；准确要求覆盖所有可达入口及 MAX/1、MAX-1/3、0 边界。文中“所有 TradingPlan 字段 pub”与现在字段可见性不符，见下列范围修正；其期限绕过结论仍成立。 |

## 当前 caller / owner / 语义核对

- `packages/engine/src/plans/state.rs:165-180` 表明 `TradingPlan` 字段当前为 `pub(in crate::plans)`，并非 crate 外调用者可直接访问；因此旧终审列出的“所有字段为 pub，外部任意改写”理由已经过时。该 struct 仍派生 `serde::Deserialize`（`:164-167`），故独立反序列化仍可构造非法期限状态。`PlanBook::from_parts`（`packages/engine/src/plans/mod.rs:244-283`）校验 key/id、review baseline、序号及活跃索引，但没有验证期限非零或可表示性。`last_valid_trading_day`（`plans/state.rs:337-339`）仍用 `created + u64::from(horizon) - 1` 裸运算；调用链如 `plans/revision.rs:71-76,302-306,323+` 直接依赖该值。故期限 D02 不是纯历史推测，恢复构造仍有可达缺口；具体是否发生运行崩溃未经测试，本批不声称已复现。
- `packages/engine/src/information/public_view.rs:151-203` 的 `from_parts` 检查 ID 小于 `next_seq`、插入重复 ID、公告时间、报告形状、max ID 与计数器严格衔接并重建索引/digest。`insert_report`（`:247-260`）调用 `ensure_report_shape`；更正目标存在性、匹配性及链结构检查需按该校验函数与对应 publication 校验继续逐项区分。已见独立 delta 曾明确将完整更正链作为 D01 线索，但本批不将“全量校验”注释直接当作保证所有历史更正目标关系的证明，也不因终审旧稿的 D01 举例单独立缺陷。
- `docs/decisions/0016-fundamental-factor-model.md:50` 将计划期限和重新考虑条件作为持续交易意图，`ADR-0026` 规定策略暂停/恢复，不承诺特定 u64 上界；因此此处是现有类型/API 的恢复不变量问题，不改变 A 股真实交易制度。`docs/trading-rules.md` 对真实撮合、T+1 等正式语义没有由本批发现的偏移。

## G/Q 关联、候选与反证

- **G/Q：** 当前 implementation-audit 总账 G01–G68 和 `reaudit-engine.md` 已对照，未找到计划期限恢复/查询缺口的独立既有编号。总账 G16 是历史 `PlanBook` 全量复制目标，与此输入校验/期限算术问题不同，不核销也不复用。Q11 的计划概念已由 ADR-0016 细化；本候选不改变其策略契约。D01 与 PublicLibrary 恢复仍可能有关，但因缺乏本批所需的调用/不变量新证据不报为新候选。
- **确认的候选：** 旧复核遗漏的期限有效性承诺仍有实质依据：Serde 可直建非法 `TradingPlan`，`PlanBook::from_parts` 未调用/等价执行期限校验，查询仍用可能溢出的裸 u64 运算。建议由总审计维护者判断是否纳入既有产品缺陷台账；本批只记录线索，不改 G/Q 状态。边界设计建议保留 horizon=0 显式拒绝；使用 `checked_sub(horizon, 1)` 再 `checked_add(created, offset)`，使 `(MAX,1)` 可表示，而越界显式失败；该行为建议需经产品接受，不冒充现有契约。
- **反证与限界：** 当前字段不是 crate 外公开字段，故不重复沿用旧稿的公开字段绕过理由。没有运行测试或构建；未证明某生产存档已包含坏期限，也未将未来 OOP 项目、未实现建议、无运行证据事项报告为当前运行缺陷。未更改交易语义或源码。
