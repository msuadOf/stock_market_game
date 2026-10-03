# 隐藏扫描批次 038（owner 3）

## 范围与读取记录

- 扫描计划：`agents/implementation-audit/hidden-review/scan-plan.json`，batch 38，owner 3；source baseline `43b1aa5`（完整 hash `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`）。来源根 `/data1/baiyifan/workplace/stock_market_game`；当前 caller `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 已读取 caller 的 `AGENTS.md`、`docs/principles.md`。相关决定核对 `ADR-0008`（D2/Rayon 指标仍为待落地）、`ADR-0013`（散户经历范围及旧“无衰减”结论已被计划契约修订）、`ADR-0016`、`ADR-0017`（费用结算消费收据实收增量）、`ADR-0019`（不恢复任意容量配额）、`ADR-0021`、`ADR-0024`、`ADR-0025`、`ADR-0026`。没有以旧 agent instructions 覆盖现行决定。
- 三个 source 均从首行连续全文阅读至 EOF；首次合并输出曾被工具截断，之后对第 01 篇分段补读并读取第 02、03 篇全文。计划行数/实测行数及 SHA-256 全部匹配；每个 source `aliases=1`。

| source | 计划/实测行数 | 计划 SHA-256 / 实测 SHA-256 | 读取状态与章节核销 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/engine-foundation-01.md` | 116 / 116 | `3edd7b79e3179c0b385fe2410e836d1a2fb0a9d523b1db319e5fe21e7a7f58d5` / 一致 | 完整、EOF。总体判断、账户 A02/A03、Account/StoredStrategy、行为决策与纯 helper、日历根/事实/日期/TradingCalendar/政策校验、ComputeBackend、GameConfig、迁移与独立发现各节均已核销。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-foundation-02.md` | 52 / 52 | `48637660bf53efe3257b119fd1bd77459f8ddd370be3cc6f980719a88dd8af52` / 一致 | 完整、EOF。诊断与因果报告、经历/个人记忆/反馈生命周期/历史/关注列表、指标、调用关系与契约、未来迁移/验证边界均已核销。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-foundation-03.md` | 21 / 21 | `b984a76d2e252e41ccb6ee9b6bb506c33c5f3ff5fb2a3a6f53fdd4ac7167a388` / 一致 | 完整、EOF。模块总结、Market、Money、observation、OrderBook/索引、verification evidence、调用契约、候选迁移、验证和依据范围各章均已核销。 |

## 当前代码与现行承诺

- **历史 A03 已实施，不是当前漏项。** 当前 `packages/engine/src/account.rs:95-112` 将 `Account` 身份/状态及 `AccountState` 字段封装；`:114-149` 提供只读读取器及 crate 内受控恢复；`:152-154` 受控恢复策略；`:604-645` 将 `Position` 事实设为私有并提供 getter。`packages/engine/src/session/pipeline/npc_state_projection.rs:200-206` 策略替换仍走受控方法；source 01 中的 `AccountBook::get_mut` 缓存失效约束被历史提案正确保留。与既有 batch 001 对照后，没有 A03 遗漏或错误核销证据。资金以分、持仓数量以股、T+1 锁定和费用收据语义未因重构漂移。
- **G15 仍是既有未完成项。** `packages/engine/src/calendar/holidays.rs:72-91` 在周末后仅于官方覆盖区间内返回官方休市；未覆盖日期仍进入模拟假日分支。与总账 `implementation-audit-2026-10-02.md:66` 和 `reaudit-foundations.md:13` 一致；默认空覆盖不代表默认局触发，不能关闭 G15。模拟 fallback 不被本审查称为官方休市规则。
- **G17 仍是既有未完成项。** `packages/engine/src/indicators.rs:237-249` 暴露单项计算；`:251-269` 的 Rayon batch 计算存在。当前生产调用 `apps/web-wasm/src/lib.rs:435-437`、`apps/server/src/routes.rs:632-646`、`apps/desktop/src-tauri/src/lib.rs:60-64` 仍调用单项 API。故 ADR-0008 D2 的 engine/Rayon 生产接线未完成，总账 `implementation-audit-2026-10-02.md:76` 及 `reaudit-foundations.md:14,23` 的 G17 状态正确；单纯存在 batch API 或指标功能不可核销。
- **其余已批准关联未因这些保留型结构判断改变。** G08/G42（散户经历消费、价格记忆修剪）及 G37（DEV 因果诊断关联真实订单/计划）属于不同 caller/产品契约；本批 source 没有提供它们已接线或应重开证据。G37 的 `CausalCollector`/report 类型不等于生产根诊断真实交易 events。
- `ADR-0017` 现行费用契约仍要求会话结算消费收据实收增量；该约束与 A03 迁移描述一致。`ADR-0019` 拒绝固定挂单配额；source 03 的 OrderBook 保留描述没有提出增量配额。交易制度未重新联网取证或改变。

## 候选、反证与排除

- **没有新增已批准承诺遗漏，也没有旧 G 错误核销。** 三份 source 都是历史结构调查材料，“候选设计，未实施”是历史状态，不能当作当前状态；账户 A03 由当前 caller 代码及既有 batch 001 证实已实施。
- source 01 记载的 `CalendarPolicy` digest 未纳入 `source_digest`、`CivilInstant` 派生反序列化可越过构造校验，以及 `Account` 费用注释漏列过户费，均是源材料提出的独立风险提示。本轮没有找到对应的当前已批准 G/ADR 修复承诺，也没有运行故障证据；不把它们升级为新 G 或宣称已复现。若作为新改动处理，需另按当前决策、TDD 和适用法源流程核实。
- source 02 的经历 writer 失败路径半更新、追加历史长度理论溢出，以及 source 03 的 Market/OrderBook 局部失败非回滚描述，属于实现边界/未来候选；没有据此推断已批准原子性承诺遗漏。不得将 P9 整 tick候选提交约束外推成单笔 `Market::place` 或每个容器更新的事务保证。
- source 01/02/03 的保留结论不重判交易制度；来源中的游戏分钟、图表 `f64` 指标、Money 分值与股票数量单位均按其既定类型解释，不推广为新的 A 股规则。

## 验证边界

仅进行静态全文、hash/行数及当前代码路径核对；未运行测试/构建/回归，未执行 Git 写操作，未修改产品代码。本记录是隐藏扫描补充，不替代完整 diff 独立复核或官方规则核验。
