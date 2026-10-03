# 隐藏扫描批次 165

## 读取记录

按 scan-plan 中 owner=5 的三项 source 顺序连续读取至 EOF。实测行数与 SHA-256 均匹配计划；aliases 均为 1。

| Source | 行数 | SHA-256 | 完整性 |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/engine-tests-07.md` | 255 | `28286441eb6424793c9272498981cc78722579b986283d4db2fbfaf0505fefc3` | EOF，完整 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/engine-tests-08.md` | 146 | `529cefd186d2edda4211ed38fc43e1b63d8f04715188cee23ef19bea1e959d3a` | EOF，完整 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/engine-tests-09.md` | 49 | `8ae42b55aa82dd47973eb65da614c2d3569ef63fc47d6f2776eb7d1f4f3416e9` | EOF，完整 |

本次对照当前 caller commit `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 的 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/trading-rules.md`、ADR-0017/0018/0019/0021/0023/0025/0026/0027/0028，以及 implementation-audit 的覆盖索引、engine 复核和 G/Q 总账。源记录是候选设计/静态测试盘点，不是生产实现变更，也没有声称本次运行测试。

## 逐源复核

### engine-tests-07.md

覆盖计划分配、`TradingPlan`/`PlanBook`、policy manifest、TickBatch/CivilUpdate/replay 协议，以及披露、前史、排期、更正与周末发布集成测试。原文逐项标记 `retain` 或 `support`，明确不把测试函数对象化、不改断言、不跑测试；其 18:00 语义是游戏披露相位，不是交易所交易时段。公开政策 fixture 的结构校验不能证明自然语言法源正确，原文也明确保留该限制。

当前 caller 的 engine 集成测试文件仍存在于 `packages/engine/tests/`，包括 `plan_allocation/`、`plans.rs`、`policy_manifest.rs`、`protocol_*.rs` 与 `publications/`；`Cargo.toml` 未关闭 Cargo 的自动 integration-test 发现。测试实际调用 engine API，但没有任何测试文件成为生产 owner 或业务 caller。计划执行和局部并行受理仍以 ADR-0017/0018 及生产 session/plan 调用链为准；测试记录不能倒推出旧的全局先后顺序。

### engine-tests-08.md

覆盖地产会计失败/金样、公司披露 fixture、存档拒绝与恢复连续性，以及规模恢复约束测试。地产利息资本化 90 日阈值被标记为游戏假设，CAS 17 原文依据不可得不等于真实准则合规；存档失败测试对既有 session 的原子性只在明确有断言的场景成立。当前 caller 同一批测试目标与 fixture 路径仍存在；记录中的旧源 SHA 描述来源快照，不能当作 caller 测试源码的当前 SHA 或已执行结果。

### engine-tests-09.md

覆盖 `packages/engine/tests/session.rs` 的大范围集成测试：setup 与 A 股边界、`SaveSlot`/restore 输入拒绝、恢复连续性、市场阶段、订单簿/资金预留/费用、T+1/整手与部分成交、NPC 状态、符号限价，以及标为 `#[ignore]` 的长人口压力案例。原文明确恢复拒绝并不自动证明一个既有运行实例失败前后不变；也明确 ignored 压测不是已执行结果。当前 caller 中 `session.rs` 仍在，测试所验证的 public API 由 engine 的 `GameSession`、`SaveSlot`、订单/账户路径拥有，fixture/helper 只供测试使用。公司会计与披露的日历边界仍应按各自游戏政策解释，不能将披露时间误作 A 股规则。

## G/Q 与决策关联

- 未确认新 G，也未核销现有 G。覆盖索引列明 G01–G68 中 67 项仍开放；engine 复核所列 G06–G09、G16、G28、G35–G38 与 Q02/Q11 仍按各自生产调用缺口或开放边界记录。此批存在相关测试覆盖，不足以证明这些 owner/caller 缺口已接通或已解决。
- Q02（公开历史主动读取经历记录）和 Q11（更正/违约 cause 的生产分发）仍按现行记录保持边界状态；不因 `session.rs`、protocol 或 plan 测试提及相邻概念而推断已解决。
- ADR-0017/0018 继续约束 tick 受理、局部资源冲突、恢复和并行语义；ADR-0019 约束配置范围；ADR-0023 将虚拟开局前史与撮合行情定义为游戏模拟；ADR-0025 规定日终持久化；ADR-0026 是机构个人经历的最新专门决策；ADR-0027/0028 约束宿主构建及发布入口。三篇测试盘点没有新增交易制度或改写这些决定。
- 大 A 语义复核：测试中出现的 100 股申报单位、T+1、交易费用和价格阶段应保持现行 `docs/trading-rules.md`/ADR 定义；披露时刻、地产资本化阈值及虚构公司账套属于游戏模型/测试夹具，不应陈述为交易所或真实会计准则结论。未发现来源材料提出领域改动。

## 结论与限制

本批来源均是测试、fixture 和模块入口的静态对象化盘点，当前 caller 保留相应 engine 集成测试路径及业务 API 所有权边界。未发现需要据此增加的生产候选或 G/Q 变更。没有运行产品测试、构建或 A 股规则联网核验；不将源文档中的测试断言描述为本次通过。
