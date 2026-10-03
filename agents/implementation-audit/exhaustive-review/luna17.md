# luna17：初始持仓与 Money 历史计划复核

## 审查范围与全文读取

基线：产品提交 08e4fc7，目标 HEAD a7c7ce3。本文仅依据目标 worktree 当前源码和文档静态复核；未运行测试、构建或产品。三个目标文档均从首行连续读取至 EOF：

| 文档 | 行数 | 全文结构 |
|---|---:|---|
| docs/superpowers/plans/2026-06-29-initial-positions.md | 525 | Goal/约束/布局 1–32；Task 1 34–134；Task 2 136–283；Task 3 285–430；Task 4 432–511；Self-Review 513–524；文末 handoff 在 525。 |
| docs/superpowers/specs/2026-06-29-initial-positions-design.md | 149 | 背景 10–20；决策 22–39；Random/ByKind 算法 41–56；API 58–96；错误 98–103；边界 105–109；测试矩阵 111–123；文件布局 125–134；验收 136–142；风险 144–149。 |
| docs/superpowers/plans/2026-06-29-money-fixed-point.md | 789 | Goal/约束/布局 1–32；Task 1 34–125；Task 2 127–213；Task 3 215–338；Task 4 340–490；Task 5 492–632；Task 6 634–702；Task 7 704–762；Self-Review 764–783；handoff 785–789。 |

## 初始持仓逐章矩阵

| 原文任务/章节 | 当前实现及测试位置 | 复核结论 |
|---|---|---|
| Plan Task 1：grant_position、float_shares、FloatAllocation（34–134）；Spec §2/§4/§8（22–39、58–96、125–134） | session.rs:890,935 有字段/枚举；account.rs:255–275 使用 Money::mul_shares checked 计算成本并返回错误；lib.rs 导出类型；账户测试含设置仓位覆盖。 | 核心已实现。旧计划代码示例的 checked_mul(...).unwrap_or(i64::MAX) 未照搬，当前实现不会把成本溢出伪装成最大值。 |
| Plan Task 2：Random 分配（136–283）；Spec §3 Random（41–49） | session.rs:1461–1464 先建三类 NPC 再分配；:1652–1683 遍历股票且零流通盘/无 NPC 早退；:1771–1808 使用种子 RNG、Pareto 权重及末账户取余；测试 tests/session.rs:2343–2464 覆盖守恒、玩家零持仓、确定性、成本、零 float。 | 守恒、成本、确定性主干已实现。均匀随机示例已演进为 Pareto 分布，见 Q06；非现行 A 股撮合/交易制度变化。 |
| Plan Task 3：ByKind 与校验（285–430）；Spec §2.2/§3/§5（27–39、50–56、98–103） | session.rs:1090–1114 拒绝负/非有限权重，并对正 float 的现存类别权重和做正且有限校验；:1690–1765 缺类重归一、散户 eligibility、类别 tail 参数；测试 tests/session.rs:2479–2600 覆盖比例、缺类、无有效正权重、非法值及持仓分配。 | ByKind 主路径已有。零 NPC 时对正 float 的 setup 校验仍拒绝，和 spec「float>0 且有 NPC 才自动分配」以及计划「无 NPC 跳过」冲突，保留 G29。总和不要求约等于 1、实际分布算法不同，保留 Q06。 |
| Plan Task 4：市场转活、导出、验证（432–511）；Spec §7/§9/§10（111–149） | 分配调用点 session.rs:1464；集成测试 tests/session.rs:2620–2654 附近；导出已在 lib.rs。 | 功能及测试已存在；本复核未运行命令，不据源码宣称测试当前通过。市场活跃只证明游戏撮合工作，不单独证明具体交易所流动性模型。 |
| Spec §1、§6：设计动机与本批次边界（10–20、105–109） | session.rs:2707–2752 恢复流程先 new() 再清初始持仓，用存档 position 精确覆盖 qty/T+1/invested/recovered；account.rs:140–149 为精确恢复替换全量 balances。 | 旧 grant_position 注释泛称“加载存档精确仓位也走它”，但现行 restore 必须恢复 recovered_cents 与 t1_locked，因此通过 Position::from_restored_parts + restore_balances 是更贴合语义的实现；不是遗漏，也不应强迫复用只设置成本价的 helper。 |

### G29 / Q06 复核

- **G29 仍成立。** SessionSetup::validate 在 session.rs:1098–1112 只要任一股票 float_shares > 0 就校验现存类别的有效权重和，没有零 NPC 例外。GameSession::new_with_company_event_multiplier 于 :1364 先 setup.validate()，之后才在 :1461–1464 建 NPC 并调用 seed_float。所以 seed_float 的 session.rs:1653–1662 空 NPC 早退无法处理 ByKind + 正 float + retail/inst/hot count 全零；权重和为零会先报 InvalidSetup。这是可配置边界，不是默认 NPC 规模问题；Random 同类配置可通过校验并由空 NPC 早退结束，不应将股份分给玩家。旧账本 implementation-audit-2026-10-02.md:81、reaudit-foundations.md:15 与 sweep17.md:39 结论维持。
- **Q06 仍待裁决。** spec 写比例和约等于 1、类内随机（Spec §2.2/§3，行 27–56）；当前校验仅要求有效类别的权重和有限且大于零（session.rs:1098–1112），任意正值归一，且 :1740–1759 对散户作 40% eligibility 抽样并用 Pareto tail。既有记录未找到该政策变化的批准依据；不回退算法，也不将旧规格自动视为当前产品要求。与 G29 的零 NPC 可构造问题相互独立。旧账本 implementation-audit-2026-10-02.md:115、reaudit-foundations.md:18、sweep17.md:41 结论维持。

