# 固定基线续核文档 diff 最终独立复核

## 范围与方法

- 复核目标：固定提交 `c0ab4299d104f07589008fee1af886198a2f783b`，worktree `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-audit-final`，分支 `codex/implementation-audit-final`。
- 已读目标树 `AGENTS.md`、`docs/principles.md`，并连续读取完整 `git diff --cached` 七文件：`coverage-index.md`、`implementation-audit-2026-10-02.md` 及 `renewed-check/` 五份 Markdown。差异共 169 行新增、7 行删除。只新增本复核记录；未修改原文、产品源码、正式文档或 Git index。
- 静态反查固定树 `SaveSlot` serde 类型、`decode_save_slot`、`validate_save_slot`、`validate_company_domain`、`GameSession::restore` 及消费处；对照 `EclPolicy::validate` / `BankBooks::new`、`CivilClock::from_parts` / `register_due`。另核对公司会计登记、游戏正式任务记录、K1 日历契约、ADR-0006 与 ADR-0016。
- 当前固定树保留 `packages/engine/src/session/persistence/v2.rs`。本次没有引用主工作区未提交的 `runtime_state` / `schema3` 变化；旧审计记录中的 `saved_runtime.rs` 不是该固定树路径。
- 未运行测试、构建或联网检索；本记录结论是文档、类型和真实生产恢复 consumer 的静态复核，不声称动态端到端复现。

## 三项门禁

### 1. 大 A 语义与依据

- 差异只更新实现审计记录，没有改变撮合、交易时段、T+1、申报单位、费用、结算或存档时点，也未提出新的交易所规则主张；不需要为这些复核结论扩展 A 股法源。
- G80 描述的是游戏银行 `EclPolicy` 的业务校验契约。现有正式公司会计登记区分官方会计准则依据和游戏假设；续核报告把 10000bp 权重约定归于引擎政策，并明确没有重新联网核验准则，未冒称该权重数值是监管规则。
- G78 所涉 2000–2099 是 `CalendarPolicy` 的游戏运行范围，文档也明示它不是交易所公布的日期规则。审查未发现把游戏日历边界升级成 A 股法定日历主张。

### 2. 需求必要性与最小范围

- 修订总账和索引有明确证据变化：Q19 原先以默认新局只装配 Industrial 推断恢复不可达；固定树的完整 `SaveSlot` 可持有 `CompanyOperations`，serde 解码后 `GameSession::restore` 在若干验证后整体安装保存值，而当前领域校验未验证 `BankBooks.ecl_policy`。新局默认装配不能否定这条外部恢复路径。正式存档边界的深度验证要求见 `docs/open-questions.md` Q7 与外部输入校验原则；银行任务记录也要求 ECL 政策构造/重估及恢复校验。由此把 Q19 转为 G80 有据且范围适当。
- G80 的边界保持在完整 SaveSlot 恢复校验，不要求改独立 serde 接受集合、不要求每次贷款发行重验全表，也没有把合法编辑的账户资产判为非法。它与 G36 默认行业闭环、G73 保险子账、G79 工商授信缺口有不同状态 owner，暂独立列项合理。
- G78 扩展已有 CivilClock 恢复 owner：`from_parts` 未按冻结日历政策适用范围检查 pending 日期，而公开 `register_due` 会经 `day_status` 做该检查。报告只陈述未来 pending due 日期边界，并明确不把证据扩大到任意越界 `current_date`；并入旧 G78 比新增编号更克制。
- Q25 的候选没有被误升为缺口。账户 key、`BeliefBook` owner 与确定性策略 profile 的已有校验可与源码对应；创建时同 profile 只证明初值。ADR-0006/0016 没有规定同账户 `BeliefBook.profile` 必须与 `StrategyProfile` 全等，因此保留待明文契约裁定，且未将个体 `AnalysisProfile`、经验或 policy 阈值强制默认化，结论适当。
- 计数一致：G01–G39 中 G27 已核销，余 38；G40–G68 为 29；G69–G79 为 11；新增 G80 为 1，总计 79 个开放 G。Q19 转入既有 G80 不增加第二个 G，G78 只扩范围；Q25 为候选，不计入 G。

### 3. 边界测试、跨层语义与证据真实性

- 银行文档追踪了存档字段、serde 入口、领域校验和恢复赋值，不以 `BankBooks` 单体 serde 行为代替 SaveSlot 结论。`EclPolicy::validate` 与 `BankBooks::new` 的规则可由固定树源码核对；历史 `issue_loan` 不重验政策的保行为注释未被误读为外部 SaveSlot 免校验。
- CivilClock 续核区分自然日时钟和 OperatingScheduler，不把二者 ID 空间混为一谈；合法未耗尽游标、空队列游标以及当前日期边界的限制均明确。建议的未来测试覆盖超界 pending 日期和范围内对照，符合新增边界。
- `BeliefBook` 身份候选保留了“缺少交叉校验”与“存在必须全等契约”之间的区别。没有把影响计算的事实当成自动证明非法，也没有将 A 股合法分析能力差异与账号身份混同。
- 工作区提交基线与报告一致；静态比较确认 `c0ab429` 相对 `43b1aa5` 没有产品源码或正式文档差异。本审查没有把主工作区在途 schema/runtime 命名迁移写成本次固定基线事实，也没有把旧测试记录复述为本轮实测结果。
- 总账新条目、交叉链接、Q19/Q25 转换和 79 项计数均相互吻合；未发现事实误述、必要边界遗漏或需要阻断的范围扩张。

## 结论

**复核通过。** 本次文档差异符合领域基线，修改必要且范围受控，恢复链、日期边界、双 profile 候选及计数都有相应限定和证据。没有 must-fix 项。本轮未运行测试或构建。