## Money 逐任务矩阵

| 原文任务/章节 | 当前实现及调用链 | 复核结论 |
|---|---|---|
| Goal、约束、文件布局（Plan 1–32） | money.rs:24–42 为 i64 分 newtype，transparent serde 与 TS number；模块和类型已导出。 | Money 核心为整数分；跨 JS 安全整数范围契约仍是 Q01。 |
| Task 1：MoneyError（34–125） | money.rs:8–22 定义 ParseFailed/Overflow/InvalidRate；tests/money.rs 覆盖显示与错误变体。 | 已实现；旧计划的阶段式占位 struct 只是实现步骤，无运行时契约遗漏。 |
| Task 2：构造、ZERO、trait（127–213） | money.rs:24–58；serde/ts-rs 派生见 :26–42；集成测试构造、Copy、比较及导出。 | 已实现。 |
| Task 3：checked 整数运算（215–338） | money.rs:63–94 add/sub/mul_shares checked；account.rs:264 是 grant_position 生产调用之一。 | 已实现。u32→i64 用 i64::from，无需恢复旧计划的 unwrap_or 示例。 |
| Task 4：元字符串解析（340–490） | money.rs:99–203 trim、符号/小数位/ASCII 数字/范围检查；正常/负数/非法精度测试在 tests/money.rs:95–136。 | 发现无数字的小数点输入仍被接受，详见“无数字 Money 候选”。解析 API 当前未发现 engine 生产 caller，不能表述成现行 Web 输入已走该路径。 |
| Task 5：费率和 half-even（492–632） | money.rs:206–247 非有限、乘积越界检查及 half-even；真实配置调用在 config.rs:212–235；成交结算费用进入 session/pipeline/transition.rs:146–186；存档费用复核见 session/persistence/v2.rs:644–685。 | 主干与 caller 均存在。apply_rate 使用 f64 桥接，与公开最大 i64 范围的 JS number 能力边界相关，现登记 Q01，不据旧用例宣称任意 i64 到 JS 均可无损。旧计划精确 -5 的注释笔误已被代码/test 的 -5 正确结果反证，不按错例改行为。 |
| Task 6：serde（634–702） | money.rs:36–42 transparent i64；Rust 往返和裸整数测试 tests/money.rs:222–242；WASM apps/web-wasm/src/lib.rs:28–40 序列化，并在 :331–335, :353–356, :481–516 解码 setup/快照/存档及序列化结果；Web apps/web/src/host/protocol/wire-values.ts:48–50 以 signedSafeInteger 校验。 | Rust 内部保持 i64；前端 JS 安全整数范围并未与 i64 统一。Q01 维持，严格 parse guard 不应未经契约决策移除。 |
| Task 7：导出、lint、回归（704–762）；Self-Review/handoff（764–789） | lib.rs 导出；Money 测试包含 tests/money.rs:245 的根导出用法。 | 实现覆盖可静态确认；本复核没有运行原文验收命令，不确认 lint/构建/测试结果。 |

### Q01 复核

Q01 维持为契约范围待定：Money 是 transparent i64（money.rs:40–42），JS DTO 声明为 number；wire-values.ts:48–50 对输入只接受 signed safe integer。WASM 将 Money 序列化为 JS DTO 的 caller 包括 lib.rs:28–40；会话创建/快照/存档恢复分别可见 :331–335, :353–356, :481–516。Rust serde 的 i64 往返测试仅证明 Rust serde，不证明超 JS safe integer 可在宿主层无损往返。当前不是要求放宽校验或缩窄 Money；旧账本 implementation-audit-2026-10-02.md:110、reaudit-foundations.md:16 与 sweep17.md:40 结论维持。

## 无数字 Money 候选复核

旧记录称 ., +., -. 及其首尾空白形式可能被解析成 Money(0)。当前控制流仍证实：money.rs:122–132 对 "." 得到空 int_part 与空 frac_part；:143–155 只对非空部分做数字校验；:159–180 将两空部分均解释为 0；:183–203 返回 Ok(Money(0))。"+."/"-." 在符号剥离后走同一流程；trim 使包围空白也不改变结果。测试 tests/money.rs:110–136 仍无这些用例，合法 .5 和 12. 则必须保留。

结论沿用 sweep17.md:43–49 的 **N17-01**，以及后续 sweep18.md:58–64 的 **S18-01**，没有看到修复或核销证据；这是公开 Money 解析 API 的确定接受集缺陷，不是目前 Web 的现金输入缺陷。旧总账 implementation-audit-2026-10-02.md:188 核销的是“已验证小数部分后的 expect 可触发 panic”这一不同命题，不能作为本候选已解决的反证。这里未运行临时样例或测试。

## 新候选与排除项

- 本轮未确认超出 G29、Q01、Q06、N17-01/S18-01 的新代码遗漏。初始持仓真实规则与类型/存档实现范围没有新增证券制度判断，因此未引入未经官方依据支撑的新大 A 规则结论。
- 账户精确存档恢复不复用 grant_position 是有意保留 T+1、净投入与已回收成本等事实，不构成旧计划通用 helper 的遗漏。
- 初始持仓测试及 Money 测试均在源码中，但本轮只做静态审查；不以存在测试代码替代运行结果。
